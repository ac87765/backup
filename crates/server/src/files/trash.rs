//! 回收站：软删除、列表、恢复与永久删除（task 7.4）。

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    NodeRow, NodeSummary, decode_name_token, is_name_conflict, list_size, load_node, lock_user,
    validate_node_id,
};
use crate::AppState;
use crate::api::error::{ApiError, ErrorBody};
use crate::auth::AuthenticatedSession;
use crate::jobs::deletion::enqueue_object_deletions;

/// 回收站列表参数。
#[derive(Debug, Deserialize, ToSchema)]
pub struct TrashQuery {
    pub limit: Option<i64>,
}

/// 回收站列表响应（只含每个 batch 的顶层项）。
#[derive(Debug, Serialize, ToSchema)]
pub struct TrashResponse {
    pub items: Vec<NodeSummary>,
}

/// 软删除响应。重复删除同一节点返回同一个 `trash_batch_id`。
#[derive(Debug, Serialize, ToSchema)]
pub struct TrashEntryResponse {
    pub node_id: Uuid,
    pub trash_batch_id: Uuid,
}

/// 恢复请求。
///
/// 不指定 `new_parent_id` 时恢复到原父节点并沿用原名称令牌；指定时必须同时
/// 提交客户端针对新父节点重新计算的 `name_token`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreNodeRequest {
    pub new_parent_id: Option<Uuid>,
    pub name_token: Option<String>,
}

/// 永久删除统计。
#[derive(Debug, Clone, Copy, Default, Serialize, ToSchema)]
pub struct PermanentDeleteResponse {
    pub deleted_nodes: u64,
    pub deleted_versions: u64,
    pub deleted_contents: u64,
    pub freed_bytes: i64,
    pub queued_objects: u64,
}

fn parse_body<T: serde::de::DeserializeOwned>(
    payload: Result<Json<T>, JsonRejection>,
) -> Result<T, ApiError> {
    payload
        .map(|Json(value)| value)
        .map_err(|_| ApiError::invalid_request())
}

/// 重建一个回收站 batch 的全部节点、版本、content 与 chunks。
///
/// 调用方必须已锁定用户行（或持有扫描任务的 advisory lock）。删除顺序严格为
/// 节点 → 版本 → 不再被任何版本引用的 content → chunks，对象删除只写 outbox，
/// 并在同一事务内扣减 `users.used_bytes`。
pub async fn permanently_delete_batch(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    trash_batch_id: Uuid,
) -> Result<PermanentDeleteResponse, sqlx::Error> {
    let node_ids: Vec<Uuid> = sqlx::query_scalar!(
        r#"
        SELECT id
        FROM nodes
        WHERE user_id = $1 AND trash_batch_id = $2
        FOR UPDATE
        "#,
        user_id,
        trash_batch_id,
    )
    .fetch_all(&mut **tx)
    .await?;

    if node_ids.is_empty() {
        return Ok(PermanentDeleteResponse::default());
    }

    let removed_content_ids: Vec<Uuid> = sqlx::query_scalar!(
        r#"
        DELETE FROM file_versions
        WHERE user_id = $1 AND node_id = ANY($2)
        RETURNING content_id
        "#,
        user_id,
        &node_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    let candidates = sqlx::query!(
        r#"
        SELECT id, ciphertext_size
        FROM contents
        WHERE user_id = $1
          AND (node_id = ANY($2) OR id = ANY($3))
        "#,
        user_id,
        &node_ids,
        &removed_content_ids,
    )
    .fetch_all(&mut **tx)
    .await?;

    let mut freed_bytes: i64 = 0;
    let mut deleted_contents: u64 = 0;
    let mut object_keys: Vec<String> = Vec::new();

    for content in candidates {
        let still_referenced = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM file_versions
                WHERE user_id = $1 AND content_id = $2
            ) AS "exists!"
            "#,
            user_id,
            content.id,
        )
        .fetch_one(&mut **tx)
        .await?;
        if still_referenced {
            continue;
        }

        let keys: Vec<String> = sqlx::query_scalar!(
            r#"
            DELETE FROM chunks
            WHERE user_id = $1 AND content_id = $2
            RETURNING object_key
            "#,
            user_id,
            content.id,
        )
        .fetch_all(&mut **tx)
        .await?;
        object_keys.extend(keys);

        let deleted = sqlx::query!(
            "DELETE FROM contents WHERE user_id = $1 AND id = $2",
            user_id,
            content.id,
        )
        .execute(&mut **tx)
        .await?;
        if deleted.rows_affected() == 1 {
            deleted_contents += 1;
            freed_bytes = freed_bytes.saturating_add(content.ciphertext_size);
        }
    }

    sqlx::query!(
        "DELETE FROM nodes WHERE user_id = $1 AND trash_batch_id = $2",
        user_id,
        trash_batch_id,
    )
    .execute(&mut **tx)
    .await?;

    sqlx::query!(
        r#"
        UPDATE users
        SET used_bytes = GREATEST(used_bytes - $2, 0),
            updated_at = now()
        WHERE id = $1
        "#,
        user_id,
        freed_bytes,
    )
    .execute(&mut **tx)
    .await?;

    let queued_objects = enqueue_object_deletions(tx, &object_keys).await?;

    Ok(PermanentDeleteResponse {
        deleted_nodes: node_ids.len() as u64,
        deleted_versions: removed_content_ids.len() as u64,
        deleted_contents,
        freed_bytes,
        queued_objects,
    })
}

