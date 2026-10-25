//! 上传会话：创建、分块 presign/确认、查询、提交与中止（tasks 8.2–8.4）。
//!
//! 状态机（design D8）：
//! ```text
//! create（content_id 幂等，创建时预留配额）
//!   → open → committed（单事务）
//!          → aborted（释放预留 + 对象入删除队列）
//!          → expired（后台任务，释放预留 + 对象入删除队列）
//! ```
//! 所有状态转换 SQL 都带 `WHERE status = 'open'`，从数据库层保证单向转换。
//!
//! 安全边界：
//! - 所有查询都带 `user_id` 条件；访问他人会话与不存在一样返回 404。
//! - 对象键只由 [`ObjectKey::generate`] 生成，客户端不能指定。
//! - 日志不记录预签名 URL、加密元数据、包裹密钥或请求体。

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    bytes_equal, decode_encrypted_meta, decode_name_token, decode_wrapped_key, format_time,
    is_name_conflict, is_unique_violation, load_node, lock_user, validate_content_id,
    validate_node_id, validate_version_id,
};
use crate::AppState;
use crate::api::error::{ApiError, ErrorBody};
use crate::auth::{AuthenticatedSession, decode_hex_array, encode_hex};
use crate::jobs::deletion::enqueue_object_deletions;
use crate::storage::{MAX_PRESIGN_TTL, ObjectKey};

/// 上传会话有效期（小时）。
pub const SESSION_TTL_HOURS: i32 = 24;
/// 单块密文大小（4 MiB 明文 + 41 字节帧开销）。
pub const CHUNK_CIPHERTEXT_SIZE: i64 = crypto_core::MAX_CHUNK_CIPHERTEXT_SIZE as i64;
/// 最小密文块（空明文的帧长度）。
pub const MIN_CHUNK_CIPHERTEXT_SIZE: i64 = crypto_core::FRAME_OVERHEAD as i64;
/// 允许的最大块数（防止异常请求构造超大布局；默认 10 GiB 配额远低于此值）。
pub const MAX_CHUNK_COUNT: i32 = 262_144;
/// 批量 presign/confirm 单次请求允许的最大块数。
pub const MAX_BATCH_CHUNKS: usize = 32;

const KIND_NEW_FILE: &str = "new_file";
const KIND_NEW_VERSION: &str = "new_version";

/// 由 `chunk_count` 与 `declared_size` 推出的分块布局。
///
/// 非最后一块固定为 4 MiB + 41 字节；最后一块在 `[41, 4 MiB + 41]` 之间。
/// 空文件恰好 1 块 41 字节。任何不一致都必须直接拒绝，不能等到确认时才
/// 发现某块永远无法满足布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLayout {
    pub chunk_count: i32,
    pub last_size: i64,
}

impl ChunkLayout {
    pub fn parse(chunk_count: i32, declared_size: i64) -> Result<Self, ApiError> {
        if !(1..=MAX_CHUNK_COUNT).contains(&chunk_count) {
            return Err(ApiError::invalid_request());
        }
        if chunk_count == 1 {
            if !(MIN_CHUNK_CIPHERTEXT_SIZE..=CHUNK_CIPHERTEXT_SIZE).contains(&declared_size) {
                return Err(ApiError::invalid_request());
            }
            return Ok(Self {
                chunk_count,
                last_size: declared_size,
            });
        }

        let prefix = (i64::from(chunk_count) - 1)
            .checked_mul(CHUNK_CIPHERTEXT_SIZE)
            .ok_or_else(ApiError::invalid_request)?;
        let last_size = declared_size
            .checked_sub(prefix)
            .ok_or_else(ApiError::invalid_request)?;
        if !(MIN_CHUNK_CIPHERTEXT_SIZE..=CHUNK_CIPHERTEXT_SIZE).contains(&last_size) {
            return Err(ApiError::invalid_request());
        }
        Ok(Self {
            chunk_count,
            last_size,
        })
    }

    /// 第 `idx` 块的期望密文大小；越界返回 `None`。
    pub fn size(&self, idx: i32) -> Option<i64> {
        if idx < 0 || idx >= self.chunk_count {
            return None;
        }
        if idx == self.chunk_count - 1 {
            Some(self.last_size)
        } else {
            Some(CHUNK_CIPHERTEXT_SIZE)
        }
    }

    pub fn total_size(&self) -> i64 {
        if self.chunk_count == 1 {
            self.last_size
        } else {
            i64::from(self.chunk_count - 1) * CHUNK_CIPHERTEXT_SIZE + self.last_size
        }
    }
}

/// 创建上传会话请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateUploadSessionRequest {
    /// 客户端生成的内容 ID（UUIDv4），创建请求的幂等键。
    pub content_id: Uuid,
    /// `new_file` 或 `new_version`。
    pub kind: String,
    /// 新文件：即将创建的节点 ID；新版本：已有文件的节点 ID。
    pub node_id: Uuid,
    /// 仅新文件：目标父文件夹 ID。
    pub parent_id: Option<Uuid>,
    /// 仅新文件：包裹的节点密钥（hex）。
    pub wrapped_key: Option<String>,
    /// 仅新文件：加密节点元数据（hex）。
    pub encrypted_meta: Option<String>,
    /// 仅新文件：名称令牌（hex）。
    pub name_token: Option<String>,
    /// 客户端生成的版本 ID。
    pub version_id: Uuid,
    /// 加密版本元数据（hex）。
    pub version_meta: String,
    pub chunk_count: i32,
    /// 密文字节总数（各块密文大小之和）。
    pub declared_size: i64,
}

/// 上传会话摘要。
#[derive(Debug, Serialize, ToSchema)]
pub struct UploadSessionResponse {
    pub id: Uuid,
    pub content_id: Uuid,
    pub kind: String,
    pub status: String,
    pub node_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub version_id: Uuid,
    pub chunk_count: i32,
    pub declared_size: i64,
    /// 已确认块数（续传时与 `GET .../chunks` 配合使用）。
    pub confirmed_chunks: i64,
    pub expires_at: String,
    pub created_at: String,
}

/// presign 响应。`url` 含签名，客户端只能用于单次 PUT；服务端不记录日志。
#[derive(Debug, Serialize, ToSchema)]
pub struct PresignChunkResponse {
    pub idx: i32,
    pub method: String,
    pub url: String,
    /// 预签名 URL 的过期时间（不超过 15 分钟）。
    pub expires_at: String,
    /// 该块的期望密文大小；客户端必须上传恰好这么多字节。
    pub expected_size: i64,
}

/// 块确认请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmChunkRequest {
    /// 客户端声明的密文字节数，必须等于服务端推出的期望大小。
    pub declared_size: i64,
    /// 客户端计算的密文 SHA-256（hex，32 字节）。
    pub sha256: String,
}

/// 块确认响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfirmChunkResponse {
    pub idx: i32,
    pub declared_size: i64,
    pub sha256: String,
    pub confirmed_at: String,
}

