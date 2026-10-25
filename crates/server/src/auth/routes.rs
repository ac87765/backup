//! 用户账号 HTTP 接口（tasks 6.2–6.7）。
//!
//! 所有会改变状态的接口都由 `api::csrf` 统一要求
//! `X-Requested-With: backup-web`；邮箱验证令牌只通过 POST 请求体接收，
//! 绝不放进路径（请求日志会记录路径）。邮件里的链接把令牌放在 URL 片段
//! `#token=` 中，前端读取后再 POST。

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{
    AuthenticatedSession, ClientIp, EMAIL_PURPOSE_VERIFY, LOCKOUT_MINUTES, MAX_FAILED_LOGINS,
    RESEND_THROTTLE_MINUTES, SESSION_ABSOLUTE_DAYS, SESSION_COOKIE_NAME, SESSION_IDLE_DAYS,
    constant_time_eq, decode_kdf_salt, decode_secret_key, decode_wrapped_material, encode_hex,
    generate_session_token, hash_token, normalize_email,
};
use crate::AppState;
use crate::api::error::{ApiError, ErrorBody};
use crate::jobs::email::enqueue_email;
use crate::mail::purpose;

/// KDF 参数（crypto-core 持久化 JSON 契约的 HTTP 表示）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct KdfParams {
    /// 算法标识；当前唯一允许 `argon2id`。
    pub algorithm: String,
    /// 内存代价（KiB）。
    pub m_cost_kib: u32,
    /// 迭代次数。
    pub t_cost: u32,
    /// 并行度。
    pub p_cost: u32,
}

impl KdfParams {
    /// 用 crypto-core 校验算法与范围，并返回核心类型。
    fn validate(&self) -> Result<crypto_core::KdfParams, ApiError> {
        if self.algorithm != "argon2id" {
            return Err(ApiError::invalid_kdf_params());
        }
        let params = crypto_core::KdfParams {
            algorithm: crypto_core::KdfAlgorithm::Argon2id,
            m_cost_kib: self.m_cost_kib,
            t_cost: self.t_cost,
            p_cost: self.p_cost,
        };
        params
            .validate()
            .map_err(|_| ApiError::invalid_kdf_params())?;
        Ok(params)
    }

    fn from_core(params: &crypto_core::KdfParams) -> Self {
        Self {
            algorithm: match params.algorithm {
                crypto_core::KdfAlgorithm::Argon2id => "argon2id".to_owned(),
            },
            m_cost_kib: params.m_cost_kib,
            t_cost: params.t_cost,
            p_cost: params.p_cost,
        }
    }

    fn from_stored(value: &serde_json::Value) -> Result<Self, ApiError> {
        let params: Self = serde_json::from_value(value.clone())
            .map_err(|_| ApiError::internal("users.kdf_params 不符合 KdfParams 契约"))?;
        params.validate()?;
        Ok(params)
    }
}

/// `POST /api/v1/auth/prelogin`。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PreloginRequest {
    pub email: String,
}

/// prelogin 响应；不存在邮箱也返回同样的结构。
#[derive(Debug, Serialize, ToSchema)]
pub struct PreloginResponse {
    pub kdf_params: KdfParams,
    pub kdf_salt: String,
}

/// 恢复重置前置查询：只返回恢复包裹后的 MK（密文）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPreloginRequest {
    pub email: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RecoveryPreloginResponse {
    pub recovery_wrapped_mk: String,
}

/// 注册请求：只包含客户端派生的材料，绝不包含密码。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterRequest {
    pub email: String,
    pub kdf_params: KdfParams,
    pub kdf_salt: String,
    pub auth_key: String,
    pub wrapped_mk: String,
    pub recovery_wrapped_mk: String,
    pub recovery_auth_key: String,
}

/// 通用成功响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct OkResponse {
    pub ok: bool,
}

fn ok() -> Json<OkResponse> {
    Json(OkResponse { ok: true })
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub email: String,
    pub auth_key: String,
}

/// 登录成功响应：客户端需要 KDF 参数、盐与包裹后的 MK 来完成本地解锁。
#[derive(Debug, Serialize, ToSchema)]
pub struct LoginResponse {
    pub user_id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub kdf_params: KdfParams,
    pub kdf_salt: String,
    pub wrapped_mk: String,
    /// 当前用户的根文件夹 ID（非密钥；供文件浏览器导航使用）。
    pub root_node_id: Uuid,
}

