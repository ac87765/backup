//! 文件传输服务端集成测试（tasks 8.2–8.6）。
//!
//! 需要真实 PostgreSQL 与 MinIO（testcontainers），由审查者在 WSL 中运行；
//! 本机 Windows 只保证 `cargo test --workspace --no-run` 通过。
//!
//! 覆盖 file-transfer spec 的服务端场景：创建/幂等/配额竞争、presign、
//! 块确认、续传查询、提交/重复提交/目标不可用、中止/过期清理、下载清单、
//! 孤儿巡检与会话状态指标。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::num::NonZeroU32;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    environment, insert_test_user, integration_config, migrated_database, static_fixture,
};
use server::jobs::scan::{ScanOutcome, run_once};
use server::jobs::uploads::{OrphanObjectScan, UploadSessionExpiry};
use server::storage::{
    FaultInjectionConfig, InjectedErrorKind, ObjectKey, ObjectOperation, Storage,
};

const CSRF_HEADER: &str = "x-requested-with";
const CSRF_VALUE: &str = "backup-web";
/// 4 MiB + 41 字节帧开销。
const FULL_CHUNK: i64 = crypto_core::MAX_CHUNK_CIPHERTEXT_SIZE as i64;
/// 空明文的密文帧长度。
const EMPTY_CHUNK: i64 = crypto_core::FRAME_OVERHEAD as i64;

struct TestApp {
    app: Router,
    database: server::Database,
    storage: Storage,
    _dir: tempfile::TempDir,
}

#[derive(Clone, Copy)]
struct TestOptions {
    /// 每用户写限流值；默认放大，避免普通测试误触限流。
    user_write_rate_limit: u32,
    database_max_connections: u32,
    /// 故障注入：让对象存储 `stat` 延迟指定时间（验证事务外 stat）。
    stat_delay: Option<Duration>,
}

impl Default for TestOptions {
    fn default() -> Self {
        Self {
            user_write_rate_limit: 600,
            database_max_connections: 5,
            stat_delay: None,
        }
    }
}

async fn setup() -> TestApp {
    setup_with(TestOptions::default()).await
}

