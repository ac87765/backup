//! 列目录、创建文件夹、重命名与移动（tasks 7.2、7.3）。

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    NodeRow, NodeSummary, bytes_equal, decode_cursor, decode_encrypted_meta, decode_name_token,
    decode_wrapped_key, is_name_conflict, load_node, lock_user, page_size, validate_node_id,
};
use crate::AppState;
use crate::api::error::{ApiError, ErrorBody};
use crate::auth::{AuthenticatedSession, WritableSession};

/// 列目录查询参数。
#[derive(Debug, Deserialize, ToSchema)]
pub struct ListChildrenQuery {
    /// 上一页返回的 `next_cursor`；只编码排序键（节点 ID）。
    pub cursor: Option<String>,
    /// 页大小（1–1000，默认 200）。
    pub limit: Option<i64>,
}

/// 列目录响应。客户端解密名称后在本地排序。
#[derive(Debug, Serialize, ToSchema)]
pub struct ChildrenResponse {
    pub items: Vec<NodeSummary>,
    /// 为 `null` 表示没有更多页。
    pub next_cursor: Option<Uuid>,
}

/// 创建文件夹请求。`id` 是客户端生成的幂等键。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateFolderRequest {
    pub id: Uuid,
    pub parent_id: Uuid,
    /// 包裹的节点密钥（hex）。
    pub wrapped_key: String,
    /// 加密元数据（hex）。
    pub encrypted_meta: String,
    /// 名称令牌（hex）。
    pub name_token: String,
}

/// 重命名请求。`row_version` 是乐观锁版本号。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameNodeRequest {
    pub row_version: i32,
    pub encrypted_meta: String,
    pub name_token: String,
}

/// 移动请求。名称令牌必须由客户端针对新父节点重新计算。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveNodeRequest {
    pub new_parent_id: Uuid,
    pub row_version: i32,
    pub name_token: String,
}

fn parse_body<T: serde::de::DeserializeOwned>(
    payload: Result<Json<T>, JsonRejection>,
) -> Result<T, ApiError> {
    payload
        .map(|Json(value)| value)
        .map_err(|_| ApiError::invalid_request())
}

fn require_folder(node: &NodeRow) -> Result<(), ApiError> {
    if node.is_deleted() {
        return Err(ApiError::target_unavailable());
    }
    if node.kind != "folder" {
        return Err(ApiError::operation_not_allowed());
    }
    Ok(())
}

