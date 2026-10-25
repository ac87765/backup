//! 管理员身份、管理接口与审计（add-admin-console）。
//!
//! 安全边界：
//! - `is_admin` 只能由 [`cli`]（`server admin grant|revoke`）修改，HTTP 层没有写路径；
//! - 非管理员访问 `/api/v1/admin/*` 一律 404（与不存在的接口不可区分）；
//! - 所有管理写操作与审计记录在同一事务内提交；
//! - 日志只记录 action / admin_id / target_user_id，不记录邮箱或原因。

pub mod cli;
pub mod routes;

use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sqlx::{Postgres, Transaction};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::AppState;
use crate::accounts;
use crate::api::error::ApiError;
use crate::auth::AuthenticatedSession;

/// 审计动作名（同时作为 `backup_admin_actions_total{action}` 的标签）。
pub mod action {
    pub const APPROVE: &str = "approve";
    pub const REJECT: &str = "reject";
    pub const SUSPEND: &str = "suspend";
    pub const REACTIVATE: &str = "reactivate";
    pub const UPDATE_QUOTA: &str = "update_quota";
    pub const REVOKE_SESSIONS: &str = "revoke_sessions";
    pub const UPDATE_SETTINGS: &str = "update_settings";
    pub const GRANT_ADMIN: &str = "grant_admin";
    pub const REVOKE_ADMIN: &str = "revoke_admin";
}

/// 命令行操作的审计操作者：nil UUID 哨兵值。
pub const SYSTEM_ADMIN_ID: Uuid = Uuid::nil();

/// 命令行审计记录的来源 IP。
pub const CLI_SOURCE_IP: &str = "cli";

/// 会话创建后允许执行管理操作的最长时间（add-admin-console D3）。
pub const ADMIN_REAUTH_HOURS: i64 = 12;

/// 在调用方事务内追加一条审计记录（只追加，不更新、不删除）。
pub async fn insert_audit(
    tx: &mut Transaction<'_, Postgres>,
    admin_id: Uuid,
    action: &str,
    target_user_id: Option<Uuid>,
    details: &serde_json::Value,
    ip: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO admin_audit_log (admin_id, action, target_user_id, details, ip)
        VALUES ($1, $2, $3, $4, $5)
        "#,
        admin_id,
        action,
        target_user_id,
        details,
        ip,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 记录一次管理操作指标（不含用户维度）。
pub fn record_action(action: &'static str) {
    metrics::counter!("backup_admin_actions_total", "action" => action).increment(1);
}

pub use crate::accounts::format_quota;

/// 管理员会话提取器（add-admin-console D3）。
///
/// 顺序很重要：
/// 1. 未登录、非管理员、非 active → 404（与不存在的接口不可区分）；
/// 2. 管理员但会话创建超过 12 小时 → 403 `reauth_required`。
#[derive(Debug, Clone)]
pub struct AdminSession(pub AuthenticatedSession);

impl FromRequestParts<AppState> for AdminSession {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // 外层 admin::guard 已校验过时直接复用，避免每个请求重复查询会话。
        if let Some(admin) = parts.extensions.get::<Self>() {
            return Ok(admin.clone());
        }
        let session = AuthenticatedSession::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::not_found())?;
        if !session.is_admin || session.status != accounts::ACTIVE {
            return Err(ApiError::not_found());
        }
        let deadline = OffsetDateTime::now_utc() - Duration::hours(ADMIN_REAUTH_HOURS);
        if session.session_created_at < deadline {
            return Err(ApiError::reauth_required());
        }
        Ok(Self(session))
    }
}

/// 管理路由的最外层守卫。
///
/// 它必须位于 CSRF 中间件之外：未登录/非管理员请求要得到 404，而不是
/// `csrf_rejected`（add-admin-console D3：与不存在的接口不可区分）。
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();
    match AdminSession::from_request_parts(&mut parts, &state).await {
        Ok(admin) => {
            parts.extensions.insert(admin);
            next.run(Request::from_parts(parts, body)).await
        }
        Err(error) => error.into_response(),
    }
}