async fn setup_with(options: TestOptions) -> TestApp {
    let env = environment().await;
    let (test_db, _database) = migrated_database(env).await;
    let pool = PgPoolOptions::new()
        .max_connections(options.database_max_connections)
        .connect(&test_db.url)
        .await
        .expect("连接测试数据库");
    let database = server::Database::from_pool(pool);

    let (dir, root) = static_fixture();
    let mut config = integration_config(env, root);
    config.auth_rate_limit_per_minute = NonZeroU32::new(600).expect("限流必须为正");
    config.user_write_rate_limit_per_minute =
        NonZeroU32::new(options.user_write_rate_limit).expect("限流必须为正");
    config.database.max_connections = options.database_max_connections;
    let storage = Storage::new(&config.storage).expect("构造 storage");
    let storage = match options.stat_delay {
        Some(delay) => storage.with_fault_injection(
            FaultInjectionConfig::new(0, InjectedErrorKind::Unavailable)
                .only(ObjectOperation::Stat)
                .with_delay(delay),
        ),
        None => storage,
    };
    let app = server::build_router(server::AppState::new(
        config,
        database.clone(),
        storage.clone(),
    ));
    TestApp {
        app,
        database,
        storage,
        _dir: dir,
    }
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: &str,
    csrf: bool,
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    if csrf {
        builder = builder.header(CSRF_HEADER, CSRF_VALUE);
    }
    builder = builder.header(header::COOKIE, format!("__Host-session={cookie}"));
    let payload = body.map(|body| body.to_string()).unwrap_or_default();
    app.clone()
        .oneshot(builder.body(Body::from(payload)).expect("构造测试请求"))
        .await
        .expect("请求应当完成")
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

async fn request(
    app: &TestApp,
    method: Method,
    uri: &str,
    body: Option<Value>,
    cookie: &str,
    csrf: bool,
) -> (StatusCode, Value) {
    let (status, _headers, value) =
        read(send(&app.app, method, uri, body, cookie, csrf).await).await;
    (status, value)
}

fn error_code(body: &Value) -> &str {
    body.get("code").and_then(Value::as_str).unwrap_or("")
}

fn hex(bytes: &[u8]) -> String {
    server::auth::encode_hex(bytes)
}

fn wrapped(seed: u16) -> String {
    let seed = seed as u8;
    let mut bytes = vec![crypto_core::CIPHERTEXT_FORMAT_VERSION; crypto_core::FRAME_OVERHEAD + 32];
    for byte in &mut bytes[1..] {
        *byte = seed;
    }
    hex(&bytes)
}

fn meta(seed: u16) -> String {
    let seed = seed as u8;
    let mut bytes = vec![crypto_core::CIPHERTEXT_FORMAT_VERSION; 64];
    for byte in &mut bytes[1..] {
        *byte = seed;
    }
    hex(&bytes)
}

fn token(seed: u16) -> String {
    hex(&[(seed & 0xff) as u8; 32])
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// 创建一个已验证邮箱的用户与一个会话，返回 (user_id, root_node_id, cookie)。
async fn verified_user_with_session(app: &TestApp) -> (Uuid, Uuid, String) {
    let user_id = Uuid::new_v4();
    let root_id = insert_test_user(&app.database, user_id).await;
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(app.database.pool())
        .await
        .expect("标记邮箱已验证");
    let cookie = format!("test-session-{}", Uuid::new_v4().simple());
    sqlx::query(
        r#"
        INSERT INTO sessions (id, user_id, token_hash, last_seen_at, expires_at, ip)
        VALUES ($1, $2, $3, now(), now() + interval '30 days', '127.0.0.1')
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(server::auth::hash_token(&cookie))
    .execute(app.database.pool())
    .await
    .expect("写入会话");
    (user_id, root_id, cookie)
}

async fn insert_node(
    app: &TestApp,
    user_id: Uuid,
    parent_id: Uuid,
    kind: &str,
    name_token: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    let wrapped_key = server::auth::decode_hex_vec(&wrapped(0x11)).expect("包裹密钥");
    let encrypted_meta = server::auth::decode_hex_vec(&meta(0x12)).expect("元数据");
    let token_bytes = server::auth::decode_hex_vec(name_token).expect("令牌");
    sqlx::query(
        r#"
        INSERT INTO nodes (id, user_id, parent_id, kind, wrapped_key, encrypted_meta, name_token)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(user_id)
    .bind(parent_id)
    .bind(kind)
    .bind(wrapped_key)
    .bind(encrypted_meta)
    .bind(token_bytes)
    .execute(app.database.pool())
    .await
    .expect("插入节点");
    id
}

fn new_file_body(
    content_id: Uuid,
    node_id: Uuid,
    parent_id: Uuid,
    version_id: Uuid,
    chunk_count: i32,
    declared_size: i64,
    seed: u16,
) -> Value {
    json!({
        "content_id": content_id,
        "kind": "new_file",
        "node_id": node_id,
        "parent_id": parent_id,
        "wrapped_key": wrapped(seed),
        "encrypted_meta": meta(seed + 1),
        "name_token": token(seed + 2),
        "version_id": version_id,
        "version_meta": meta(seed + 3),
        "chunk_count": chunk_count,
        "declared_size": declared_size,
    })
}

fn new_version_body(
    content_id: Uuid,
    node_id: Uuid,
    version_id: Uuid,
    chunk_count: i32,
    declared_size: i64,
    seed: u16,
) -> Value {
    json!({
        "content_id": content_id,
        "kind": "new_version",
        "node_id": node_id,
        "version_id": version_id,
        "version_meta": meta(seed),
        "chunk_count": chunk_count,
        "declared_size": declared_size,
    })
}

async fn create_session(app: &TestApp, cookie: &str, body: Value) -> (StatusCode, Value) {
    request(
        app,
        Method::POST,
        "/api/v1/upload-sessions",
        Some(body),
        cookie,
        true,
    )
    .await
}

async fn create_session_ok(app: &TestApp, cookie: &str, body: Value) -> Value {
    let (status, value) = create_session(app, cookie, body).await;
    assert_eq!(status, StatusCode::OK, "创建会话失败: {value}");
    value
}

async fn presign(app: &TestApp, cookie: &str, session_id: &str, idx: i32) -> (StatusCode, Value) {
    request(
        app,
        Method::POST,
        &format!("/api/v1/upload-sessions/{session_id}/chunks/{idx}/presign"),
        None,
        cookie,
        true,
    )
    .await
}

async fn confirm(
    app: &TestApp,
    cookie: &str,
    session_id: &str,
    idx: i32,
    declared_size: i64,
    sha256: &str,
) -> (StatusCode, Value) {
    request(
        app,
        Method::POST,
        &format!("/api/v1/upload-sessions/{session_id}/chunks/{idx}/confirm"),
        Some(json!({"declared_size": declared_size, "sha256": sha256})),
        cookie,
        true,
    )
    .await
}

async fn commit(app: &TestApp, cookie: &str, session_id: &str) -> (StatusCode, Value) {
    request(
        app,
        Method::POST,
        &format!("/api/v1/upload-sessions/{session_id}/commit"),
        None,
        cookie,
        true,
    )
    .await
}

async fn abort(app: &TestApp, cookie: &str, session_id: &str) -> (StatusCode, Value) {
    request(
        app,
        Method::POST,
        &format!("/api/v1/upload-sessions/{session_id}/abort"),
        None,
        cookie,
        true,
    )
    .await
}

async fn put_object(url: &str, bytes: &[u8]) {
    let response = reqwest::Client::new()
        .put(url)
        .body(bytes.to_vec())
        .send()
        .await
        .expect("预签名 PUT 应当成功");
    assert!(
        response.status().is_success(),
        "预签名 PUT 失败: {}",
        response.status()
    );
}

/// presign 一个块、上传 `bytes`，返回对象键。
async fn presign_and_upload(
    app: &TestApp,
    cookie: &str,
    session_id: &str,
    idx: i32,
    bytes: &[u8],
) -> String {
    let (status, body) = presign(app, cookie, session_id, idx).await;
    assert_eq!(status, StatusCode::OK, "presign 失败: {body}");
    assert_eq!(body["expected_size"], bytes.len());
    put_object(body["url"].as_str().expect("URL"), bytes).await;
    sqlx::query_scalar("SELECT object_key FROM upload_chunks WHERE session_id = $1 AND idx = $2")
        .bind(Uuid::parse_str(session_id).expect("session id"))
        .bind(idx)
        .fetch_one(app.database.pool())
        .await
        .expect("读取对象键")
}

async fn upload_and_confirm(
    app: &TestApp,
    cookie: &str,
    session_id: &str,
    idx: i32,
    bytes: &[u8],
) -> String {
    let key = presign_and_upload(app, cookie, session_id, idx, bytes).await;
    let (status, body) = confirm(
        app,
        cookie,
        session_id,
        idx,
        bytes.len() as i64,
        &sha256_hex(bytes),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "确认失败: {body}");
    key
}

async fn batch_presign(
    app: &TestApp,
    cookie: &str,
    session_id: &str,
    indices: Vec<i32>,
) -> (StatusCode, Value) {
    request(
        app,
        Method::POST,
        &format!("/api/v1/upload-sessions/{session_id}/chunks/presign"),
        Some(json!({ "indices": indices })),
        cookie,
        true,
    )
    .await
}

async fn batch_confirm(
    app: &TestApp,
    cookie: &str,
    session_id: &str,
    chunks: Vec<Value>,
) -> (StatusCode, Value) {
    request(
        app,
        Method::POST,
        &format!("/api/v1/upload-sessions/{session_id}/chunks/confirm"),
        Some(json!({ "chunks": chunks })),
        cookie,
        true,
    )
    .await
}

async fn delete_count(app: &TestApp, object_key: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM object_deletions WHERE object_key = $1")
        .bind(object_key)
        .fetch_one(app.database.pool())
        .await
        .expect("查询删除队列")
}

async fn reserved(app: &TestApp, user_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT reserved_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("查询预留字节")
}

// ---------------------------------------------------------------------------
// 8.2 创建会话
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_session_for_ten_mib_file_reserves_quota_and_returns_three_chunks() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let declared = 10 * 1024 * 1024 + 3 * EMPTY_CHUNK;
    let body = new_file_body(
        Uuid::new_v4(),
        Uuid::new_v4(),
        root,
        Uuid::new_v4(),
        3,
        declared,
        1,
    );

    let value = create_session_ok(&app, &cookie, body).await;
    assert_eq!(value["chunk_count"], 3);
    assert_eq!(value["declared_size"], declared);
    assert_eq!(value["status"], "open");
    assert_eq!(value["confirmed_chunks"], 0);
    assert!(value["id"].is_string());
    assert!(value["expires_at"].is_string());
    assert_eq!(reserved(&app, user_id).await, declared);

    let created_at = time::OffsetDateTime::parse(
        value["created_at"].as_str().expect("created_at"),
        &time::format_description::well_known::Rfc3339,
    )
    .expect("解析 created_at");
    let expires_at = time::OffsetDateTime::parse(
        value["expires_at"].as_str().expect("expires_at"),
        &time::format_description::well_known::Rfc3339,
    )
    .expect("解析 expires_at");
    let hours = (expires_at - created_at).whole_hours();
    assert!(
        (23..=24).contains(&hours),
        "会话有效期应约为 24 小时，实际 {hours}"
    );
}

#[tokio::test]
async fn create_session_is_idempotent_and_conflicts_on_different_content() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let content_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let declared = 100 + EMPTY_CHUNK;
    let body = new_file_body(content_id, node_id, root, version_id, 1, declared, 4);

    let first = create_session_ok(&app, &cookie, body.clone()).await;
    let reserved_after_first = reserved(&app, user_id).await;

    // 完全相同请求：同一会话、不重复预留。
    let second = create_session_ok(&app, &cookie, body).await;
    assert_eq!(first["id"], second["id"]);
    assert_eq!(reserved(&app, user_id).await, reserved_after_first);

    // 同一 content_id 不同内容：conflict。
    let changed = new_file_body(content_id, node_id, root, version_id, 1, declared + 1, 4);
    let (status, value) = create_session(&app, &cookie, changed).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&value), "conflict");
    assert_eq!(reserved(&app, user_id).await, reserved_after_first);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM upload_sessions")
        .fetch_one(app.database.pool())
        .await
        .expect("会话数量");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn unverified_email_cannot_create_upload_session() {
    let app = setup().await;
    let user_id = Uuid::new_v4();
    let root = insert_test_user(&app.database, user_id).await;
    let cookie = format!("test-session-{}", Uuid::new_v4().simple());
    sqlx::query(
        r#"
        INSERT INTO sessions (id, user_id, token_hash, last_seen_at, expires_at, ip)
        VALUES ($1, $2, $3, now(), now() + interval '30 days', '127.0.0.1')
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(server::auth::hash_token(&cookie))
    .execute(app.database.pool())
    .await
    .expect("写入会话");

    let body = new_file_body(
        Uuid::new_v4(),
        Uuid::new_v4(),
        root,
        Uuid::new_v4(),
        1,
        100 + EMPTY_CHUNK,
        5,
    );
    let (status, value) = create_session(&app, &cookie, body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&value), "email_not_verified");
    assert_eq!(reserved(&app, user_id).await, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM upload_sessions")
        .fetch_one(app.database.pool())
        .await
        .expect("会话数量");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn quota_exceeded_is_rejected_without_creating_session() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    sqlx::query("UPDATE users SET quota_bytes = $2 WHERE id = $1")
        .bind(user_id)
        .bind(10_i64 * 1024 * 1024)
        .execute(app.database.pool())
        .await
        .expect("设置配额");

    // 2 GiB 明文恰好 512 个整块。
    let declared = 2_i64 * 1024 * 1024 * 1024 + 512 * EMPTY_CHUNK;
    let body = new_file_body(
        Uuid::new_v4(),
        Uuid::new_v4(),
        root,
        Uuid::new_v4(),
        512,
        declared,
        6,
    );
    let (status, value) = create_session(&app, &cookie, body).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&value), "quota_exceeded");
    assert_eq!(reserved(&app, user_id).await, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM upload_sessions")
        .fetch_one(app.database.pool())
        .await
        .expect("会话数量");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn concurrent_session_creation_only_one_succeeds() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    // 两个 6 MiB 会话单独都不超过 10 MiB，合计超过。
    sqlx::query("UPDATE users SET quota_bytes = $2 WHERE id = $1")
        .bind(user_id)
        .bind(10_i64 * 1024 * 1024)
        .execute(app.database.pool())
        .await
        .expect("设置配额");
    let declared = 6 * 1024 * 1024 + 2 * EMPTY_CHUNK;

    let first_body = new_file_body(
        Uuid::new_v4(),
        Uuid::new_v4(),
        root,
        Uuid::new_v4(),
        2,
        declared,
        7,
    );
    let second_body = new_file_body(
        Uuid::new_v4(),
        Uuid::new_v4(),
        root,
        Uuid::new_v4(),
        2,
        declared,
        8,
    );
    let first_future = send(
        &app.app,
        Method::POST,
        "/api/v1/upload-sessions",
        Some(first_body),
        &cookie,
        true,
    );
    let second_future = send(
        &app.app,
        Method::POST,
        "/api/v1/upload-sessions",
        Some(second_body),
        &cookie,
        true,
    );
    let (first_response, second_response) = tokio::join!(first_future, second_future);
    let (_, _, first_value) = read(first_response).await;
    let (_, _, second_value) = read(second_response).await;

    let mut codes = [
        first_value["code"].as_str().unwrap_or("ok"),
        second_value["code"].as_str().unwrap_or("ok"),
    ];
    codes.sort_unstable();
    assert_eq!(
        codes,
        ["ok", "quota_exceeded"],
        "恰好一个成功: {first_value} / {second_value}"
    );

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM upload_sessions")
        .fetch_one(app.database.pool())
        .await
        .expect("会话数量");
    assert_eq!(count, 1);
    assert_eq!(reserved(&app, user_id).await, declared);
}

#[tokio::test]
async fn chunk_layout_mismatch_is_rejected() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    // 3 块时最后一块必须落在 [41, 4 MiB+41]；声明总量少 1 字节直接拒绝。
    let declared = 2 * FULL_CHUNK + EMPTY_CHUNK - 1;
    let body = new_file_body(
        Uuid::new_v4(),
        Uuid::new_v4(),
        root,
        Uuid::new_v4(),
        3,
        declared,
        9,
    );
    let (status, value) = create_session(&app, &cookie, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&value), "invalid_request");
}

