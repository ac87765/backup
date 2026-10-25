//! 管理接口（add-admin-console D4）。
//!
//! 所有接口只对 active 管理员开放；非管理员/未登录一律 404。
//! 写操作与 `admin_audit_log` 在同一事务内提交；重复提交同一状态变更幂等，
//! 不重复写审计、不重复发邮件。

use std::time::Duration as StdDuration;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::PgConnection;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{AdminSession, action, insert_audit, record_action};
use crate::AppState;
use crate::accounts;
use crate::api::error::{ApiError, ErrorBody};
use crate::app_settings;
use crate::auth::ClientIp;
use crate::files::format_time;
use crate::files::uploads::collect_session_object_keys;
use crate::jobs::deletion::enqueue_object_deletions;
use crate::jobs::email::enqueue_email;
use crate::mail::purpose;

/// 就绪检查单项超时（与 `/readyz` 保持一致）。
const HEALTH_CHECK_TIMEOUT: StdDuration = StdDuration::from_secs(2);
const DEFAULT_PAGE_SIZE: i64 = 50;
const MAX_PAGE_SIZE: i64 = 100;
const MAX_REASON_CHARS: usize = 500;

// ---------------------------------------------------------------------------
// 请求 / 响应类型
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminOkResponse {
    pub ok: bool,
}