/// 请求中的节点是否是一个回收站 batch 的顶层项。
async fn is_trash_batch_top(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    node: &NodeRow,
    batch_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let Some(parent_id) = node.parent_id else {
        // 原父节点被永久删除后 parent_id 会被 FK 置空；这仍然是 batch 顶层项。
        return Ok(node.is_deleted());
    };
    let nested = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM nodes p
            WHERE p.user_id = $1
              AND p.id = $2
              AND p.trash_batch_id = $3
              AND p.deleted_at IS NOT NULL
        ) AS "exists!"
        "#,
        user_id,
        parent_id,
        batch_id,
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(!nested)
}

/// `DELETE /api/v1/nodes/{node_id}`：把节点及其子树移入回收站。
#[utoipa::path(
    delete,
    path = "/api/v1/nodes/{node_id}",
    tag = "files",
    params(("node_id" = Uuid, Path, description = "节点 ID")),
    responses(
        (status = 200, body = TrashEntryResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn delete_node(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(node_id): Path<Uuid>,
) -> Result<Json<TrashEntryResponse>, ApiError> {
    let node_id = validate_node_id(node_id)?;
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
    if node.is_root() {
        return Err(ApiError::operation_not_allowed());
    }
    if let Some(batch_id) = node.trash_batch_id {
        // 幂等：重复删除返回同一个 batch，不重复打时间戳。
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(TrashEntryResponse {
            node_id,
            trash_batch_id: batch_id,
        }));
    }

    let batch_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        WITH RECURSIVE subtree AS (
            SELECT id
            FROM nodes
            WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL
            UNION ALL
            SELECT child.id
            FROM nodes child
            JOIN subtree parent ON child.parent_id = parent.id
            WHERE child.user_id = $2 AND child.deleted_at IS NULL
        )
        UPDATE nodes target
        SET deleted_at = now(),
            trash_batch_id = $3,
            trashed_from_parent_id = target.parent_id,
            row_version = target.row_version + 1,
            updated_at = now()
        FROM subtree
        WHERE target.id = subtree.id AND target.user_id = $2
        "#,
        node_id,
        session.user_id,
        batch_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(TrashEntryResponse {
        node_id,
        trash_batch_id: batch_id,
    }))
}

