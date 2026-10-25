//! 用户认证基础设施（design D4/D7/D14）。
//!
//! 本模块只放跨 handler 复用的安全原语：
//! - pepper 版本链与 `HMAC-SHA256(pepper, auth_key)`；
//! - prelogin 对不存在邮箱的确定性伪造盐；
//! - 令牌生成、SHA-256 哈希、十六进制编解码与常量时间比较；
//! - 会话 Cookie 提取与 [`AuthenticatedSession`] 提取器；
//! - 可信代理网段解析后的客户端 IP 与进程内 IP 限流。
//!
//! 具体 HTTP 路由在 [`routes`]。

pub mod routes;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use governor::clock::Clock;
use governor::state::keyed::DefaultKeyedStateStore;
use governor::{Quota, RateLimiter};
use hmac::{Hmac, Mac};
use ipnet::IpNet;
use rand::TryRngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::accounts;
use crate::api::error::ApiError;
use crate::config::{AuthConfig, Secret};

/// 会话 Cookie 名（design D7，`__Host-` 前缀要求 Secure + Path=/ 且无 Domain）。
pub const SESSION_COOKIE_NAME: &str = "__Host-session";
/// 改变状态请求必须携带的自定义头（user-accounts spec：跨站请求防护）。
pub const CSRF_HEADER: &str = "x-requested-with";
/// 自定义头的固定值。
pub const CSRF_VALUE: &str = "backup-web";
/// 空闲过期天数。
pub const SESSION_IDLE_DAYS: i32 = 7;
/// 绝对过期天数。
pub const SESSION_ABSOLUTE_DAYS: i32 = 30;
/// `last_seen_at` 写节流：最多每 5 分钟写一次。
pub const SESSION_LAST_SEEN_THROTTLE_SECONDS: f64 = 300.0;
/// 连续失败次数达到该值后锁定登录。
pub const MAX_FAILED_LOGINS: i32 = 10;
/// 锁定时长（分钟）。
pub const LOCKOUT_MINUTES: i32 = 15;
/// 邮件令牌有效期（小时）。
pub const EMAIL_TOKEN_TTL_HOURS: i32 = 24;
/// 邮箱验证令牌用途。
pub const EMAIL_PURPOSE_VERIFY: &str = "verify_email";
/// 重发验证邮件的账号维度节流窗口（分钟）。
pub const RESEND_THROTTLE_MINUTES: i32 = 10;

/// 每 IP 每小时允许的成功注册账号数（add-admin-console D2）。
pub const REGISTRATION_LIMIT_PER_HOUR: u32 = 5;
/// 以 `NonZeroU32` 形式暴露给 governor（常量求值，不产生运行时错误路径）。
const REGISTRATIONS_PER_HOUR: NonZeroU32 = match NonZeroU32::new(REGISTRATION_LIMIT_PER_HOUR) {
    Some(value) => value,
    None => NonZeroU32::MIN,
};

/// 每用户每分钟的写接口上限由配置项
/// `BACKUP_USER_WRITE_RATE_LIMIT_PER_MINUTE` 提供；批量上传接口各算一次请求。
type KeyedLimiter =
    RateLimiter<IpAddr, DefaultKeyedStateStore<IpAddr>, governor::clock::DefaultClock>;
type UserKeyedLimiter =
    RateLimiter<Uuid, DefaultKeyedStateStore<Uuid>, governor::clock::DefaultClock>;

/// 认证共享状态：pepper 版本链 + IP 限流器 + 每用户写限流器。
#[derive(Clone)]
pub struct AuthContext {
    config: Arc<AuthConfig>,
    limiter: Arc<IpRateLimiter>,
    registration_limiter: Arc<IpRateLimiter>,
    user_write_limiter: Arc<UserWriteRateLimiter>,
}

impl AuthContext {
    pub fn new(
        config: AuthConfig,
        per_minute: NonZeroU32,
        user_write_per_minute: NonZeroU32,
    ) -> Self {
        Self {
            config: Arc::new(config),
            limiter: Arc::new(IpRateLimiter::new(per_minute)),
            registration_limiter: Arc::new(IpRateLimiter::with_quota(Quota::per_hour(
                REGISTRATIONS_PER_HOUR,
            ))),
            user_write_limiter: Arc::new(UserWriteRateLimiter::new(user_write_per_minute)),
        }
    }