#[tokio::test]
async fn per_user_write_rate_limit_returns_429() {
    // 用较低的上限（而不是默认 240）避免 governor 在测试循环期间补充令牌，
    // 并让请求是合法请求：限流必须在参数校验之前生效。
    let app = setup_with(TestOptions {
        user_write_rate_limit: 2,
        ..TestOptions::default()
    })
    .await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let valid_body = || {
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            100 + EMPTY_CHUNK,
            70,
        )
    };

    for _ in 0..2 {
        let (status, value) = create_session(&app, &cookie, valid_body()).await;
        assert_eq!(status, StatusCode::OK, "前两次合法请求应成功: {value}");
    }

    let (status, headers, value) = read(
        send(
            &app.app,
            Method::POST,
            "/api/v1/upload-sessions",
            Some(valid_body()),
            &cookie,
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_code(&value), "rate_limited");
    let retry_after = headers
        .get(header::RETRY_AFTER)
        .expect("限流响应必须带 Retry-After");
    assert!(
        retry_after
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|seconds| seconds >= 1),
        "Retry-After 必须是正整数秒: {retry_after:?}"
    );
}

// ---------------------------------------------------------------------------
// 8.3 presign / 确认 / 续传查询
// ---------------------------------------------------------------------------

#[tokio::test]
async fn presign_uses_short_lived_url_and_tracks_superseded_keys() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            10,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();

    let (status, first) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["expected_size"], declared);
    assert_eq!(first["method"], "PUT");
    assert!(
        first["url"]
            .as_str()
            .expect("URL")
            .contains("X-Amz-Expires=900"),
        "预签名有效期不得超过 15 分钟: {}",
        first["url"]
    );
    let first_key = sqlx::query_scalar::<_, String>(
        "SELECT object_key FROM upload_chunks WHERE session_id = $1 AND idx = 0",
    )
    .bind(Uuid::parse_str(&session_id).expect("session id"))
    .fetch_one(app.database.pool())
    .await
    .expect("第一个对象键");

    let (status, second) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let second_key = sqlx::query_scalar::<_, String>(
        "SELECT object_key FROM upload_chunks WHERE session_id = $1 AND idx = 0",
    )
    .bind(Uuid::parse_str(&session_id).expect("session id"))
    .fetch_one(app.database.pool())
    .await
    .expect("第二个对象键");
    assert_ne!(first_key, second_key, "重新 presign 必须生成新对象键");

    let superseded: Vec<String> = sqlx::query_scalar(
        "SELECT superseded_object_keys FROM upload_chunks WHERE session_id = $1 AND idx = 0",
    )
    .bind(Uuid::parse_str(&session_id).expect("session id"))
    .fetch_one(app.database.pool())
    .await
    .expect("旧对象键");
    assert_eq!(superseded, vec![first_key]);
}

