//! 用户账号集成测试（tasks 6.1–6.8）。
//!
//! 需要真实 PostgreSQL（testcontainers）；本机没有 Docker 时不会跳过，
//! 由审查者在 WSL 中运行。这里覆盖 user-accounts spec 的每个服务端场景。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU32;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use ipnet::IpNet;
use serde_json::{Value, json};
use sqlx::Row;
use tower::ServiceExt;
use uuid::Uuid;

use common::{environment, integration_config, migrated_database, static_fixture};

const PEPPER: &str = "test-pepper-value-0123456789abcdef";
const CSRF_HEADER: &str = "x-requested-with";
const CSRF_VALUE: &str = "backup-web";

struct TestApp {
    app: Router,
    database: server::Database,
    _dir: tempfile::TempDir,
}

async fn setup() -> TestApp {
    setup_with(60, Vec::new()).await
}

async fn setup_with(rate_limit: u32, trusted_proxies: Vec<IpNet>) -> TestApp {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let (dir, root) = static_fixture();
    let mut config = integration_config(env, root);
    config.auth_rate_limit_per_minute = NonZeroU32::new(rate_limit).expect("测试限流必须为正");
    config.trusted_proxies = trusted_proxies;
    let storage = server::Storage::new(&config.storage).expect("构造 storage");
    let app = server::build_router(server::AppState::new(config, database.clone(), storage));
    TestApp {
        app,
        database,
        _dir: dir,
    }
}

#[allow(clippy::too_many_arguments)]
async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: Option<&str>,
    csrf: bool,
    extra_headers: &[(&str, &str)],
    peer: Option<IpAddr>,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if csrf {
        builder = builder.header(CSRF_HEADER, CSRF_VALUE);
    }
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, format!("__Host-session={cookie}"));
    }
    for (name, value) in extra_headers {
        builder = builder.header(*name, *value);
    }
    let payload = body.map(|body| body.to_string()).unwrap_or_default();
    let mut request = builder.body(Body::from(payload)).expect("构造测试请求");
    if let Some(peer) = peer {
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::new(peer, 40000)));
    }
    app.clone().oneshot(request).await.expect("请求应当完成")
}

async fn read(response: axum::response::Response) -> (StatusCode, HeaderMap, Value) {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("读取响应体")
        .to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("响应体必须是 JSON")
    };
    (status, headers, body)
}

fn session_cookie(headers: &HeaderMap) -> String {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| {
            let first = value.split(';').next()?;
            first.strip_prefix("__Host-session=").map(str::to_owned)
        })
        .expect("响应必须设置 __Host-session Cookie")
}

fn hex(bytes: &[u8]) -> String {
    server::auth::encode_hex(bytes)
}

fn auth_key(seed: u8) -> [u8; 32] {
    [seed; 32]
}

fn recovery_key(seed: u8) -> [u8; 32] {
    [seed.wrapping_add(1); 32]
}

fn wrapped(seed: u8) -> String {
    let mut bytes = vec![crypto_core::CIPHERTEXT_FORMAT_VERSION; crypto_core::FRAME_OVERHEAD + 32];
    for byte in &mut bytes[1..] {
        *byte = seed;
    }
    hex(&bytes)
}

fn recommended_params() -> Value {
    json!({
        "algorithm": "argon2id",
        "m_cost_kib": 65536,
        "t_cost": 3,
        "p_cost": 1,
    })
}

fn register_body(email: &str, auth: [u8; 32], recovery: [u8; 32]) -> Value {
    json!({
        "email": email,
        "kdf_params": recommended_params(),
        "kdf_salt": hex(&[0x11; 16]),
        "auth_key": hex(&auth),
        "wrapped_mk": wrapped(0x22),
        "recovery_wrapped_mk": wrapped(0x33),
        "recovery_auth_key": hex(&recovery),
    })
}

async fn register(
    app: &TestApp,
    email: &str,
    auth: [u8; 32],
    recovery: [u8; 32],
) -> (StatusCode, Value) {
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/register",
        Some(register_body(email, auth, recovery)),
        None,
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    (status, body)
}

async fn login_from(app: &TestApp, peer: IpAddr, forwarded: &str) -> axum::response::Response {
    let headers = [("x-forwarded-for", forwarded)];
    send(
        &app.app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({
            "email": "missing@example.com",
            "auth_key": hex(&auth_key(0x10)),
        })),
        None,
        true,
        &headers,
        Some(peer),
    )
    .await
}