    pub fn config(&self) -> &AuthConfig {
        &self.config
    }

    pub fn current_pepper_version(&self) -> i32 {
        self.config.pepper_version
    }

    pub fn pepper_for(&self, version: i32) -> Option<&Secret> {
        self.config.pepper_for(version)
    }

    /// 用指定版本的 pepper 计算 auth_hash / recovery_auth_hash。
    pub fn hash_with_pepper(&self, version: i32, value: &[u8]) -> Result<[u8; 32], ApiError> {
        let pepper = self
            .config
            .pepper_for(version)
            .ok_or_else(|| ApiError::internal("配置中缺少所需的 pepper 版本"))?;
        hmac_sha256(pepper.expose().as_bytes(), value)
    }

    /// 用当前 pepper 计算哈希（注册、升级、恢复/轮换成功后使用）。
    pub fn hash_with_current_pepper(&self, value: &[u8]) -> Result<[u8; 32], ApiError> {
        self.hash_with_pepper(self.config.pepper_version, value)
    }

    /// 对不存在邮箱生成确定性伪造盐：HMAC(server_key, "fake-salt|" + email) 前 16 字节。
    pub fn fake_salt(&self, normalized_email: &str) -> Result<[u8; 16], ApiError> {
        let message = format!("fake-salt|{normalized_email}");
        let digest = hmac_sha256(
            self.config.fake_salt_key.expose().as_bytes(),
            message.as_bytes(),
        )?;
        let mut salt = [0_u8; 16];
        salt.copy_from_slice(&digest[..16]);
        Ok(salt)
    }

    /// 对不存在邮箱生成确定性的伪造恢复包裹材料。
    ///
    /// 结构与真实 `recovery_wrapped_mk` 完全一致（版本字节 + 73 字节帧），
    /// 使用独立的 fake-salt 密钥派生，避免恢复前置查询泄露邮箱是否注册。
    pub fn fake_recovery_wrapped_mk(&self, normalized_email: &str) -> Result<Vec<u8>, ApiError> {
        let mut output = Vec::with_capacity(crypto_core::FRAME_OVERHEAD + crypto_core::KEY_LEN);
        output.push(crypto_core::CIPHERTEXT_FORMAT_VERSION);
        for counter in 0..3 {
            let message = format!("fake-recovery|{normalized_email}|{counter}");
            let digest = hmac_sha256(
                self.config.fake_salt_key.expose().as_bytes(),
                message.as_bytes(),
            )?;
            output.extend_from_slice(&digest);
        }
        output.truncate(crypto_core::FRAME_OVERHEAD + crypto_core::KEY_LEN);
        Ok(output)
    }

    /// 检查并消耗该 IP 的一次认证类请求配额。
    pub fn check_ip(&self, ip: IpAddr) -> Result<(), ApiError> {
        self.limiter.check(ip).map_err(ApiError::too_many_requests)
    }

    /// 检查并消耗该 IP 的一次注册配额（每小时 5 个成功注册）。
    pub fn check_registration(&self, ip: IpAddr) -> Result<(), ApiError> {
        self.registration_limiter
            .check(ip)
            .map_err(ApiError::too_many_requests)
    }

    /// 检查并消耗该已认证用户的一次写接口配额。
    pub fn check_user_write(&self, user_id: Uuid) -> Result<(), ApiError> {
        self.user_write_limiter
            .check(user_id)
            .map_err(ApiError::too_many_requests)
    }
}

/// 进程内每用户写限流器（与 IP 限流器同一套 governor 语义）。
struct UserWriteRateLimiter {
    limiter: UserKeyedLimiter,
    checks: AtomicU64,
}

impl UserWriteRateLimiter {
    fn new(per_minute: NonZeroU32) -> Self {
        Self {
            limiter: RateLimiter::keyed(Quota::per_minute(per_minute)),
            checks: AtomicU64::new(0),
        }
    }