/// 批量 presign 请求。一次最多 [`MAX_BATCH_CHUNKS`] 个块。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PresignChunksRequest {
    pub indices: Vec<i32>,
}

/// 批量 presign 中的一项：成功时带 URL，失败时带 code/message。
#[derive(Debug, Serialize, ToSchema)]
pub struct PresignChunkItemResult {
    pub idx: i32,
    pub ok: bool,
    pub url: Option<String>,
    pub method: Option<String>,
    pub expires_at: Option<String>,
    pub expected_size: Option<i64>,
    pub code: Option<String>,
    pub message: Option<String>,
}

/// 批量 presign 响应。逐块独立结果，部分失败不影响其他块。
#[derive(Debug, Serialize, ToSchema)]
pub struct PresignChunksResponse {
    pub session_id: Uuid,
    pub items: Vec<PresignChunkItemResult>,
}

/// 批量确认中的一块。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmChunkItem {
    pub idx: i32,
    pub declared_size: i64,
    pub sha256: String,
}

/// 批量确认请求。一次最多 [`MAX_BATCH_CHUNKS`] 个块。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmChunksRequest {
    pub chunks: Vec<ConfirmChunkItem>,
}

/// 批量确认中的一项结果。
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfirmChunkItemResult {
    pub idx: i32,
    pub ok: bool,
    pub declared_size: Option<i64>,
    pub sha256: Option<String>,
    pub confirmed_at: Option<String>,
    pub code: Option<String>,
    pub message: Option<String>,
}

/// 批量确认响应。逐块独立结果，部分失败不影响其他块。
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfirmChunksResponse {
    pub session_id: Uuid,
    pub items: Vec<ConfirmChunkItemResult>,
}

/// 已确认块列表中的一项。
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfirmedChunk {
    pub idx: i32,
    pub declared_size: i64,
    pub sha256: String,
    pub confirmed_at: String,
}

/// 已确认块列表响应（供断点续传）。
#[derive(Debug, Serialize, ToSchema)]
pub struct ConfirmedChunksResponse {
    pub session_id: Uuid,
    pub chunk_count: i32,
    pub confirmed_count: i64,
    pub items: Vec<ConfirmedChunk>,
}

/// 提交上传响应。重复提交返回完全相同的字段。
#[derive(Debug, Serialize, ToSchema)]
pub struct CommitUploadResponse {
    pub session_id: Uuid,
    pub status: String,
    pub node_id: Uuid,
    pub version_id: Uuid,
    pub content_id: Uuid,
    pub ciphertext_size: i64,
    pub chunk_count: i32,
    pub committed_at: String,
}

/// 中止上传响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct AbortUploadResponse {
    pub session_id: Uuid,
    pub status: String,
}

/// 上传会话行。
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct SessionRow {
    pub(crate) id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) content_id: Uuid,
    pub(crate) kind: String,
    pub(crate) node_id: Uuid,
    pub(crate) parent_id: Option<Uuid>,
    pub(crate) wrapped_key: Option<Vec<u8>>,
    pub(crate) encrypted_meta: Option<Vec<u8>>,
    pub(crate) name_token: Option<Vec<u8>>,
    pub(crate) version_id: Uuid,
    pub(crate) version_meta: Vec<u8>,
    pub(crate) chunk_count: i32,
    pub(crate) declared_size: i64,
    pub(crate) reserved_bytes: i64,
    pub(crate) status: String,
    pub(crate) expires_at: OffsetDateTime,
    pub(crate) created_at: OffsetDateTime,
    pub(crate) committed_at: Option<OffsetDateTime>,
}

/// 已解析并校验过的创建请求（所有二进制字段已解码）。
struct ValidatedCreate {
    content_id: Uuid,
    kind: String,
    node_id: Uuid,
    parent_id: Option<Uuid>,
    wrapped_key: Option<Vec<u8>>,
    encrypted_meta: Option<Vec<u8>>,
    name_token: Option<Vec<u8>>,
    version_id: Uuid,
    version_meta: Vec<u8>,
    layout: ChunkLayout,
}

impl ValidatedCreate {
    fn validate(request: CreateUploadSessionRequest) -> Result<Self, ApiError> {
        let content_id = validate_content_id(request.content_id)?;
        let node_id = validate_node_id(request.node_id)?;
        let version_id = validate_version_id(request.version_id)?;
        let version_meta = decode_encrypted_meta(&request.version_meta)?;
        let layout = ChunkLayout::parse(request.chunk_count, request.declared_size)?;

        let (parent_id, wrapped_key, encrypted_meta, name_token) = match request.kind.as_str() {
            KIND_NEW_FILE => {
                let parent_id = request.parent_id.ok_or_else(ApiError::invalid_request)?;
                let parent_id = validate_node_id(parent_id)?;
                if request.wrapped_key.is_none()
                    || request.encrypted_meta.is_none()
                    || request.name_token.is_none()
                {
                    return Err(ApiError::invalid_request());
                }
                let wrapped_key =
                    decode_wrapped_key(request.wrapped_key.as_deref().unwrap_or_default())?;
                let encrypted_meta =
                    decode_encrypted_meta(request.encrypted_meta.as_deref().unwrap_or_default())?;
                let name_token =
                    decode_name_token(request.name_token.as_deref().unwrap_or_default())?;
                (
                    Some(parent_id),
                    Some(wrapped_key),
                    Some(encrypted_meta),
                    Some(name_token),
                )
            }
            KIND_NEW_VERSION => {
                if request.parent_id.is_some()
                    || request.wrapped_key.is_some()
                    || request.encrypted_meta.is_some()
                    || request.name_token.is_some()
                {
                    return Err(ApiError::invalid_request());
                }
                (None, None, None, None)
            }
            _ => return Err(ApiError::invalid_request()),
        };

        Ok(Self {
            content_id,
            kind: request.kind,
            node_id,
            parent_id,
            wrapped_key,
            encrypted_meta,
            name_token,
            version_id,
            version_meta,
            layout,
        })
    }
}

fn parse_body<T: serde::de::DeserializeOwned>(
    payload: Result<Json<T>, JsonRejection>,
) -> Result<T, ApiError> {
    payload
        .map(|Json(value)| value)
        .map_err(|_| ApiError::invalid_request())
}