async fn login(app: &TestApp, email: &str, auth: [u8; 32]) -> (StatusCode, HeaderMap, Value) {
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({ "email": email, "auth_key": hex(&auth) })),
        None,
        true,
        &[],
        None,
    )
    .await;
    read(response).await
}

#[derive(Debug, Clone)]
struct UserSnapshot {
    id: Uuid,
    verified: bool,
    auth_hash: String,
    pepper_version: i32,
    recovery_auth_hash: String,
    recovery_pepper_version: i32,
    kdf_params: String,
    kdf_salt: String,
    wrapped_mk: String,
    recovery_wrapped_mk: String,
    failed_logins: i32,
    locked: bool,
}

async fn snapshot(app: &TestApp, email: &str) -> UserSnapshot {
    let row = sqlx::query(
        r#"
        SELECT id,
               email_verified_at IS NOT NULL AS verified,
               encode(auth_hash, 'hex') AS auth_hash,
               pepper_version,
               encode(recovery_auth_hash, 'hex') AS recovery_auth_hash,
               recovery_pepper_version,
               kdf_params::text AS kdf_params,
               encode(kdf_salt, 'hex') AS kdf_salt,
               encode(wrapped_mk, 'hex') AS wrapped_mk,
               encode(recovery_wrapped_mk, 'hex') AS recovery_wrapped_mk,
               failed_logins,
               locked_until IS NOT NULL AS locked
        FROM users
        WHERE email = $1::text::citext
        "#,
    )
    .bind(email)
    .fetch_one(app.database.pool())
    .await
    .expect("读取用户");
    UserSnapshot {
        id: row.get("id"),
        verified: row.get("verified"),
        auth_hash: row.get("auth_hash"),
        pepper_version: row.get("pepper_version"),
        recovery_auth_hash: row.get("recovery_auth_hash"),
        recovery_pepper_version: row.get("recovery_pepper_version"),
        kdf_params: row.get("kdf_params"),
        kdf_salt: row.get("kdf_salt"),
        wrapped_mk: row.get("wrapped_mk"),
        recovery_wrapped_mk: row.get("recovery_wrapped_mk"),
        failed_logins: row.get("failed_logins"),
        locked: row.get("locked"),
    }
}

async fn outbox_count(app: &TestApp, user_id: Uuid, purpose: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE user_id = $1 AND purpose = $2")
        .bind(user_id)
        .bind(purpose)
        .fetch_one(app.database.pool())
        .await
        .expect("统计 outbox")
}

async fn insert_email_token(
    app: &TestApp,
    user_id: Uuid,
    token: &str,
    expires_in: &str,
    used: bool,
) {
    let hash = server::auth::hash_token(token);
    sqlx::query(
        r#"
        INSERT INTO email_tokens (user_id, purpose, token_hash, expires_at, used_at)
        VALUES ($1, 'verify_email', $2, now() + $3::interval, CASE WHEN $4 THEN now() ELSE NULL END)
        "#,
    )
    .bind(user_id)
    .bind(hash)
    .bind(expires_in)
    .bind(used)
    .execute(app.database.pool())
    .await
    .expect("写入邮件令牌");
}