/// `GET /api/v1/nodes/{node_id}/children`。
#[utoipa::path(
    get,
    path = "/api/v1/nodes/{node_id}/children",
    tag = "files",
    params(
        ("node_id" = Uuid, Path, description = "文件夹节点 ID"),
        ("cursor" = Option<String>, Query, description = "上一页的 next_cursor"),
        ("limit" = Option<i64>, Query, description = "页大小 1–1000，默认 200")
    ),
    responses(
        (status = 200, body = ChildrenResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn list_children(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(node_id): Path<Uuid>,
    Query(query): Query<ListChildrenQuery>,
) -> Result<Json<ChildrenResponse>, ApiError> {
    let node_id = validate_node_id(node_id)?;
    let page = page_size(query.limit)?;
    let cursor = decode_cursor(query.cursor.as_deref())?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    let folder = load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    require_folder(&folder)?;

    let mut rows = sqlx::query_as!(
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
          AND n.parent_id = $2
          AND n.deleted_at IS NULL
          AND ($3::uuid IS NULL OR n.id > $3)
        ORDER BY n.id
        LIMIT $4
        "#,
        session.user_id,
        node_id,
        cursor,
        page + 1,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let next_cursor = if rows.len() as i64 > page {
        rows.truncate(page as usize);
        rows.last().map(|row| row.id)
    } else {
        None
    };
    let items = rows.iter().map(NodeRow::summary).collect();
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(ChildrenResponse { items, next_cursor }))
}

/// `POST /api/v1/folders`。
///
/// 同一客户端 ID + 同一内容重复提交返回同一结果；同一 ID 不同内容返回
/// `conflict`。同名冲突由部分唯一索引兜底。
#[utoipa::path(
    post,
    path = "/api/v1/folders",
    tag = "files",
    request_body = CreateFolderRequest,
    responses(
        (status = 200, body = NodeSummary),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn create_folder(
    State(state): State<AppState>,
    session: WritableSession,
    payload: Result<Json<CreateFolderRequest>, JsonRejection>,
) -> Result<Json<NodeSummary>, ApiError> {
    state.auth.check_user_write(session.user_id)?;
    let request = parse_body(payload)?;
    let node_id = validate_node_id(request.id)?;
    let parent_id = validate_node_id(request.parent_id)?;
    let wrapped_key = decode_wrapped_key(&request.wrapped_key)?;
    let encrypted_meta = decode_encrypted_meta(&request.encrypted_meta)?;
    let name_token = decode_name_token(&request.name_token)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    // 与移动/删除/恢复串行化，避免在父节点刚被移入回收站后仍插入子节点。
    lock_user(&mut tx, session.user_id).await?;

    let parent = load_node(&mut tx, session.user_id, parent_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    require_folder(&parent)?;

    // 幂等：同一 ID + 同一内容直接返回已有节点；内容不同则冲突。
    if let Some(existing) = load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
    {
        let same = !existing.is_deleted()
            && existing.kind == "folder"
            && existing.parent_id == Some(parent_id)
            && existing
                .wrapped_key
                .as_deref()
                .is_some_and(|stored| bytes_equal(stored, &wrapped_key))
            && existing
                .encrypted_meta
                .as_deref()
                .is_some_and(|stored| bytes_equal(stored, &encrypted_meta))
            && existing
                .name_token
                .as_deref()
                .is_some_and(|stored| bytes_equal(stored, &name_token));
        if !same {
            return Err(ApiError::conflict());
        }
        let summary = existing.summary();
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(summary));
    }

    let inserted = sqlx::query_scalar!(
        r#"
        INSERT INTO nodes (
            id, user_id, parent_id, kind, wrapped_key, encrypted_meta, name_token
        )
        VALUES ($1, $2, $3, 'folder', $4, $5, $6)
        ON CONFLICT (id) DO NOTHING
        RETURNING id
        "#,
        node_id,
        session.user_id,
        parent_id,
        wrapped_key,
        encrypted_meta,
        name_token,
    )
    .fetch_optional(&mut *tx)
    .await;

    let inserted = match inserted {
        Ok(row) => row,
        Err(error) if is_name_conflict(&error) => {
            return Err(ApiError::name_conflict());
        }
        Err(error) => return Err(ApiError::internal(error)),
    };

    // `ON CONFLICT (id)` 命中：可能是并发重复提交（同一内容）或 ID 被占用。
    if inserted.is_none() {
        let Some(existing) = load_node(&mut tx, session.user_id, node_id)
            .await
            .map_err(ApiError::internal)?
        else {
            return Err(ApiError::conflict());
        };
        let same = !existing.is_deleted()
            && existing.kind == "folder"
            && existing.parent_id == Some(parent_id)
            && existing
                .wrapped_key
                .as_deref()
                .is_some_and(|stored| bytes_equal(stored, &wrapped_key))
            && existing
                .encrypted_meta
                .as_deref()
                .is_some_and(|stored| bytes_equal(stored, &encrypted_meta))
            && existing
                .name_token
                .as_deref()
                .is_some_and(|stored| bytes_equal(stored, &name_token));
        if !same {
            return Err(ApiError::conflict());
        }
        let summary = existing.summary();
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(summary));
    }

    let created = load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("新创建的文件夹读取失败"))?;
    let summary = created.summary();
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(summary))
}

