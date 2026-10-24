//! 请求体上限。
//!
//! 在路由与提取器之前根据 `Content-Length` 直接拒绝超大请求（413），
//! 不读取、不解析请求体。没有 `Content-Length` 的分块请求由
//! `DefaultBodyLimit` 在提取阶段兜底。

use axum::extract::Request;
use axum::http::header::CONTENT_LENGTH;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::MAX_BODY_BYTES;
use super::error::ApiError;

/// 超过上限时立即返回 413。
pub async fn reject_oversized(request: Request, next: Next) -> Response {
    let declared_length = request
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    if declared_length.is_some_and(|length| length > MAX_BODY_BYTES as u64) {
        return ApiError::payload_too_large().into_response();
    }
    next.run(request).await
}