/// 刷新页面后解锁所需的数据。
#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub user_id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub kdf_params: KdfParams,
    pub kdf_salt: String,
    pub wrapped_mk: String,
    /// 当前用户的根文件夹 ID（非密钥；供文件浏览器导航使用）。
    pub root_node_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyEmailRequest {
    /// 前端从邮件链接的 `#token=` 片段读取后提交，绝不放在 URL 路径里。
    pub token: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResendVerificationRequest {
    pub email: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangePasswordRequest {
    pub old_auth_key: String,
    pub new_auth_key: String,
    pub new_kdf_params: KdfParams,
    pub new_kdf_salt: String,
    pub new_wrapped_mk: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoverRequest {
    pub email: String,
    pub recovery_auth_key: String,
    pub new_auth_key: String,
    pub new_kdf_params: KdfParams,
    pub new_kdf_salt: String,
    pub new_wrapped_mk: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RotateRecoveryKeyRequest {
    /// 当前密码派生出的认证密钥，用于证明知道当前密码。
    pub current_auth_key: String,
    pub new_recovery_wrapped_mk: String,
    pub new_recovery_auth_key: String,
}

/// 会话列表项。绝不返回令牌或令牌哈希。
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionSummary {
    pub id: Uuid,
    pub created_at: String,
    pub last_seen_at: String,
    pub expires_at: String,
    pub ip: String,
    pub user_agent: Option<String>,
    pub current: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SessionsResponse {
    pub sessions: Vec<SessionSummary>,
}

// ---------------------------------------------------------------------------
// 公共辅助
// ---------------------------------------------------------------------------

fn parse_body<T: DeserializeOwned>(payload: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    payload
        .map(|Json(value)| value)
        .map_err(|_| ApiError::invalid_request())
}

fn format_time(value: OffsetDateTime) -> String {
    value.format(&Rfc3339).unwrap_or_else(|_| value.to_string())
}

fn user_agent(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::USER_AGENT)?.to_str().ok()?.trim();
    if value.is_empty() {
        return None;
    }
    // 限制长度，避免把超大头部写入数据库。
    Some(value.chars().take(512).collect())
}

fn session_cookie_header(token: &str) -> Result<HeaderMap, ApiError> {
    let max_age = SESSION_ABSOLUTE_DAYS * 24 * 60 * 60;
    let value = format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; Max-Age={max_age}; HttpOnly; Secure; SameSite=Strict"
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&value).map_err(ApiError::internal)?,
    );
    Ok(headers)
}

fn clear_session_cookie_header() -> Result<HeaderMap, ApiError> {
    let value =
        format!("{SESSION_COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Strict");
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&value).map_err(ApiError::internal)?,
    );
    Ok(headers)
}

async fn record_failed_login(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
) -> Result<(), ApiError> {
    sqlx::query!(
        r#"
        UPDATE users
        SET failed_logins = failed_logins + 1,
            locked_until = CASE
                WHEN failed_logins + 1 >= $2 THEN now() + make_interval(mins => $3)
                ELSE locked_until
            END,
            updated_at = now()
        WHERE id = $1
        "#,
        user_id,
        MAX_FAILED_LOGINS,
        LOCKOUT_MINUTES,
    )
    .execute(&mut **tx)
    .await
    .map_err(ApiError::internal)?;
    Ok(())
}

fn lock_retry_after(locked_until: OffsetDateTime) -> u64 {
    let now = OffsetDateTime::now_utc();
    let remaining = locked_until - now;
    remaining.whole_seconds().max(1) as u64
}