    fn check(&self, user_id: Uuid) -> Result<(), u64> {
        if self
            .checks
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(RATE_LIMITER_CLEANUP_INTERVAL)
        {
            self.limiter.retain_recent();
        }
        match self.limiter.check_key(&user_id) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                let wait = not_until.wait_time_from(self.limiter.clock().now());
                Err(wait.as_secs().max(1))
            }
        }
    }
}

/// 进程内 IP 限流器（design D14：governor 令牌桶，多实例下近似 ×N）。
struct IpRateLimiter {
    limiter: KeyedLimiter,
    checks: AtomicU64,
}

/// 每 N 次检查清理一次已“冷却”的 key，避免大量一次性 IP 让 HashMap 无限增长。
const RATE_LIMITER_CLEANUP_INTERVAL: u64 = 1024;

impl IpRateLimiter {
    fn new(per_minute: NonZeroU32) -> Self {
        Self::with_quota(Quota::per_minute(per_minute))
    }

    fn with_quota(quota: Quota) -> Self {
        Self {
            // 关闭 dashmap/quanta 特性：默认使用 parking_lot HashMap 与 std Instant，
            // 与 design 的“进程内令牌桶”语义一致，且不引入额外并发组件。
            limiter: RateLimiter::keyed(quota),
            checks: AtomicU64::new(0),
        }
    }

    fn check(&self, ip: IpAddr) -> Result<(), u64> {
        if self
            .checks
            .fetch_add(1, Ordering::Relaxed)
            .is_multiple_of(RATE_LIMITER_CLEANUP_INTERVAL)
        {
            self.limiter.retain_recent();
        }
        match self.limiter.check_key(&ip) {
            Ok(()) => Ok(()),
            Err(not_until) => {
                let wait = not_until.wait_time_from(self.limiter.clock().now());
                Err(wait.as_secs().max(1))
            }
        }
    }
}

/// HMAC-SHA256。HMAC 接受任意长度密钥，这里的错误路径只用于保持无 unwrap。
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<[u8; 32], ApiError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key)
        .map_err(|_| ApiError::internal("HMAC 密钥初始化失败"))?;
    mac.update(data);
    let bytes = mac.finalize().into_bytes();
    let mut output = [0_u8; 32];
    output.copy_from_slice(&bytes);
    Ok(output)
}

/// 常量时间比较（长度不同直接返回 false，长度不是秘密）。
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && bool::from(left.ct_eq(right))
}

/// 邮箱规范化：去首尾空白 + 小写。返回 `None` 表示格式明显无效。
pub fn normalize_email(email: &str) -> Option<String> {
    let normalized = email.trim().to_lowercase();
    if normalized.is_empty()
        || normalized.len() > 254
        || !normalized.contains('@')
        || normalized.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(normalized)
}

/// 生成 256 位会话令牌（hex，64 字符），只返回明文用于 Set-Cookie。
pub fn generate_session_token() -> Result<String, ApiError> {
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(ApiError::internal)?;
    Ok(encode_hex(&bytes))
}

/// 会话/邮件令牌只保存 SHA-256。
pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// 小写十六进制编码。
pub fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        // 写入 String 不会失败。
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// 解码固定长度的十六进制字符串。
pub fn decode_hex_array<const N: usize>(value: &str) -> Option<[u8; N]> {
    let decoded = decode_hex_vec(value)?;
    decoded.try_into().ok()
}