async fn insert_session(
    app: &TestApp,
    user_id: Uuid,
    token: &str,
    last_seen_offset: &str,
    expires_offset: &str,
    revoked: bool,
) {
    let hash = server::auth::hash_token(token);
    sqlx::query(
        r#"
        INSERT INTO sessions (id, user_id, token_hash, last_seen_at, expires_at, revoked_at, ip)
        VALUES ($1, $2, $3, now() + $4::interval, now() + $5::interval,
                CASE WHEN $6 THEN now() ELSE NULL END, '127.0.0.1')
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(hash)
    .bind(last_seen_offset)
    .bind(expires_offset)
    .bind(revoked)
    .execute(app.database.pool())
    .await
    .expect("写入会话");
}

fn extract_token_from_mail(body: &str) -> String {
    // 邮件正文经过 quoted-printable 编码，`#token=` 的 `=` 可能变成 `=3D`，
    // 因此按“连续 64 位十六进制串”提取令牌。
    body.split(|character: char| !character.is_ascii_hexdigit())
        .find(|run| run.len() >= 64)
        .map(|run| run[..64].to_owned())
        .expect("邮件正文中应包含 64 位十六进制令牌")
}

// ---------------------------------------------------------------------------
// 注册
// ---------------------------------------------------------------------------

#[tokio::test]
async fn registration_creates_pending_user_and_queues_verification_email() {
    let app = setup().await;
    let recovery = recovery_key(0x40);
    let (status, body) = register(&app, "user@example.com", auth_key(0x10), recovery).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true }));

    let user = snapshot(&app, "user@example.com").await;
    assert!(!user.verified);
    assert_eq!(
        user.auth_hash,
        hex(&server::auth::hmac_sha256(PEPPER.as_bytes(), &auth_key(0x10),).expect("HMAC"))
    );
    assert_eq!(
        user.recovery_auth_hash,
        hex(&server::auth::hmac_sha256(PEPPER.as_bytes(), &recovery).expect("HMAC"))
    );
    assert_eq!(user.pepper_version, 1);
    assert_eq!(user.recovery_pepper_version, 1);
    assert_eq!(outbox_count(&app, user.id, "verify_email").await, 1);

    // 令牌在发送时才生成；outbox 中只有非敏感参数。
    let mail_dir = tempfile::tempdir().expect("邮件目录");
    let mailer = server::mail::build_mailer(&server::config::MailConfig::File {
        dir: mail_dir.path().to_path_buf(),
    })
    .expect("构造 file mailer");
    let resolver: std::sync::Arc<dyn server::jobs::email::RecipientResolver> =
        std::sync::Arc::new(server::jobs::email::SqlRecipientResolver);
    let stats = server::jobs::email::run_batch(
        &app.database,
        &mailer,
        &resolver,
        "http://127.0.0.1:8080",
        10,
    )
    .await
    .expect("发送验证邮件");
    assert_eq!(stats.sent, 1);

    let file = std::fs::read_dir(mail_dir.path())
        .expect("读取邮件目录")
        .next()
        .expect("应有一封邮件")
        .expect("目录项");
    let content = std::fs::read_to_string(file.path()).expect("读取邮件");
    assert!(
        content.contains("#token=") || content.contains("#token=3D"),
        "验证链接必须把令牌放在 URL 片段里: {content}"
    );
    let token = extract_token_from_mail(&content);
    assert_eq!(token.len(), 64);

    let stored: String =
        sqlx::query_scalar("SELECT encode(token_hash, 'hex') FROM email_tokens WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(app.database.pool())
            .await
            .expect("读取令牌哈希");
    assert_ne!(stored, token, "数据库只能保存令牌哈希");
    assert_eq!(stored.len(), 64);
}

#[tokio::test]
async fn duplicate_registration_returns_same_response_and_queues_existing_account_mail() {
    let app = setup().await;
    let (first_status, first_body) =
        register(&app, "dupe@example.com", auth_key(0x10), recovery_key(0x20)).await;
    assert_eq!(first_status, StatusCode::OK);
    let before = snapshot(&app, "dupe@example.com").await;

    let (second_status, second_body) =
        register(&app, "DUPE@example.com", auth_key(0x11), recovery_key(0x21)).await;
    assert_eq!(second_status, first_status);
    assert_eq!(second_body, first_body);

    let users: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE email = $1::text::citext")
            .bind("dupe@example.com")
            .fetch_one(app.database.pool())
            .await
            .expect("统计用户");
    assert_eq!(users, 1, "重复注册不得创建新账号");

    let after = snapshot(&app, "dupe@example.com").await;
    assert_eq!(
        before.auth_hash, after.auth_hash,
        "重复注册不得覆盖认证材料"
    );
    assert_eq!(outbox_count(&app, after.id, "verify_email").await, 1);
    assert_eq!(outbox_count(&app, after.id, "existing_account").await, 1);

    // 账号维度限流：紧接着的第二次重复注册仍返回相同响应，但不重复发信。
    let (third_status, third_body) =
        register(&app, "dupe@example.com", auth_key(0x12), recovery_key(0x22)).await;
    assert_eq!(third_status, first_status);
    assert_eq!(third_body, first_body);
    assert_eq!(
        outbox_count(&app, after.id, "existing_account").await,
        1,
        "10 分钟内不得重复发送已有账号邮件"
    );
}

#[tokio::test]
async fn registration_rejects_plaintext_password_field() {
    let app = setup().await;
    let mut body = register_body("user@example.com", auth_key(0x10), recovery_key(0x20));
    body["password"] = json!("password1234");
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/register",
        Some(body),
        None,
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_request");
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(app.database.pool())
        .await
        .expect("统计用户");
    assert_eq!(users, 0);
}