// ---------------------------------------------------------------------------
// prelogin
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/v1/auth/prelogin",
    tag = "auth",
    request_body = PreloginRequest,
    responses(
        (status = 200, body = PreloginResponse),
        (status = 400, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn prelogin(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    payload: Result<Json<PreloginRequest>, JsonRejection>,
) -> Result<Json<PreloginResponse>, ApiError> {
    state.auth.check_ip(ip)?;
    let request = parse_body(payload)?;
    let email = normalize_email(&request.email).ok_or_else(ApiError::invalid_request)?;

    let row = sqlx::query!(
        "SELECT kdf_params, kdf_salt FROM users WHERE email = $1::text::citext",
        email,
    )
    .fetch_optional(state.database.pool())
    .await
    .map_err(ApiError::internal)?;

    let response = match row {
        Some(row) => PreloginResponse {
            kdf_params: KdfParams::from_stored(&row.kdf_params)?,
            kdf_salt: encode_hex(&row.kdf_salt),
        },
        None => PreloginResponse {
            kdf_params: KdfParams::from_core(&crypto_core::KdfParams::RECOMMENDED),
            kdf_salt: encode_hex(&state.auth.fake_salt(&email)?),
        },
    };
    Ok(Json(response))
}

/// 恢复重置前置查询。
///
/// `recovery_wrapped_mk` 是密文，但忘记密码的用户在认证前就需要它；
/// 对不存在邮箱返回结构相同的伪造帧，避免泄露邮箱是否注册。
#[utoipa::path(
    post,
    path = "/api/v1/auth/recovery-prelogin",
    tag = "auth",
    request_body = RecoveryPreloginRequest,
    responses(
        (status = 200, body = RecoveryPreloginResponse),
        (status = 400, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn recovery_prelogin(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    payload: Result<Json<RecoveryPreloginRequest>, JsonRejection>,
) -> Result<Json<RecoveryPreloginResponse>, ApiError> {
    state.auth.check_ip(ip)?;
    let request = parse_body(payload)?;
    let email = normalize_email(&request.email).ok_or_else(ApiError::invalid_request)?;

    let row = sqlx::query!(
        "SELECT recovery_wrapped_mk FROM users WHERE email = $1::text::citext",
        email,
    )
    .fetch_optional(state.database.pool())
    .await
    .map_err(ApiError::internal)?;

    let recovery_wrapped_mk = match row {
        Some(row) => row.recovery_wrapped_mk,
        None => state.auth.fake_recovery_wrapped_mk(&email)?,
    };
    Ok(Json(RecoveryPreloginResponse {
        recovery_wrapped_mk: encode_hex(&recovery_wrapped_mk),
    }))
}

// ---------------------------------------------------------------------------
// 注册
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/v1/auth/register",
    tag = "auth",
    request_body = RegisterRequest,
    responses(
        (status = 200, body = OkResponse),
        (status = 400, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    payload: Result<Json<RegisterRequest>, JsonRejection>,
) -> Result<Json<OkResponse>, ApiError> {
    state.auth.check_ip(ip)?;
    let request = parse_body(payload)?;
    let email = normalize_email(&request.email).ok_or_else(ApiError::invalid_request)?;

    let kdf_params = request.kdf_params.validate()?;
    let kdf_salt = decode_kdf_salt(&request.kdf_salt)?;
    let auth_key = decode_secret_key(&request.auth_key)?;
    let wrapped_mk = decode_wrapped_material(&request.wrapped_mk)?;
    let recovery_wrapped_mk = decode_wrapped_material(&request.recovery_wrapped_mk)?;
    let recovery_auth_key = decode_secret_key(&request.recovery_auth_key)?;

    let auth_hash = state.auth.hash_with_current_pepper(&auth_key)?;
    let recovery_auth_hash = state.auth.hash_with_current_pepper(&recovery_auth_key)?;
    let kdf_params_json = serde_json::to_value(kdf_params).map_err(ApiError::internal)?;

    let user_id = Uuid::new_v4();
    // 根节点由服务端生成 ID（design D9）；用户表与节点表互相引用，
    // 因此外键是 DEFERRABLE 的，先插入 user 再插入 node 可在同一事务完成。
    let root_node_id = Uuid::new_v4();
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;

    let inserted = sqlx::query!(
        r#"
        INSERT INTO users (
            id, email, kdf_params, kdf_salt, auth_hash, pepper_version,
            recovery_auth_hash, recovery_pepper_version, wrapped_mk, recovery_wrapped_mk,
            root_node_id
        )
        VALUES ($1, $2::text::citext, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT (email) DO NOTHING
        RETURNING id
        "#,
        user_id,
        email,
        kdf_params_json,
        kdf_salt.as_slice(),
        auth_hash.as_slice(),
        state.auth.current_pepper_version(),
        recovery_auth_hash.as_slice(),
        state.auth.current_pepper_version(),
        wrapped_mk,
        recovery_wrapped_mk,
        root_node_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    match inserted {
        Some(row) => {
            // 根节点没有包裹密钥/元数据/名称令牌，也不能被删除、移动或重命名。
            sqlx::query!(
                r#"
                INSERT INTO nodes (id, user_id, parent_id, kind, row_version)
                VALUES ($1, $2, NULL, 'folder', 1)
                "#,
                root_node_id,
                row.id,
            )
            .execute(&mut *tx)
            .await
            .map_err(ApiError::internal)?;

            enqueue_email(
                &mut tx,
                row.id,
                purpose::VERIFY_EMAIL,
                &serde_json::json!({}),
            )
            .await
            .map_err(ApiError::internal)?;
        }
        None => {
            // 邮箱已存在：返回与成功完全相同的响应，并发“已有账号”提示邮件。
            let existing =
                sqlx::query!("SELECT id FROM users WHERE email = $1::text::citext", email,)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(ApiError::internal)?;
            // 账号维度限流：同一账号 10 分钟内只发一封“已有账号”提示邮件，
            // 避免攻击者用重复注册刷受害者的邮箱。
            let recent = sqlx::query_scalar!(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM email_outbox
                    WHERE user_id = $1 AND purpose = $2
                      AND created_at > now() - make_interval(mins => $3)
                ) AS "recent!"
                "#,
                existing.id,
                purpose::EXISTING_ACCOUNT,
                RESEND_THROTTLE_MINUTES,
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(ApiError::internal)?;
            if !recent {
                enqueue_email(
                    &mut tx,
                    existing.id,
                    purpose::EXISTING_ACCOUNT,
                    &serde_json::json!({}),
                )
                .await
                .map_err(ApiError::internal)?;
            }
        }
    }

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(ok())
}

// ---------------------------------------------------------------------------
// 邮箱验证与重发
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/v1/auth/verify-email",
    tag = "auth",
    request_body = VerifyEmailRequest,
    responses(
        (status = 200, body = OkResponse),
        (status = 400, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn verify_email(
    State(state): State<AppState>,
    payload: Result<Json<VerifyEmailRequest>, JsonRejection>,
) -> Result<Json<OkResponse>, ApiError> {
    let request = parse_body(payload)?;
    let token = request.token.trim();
    // 令牌格式错误时也走同一个 invalid_token，避免区分“格式错”和“不存在”。
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApiError::invalid_token());
    }
    let token_hash = hash_token(token);

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let row = sqlx::query!(
        r#"
        SELECT t.id, t.user_id
        FROM email_tokens t
        WHERE t.token_hash = $1
          AND t.purpose = $2
          AND t.used_at IS NULL
          AND t.expires_at > now()
        FOR UPDATE OF t
        "#,
        token_hash,
        EMAIL_PURPOSE_VERIFY,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(ApiError::invalid_token());
    };

    sqlx::query!(
        r#"
        UPDATE email_tokens
        SET used_at = now()
        WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL
        "#,
        row.user_id,
        EMAIL_PURPOSE_VERIFY,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    sqlx::query!(
        r#"
        UPDATE users
        SET email_verified_at = COALESCE(email_verified_at, now()), updated_at = now()
        WHERE id = $1
        "#,
        row.user_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(ok())
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/resend-verification",
    tag = "auth",
    request_body = ResendVerificationRequest,
    responses(
        (status = 200, body = OkResponse),
        (status = 400, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn resend_verification(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    payload: Result<Json<ResendVerificationRequest>, JsonRejection>,
) -> Result<Json<OkResponse>, ApiError> {
    state.auth.check_ip(ip)?;
    let request = parse_body(payload)?;
    // 不论邮箱是否存在、是否已验证，都返回同一个成功响应。
    let Some(email) = normalize_email(&request.email) else {
        return Ok(ok());
    };

    let row = sqlx::query!(
        "SELECT id, email_verified_at FROM users WHERE email = $1::text::citext",
        email,
    )
    .fetch_optional(state.database.pool())
    .await
    .map_err(ApiError::internal)?;

    if let Some(row) = row
        && row.email_verified_at.is_none()
    {
        let recent = sqlx::query_scalar!(
            r#"
                SELECT EXISTS (
                    SELECT 1 FROM email_outbox
                    WHERE user_id = $1 AND purpose = $2
                      AND created_at > now() - make_interval(mins => $3)
                    UNION ALL
                    SELECT 1 FROM email_tokens
                    WHERE user_id = $1 AND purpose = $2
                      AND created_at > now() - make_interval(mins => $3)
                ) AS "recent!"
                "#,
            row.id,
            purpose::VERIFY_EMAIL,
            RESEND_THROTTLE_MINUTES,
        )
        .fetch_one(state.database.pool())
        .await
        .map_err(ApiError::internal)?;

        if !recent {
            let mut tx = state
                .database
                .pool()
                .begin()
                .await
                .map_err(ApiError::internal)?;
            enqueue_email(
                &mut tx,
                row.id,
                purpose::VERIFY_EMAIL,
                &serde_json::json!({ "resend": true }),
            )
            .await
            .map_err(ApiError::internal)?;
            tx.commit().await.map_err(ApiError::internal)?;
        }
    }

    Ok(ok())
}

// ---------------------------------------------------------------------------
// 登录
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, body = LoginResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    headers: HeaderMap,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<(HeaderMap, Json<LoginResponse>), ApiError> {
    state.auth.check_ip(ip)?;
    let request = parse_body(payload)?;
    let email = normalize_email(&request.email);
    let auth_key =
        decode_secret_key(&request.auth_key).map_err(|_| ApiError::invalid_credentials())?;

    let user = match email {
        Some(email) => sqlx::query!(
            r#"
            SELECT id, email::text AS "email!", email_verified_at, kdf_params, kdf_salt,
                   auth_hash, pepper_version, wrapped_mk, failed_logins, locked_until,
                   root_node_id
            FROM users
            WHERE email = $1::text::citext
            "#,
            email,
        )
        .fetch_optional(state.database.pool())
        .await
        .map_err(ApiError::internal)?,
        // 邮箱格式无效时不做查询，但仍执行一次同样的 HMAC 计算。
        None => None,
    };

    // 无论账号是否存在都做一次 HMAC-SHA256，缩小两条路径的耗时差异。
    let (pepper_version, stored_hash) = match &user {
        Some(user) => (user.pepper_version, user.auth_hash.as_slice()),
        None => (state.auth.current_pepper_version(), &ZERO_HASH[..]),
    };
    let presented = state.auth.hash_with_pepper(pepper_version, &auth_key)?;
    let matches = constant_time_eq(&presented, stored_hash);

    let Some(user) = user else {
        return Err(ApiError::invalid_credentials());
    };

    if let Some(locked_until) = user.locked_until {
        if locked_until > OffsetDateTime::now_utc() {
            return Err(ApiError::account_locked(lock_retry_after(locked_until)));
        }
        // 锁定已过期：先清零失败计数，避免下一次失败立即再次锁定。
        let mut tx = state
            .database
            .pool()
            .begin()
            .await
            .map_err(ApiError::internal)?;
        sqlx::query!(
            "UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1",
            user.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
        tx.commit().await.map_err(ApiError::internal)?;
    }

    if !matches {
        let mut tx = state
            .database
            .pool()
            .begin()
            .await
            .map_err(ApiError::internal)?;
        record_failed_login(&mut tx, user.id).await?;
        tx.commit().await.map_err(ApiError::internal)?;
        return Err(ApiError::invalid_credentials());
    }

    // 登录成功：清零失败计数；若账号仍使用旧 pepper，则升级 auth_hash。
    let (upgraded_hash, upgraded_version) =
        if user.pepper_version != state.auth.current_pepper_version() {
            (
                Some(state.auth.hash_with_current_pepper(&auth_key)?.to_vec()),
                Some(state.auth.current_pepper_version()),
            )
        } else {
            (None, None)
        };
    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    sqlx::query!(
        r#"
        UPDATE users
        SET failed_logins = 0,
            locked_until = NULL,
            auth_hash = COALESCE($2::bytea, auth_hash),
            pepper_version = COALESCE($3::integer, pepper_version),
            updated_at = now()
        WHERE id = $1
        "#,
        user.id,
        upgraded_hash.as_deref(),
        upgraded_version,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    let token = generate_session_token()?;
    let token_hash = hash_token(&token);
    let session_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO sessions (id, user_id, token_hash, expires_at, ip, user_agent)
        VALUES ($1, $2, $3, now() + make_interval(days => $4), $5, $6)
        "#,
        session_id,
        user.id,
        token_hash,
        SESSION_ABSOLUTE_DAYS,
        ip.to_string(),
        user_agent(&headers),
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    tx.commit().await.map_err(ApiError::internal)?;

    let response = LoginResponse {
        user_id: user.id,
        email: user.email,
        email_verified: user.email_verified_at.is_some(),
        kdf_params: KdfParams::from_stored(&user.kdf_params)?,
        kdf_salt: encode_hex(&user.kdf_salt),
        wrapped_mk: encode_hex(&user.wrapped_mk),
        root_node_id: user.root_node_id,
    };
    Ok((session_cookie_header(&token)?, Json(response)))
}

/// 仅用于不存在账号时的一次常量时间比较；不是任何真实密钥。
const ZERO_HASH: [u8; 32] = [0_u8; 32];

// ---------------------------------------------------------------------------
// 当前用户 / 登出 / 会话管理
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    tag = "auth",
    responses(
        (status = 200, body = MeResponse),
        (status = 401, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn me(
    State(state): State<AppState>,
    session: AuthenticatedSession,
) -> Result<Json<MeResponse>, ApiError> {
    let row = sqlx::query!(
        r#"
        SELECT id, email::text AS "email!", email_verified_at, kdf_params, kdf_salt, wrapped_mk,
               root_node_id
        FROM users
        WHERE id = $1
        "#,
        session.user_id,
    )
    .fetch_optional(state.database.pool())
    .await
    .map_err(ApiError::internal)?;

    let Some(row) = row else {
        return Err(ApiError::unauthenticated());
    };
    Ok(Json(MeResponse {
        user_id: row.id,
        email: row.email,
        email_verified: row.email_verified_at.is_some(),
        kdf_params: KdfParams::from_stored(&row.kdf_params)?,
        kdf_salt: encode_hex(&row.kdf_salt),
        wrapped_mk: encode_hex(&row.wrapped_mk),
        root_node_id: row.root_node_id,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "auth",
    responses(
        (status = 200, body = OkResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn logout(
    State(state): State<AppState>,
    session: AuthenticatedSession,
) -> Result<(HeaderMap, Json<OkResponse>), ApiError> {
    sqlx::query!(
        r#"
        UPDATE sessions
        SET revoked_at = now()
        WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
        session.session_id,
        session.user_id,
    )
    .execute(state.database.pool())
    .await
    .map_err(ApiError::internal)?;
    Ok((clear_session_cookie_header()?, ok()))
}

#[derive(Debug, sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    created_at: OffsetDateTime,
    last_seen_at: OffsetDateTime,
    expires_at: OffsetDateTime,
    ip: String,
    user_agent: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/auth/sessions",
    tag = "auth",
    responses(
        (status = 200, body = SessionsResponse),
        (status = 401, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    session: AuthenticatedSession,
) -> Result<Json<SessionsResponse>, ApiError> {
    let rows = sqlx::query_as!(
        SessionRow,
        r#"
        SELECT id, created_at, last_seen_at, expires_at, ip, user_agent
        FROM sessions
        WHERE user_id = $1
          AND revoked_at IS NULL
          AND expires_at > now()
          AND last_seen_at > now() - make_interval(days => $2)
        ORDER BY last_seen_at DESC
        "#,
        session.user_id,
        SESSION_IDLE_DAYS,
    )
    .fetch_all(state.database.pool())
    .await
    .map_err(ApiError::internal)?;

    let sessions = rows
        .into_iter()
        .map(|row| SessionSummary {
            id: row.id,
            created_at: format_time(row.created_at),
            last_seen_at: format_time(row.last_seen_at),
            expires_at: format_time(row.expires_at),
            ip: row.ip,
            user_agent: row.user_agent,
            current: row.id == session.session_id,
        })
        .collect();
    Ok(Json(SessionsResponse { sessions }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/auth/sessions/{session_id}",
    tag = "auth",
    params(("session_id" = String, Path, description = "要撤销的会话 ID")),
    responses(
        (status = 200, body = OkResponse),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 404, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn revoke_session(
    State(state): State<AppState>,
    session: AuthenticatedSession,
    Path(session_id): Path<String>,
) -> Result<Response, ApiError> {
    let Ok(target) = Uuid::parse_str(&session_id) else {
        return Err(ApiError::not_found());
    };
    let result = sqlx::query!(
        r#"
        UPDATE sessions
        SET revoked_at = now()
        WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
        "#,
        target,
        session.user_id,
    )
    .execute(state.database.pool())
    .await
    .map_err(ApiError::internal)?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found());
    }

    if target == session.session_id {
        let (headers, body) = (clear_session_cookie_header()?, ok());
        let mut response = (headers, body).into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        Ok(response)
    } else {
        Ok(ok().into_response())
    }
}

// ---------------------------------------------------------------------------
// 修改密码 / 恢复重置 / 轮换恢复密钥
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/api/v1/auth/change-password",
    tag = "auth",
    request_body = ChangePasswordRequest,
    responses(
        (status = 200, body = OkResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn change_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    session: AuthenticatedSession,
    payload: Result<Json<ChangePasswordRequest>, JsonRejection>,
) -> Result<Json<OkResponse>, ApiError> {
    state.auth.check_ip(ip)?;
    let request = parse_body(payload)?;
    let old_auth_key = decode_secret_key(&request.old_auth_key)?;
    let new_auth_key = decode_secret_key(&request.new_auth_key)?;
    let new_kdf_params = request.new_kdf_params.validate()?;
    let new_kdf_salt = decode_kdf_salt(&request.new_kdf_salt)?;
    let new_wrapped_mk = decode_wrapped_material(&request.new_wrapped_mk)?;
    let new_kdf_params_json = serde_json::to_value(new_kdf_params).map_err(ApiError::internal)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let user = sqlx::query!(
        "SELECT auth_hash, pepper_version FROM users WHERE id = $1 FOR UPDATE",
        session.user_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    let Some(user) = user else {
        return Err(ApiError::unauthenticated());
    };

    let presented = state
        .auth
        .hash_with_pepper(user.pepper_version, &old_auth_key)?;
    if !constant_time_eq(&presented, &user.auth_hash) {
        // 事务回滚：不改任何数据（包括失败计数）。
        return Err(ApiError::invalid_credentials());
    }

    let new_auth_hash = state.auth.hash_with_current_pepper(&new_auth_key)?;
    sqlx::query!(
        r#"
        UPDATE users
        SET auth_hash = $2,
            pepper_version = $3,
            kdf_params = $4,
            kdf_salt = $5,
            wrapped_mk = $6,
            failed_logins = 0,
            locked_until = NULL,
            updated_at = now()
        WHERE id = $1
        "#,
        session.user_id,
        new_auth_hash.as_slice(),
        state.auth.current_pepper_version(),
        new_kdf_params_json,
        new_kdf_salt.as_slice(),
        new_wrapped_mk,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    sqlx::query!(
        r#"
        UPDATE sessions
        SET revoked_at = now()
        WHERE user_id = $1 AND id <> $2 AND revoked_at IS NULL
        "#,
        session.user_id,
        session.session_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    enqueue_email(
        &mut tx,
        session.user_id,
        purpose::PASSWORD_CHANGED,
        &serde_json::json!({}),
    )
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(ok())
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/recover",
    tag = "auth",
    request_body = RecoverRequest,
    responses(
        (status = 200, body = OkResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn recover(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    payload: Result<Json<RecoverRequest>, JsonRejection>,
) -> Result<Json<OkResponse>, ApiError> {
    state.auth.check_ip(ip)?;
    let request = parse_body(payload)?;
    let Some(email) = normalize_email(&request.email) else {
        return Err(ApiError::invalid_recovery_key());
    };
    let recovery_auth_key = decode_secret_key(&request.recovery_auth_key)
        .map_err(|_| ApiError::invalid_recovery_key())?;
    let new_auth_key = decode_secret_key(&request.new_auth_key)?;
    let new_kdf_params = request.new_kdf_params.validate()?;
    let new_kdf_salt = decode_kdf_salt(&request.new_kdf_salt)?;
    let new_wrapped_mk = decode_wrapped_material(&request.new_wrapped_mk)?;
    let new_kdf_params_json = serde_json::to_value(new_kdf_params).map_err(ApiError::internal)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let user = sqlx::query!(
        r#"
        SELECT id, recovery_auth_hash, recovery_pepper_version, locked_until
        FROM users
        WHERE email = $1::text::citext
        FOR UPDATE
        "#,
        email,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    let Some(user) = user else {
        // 与恢复密钥错误不可区分，避免账号枚举。
        return Err(ApiError::invalid_recovery_key());
    };

    if let Some(locked_until) = user.locked_until {
        if locked_until > OffsetDateTime::now_utc() {
            return Err(ApiError::account_locked(lock_retry_after(locked_until)));
        }
        sqlx::query!(
            "UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1",
            user.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    }

    let presented = state
        .auth
        .hash_with_pepper(user.recovery_pepper_version, &recovery_auth_key)?;
    if !constant_time_eq(&presented, &user.recovery_auth_hash) {
        record_failed_login(&mut tx, user.id).await?;
        // 失败计数需要提交；认证材料保持不变。
        tx.commit().await.map_err(ApiError::internal)?;
        return Err(ApiError::invalid_recovery_key());
    }

    let new_auth_hash = state.auth.hash_with_current_pepper(&new_auth_key)?;
    let new_recovery_hash = state.auth.hash_with_current_pepper(&recovery_auth_key)?;
    sqlx::query!(
        r#"
        UPDATE users
        SET auth_hash = $2,
            pepper_version = $3,
            recovery_auth_hash = $4,
            recovery_pepper_version = $5,
            kdf_params = $6,
            kdf_salt = $7,
            wrapped_mk = $8,
            failed_logins = 0,
            locked_until = NULL,
            updated_at = now()
        WHERE id = $1
        "#,
        user.id,
        new_auth_hash.as_slice(),
        state.auth.current_pepper_version(),
        new_recovery_hash.as_slice(),
        state.auth.current_pepper_version(),
        new_kdf_params_json,
        new_kdf_salt.as_slice(),
        new_wrapped_mk,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    // 重置后所有会话失效（包括可能残留的当前会话）。
    sqlx::query!(
        "UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
        user.id,
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    enqueue_email(
        &mut tx,
        user.id,
        purpose::PASSWORD_CHANGED,
        &serde_json::json!({ "reason": "recovery" }),
    )
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(ok())
}

#[utoipa::path(
    post,
    path = "/api/v1/auth/rotate-recovery-key",
    tag = "auth",
    request_body = RotateRecoveryKeyRequest,
    responses(
        (status = 200, body = OkResponse),
        (status = 400, body = ErrorBody),
        (status = 401, body = ErrorBody),
        (status = 403, body = ErrorBody),
        (status = 429, body = ErrorBody),
        (status = 500, body = ErrorBody)
    )
)]
pub async fn rotate_recovery_key(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    session: AuthenticatedSession,
    payload: Result<Json<RotateRecoveryKeyRequest>, JsonRejection>,
) -> Result<Json<OkResponse>, ApiError> {
    state.auth.check_ip(ip)?;
    let request = parse_body(payload)?;
    let current_auth_key = decode_secret_key(&request.current_auth_key)
        .map_err(|_| ApiError::invalid_credentials())?;
    let new_recovery_wrapped_mk = decode_wrapped_material(&request.new_recovery_wrapped_mk)?;
    let new_recovery_auth_key = decode_secret_key(&request.new_recovery_auth_key)?;
    let new_recovery_hash = state
        .auth
        .hash_with_current_pepper(&new_recovery_auth_key)?;

    let mut tx = state
        .database
        .pool()
        .begin()
        .await
        .map_err(ApiError::internal)?;
    let user = sqlx::query!(
        r#"
        SELECT auth_hash, pepper_version, locked_until
        FROM users
        WHERE id = $1
        FOR UPDATE
        "#,
        session.user_id,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(ApiError::internal)?;
    let Some(user) = user else {
        return Err(ApiError::unauthenticated());
    };

    // 与登录共用账号锁定逻辑：锁定期间即使密码正确也直接拒绝。
    if let Some(locked_until) = user.locked_until {
        if locked_until > OffsetDateTime::now_utc() {
            return Err(ApiError::account_locked(lock_retry_after(locked_until)));
        }
        sqlx::query!(
            "UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1",
            session.user_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(ApiError::internal)?;
    }

    let presented = state
        .auth
        .hash_with_pepper(user.pepper_version, &current_auth_key)?;
    if !constant_time_eq(&presented, &user.auth_hash) {
        record_failed_login(&mut tx, session.user_id).await?;
        // 只提交失败计数；恢复材料保持原样。
        tx.commit().await.map_err(ApiError::internal)?;
        return Err(ApiError::invalid_credentials());
    }

    sqlx::query!(
        r#"
        UPDATE users
        SET recovery_wrapped_mk = $2,
            recovery_auth_hash = $3,
            recovery_pepper_version = $4,
            failed_logins = 0,
            locked_until = NULL,
            updated_at = now()
        WHERE id = $1
        "#,
        session.user_id,
        new_recovery_wrapped_mk,
        new_recovery_hash.as_slice(),
        state.auth.current_pepper_version(),
    )
    .execute(&mut *tx)
    .await
    .map_err(ApiError::internal)?;

    enqueue_email(
        &mut tx,
        session.user_id,
        purpose::RECOVERY_KEY_CHANGED,
        &serde_json::json!({}),
    )
    .await
    .map_err(ApiError::internal)?;

    tx.commit().await.map_err(ApiError::internal)?;
    Ok(ok())
}
