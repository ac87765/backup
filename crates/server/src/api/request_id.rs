//! 服务端生成的请求 ID。
//!
//! - 每个请求生成新的 UUIDv4，**不信任**客户端传入的 `X-Request-Id`。
//! - 通过响应头 `X-Request-Id` 返回，并写入错误体 `request_id` 字段。
//! - 通过 task-local 传递给 `IntoResponse` 与日志，避免每个 handler 手工传递。

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

tokio::task_local! {
    static CURRENT_REQUEST_ID: String;
}

/// 当前请求 ID（仅在同一请求的异步任务内可取）。
pub fn current() -> String {
    CURRENT_REQUEST_ID
        .try_with(Clone::clone)
        .unwrap_or_else(|_| "unknown".to_owned())
}

/// 请求 ID 中间件：生成 ID、写入请求扩展、作用域内执行下游、写响应头。
pub async fn assign(mut request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let header_value = HeaderValue::from_str(&request_id)
        .unwrap_or_else(|_| HeaderValue::from_static("invalid-request-id"));
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));

    let mut response = CURRENT_REQUEST_ID
        .scope(request_id, next.run(request))
        .await;
    response.headers_mut().insert("x-request-id", header_value);
    response
}

/// 供 handler/测试提取当前请求 ID。
#[derive(Debug, Clone)]
pub struct RequestId(pub String);