// ---------------------------------------------------------------------------
// prelogin
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prelogin_for_unknown_email_is_stable_and_matches_shape() {
    let app = setup().await;
    let request = || {
        send(
            &app.app,
            Method::POST,
            "/api/v1/auth/prelogin",
            Some(json!({ "email": "missing@example.com" })),
            None,
            true,
            &[],
            None,
        )
    };
    let (first_status, _first_headers, first) = read(request().await).await;
    let (second_status, _second_headers, second) = read(request().await).await;
    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(first, second, "伪造盐必须确定性生成");
    assert_eq!(
        first["kdf_params"],
        json!({
            "algorithm": "argon2id",
            "m_cost_kib": 65536,
            "t_cost": 3,
            "p_cost": 1,
        })
    );
    assert_eq!(first["kdf_salt"].as_str().expect("盐").len(), 32);

    // 真实账号的结构完全相同（只有值不同）。
    register(&app, "real@example.com", auth_key(0x10), recovery_key(0x20)).await;
    let (real_status, _headers, real) = read(
        send(
            &app.app,
            Method::POST,
            "/api/v1/auth/prelogin",
            Some(json!({ "email": "REAL@example.com" })),
            None,
            true,
            &[],
            None,
        )
        .await,
    )
    .await;
    assert_eq!(real_status, StatusCode::OK);
    let first_keys: Vec<&String> = first.as_object().expect("对象").keys().collect();
    let real_keys: Vec<&String> = real.as_object().expect("对象").keys().collect();
    assert_eq!(first_keys, real_keys);
}

// ---------------------------------------------------------------------------
// 邮箱验证与重发
// ---------------------------------------------------------------------------

#[tokio::test]
async fn verify_email_marks_user_verified_and_token_is_single_use() {
    let app = setup().await;
    register(
        &app,
        "verify@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let user = snapshot(&app, "verify@example.com").await;
    let token = "ab".repeat(32);
    insert_email_token(&app, user.id, &token, "24 hours", false).await;

    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/verify-email",
        Some(json!({ "token": token })),
        None,
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true }));
    assert!(snapshot(&app, "verify@example.com").await.verified);

    let used: bool =
        sqlx::query_scalar("SELECT used_at IS NOT NULL FROM email_tokens WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(app.database.pool())
            .await
            .expect("读取令牌状态");
    assert!(used);

    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/verify-email",
        Some(json!({ "token": token })),
        None,
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_token");
}

#[tokio::test]
async fn verify_email_rejects_expired_and_used_tokens() {
    let app = setup().await;
    register(
        &app,
        "expired@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let user = snapshot(&app, "expired@example.com").await;

    let expired = "cd".repeat(32);
    insert_email_token(&app, user.id, &expired, "-1 hour", false).await;
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/verify-email",
        Some(json!({ "token": expired })),
        None,
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_token");

    let used = "ef".repeat(32);
    insert_email_token(&app, user.id, &used, "24 hours", true).await;
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/verify-email",
        Some(json!({ "token": used })),
        None,
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_token");
    assert!(!snapshot(&app, "expired@example.com").await.verified);
}

#[tokio::test]
async fn resend_verification_is_uniform_and_throttled_per_account() {
    let app = setup().await;
    register(
        &app,
        "resend@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let user = snapshot(&app, "resend@example.com").await;
    // 注册邮件还在节流窗口内；把它回拨到 20 分钟前，单独验证重发接口的节流。
    sqlx::query(
        "UPDATE email_outbox SET created_at = now() - interval '20 minutes' WHERE user_id = $1",
    )
    .bind(user.id)
    .execute(app.database.pool())
    .await
    .expect("回拨 outbox 时间");

    let request = |email: &str| {
        send(
            &app.app,
            Method::POST,
            "/api/v1/auth/resend-verification",
            Some(json!({ "email": email })),
            None,
            true,
            &[],
            None,
        )
    };
    let (first_status, _headers, first) = read(request("resend@example.com").await).await;
    let (second_status, _headers, second) = read(request("resend@example.com").await).await;
    let (unknown_status, _headers, unknown) = read(request("nobody@example.com").await).await;
    assert_eq!(first_status, StatusCode::OK);
    assert_eq!(second_status, StatusCode::OK);
    assert_eq!(unknown_status, StatusCode::OK);
    assert_eq!(first, second);
    assert_eq!(first, unknown, "响应不得泄露邮箱是否存在");
    assert_eq!(
        outbox_count(&app, user.id, "verify_email").await,
        2,
        "注册时一封 + 首次重发一封；第二次被账号维度节流"
    );
}

// ---------------------------------------------------------------------------
// 登录
// ---------------------------------------------------------------------------

#[tokio::test]
async fn login_sets_host_cookie_and_returns_wrapped_mk() {
    let app = setup().await;
    register(
        &app,
        "login@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let user = snapshot(&app, "login@example.com").await;

    let (status, headers, body) = login(&app, "login@example.com", auth_key(0x10)).await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with("__Host-session="))
        .expect("必须设置 __Host-session");
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("Secure"));
    assert!(cookie.contains("SameSite=Strict"));
    assert!(cookie.contains("Path=/"));
    assert_eq!(body["user_id"], json!(user.id));
    assert_eq!(body["wrapped_mk"], json!(wrapped(0x22)));
    assert_eq!(body["kdf_salt"], json!(hex(&[0x11; 16])));
    assert_eq!(body["kdf_params"], recommended_params());

    let token = session_cookie(&headers);
    let body_text = body.to_string();
    assert!(!body_text.contains(&token), "响应不得泄露会话令牌");
    assert!(!body_text.contains("token_hash"));
    let stored: String =
        sqlx::query_scalar("SELECT encode(token_hash, 'hex') FROM sessions WHERE user_id = $1")
            .bind(user.id)
            .fetch_one(app.database.pool())
            .await
            .expect("读取会话哈希");
    assert_ne!(stored, token, "数据库只能保存会话令牌哈希");

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&token),
        false,
        &[],
        None,
    )
    .await;
    let (status, _headers, me) = read(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["wrapped_mk"], json!(wrapped(0x22)));
    assert_eq!(me["user_id"], json!(user.id));
}

