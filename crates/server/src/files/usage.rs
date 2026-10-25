//! 存储用量（task 7.7）。

use axum::Json;
use axum::extract::State;
use serde::Serialize;
use utoipa::ToSchema;

use crate::AppState;
use crate::api::error::{ApiError, ErrorBody};
use crate::auth::AuthenticatedSession;

/// 已用 / 预留 / 总额（字节）。
#[derive(Debug, Serialize, ToSchema)]
pub struct UsageResponse {
    /// 所有保留版本（含回收站）引用 content 的密文字节数之和。
    pub used_bytes: i64,
    /// 进行中上传会话预留的字节数（第 8 组使用）。
    pub reserved_bytes: i64,
    /// 账号配额上限。
    pub quota_bytes: i64,
}

/// `GET /api/v1/me/usage`。
#[utoipa::path(
    get,
    path = "/api/v1/me/usage",
    tag = "files",
    responses(
        (status = 200, body = UsageResponse),
        (status = 401, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn get_usage(
    State(state): State<AppState>,
    session: AuthenticatedSession,
) -> Result<Json<UsageResponse>, ApiError> {
    let row = sqlx::query!(
        r#"
        SELECT used_bytes, reserved_bytes, quota_bytes
        FROM users
        WHERE id = $1
        "#,
        session.user_id,
    )
    .fetch_optional(state.database.pool())
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::unauthenticated)?;

    Ok(Json(UsageResponse {
        used_bytes: row.used_bytes,
        reserved_bytes: row.reserved_bytes,
        quota_bytes: row.quota_bytes,
    }))
}