fn ok() -> Json<AdminOkResponse> {
    Json(AdminOkResponse { ok: true })
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminStatusCounts {
    pub pending: i64,
    pub active: i64,
    pub suspended: i64,
    pub rejected: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminQueueBacklog {
    pub email_outbox: i64,
    pub object_deletions: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminHealth {
    pub status: String,
    pub database: String,
    pub storage: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminOverview {
    pub pending_users: i64,
    pub status_counts: AdminStatusCounts,
    pub total_used_bytes: i64,
    pub total_reserved_bytes: i64,
    pub total_quota_bytes: i64,
    pub registrations_last_7_days: i64,
    pub queues: AdminQueueBacklog,
    pub health: AdminHealth,
    pub version: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserSummary {
    pub id: Uuid,
    pub email: String,
    pub status: String,
    pub is_admin: bool,
    pub created_at: String,
    pub email_verified_at: Option<String>,
    pub approved_at: Option<String>,
    pub last_active_at: Option<String>,
    pub used_bytes: i64,
    pub reserved_bytes: i64,
    pub quota_bytes: i64,
    /// 未删除的文件数（不含根文件夹）。
    pub file_count: i64,
    /// 未删除的文件夹数（不含根文件夹）。
    pub folder_count: i64,
    pub version_count: i64,
    pub active_session_count: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct UserSummaryRow {
    id: Uuid,
    email: String,
    status: String,
    is_admin: bool,
    created_at: OffsetDateTime,
    email_verified_at: Option<OffsetDateTime>,
    approved_at: Option<OffsetDateTime>,
    last_active_at: Option<OffsetDateTime>,
    used_bytes: i64,
    reserved_bytes: i64,
    quota_bytes: i64,
    file_count: i64,
    folder_count: i64,
    version_count: i64,
    active_session_count: i64,
}

impl UserSummaryRow {
    fn into_view(self) -> AdminUserSummary {
        AdminUserSummary {
            id: self.id,
            email: self.email,
            status: self.status,
            is_admin: self.is_admin,
            created_at: format_time(self.created_at),
            email_verified_at: self.email_verified_at.map(format_time),
            approved_at: self.approved_at.map(format_time),
            last_active_at: self.last_active_at.map(format_time),
            used_bytes: self.used_bytes,
            reserved_bytes: self.reserved_bytes,
            quota_bytes: self.quota_bytes,
            file_count: self.file_count,
            folder_count: self.folder_count,
            version_count: self.version_count,
            active_session_count: self.active_session_count,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserListResponse {
    pub items: Vec<AdminUserSummary>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminSessionSummary {
    pub id: Uuid,
    pub created_at: String,
    pub last_seen_at: String,
    pub ip: String,
    pub user_agent: Option<String>,
    pub revoked: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminAuditEntry {
    pub id: i64,
    pub admin_id: Uuid,
    pub action: String,
    pub target_user_id: Option<Uuid>,
    pub details: Value,
    pub ip: String,
    pub created_at: String,
}

#[derive(Debug, sqlx::FromRow)]
struct AuditRow {
    id: i64,
    admin_id: Uuid,
    action: String,
    target_user_id: Option<Uuid>,
    details: Value,
    ip: String,
    created_at: OffsetDateTime,
}

impl AuditRow {
    fn into_view(self) -> AdminAuditEntry {
        AdminAuditEntry {
            id: self.id,
            admin_id: self.admin_id,
            action: self.action,
            target_user_id: self.target_user_id,
            details: self.details,
            ip: self.ip,
            created_at: format_time(self.created_at),
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminUserDetailResponse {
    pub user: AdminUserSummary,
    pub sessions: Vec<AdminSessionSummary>,
    pub audit: Vec<AdminAuditEntry>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApproveUserRequest {
    /// 缺省时使用 app_settings.default_quota_bytes（再回退到配置）。
    pub quota_bytes: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReasonRequest {
    pub reason: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateQuotaRequest {
    pub quota_bytes: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateQuotaResponse {
    pub ok: bool,
    /// 新额度低于已用量时提示前端“该用户将无法继续上传”。
    pub below_used: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminUsersQuery {
    pub status: Option<String>,
    /// 邮箱子串（ILIKE，转义 `%`/`_`）。
    pub q: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AdminAuditQuery {
    pub cursor: Option<String>,
    pub target_user_id: Option<Uuid>,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminAuditResponse {
    pub items: Vec<AdminAuditEntry>,
    pub next_cursor: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AdminSettingsResponse {
    pub registration_mode: String,
    pub default_quota_bytes: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettingsRequest {
    pub registration_mode: String,
    pub default_quota_bytes: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct LockedUser {
    status: String,
    quota_bytes: i64,
    used_bytes: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct OverviewRow {
    pending: i64,
    active: i64,
    suspended: i64,
    rejected: i64,
    total_used: i64,
    total_reserved: i64,
    total_quota: i64,
    registrations: i64,
}

// ---------------------------------------------------------------------------
// 公共辅助
// ---------------------------------------------------------------------------

fn parse_body<T: DeserializeOwned>(payload: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    payload
        .map(|Json(value)| value)
        .map_err(|_| ApiError::invalid_request())
}

fn validate_quota(value: i64, max: i64) -> Result<(), ApiError> {
    if !(0..=max).contains(&value) {
        return Err(ApiError::invalid_request());
    }
    Ok(())
}

fn validate_reason(reason: &str) -> Result<String, ApiError> {
    let reason = reason.trim();
    if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARS {
        return Err(ApiError::invalid_request());
    }
    Ok(reason.to_owned())
}

fn page_size(limit: Option<i64>) -> Result<i64, ApiError> {
    match limit {
        None => Ok(DEFAULT_PAGE_SIZE),
        Some(value) if (1..=MAX_PAGE_SIZE).contains(&value) => Ok(value),
        Some(_) => Err(ApiError::invalid_request()),
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn user_cursor(created_at: OffsetDateTime, id: Uuid) -> String {
    format!("{}.{}", created_at.unix_timestamp_nanos(), id)
}

fn decode_user_cursor(cursor: Option<&str>) -> Result<Option<(OffsetDateTime, Uuid)>, ApiError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let (nanos, id) = cursor
        .split_once('.')
        .ok_or_else(ApiError::invalid_cursor)?;
    let nanos: i128 = nanos.parse().map_err(|_| ApiError::invalid_cursor())?;
    let created_at =
        OffsetDateTime::from_unix_timestamp_nanos(nanos).map_err(|_| ApiError::invalid_cursor())?;
    let id = Uuid::parse_str(id).map_err(|_| ApiError::invalid_cursor())?;
    Ok(Some((created_at, id)))
}

fn decode_audit_cursor(cursor: Option<&str>) -> Result<Option<i64>, ApiError> {
    match cursor {
        None => Ok(None),
        Some(value) => value
            .parse::<i64>()
            .ok()
            .filter(|value| *value > 0)
            .map(Some)
            .ok_or_else(ApiError::invalid_cursor),
    }
}

async fn lock_user(conn: &mut PgConnection, user_id: Uuid) -> Result<LockedUser, ApiError> {
    sqlx::query_as!(
        LockedUser,
        r#"
        SELECT status, quota_bytes, used_bytes
        FROM users
        WHERE id = $1
        FOR UPDATE
        "#,
        user_id,
    )
    .fetch_optional(conn)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)
}

async fn default_quota(conn: &mut PgConnection, state: &AppState) -> Result<i64, ApiError> {
    Ok(app_settings::default_quota(conn)
        .await
        .map_err(ApiError::internal)?
        .unwrap_or(state.config.default_quota_bytes)
        .clamp(0, state.config.max_quota_bytes))
}

async fn revoke_all_sessions(conn: &mut PgConnection, user_id: Uuid) -> Result<u64, ApiError> {
    let result = sqlx::query!(
        r#"
        UPDATE sessions
        SET revoked_at = now()
        WHERE user_id = $1 AND revoked_at IS NULL
        "#,
        user_id,
    )
    .execute(conn)
    .await
    .map_err(ApiError::internal)?;
    Ok(result.rows_affected())
}

/// 中止用户所有进行中的上传会话并释放预留配额（复用过期清理的对象删除队列逻辑）。
async fn abort_open_uploads(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> Result<u64, ApiError> {
    let sessions = sqlx::query!(
        r#"
        SELECT id, reserved_bytes
        FROM upload_sessions
        WHERE user_id = $1 AND status = 'open'
        ORDER BY id
        FOR UPDATE
        "#,
        user_id,
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(ApiError::internal)?;

    let mut aborted = 0_u64;
    for session in sessions {
        let keys = collect_session_object_keys(tx, user_id, session.id)
            .await
            .map_err(ApiError::internal)?;
        enqueue_object_deletions(tx, &keys)
            .await
            .map_err(ApiError::internal)?;
        sqlx::query!(
            "DELETE FROM upload_chunks WHERE user_id = $1 AND session_id = $2",
            user_id,
            session.id,
        )
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
        let updated = sqlx::query!(
            r#"
            UPDATE upload_sessions
            SET status = 'aborted'
            WHERE id = $1 AND user_id = $2 AND status = 'open'
            "#,
            session.id,
            user_id,
        )
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
        if updated.rows_affected() != 1 {
            continue;
        }
        sqlx::query!(
            r#"
            UPDATE users
            SET reserved_bytes = reserved_bytes - $2,
                updated_at = now()
            WHERE id = $1
            "#,
            user_id,
            session.reserved_bytes,
        )
        .execute(&mut **tx)
        .await
        .map_err(ApiError::internal)?;
        aborted += 1;
    }
    Ok(aborted)
}

fn log_action(action: &'static str, admin_id: Uuid, target_user_id: Option<Uuid>) {
    // 只记录 action / admin_id / target_user_id；邮箱与原因不写日志（design D7）。
    tracing::info!(
        action,
        admin_id = %admin_id,
        target_user_id = ?target_user_id,
        "管理操作"
    );
}

// ---------------------------------------------------------------------------
// 概览
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/v1/admin/overview",
    tag = "admin",
    responses(
        (status = 200, body = AdminOverview),
        (status = 404, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn overview(
    State(state): State<AppState>,
    _admin: AdminSession,
) -> Result<Json<AdminOverview>, ApiError> {
    let row = sqlx::query_as!(
        OverviewRow,
        r#"
        SELECT
            count(*) FILTER (WHERE status = 'pending') AS "pending!",
            count(*) FILTER (WHERE status = 'active') AS "active!",
            count(*) FILTER (WHERE status = 'suspended') AS "suspended!",
            count(*) FILTER (WHERE status = 'rejected') AS "rejected!",
            COALESCE(sum(used_bytes), 0)::bigint AS "total_used!",
            COALESCE(sum(reserved_bytes), 0)::bigint AS "total_reserved!",
            COALESCE(sum(quota_bytes), 0)::bigint AS "total_quota!",
            count(*) FILTER (WHERE created_at > now() - interval '7 days') AS "registrations!"
        FROM users
        "#,
    )
    .fetch_one(state.database.pool())
    .await
    .map_err(ApiError::internal)?;

    let queues = sqlx::query!(
        r#"
        SELECT
            (SELECT count(*) FROM email_outbox) AS "email!",
            (SELECT count(*) FROM object_deletions) AS "deletions!"
        "#,
    )
    .fetch_one(state.database.pool())
    .await
    .map_err(ApiError::internal)?;

    let health = overview_health(&state).await;
    Ok(Json(AdminOverview {
        pending_users: row.pending,
        status_counts: AdminStatusCounts {
            pending: row.pending,
            active: row.active,
            suspended: row.suspended,
            rejected: row.rejected,
        },
        total_used_bytes: row.total_used,
        total_reserved_bytes: row.total_reserved,
        total_quota_bytes: row.total_quota,
        registrations_last_7_days: row.registrations,
        queues: AdminQueueBacklog {
            email_outbox: queues.email,
            object_deletions: queues.deletions,
        },
        health,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    }))
}

async fn overview_health(state: &AppState) -> AdminHealth {
    let database = tokio::time::timeout(HEALTH_CHECK_TIMEOUT, state.database.ping()).await;
    let database = match database {
        Ok(Ok(())) => "ok",
        Ok(Err(_)) | Err(_) => "error",
    };
    let storage = tokio::time::timeout(HEALTH_CHECK_TIMEOUT, state.storage.probe()).await;
    let storage = match storage {
        Ok(Ok(())) => "ok",
        Ok(Err(_)) | Err(_) => "error",
    };
    let status = if database == "ok" && storage == "ok" {
        "ready"
    } else {
        "not_ready"
    };
    AdminHealth {
        status: status.to_owned(),
        database: database.to_owned(),
        storage: storage.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// 用户列表 / 详情
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/v1/admin/users",
    tag = "admin",
    params(
        ("status" = Option<String>, Query, description = "pending/active/suspended/rejected"),
        ("q" = Option<String>, Query, description = "邮箱子串"),
        ("cursor" = Option<String>, Query, description = "上一页的 next_cursor"),
        ("limit" = Option<i64>, Query, description = "页大小 1–100，默认 50")
    ),
    responses(
        (status = 200, body = AdminUserListResponse),
        (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn list_users(
    State(state): State<AppState>,
    _admin: AdminSession,
    Query(query): Query<AdminUsersQuery>,
) -> Result<Json<AdminUserListResponse>, ApiError> {
    if let Some(status) = query.status.as_deref()
        && !accounts::is_valid_status(status)
    {
        return Err(ApiError::invalid_request());
    }
    let limit = page_size(query.limit)?;
    let cursor = decode_user_cursor(query.cursor.as_deref())?;
    let (cursor_at, cursor_id) = match cursor {
        Some((created_at, id)) => (Some(created_at), Some(id)),
        None => (None, None),
    };
    let pattern = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("%{}%", escape_like(value)));

    let rows = sqlx::query_as!(
        UserSummaryRow,
        r#"
        SELECT
            u.id,
            u.email::text AS "email!",
            u.status,
            u.is_admin,
            u.created_at,
            u.email_verified_at,
            u.approved_at,
            activity.last_seen_at AS "last_active_at?",
            u.used_bytes,
            u.reserved_bytes,
            u.quota_bytes,
            (SELECT count(*) FROM nodes n
              WHERE n.user_id = u.id AND n.deleted_at IS NULL AND n.kind = 'file') AS "file_count!",
            (SELECT count(*) FROM nodes n
              WHERE n.user_id = u.id AND n.deleted_at IS NULL
                AND n.kind = 'folder' AND n.parent_id IS NOT NULL) AS "folder_count!",
            (SELECT count(*) FROM file_versions fv WHERE fv.user_id = u.id) AS "version_count!",
            (SELECT count(*) FROM sessions s
              WHERE s.user_id = u.id AND s.revoked_at IS NULL
                AND s.expires_at > now()
                AND s.last_seen_at > now() - make_interval(days => 7)) AS "active_session_count!"
        FROM users u
        LEFT JOIN LATERAL (
            SELECT max(s.last_seen_at) AS last_seen_at
            FROM sessions s
            WHERE s.user_id = u.id AND s.revoked_at IS NULL
        ) activity ON true
        WHERE ($1::text IS NULL OR u.status = $1)
          AND ($2::text IS NULL OR u.email::text ILIKE $2 ESCAPE '\')
          AND ($3::timestamptz IS NULL OR (u.created_at, u.id) < ($3, $4))
        ORDER BY u.created_at DESC, u.id DESC
        LIMIT $5
        "#,
        query.status,
        pattern,
        cursor_at,
        cursor_id,
        limit,
    )
    .fetch_all(state.database.pool())
    .await
    .map_err(ApiError::internal)?;

    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|row| user_cursor(row.created_at, row.id))
    } else {
        None
    };
    Ok(Json(AdminUserListResponse {
        items: rows.into_iter().map(UserSummaryRow::into_view).collect(),
        next_cursor,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/users/{user_id}",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "用户 ID")),
    responses(
        (status = 200, body = AdminUserDetailResponse),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn get_user(
    State(state): State<AppState>,
    _admin: AdminSession,
    Path(user_id): Path<Uuid>,
) -> Result<Json<AdminUserDetailResponse>, ApiError> {
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    let user = sqlx::query_as!(
        UserSummaryRow,
        r#"
        SELECT
            u.id,
            u.email::text AS "email!",
            u.status,
            u.is_admin,
            u.created_at,
            u.email_verified_at,
            u.approved_at,
            activity.last_seen_at AS "last_active_at?",
            u.used_bytes,
            u.reserved_bytes,
            u.quota_bytes,
            (SELECT count(*) FROM nodes n
              WHERE n.user_id = u.id AND n.deleted_at IS NULL AND n.kind = 'file') AS "file_count!",
            (SELECT count(*) FROM nodes n
              WHERE n.user_id = u.id AND n.deleted_at IS NULL
                AND n.kind = 'folder' AND n.parent_id IS NOT NULL) AS "folder_count!",
            (SELECT count(*) FROM file_versions fv WHERE fv.user_id = u.id) AS "version_count!",
            (SELECT count(*) FROM sessions s
              WHERE s.user_id = u.id AND s.revoked_at IS NULL
                AND s.expires_at > now()
                AND s.last_seen_at > now() - make_interval(days => 7)) AS "active_session_count!"
        FROM users u
        LEFT JOIN LATERAL (
            SELECT max(s.last_seen_at) AS last_seen_at
            FROM sessions s
            WHERE s.user_id = u.id AND s.revoked_at IS NULL
        ) activity ON true
        WHERE u.id = $1
        "#,
        user_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?
    .ok_or_else(ApiError::not_found)?;

    let sessions = sqlx::query!(
        r#"
        SELECT id, created_at, last_seen_at, ip, user_agent,
               (revoked_at IS NOT NULL) AS "revoked!"
        FROM sessions
        WHERE user_id = $1
        ORDER BY created_at DESC, id DESC
        LIMIT 20
        "#,
        user_id,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let audit = sqlx::query_as!(
        AuditRow,
        r#"
        SELECT id, admin_id, action, target_user_id, details, ip, created_at
        FROM admin_audit_log
        WHERE target_user_id = $1
        ORDER BY created_at DESC, id DESC
        LIMIT 20
        "#,
        user_id,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(AdminUserDetailResponse {
        user: user.into_view(),
        sessions: sessions
            .into_iter()
            .map(|session| AdminSessionSummary {
                id: session.id,
                created_at: format_time(session.created_at),
                last_seen_at: format_time(session.last_seen_at),
                ip: session.ip,
                user_agent: session.user_agent,
                revoked: session.revoked,
            })
            .collect(),
        audit: audit.into_iter().map(AuditRow::into_view).collect(),
    }))
}

// ---------------------------------------------------------------------------
// 用户状态操作
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/approve",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "用户 ID")),
    request_body = ApproveUserRequest,
    responses(
        (status = 200, body = AdminOkResponse),
        (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn approve_user(
    State(state): State<AppState>,
    admin: AdminSession,
    ClientIp(ip): ClientIp,
    Path(user_id): Path<Uuid>,
    payload: Result<Json<ApproveUserRequest>, JsonRejection>,
) -> Result<Json<AdminOkResponse>, ApiError> {
    let request = parse_body(payload)?;
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let user = lock_user(&mut tx, user_id).await?;
    if user.status == accounts::ACTIVE {
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(ok());
    }
    if !matches!(user.status.as_str(), accounts::PENDING | accounts::REJECTED) {
        return Err(ApiError::invalid_account_state());
    }
    let quota_bytes = match request.quota_bytes {
        Some(quota) => quota,
        None => default_quota(&mut tx, &state).await?,
    };
    validate_quota(quota_bytes, state.config.max_quota_bytes)?;

    sqlx::query!(
        r#"
        UPDATE users
        SET status = 'active',
            quota_bytes = $2,
            status_changed_at = now(),
            status_reason = NULL,
            approved_at = now(),
            approved_by = $3,
            updated_at = now()
        WHERE id = $1
        "#,
        user_id,
        quota_bytes,
        admin.0.user_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    insert_audit(
        &mut tx,
        admin.0.user_id,
        action::APPROVE,
        Some(user_id),
        &json!({
            "old_status": user.status,
            "new_status": accounts::ACTIVE,
            "old_quota_bytes": user.quota_bytes,
            "new_quota_bytes": quota_bytes,
        }),
        &ip.to_string(),
    )
    .await
    .map_err(ApiError::internal)?;

    enqueue_email(
        &mut tx,
        user_id,
        purpose::ACCOUNT_APPROVED,
        &json!({ "quota_bytes": quota_bytes }),
    )
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    record_action(action::APPROVE);
    log_action(action::APPROVE, admin.0.user_id, Some(user_id));
    Ok(ok())
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/reject",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "用户 ID")),
    request_body = ReasonRequest,
    responses(
        (status = 200, body = AdminOkResponse),
        (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn reject_user(
    State(state): State<AppState>,
    admin: AdminSession,
    ClientIp(ip): ClientIp,
    Path(user_id): Path<Uuid>,
    payload: Result<Json<ReasonRequest>, JsonRejection>,
) -> Result<Json<AdminOkResponse>, ApiError> {
    if user_id == admin.0.user_id {
        return Err(ApiError::cannot_modify_self());
    }
    let reason = validate_reason(&parse_body(payload)?.reason)?;
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let user = lock_user(&mut tx, user_id).await?;
    if user.status == accounts::REJECTED {
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(ok());
    }
    if user.status != accounts::PENDING {
        return Err(ApiError::invalid_account_state());
    }

    sqlx::query!(
        r#"
        UPDATE users
        SET status = 'rejected',
            status_reason = $2,
            status_changed_at = now(),
            updated_at = now()
        WHERE id = $1
        "#,
        user_id,
        reason,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    revoke_all_sessions(&mut tx, user_id).await?;

    insert_audit(
        &mut tx,
        admin.0.user_id,
        action::REJECT,
        Some(user_id),
        &json!({
            "old_status": user.status,
            "new_status": accounts::REJECTED,
            "reason": reason,
        }),
        &ip.to_string(),
    )
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    record_action(action::REJECT);
    log_action(action::REJECT, admin.0.user_id, Some(user_id));
    Ok(ok())
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/suspend",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "用户 ID")),
    request_body = ReasonRequest,
    responses(
        (status = 200, body = AdminOkResponse),
        (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn suspend_user(
    State(state): State<AppState>,
    admin: AdminSession,
    ClientIp(ip): ClientIp,
    Path(user_id): Path<Uuid>,
    payload: Result<Json<ReasonRequest>, JsonRejection>,
) -> Result<Json<AdminOkResponse>, ApiError> {
    if user_id == admin.0.user_id {
        return Err(ApiError::cannot_modify_self());
    }
    let reason = validate_reason(&parse_body(payload)?.reason)?;
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let user = lock_user(&mut tx, user_id).await?;
    if user.status == accounts::SUSPENDED {
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(ok());
    }
    if !matches!(user.status.as_str(), accounts::ACTIVE | accounts::PENDING) {
        return Err(ApiError::invalid_account_state());
    }

    let revoked = revoke_all_sessions(&mut tx, user_id).await?;
    let aborted = abort_open_uploads(&mut tx, user_id).await?;
    sqlx::query!(
        r#"
        UPDATE users
        SET status = 'suspended',
            status_reason = $2,
            status_changed_at = now(),
            updated_at = now()
        WHERE id = $1
        "#,
        user_id,
        reason,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    insert_audit(
        &mut tx,
        admin.0.user_id,
        action::SUSPEND,
        Some(user_id),
        &json!({
            "old_status": user.status,
            "new_status": accounts::SUSPENDED,
            "reason": reason,
            "revoked_sessions": revoked,
            "aborted_upload_sessions": aborted,
        }),
        &ip.to_string(),
    )
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    record_action(action::SUSPEND);
    log_action(action::SUSPEND, admin.0.user_id, Some(user_id));
    Ok(ok())
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/reactivate",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "用户 ID")),
    responses(
        (status = 200, body = AdminOkResponse),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn reactivate_user(
    State(state): State<AppState>,
    admin: AdminSession,
    ClientIp(ip): ClientIp,
    Path(user_id): Path<Uuid>,
) -> Result<Json<AdminOkResponse>, ApiError> {
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let user = lock_user(&mut tx, user_id).await?;
    if user.status == accounts::ACTIVE {
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(ok());
    }
    if !matches!(
        user.status.as_str(),
        accounts::SUSPENDED | accounts::REJECTED
    ) {
        return Err(ApiError::invalid_account_state());
    }

    sqlx::query!(
        r#"
        UPDATE users
        SET status = 'active',
            status_reason = NULL,
            status_changed_at = now(),
            approved_at = COALESCE(approved_at, now()),
            updated_at = now()
        WHERE id = $1
        "#,
        user_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    insert_audit(
        &mut tx,
        admin.0.user_id,
        action::REACTIVATE,
        Some(user_id),
        &json!({
            "old_status": user.status,
            "new_status": accounts::ACTIVE,
        }),
        &ip.to_string(),
    )
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    record_action(action::REACTIVATE);
    log_action(action::REACTIVATE, admin.0.user_id, Some(user_id));
    Ok(ok())
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/users/{user_id}/quota",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "用户 ID")),
    request_body = UpdateQuotaRequest,
    responses(
        (status = 200, body = UpdateQuotaResponse),
        (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn update_quota(
    State(state): State<AppState>,
    admin: AdminSession,
    ClientIp(ip): ClientIp,
    Path(user_id): Path<Uuid>,
    payload: Result<Json<UpdateQuotaRequest>, JsonRejection>,
) -> Result<Json<UpdateQuotaResponse>, ApiError> {
    let request = parse_body(payload)?;
    validate_quota(request.quota_bytes, state.config.max_quota_bytes)?;
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let user = lock_user(&mut tx, user_id).await?;
    let below_used = request.quota_bytes < user.used_bytes;
    if request.quota_bytes == user.quota_bytes {
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(UpdateQuotaResponse {
            ok: true,
            below_used,
        }));
    }

    sqlx::query!(
        r#"
        UPDATE users
        SET quota_bytes = $2, updated_at = now()
        WHERE id = $1
        "#,
        user_id,
        request.quota_bytes,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    insert_audit(
        &mut tx,
        admin.0.user_id,
        action::UPDATE_QUOTA,
        Some(user_id),
        &json!({
            "old_quota_bytes": user.quota_bytes,
            "new_quota_bytes": request.quota_bytes,
        }),
        &ip.to_string(),
    )
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    record_action(action::UPDATE_QUOTA);
    log_action(action::UPDATE_QUOTA, admin.0.user_id, Some(user_id));
    Ok(Json(UpdateQuotaResponse {
        ok: true,
        below_used,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/users/{user_id}/revoke-sessions",
    tag = "admin",
    params(("user_id" = Uuid, Path, description = "用户 ID")),
    responses(
        (status = 200, body = AdminOkResponse),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn revoke_user_sessions(
    State(state): State<AppState>,
    admin: AdminSession,
    ClientIp(ip): ClientIp,
    Path(user_id): Path<Uuid>,
) -> Result<Json<AdminOkResponse>, ApiError> {
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let exists = sqlx::query_scalar!("SELECT id FROM users WHERE id = $1", user_id,)
        .fetch_optional(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    if exists.is_none() {
        return Err(ApiError::not_found());
    }
    let revoked = revoke_all_sessions(&mut tx, user_id).await?;
    if revoked == 0 {
        // 幂等：没有需要撤销的会话时不写重复审计。
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(ok());
    }
    insert_audit(
        &mut tx,
        admin.0.user_id,
        action::REVOKE_SESSIONS,
        Some(user_id),
        &json!({ "revoked_sessions": revoked }),
        &ip.to_string(),
    )
    .await
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;
    record_action(action::REVOKE_SESSIONS);
    log_action(action::REVOKE_SESSIONS, admin.0.user_id, Some(user_id));
    Ok(ok())
}

// ---------------------------------------------------------------------------
// 设置
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/v1/admin/settings",
    tag = "admin",
    responses(
        (status = 200, body = AdminSettingsResponse),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn get_settings(
    State(state): State<AppState>,
    _admin: AdminSession,
) -> Result<Json<AdminSettingsResponse>, ApiError> {
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let registration_mode = app_settings::registration_mode(&mut tx)
        .await
        .map_err(ApiError::internal)?;
    let default_quota_bytes = default_quota(&mut tx, &state).await?;
    tx.commit().await.map_err(ApiError::internal)?;
    Ok(Json(AdminSettingsResponse {
        registration_mode,
        default_quota_bytes,
    }))
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/settings",
    tag = "admin",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, body = AdminSettingsResponse),
        (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn update_settings(
    State(state): State<AppState>,
    admin: AdminSession,
    ClientIp(ip): ClientIp,
    payload: Result<Json<UpdateSettingsRequest>, JsonRejection>,
) -> Result<Json<AdminSettingsResponse>, ApiError> {
    let request = parse_body(payload)?;
    if !app_settings::is_valid_registration_mode(&request.registration_mode) {
        return Err(ApiError::invalid_request());
    }
    validate_quota(request.default_quota_bytes, state.config.max_quota_bytes)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let old_mode = app_settings::registration_mode(&mut tx)
        .await
        .map_err(ApiError::internal)?;
    let old_quota = default_quota(&mut tx, &state).await?;
    if old_mode == request.registration_mode && old_quota == request.default_quota_bytes {
        tx.commit().await.map_err(ApiError::internal)?;
        return Ok(Json(AdminSettingsResponse {
            registration_mode: old_mode,
            default_quota_bytes: old_quota,
        }));
    }

    app_settings::set(
        &mut tx,
        app_settings::REGISTRATION_MODE_KEY,
        &json!(request.registration_mode),
        admin.0.user_id,
    )
    .await
    .map_err(ApiError::internal)?;
    app_settings::set(
        &mut tx,
        app_settings::DEFAULT_QUOTA_KEY,
        &json!(request.default_quota_bytes),
        admin.0.user_id,
    )
    .await
    .map_err(ApiError::internal)?;

    insert_audit(
        &mut tx,
        admin.0.user_id,
        action::UPDATE_SETTINGS,
        None,
        &json!({
            "old_registration_mode": old_mode,
            "new_registration_mode": request.registration_mode,
            "old_default_quota_bytes": old_quota,
            "new_default_quota_bytes": request.default_quota_bytes,
        }),
        &ip.to_string(),
    )
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    record_action(action::UPDATE_SETTINGS);
    log_action(action::UPDATE_SETTINGS, admin.0.user_id, None);
    Ok(Json(AdminSettingsResponse {
        registration_mode: request.registration_mode,
        default_quota_bytes: request.default_quota_bytes,
    }))
}

// ---------------------------------------------------------------------------
// 审计日志
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/v1/admin/audit",
    tag = "admin",
    params(
        ("cursor" = Option<String>, Query, description = "上一页的 next_cursor（审计 ID）"),
        ("target_user_id" = Option<Uuid>, Query, description = "只看某个用户"),
        ("limit" = Option<i64>, Query, description = "页大小 1–100，默认 50")
    ),
    responses(
        (status = 200, body = AdminAuditResponse),
        (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn list_audit(
    State(state): State<AppState>,
    _admin: AdminSession,
    Query(query): Query<AdminAuditQuery>,
) -> Result<Json<AdminAuditResponse>, ApiError> {
    let limit = page_size(query.limit)?;
    let cursor = decode_audit_cursor(query.cursor.as_deref())?;
    let rows = sqlx::query_as!(
        AuditRow,
        r#"
        SELECT id, admin_id, action, target_user_id, details, ip, created_at
        FROM admin_audit_log
        WHERE ($1::uuid IS NULL OR target_user_id = $1)
          AND ($2::bigint IS NULL OR id < $2)
        ORDER BY id DESC
        LIMIT $3
        "#,
        query.target_user_id,
        cursor,
        limit,
    )
    .fetch_all(state.database.pool())
    .await
    .map_err(ApiError::internal)?;
    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|row| row.id)
    } else {
        None
    };
    Ok(Json(AdminAuditResponse {
        items: rows.into_iter().map(AuditRow::into_view).collect(),
        next_cursor,
    }))
}