#[tokio::test]
async fn expired_presigned_upload_is_rejected_by_object_storage() {
    let app = setup().await;
    let user_id = Uuid::new_v4();
    let key = ObjectKey::generate(user_id);
    let url = app
        .storage
        .presign_write(&key, Duration::from_secs(1))
        .await
        .expect("生成 1 秒有效的 URL");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let response = reqwest::Client::new()
        .put(url.url())
        .body(b"late-upload".to_vec())
        .send()
        .await
        .expect("请求对象存储");
    assert!(
        !response.status().is_success(),
        "过期 URL 必须被对象存储拒绝，实际 {}",
        response.status()
    );
}

#[tokio::test]
async fn presign_on_aborted_session_returns_upload_session_closed() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            100 + EMPTY_CHUNK,
            11,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let (status, value) = abort(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{value}");

    let (status, value) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&value), "upload_session_closed");

    // 已关闭会话不能通过提交接口“复活”。
    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&value), "upload_session_closed");
}

#[tokio::test]
async fn presign_on_expired_open_session_returns_upload_session_closed() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            100 + EMPTY_CHUNK,
            28,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    sqlx::query(
        "UPDATE upload_sessions SET expires_at = now() - interval '1 second' WHERE id = $1",
    )
    .bind(Uuid::parse_str(&session_id).expect("session id"))
    .execute(app.database.pool())
    .await
    .expect("设置过期时间");

    // 后台扫描尚未运行（status 仍为 open），但已过期的会话不能再 presign。
    let (status, value) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&value), "upload_session_closed");
}

#[tokio::test]
async fn confirm_missing_object_returns_chunk_not_found() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            12,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();

    let (status, value) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let (status, value) = confirm(
        &app,
        &cookie,
        &session_id,
        0,
        declared,
        &sha256_hex(b"never-uploaded"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&value), "chunk_not_found");
}

#[tokio::test]
async fn confirm_rejects_size_mismatch_and_allows_retry_after_reupload() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            13,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();

    let (status, presigned) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{presigned}");
    let url = presigned["url"].as_str().expect("URL").to_owned();
    // 先上传少 1 字节的对象。
    let short = vec![0xAA_u8; (declared - 1) as usize];
    put_object(&url, &short).await;
    let (status, value) =
        confirm(&app, &cookie, &session_id, 0, declared, &sha256_hex(&short)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&value), "chunk_size_mismatch");

    // 同一 URL 重新上传正确大小后可以确认成功（块保持未确认状态）。
    let correct = vec![0xAA_u8; declared as usize];
    put_object(&url, &correct).await;
    let (status, value) = confirm(
        &app,
        &cookie,
        &session_id,
        0,
        declared,
        &sha256_hex(&correct),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
}

#[tokio::test]
async fn confirm_is_idempotent_for_same_hash() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            14,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let bytes = vec![0xBB_u8; declared as usize];
    presign_and_upload(&app, &cookie, &session_id, 0, &bytes).await;
    let hash = sha256_hex(&bytes);

    let (status, first) = confirm(&app, &cookie, &session_id, 0, declared, &hash).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, second) = confirm(&app, &cookie, &session_id, 0, declared, &hash).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(first["confirmed_at"], second["confirmed_at"]);
    assert_eq!(first["sha256"], second["sha256"]);
}

#[tokio::test]
async fn confirmed_chunk_list_supports_resume() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let tail = 100 + EMPTY_CHUNK;
    let declared = FULL_CHUNK + tail;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            2,
            declared,
            15,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();

    let first = vec![0xCC_u8; FULL_CHUNK as usize];
    upload_and_confirm(&app, &cookie, &session_id, 0, &first).await;

    let (status, value) = request(
        &app,
        Method::GET,
        &format!("/api/v1/upload-sessions/{session_id}/chunks"),
        None,
        &cookie,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["confirmed_count"], 1);
    assert_eq!(value["items"][0]["idx"], 0);
    assert_eq!(value["items"][0]["declared_size"], FULL_CHUNK);

    let second = vec![0xDD_u8; tail as usize];
    upload_and_confirm(&app, &cookie, &session_id, 1, &second).await;
    let (status, value) = request(
        &app,
        Method::GET,
        &format!("/api/v1/upload-sessions/{session_id}/chunks"),
        None,
        &cookie,
        false,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["confirmed_count"], 2);
    assert_eq!(value["items"][1]["idx"], 1);
}

