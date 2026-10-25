//! 版本下载清单（task 8.6）。
//!
//! 服务端只返回块清单与短时预签名 GET URL；密文由浏览器直接向对象存储读取。
//! 他人资源与不存在一样返回 404（访问用户数据的查询都带 `user_id`）。
//! 预签名 URL 含签名，绝不能写入日志；[`PresignedUrl`] 的 Debug 已脱敏。

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::uploads::load_content_chunks;
use super::{format_time, validate_node_id, validate_version_id};
use crate::AppState;
use crate::api::error::{ApiError, ErrorBody};
use crate::auth::{AuthenticatedSession, encode_hex};
use crate::storage::MAX_PRESIGN_TTL;

/// 下载清单中的一块。
#[derive(Debug, Serialize, ToSchema)]
pub struct DownloadChunk {
    pub idx: i32,
    /// 密文字节数（解密后应等于 4 MiB 或更小）。
    pub ciphertext_size: i64,
    /// 密文 SHA-256（hex），客户端逐块校验。
    pub sha256: String,
    pub method: String,
    /// 预签名 GET URL（≤ 15 分钟，含签名，禁止日志）。
    pub url: String,
}

/// 下载清单。客户端 MUST 先解密 `encrypted_meta` 并校验其中的 content_id
/// 等于本响应的 `content_id`，再逐块解密（client-encryption spec）。
#[derive(Debug, Serialize, ToSchema)]
pub struct DownloadManifest {
    pub node_id: Uuid,
    pub version_id: Uuid,
    pub content_id: Uuid,
    /// 加密版本元数据（hex）。
    pub encrypted_meta: String,
    /// 该 content 的密文字节总数。
    pub ciphertext_size: i64,
    pub chunk_count: i32,
    /// 所有 URL 的过期时间（同一批次生成，≤ 15 分钟）。
    pub expires_at: String,
    pub chunks: Vec<DownloadChunk>,
}

/// `GET /api/v1/nodes/{node_id}/versions/{version_id}/download`。
#[utoipa::path(
    get,
    path = "/api/v1/nodes/{node_id}/versions/{version_id}/download",
    tag = "uploads",
    params(
        ("node_id" = Uuid, Path, description = "文件节点 ID"),
        ("version_id" = Uuid, Path, description = "版本 ID")
    ),
    responses(
        (status = 200, body = DownloadManifest),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn download_version(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path((node_id, version_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<DownloadManifest>, ApiError> {
    let node_id = validate_node_id(node_id)?;
    let version_id = validate_version_id(version_id)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    // 关键：同时限定 version、node 与 user_id；跨用户与不存在都落到 404。
    let version = sqlx::query!(
        r#"
        SELECT fv.content_id, fv.encrypted_meta, c.ciphertext_size, c.chunk_count
        FROM file_versions fv
        JOIN contents c
            ON c.id = fv.content_id AND c.user_id = fv.user_id
        WHERE fv.id = $1
          AND fv.node_id = $2
          AND fv.user_id = $3
        "#,
        version_id,
        node_id,
        session.user_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;

    let chunks = load_content_chunks(&mut tx, session.user_id, version.content_id)
        .await
        .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;

    // 数据完整性自检：块序号连续、数量与大小与 content 记录一致。
    if chunks.len() != usize::try_from(version.chunk_count).unwrap_or(0) {
        return Err(ApiError::internal("content 的块数量与记录不一致"));
    }
    let mut total: i64 = 0;
    for (expected_idx, chunk) in chunks.iter().enumerate() {
        if chunk.idx != expected_idx as i32 {
            return Err(ApiError::internal("content 的块序号不连续"));
        }
        total = total
            .checked_add(chunk.ciphertext_size)
            .ok_or_else(|| ApiError::internal("块大小求和溢出"))?;
    }
    if total != version.ciphertext_size {
        return Err(ApiError::internal("content 的块大小之和与记录不一致"));
    }

    let mut items = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let presigned = state
            .storage
            .presign_read(&chunk.object_key, MAX_PRESIGN_TTL)
            .await
            .map_err(ApiError::internal)?;
        items.push(DownloadChunk {
            idx: chunk.idx,
            ciphertext_size: chunk.ciphertext_size,
            sha256: encode_hex(&chunk.sha256),
            method: presigned.method().to_owned(),
            url: presigned.url().to_owned(),
        });
    }

    Ok(Json(DownloadManifest {
        node_id,
        version_id,
        content_id: version.content_id,
        encrypted_meta: encode_hex(&version.encrypted_meta),
        ciphertext_size: version.ciphertext_size,
        chunk_count: version.chunk_count,
        expires_at: format_time(OffsetDateTime::now_utc() + time::Duration::minutes(15)),
        chunks: items,
    }))
}