/// 解码任意长度的十六进制字符串。
pub fn decode_hex_vec(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let mut output = Vec::with_capacity(value.len() / 2);
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    for chunk in pairs {
        let high = hex_digit(chunk[0])?;
        let low = hex_digit(chunk[1])?;
        output.push((high << 4) | low);
    }
    Some(output)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// 认证密钥 / 恢复认证值必须是 32 字节。
pub fn decode_secret_key(value: &str) -> Result<[u8; 32], ApiError> {
    decode_hex_array::<32>(value).ok_or_else(ApiError::invalid_key_material)
}

/// KDF 盐必须是 16 字节。
pub fn decode_kdf_salt(value: &str) -> Result<[u8; 16], ApiError> {
    decode_hex_array::<16>(value).ok_or_else(ApiError::invalid_key_material)
}

/// 包裹材料：版本字节 + AEAD 帧，长度必须是 FRAME_OVERHEAD + 32。
///
/// 服务端不持有任何解包密钥，因此只能校验格式（版本字节、长度）；
/// 认证失败由客户端解包时发现。
pub fn decode_wrapped_material(value: &str) -> Result<Vec<u8>, ApiError> {
    let bytes = decode_hex_vec(value).ok_or_else(ApiError::invalid_key_material)?;
    if bytes.len() != crypto_core::FRAME_OVERHEAD + crypto_core::KEY_LEN {
        return Err(ApiError::invalid_key_material());
    }
    if bytes.first() != Some(&crypto_core::CIPHERTEXT_FORMAT_VERSION) {
        return Err(ApiError::invalid_key_material());
    }
    Ok(bytes)
}

/// 从 Cookie 头读取会话令牌。
pub fn session_cookie(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for pair in cookies.split(';') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if name.trim() == SESSION_COOKIE_NAME {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }
    None
}

/// 当前客户端 IP。
///
/// 只有建连的对端位于 `trusted_proxies` 中时才读取 `X-Forwarded-For`；
/// 从右向左跳过可信代理，取最右侧的不可信地址（防止客户端伪造左侧值）。
pub fn client_ip(parts: &Parts, trusted_proxies: &[IpNet]) -> IpAddr {
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));

    if !is_trusted(peer, trusted_proxies) {
        return peer;
    }

    let Some(raw) = parts
        .headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    else {
        return peer;
    };

    let mut candidate = peer;
    for part in raw.split(',').map(str::trim).rev() {
        let Ok(ip) = part.parse::<IpAddr>() else {
            break;
        };
        candidate = ip;
        if !is_trusted(ip, trusted_proxies) {
            return ip;
        }
    }
    candidate
}

fn is_trusted(ip: IpAddr, trusted_proxies: &[IpNet]) -> bool {
    trusted_proxies.iter().any(|network| network.contains(&ip))
}

/// 已通过会话 Cookie 认证的当前会话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedSession {
    pub session_id: Uuid,
    pub user_id: Uuid,
    /// 用户当前状态（add-admin-console D2）。
    pub status: String,
    /// 是否为管理员（只读；不能通过任何 HTTP 接口修改）。
    pub is_admin: bool,
    /// 会话创建时间；管理操作要求它在最近 12 小时内（D3）。
    pub session_created_at: OffsetDateTime,
}

impl FromRequestParts<AppState> for AuthenticatedSession {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // 改变状态的 CSRF 头由 api::csrf 中间件统一检查；这里只负责会话。
        let token = session_cookie(&parts.headers).ok_or_else(ApiError::unauthenticated)?;
        let token_hash = hash_token(&token);

        // 用户状态与会话状态在同一次 JOIN 查询中读取，不额外往返（design D2）。
        let row = sqlx::query!(
            r#"
            SELECT s.id, s.user_id, s.created_at, u.status, u.is_admin
            FROM sessions s
            JOIN users u ON u.id = s.user_id
            WHERE s.token_hash = $1
              AND s.revoked_at IS NULL
              AND s.expires_at > now()
              AND s.last_seen_at > now() - make_interval(days => $2)
            "#,
            token_hash,
            SESSION_IDLE_DAYS,
        )
        .fetch_optional(state.database.pool())
        .await
        .map_err(ApiError::internal)?;

        let Some(row) = row else {
            return Err(ApiError::unauthenticated());
        };

        // 被停用/拒绝的账号即使还残留会话也不能继续使用（停用时已撤销全部会话）。
        if matches!(
            row.status.as_str(),
            accounts::SUSPENDED | accounts::REJECTED
        ) {
            return Err(ApiError::unauthenticated());
        }

