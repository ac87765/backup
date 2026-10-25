//! 版本列表与恢复旧版本（task 7.5）。
//!
//! 恢复旧版本只新建一个 `file_versions` 行指向原 content，不复制数据、
//! 也不改变 `users.used_bytes`。

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    VersionSummary, bytes_equal, decode_encrypted_meta, format_time, is_unique_violation,
    list_size, load_node, lock_user, validate_node_id, validate_version_id,
};
use crate::AppState;
use crate::api::error::{ApiError, ErrorBody};
use crate::auth::AuthenticatedSession;

/// 版本列表参数。
#[derive(Debug, Deserialize, ToSchema)]
pub struct VersionQuery {
    pub limit: Option<i64>,
}

/// 版本列表响应（按创建时间倒序，含当前版本标记）。
#[derive(Debug, Serialize, ToSchema)]
pub struct VersionsResponse {
    pub items: Vec<VersionSummary>,
}

/// 恢复旧版本请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreVersionRequest {
    /// 被恢复的历史版本 ID。
    pub source_version_id: Uuid,
    /// 客户端生成的新版本 ID（幂等键，必须为 UUIDv4）。
    pub new_version_id: Uuid,
    /// 客户端用同一 content_id 重新加密的版本元数据（hex）。
    pub encrypted_meta: String,
}

/// 读取某个版本的摘要（含 content 信息）。
async fn load_version_summary(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    node_id: Uuid,
    version_id: Uuid,
) -> Result<Option<VersionSummary>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT
            fv.id,
            fv.content_id,
            fv.encrypted_meta,
            c.ciphertext_size,
            c.chunk_count,
            fv.created_at,
            (fv.id = n.current_version_id) AS "is_current!"
        FROM file_versions fv
        JOIN contents c ON c.id = fv.content_id AND c.user_id = fv.user_id
        JOIN nodes n ON n.id = fv.node_id AND n.user_id = fv.user_id
        WHERE fv.id = $1
          AND fv.node_id = $2
          AND fv.user_id = $3
        "#,
        version_id,
        node_id,
        user_id,
    )
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row.map(|row| VersionSummary {
        id: row.id,
        content_id: row.content_id,
        encrypted_meta: crate::auth::encode_hex(&row.encrypted_meta),
        ciphertext_size: row.ciphertext_size,
        chunk_count: row.chunk_count,
        is_current: row.is_current,
        created_at: format_time(row.created_at),
    }))
}

/// `GET /api/v1/nodes/{node_id}/versions`。
#[utoipa::path(
    get,
    path = "/api/v1/nodes/{node_id}/versions",
    tag = "files",
    params(
        ("node_id" = Uuid, Path, description = "文件节点 ID"),
        ("limit" = Option<i64>, Query, description = "最多返回条数，默认 200")
    ),
    responses(
        (status = 200, body = VersionsResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn list_versions(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(node_id): Path<Uuid>,
    Query(query): Query<VersionQuery>,
) -> Result<Json<VersionsResponse>, ApiError> {
    let node_id = validate_node_id(node_id)?;
    let limit = list_size(query.limit)?;
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    // 只用于确认节点属于当前用户；不存在与跨用户都返回 404。
    if load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
        .is_none()
    {
        return Err(ApiError::not_found());
    }

    let rows = sqlx::query!(
        r#"
        SELECT
            fv.id,
            fv.content_id,
            fv.encrypted_meta,
            c.ciphertext_size,
            c.chunk_count,
            fv.created_at,
            (fv.id = n.current_version_id) AS "is_current!"
        FROM file_versions fv
        JOIN contents c ON c.id = fv.content_id AND c.user_id = fv.user_id
        JOIN nodes n ON n.id = fv.node_id AND n.user_id = fv.user_id
        WHERE fv.user_id = $1 AND fv.node_id = $2
        ORDER BY fv.created_at DESC, fv.id DESC
        LIMIT $3
        "#,
        session.user_id,
        node_id,
        limit,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let items = rows
        .into_iter()
        .map(|row| VersionSummary {
            id: row.id,
            content_id: row.content_id,
            encrypted_meta: crate::auth::encode_hex(&row.encrypted_meta),
            ciphertext_size: row.ciphertext_size,
            chunk_count: row.chunk_count,
            is_current: row.is_current,
            created_at: format_time(row.created_at),
        })
        .collect();
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(VersionsResponse { items }))
}

/// `POST /api/v1/nodes/{node_id}/versions/restore`。
///
/// 恢复会新建一个指向原 content 的版本并把它设为当前版本；旧版本仍保留。
#[utoipa::path(
    post,
    path = "/api/v1/nodes/{node_id}/versions/restore",
    tag = "files",
    params(("node_id" = Uuid, Path, description = "文件节点 ID")),
    request_body = RestoreVersionRequest,
    responses(
        (status = 200, body = VersionSummary),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn restore_version(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(node_id): Path<Uuid>,
    payload: Result<Json<RestoreVersionRequest>, JsonRejection>,
) -> Result<Json<VersionSummary>, ApiError> {
    let request = payload
        .map(|Json(value)| value)
        .map_err(|_| ApiError::invalid_request())?;
    let node_id = validate_node_id(node_id)?;
    let source_version_id = validate_version_id(request.source_version_id)?;
    let new_version_id = validate_version_id(request.new_version_id)?;
    let encrypted_meta = decode_encrypted_meta(&request.encrypted_meta)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    lock_user(&mut tx, session.user_id).await?;

    let node = load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if node.is_deleted() {
        return Err(ApiError::target_unavailable());
    }

    let source = sqlx::query!(
        r#"
        SELECT content_id, encrypted_meta
        FROM file_versions
        WHERE id = $1 AND node_id = $2 AND user_id = $3
        "#,
        source_version_id,
        node_id,
        session.user_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    let source = source.ok_or_else(ApiError::not_found)?;

    // 幂等：相同 new_version_id + 相同内容返回已有版本；不同内容返回 conflict。
    let existing = sqlx::query!(
        r#"
        SELECT content_id, encrypted_meta
        FROM file_versions
        WHERE id = $1 AND node_id = $2 AND user_id = $3
        "#,
        new_version_id,
        node_id,
        session.user_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if let Some(existing) = existing {
        let same = existing.content_id == source.content_id
            && bytes_equal(&existing.encrypted_meta, &encrypted_meta);
        if !same {
            return Err(ApiError::conflict());
        }
        let summary = load_version_summary(&mut tx, session.user_id, node_id, new_version_id)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::internal("幂等版本读取失败"))?;
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(summary));
    }

    let inserted = sqlx::query!(
        r#"
        INSERT INTO file_versions (id, user_id, node_id, content_id, encrypted_meta)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        new_version_id,
        session.user_id,
        node_id,
        source.content_id,
        encrypted_meta,
    )
    .execute(&mut *tx)
    .await;

    match inserted {
        Ok(_) => {}
        Err(error) if is_unique_violation(&error, "file_versions_pkey") => {
            return Err(ApiError::conflict());
        }
        Err(error) => return Err(ApiError::internal(error)),
    }

    sqlx::query!(
        r#"
        UPDATE nodes
        SET current_version_id = $3,
            row_version = row_version + 1,
            updated_at = now()
        WHERE id = $1 AND user_id = $2
        "#,
        node_id,
        session.user_id,
        new_version_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let summary = load_version_summary(&mut tx, session.user_id, node_id, new_version_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("恢复后的版本读取失败"))?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(summary))
}