/// 读取会话（带 `user_id` 条件；不存在与跨用户都返回 `None`）。
async fn load_session(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<Option<SessionRow>, sqlx::Error> {
    sqlx::query_as!(
        SessionRow,
        r#"
        SELECT
            id, user_id, content_id, kind, node_id, parent_id,
            wrapped_key, encrypted_meta, name_token,
            version_id, version_meta, chunk_count, declared_size, reserved_bytes,
            status, expires_at, created_at, committed_at
        FROM upload_sessions
        WHERE id = $1 AND user_id = $2
        "#,
        session_id,
        user_id,
    )
    .fetch_optional(&mut **tx)
    .await
}

/// 读取并锁定会话（写路径）。
async fn load_session_for_update(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<Option<SessionRow>, sqlx::Error> {
    sqlx::query_as!(
        SessionRow,
        r#"
        SELECT
            id, user_id, content_id, kind, node_id, parent_id,
            wrapped_key, encrypted_meta, name_token,
            version_id, version_meta, chunk_count, declared_size, reserved_bytes,
            status, expires_at, created_at, committed_at
        FROM upload_sessions
        WHERE id = $1 AND user_id = $2
        FOR UPDATE
        "#,
        session_id,
        user_id,
    )
    .fetch_optional(&mut **tx)
    .await
}

/// 按 `content_id` 读取会话（创建幂等检查；`content_id` 全局唯一）。
async fn load_session_by_content(
    tx: &mut Transaction<'_, Postgres>,
    content_id: Uuid,
) -> Result<Option<SessionRow>, sqlx::Error> {
    sqlx::query_as!(
        SessionRow,
        r#"
        SELECT
            id, user_id, content_id, kind, node_id, parent_id,
            wrapped_key, encrypted_meta, name_token,
            version_id, version_meta, chunk_count, declared_size, reserved_bytes,
            status, expires_at, created_at, committed_at
        FROM upload_sessions
        WHERE content_id = $1
        "#,
        content_id,
    )
    .fetch_optional(&mut **tx)
    .await
}

fn same_request(existing: &SessionRow, request: &ValidatedCreate) -> bool {
    existing.content_id == request.content_id
        && existing.kind == request.kind
        && existing.node_id == request.node_id
        && existing.parent_id == request.parent_id
        && existing.version_id == request.version_id
        && existing.chunk_count == request.layout.chunk_count
        && existing.declared_size == request.layout.total_size()
        && option_bytes_equal(
            existing.wrapped_key.as_deref(),
            request.wrapped_key.as_deref(),
        )
        && option_bytes_equal(
            existing.encrypted_meta.as_deref(),
            request.encrypted_meta.as_deref(),
        )
        && option_bytes_equal(
            existing.name_token.as_deref(),
            request.name_token.as_deref(),
        )
        && bytes_equal(&existing.version_meta, &request.version_meta)
}

fn option_bytes_equal(left: Option<&[u8]>, right: Option<&[u8]>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => bytes_equal(left, right),
        _ => false,
    }
}

async fn confirmed_count(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        SELECT count(*) AS "count!"
        FROM upload_chunks
        WHERE user_id = $1 AND session_id = $2 AND confirmed_at IS NOT NULL
        "#,
        user_id,
        session_id,
    )
    .fetch_one(&mut **tx)
    .await
}

fn session_response(row: &SessionRow, confirmed: i64) -> UploadSessionResponse {
    UploadSessionResponse {
        id: row.id,
        content_id: row.content_id,
        kind: row.kind.clone(),
        status: row.status.clone(),
        node_id: row.node_id,
        parent_id: row.parent_id,
        version_id: row.version_id,
        chunk_count: row.chunk_count,
        declared_size: row.declared_size,
        confirmed_chunks: confirmed,
        expires_at: format_time(row.expires_at),
        created_at: format_time(row.created_at),
    }
}

/// `POST /api/v1/upload-sessions`。
///
/// 创建时在同一事务内锁定用户行、检查邮箱验证状态并预留配额。
/// `content_id` 是幂等键：同一请求重复提交返回同一会话且不重复预留；
/// 同一 `content_id` 不同内容返回 `conflict`。
#[utoipa::path(
    post,
    path = "/api/v1/upload-sessions",
    tag = "uploads",
    request_body = CreateUploadSessionRequest,
    responses(
        (status = 200, body = UploadSessionResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn create_upload_session(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    payload: Result<Json<CreateUploadSessionRequest>, JsonRejection>,
) -> Result<Json<UploadSessionResponse>, ApiError> {
    state.auth.check_user_write(session.user_id)?;
    let request = ValidatedCreate::validate(parse_body(payload)?)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    // 同一用户的配额检查与预留必须串行化（AGENTS.md 硬性规则）。
    lock_user(&mut tx, session.user_id).await?;

    let account = sqlx::query!(
        r#"
        SELECT email_verified_at, quota_bytes, used_bytes, reserved_bytes
        FROM users
        WHERE id = $1
        FOR UPDATE
        "#,
        session.user_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::unauthenticated)?;
    if account.email_verified_at.is_none() {
        return Err(ApiError::email_not_verified());
    }

    // 幂等：content_id 已存在时比较请求内容。
    if let Some(existing) = load_session_by_content(&mut tx, request.content_id)
        .await
        .map_err(ApiError::internal)?
    {
        if existing.user_id != session.user_id || !same_request(&existing, &request) {
            return Err(ApiError::conflict());
        }
        let confirmed = confirmed_count(&mut tx, session.user_id, existing.id)
            .await
            .map_err(ApiError::internal)?;
        let response = session_response(&existing, confirmed);
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(response));
    }

    let used = i128::from(account.used_bytes);
    let reserved = i128::from(account.reserved_bytes);
    let declared = i128::from(request.layout.total_size());
    let quota = i128::from(account.quota_bytes);
    if used + reserved + declared > quota {
        return Err(ApiError::quota_exceeded());
    }

    let session_id = Uuid::new_v4();
    let inserted = sqlx::query_scalar!(
        r#"
        INSERT INTO upload_sessions (
            id, user_id, content_id, kind, node_id, parent_id,
            wrapped_key, encrypted_meta, name_token,
            version_id, version_meta, chunk_count, declared_size, reserved_bytes,
            expires_at
        )
        VALUES (
            $1, $2, $3, $4, $5, $6,
            $7, $8, $9,
            $10, $11, $12, $13, $13,
            now() + make_interval(hours => $14)
        )
        ON CONFLICT (content_id) DO NOTHING
        RETURNING id
        "#,
        session_id,
        session.user_id,
        request.content_id,
        request.kind,
        request.node_id,
        request.parent_id,
        request.wrapped_key,
        request.encrypted_meta,
        request.name_token,
        request.version_id,
        request.version_meta,
        request.layout.chunk_count,
        request.layout.total_size(),
        SESSION_TTL_HOURS,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let Some(inserted) = inserted else {
        // 跨用户并发使用了同一个 content_id：按“同一幂等键不同内容”处理。
        let existing = load_session_by_content(&mut tx, request.content_id)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| ApiError::internal("content_id 冲突但读取不到会话"))?;
        if existing.user_id != session.user_id || !same_request(&existing, &request) {
            return Err(ApiError::conflict());
        }
        let confirmed = confirmed_count(&mut tx, session.user_id, existing.id)
            .await
            .map_err(ApiError::internal)?;
        let response = session_response(&existing, confirmed);
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(response));
    };

    sqlx::query!(
        r#"
        UPDATE users
        SET reserved_bytes = reserved_bytes + $2,
            updated_at = now()
        WHERE id = $1
        "#,
        session.user_id,
        request.layout.total_size(),
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let created = load_session(&mut tx, session.user_id, inserted)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(|| ApiError::internal("新创建的上传会话读取失败"))?;
    let response = session_response(&created, 0);
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(response))
}

