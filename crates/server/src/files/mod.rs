//! 文件管理（第 7 组 tasks 7.1–7.7）。
//!
//! 服务端只保存结构、密文与生命周期信息：客户端生成节点/内容/版本 ID 与
//! 名称令牌，服务端用 crypto-core 的 ID 类型校验格式，并用部分唯一索引
//! 保证同一目录内不重名（design D5/D6/D9）。
//!
//! 安全边界：
//! - 所有请求路径上的查询都带 `user_id` 条件；访问他人资源与不存在一样返回 404。
//! - 日志不记录加密元数据、包裹密钥或名称令牌。
//! - 所有改变状态的接口都在事务内完成，可安全重试。

pub mod download;
pub mod nodes;
pub mod trash;
pub mod uploads;
pub mod usage;
pub mod versions;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::auth;

/// 列目录默认页大小。
pub const DEFAULT_PAGE_SIZE: i64 = 200;
/// 列目录最大页大小。
pub const MAX_PAGE_SIZE: i64 = 1000;
/// 回收站/版本列表的硬上限（这些列表没有分页需求，但仍限制单次返回量）。
pub const MAX_LIST_SIZE: i64 = 500;

/// 当前节点的版本摘要（当前版本与历史版本共用）。
#[derive(Debug, Clone, serde::Serialize, ToSchema)]
pub struct VersionSummary {
    pub id: Uuid,
    pub content_id: Uuid,
    /// 版本元数据密文（hex）。
    pub encrypted_meta: String,
    /// 该版本所引用 content 的密文字节数。
    pub ciphertext_size: i64,
    pub chunk_count: i32,
    pub is_current: bool,
    pub created_at: String,
}

/// 节点摘要。所有二进制字段都用 hex 传输。
#[derive(Debug, Clone, serde::Serialize, ToSchema)]
pub struct NodeSummary {
    pub id: Uuid,
    pub kind: String,
    pub parent_id: Option<Uuid>,
    pub wrapped_key: Option<String>,
    pub encrypted_meta: Option<String>,
    pub name_token: Option<String>,
    pub row_version: i32,
    pub current_version: Option<VersionSummary>,
    pub created_at: String,
    pub updated_at: String,
    /// 回收站中的节点才非空。
    pub deleted_at: Option<String>,
    pub trash_batch_id: Option<Uuid>,
}

/// 列目录/回收站/节点响应共用的行。
#[derive(Debug, sqlx::FromRow)]
pub struct NodeRow {
    pub id: Uuid,
    pub kind: String,
    pub parent_id: Option<Uuid>,
    pub wrapped_key: Option<Vec<u8>>,
    pub encrypted_meta: Option<Vec<u8>>,
    pub name_token: Option<Vec<u8>>,
    pub row_version: i32,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub deleted_at: Option<OffsetDateTime>,
    pub trash_batch_id: Option<Uuid>,
    pub trashed_from_parent_id: Option<Uuid>,
    pub current_version_id: Option<Uuid>,
    pub content_id: Option<Uuid>,
    pub version_meta: Option<Vec<u8>>,
    pub ciphertext_size: Option<i64>,
    pub chunk_count: Option<i32>,
    pub version_created_at: Option<OffsetDateTime>,
}