#[tokio::test]
async fn login_rejects_unknown_email_and_bad_auth_key_with_same_error() {
    let app = setup().await;
    register(
        &app,
        "known@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;

    let (unknown_status, _headers, unknown) =
        login(&app, "missing@example.com", auth_key(0x10)).await;
    let (bad_status, _headers, bad) = login(&app, "known@example.com", auth_key(0x99)).await;
    assert_eq!(unknown_status, StatusCode::UNAUTHORIZED);
    assert_eq!(bad_status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown["code"], "invalid_credentials");
    assert_eq!(bad["code"], "invalid_credentials");
    assert_eq!(unknown["message"], bad["message"]);

    let user = snapshot(&app, "known@example.com").await;
    assert_eq!(user.failed_logins, 1);
}

#[tokio::test]
async fn login_locks_account_after_ten_failures() {
    let app = setup().await;
    register(
        &app,
        "locked@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;

    for _ in 0..10 {
        let (status, _headers, body) = login(&app, "locked@example.com", auth_key(0x99)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    }
    let user = snapshot(&app, "locked@example.com").await;
    assert_eq!(user.failed_logins, 10);
    assert!(user.locked, "连续 10 次失败后必须锁定");

    let (status, headers, body) = login(&app, "locked@example.com", auth_key(0x10)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "account_temporarily_locked");
    assert!(headers.contains_key(header::RETRY_AFTER));

    // 锁定过期后失败计数清零，正确凭据可以登录。
    sqlx::query(
        "UPDATE users SET locked_until = now() - interval '1 minute' WHERE email = $1::text::citext",
    )
    .bind("locked@example.com")
    .execute(app.database.pool())
    .await
    .expect("使锁定过期");
    let (status, _headers, _body) = login(&app, "locked@example.com", auth_key(0x10)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot(&app, "locked@example.com").await.failed_logins, 0);
}

#[tokio::test]
async fn login_rate_limit_returns_429_and_retry_after() {
    let app = setup_with(2, Vec::new()).await;
    for _ in 0..2 {
        let (status, _headers, _body) = login(&app, "nobody@example.com", auth_key(0x10)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, headers, body) = login(&app, "nobody@example.com", auth_key(0x10)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rate_limited");
    assert!(headers.contains_key(header::RETRY_AFTER));
}

#[tokio::test]
async fn forwarded_for_is_used_only_from_trusted_proxies() {
    let app = setup_with(1, vec!["10.0.0.0/8".parse::<IpNet>().expect("代理网段")]).await;
    let untrusted: IpAddr = "198.51.100.5".parse().expect("IP");
    let trusted: IpAddr = "10.0.0.5".parse().expect("IP");

    // 不可信对端：X-Forwarded-For 被忽略，同一对端的第二次请求被限流。
    let (first, _headers, _body) = read(login_from(&app, untrusted, "203.0.113.1").await).await;
    assert_eq!(first, StatusCode::UNAUTHORIZED);
    let (second, headers, body) = read(login_from(&app, untrusted, "203.0.113.2").await).await;
    assert_eq!(second, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rate_limited");
    assert!(headers.contains_key(header::RETRY_AFTER));

    // 可信对端：不同 X-Forwarded-For 客户端使用不同桶。
    let (third, _headers, _body) = read(login_from(&app, trusted, "203.0.113.9").await).await;
    assert_eq!(third, StatusCode::UNAUTHORIZED, "新客户端不应被前一桶限流");
}

// ---------------------------------------------------------------------------
// 会话
// ---------------------------------------------------------------------------

#[tokio::test]
async fn logout_revokes_session_and_clears_cookie() {
    let app = setup().await;
    register(
        &app,
        "logout@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let (status, headers, _body) = login(&app, "logout@example.com", auth_key(0x10)).await;
    assert_eq!(status, StatusCode::OK);
    let token = session_cookie(&headers);

    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/logout",
        None,
        Some(&token),
        true,
        &[],
        None,
    )
    .await;
    let (status, headers, body) = read(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true }));
    let cleared = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with("__Host-session="))
        .expect("必须清除 Cookie");
    assert!(cleared.contains("Max-Age=0"));

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&token),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::UNAUTHORIZED);

    let revoked: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NOT NULL FROM sessions WHERE encode(token_hash,'hex') = $1",
    )
    .bind(server::auth::encode_hex(&server::auth::hash_token(&token)))
    .fetch_one(app.database.pool())
    .await
    .expect("读取会话");
    assert!(revoked);
}