/// `GET /api/v1/trash`：只列每个 batch 的顶层项。
#[utoipa::path(
    get,
    path = "/api/v1/trash",
    tag = "files",
    params(("limit" = Option<i64>, Query, description = "最多返回条数，默认 200")),
    responses(
        (status = 200, body = TrashResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn list_trash(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Query(query): Query<TrashQuery>,
) -> Result<Json<TrashResponse>, ApiError> {
    let limit = list_size(query.limit)?;
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    let rows = sqlx::query_as!(
        NodeRow,
        r#"
        SELECT
            n.id,
            n.kind,
            n.parent_id,
            n.wrapped_key,
            n.encrypted_meta,
            n.name_token,
            n.row_version,
            n.created_at,
            n.updated_at,
            n.deleted_at,
            n.trash_batch_id,
            n.trashed_from_parent_id,
            fv.id AS "current_version_id?",
            fv.content_id AS "content_id?",
            fv.encrypted_meta AS "version_meta?",
            c.ciphertext_size AS "ciphertext_size?",
            c.chunk_count AS "chunk_count?",
            fv.created_at AS "version_created_at?"
        FROM nodes n
        LEFT JOIN file_versions fv
            ON fv.id = n.current_version_id AND fv.user_id = n.user_id
        LEFT JOIN contents c
            ON c.id = fv.content_id AND c.user_id = n.user_id
        WHERE n.user_id = $1
          AND n.deleted_at IS NOT NULL
          AND NOT EXISTS (
              SELECT 1 FROM nodes p
              WHERE p.user_id = $1
                AND p.id = n.parent_id
                AND p.trash_batch_id = n.trash_batch_id
                AND p.deleted_at IS NOT NULL
          )
        ORDER BY n.deleted_at DESC, n.id
        LIMIT $2
        "#,
        session.user_id,
        limit,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let items = rows.iter().map(NodeRow::summary).collect();
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(TrashResponse { items }))
}

/// `POST /api/v1/trash/{node_id}/restore`。
#[utoipa::path(
    post,
    path = "/api/v1/trash/{node_id}/restore",
    tag = "files",
    params(("node_id" = Uuid, Path, description = "回收站顶层节点 ID")),
    request_body = RestoreNodeRequest,
    responses(
        (status = 200, body = NodeSummary),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn restore_node(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(node_id): Path<Uuid>,
    payload: Result<Json<RestoreNodeRequest>, JsonRejection>,
) -> Result<Json<NodeSummary>, ApiError> {
    let request = parse_body(payload)?;
    let node_id = validate_node_id(node_id)?;
    if request.new_parent_id.is_none() && request.name_token.is_some() {
        return Err(ApiError::invalid_request());
    }
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
    if !node.is_deleted() {
        // 幂等：已经恢复的节点直接返回当前状态。
        let summary = node.summary();
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(summary));
    }
    let batch_id = node
        .trash_batch_id
        .ok_or_else(|| ApiError::internal("回收站节点缺少 trash_batch_id"))?;
    if !is_trash_batch_top(&mut tx, session.user_id, &node, batch_id)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::operation_not_allowed());
    }

    let (target_parent_id, name_token) = match request.new_parent_id {
        Some(new_parent_id) => {
            let new_parent_id = validate_node_id(new_parent_id)?;
            let token = request
                .name_token
                .as_deref()
                .ok_or_else(ApiError::invalid_request)
                .and_then(decode_name_token)?;
            let target = load_node(&mut tx, session.user_id, new_parent_id)
                .await
                .map_err(ApiError::internal)?
                .ok_or_else(ApiError::not_found)?;
            if target.is_deleted() || target.kind != "folder" {
                return Err(ApiError::target_unavailable());
            }
            (new_parent_id, token)
        }
        None => {
            let original_parent_id = node
                .trashed_from_parent_id
                .or(node.parent_id)
                .ok_or_else(ApiError::target_unavailable)?;
            let target = load_node(&mut tx, session.user_id, original_parent_id)
                .await
                .map_err(ApiError::internal)?;
            match target {
                Some(target) if !target.is_deleted() && target.kind == "folder" => {}
                _ => return Err(ApiError::target_unavailable()),
            }
            let token = node
                .name_token
                .clone()
                .ok_or_else(|| ApiError::internal("回收站节点缺少名称令牌"))?;
            (original_parent_id, token)
        }
    };

    let restored = sqlx::query!(
        r#"
        UPDATE nodes
        SET parent_id = $3,
            name_token = $4,
            deleted_at = NULL,
            trash_batch_id = NULL,
            trashed_from_parent_id = NULL,
            row_version = row_version + 1,
            updated_at = now()
        WHERE id = $1
          AND user_id = $2
          AND deleted_at IS NOT NULL
          AND trash_batch_id = $5
        "#,
        node_id,
        session.user_id,
        target_parent_id,
        name_token,
        batch_id,
    )
    .execute(&mut *tx)
    .await;

    match restored {
        Ok(result) if result.rows_affected() == 1 => {}
        Ok(_) => return Err(ApiError::conflict()),
        Err(error) if is_name_conflict(&error) => return Err(ApiError::name_conflict()),
        Err(error) => return Err(ApiError::internal(error)),
    }

    let restored_children = sqlx::query!(
        r#"
        UPDATE nodes
        SET deleted_at = NULL,
            trash_batch_id = NULL,
            trashed_from_parent_id = NULL,
            row_version = row_version + 1,
            updated_at = now()
        WHERE user_id = $1
          AND trash_batch_id = $2
          AND id <> $3
          AND deleted_at IS NOT NULL
        "#,
        session.user_id,
        batch_id,
        node_id,
    )
    .execute(&mut *tx)
    .await;

    match restored_children {
        Ok(_) => {}
        Err(error) if is_name_conflict(&error) => return Err(ApiError::name_conflict()),
        Err(error) => return Err(ApiError::internal(error)),
    }

    let restored_node = load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let summary = restored_node.summary();
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(summary))
}

/// `DELETE /api/v1/trash/{node_id}`：永久删除一个回收站项目。
#[utoipa::path(
    delete,
    path = "/api/v1/trash/{node_id}",
    tag = "files",
    params(("node_id" = Uuid, Path, description = "回收站顶层节点 ID")),
    responses(
        (status = 200, body = PermanentDeleteResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn permanently_delete_node(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(node_id): Path<Uuid>,
) -> Result<Json<PermanentDeleteResponse>, ApiError> {
    let node_id = validate_node_id(node_id)?;
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
    if !node.is_deleted() {
        return Err(ApiError::operation_not_allowed());
    }
    let batch_id = node
        .trash_batch_id
        .ok_or_else(|| ApiError::internal("回收站节点缺少 trash_batch_id"))?;
    if !is_trash_batch_top(&mut tx, session.user_id, &node, batch_id)
        .await
        .map_err(ApiError::internal)?
    {
        return Err(ApiError::operation_not_allowed());
    }

    let summary = permanently_delete_batch(&mut tx, session.user_id, batch_id)
        .await
        .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(summary))
}