impl NodeRow {
    /// 根节点 = 未删除且没有父节点。回收站顶层项在原父节点被永久删除后
    /// `parent_id` 也会变成 NULL，但它是已删除节点，不是根节点。
    pub fn is_root(&self) -> bool {
        self.parent_id.is_none() && self.deleted_at.is_none()
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    pub fn summary(&self) -> NodeSummary {
        NodeSummary {
            id: self.id,
            kind: self.kind.clone(),
            parent_id: self.parent_id,
            wrapped_key: self.wrapped_key.as_deref().map(auth::encode_hex),
            encrypted_meta: self.encrypted_meta.as_deref().map(auth::encode_hex),
            name_token: self.name_token.as_deref().map(auth::encode_hex),
            row_version: self.row_version,
            current_version: self.current_version(),
            created_at: format_time(self.created_at),
            updated_at: format_time(self.updated_at),
            deleted_at: self.deleted_at.map(format_time),
            trash_batch_id: self.trash_batch_id,
        }
    }

    pub fn current_version(&self) -> Option<VersionSummary> {
        let (Some(id), Some(content_id), Some(meta), Some(size), Some(chunks)) = (
            self.current_version_id,
            self.content_id,
            self.version_meta.as_ref(),
            self.ciphertext_size,
            self.chunk_count,
        ) else {
            return None;
        };
        Some(VersionSummary {
            id,
            content_id,
            encrypted_meta: auth::encode_hex(meta),
            ciphertext_size: size,
            chunk_count: chunks,
            is_current: true,
            created_at: self.version_created_at.map(format_time).unwrap_or_default(),
        })
    }
}

/// 节点查询（含当前版本）。
///
/// 所有字段都来自同一 user_id 条件之下；调用方必须传入已认证会话的 user_id。
pub async fn load_node(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    node_id: Uuid,
) -> Result<Option<NodeRow>, sqlx::Error> {
    sqlx::query_as!(
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
        WHERE n.id = $1 AND n.user_id = $2
        "#,
        node_id,
        user_id,
    )
    .fetch_optional(&mut **tx)
    .await
}

/// 用户行锁：同一用户的所有结构变更（移动/删除/恢复）串行化，
/// 从而避免并发移动共同制造环（task 7.3）。
///
/// 使用 `FOR NO KEY UPDATE`：彼此之间仍互斥（配额与结构变更照常串行），
/// 但不阻塞外键检查所需的 `KEY SHARE`。否则 presign（持会话行锁 → 插入
/// upload_chunks 需要用户行 `KEY SHARE`）与 abort/commit（持用户行锁 → 等会话行锁）
/// 会互相等待而死锁。
pub async fn lock_user(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> Result<(), ApiError> {
    let locked = sqlx::query_scalar!(
        "SELECT id FROM users WHERE id = $1 FOR NO KEY UPDATE",
        user_id,
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    if locked.is_none() {
        return Err(ApiError::unauthenticated());
    }
    Ok(())
}

/// 校验客户端生成的节点 ID：必须是 UUIDv4 且非 nil（crypto-core 契约）。
pub fn validate_node_id(id: Uuid) -> Result<Uuid, ApiError> {
    crypto_core::NodeId::parse(&id.to_string())
        .map(|_| id)
        .map_err(|_| ApiError::invalid_request())
}

/// 校验客户端生成的版本 ID。
pub fn validate_version_id(id: Uuid) -> Result<Uuid, ApiError> {
    crypto_core::VersionId::parse(&id.to_string())
        .map(|_| id)
        .map_err(|_| ApiError::invalid_request())
}

/// 校验客户端生成的内容 ID（UUIDv4 且非 nil，见 crypto-core 契约）。
pub fn validate_content_id(id: Uuid) -> Result<Uuid, ApiError> {
    crypto_core::ContentId::parse(&id.to_string())
        .map(|_| id)
        .map_err(|_| ApiError::invalid_request())
}

/// 包裹的节点密钥：版本字节 + AEAD 帧，长度由 crypto-core 常量决定。
pub fn decode_wrapped_key(value: &str) -> Result<Vec<u8>, ApiError> {
    auth::decode_wrapped_material(value)
}

/// 加密元数据：只校验格式（版本字节 + 最小 AEAD 帧长度）。
pub fn decode_encrypted_meta(value: &str) -> Result<Vec<u8>, ApiError> {
    let bytes = auth::decode_hex_vec(value).ok_or_else(ApiError::invalid_request)?;
    if bytes.len() < crypto_core::FRAME_OVERHEAD + 1
        || bytes.first() != Some(&crypto_core::CIPHERTEXT_FORMAT_VERSION)
    {
        return Err(ApiError::invalid_request());
    }
    Ok(bytes)
}

/// 名称令牌：HMAC-SHA256 输出 32 字节。
pub fn decode_name_token(value: &str) -> Result<Vec<u8>, ApiError> {
    auth::decode_hex_array::<32>(value)
        .map(|bytes| bytes.to_vec())
        .ok_or_else(ApiError::invalid_request)
}

/// 分页游标只编码排序键（节点 ID）；查询仍然强制 `user_id` 条件。
pub fn decode_cursor(cursor: Option<&str>) -> Result<Option<Uuid>, ApiError> {
    match cursor {
        None => Ok(None),
        Some(value) => Uuid::parse_str(value)
            .map(Some)
            .map_err(|_| ApiError::invalid_cursor()),
    }
}

/// 列目录页大小。
pub fn page_size(limit: Option<i64>) -> Result<i64, ApiError> {
    match limit {
        None => Ok(DEFAULT_PAGE_SIZE),
        Some(value) if (1..=MAX_PAGE_SIZE).contains(&value) => Ok(value),
        Some(_) => Err(ApiError::invalid_request()),
    }
}

/// 版本/回收站列表大小。
pub fn list_size(limit: Option<i64>) -> Result<i64, ApiError> {
    match limit {
        None => Ok(DEFAULT_PAGE_SIZE),
        Some(value) if (1..=MAX_LIST_SIZE).contains(&value) => Ok(value),
        Some(_) => Err(ApiError::invalid_request()),
    }
}

/// 常量时间比较（名称令牌/包裹材料的幂等比较）。
pub fn bytes_equal(left: &[u8], right: &[u8]) -> bool {
    auth::constant_time_eq(left, right)
}

/// RFC 3339 时间字符串；格式化失败时退回 Display（不会 panic）。
pub fn format_time(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).unwrap_or_else(|_| value.to_string())
}

/// 是否为部分唯一索引冲突（同一目录内重名）。
pub fn is_name_conflict(error: &sqlx::Error) -> bool {
    is_unique_violation(error, "nodes_name_unique_idx")
}

/// 是否为指定的唯一约束冲突。
pub fn is_unique_violation(error: &sqlx::Error, constraint: &str) -> bool {
    connection_error(error).is_some_and(|db| {
        db.code().as_deref() == Some("23505") && db.constraint() == Some(constraint)
    })
}

fn connection_error(error: &sqlx::Error) -> Option<&dyn sqlx::error::DatabaseError> {
    match error {
        sqlx::Error::Database(database) => Some(database.as_ref()),
        _ => None,
    }
}