#[tokio::test]
async fn revoke_other_session_makes_it_unauthenticated() {
    let app = setup().await;
    register(
        &app,
        "sessions@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let (_status, first_headers, _body) = login(&app, "sessions@example.com", auth_key(0x10)).await;
    let first = session_cookie(&first_headers);
    let (_status, second_headers, _body) =
        login(&app, "sessions@example.com", auth_key(0x10)).await;
    let second = session_cookie(&second_headers);

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/sessions",
        None,
        Some(&first),
        false,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::OK);
    let sessions = body["sessions"].as_array().expect("会话数组");
    assert_eq!(sessions.len(), 2);
    assert_eq!(
        sessions
            .iter()
            .filter(|session| session["current"] == json!(true))
            .count(),
        1
    );
    let other = sessions
        .iter()
        .find(|session| session["current"] == json!(false))
        .expect("另一个会话")["id"]
        .as_str()
        .expect("会话 ID")
        .to_owned();

    let response = send(
        &app.app,
        Method::DELETE,
        &format!("/api/v1/auth/sessions/{other}"),
        None,
        Some(&first),
        true,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::OK);

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&second),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::UNAUTHORIZED);

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&first),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::OK);

    // 撤销别人的会话不可区分于不存在。
    register(
        &app,
        "other@example.com",
        auth_key(0x30),
        recovery_key(0x40),
    )
    .await;
    let (_status, other_headers, _body) = login(&app, "other@example.com", auth_key(0x30)).await;
    let other_cookie = session_cookie(&other_headers);
    let response = send(
        &app.app,
        Method::DELETE,
        &format!("/api/v1/auth/sessions/{other}"),
        None,
        Some(&other_cookie),
        true,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn state_changing_request_without_custom_header_is_forbidden() {
    let app = setup().await;
    register(&app, "csrf@example.com", auth_key(0x10), recovery_key(0x20)).await;
    let (_status, headers, _body) = login(&app, "csrf@example.com", auth_key(0x10)).await;
    let token = session_cookie(&headers);

    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/logout",
        None,
        Some(&token),
        false,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "csrf_rejected");

    // 会话未被撤销。
    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&token),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::OK);
}

#[tokio::test]
async fn sessions_expire_when_idle_or_past_absolute_deadline() {
    let app = setup().await;
    register(
        &app,
        "expiry@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let user = snapshot(&app, "expiry@example.com").await;

    let idle = "11".repeat(32);
    insert_session(&app, user.id, &idle, "-8 days", "30 days", false).await;
    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&idle),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::UNAUTHORIZED);

    let absolute = "22".repeat(32);
    insert_session(&app, user.id, &absolute, "0 seconds", "-1 second", false).await;
    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&absolute),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn last_seen_is_written_at_most_once_per_five_minutes() {
    let app = setup().await;
    register(&app, "seen@example.com", auth_key(0x10), recovery_key(0x20)).await;
    let (_status, headers, _body) = login(&app, "seen@example.com", auth_key(0x10)).await;
    let token = session_cookie(&headers);
    let hash = server::auth::encode_hex(&server::auth::hash_token(&token));

    sqlx::query(
        "UPDATE sessions SET last_seen_at = now() - interval '10 minutes' WHERE encode(token_hash,'hex') = $1",
    )
    .bind(&hash)
    .execute(app.database.pool())
    .await
    .expect("回拨 last_seen_at");

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&token),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::OK);
    let first: String = sqlx::query_scalar(
        "SELECT last_seen_at::text FROM sessions WHERE encode(token_hash,'hex') = $1",
    )
    .bind(&hash)
    .fetch_one(app.database.pool())
    .await
    .expect("读取 last_seen_at");

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&token),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::OK);
    let second: String = sqlx::query_scalar(
        "SELECT last_seen_at::text FROM sessions WHERE encode(token_hash,'hex') = $1",
    )
    .bind(&hash)
    .fetch_one(app.database.pool())
    .await
    .expect("读取 last_seen_at");
    assert_eq!(first, second, "5 分钟内不应重复写 last_seen_at");
}

