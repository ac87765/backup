//! 管理接口集成测试（add-admin-console task 3.6）。
//!
//! 覆盖 admin-console spec 的全部服务端场景：访问控制、12 小时重新登录、
//! 概览、用户管理、额度、状态机幂等、同事务审计、注册设置与审计筛选。
//! 需要 Docker（testcontainers），由审查者在 WSL 中运行。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::net::IpAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

use common::{environment, integration_config, migrated_database, static_fixture};

const CSRF_HEADER: &str = "x-requested-with";
const CSRF_VALUE: &str = "backup-web";
const GIB: i64 = 1024 * 1024 * 1024;

struct TestApp {
    app: Router,
    database: server::Database,
    _dir: tempfile::TempDir,
}

async fn setup() -> TestApp {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let (dir, root) = static_fixture();
    let mut config = integration_config(env, root);
    // 管理测试会连续注册多个账号，放宽认证 IP 限流；注册限流仍是固定的 5/小时。
    config.auth_rate_limit_per_minute = std::num::NonZeroU32::new(600).expect("限流");
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
    let payload = body.map(|body| body.to_string()).unwrap_or_default();
    let mut request = builder.body(Body::from(payload)).expect("构造测试请求");
    request
        .extensions_mut()
        .insert(ConnectInfo(std::net::SocketAddr::new(
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            40000,
        )));
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

fn wrapped(seed: u8) -> String {
    let mut bytes = vec![crypto_core::CIPHERTEXT_FORMAT_VERSION; crypto_core::FRAME_OVERHEAD + 32];
    for byte in &mut bytes[1..] {
        *byte = seed;
    }
    hex(&bytes)
}

fn auth_key(seed: u8) -> [u8; 32] {
    [seed; 32]
}

fn register_body(email: &str, seed: u8) -> Value {
    json!({
        "email": email,
        "kdf_params": {
            "algorithm": "argon2id",
            "m_cost_kib": 65536,
            "t_cost": 3,
            "p_cost": 1,
        },
        "kdf_salt": hex(&[0x11; 16]),
        "auth_key": hex(&auth_key(seed)),
        "wrapped_mk": wrapped(seed.wrapping_add(1)),
        "recovery_wrapped_mk": wrapped(seed.wrapping_add(2)),
        "recovery_auth_key": hex(&auth_key(seed.wrapping_add(3))),
    })
}

async fn register(app: &TestApp, email: &str, seed: u8) -> (StatusCode, Value) {
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/register",
        Some(register_body(email, seed)),
        None,
        true,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    (status, body)
}

async fn login(app: &TestApp, email: &str, seed: u8) -> (StatusCode, HeaderMap, Value) {
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/login",
        Some(json!({ "email": email, "auth_key": hex(&auth_key(seed)) })),
        None,
        true,
    )
    .await;
    read(response).await
}

/// 注册、登录并（可选）授予管理员，返回 (用户 ID, 会话 Cookie)。
async fn user_session(app: &TestApp, email: &str, seed: u8, admin: bool) -> (Uuid, String) {
    let (status, body) = register(app, email, seed).await;
    assert_eq!(status, StatusCode::OK, "注册失败: {body}");
    let (status, headers, body) = login(app, email, seed).await;
    assert_eq!(status, StatusCode::OK, "登录失败: {body}");
    let user_id = body["user_id"]
        .as_str()
        .expect("user_id")
        .parse()
        .expect("UUID");
    if admin {
        sqlx::query(
            "UPDATE users SET is_admin = true, status = 'active', \
             approved_at = COALESCE(approved_at, now()) WHERE id = $1",
        )
        .bind(user_id)
        .execute(app.database.pool())
        .await
        .expect("授予管理员");
    }
    (user_id, session_cookie(&headers))
}

async fn admin_request(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: &str,
) -> (StatusCode, Value) {
    let response = send(&app.app, method, uri, body, Some(cookie), true).await;
    let (status, _headers, body) = read(response).await;
    (status, body)
}

async fn audit_count(app: &TestApp, action: &str, target: Option<Uuid>) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM admin_audit_log \
         WHERE action = $1 AND ($2::uuid IS NULL OR target_user_id = $2)",
    )
    .bind(action)
    .bind(target)
    .fetch_one(app.database.pool())
    .await
    .expect("统计审计")
}