#[tokio::test]
async fn batch_presign_and_confirm_support_partial_results() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let tail = 100 + EMPTY_CHUNK;
    let declared = FULL_CHUNK + tail;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            2,
            declared,
            29,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();

    // 批量 presign：两个块各得一个 URL。
    let (status, presigned) = batch_presign(&app, &cookie, &session_id, vec![0, 1]).await;
    assert_eq!(status, StatusCode::OK, "{presigned}");
    assert_eq!(presigned["items"].as_array().expect("items").len(), 2);
    assert_eq!(presigned["items"][0]["ok"], true);
    assert_eq!(presigned["items"][0]["expected_size"], FULL_CHUNK);
    assert_eq!(presigned["items"][1]["ok"], true);
    assert_eq!(presigned["items"][1]["expected_size"], tail);
    let first_url = presigned["items"][0]["url"]
        .as_str()
        .expect("第一块 URL")
        .to_owned();
    let second_url = presigned["items"][1]["url"]
        .as_str()
        .expect("第二块 URL")
        .to_owned();

    // 只上传第 0 块；第 1 块应独立返回 chunk_not_found，越界 idx 返回 invalid_request。
    let first = vec![0xAB_u8; FULL_CHUNK as usize];
    put_object(&first_url, &first).await;
    let (status, confirmed) = batch_confirm(
        &app,
        &cookie,
        &session_id,
        vec![
            json!({"idx": 0, "declared_size": FULL_CHUNK, "sha256": sha256_hex(&first)}),
            json!({"idx": 1, "declared_size": tail, "sha256": sha256_hex(b"not-uploaded")}),
            json!({"idx": 99, "declared_size": 1, "sha256": sha256_hex(b"x")}),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    let items = confirmed["items"].as_array().expect("items");
    assert_eq!(items[0]["ok"], true);
    assert_eq!(items[1]["ok"], false);
    assert_eq!(items[1]["code"], "chunk_not_found");
    assert_eq!(items[2]["ok"], false);
    assert_eq!(items[2]["code"], "invalid_request");

    // 上传第 1 块后批量确认成功，随后可以提交。
    let second = vec![0xCD_u8; tail as usize];
    put_object(&second_url, &second).await;
    let (status, confirmed) = batch_confirm(
        &app,
        &cookie,
        &session_id,
        vec![json!({"idx": 1, "declared_size": tail, "sha256": sha256_hex(&second)})],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{confirmed}");
    assert_eq!(confirmed["items"][0]["ok"], true);
    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{value}");

    // 单批最多 32 块；空列表也拒绝。
    let too_many: Vec<i32> = (0..33).collect();
    let (status, value) = batch_presign(&app, &cookie, &session_id, too_many).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&value), "invalid_request");
    let (status, _value) = batch_presign(&app, &cookie, &session_id, vec![0, 0]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "重复 idx 必须被拒绝");
    let (status, _value) = batch_confirm(&app, &cookie, &session_id, vec![]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn slow_stat_does_not_hold_connections_or_block_commit() {
    // 2 个连接 + 3 秒 stat 延迟：旧实现持事务 stat 会让两个连接全部被占住，
    // ping/commit 只能等；新实现 stat 在事务外，连接应立即归还。
    let stat_delay = Duration::from_secs(3);
    let app = setup_with(TestOptions {
        database_max_connections: 2,
        stat_delay: Some(stat_delay),
        ..TestOptions::default()
    })
    .await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let tail = 100 + EMPTY_CHUNK;
    let declared = FULL_CHUNK + tail;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            2,
            declared,
            30,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();

    let (status, presigned) = batch_presign(&app, &cookie, &session_id, vec![0, 1]).await;
    assert_eq!(status, StatusCode::OK, "{presigned}");
    let first = vec![0xEF_u8; FULL_CHUNK as usize];
    let second = vec![0x12_u8; tail as usize];
    put_object(
        presigned["items"][0]["url"].as_str().expect("URL 0"),
        &first,
    )
    .await;
    put_object(
        presigned["items"][1]["url"].as_str().expect("URL 1"),
        &second,
    )
    .await;

    let pool = app.database.pool().clone();
    let started = tokio::time::Instant::now();
    let first_hash = sha256_hex(&first);
    let second_hash = sha256_hex(&second);
    let confirm_first = confirm(&app, &cookie, &session_id, 0, FULL_CHUNK, &first_hash);
    let confirm_second = confirm(&app, &cookie, &session_id, 1, tail, &second_hash);
    let ping = async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&pool),
        )
        .await
    };
    let concurrent_commit = async {
        let commit_started = tokio::time::Instant::now();
        let result = commit(&app, &cookie, &session_id).await;
        (result, commit_started.elapsed())
    };

    let (first_result, second_result, ping_result, (commit_result, commit_elapsed)) =
        tokio::join!(confirm_first, confirm_second, ping, concurrent_commit);
    let elapsed = started.elapsed();

    assert_eq!(first_result.0, StatusCode::OK, "{}", first_result.1);
    assert_eq!(second_result.0, StatusCode::OK, "{}", second_result.1);
    let ping = ping_result
        .expect("stat 期间数据库连接不应被占满")
        .expect("SELECT 1");
    assert_eq!(ping, 1);
    assert!(
        commit_elapsed < Duration::from_secs(2),
        "并发提交被慢 stat 阻塞了 {commit_elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "两个确认被串行化，总耗时 {elapsed:?}"
    );

    // 并发提交可能先看到“仍有块未确认”，重试必须成功且只产生一个版本。
    let (status, value) = commit_result;
    if status != StatusCode::OK {
        assert_eq!(status, StatusCode::CONFLICT, "{value}");
        assert_eq!(error_code(&value), "upload_incomplete");
    }
    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{value}");
}

// ---------------------------------------------------------------------------
// 8.4 提交 / 中止
// ---------------------------------------------------------------------------

#[tokio::test]
async fn commit_new_file_writes_version_and_converts_reservation() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let content_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(content_id, node_id, root, version_id, 1, declared, 16),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let bytes = vec![0xEE_u8; declared as usize];
    let object_key = upload_and_confirm(&app, &cookie, &session_id, 0, &bytes).await;

    let (status, committed) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{committed}");
    assert_eq!(committed["status"], "committed");
    assert_eq!(committed["session_id"], session_id);
    assert_eq!(committed["node_id"], node_id.to_string());
    assert_eq!(committed["version_id"], version_id.to_string());
    assert_eq!(committed["content_id"], content_id.to_string());
    assert_eq!(committed["ciphertext_size"], declared);

    // 节点、内容、块、版本都写入，当前版本指针正确。
    let node = sqlx::query(
        "SELECT kind, parent_id, current_version_id, row_version FROM nodes WHERE id = $1 AND user_id = $2",
    )
    .bind(node_id)
    .bind(user_id)
    .fetch_one(app.database.pool())
    .await
    .expect("新文件节点");
    assert_eq!(node.get::<String, _>("kind"), "file");
    assert_eq!(node.get::<Uuid, _>("parent_id"), root);
    assert_eq!(
        node.get::<Option<Uuid>, _>("current_version_id"),
        Some(version_id)
    );
    assert_eq!(node.get::<i32, _>("row_version"), 1);

    let content = sqlx::query(
        "SELECT chunk_count, ciphertext_size FROM contents WHERE id = $1 AND user_id = $2",
    )
    .bind(content_id)
    .bind(user_id)
    .fetch_one(app.database.pool())
    .await
    .expect("content");
    assert_eq!(content.get::<i32, _>("chunk_count"), 1);
    assert_eq!(content.get::<i64, _>("ciphertext_size"), declared);

    let chunk = sqlx::query(
        "SELECT object_key, ciphertext_size, sha256 FROM chunks WHERE content_id = $1 AND idx = 0",
    )
    .bind(content_id)
    .fetch_one(app.database.pool())
    .await
    .expect("chunk");
    assert_eq!(chunk.get::<String, _>("object_key"), object_key);
    assert_eq!(chunk.get::<i64, _>("ciphertext_size"), declared);
    assert_eq!(
        chunk.get::<Vec<u8>, _>("sha256"),
        Sha256::digest(&bytes).to_vec()
    );

    let version =
        sqlx::query("SELECT node_id, content_id FROM file_versions WHERE id = $1 AND user_id = $2")
            .bind(version_id)
            .bind(user_id)
            .fetch_one(app.database.pool())
            .await
            .expect("version");
    assert_eq!(version.get::<Uuid, _>("node_id"), node_id);
    assert_eq!(version.get::<Uuid, _>("content_id"), content_id);

    let usage = sqlx::query("SELECT used_bytes, reserved_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("用量");
    assert_eq!(usage.get::<i64, _>("used_bytes"), declared);
    assert_eq!(usage.get::<i64, _>("reserved_bytes"), 0);

    let chunks_left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM upload_chunks WHERE session_id = $1")
            .bind(Uuid::parse_str(&session_id).expect("session id"))
            .fetch_one(app.database.pool())
            .await
            .expect("上传块清理");
    assert_eq!(chunks_left, 0);
}

#[tokio::test]
async fn commit_is_idempotent_and_does_not_create_second_version() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let content_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(content_id, node_id, root, version_id, 1, declared, 17),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let bytes = vec![0x11_u8; declared as usize];
    upload_and_confirm(&app, &cookie, &session_id, 0, &bytes).await;

    let (status, first) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, second) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(first, second, "重复提交必须返回完全相同的结果");

    let versions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM file_versions WHERE user_id = $1 AND node_id = $2",
    )
    .bind(user_id)
    .bind(node_id)
    .fetch_one(app.database.pool())
    .await
    .expect("版本数量");
    assert_eq!(versions, 1);
    let usage = sqlx::query("SELECT used_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("用量");
    assert_eq!(
        usage.get::<i64, _>("used_bytes"),
        declared,
        "配额只能计一次"
    );
}