// ---------------------------------------------------------------------------
// 修改密码 / 恢复重置 / 轮换恢复密钥
// ---------------------------------------------------------------------------

fn new_password_body() -> Value {
    json!({
        "new_auth_key": hex(&auth_key(0xaa)),
        "new_kdf_params": recommended_params(),
        "new_kdf_salt": hex(&[0x55; 16]),
        "new_wrapped_mk": wrapped(0x66),
    })
}

#[tokio::test]
async fn change_password_replaces_materials_revokes_other_sessions_and_notifies() {
    let app = setup().await;
    register(
        &app,
        "change@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let (_status, first_headers, _body) = login(&app, "change@example.com", auth_key(0x10)).await;
    let first = session_cookie(&first_headers);
    let (_status, second_headers, _body) = login(&app, "change@example.com", auth_key(0x10)).await;
    let second = session_cookie(&second_headers);

    let mut body = new_password_body();
    body["old_auth_key"] = json!(hex(&auth_key(0x10)));
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/change-password",
        Some(body),
        Some(&first),
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let user = snapshot(&app, "change@example.com").await;
    assert_eq!(
        user.auth_hash,
        hex(&server::auth::hmac_sha256(PEPPER.as_bytes(), &auth_key(0xaa)).expect("HMAC"))
    );
    assert_eq!(user.kdf_salt, hex(&[0x55; 16]));
    assert_eq!(user.wrapped_mk, wrapped(0x66));
    assert_eq!(outbox_count(&app, user.id, "password_changed").await, 1);

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&second),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::UNAUTHORIZED);
    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&first),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::OK);

    assert_eq!(
        login(&app, "change@example.com", auth_key(0x10)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "change@example.com", auth_key(0xaa)).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn change_password_with_wrong_old_key_keeps_data_unchanged() {
    let app = setup().await;
    register(
        &app,
        "wrong-old@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let (_status, headers, _body) = login(&app, "wrong-old@example.com", auth_key(0x10)).await;
    let cookie = session_cookie(&headers);
    let before = snapshot(&app, "wrong-old@example.com").await;

    let mut body = new_password_body();
    body["old_auth_key"] = json!(hex(&auth_key(0x99)));
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/change-password",
        Some(body),
        Some(&cookie),
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "invalid_credentials");

    let after = snapshot(&app, "wrong-old@example.com").await;
    assert_eq!(before.auth_hash, after.auth_hash);
    assert_eq!(before.kdf_params, after.kdf_params);
    assert_eq!(before.kdf_salt, after.kdf_salt);
    assert_eq!(before.wrapped_mk, after.wrapped_mk);
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sessions WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(after.id)
    .fetch_one(app.database.pool())
    .await
    .expect("统计会话");
    assert_eq!(active, 1, "失败时不得撤销会话");
}

fn recovery_body(email: &str, recovery: [u8; 32]) -> Value {
    json!({
        "email": email,
        "recovery_auth_key": hex(&recovery),
        "new_auth_key": hex(&auth_key(0xaa)),
        "new_kdf_params": recommended_params(),
        "new_kdf_salt": hex(&[0x55; 16]),
        "new_wrapped_mk": wrapped(0x66),
    })
}

#[tokio::test]
async fn recovery_reset_allows_new_password_and_revokes_sessions() {
    let app = setup().await;
    register(
        &app,
        "recover@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let (_status, headers, _body) = login(&app, "recover@example.com", auth_key(0x10)).await;
    let cookie = session_cookie(&headers);

    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/recover",
        Some(recovery_body("recover@example.com", recovery_key(0x20))),
        None,
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&cookie),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::UNAUTHORIZED);

    assert_eq!(
        login(&app, "recover@example.com", auth_key(0x10)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "recover@example.com", auth_key(0xaa)).await.0,
        StatusCode::OK
    );
    let user = snapshot(&app, "recover@example.com").await;
    assert_eq!(user.failed_logins, 0);
    assert!(!user.locked);
    assert_eq!(outbox_count(&app, user.id, "password_changed").await, 1);
}

