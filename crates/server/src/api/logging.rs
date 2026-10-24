//! 请求日志。
//!
//! 只记录结构化、低敏感度的字段：method、path（不含查询串）、status、latency、request_id。
//! 明确禁止：请求体/响应体、Cookie、Authorization、查询串、预签名 URL（含签名）。

use std::time::Instant;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use super::request_id;

/// 请求日志中间件。
pub async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    // 只取 path：查询串可能包含令牌或预签名参数，一律不记录。
    let path = request.uri().path().to_owned();
    let request_id = request_id::current();
    let started = Instant::now();

    let response = next.run(request).await;

    let status = response.status().as_u16();
    let latency_ms = started.elapsed().as_millis() as u64;
    tracing::info!(
        request_id = %request_id,
        method = %method,
        path = %path,
        status,
        latency_ms,
        "http_request"
    );
    response
}