fn upload_body(root: Uuid) -> Value {
    json!({
        "content_id": Uuid::new_v4(),
        "kind": "new_file",
        "node_id": Uuid::new_v4(),
        "parent_id": root,
        "wrapped_key": wrapped(0x31),
        "encrypted_meta": wrapped(0x32),
        "name_token": hex(&[0x33; 32]),
        "version_id": Uuid::new_v4(),
        "version_meta": wrapped(0x34),
        "chunk_count": 1,
        "declared_size": 1024 * 1024,
    })
}

// ---------------------------------------------------------------------------
// 访问控制
// ---------------------------------------------------------------------------

#[tokio::test]
async fn normal_user_and_anonymous_get_404_from_admin_api() {
    let app = setup().await;
    let (user_id, cookie) = user_session(&app, "plain@example.com", 0x10, false).await;

    let (status, body) =
        admin_request(&app, Method::GET, "/api/v1/admin/users", None, &cookie).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");

    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/admin/users",
        None,
        None,
        false,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");

    // 写接口在未登录/非管理员时也必须先得到 404，而不是 CSRF 403。
    let response = send(
        &app.app,
        Method::POST,
        &format!("/api/v1/admin/users/{user_id}/suspend"),
        Some(json!({ "reason": "x" })),
        None,
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = send(
        &app.app,
        Method::POST,
        &format!("/api/v1/admin/users/{user_id}/suspend"),
        Some(json!({ "reason": "x" })),
        Some(&cookie),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_session_older_than_twelve_hours_requires_reauth() {
    let app = setup().await;
    let (admin_id, cookie) = user_session(&app, "old-admin@example.com", 0x10, true).await;
    sqlx::query(
        "UPDATE sessions SET created_at = now() - interval '13 hours' \
         WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(admin_id)
    .execute(app.database.pool())
    .await
    .expect("把会话改老");

    let (status, body) =
        admin_request(&app, Method::GET, "/api/v1/admin/overview", None, &cookie).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "reauth_required");
}

#[tokio::test]
async fn admin_cannot_suspend_or_reject_self() {
    let app = setup().await;
    let (admin_id, cookie) = user_session(&app, "self-admin@example.com", 0x10, true).await;
    let body = json!({ "reason": "自己" });

    let (status, body) = admin_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{admin_id}/suspend"),
        Some(body.clone()),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "cannot_modify_self");

    let (status, body) = admin_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{admin_id}/reject"),
        Some(body),
        &cookie,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "cannot_modify_self");
}

#[tokio::test]
async fn interface_cannot_grant_admin_flag() {
    let app = setup().await;
    let mut body = register_body("sneaky@example.com", 0x10);
    body["is_admin"] = json!(true);
    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/auth/register",
        Some(body),
        None,
        true,
    )
    .await;
    let (status, _headers, error) = read(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["code"], "invalid_request");
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE email = $1::text::citext")
            .bind("sneaky@example.com")
            .fetch_one(app.database.pool())
            .await
            .expect("统计用户");
    assert_eq!(count, 0, "包含禁止字段的注册不得创建账号");
}

// ---------------------------------------------------------------------------
// 概览与用户管理
// ---------------------------------------------------------------------------

#[tokio::test]
async fn overview_reports_pending_count_and_queues() {
    let app = setup().await;
    let (_admin_id, admin_cookie) =
        user_session(&app, "overview-admin@example.com", 0x10, true).await;
    for index in 0..3_u8 {
        register(&app, &format!("pending-{index}@example.com"), 0x20 + index).await;
    }

    let (status, body) = admin_request(
        &app,
        Method::GET,
        "/api/v1/admin/overview",
        None,
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["pending_users"], 3);
    assert_eq!(body["status_counts"]["pending"], 3);
    assert!(body["queues"]["email_outbox"].as_i64().is_some());
    assert!(body["queues"]["object_deletions"].as_i64().is_some());
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(body["health"]["status"].is_string());
}