/// `GET /api/v1/upload-sessions/{session_id}`。
#[utoipa::path(
    get,
    path = "/api/v1/upload-sessions/{session_id}",
    tag = "uploads",
    params(("session_id" = Uuid, Path, description = "上传会话 ID")),
    responses(
        (status = 200, body = UploadSessionResponse),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn get_upload_session(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(session_id): Path<Uuid>,
) -> Result<Json<UploadSessionResponse>, ApiError> {
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let row = load_session(&mut tx, session.user_id, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    let confirmed = confirmed_count(&mut tx, session.user_id, row.id)
        .await
        .map_err(ApiError::internal)?;
    let response = session_response(&row, confirmed);
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(response))
}

/// `GET /api/v1/upload-sessions/{session_id}/chunks`。
///
/// 只返回已确认的块；客户端据此只上传缺失块（断点续传）。
#[utoipa::path(
    get,
    path = "/api/v1/upload-sessions/{session_id}/chunks",
    tag = "uploads",
    params(("session_id" = Uuid, Path, description = "上传会话 ID")),
    responses(
        (status = 200, body = ConfirmedChunksResponse),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn list_confirmed_chunks(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(session_id): Path<Uuid>,
) -> Result<Json<ConfirmedChunksResponse>, ApiError> {
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let row = load_session(&mut tx, session.user_id, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;

    let rows = sqlx::query!(
        r#"
        SELECT idx, declared_size, sha256, confirmed_at
        FROM upload_chunks
        WHERE user_id = $1 AND session_id = $2 AND confirmed_at IS NOT NULL
        ORDER BY idx
        "#,
        session.user_id,
        session_id,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let mut count = 0_i64;
    let items = rows
        .into_iter()
        .filter_map(|chunk| {
            let sha256 = chunk.sha256?;
            let confirmed_at = chunk.confirmed_at?;
            count += 1;
            Some(ConfirmedChunk {
                idx: chunk.idx,
                declared_size: chunk.declared_size,
                sha256: encode_hex(&sha256),
                confirmed_at: format_time(confirmed_at),
            })
        })
        .collect();
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(ConfirmedChunksResponse {
        session_id: row.id,
        chunk_count: row.chunk_count,
        confirmed_count: count,
        items,
    }))
}

/// `POST /api/v1/upload-sessions/{session_id}/chunks/{idx}/presign`。
///
/// 每次调用都生成新的服务端对象键；旧键写入 `superseded_object_keys`，
/// 在提交/中止/过期时统一进入删除队列。已确认状态会被重置（新键尚未上传）。
#[utoipa::path(
    post,
    path = "/api/v1/upload-sessions/{session_id}/chunks/{idx}/presign",
    tag = "uploads",
    params(
        ("session_id" = Uuid, Path, description = "上传会话 ID"),
        ("idx" = i32, Path, description = "块序号（从 0 开始）")
    ),
    responses(
        (status = 200, body = PresignChunkResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn presign_chunk(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path((session_id, idx)): Path<(Uuid, i32)>,
) -> Result<Json<PresignChunkResponse>, ApiError> {
    state.auth.check_user_write(session.user_id)?;
    Ok(Json(
        presign_chunk_core(&state, session.user_id, session_id, idx).await?,
    ))
}

/// `POST /api/v1/upload-sessions/{session_id}/chunks/presign`。
///
/// 一次为最多 [`MAX_BATCH_CHUNKS`] 个块生成 URL；逐块独立结果。
#[utoipa::path(
    post,
    path = "/api/v1/upload-sessions/{session_id}/chunks/presign",
    tag = "uploads",
    params(("session_id" = Uuid, Path, description = "上传会话 ID")),
    request_body = PresignChunksRequest,
    responses(
        (status = 200, body = PresignChunksResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn presign_chunks(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(session_id): Path<Uuid>,
    payload: Result<Json<PresignChunksRequest>, JsonRejection>,
) -> Result<Json<PresignChunksResponse>, ApiError> {
    state.auth.check_user_write(session.user_id)?;
    let request = parse_body(payload)?;
    if request.indices.is_empty() || request.indices.len() > MAX_BATCH_CHUNKS {
        return Err(ApiError::invalid_request());
    }
    // 同一块在一次批量 presign 中出现两次会让前一个 URL 立即失效。
    let mut seen = std::collections::HashSet::with_capacity(request.indices.len());
    if !request.indices.iter().all(|idx| seen.insert(*idx)) {
        return Err(ApiError::invalid_request());
    }

    // 先确认会话归属与 open/未过期；逐块错误在 items 中返回。
    let row = load_open_session(&state, session.user_id, session_id).await?;
    let layout = ChunkLayout::parse(row.chunk_count, row.declared_size)
        .map_err(|_| ApiError::internal("上传会话的分块布局无效"))?;

    let mut items = Vec::with_capacity(request.indices.len());
    for idx in request.indices {
        if layout.size(idx).is_none() {
            items.push(PresignChunkItemResult {
                idx,
                ok: false,
                url: None,
                method: None,
                expires_at: None,
                expected_size: None,
                code: Some("invalid_request".to_owned()),
                message: Some("块序号超出会话范围".to_owned()),
            });
            continue;
        }
        let presigned = presign_chunk_core(&state, session.user_id, session_id, idx).await?;
        items.push(PresignChunkItemResult {
            idx,
            ok: true,
            url: Some(presigned.url),
            method: Some(presigned.method),
            expires_at: Some(presigned.expires_at),
            expected_size: Some(presigned.expected_size),
            code: None,
            message: None,
        });
    }
    Ok(Json(PresignChunksResponse { session_id, items }))
}

async fn presign_chunk_core(
    state: &AppState,
    user_id: Uuid,
    session_id: Uuid,
    idx: i32,
) -> Result<PresignChunkResponse, ApiError> {
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let row = load_session_for_update(&mut tx, user_id, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    require_open(&row)?;

    let layout = ChunkLayout::parse(row.chunk_count, row.declared_size)
        .map_err(|_| ApiError::internal("上传会话的分块布局无效"))?;
    let expected_size = layout.size(idx).ok_or_else(ApiError::invalid_request)?;
    let object_key = ObjectKey::generate(user_id);

    sqlx::query!(
        r#"
        INSERT INTO upload_chunks (
            session_id, user_id, idx, object_key, declared_size
        )
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (session_id, idx) DO UPDATE
        SET superseded_object_keys =
                array_append(upload_chunks.superseded_object_keys, upload_chunks.object_key),
            object_key = EXCLUDED.object_key,
            declared_size = EXCLUDED.declared_size,
            sha256 = NULL,
            confirmed_at = NULL,
            updated_at = now()
        "#,
        session_id,
        user_id,
        idx,
        object_key.as_str(),
        expected_size,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;

    // 预签名 URL 只在返回给客户端时使用；Debug 输出已脱敏，日志中绝不出现。
    let presigned = state
        .storage
        .presign_write(&object_key, MAX_PRESIGN_TTL)
        .await
        .map_err(ApiError::internal)?;
    Ok(PresignChunkResponse {
        idx,
        method: presigned.method().to_owned(),
        url: presigned.url().to_owned(),
        expires_at: format_time(OffsetDateTime::now_utc() + time::Duration::minutes(15)),
        expected_size,
    })
}

/// 读取一个归属当前用户、open 且未过期的会话（短事务，不持有锁）。
async fn load_open_session(
    state: &AppState,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<SessionRow, ApiError> {
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let row = load_session(&mut tx, user_id, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    require_open(&row)?;
    tx.rollback().await.map_err(ApiError::internal)?;
    Ok(row)
}

/// 会话必须处于 open 且未过期。
fn require_open(row: &SessionRow) -> Result<(), ApiError> {
    if row.status != "open" || row.expires_at <= OffsetDateTime::now_utc() {
        return Err(ApiError::upload_session_closed());
    }
    Ok(())
}

/// `POST /api/v1/upload-sessions/{session_id}/chunks/{idx}/confirm`。
///
/// 服务端向对象存储 `stat` 实际大小，必须等于该块期望大小；相同块、相同哈希
/// 的重复确认幂等成功。
#[utoipa::path(
    post,
    path = "/api/v1/upload-sessions/{session_id}/chunks/{idx}/confirm",
    tag = "uploads",
    params(
        ("session_id" = Uuid, Path, description = "上传会话 ID"),
        ("idx" = i32, Path, description = "块序号（从 0 开始）")
    ),
    request_body = ConfirmChunkRequest,
    responses(
        (status = 200, body = ConfirmChunkResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn confirm_chunk(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path((session_id, idx)): Path<(Uuid, i32)>,
    payload: Result<Json<ConfirmChunkRequest>, JsonRejection>,
) -> Result<Json<ConfirmChunkResponse>, ApiError> {
    state.auth.check_user_write(session.user_id)?;
    let request = parse_body(payload)?;
    let sha256 = decode_hex_array::<32>(&request.sha256).ok_or_else(ApiError::invalid_request)?;
    let outcome = confirm_chunk_core(
        &state,
        session.user_id,
        session_id,
        idx,
        request.declared_size,
        sha256,
    )
    .await?;
    match outcome {
        ConfirmOutcome::Confirmed(response) => Ok(Json(response)),
        ConfirmOutcome::ChunkNotFound => Err(ApiError::chunk_not_found()),
        ConfirmOutcome::SizeMismatch => Err(ApiError::chunk_size_mismatch()),
        ConfirmOutcome::Conflict => Err(ApiError::conflict()),
    }
}

/// `POST /api/v1/upload-sessions/{session_id}/chunks/confirm`。
///
/// 一次确认最多 [`MAX_BATCH_CHUNKS`] 个块；逐块独立结果，部分失败不影响其他块。
#[utoipa::path(
    post,
    path = "/api/v1/upload-sessions/{session_id}/chunks/confirm",
    tag = "uploads",
    params(("session_id" = Uuid, Path, description = "上传会话 ID")),
    request_body = ConfirmChunksRequest,
    responses(
        (status = 200, body = ConfirmChunksResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn confirm_chunks(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(session_id): Path<Uuid>,
    payload: Result<Json<ConfirmChunksRequest>, JsonRejection>,
) -> Result<Json<ConfirmChunksResponse>, ApiError> {
    state.auth.check_user_write(session.user_id)?;
    let request = parse_body(payload)?;
    if request.chunks.is_empty() || request.chunks.len() > MAX_BATCH_CHUNKS {
        return Err(ApiError::invalid_request());
    }

    let row = load_open_session(&state, session.user_id, session_id).await?;
    let layout = ChunkLayout::parse(row.chunk_count, row.declared_size)
        .map_err(|_| ApiError::internal("上传会话的分块布局无效"))?;

    let mut items = Vec::with_capacity(request.chunks.len());
    for chunk in request.chunks {
        let Some(sha256) = decode_hex_array::<32>(&chunk.sha256) else {
            items.push(ConfirmChunkItemResult {
                idx: chunk.idx,
                ok: false,
                declared_size: None,
                sha256: None,
                confirmed_at: None,
                code: Some("invalid_request".to_owned()),
                message: Some("SHA-256 格式无效".to_owned()),
            });
            continue;
        };
        let Some(expected_size) = layout.size(chunk.idx) else {
            items.push(ConfirmChunkItemResult {
                idx: chunk.idx,
                ok: false,
                declared_size: None,
                sha256: None,
                confirmed_at: None,
                code: Some("invalid_request".to_owned()),
                message: Some("块序号超出会话范围".to_owned()),
            });
            continue;
        };
        if chunk.declared_size != expected_size {
            items.push(ConfirmChunkItemResult {
                idx: chunk.idx,
                ok: false,
                declared_size: Some(chunk.declared_size),
                sha256: Some(encode_hex(&sha256)),
                confirmed_at: None,
                code: Some("chunk_size_mismatch".to_owned()),
                message: Some("块大小与声明不一致".to_owned()),
            });
            continue;
        }

        // 每块独立完成“短读 → 事务外 stat → 短写”，互不占用连接与行锁。
        let outcome = confirm_chunk_core(
            &state,
            session.user_id,
            session_id,
            chunk.idx,
            chunk.declared_size,
            sha256,
        )
        .await?;
        items.push(match outcome {
            ConfirmOutcome::Confirmed(response) => ConfirmChunkItemResult {
                idx: chunk.idx,
                ok: true,
                declared_size: Some(response.declared_size),
                sha256: Some(response.sha256),
                confirmed_at: Some(response.confirmed_at),
                code: None,
                message: None,
            },
            ConfirmOutcome::ChunkNotFound => ConfirmChunkItemResult {
                idx: chunk.idx,
                ok: false,
                declared_size: None,
                sha256: None,
                confirmed_at: None,
                code: Some("chunk_not_found".to_owned()),
                message: Some("块对象不存在".to_owned()),
            },
            ConfirmOutcome::SizeMismatch => ConfirmChunkItemResult {
                idx: chunk.idx,
                ok: false,
                declared_size: Some(chunk.declared_size),
                sha256: Some(encode_hex(&sha256)),
                confirmed_at: None,
                code: Some("chunk_size_mismatch".to_owned()),
                message: Some("块大小与声明不一致".to_owned()),
            },
            ConfirmOutcome::Conflict => ConfirmChunkItemResult {
                idx: chunk.idx,
                ok: false,
                declared_size: None,
                sha256: None,
                confirmed_at: None,
                code: Some("conflict".to_owned()),
                message: Some("块对象已被重新 presign，请重试".to_owned()),
            },
        });
    }
    Ok(Json(ConfirmChunksResponse { session_id, items }))
}

/// 单块确认的逐块结果。
enum ConfirmOutcome {
    Confirmed(ConfirmChunkResponse),
    ChunkNotFound,
    SizeMismatch,
    Conflict,
}

/// 单块确认核心：短读校验 → 事务外 stat → 短事务复核并写入。
///
/// 绝不在持有连接/事务/会话行锁时调用对象存储；慢 OSS 只会拖慢当前请求，
/// 不会占住连接池或阻塞同一会话的其他块。
async fn confirm_chunk_core(
    state: &AppState,
    user_id: Uuid,
    session_id: Uuid,
    idx: i32,
    declared_size: i64,
    sha256: [u8; 32],
) -> Result<ConfirmOutcome, ApiError> {
    // 1. 短读：归属、open、布局与当前对象键。读完立即归还连接。
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let row = load_session(&mut tx, user_id, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    require_open(&row)?;
    let layout = ChunkLayout::parse(row.chunk_count, row.declared_size)
        .map_err(|_| ApiError::internal("上传会话的分块布局无效"))?;
    let expected_size = layout.size(idx).ok_or_else(ApiError::invalid_request)?;
    if declared_size != expected_size {
        tx.rollback().await.map_err(ApiError::internal)?;
        return Ok(ConfirmOutcome::SizeMismatch);
    }
    let chunk = sqlx::query!(
        r#"
        SELECT object_key
        FROM upload_chunks
        WHERE user_id = $1 AND session_id = $2 AND idx = $3
        "#,
        user_id,
        session_id,
        idx,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    let Some(chunk) = chunk else {
        tx.rollback().await.map_err(ApiError::internal)?;
        return Ok(ConfirmOutcome::ChunkNotFound);
    };
    let object_key = chunk.object_key;
    tx.rollback().await.map_err(ApiError::internal)?;

    // 2. 事务外 stat：慢 OSS 不占用数据库连接，也不持有任何行锁。
    let metadata = state
        .storage
        .stat(&object_key)
        .await
        .map_err(ApiError::internal)?;
    let Some(metadata) = metadata else {
        return Ok(ConfirmOutcome::ChunkNotFound);
    };
    if metadata.size != expected_size as u64 {
        return Ok(ConfirmOutcome::SizeMismatch);
    }

    // 3. 短写事务：FOR UPDATE 复核对象键未变、会话仍 open，再写入确认。
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let row = load_session_for_update(&mut tx, user_id, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;
    require_open(&row)?;
    let layout = ChunkLayout::parse(row.chunk_count, row.declared_size)
        .map_err(|_| ApiError::internal("上传会话的分块布局无效"))?;
    let expected_size = layout.size(idx).ok_or_else(ApiError::invalid_request)?;
    if declared_size != expected_size {
        tx.rollback().await.map_err(ApiError::internal)?;
        return Ok(ConfirmOutcome::SizeMismatch);
    }
    let chunk = sqlx::query!(
        r#"
        SELECT object_key, declared_size, sha256, confirmed_at
        FROM upload_chunks
        WHERE user_id = $1 AND session_id = $2 AND idx = $3
        FOR UPDATE
        "#,
        user_id,
        session_id,
        idx,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    let Some(chunk) = chunk else {
        tx.rollback().await.map_err(ApiError::internal)?;
        return Ok(ConfirmOutcome::ChunkNotFound);
    };
    if chunk.object_key != object_key {
        // stat 期间该块被重新 presign：旧对象的确认不能覆盖新对象。
        tx.rollback().await.map_err(ApiError::internal)?;
        return Ok(ConfirmOutcome::Conflict);
    }

    if let (Some(confirmed_at), Some(stored_sha)) = (chunk.confirmed_at, chunk.sha256.as_deref()) {
        if bytes_equal(stored_sha, &sha256) {
            let response = ConfirmChunkResponse {
                idx,
                declared_size: chunk.declared_size,
                sha256: encode_hex(stored_sha),
                confirmed_at: format_time(confirmed_at),
            };
            tx.rollback().await.map_err(ApiError::internal)?;
            return Ok(ConfirmOutcome::Confirmed(response));
        }
        tx.rollback().await.map_err(ApiError::internal)?;
        return Ok(ConfirmOutcome::Conflict);
    }

    let confirmed_at = sqlx::query_scalar!(
        r#"
        UPDATE upload_chunks
        SET sha256 = $4,
            confirmed_at = now(),
            declared_size = $5,
            updated_at = now()
        WHERE user_id = $1 AND session_id = $2 AND idx = $3
        RETURNING confirmed_at
        "#,
        user_id,
        session_id,
        idx,
        sha256.as_slice(),
        expected_size,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::internal("确认时间写入失败"))?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(ConfirmOutcome::Confirmed(ConfirmChunkResponse {
        idx,
        declared_size: expected_size,
        sha256: encode_hex(&sha256),
        confirmed_at: format_time(confirmed_at),
    }))
}

/// `POST /api/v1/upload-sessions/{session_id}/commit`。
#[utoipa::path(
    post,
    path = "/api/v1/upload-sessions/{session_id}/commit",
    tag = "uploads",
    params(("session_id" = Uuid, Path, description = "上传会话 ID")),
    responses(
        (status = 200, body = CommitUploadResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn commit_upload(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(session_id): Path<Uuid>,
) -> Result<Json<CommitUploadResponse>, ApiError> {
    state.auth.check_user_write(session.user_id)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    lock_user(&mut tx, session.user_id).await?;
    let row = load_session_for_update(&mut tx, session.user_id, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;

    // 幂等：已提交的会话直接返回第一次的结果。
    if row.status == "committed" {
        let response = committed_response(&row)?;
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(response));
    }
    require_open(&row)?;

    let layout = ChunkLayout::parse(row.chunk_count, row.declared_size)
        .map_err(|_| ApiError::internal("上传会话的分块布局无效"))?;

    let chunks = sqlx::query!(
        r#"
        SELECT idx, object_key, declared_size, sha256, confirmed_at, superseded_object_keys
        FROM upload_chunks
        WHERE user_id = $1 AND session_id = $2
        ORDER BY idx
        "#,
        session.user_id,
        session_id,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let mut confirmed = vec![false; row.chunk_count as usize];
    let mut total: i64 = 0;
    for chunk in &chunks {
        if let Ok(position) = usize::try_from(chunk.idx)
            && position < confirmed.len()
            && chunk.confirmed_at.is_some()
        {
            confirmed[position] = true;
        }
        total = total
            .checked_add(chunk.declared_size)
            .ok_or_else(|| ApiError::internal("块大小求和溢出"))?;
    }
    let missing: Vec<i32> = confirmed
        .iter()
        .enumerate()
        .filter_map(|(idx, done)| (!done).then_some(idx as i32))
        .collect();
    if !missing.is_empty() {
        return Err(ApiError::upload_incomplete(missing));
    }

    // 确认大小之和必须等于声明值（防止数据库被篡改或块声明不一致）。
    if total != row.declared_size || total != layout.total_size() {
        return Err(ApiError::chunk_size_mismatch());
    }

    // 目标可用性检查：目标文件夹/文件必须仍存在且未删除。
    if row.kind == KIND_NEW_FILE {
        let parent_id = row.parent_id.ok_or_else(ApiError::target_unavailable)?;
        let parent = load_node(&mut tx, session.user_id, parent_id)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::target_unavailable)?;
        if parent.is_deleted() || parent.kind != "folder" {
            return Err(ApiError::target_unavailable());
        }
        let name_token = row
            .name_token
            .as_deref()
            .ok_or_else(|| ApiError::internal("新文件会话缺少名称令牌"))?;
        let conflict = sqlx::query_scalar!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM nodes
                WHERE user_id = $1
                  AND parent_id = $2
                  AND name_token = $3
                  AND deleted_at IS NULL
            ) AS "exists!"
            "#,
            session.user_id,
            parent_id,
            name_token,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
        if conflict {
            return Err(ApiError::name_conflict());
        }
    } else {
        let target = load_node(&mut tx, session.user_id, row.node_id)
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(ApiError::target_unavailable)?;
        if target.is_deleted() || target.kind != "file" {
            return Err(ApiError::target_unavailable());
        }
    }

    // 客户端生成的 ID 不能已经被占用；占用说明这是另一个创建请求。
    let node_exists = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM nodes WHERE id = $1) AS \"exists!\"",
        row.node_id,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if node_exists && row.kind == KIND_NEW_FILE {
        return Err(ApiError::conflict());
    }
    let content_exists = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM contents WHERE id = $1) AS \"exists!\"",
        row.content_id,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if content_exists {
        return Err(ApiError::conflict());
    }
    let version_exists = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM file_versions WHERE id = $1) AS \"exists!\"",
        row.version_id,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if version_exists {
        return Err(ApiError::conflict());
    }

    if row.kind == KIND_NEW_FILE {
        let parent_id = row.parent_id.ok_or_else(ApiError::target_unavailable)?;
        let wrapped_key = row
            .wrapped_key
            .as_deref()
            .ok_or_else(|| ApiError::internal("新文件会话缺少包裹密钥"))?;
        let encrypted_meta = row
            .encrypted_meta
            .as_deref()
            .ok_or_else(|| ApiError::internal("新文件会话缺少加密元数据"))?;
        let name_token = row
            .name_token
            .as_deref()
            .ok_or_else(|| ApiError::internal("新文件会话缺少名称令牌"))?;
        let inserted = sqlx::query!(
            r#"
            INSERT INTO nodes (
                id, user_id, parent_id, kind, wrapped_key, encrypted_meta,
                name_token, current_version_id
            )
            VALUES ($1, $2, $3, 'file', $4, $5, $6, $7)
            "#,
            row.node_id,
            session.user_id,
            parent_id,
            wrapped_key,
            encrypted_meta,
            name_token,
            row.version_id,
        )
        .execute(&mut *tx)
        .await;
        match inserted {
            Ok(_) => {}
            Err(error) if is_name_conflict(&error) => return Err(ApiError::name_conflict()),
            Err(error) => return Err(ApiError::internal(error)),
        }
    }

    sqlx::query!(
        r#"
        INSERT INTO contents (id, user_id, node_id, chunk_count, ciphertext_size)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        row.content_id,
        session.user_id,
        row.node_id,
        row.chunk_count,
        row.declared_size,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    sqlx::query!(
        r#"
        INSERT INTO file_versions (id, user_id, node_id, content_id, encrypted_meta)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        row.version_id,
        session.user_id,
        row.node_id,
        row.content_id,
        row.version_meta,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    // 新文件已在插入节点时写入 current_version_id；新版本需要移动指针。
    if row.kind == KIND_NEW_VERSION {
        sqlx::query!(
            r#"
            UPDATE nodes
            SET current_version_id = $3,
                row_version = row_version + 1,
                updated_at = now()
            WHERE id = $1 AND user_id = $2
            "#,
            row.node_id,
            session.user_id,
            row.version_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    }

    let mut superseded = Vec::new();
    for chunk in &chunks {
        let Some(sha256) = chunk.sha256.as_deref() else {
            return Err(ApiError::internal("已确认块缺少 SHA-256"));
        };
        sqlx::query!(
            r#"
            INSERT INTO chunks (content_id, user_id, idx, object_key, ciphertext_size, sha256)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            row.content_id,
            session.user_id,
            chunk.idx,
            chunk.object_key,
            chunk.declared_size,
            sha256,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            if is_unique_violation(&error, "chunks_object_key_key") {
                ApiError::conflict()
            } else {
                ApiError::internal(error)
            }
        })?;
        superseded.extend(chunk.superseded_object_keys.iter().cloned());
    }
    enqueue_object_deletions(&mut tx, &superseded)
        .await
        .map_err(ApiError::internal)?;

    sqlx::query!(
        "DELETE FROM upload_chunks WHERE user_id = $1 AND session_id = $2",
        session.user_id,
        session_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    sqlx::query!(
        r#"
        UPDATE users
        SET reserved_bytes = reserved_bytes - $2,
            used_bytes = used_bytes + $2,
            updated_at = now()
        WHERE id = $1
        "#,
        session.user_id,
        row.declared_size,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let committed_at = sqlx::query_scalar!(
        r#"
        UPDATE upload_sessions
        SET status = 'committed',
            committed_at = now()
        WHERE id = $1 AND user_id = $2 AND status = 'open'
        RETURNING committed_at
        "#,
        session_id,
        session.user_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(|| ApiError::internal("提交时会话状态已被并发修改"))?
    .ok_or_else(|| ApiError::internal("提交时间写入失败"))?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(CommitUploadResponse {
        session_id,
        status: "committed".to_owned(),
        node_id: row.node_id,
        version_id: row.version_id,
        content_id: row.content_id,
        ciphertext_size: row.declared_size,
        chunk_count: row.chunk_count,
        committed_at: format_time(committed_at),
    }))
}

fn committed_response(row: &SessionRow) -> Result<CommitUploadResponse, ApiError> {
    let committed_at = row
        .committed_at
        .ok_or_else(|| ApiError::internal("已提交会话缺少 committed_at"))?;
    Ok(CommitUploadResponse {
        session_id: row.id,
        status: "committed".to_owned(),
        node_id: row.node_id,
        version_id: row.version_id,
        content_id: row.content_id,
        ciphertext_size: row.declared_size,
        chunk_count: row.chunk_count,
        committed_at: format_time(committed_at),
    })
}

/// `POST /api/v1/upload-sessions/{session_id}/abort`。
#[utoipa::path(
    post,
    path = "/api/v1/upload-sessions/{session_id}/abort",
    tag = "uploads",
    params(("session_id" = Uuid, Path, description = "上传会话 ID")),
    responses(
        (status = 200, body = AbortUploadResponse),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn abort_upload(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(session_id): Path<Uuid>,
) -> Result<Json<AbortUploadResponse>, ApiError> {
    state.auth.check_user_write(session.user_id)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    lock_user(&mut tx, session.user_id).await?;
    let row = load_session_for_update(&mut tx, session.user_id, session_id)
        .await
        .map_err(ApiError::internal)?
        .ok_or_else(ApiError::not_found)?;

    if row.status == "aborted" {
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(AbortUploadResponse {
            session_id,
            status: "aborted".to_owned(),
        }));
    }
    require_open(&row)?;

    let object_keys = collect_session_object_keys(&mut tx, session.user_id, session_id)
        .await
        .map_err(ApiError::internal)?;
    enqueue_object_deletions(&mut tx, &object_keys)
        .await
        .map_err(ApiError::internal)?;

    sqlx::query!(
        "DELETE FROM upload_chunks WHERE user_id = $1 AND session_id = $2",
        session.user_id,
        session_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let updated = sqlx::query!(
        r#"
        UPDATE upload_sessions
        SET status = 'aborted'
        WHERE id = $1 AND user_id = $2 AND status = 'open'
        "#,
        session_id,
        session.user_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    if updated.rows_affected() != 1 {
        return Err(ApiError::upload_session_closed());
    }

    sqlx::query!(
        r#"
        UPDATE users
        SET reserved_bytes = reserved_bytes - $2,
            updated_at = now()
        WHERE id = $1
        "#,
        session.user_id,
        row.declared_size,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(AbortUploadResponse {
        session_id,
        status: "aborted".to_owned(),
    }))
}

/// 读取会话中所有对象键（当前键 + 被替换的旧键），供中止/过期清理复用。
pub(crate) async fn collect_session_object_keys(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    let chunks = sqlx::query!(
        r#"
        SELECT object_key, superseded_object_keys
        FROM upload_chunks
        WHERE user_id = $1 AND session_id = $2
        "#,
        user_id,
        session_id,
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut keys = Vec::new();
    for chunk in chunks {
        keys.push(chunk.object_key);
        keys.extend(chunk.superseded_object_keys);
    }
    Ok(keys)
}

/// 读取并锁定一个仍为 open 且已过期的会话（扫描任务使用）。
pub(crate) async fn load_expired_session_for_update(
    tx: &mut Transaction<'_, Postgres>,
    session_id: Uuid,
) -> Result<Option<SessionRow>, sqlx::Error> {
    sqlx::query_as!(
        SessionRow,
        r#"
        SELECT
            id, user_id, content_id, kind, node_id, parent_id,
            wrapped_key, encrypted_meta, name_token,
            version_id, version_meta, chunk_count, declared_size, reserved_bytes,
            status, expires_at, created_at, committed_at
        FROM upload_sessions
        WHERE id = $1 AND status = 'open' AND expires_at <= now()
        FOR UPDATE
        "#,
        session_id,
    )
    .fetch_optional(&mut **tx)
    .await
}

/// 下载接口复用的内容块行。
#[derive(Debug, sqlx::FromRow)]
pub struct ContentChunkRow {
    pub idx: i32,
    pub object_key: String,
    pub ciphertext_size: i64,
    pub sha256: Vec<u8>,
}

/// 读取某个 content 的全部块（带 user_id）。
pub async fn load_content_chunks(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    content_id: Uuid,
) -> Result<Vec<ContentChunkRow>, sqlx::Error> {
    sqlx::query_as!(
        ContentChunkRow,
        r#"
        SELECT idx, object_key, ciphertext_size, sha256
        FROM chunks
        WHERE user_id = $1 AND content_id = $2
        ORDER BY idx
        "#,
        user_id,
        content_id,
    )
    .fetch_all(&mut **tx)
    .await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn empty_file_is_one_chunk_of_frame_overhead() {
        let layout = ChunkLayout::parse(1, MIN_CHUNK_CIPHERTEXT_SIZE).expect("空文件布局");
        assert_eq!(layout.chunk_count, 1);
        assert_eq!(layout.total_size(), crypto_core::FRAME_OVERHEAD as i64);
        assert_eq!(layout.size(0), Some(crypto_core::FRAME_OVERHEAD as i64));
    }

    #[test]
    fn ten_mib_file_has_two_full_chunks_and_one_tail() {
        let plaintext: i64 = 10 * 1024 * 1024;
        let declared = plaintext + 3 * crypto_core::FRAME_OVERHEAD as i64;
        let layout = ChunkLayout::parse(3, declared).expect("10 MiB 布局");
        assert_eq!(layout.size(0), Some(CHUNK_CIPHERTEXT_SIZE));
        assert_eq!(layout.size(1), Some(CHUNK_CIPHERTEXT_SIZE));
        assert_eq!(
            layout.size(2),
            Some(2 * 1024 * 1024 + crypto_core::FRAME_OVERHEAD as i64)
        );
        assert_eq!(layout.total_size(), declared);
    }

    #[test]
    fn non_last_chunks_must_be_exactly_four_mib_plus_overhead() {
        // 声称 3 块，但最后一块过小 / 过大 / 声明总量不足都必须拒绝。
        for declared in [
            2 * CHUNK_CIPHERTEXT_SIZE + MIN_CHUNK_CIPHERTEXT_SIZE - 1,
            2 * CHUNK_CIPHERTEXT_SIZE + CHUNK_CIPHERTEXT_SIZE + 1,
            2 * CHUNK_CIPHERTEXT_SIZE,
        ] {
            assert!(
                ChunkLayout::parse(3, declared).is_err(),
                "非法布局必须被拒绝: {declared}"
            );
        }
        assert!(ChunkLayout::parse(3, 2 * CHUNK_CIPHERTEXT_SIZE + 1).is_err());
        // 最后一块可以比 4 MiB 小，也可以正好是 4 MiB。
        assert!(
            ChunkLayout::parse(3, 3 * CHUNK_CIPHERTEXT_SIZE - 1).is_ok(),
            "最后一块允许小于 4 MiB"
        );
        assert!(
            ChunkLayout::parse(3, 2 * CHUNK_CIPHERTEXT_SIZE + MIN_CHUNK_CIPHERTEXT_SIZE).is_ok()
        );
        assert!(ChunkLayout::parse(3, 3 * CHUNK_CIPHERTEXT_SIZE).is_ok());
    }

    #[test]
    fn single_chunk_must_be_within_frame_bounds() {
        assert!(ChunkLayout::parse(1, 0).is_err());
        assert!(ChunkLayout::parse(1, MIN_CHUNK_CIPHERTEXT_SIZE - 1).is_err());
        assert!(ChunkLayout::parse(1, CHUNK_CIPHERTEXT_SIZE + 1).is_err());
        assert!(ChunkLayout::parse(1, MIN_CHUNK_CIPHERTEXT_SIZE).is_ok());
        assert!(ChunkLayout::parse(1, CHUNK_CIPHERTEXT_SIZE).is_ok());
    }

    #[test]
    fn chunk_count_out_of_range_is_rejected() {
        assert!(ChunkLayout::parse(0, MIN_CHUNK_CIPHERTEXT_SIZE).is_err());
        assert!(ChunkLayout::parse(-1, MIN_CHUNK_CIPHERTEXT_SIZE).is_err());
        assert!(ChunkLayout::parse(MAX_CHUNK_COUNT + 1, CHUNK_CIPHERTEXT_SIZE).is_err());
    }
}