        // 写节流：最多每 5 分钟更新一次 last_seen_at。失败不影响本次请求。
        if let Err(error) = sqlx::query!(
            r#"
            UPDATE sessions
            SET last_seen_at = now()
            WHERE id = $1
              AND last_seen_at < now() - make_interval(secs => $2)
            "#,
            row.id,
            SESSION_LAST_SEEN_THROTTLE_SECONDS,
        )
        .execute(state.database.pool())
        .await
        {
            tracing::warn!(error = %error, "更新会话 last_seen_at 失败");
        }

        Ok(Self {
            session_id: row.id,
            user_id: row.user_id,
            status: row.status,
            is_admin: row.is_admin,
            session_created_at: row.created_at,
        })
    }
}

/// 需要写权限的会话：账号必须处于 `active`。
///
/// 只读接口（列目录、下载、会话列表等）继续使用 [`AuthenticatedSession`]；
/// 所有创建/修改/删除接口改用本提取器，从而把“待审核用户不能写”的规则集中在一处。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritableSession(pub AuthenticatedSession);

impl std::ops::Deref for WritableSession {
    type Target = AuthenticatedSession;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromRequestParts<AppState> for WritableSession {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let session = AuthenticatedSession::from_request_parts(parts, state).await?;
        match session.status.as_str() {
            accounts::ACTIVE => Ok(Self(session)),
            accounts::PENDING => Err(ApiError::account_pending()),
            accounts::SUSPENDED => Err(ApiError::account_suspended(None)),
            accounts::REJECTED => Err(ApiError::account_rejected(None)),
            _ => Err(ApiError::internal("遇到未知的账号状态")),
        }
    }
}