#[tokio::test]
async fn approve_sets_quota_queues_email_and_is_idempotent() {
    let app = setup().await;
    let (_admin_id, admin_cookie) =
        user_session(&app, "approve-admin@example.com", 0x10, true).await;
    let (user_id, _user_cookie) = user_session(&app, "approve-me@example.com", 0x20, false).await;

    let quota = 50 * GIB;
    let (status, body) = admin_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{user_id}/approve"),
        Some(json!({ "quota_bytes": quota })),
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);

    let (status, detail) = admin_request(
        &app,
        Method::GET,
        &format!("/api/v1/admin/users/{user_id}"),
        None,
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["user"]["status"], "active");
    assert_eq!(detail["user"]["quota_bytes"], quota);
    assert!(detail["user"]["approved_at"].is_string());
    assert_eq!(audit_count(&app, "approve", Some(user_id)).await, 1);
    let mails: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_outbox WHERE user_id = $1 AND purpose = 'account_approved'",
    )
    .bind(user_id)
    .fetch_one(app.database.pool())
    .await
    .expect("统计邮件");
    assert_eq!(mails, 1);

    // 重复批准：200，且不重复审计/邮件。
    let (status, _body) = admin_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{user_id}/approve"),
        Some(json!({ "quota_bytes": 5 * GIB })),
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audit_count(&app, "approve", Some(user_id)).await, 1);
    let mails: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_outbox WHERE user_id = $1 AND purpose = 'account_approved'",
    )
    .bind(user_id)
    .fetch_one(app.database.pool())
    .await
    .expect("统计邮件");
    assert_eq!(mails, 1);
    let quota_after: i64 = sqlx::query_scalar("SELECT quota_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("读取额度");
    assert_eq!(quota_after, quota, "重复批准不得改写额度");
}