#[tokio::test]
async fn commit_reports_missing_chunks() {
    let app = setup().await;
    let (_user, root, cookie) = verified_user_with_session(&app).await;
    let tail = 100 + EMPTY_CHUNK;
    let declared = FULL_CHUNK + tail;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            2,
            declared,
            18,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let first = vec![0x22_u8; FULL_CHUNK as usize];
    upload_and_confirm(&app, &cookie, &session_id, 0, &first).await;

    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&value), "upload_incomplete");
    assert_eq!(value["missing_chunks"], json!([1]));
}

#[tokio::test]
async fn commit_keeps_session_open_when_target_folder_was_deleted() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let folder = insert_node(&app, user_id, root, "folder", &token(30)).await;
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            folder,
            Uuid::new_v4(),
            1,
            declared,
            19,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let bytes = vec![0x33_u8; declared as usize];
    let object_key = upload_and_confirm(&app, &cookie, &session_id, 0, &bytes).await;

    // 模拟“上传期间目标文件夹被移入回收站”。
    sqlx::query(
        r#"
        UPDATE nodes
        SET deleted_at = now(),
            trash_batch_id = $2,
            trashed_from_parent_id = parent_id
        WHERE id = $1 AND user_id = $3
        "#,
    )
    .bind(folder)
    .bind(Uuid::new_v4())
    .bind(user_id)
    .execute(app.database.pool())
    .await
    .expect("移入回收站");

    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&value), "target_unavailable");

    let state: String = sqlx::query_scalar("SELECT status FROM upload_sessions WHERE id = $1")
        .bind(Uuid::parse_str(&session_id).expect("session id"))
        .fetch_one(app.database.pool())
        .await
        .expect("会话状态");
    assert_eq!(state, "open", "目标不可用时会话必须保持 open");
    assert_eq!(reserved(&app, user_id).await, declared);

    // 用户可以中止会话，预留立即释放、对象入删除队列。
    let (status, value) = abort(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(reserved(&app, user_id).await, 0);
    assert_eq!(delete_count(&app, &object_key).await, 1);
}

#[tokio::test]
async fn commit_new_version_updates_current_version() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let file = insert_node(&app, user_id, root, "file", &token(31)).await;
    let content_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let declared = 200 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_version_body(content_id, file, version_id, 1, declared, 20),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let bytes = vec![0x44_u8; declared as usize];
    upload_and_confirm(&app, &cookie, &session_id, 0, &bytes).await;

    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let node = sqlx::query("SELECT current_version_id, row_version FROM nodes WHERE id = $1")
        .bind(file)
        .fetch_one(app.database.pool())
        .await
        .expect("文件节点");
    assert_eq!(
        node.get::<Option<Uuid>, _>("current_version_id"),
        Some(version_id)
    );
    assert_eq!(node.get::<i32, _>("row_version"), 2);
    let usage = sqlx::query("SELECT used_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("用量");
    assert_eq!(usage.get::<i64, _>("used_bytes"), declared);
}

#[tokio::test]
async fn commit_detects_name_conflict_and_keeps_session_open() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let existing = insert_node(&app, user_id, root, "folder", &token(40)).await;
    assert_ne!(existing, Uuid::nil());
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            21,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    // 新文件使用与既有节点相同的名称令牌（new_file_body seed=21 → token(23)）。
    // 这里直接把既有节点的令牌改成同一值，模拟同目录重名。
    let same_token = server::auth::decode_hex_vec(&token(23)).expect("令牌");
    sqlx::query("UPDATE nodes SET name_token = $2 WHERE id = $1")
        .bind(existing)
        .bind(same_token)
        .execute(app.database.pool())
        .await
        .expect("更新名称令牌");
    let bytes = vec![0x55_u8; declared as usize];
    upload_and_confirm(&app, &cookie, &session_id, 0, &bytes).await;

    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&value), "name_conflict");
    let state: String = sqlx::query_scalar("SELECT status FROM upload_sessions WHERE id = $1")
        .bind(Uuid::parse_str(&session_id).expect("session id"))
        .fetch_one(app.database.pool())
        .await
        .expect("会话状态");
    assert_eq!(state, "open");
}

/// 取消文件夹任务时 abort 与进行中的 presign 并发：presign 持会话行锁后插入
/// upload_chunks（外键检查需要用户行 KEY SHARE），abort 持用户行锁后等会话行锁。
/// 用户行锁若是 FOR UPDATE 会形成环并被判死锁（abort 返回 500、预留不释放）。
#[tokio::test]
async fn abort_waiting_on_presign_session_lock_does_not_deadlock() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            100 + EMPTY_CHUNK,
            23,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let session_uuid = Uuid::parse_str(&session_id).expect("session id");

    // 模拟 presign 事务的第一步：锁住会话行。
    let mut presign_tx = app.database.pool().begin().await.expect("开始事务");
    sqlx::query("SELECT id FROM upload_sessions WHERE id = $1 AND user_id = $2 FOR UPDATE")
        .bind(session_uuid)
        .bind(user_id)
        .execute(&mut *presign_tx)
        .await
        .expect("锁会话行");

    let router = app.app.clone();
    let abort_uri = format!("/api/v1/upload-sessions/{session_id}/abort");
    let abort_cookie = cookie.clone();
    let aborting = tokio::spawn(async move {
        read(send(&router, Method::POST, &abort_uri, None, &abort_cookie, true).await).await
    });

    // 等 abort 已持有用户行锁、阻塞在会话行锁上。
    let mut waiting = false;
    for _ in 0..500 {
        let blocked: i64 = sqlx::query_scalar(
            r#"
            SELECT count(*) FROM pg_stat_activity
            WHERE datname = current_database()
              AND wait_event_type = 'Lock'
              AND query LIKE '%FROM upload_sessions%FOR UPDATE%'
            "#,
        )
        .fetch_one(app.database.pool())
        .await
        .expect("查询锁等待");
        if blocked > 0 {
            waiting = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(waiting, "abort 应当阻塞在会话行锁上");

    // presign 的第二步：插入块记录（外键检查需要用户行 KEY SHARE）。
    sqlx::query(
        r#"
        INSERT INTO upload_chunks (session_id, user_id, idx, object_key, declared_size)
        VALUES ($1, $2, 0, $3, $4)
        "#,
    )
    .bind(session_uuid)
    .bind(user_id)
    .bind(ObjectKey::generate(user_id).as_str().to_owned())
    .bind(100 + EMPTY_CHUNK)
    .execute(&mut *presign_tx)
    .await
    .expect("presign 插入块记录不应死锁");
    presign_tx.commit().await.expect("提交 presign 事务");

    let (status, _headers, value) = aborting.await.expect("abort 任务");
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["status"], "aborted");
    assert_eq!(reserved(&app, user_id).await, 0);
}