#[tokio::test]
async fn recovery_with_wrong_key_leaves_materials_unchanged_and_counts_failure() {
    let app = setup().await;
    register(
        &app,
        "bad-recover@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let (_status, headers, _body) = login(&app, "bad-recover@example.com", auth_key(0x10)).await;
    let cookie = session_cookie(&headers);
    let before = snapshot(&app, "bad-recover@example.com").await;

    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/recover",
        Some(recovery_body("bad-recover@example.com", recovery_key(0x99))),
        None,
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "invalid_recovery_key");

    let after = snapshot(&app, "bad-recover@example.com").await;
    assert_eq!(before.auth_hash, after.auth_hash);
    assert_eq!(before.recovery_auth_hash, after.recovery_auth_hash);
    assert_eq!(before.wrapped_mk, after.wrapped_mk);
    assert_eq!(after.failed_logins, 1, "失败必须计入账号维度限流");
    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&cookie),
        false,
        &[],
        None,
    )
    .await;
    assert_eq!(read(response).await.0, StatusCode::OK, "失败不得撤销会话");
}

#[tokio::test]
async fn rotate_recovery_key_replaces_material_and_invalidates_old_key() {
    let app = setup().await;
    register(
        &app,
        "rotate@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let (_status, headers, _body) = login(&app, "rotate@example.com", auth_key(0x10)).await;
    let cookie = session_cookie(&headers);

    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/rotate-recovery-key",
        Some(json!({
            "new_recovery_wrapped_mk": wrapped(0x77),
            "new_recovery_auth_key": hex(&recovery_key(0x30)),
        })),
        Some(&cookie),
        true,
        &[],
        None,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let user = snapshot(&app, "rotate@example.com").await;
    assert_eq!(user.recovery_wrapped_mk, wrapped(0x77));
    assert_eq!(
        user.recovery_auth_hash,
        hex(&server::auth::hmac_sha256(PEPPER.as_bytes(), &recovery_key(0x30)).expect("HMAC"))
    );
    assert_eq!(outbox_count(&app, user.id, "recovery_key_changed").await, 1);

    let old = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/recover",
        Some(recovery_body("rotate@example.com", recovery_key(0x20))),
        None,
        true,
        &[],
        None,
    )
    .await;
    assert_eq!(read(old).await.0, StatusCode::UNAUTHORIZED);

    let new = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/recover",
        Some(recovery_body("rotate@example.com", recovery_key(0x30))),
        None,
        true,
        &[],
        None,
    )
    .await;
    assert_eq!(read(new).await.0, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// 清理任务
// ---------------------------------------------------------------------------

#[tokio::test]
async fn expired_token_cleanup_removes_expired_sessions_and_used_tokens() {
    let app = setup().await;
    register(
        &app,
        "cleanup@example.com",
        auth_key(0x10),
        recovery_key(0x20),
    )
    .await;
    let user = snapshot(&app, "cleanup@example.com").await;

    insert_session(
        &app,
        user.id,
        &"aa".repeat(32),
        "0 seconds",
        "-1 second",
        false,
    )
    .await;
    insert_session(
        &app,
        user.id,
        &"bb".repeat(32),
        "0 seconds",
        "30 days",
        true,
    )
    .await;
    insert_session(
        &app,
        user.id,
        &"cc".repeat(32),
        "0 seconds",
        "30 days",
        false,
    )
    .await;
    insert_email_token(&app, user.id, &"dd".repeat(32), "-1 hour", false).await;
    insert_email_token(&app, user.id, &"ee".repeat(32), "24 hours", true).await;
    insert_email_token(&app, user.id, &"ff".repeat(32), "24 hours", false).await;

    let outcome =
        server::jobs::scan::run_once(&app.database, &server::jobs::scan::ExpiredTokenCleanup, 100)
            .await
            .expect("运行清理任务");
    assert_eq!(
        outcome,
        server::jobs::scan::ScanOutcome::Executed(4),
        "2 个应清理的会话 + 2 个应清理的令牌"
    );

    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(app.database.pool())
        .await
        .expect("统计会话");
    assert_eq!(sessions, 1, "只保留活跃会话");
    let tokens: i64 = sqlx::query_scalar("SELECT count(*) FROM email_tokens WHERE user_id = $1")
        .bind(user.id)
        .fetch_one(app.database.pool())
        .await
        .expect("统计令牌");
    assert_eq!(tokens, 1, "只保留未过期且未使用的令牌");
}