/// `PATCH /api/v1/nodes/{node_id}/rename`。
#[utoipa::path(
    patch,
    path = "/api/v1/nodes/{node_id}/rename",
    tag = "files",
    params(("node_id" = Uuid, Path, description = "节点 ID")),
    request_body = RenameNodeRequest,
    responses(
        (status = 200, body = NodeSummary),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn rename_node(
    State(state): State<AppState>,
    session: WritableSession,
    Path(node_id): Path<Uuid>,
    payload: Result<Json<RenameNodeRequest>, JsonRejection>,
) -> Result<Json<NodeSummary>, ApiError> {
    state.auth.check_user_write(session.user_id)?;
    let request = parse_body(payload)?;
    let node_id = validate_node_id(node_id)?;
    let encrypted_meta = decode_encrypted_meta(&request.encrypted_meta)?;
    let name_token = decode_name_token(&request.name_token)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    let node = load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if node.is_deleted() {
        return Err(ApiError::target_unavailable());
    }
    if node.is_root() {
        return Err(ApiError::operation_not_allowed());
    }
    if node.row_version != request.row_version {
        return Err(ApiError::conflict());
    }

    let updated = sqlx::query!(
        r#"
        UPDATE nodes
        SET encrypted_meta = $3,
            name_token = $4,
            row_version = row_version + 1,
            updated_at = now()
        WHERE id = $1
          AND user_id = $2
          AND deleted_at IS NULL
          AND parent_id IS NOT NULL
          AND row_version = $5
        "#,
        node_id,
        session.user_id,
        encrypted_meta,
        name_token,
        request.row_version,
    )
    .execute(&mut *tx)
    .await;

    match updated {
        Ok(result) if result.rows_affected() == 1 => {}
        Ok(_) => return Err(ApiError::conflict()),
        Err(error) if is_name_conflict(&error) => return Err(ApiError::name_conflict()),
        Err(error) => return Err(ApiError::internal(error)),
    }

    let updated_node = load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let summary = updated_node.summary();
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(summary))
}

/// `PATCH /api/v1/nodes/{node_id}/move`。
///
/// 事务内先锁 `users` 行，使同一用户的移动串行化；再用递归 CTE 检查目标
/// 不是自身或后代。这样并发 A→B 与 B→A 最多只有一个成功，不会形成环。
#[utoipa::path(
    patch,
    path = "/api/v1/nodes/{node_id}/move",
    tag = "files",
    params(("node_id" = Uuid, Path, description = "节点 ID")),
    request_body = MoveNodeRequest,
    responses(
        (status = 200, body = NodeSummary),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn move_node(
    State(state): State<AppState>,
    session: WritableSession,
    Path(node_id): Path<Uuid>,
    payload: Result<Json<MoveNodeRequest>, JsonRejection>,
) -> Result<Json<NodeSummary>, ApiError> {
    state.auth.check_user_write(session.user_id)?;
    let request = parse_body(payload)?;
    let node_id = validate_node_id(node_id)?;
    let new_parent_id = validate_node_id(request.new_parent_id)?;
    let name_token = decode_name_token(&request.name_token)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    // 同一用户的结构变更串行化（task 7.3 的并发安全要求）。
    lock_user(&mut tx, session.user_id).await?;

    let node = load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    if node.is_deleted() {
        return Err(ApiError::target_unavailable());
    }
    if node.is_root() {
        return Err(ApiError::operation_not_allowed());
    }

    let target = load_node(&mut tx, session.user_id, new_parent_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    require_folder(&target)?;

    if target.id == node.id {
        return Err(ApiError::invalid_move());
    }

    let is_descendant = sqlx::query_scalar!(
        r#"
        WITH RECURSIVE subtree AS (
            SELECT id
            FROM nodes
            WHERE id = $1 AND user_id = $2
            UNION ALL
            SELECT child.id
            FROM nodes child
            JOIN subtree parent ON child.parent_id = parent.id
            WHERE child.user_id = $2
        )
        SELECT EXISTS (SELECT 1 FROM subtree WHERE id = $3) AS "exists!"
        "#,
        node_id,
        session.user_id,
        new_parent_id,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if is_descendant {
        return Err(ApiError::invalid_move());
    }

    let updated = sqlx::query!(
        r#"
        UPDATE nodes
        SET parent_id = $3,
            name_token = $4,
            row_version = row_version + 1,
            updated_at = now()
        WHERE id = $1
          AND user_id = $2
          AND deleted_at IS NULL
          AND parent_id IS NOT NULL
          AND row_version = $5
        "#,
        node_id,
        session.user_id,
        new_parent_id,
        name_token,
        request.row_version,
    )
    .execute(&mut *tx)
    .await;

    match updated {
        Ok(result) if result.rows_affected() == 1 => {}
        Ok(_) => return Err(ApiError::conflict()),
        Err(error) if is_name_conflict(&error) => return Err(ApiError::name_conflict()),
        Err(error) => return Err(ApiError::internal(error)),
    }

    let updated_node = load_node(&mut tx, session.user_id, node_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let summary = updated_node.summary();
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(summary))
}