#[tokio::test]
async fn abort_releases_quota_and_enqueues_current_and_superseded_objects() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            22,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let session_uuid = Uuid::parse_str(&session_id).expect("session id");

    // 同一块 presign 两次并都上传：旧键与新键都必须进入删除队列。
    let (status, first) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    put_object(first["url"].as_str().expect("URL"), b"first").await;
    let first_key = sqlx::query_scalar::<_, String>(
        "SELECT object_key FROM upload_chunks WHERE session_id = $1 AND idx = 0",
    )
    .bind(session_uuid)
    .fetch_one(app.database.pool())
    .await
    .expect("第一个键");
    let (status, second) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    put_object(second["url"].as_str().expect("URL"), b"second").await;
    let second_key = sqlx::query_scalar::<_, String>(
        "SELECT object_key FROM upload_chunks WHERE session_id = $1 AND idx = 0",
    )
    .bind(session_uuid)
    .fetch_one(app.database.pool())
    .await
    .expect("第二个键");

    let (status, value) = abort(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(value["status"], "aborted");
    assert_eq!(reserved(&app, user_id).await, 0);
    assert_eq!(delete_count(&app, &first_key).await, 1);
    assert_eq!(delete_count(&app, &second_key).await, 1);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM upload_chunks WHERE session_id = $1")
        .bind(session_uuid)
        .fetch_one(app.database.pool())
        .await
        .expect("清理上传块");
    assert_eq!(left, 0);
    let state: String = sqlx::query_scalar("SELECT status FROM upload_sessions WHERE id = $1")
        .bind(session_uuid)
        .fetch_one(app.database.pool())
        .await
        .expect("会话状态");
    assert_eq!(state, "aborted");

    // 重复中止幂等；会话不允许回到 open 后再提交。
    let (status, value) = abort(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{value}");
    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&value), "upload_session_closed");
}

// ---------------------------------------------------------------------------
// 8.5 过期与孤儿巡检
// ---------------------------------------------------------------------------

#[tokio::test]
async fn expiry_scan_expires_session_releases_quota_and_cleans_objects() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            23,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let session_uuid = Uuid::parse_str(&session_id).expect("session id");

    let (status, first) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    put_object(first["url"].as_str().expect("URL"), b"expire-me").await;
    let first_key = sqlx::query_scalar::<_, String>(
        "SELECT object_key FROM upload_chunks WHERE session_id = $1 AND idx = 0",
    )
    .bind(session_uuid)
    .fetch_one(app.database.pool())
    .await
    .expect("第一个键");
    let (status, second) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    let second_key = sqlx::query_scalar::<_, String>(
        "SELECT object_key FROM upload_chunks WHERE session_id = $1 AND idx = 0",
    )
    .bind(session_uuid)
    .fetch_one(app.database.pool())
    .await
    .expect("第二个键");

    sqlx::query(
        "UPDATE upload_sessions SET expires_at = now() - interval '1 minute' WHERE id = $1",
    )
    .bind(session_uuid)
    .execute(app.database.pool())
    .await
    .expect("设置过期时间");

    let outcome = run_once(&app.database, &UploadSessionExpiry, 100)
        .await
        .expect("运行过期扫描");
    assert_eq!(outcome, ScanOutcome::Executed(1));
    assert_eq!(reserved(&app, user_id).await, 0);
    assert_eq!(delete_count(&app, &first_key).await, 1);
    assert_eq!(delete_count(&app, &second_key).await, 1);
    let state: String = sqlx::query_scalar("SELECT status FROM upload_sessions WHERE id = $1")
        .bind(session_uuid)
        .fetch_one(app.database.pool())
        .await
        .expect("会话状态");
    assert_eq!(state, "expired");

    // 再次运行不会重复释放配额。
    let outcome = run_once(&app.database, &UploadSessionExpiry, 100)
        .await
        .expect("再次运行过期扫描");
    assert_eq!(outcome, ScanOutcome::Executed(0));
    assert_eq!(reserved(&app, user_id).await, 0);
}

async fn put_orphan_object(storage: &Storage, user_id: Uuid, body: &[u8]) -> String {
    let key = ObjectKey::generate(user_id);
    let url = storage
        .presign_write(&key, Duration::from_secs(300))
        .await
        .expect("生成预签名 URL");
    put_object(url.url(), body).await;
    key.as_str().to_owned()
}

#[tokio::test]
async fn orphan_scan_only_deletes_old_unreferenced_objects() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;

    // A：没有任何数据库引用的孤儿对象。
    let orphan = put_orphan_object(&app.storage, user_id, b"orphan").await;

    // B：已被提交的 content/chunks 引用。
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            24,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let bytes = vec![0x66_u8; declared as usize];
    let referenced = upload_and_confirm(&app, &cookie, &session_id, 0, &bytes).await;
    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{value}");

    // C：未关闭会话的 upload_chunks 仍引用它（即使尚未确认）。
    let open_session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            25,
        ),
    )
    .await;
    let open_session_id = open_session["id"].as_str().expect("会话 ID").to_owned();
    let (status, presigned) = presign(&app, &cookie, &open_session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{presigned}");
    put_object(presigned["url"].as_str().expect("URL"), b"in-progress").await;
    let protected = sqlx::query_scalar::<_, String>(
        "SELECT object_key FROM upload_chunks WHERE session_id = $1 AND idx = 0",
    )
    .bind(Uuid::parse_str(&open_session_id).expect("session id"))
    .fetch_one(app.database.pool())
    .await
    .expect("进行中会话的对象键");

    let scan = OrphanObjectScan::with_options(
        std::sync::Arc::new(app.storage.clone()),
        Duration::ZERO,
        format!("v1/{user_id}/"),
    );
    let outcome = run_once(&app.database, &scan, 100)
        .await
        .expect("运行孤儿巡检");
    assert_eq!(outcome, ScanOutcome::Executed(1));

    assert_eq!(delete_count(&app, &orphan).await, 1, "孤儿对象应入删除队列");
    assert_eq!(
        delete_count(&app, &referenced).await,
        0,
        "被 chunks 引用的对象不能删除"
    );
    assert_eq!(
        delete_count(&app, &protected).await,
        0,
        "未关闭会话仍在使用的对象不能删除"
    );
}

#[tokio::test]
async fn orphan_scan_advances_cursor_across_runs() {
    let app = setup().await;
    let (user_id, _root, _cookie) = verified_user_with_session(&app).await;
    let first = put_orphan_object(&app.storage, user_id, b"one").await;
    let second = put_orphan_object(&app.storage, user_id, b"two").await;
    let third = put_orphan_object(&app.storage, user_id, b"three").await;

    let scan = OrphanObjectScan::with_options(
        std::sync::Arc::new(app.storage.clone()),
        Duration::ZERO,
        format!("v1/{user_id}/"),
    );
    let outcome = run_once(&app.database, &scan, 2).await.expect("第一次巡检");
    assert_eq!(outcome, ScanOutcome::Executed(2));
    let cursor: Option<String> =
        sqlx::query_scalar("SELECT cursor FROM job_state WHERE job_id = $1")
            .bind(server::jobs::job_ids::ORPHAN_OBJECT_SCAN)
            .fetch_one(app.database.pool())
            .await
            .expect("游标");
    assert!(cursor.is_some(), "处理满一页后必须持久化游标");

    let outcome = run_once(&app.database, &scan, 2).await.expect("第二次巡检");
    assert_eq!(outcome, ScanOutcome::Executed(1));
    let cursor: Option<String> =
        sqlx::query_scalar("SELECT cursor FROM job_state WHERE job_id = $1")
            .bind(server::jobs::job_ids::ORPHAN_OBJECT_SCAN)
            .fetch_one(app.database.pool())
            .await
            .expect("游标");
    assert!(cursor.is_none(), "扫描到末尾后游标回到开头");

    for key in [&first, &second, &third] {
        assert_eq!(delete_count(&app, key).await, 1, "三个对象都必须入队");
    }
}