#[tokio::test]
async fn quota_below_used_is_allowed_with_hint_and_audited() {
    let app = setup().await;
    let (_admin_id, admin_cookie) = user_session(&app, "quota-admin@example.com", 0x10, true).await;
    let (user_id, _user_cookie) = user_session(&app, "quota-user@example.com", 0x20, false).await;
    sqlx::query("UPDATE users SET used_bytes = $2 WHERE id = $1")
        .bind(user_id)
        .bind(8 * GIB)
        .execute(app.database.pool())
        .await
        .expect("写入已用量");

    let (status, body) = admin_request(
        &app,
        Method::PUT,
        &format!("/api/v1/admin/users/{user_id}/quota"),
        Some(json!({ "quota_bytes": 5 * GIB })),
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["below_used"], true);
    let quota: i64 = sqlx::query_scalar("SELECT quota_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("读取额度");
    assert_eq!(quota, 5 * GIB);

    let audit = sqlx::query_scalar::<_, Value>(
        "SELECT details FROM admin_audit_log \
         WHERE action = 'update_quota' AND target_user_id = $1 \
         ORDER BY id DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_one(app.database.pool())
    .await
    .expect("读取审计");
    assert_eq!(audit["old_quota_bytes"], 10 * GIB);
    assert_eq!(audit["new_quota_bytes"], 5 * GIB);
}

#[tokio::test]
async fn user_list_filters_by_status_searches_escaped_email_and_paginates() {
    let app = setup().await;
    let (_admin_id, admin_cookie) = user_session(&app, "list-admin@example.com", 0x10, true).await;
    // `_` 必须按字面匹配，不能被 LIKE 当成通配符。
    register(&app, "a_b@example.com", 0x20).await;
    register(&app, "axb@example.com", 0x21).await;
    let (active_id, _cookie) = user_session(&app, "list-active@example.com", 0x22, false).await;
    admin_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{active_id}/approve"),
        Some(json!({})),
        &admin_cookie,
    )
    .await;

    let (status, body) = admin_request(
        &app,
        Method::GET,
        "/api/v1/admin/users?q=a_b",
        None,
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "搜索 a_b 只能匹配 a_b: {body}");
    assert_eq!(items[0]["email"], "a_b@example.com");

    let (status, body) = admin_request(
        &app,
        Method::GET,
        "/api/v1/admin/users?status=active",
        None,
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "管理员自己与刚批准的用户都是 active");
    assert!(items.iter().all(|item| item["status"] == "active"));

    let (status, first) = admin_request(
        &app,
        Method::GET,
        "/api/v1/admin/users?limit=1",
        None,
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cursor = first["next_cursor"].as_str().expect("next_cursor");
    let first_id = first["items"][0]["id"].as_str().expect("id");
    let (status, second) = admin_request(
        &app,
        Method::GET,
        &format!("/api/v1/admin/users?limit=1&cursor={cursor}"),
        None,
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(second["items"][0]["id"].as_str().expect("id"), first_id);
}

#[tokio::test]
async fn user_detail_exposes_only_metadata() {
    let app = setup().await;
    let (_admin_id, admin_cookie) =
        user_session(&app, "detail-admin@example.com", 0x10, true).await;
    let (user_id, _cookie) = user_session(&app, "detail-user@example.com", 0x20, false).await;

    let (status, body) = admin_request(
        &app,
        Method::GET,
        &format!("/api/v1/admin/users/{user_id}"),
        None,
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let serialized = body.to_string();
    for forbidden in [
        "encrypted_meta",
        "wrapped_key",
        "name_token",
        "file_name",
        "content_id",
    ] {
        assert!(
            !serialized.contains(forbidden),
            "用户详情不得包含 {forbidden}: {serialized}"
        );
    }
    assert!(body["sessions"].is_array());
    assert!(body["audit"].is_array());
}

// ---------------------------------------------------------------------------
// 停用与设置
// ---------------------------------------------------------------------------

#[tokio::test]
async fn suspend_revokes_sessions_aborts_uploads_and_releases_reservation() {
    let app = setup().await;
    let (_admin_id, admin_cookie) =
        user_session(&app, "suspend-admin@example.com", 0x10, true).await;
    let (user_id, user_cookie) = user_session(&app, "suspend-me@example.com", 0x20, false).await;
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(app.database.pool())
        .await
        .expect("验证邮箱");
    let root: Uuid = sqlx::query_scalar("SELECT root_node_id FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("根节点");

    let response = send(
        &app.app,
        Method::POST,
        "/api/v1/upload-sessions",
        Some(upload_body(root)),
        Some(&user_cookie),
        true,
    )
    .await;
    let (status, _headers, body) = read(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let reserved: i64 = sqlx::query_scalar("SELECT reserved_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("读取预留");
    assert!(reserved > 0);

    let (status, body) = admin_request(
        &app,
        Method::POST,
        &format!("/api/v1/admin/users/{user_id}/suspend"),
        Some(json!({ "reason": "滥用" })),
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let row = sqlx::query("SELECT status, status_reason, reserved_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("读取用户");
    use sqlx::Row as _;
    let status: String = row.get("status");
    let reason: Option<String> = row.get("status_reason");
    let reserved: i64 = row.get("reserved_bytes");
    assert_eq!(status, "suspended");
    assert_eq!(reason.as_deref(), Some("滥用"));
    assert_eq!(reserved, 0, "进行中上传的预留必须释放");
    let open: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM upload_sessions WHERE user_id = $1 AND status = 'open'",
    )
    .bind(user_id)
    .fetch_one(app.database.pool())
    .await
    .expect("统计会话");
    assert_eq!(open, 0);

    // 旧会话立即 401；正确密码登录返回原因且不建会话。
    let response = send(
        &app.app,
        Method::GET,
        "/api/v1/auth/me",
        None,
        Some(&user_cookie),
        false,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let (status, _headers, body) = login(&app, "suspend-me@example.com", 0x20).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "account_suspended");
    assert_eq!(body["message"], "滥用");
    assert_eq!(audit_count(&app, "suspend", Some(user_id)).await, 1);
}

#[tokio::test]
async fn settings_update_is_applied_immediately_and_audited_once() {
    let app = setup().await;
    let (_admin_id, admin_cookie) =
        user_session(&app, "settings-admin@example.com", 0x10, true).await;

    let body = json!({
        "registration_mode": "closed",
        "default_quota_bytes": 5 * GIB,
    });
    let (status, response) = admin_request(
        &app,
        Method::PUT,
        "/api/v1/admin/settings",
        Some(body.clone()),
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["registration_mode"], "closed");

    let (status, register_body) = register(&app, "after-closed@example.com", 0x20).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(register_body["code"], "registration_closed");

    let (status, _response) = admin_request(
        &app,
        Method::PUT,
        "/api/v1/admin/settings",
        Some(body),
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        audit_count(&app, "update_settings", None).await,
        1,
        "重复提交相同设置不得产生重复审计"
    );
}

#[tokio::test]
async fn audit_can_be_filtered_by_target_user() {
    let app = setup().await;
    let (_admin_id, admin_cookie) = user_session(&app, "audit-admin@example.com", 0x10, true).await;
    let (first, _first_cookie) = user_session(&app, "audit-one@example.com", 0x20, false).await;
    let (second, _second_cookie) = user_session(&app, "audit-two@example.com", 0x21, false).await;
    for user_id in [first, second] {
        admin_request(
            &app,
            Method::POST,
            &format!("/api/v1/admin/users/{user_id}/approve"),
            Some(json!({})),
            &admin_cookie,
        )
        .await;
    }

    let (status, body) = admin_request(
        &app,
        Method::GET,
        &format!("/api/v1/admin/audit?target_user_id={first}"),
        None,
        &admin_cookie,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["target_user_id"], first.to_string());
    assert_eq!(items[0]["action"], "approve");
    assert!(items[0]["admin_id"].is_string());
    assert!(items[0]["created_at"].is_string());
}
