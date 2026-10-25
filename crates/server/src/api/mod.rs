//! API 层：路由、请求 ID、日志、统一错误与静态文件托管。
//!
//! 安全边界：
//! - 请求 ID 只由服务端生成，客户端传入的 `X-Request-Id` 一律忽略。
//! - 日志只记录 method / path / status / latency / request_id，
//!   **不记录** 请求体、响应体、Cookie、Authorization、查询串或预签名 URL。
//! - 统一错误体 `{code, message, request_id}`；500 只返回通用信息。

pub mod body_limit;
pub mod csrf;
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
use axum::routing::{delete, get, patch, post};
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
    pub auth: Arc<crate::auth::AuthContext>,
}

impl AppState {
    pub fn new(config: Config, database: Database, storage: Storage) -> Self {
        let auth = Arc::new(crate::auth::AuthContext::new(
            config.auth.clone(),
            config.auth_rate_limit_per_minute,
            config.user_write_rate_limit_per_minute,
        ));
        Self {
            config: Arc::new(config),
            database,
            storage: Arc::new(storage),
            auth,
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
    // `/assets/` 只存放构建产物；缺失文件必须 404，不能回退到 index.html。
    let assets_service = ServeDir::new(static_dir.join("assets"));
    // 其余非 API 路径回退到 index.html（React Router 的单页应用语义）。
    let spa_service = ServeDir::new(&static_dir)
        .append_index_html_on_directories(true)
        .fallback(ServeFile::new(index));

    let api = Router::new()
        .route("/v1/auth/prelogin", post(crate::auth::routes::prelogin))
        .route(
            "/v1/auth/recovery-prelogin",
            post(crate::auth::routes::recovery_prelogin),
        )
        .route("/v1/auth/register", post(crate::auth::routes::register))
        .route(
            "/v1/auth/verify-email",
            post(crate::auth::routes::verify_email),
        )
        .route(
            "/v1/auth/resend-verification",
            post(crate::auth::routes::resend_verification),
        )
        .route("/v1/auth/login", post(crate::auth::routes::login))
        .route("/v1/auth/logout", post(crate::auth::routes::logout))
        .route("/v1/auth/me", get(crate::auth::routes::me))
        .route("/v1/auth/sessions", get(crate::auth::routes::list_sessions))
        .route(
            "/v1/auth/sessions/{session_id}",
            delete(crate::auth::routes::revoke_session),
        )
        .route(
            "/v1/auth/change-password",
            post(crate::auth::routes::change_password),
        )
        .route("/v1/auth/recover", post(crate::auth::routes::recover))
        .route(
            "/v1/auth/rotate-recovery-key",
            post(crate::auth::routes::rotate_recovery_key),
        )
        // 文件管理（第 7 组）
        .route(
            "/v1/nodes/{node_id}/children",
            get(crate::files::nodes::list_children),
        )
        .route("/v1/folders", post(crate::files::nodes::create_folder))
        .route(
            "/v1/nodes/{node_id}/rename",
            patch(crate::files::nodes::rename_node),
        )
        .route(
            "/v1/nodes/{node_id}/move",
            patch(crate::files::nodes::move_node),
        )
        .route(
            "/v1/nodes/{node_id}",
            delete(crate::files::trash::delete_node),
        )
        .route("/v1/trash", get(crate::files::trash::list_trash))
        .route(
            "/v1/trash/{node_id}/restore",
            post(crate::files::trash::restore_node),
        )
        .route(
            "/v1/trash/{node_id}",
            delete(crate::files::trash::permanently_delete_node),
        )
        .route(
            "/v1/nodes/{node_id}/versions",
            get(crate::files::versions::list_versions),
        )
        .route(
            "/v1/nodes/{node_id}/versions/restore",
            post(crate::files::versions::restore_version),
        )
        .route("/v1/me/usage", get(crate::files::usage::get_usage))
        // 文件传输（第 8 组）
        .route(
            "/v1/upload-sessions",
            post(crate::files::uploads::create_upload_session),
        )
        .route(
            "/v1/upload-sessions/{session_id}",
            get(crate::files::uploads::get_upload_session),
        )
        .route(
            "/v1/upload-sessions/{session_id}/chunks",
            get(crate::files::uploads::list_confirmed_chunks),
        )
        .route(
            "/v1/upload-sessions/{session_id}/chunks/presign",
            post(crate::files::uploads::presign_chunks),
        )
        .route(
            "/v1/upload-sessions/{session_id}/chunks/confirm",
            post(crate::files::uploads::confirm_chunks),
        )
        .route(
            "/v1/upload-sessions/{session_id}/chunks/{idx}/presign",
            post(crate::files::uploads::presign_chunk),
        )
        .route(
            "/v1/upload-sessions/{session_id}/chunks/{idx}/confirm",
            post(crate::files::uploads::confirm_chunk),
        )
        .route(
            "/v1/upload-sessions/{session_id}/commit",
            post(crate::files::uploads::commit_upload),
        )
        .route(
            "/v1/upload-sessions/{session_id}/abort",
            post(crate::files::uploads::abort_upload),
        )
        .route(
            "/v1/nodes/{node_id}/versions/{version_id}/download",
            get(crate::files::download::download_version),
        )
        .layer(from_fn(csrf::require_custom_header))
        .fallback(api_not_found)
        .method_not_allowed_fallback(api_method_not_allowed);

    let security_headers = security::SecurityHeaders::new(&state.config.storage.public_origin());

    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .nest("/api", api)
        .merge(extra)
        .nest_service("/assets", assets_service)
        // 未知的非 API 路径回退到前端 index.html（React Router 的单页应用语义）。
        .fallback_service(spa_service)
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