#[tokio::test]
async fn orphan_scan_recovers_object_from_expired_session_after_cleanup_miss() {
    let app = setup().await;
    let (user_id, root, cookie) = verified_user_with_session(&app).await;
    let declared = 100 + EMPTY_CHUNK;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            declared,
            26,
        ),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let session_uuid = Uuid::parse_str(&session_id).expect("session id");
    let (status, presigned) = presign(&app, &cookie, &session_id, 0).await;
    assert_eq!(status, StatusCode::OK, "{presigned}");
    put_object(presigned["url"].as_str().expect("URL"), b"forgotten").await;
    let object_key = sqlx::query_scalar::<_, String>(
        "SELECT object_key FROM upload_chunks WHERE session_id = $1 AND idx = 0",
    )
    .bind(session_uuid)
    .fetch_one(app.database.pool())
    .await
    .expect("对象键");

    sqlx::query(
        "UPDATE upload_sessions SET expires_at = now() - interval '1 minute' WHERE id = $1",
    )
    .bind(session_uuid)
    .execute(app.database.pool())
    .await
    .expect("设置过期时间");
    run_once(&app.database, &UploadSessionExpiry, 100)
        .await
        .expect("过期清理");
    assert_eq!(delete_count(&app, &object_key).await, 1);

    // 模拟删除队列记录丢失（对象仍在对象存储中）。
    sqlx::query("DELETE FROM object_deletions WHERE object_key = $1")
        .bind(&object_key)
        .execute(app.database.pool())
        .await
        .expect("模拟清理遗漏");
    assert_eq!(delete_count(&app, &object_key).await, 0);

    let scan = OrphanObjectScan::with_options(
        std::sync::Arc::new(app.storage.clone()),
        Duration::ZERO,
        format!("v1/{user_id}/"),
    );
    let outcome = run_once(&app.database, &scan, 100).await.expect("孤儿巡检");
    assert_eq!(outcome, ScanOutcome::Executed(1));
    assert_eq!(delete_count(&app, &object_key).await, 1, "巡检必须补上删除");
}

// ---------------------------------------------------------------------------
// 8.6 下载
// ---------------------------------------------------------------------------

#[tokio::test]
async fn download_manifest_returns_chunk_urls_and_hides_other_users() {
    let app = setup().await;
    let (_user_id, root, cookie) = verified_user_with_session(&app).await;
    let content_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let tail = 100 + EMPTY_CHUNK;
    let declared = FULL_CHUNK + tail;
    let session = create_session_ok(
        &app,
        &cookie,
        new_file_body(content_id, node_id, root, version_id, 2, declared, 27),
    )
    .await;
    let session_id = session["id"].as_str().expect("会话 ID").to_owned();
    let first = vec![0x77_u8; FULL_CHUNK as usize];
    let second = vec![0x88_u8; tail as usize];
    upload_and_confirm(&app, &cookie, &session_id, 0, &first).await;
    upload_and_confirm(&app, &cookie, &session_id, 1, &second).await;
    let (status, value) = commit(&app, &cookie, &session_id).await;
    assert_eq!(status, StatusCode::OK, "{value}");

    let uri = format!("/api/v1/nodes/{node_id}/versions/{version_id}/download");
    let (status, manifest) = request(&app, Method::GET, &uri, None, &cookie, false).await;
    assert_eq!(status, StatusCode::OK, "{manifest}");
    assert_eq!(manifest["content_id"], content_id.to_string());
    assert_eq!(manifest["ciphertext_size"], declared);
    assert_eq!(manifest["chunk_count"], 2);
    assert_eq!(manifest["chunks"].as_array().expect("chunks").len(), 2);
    assert_eq!(manifest["chunks"][0]["idx"], 0);
    assert_eq!(manifest["chunks"][0]["ciphertext_size"], FULL_CHUNK);
    assert_eq!(manifest["chunks"][0]["sha256"], sha256_hex(&first));
    assert_eq!(manifest["chunks"][1]["ciphertext_size"], tail);
    assert!(
        manifest["chunks"][0]["url"]
            .as_str()
            .expect("URL")
            .contains("X-Amz-Expires=900")
    );

    // 预签名 GET 返回的字节必须与上传时一致。
    let downloaded = reqwest::Client::new()
        .get(manifest["chunks"][1]["url"].as_str().expect("URL"))
        .send()
        .await
        .expect("下载块")
        .bytes()
        .await
        .expect("读取块");
    assert_eq!(downloaded.as_ref(), second.as_slice());

    // 另一个用户访问同一版本：404，与不存在不可区分。
    let (_other_user, _other_root, other_cookie) = verified_user_with_session(&app).await;
    let (status, value) = request(&app, Method::GET, &uri, None, &other_cookie, false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&value), "not_found");

    // 不存在的版本同样 404。
    let missing = format!(
        "/api/v1/nodes/{node_id}/versions/{}/download",
        Uuid::new_v4()
    );
    let (status, _value) = request(&app, Method::GET, &missing, None, &cookie, false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 指标
// ---------------------------------------------------------------------------

#[tokio::test]
async fn upload_session_metrics_are_sampled_by_status() {
    let app = setup().await;
    let handle = server::metrics::install().expect("安装指标记录器");
    let (_user, root, cookie) = verified_user_with_session(&app).await;

    let make = |seed: u16| {
        new_file_body(
            Uuid::new_v4(),
            Uuid::new_v4(),
            root,
            Uuid::new_v4(),
            1,
            100 + EMPTY_CHUNK,
            seed,
        )
    };
    // open
    let _open = create_session_ok(&app, &cookie, make(60)).await;
    // aborted
    let aborted = create_session_ok(&app, &cookie, make(61)).await;
    abort(&app, &cookie, aborted["id"].as_str().expect("会话 ID")).await;
    // expired
    let expired = create_session_ok(&app, &cookie, make(62)).await;
    let expired_id = Uuid::parse_str(expired["id"].as_str().expect("会话 ID")).expect("id");
    sqlx::query(
        "UPDATE upload_sessions SET expires_at = now() - interval '1 minute' WHERE id = $1",
    )
    .bind(expired_id)
    .execute(app.database.pool())
    .await
    .expect("设置过期时间");
    run_once(&app.database, &UploadSessionExpiry, 100)
        .await
        .expect("过期清理");

    server::jobs::metrics::record_queue_metrics(app.database.pool())
        .await
        .expect("采样指标");
    let rendered = handle.render();
    assert!(
        rendered.contains("upload_sessions{status=\"open\"} 1"),
        "缺少 open 指标:\n{rendered}"
    );
    assert!(
        rendered.contains("upload_sessions{status=\"aborted\"} 1"),
        "缺少 aborted 指标:\n{rendered}"
    );
    assert!(
        rendered.contains("upload_sessions{status=\"expired\"} 1"),
        "缺少 expired 指标:\n{rendered}"
    );
}
