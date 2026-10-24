//! API 层：路由、请求 ID、日志、统一错误与静态文件托管。
//!
//! 安全边界：
//! - 请求 ID 只由服务端生成，客户端传入的 `X-Request-Id` 一律忽略。
//! - 日志只记录 method / path / status / latency / request_id，
//!   **不记录** 请求体、响应体、Cookie、Authorization、查询串或预签名 URL。
//! - 统一错误体 `{code, message, request_id}`；500 只返回通用信息。

pub mod body_limit;
pub mod error;
pub mod health;
pub mod logging;
pub mod request_id;
pub mod security;
pub mod static_files;

use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware::from_fn;
use axum::routing::get;
use tower_http::services::{ServeDir, ServeFile};

use crate::{Config, Database, Storage, metrics};

/// API 请求体上限（1 MiB 量级）。文件内容不经过服务端，因此 JSON 请求体很小。
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// 应用共享状态。
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub database: Database,
    pub storage: Arc<Storage>,
}

impl AppState {
    pub fn new(config: Config, database: Database, storage: Storage) -> Self {
        Self {
            config: Arc::new(config),
            database,
            storage: Arc::new(storage),
        }
    }
}

/// 构建生产路由。
pub fn build_router(state: AppState) -> Router {
    build_router_with(state, Router::new())
}

/// 构建路由，并允许测试注入额外路由（例如慢请求、内部错误探针）。
///
/// 注入的路由同样经过完整的中间件链；生产入口只调用 [`build_router`]。
pub fn build_router_with(state: AppState, extra: Router<AppState>) -> Router {
    let static_dir = state.config.static_dir.clone();
    let index = static_dir.join("index.html");
    let static_service = ServeDir::new(&static_dir)
        .append_index_html_on_directories(true)
        .fallback(ServeFile::new(index));

    let api = Router::new()
        .fallback(api_not_found)
        .method_not_allowed_fallback(api_method_not_allowed);

    let security_headers = security::SecurityHeaders::new(&state.config.storage.public_origin());

    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .nest("/api", api)
        .merge(extra)
        // 未知的非 API 路径回退到前端 index.html（React Router 的单页应用语义）。
        .fallback_service(static_service)
        .with_state(state)
        .layer(from_fn(static_files::add_static_headers))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(from_fn(body_limit::reject_oversized))
        .layer(from_fn(metrics::track_http))
        .layer(from_fn(logging::log_request))
        .layer(from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let security_headers = security_headers.clone();
                async move { security::apply(security_headers, request, next).await }
            },
        ))
        .layer(from_fn(request_id::assign))
}

async fn api_not_found() -> error::ApiError {
    error::ApiError::not_found()
}

async fn api_method_not_allowed() -> error::ApiError {
    error::ApiError::method_not_allowed()
}