/// 客户端 IP 提取器（使用 [`client_ip`]）。
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(client_ip(parts, &state.config.trusted_proxies)))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::net::Ipv6Addr;

    #[test]
    fn email_normalization_is_trimmed_and_lowercased() {
        assert_eq!(
            normalize_email("  User@Example.COM ").as_deref(),
            Some("user@example.com")
        );
        assert_eq!(normalize_email("missing-at"), None);
        assert_eq!(normalize_email("has space@example.com"), None);
        assert_eq!(normalize_email(""), None);
    }

    #[test]
    fn fake_salt_is_deterministic_and_email_specific() {
        let context = AuthContext::new(
            test_auth_config(),
            NonZeroU32::new(60).expect("60"),
            NonZeroU32::new(240).expect("240"),
        );
        let first = context.fake_salt("user@example.com").expect("盐");
        let second = context.fake_salt("user@example.com").expect("盐");
        let other = context.fake_salt("other@example.com").expect("盐");
        assert_eq!(first, second);
        assert_ne!(first, other);
    }

    #[test]
    fn fake_salt_uses_a_key_separate_from_pepper() {
        let context = AuthContext::new(
            test_auth_config(),
            NonZeroU32::new(60).expect("60"),
            NonZeroU32::new(240).expect("240"),
        );
        let salt = context.fake_salt("user@example.com").expect("盐");
        let with_pepper = hmac_sha256(
            context.config().pepper.expose().as_bytes(),
            b"fake-salt|user@example.com",
        )
        .expect("HMAC");
        assert_ne!(&salt[..], &with_pepper[..16]);
    }

    #[test]
    fn hex_roundtrip_and_rejections() {
        let bytes = [0x00, 0x0f, 0xa5, 0xff];
        let encoded = encode_hex(&bytes);
        assert_eq!(encoded, "000fa5ff");
        assert_eq!(decode_hex_vec(&encoded).expect("解码"), bytes);
        assert_eq!(decode_hex_vec("0"), None);
        assert_eq!(decode_hex_vec("zz"), None);
        assert_eq!(decode_hex_array::<4>("000fa5ff").expect("定长"), bytes);
        assert!(decode_hex_array::<3>("000fa5ff").is_none());
    }

    #[test]
    fn wrapped_material_requires_frame_version_and_length() {
        let mut frame = vec![crypto_core::CIPHERTEXT_FORMAT_VERSION];
        frame.extend(std::iter::repeat_n(
            0_u8,
            crypto_core::FRAME_OVERHEAD + crypto_core::KEY_LEN - 1,
        ));
        assert!(decode_wrapped_material(&encode_hex(&frame)).is_ok());

        let mut wrong_version = frame.clone();
        wrong_version[0] = 0xff;
        assert!(decode_wrapped_material(&encode_hex(&wrong_version)).is_err());

        assert!(decode_wrapped_material(&encode_hex(&frame[..10])).is_err());
        assert!(decode_wrapped_material("not-hex").is_err());
    }

    #[test]
    fn constant_time_compare_matches_slices() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn session_cookie_skips_malformed_pairs() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "malformed; theme=dark; __Host-session=abc123"
                .parse()
                .expect("Cookie 头"),
        );
        assert_eq!(session_cookie(&headers).as_deref(), Some("abc123"));
    }

    #[test]
    fn client_ip_ignores_forwarded_for_from_untrusted_peer() {
        let mut parts = request_parts();
        parts
            .headers
            .insert("x-forwarded-for", "203.0.113.9".parse().expect("头"));
        let trusted = Vec::new();
        let ip = client_ip(&parts, &trusted);
        assert_eq!(ip, IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn client_ip_uses_forwarded_for_only_from_trusted_peer() {
        let mut parts = request_parts_with_peer(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)));
        parts.headers.insert(
            "x-forwarded-for",
            "203.0.113.9, 10.0.0.5".parse().expect("头"),
        );
        let trusted: Vec<IpNet> = vec!["10.0.0.0/8".parse().expect("网段")];
        assert_eq!(
            client_ip(&parts, &trusted),
            "203.0.113.9".parse::<IpAddr>().expect("IP")
        );

        // 全部可信时取最左侧（仍视为代理链的一部分）。
        parts
            .headers
            .insert("x-forwarded-for", "10.0.0.7, 10.0.0.5".parse().expect("头"));
        assert_eq!(
            client_ip(&parts, &trusted),
            "10.0.0.7".parse::<IpAddr>().expect("IP")
        );
    }

    #[test]
    fn client_ip_supports_ipv6_peers() {
        let mut parts = request_parts_with_peer(IpAddr::V6(Ipv6Addr::LOCALHOST));
        parts
            .headers
            .insert("x-forwarded-for", "2001:db8::1".parse().expect("头"));
        let trusted: Vec<IpNet> = vec!["::1/128".parse().expect("网段")];
        assert_eq!(
            client_ip(&parts, &trusted),
            "2001:db8::1".parse::<IpAddr>().expect("IP")
        );
    }

    #[test]
    fn rate_limiter_allows_quota_then_reports_retry_after() {
        let limiter = IpRateLimiter::new(NonZeroU32::new(2).expect("2"));
        let ip: IpAddr = "198.51.100.1".parse().expect("IP");
        assert!(limiter.check(ip).is_ok());
        assert!(limiter.check(ip).is_ok());
        let retry = limiter.check(ip).expect_err("第三次必须被限流");
        assert!(retry >= 1);
    }

    #[test]
    fn per_user_write_limiter_is_keyed_by_user() {
        let limiter = UserWriteRateLimiter::new(NonZeroU32::new(2).expect("2"));
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        assert!(limiter.check(first).is_ok());
        assert!(limiter.check(first).is_ok());
        assert!(
            limiter.check(first).is_err(),
            "同一用户的第三次写请求必须被限流"
        );
        // 另一个用户不受影响（按用户而不是全局计数）。
        assert!(limiter.check(second).is_ok());
    }

    fn test_auth_config() -> AuthConfig {
        AuthConfig {
            pepper_version: 1,
            pepper: Secret::new("pepper-value-0123456789abcdef"),
            old_peppers: Vec::new(),
            fake_salt_key: Secret::new("fake-salt-key-0123456789abcdef"),
        }
    }

    fn request_parts() -> Parts {
        Request::builder()
            .body(Body::empty())
            .expect("请求")
            .into_parts()
            .0
    }

    fn request_parts_with_peer(peer: IpAddr) -> Parts {
        let mut parts = request_parts();
        parts
            .extensions
            .insert(ConnectInfo(SocketAddr::new(peer, 12345)));
        parts
    }
}
