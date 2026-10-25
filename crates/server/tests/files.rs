//! 文件管理集成测试（tasks 7.1–7.7）。
//!
//! 需要真实 PostgreSQL（testcontainers），由审查者在 WSL 中运行；本机
//! Windows 只保证 `cargo test --workspace --no-run` 通过。
//!
//! 本文件覆盖 file-management spec 的服务端场景，并直接用 SQL 插入
//! contents/chunks/file_versions 夹具（上传接口在第 8 组）。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::num::NonZeroU32;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::Row;
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    environment, insert_test_user, integration_config, migrated_database, static_fixture,
};
use server::jobs::content::{QuotaReconciliation, TrashCleanup, VersionRetentionCleanup};
use server::jobs::scan::{ScanOutcome, run_once};

const CSRF_HEADER: &str = "x-requested-with";
const CSRF_VALUE: &str = "backup-web";

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
    config.auth_rate_limit_per_minute = NonZeroU32::new(600).expect("限流必须为正");
    let storage = server::Storage::new(&config.storage).expect("构造 storage");
    let app = server::build_router(server::AppState::new(config, database.clone(), storage));
    TestApp {
        app,
        database,
        _dir: dir,
    }
}

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

fn unique_token(index: usize) -> String {
    let mut bytes = [0_u8; 32];
    bytes[..8].copy_from_slice(&(index as u64).to_le_bytes());
    hex(&bytes)
}

/// 创建一个用户与一个会话，返回 (user_id, root_node_id, cookie)。
async fn user_with_session(app: &TestApp) -> (Uuid, Uuid, String) {
    let user_id = Uuid::new_v4();
    let root_id = insert_test_user(&app.database, user_id).await;
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
    meta_hex: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    let wrapped = server::auth::decode_hex_vec(&wrapped(0x11)).expect("包裹密钥");
    let encrypted_meta = server::auth::decode_hex_vec(meta_hex).expect("元数据");
    let token_bytes = server::auth::decode_hex_vec(name_token).expect("令牌");
    sqlx::query(
        r#"
        INSERT INTO nodes (
            id, user_id, parent_id, kind, wrapped_key, encrypted_meta, name_token
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(user_id)
    .bind(parent_id)
    .bind(kind)
    .bind(wrapped)
    .bind(encrypted_meta)
    .bind(token_bytes)
    .execute(app.database.pool())
    .await
    .expect("插入节点");
    id
}

async fn insert_content(
    app: &TestApp,
    user_id: Uuid,
    node_id: Uuid,
    content_id: Uuid,
    ciphertext_size: i64,
    chunks: &[(&str, i64)],
) {
    sqlx::query(
        r#"
        INSERT INTO contents (id, user_id, node_id, chunk_count, ciphertext_size)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(content_id)
    .bind(user_id)
    .bind(node_id)
    .bind(chunks.len() as i32)
    .bind(ciphertext_size)
    .execute(app.database.pool())
    .await
    .expect("插入 content");

    for (index, (object_key, size)) in chunks.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO chunks (content_id, user_id, idx, object_key, ciphertext_size, sha256)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(content_id)
        .bind(user_id)
        .bind(index as i32)
        .bind(*object_key)
        .bind(*size)
        .bind([7_u8; 32].as_slice())
        .execute(app.database.pool())
        .await
        .expect("插入 chunk");
    }
}

async fn insert_version(
    app: &TestApp,
    user_id: Uuid,
    node_id: Uuid,
    version_id: Uuid,
    content_id: Uuid,
    created_offset: &str,
    meta_hex: &str,
) {
    let encrypted_meta = server::auth::decode_hex_vec(meta_hex).expect("版本元数据");
    sqlx::query(
        r#"
        INSERT INTO file_versions (id, user_id, node_id, content_id, encrypted_meta, created_at)
        VALUES ($1, $2, $3, $4, $5, now() + $6::interval)
        "#,
    )
    .bind(version_id)
    .bind(user_id)
    .bind(node_id)
    .bind(content_id)
    .bind(encrypted_meta)
    .bind(created_offset)
    .execute(app.database.pool())
    .await
    .expect("插入版本");
}

async fn set_current_version(app: &TestApp, user_id: Uuid, node_id: Uuid, version_id: Uuid) {
    sqlx::query("UPDATE nodes SET current_version_id = $3 WHERE id = $1 AND user_id = $2")
        .bind(node_id)
        .bind(user_id)
        .bind(version_id)
        .execute(app.database.pool())
        .await
        .expect("更新当前版本");
}

async fn set_used_bytes(app: &TestApp, user_id: Uuid, used: i64) {
    sqlx::query("UPDATE users SET used_bytes = $2 WHERE id = $1")
        .bind(user_id)
        .bind(used)
        .execute(app.database.pool())
        .await
        .expect("设置已用量");
}

/// 一个带一个 content/version/chunk 的文件夹具。
async fn insert_file(
    app: &TestApp,
    user_id: Uuid,
    parent_id: Uuid,
    size: i64,
    token_seed: u16,
    object_key: &str,
) -> (Uuid, Uuid, Uuid) {
    let node_id = insert_node(
        app,
        user_id,
        parent_id,
        "file",
        &token(token_seed),
        &meta(token_seed),
    )
    .await;
    let content_id = Uuid::new_v4();
    insert_content(
        app,
        user_id,
        node_id,
        content_id,
        size,
        &[(object_key, size)],
    )
    .await;
    let version_id = Uuid::new_v4();
    insert_version(
        app,
        user_id,
        node_id,
        version_id,
        content_id,
        "0 seconds",
        &meta(token_seed.wrapping_add(1)),
    )
    .await;
    set_current_version(app, user_id, node_id, version_id).await;
    (node_id, version_id, content_id)
}

async fn count(app: &TestApp, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(app.database.pool())
        .await
        .expect("计数查询")
}

// ---------------------------------------------------------------------------
// 7.1 / 7.7 根节点与用量
// ---------------------------------------------------------------------------

#[tokio::test]
async fn root_folder_cannot_be_deleted_renamed_or_moved() {
    let app = setup().await;
    let (_user, root, cookie) = user_with_session(&app).await;

    for (method, uri, body) in [
        (Method::DELETE, format!("/api/v1/nodes/{root}"), None),
        (
            Method::PATCH,
            format!("/api/v1/nodes/{root}/rename"),
            Some(json!({
                "row_version": 1,
                "encrypted_meta": meta(0x20),
                "name_token": token(0x21),
            })),
        ),
        (
            Method::PATCH,
            format!("/api/v1/nodes/{root}/move"),
            Some(json!({
                "new_parent_id": root,
                "row_version": 1,
                "name_token": token(0x22),
            })),
        ),
    ] {
        let (status, _headers, body) =
            read(send(&app.app, method, &uri, body, Some(&cookie), true).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error_code(&body), "operation_not_allowed");
    }
}

#[tokio::test]
async fn registration_creates_root_folder_without_key_material() {
    let app = setup().await;
    let email = format!("root-{}@example.com", Uuid::new_v4());

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::POST,
            "/api/v1/auth/register",
            Some(json!({
                "email": email,
                "kdf_params": {
                    "algorithm": "argon2id",
                    "m_cost_kib": 65536,
                    "t_cost": 3,
                    "p_cost": 1,
                },
                "kdf_salt": hex(&[0x11; 16]),
                "auth_key": hex(&[0x22; 32]),
                "wrapped_mk": wrapped(0x23),
                "recovery_wrapped_mk": wrapped(0x24),
                "recovery_auth_key": hex(&[0x25; 32]),
            })),
            None,
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true }));

    let row = sqlx::query("SELECT id, root_node_id FROM users WHERE email = $1::text::citext")
        .bind(&email)
        .fetch_one(app.database.pool())
        .await
        .expect("读取注册用户");
    let user_id: Uuid = row.get("id");
    let root_id: Uuid = row.get("root_node_id");

    let node = sqlx::query(
        "SELECT kind, parent_id, wrapped_key, encrypted_meta, name_token FROM nodes WHERE id = $1 AND user_id = $2",
    )
    .bind(root_id)
    .bind(user_id)
    .fetch_one(app.database.pool())
    .await
    .expect("注册必须创建根节点");
    assert_eq!(node.get::<String, _>("kind"), "folder");
    assert!(node.get::<Option<Uuid>, _>("parent_id").is_none());
    assert!(node.get::<Option<Vec<u8>>, _>("wrapped_key").is_none());
    assert!(node.get::<Option<Vec<u8>>, _>("encrypted_meta").is_none());
    assert!(node.get::<Option<Vec<u8>>, _>("name_token").is_none());
}

#[tokio::test]
async fn usage_reports_used_reserved_and_quota() {
    let app = setup().await;
    let (user_id, _root, cookie) = user_with_session(&app).await;
    sqlx::query(
        "UPDATE users SET used_bytes = 1234, reserved_bytes = 56, quota_bytes = 98765 WHERE id = $1",
    )
    .bind(user_id)
    .execute(app.database.pool())
    .await
    .expect("设置用量");

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::GET,
            "/api/v1/me/usage",
            None,
            Some(&cookie),
            false,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["used_bytes"], 1234);
    assert_eq!(body["reserved_bytes"], 56);
    assert_eq!(body["quota_bytes"], 98765);
}

// ---------------------------------------------------------------------------
// 7.2 列目录 / 创建 / 重命名
// ---------------------------------------------------------------------------

#[tokio::test]
async fn listing_5000_children_is_paginated_without_gaps_or_duplicates() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;

    let mut tx = app.database.pool().begin().await.expect("开始批量插入");
    for index in 0..5000_usize {
        sqlx::query(
            r#"
            INSERT INTO nodes (id, user_id, parent_id, kind, wrapped_key, encrypted_meta, name_token)
            VALUES ($1, $2, $3, 'folder', $4, $5, $6)
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(root)
        .bind(server::auth::decode_hex_vec(&wrapped(0x31)).expect("密钥"))
        .bind(server::auth::decode_hex_vec(&meta(0x32)).expect("元数据"))
        .bind(server::auth::decode_hex_vec(&unique_token(index)).expect("令牌"))
        .execute(&mut *tx)
        .await
        .expect("批量插入子项");
    }
    tx.commit().await.expect("提交批量插入");

    let mut seen = std::collections::HashSet::new();
    let mut cursor: Option<String> = None;
    loop {
        let uri = match &cursor {
            Some(cursor) => format!("/api/v1/nodes/{root}/children?limit=137&cursor={cursor}"),
            None => format!("/api/v1/nodes/{root}/children?limit=137"),
        };
        let (status, _headers, body) =
            read(send(&app.app, Method::GET, &uri, None, Some(&cookie), false).await).await;
        assert_eq!(status, StatusCode::OK);
        let items = body["items"].as_array().expect("items");
        assert!(items.len() <= 137);
        for item in items {
            let id = item["id"].as_str().expect("id").to_owned();
            assert!(seen.insert(id), "分页不得返回重复项");
        }
        match body["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 5000, "分页必须取回全部子项");
}

#[tokio::test]
async fn create_folder_is_idempotent_and_stores_only_ciphertext() {
    let app = setup().await;
    let (_user_id, root, cookie) = user_with_session(&app).await;
    let folder_id = Uuid::new_v4();
    let body = json!({
        "id": folder_id,
        "parent_id": root,
        "wrapped_key": wrapped(0x41),
        "encrypted_meta": meta(0x42),
        "name_token": token(0x43),
    });

    let (status, _headers, first) = read(
        send(
            &app.app,
            Method::POST,
            "/api/v1/folders",
            Some(body.clone()),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["id"], folder_id.to_string());
    assert_eq!(first["kind"], "folder");

    let (status, _headers, second) = read(
        send(
            &app.app,
            Method::POST,
            "/api/v1/folders",
            Some(body),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first, second, "同一 ID 同一内容必须返回同一结果");

    // 同一 ID 不同内容 → conflict。
    let (status, _headers, conflict) = read(
        send(
            &app.app,
            Method::POST,
            "/api/v1/folders",
            Some(json!({
                "id": folder_id,
                "parent_id": root,
                "wrapped_key": wrapped(0x41),
                "encrypted_meta": meta(0x99),
                "name_token": token(0x43),
            })),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&conflict), "conflict");

    // 服务端数据库只有密文，没有明文文件名。
    let plaintext_rows = count(
        &app,
        "SELECT count(*) FROM nodes WHERE encode(encrypted_meta, 'escape') LIKE '%照片%'",
    )
    .await;
    assert_eq!(plaintext_rows, 0);

    let (status, _headers, children) = read(
        send(
            &app.app,
            Method::GET,
            &format!("/api/v1/nodes/{root}/children"),
            None,
            Some(&cookie),
            false,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(children["items"].as_array().expect("items").len(), 1);
}

#[tokio::test]
async fn create_folder_under_trashed_parent_returns_target_unavailable() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let parent = insert_node(&app, user_id, root, "folder", &token(0x51), &meta(0x52)).await;

    let (status, _headers, _body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/nodes/{parent}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::POST,
            "/api/v1/folders",
            Some(json!({
                "id": Uuid::new_v4(),
                "parent_id": parent,
                "wrapped_key": wrapped(0x53),
                "encrypted_meta": meta(0x54),
                "name_token": token(0x55),
            })),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "target_unavailable");
}

#[tokio::test]
async fn duplicate_name_in_same_folder_returns_name_conflict() {
    let app = setup().await;
    let (_user, root, cookie) = user_with_session(&app).await;

    let first = send(
        &app.app,
        Method::POST,
        "/api/v1/folders",
        Some(json!({
            "id": Uuid::new_v4(),
            "parent_id": root,
            "wrapped_key": wrapped(0x61),
            "encrypted_meta": meta(0x62),
            "name_token": token(0x63),
        })),
        Some(&cookie),
        true,
    )
    .await;
    assert_eq!(read(first).await.0, StatusCode::OK);

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::POST,
            "/api/v1/folders",
            Some(json!({
                "id": Uuid::new_v4(),
                "parent_id": root,
                "wrapped_key": wrapped(0x64),
                "encrypted_meta": meta(0x65),
                "name_token": token(0x63),
            })),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "name_conflict");
}

#[tokio::test]
async fn concurrent_rename_has_exactly_one_winner() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let file = insert_file(&app, user_id, root, 100, 0x71, "k-rename-1").await;

    let body_a = json!({
        "row_version": 1,
        "encrypted_meta": meta(0x72),
        "name_token": token(0x73),
    });
    let body_b = json!({
        "row_version": 1,
        "encrypted_meta": meta(0x74),
        "name_token": token(0x75),
    });
    let uri = format!("/api/v1/nodes/{}/rename", file.0);
    let (first, second) = tokio::join!(
        send(
            &app.app,
            Method::PATCH,
            &uri,
            Some(body_a),
            Some(&cookie),
            true
        ),
        send(
            &app.app,
            Method::PATCH,
            &uri,
            Some(body_b),
            Some(&cookie),
            true
        ),
    );
    let first = read(first).await;
    let second = read(second).await;

    let statuses = [first.0, second.0];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == StatusCode::OK)
            .count(),
        1,
        "只能有一个重命名成功"
    );
    let conflict = if first.0 == StatusCode::CONFLICT {
        first.2
    } else {
        second.2
    };
    assert_eq!(error_code(&conflict), "conflict");

    let version: i32 = sqlx::query_scalar("SELECT row_version FROM nodes WHERE id = $1")
        .bind(file.0)
        .fetch_one(app.database.pool())
        .await
        .expect("读取版本号");
    assert_eq!(version, 2);
}

#[tokio::test]
async fn rename_to_existing_name_returns_name_conflict() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let first = insert_node(&app, user_id, root, "folder", &token(0x81), &meta(0x82)).await;
    let second = insert_node(&app, user_id, root, "folder", &token(0x83), &meta(0x84)).await;
    assert_ne!(first, second);

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::PATCH,
            &format!("/api/v1/nodes/{second}/rename"),
            Some(json!({
                "row_version": 1,
                "encrypted_meta": meta(0x85),
                "name_token": token(0x81),
            })),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "name_conflict");
}

// ---------------------------------------------------------------------------
// 7.3 移动
// ---------------------------------------------------------------------------

#[tokio::test]
async fn move_into_own_descendant_is_rejected() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let a = insert_node(&app, user_id, root, "folder", &token(0x91), &meta(0x92)).await;
    let b = insert_node(&app, user_id, a, "folder", &token(0x93), &meta(0x94)).await;

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::PATCH,
            &format!("/api/v1/nodes/{a}/move"),
            Some(json!({
                "new_parent_id": b,
                "row_version": 1,
                "name_token": token(0x95),
            })),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "invalid_move");

    let a_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1")
        .bind(a)
        .fetch_one(app.database.pool())
        .await
        .expect("读取父节点");
    let b_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1")
        .bind(b)
        .fetch_one(app.database.pool())
        .await
        .expect("读取父节点");
    assert_eq!(a_parent, Some(root), "树结构不得改变");
    assert_eq!(b_parent, Some(a));
}

#[tokio::test]
async fn move_to_folder_with_existing_name_returns_name_conflict() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let source_parent = insert_node(&app, user_id, root, "folder", &token(0xa1), &meta(0xa2)).await;
    let target_parent = insert_node(&app, user_id, root, "folder", &token(0xa3), &meta(0xa4)).await;
    let moved = insert_node(
        &app,
        user_id,
        source_parent,
        "folder",
        &token(0xa5),
        &meta(0xa6),
    )
    .await;
    let _existing = insert_node(
        &app,
        user_id,
        target_parent,
        "folder",
        &token(0xa5),
        &meta(0xa7),
    )
    .await;

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::PATCH,
            &format!("/api/v1/nodes/{moved}/move"),
            Some(json!({
                "new_parent_id": target_parent,
                "row_version": 1,
                "name_token": token(0xa5),
            })),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "name_conflict");

    let parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1")
        .bind(moved)
        .fetch_one(app.database.pool())
        .await
        .expect("读取父节点");
    assert_eq!(parent, Some(source_parent));
}

#[tokio::test]
async fn concurrent_cross_moves_cannot_create_a_cycle() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let a = insert_node(&app, user_id, root, "folder", &token(0xb1), &meta(0xb2)).await;
    let b = insert_node(&app, user_id, root, "folder", &token(0xb3), &meta(0xb4)).await;

    let move_a = format!("/api/v1/nodes/{a}/move");
    let move_b = format!("/api/v1/nodes/{b}/move");
    let (first, second) = tokio::join!(
        send(
            &app.app,
            Method::PATCH,
            &move_a,
            Some(json!({
                "new_parent_id": b,
                "row_version": 1,
                "name_token": token(0xb5),
            })),
            Some(&cookie),
            true,
        ),
        send(
            &app.app,
            Method::PATCH,
            &move_b,
            Some(json!({
                "new_parent_id": a,
                "row_version": 1,
                "name_token": token(0xb6),
            })),
            Some(&cookie),
            true,
        ),
    );
    let first = read(first).await;
    let second = read(second).await;

    let successes = [&first, &second]
        .iter()
        .filter(|(status, _headers, _body)| *status == StatusCode::OK)
        .count();
    assert_eq!(successes, 1, "并发交叉移动只能有一个成功");
    let failed = if first.0 == StatusCode::OK {
        &second.2
    } else {
        &first.2
    };
    assert_eq!(error_code(failed), "invalid_move");

    let a_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1")
        .bind(a)
        .fetch_one(app.database.pool())
        .await
        .expect("读取 A 父节点");
    let b_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1")
        .bind(b)
        .fetch_one(app.database.pool())
        .await
        .expect("读取 B 父节点");

    // 要么 B 在根、A 在 B 下，要么 A 在根、B 在 A 下；绝不能互相为父。
    assert!(
        (a_parent == Some(b) && b_parent == Some(root))
            || (b_parent == Some(a) && a_parent == Some(root)),
        "移动后必须无环：A.parent={a_parent:?}, B.parent={b_parent:?}"
    );
}

// ---------------------------------------------------------------------------
// 7.4 回收站
// ---------------------------------------------------------------------------

#[tokio::test]
async fn soft_delete_marks_subtree_and_lists_only_top_level() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let a = insert_node(&app, user_id, root, "folder", &token(0xc1), &meta(0xc2)).await;
    let b = insert_node(&app, user_id, a, "folder", &token(0xc3), &meta(0xc4)).await;
    let c = insert_node(&app, user_id, b, "file", &token(0xc5), &meta(0xc6)).await;

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/nodes/{a}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let batch = body["trash_batch_id"].as_str().expect("batch").to_owned();

    let rows = sqlx::query(
        "SELECT id, trash_batch_id::text AS batch, deleted_at IS NOT NULL AS deleted FROM nodes WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_all(app.database.pool())
    .await
    .expect("读取节点");
    for row in rows {
        let id: Uuid = row.get("id");
        if id == root {
            continue;
        }
        assert!(row.get::<bool, _>("deleted"));
        assert_eq!(row.get::<String, _>("batch"), batch);
    }

    // 重复删除幂等：返回同一个 batch。
    let (status, _headers, again) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/nodes/{a}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["trash_batch_id"], batch);

    let (status, _headers, trash) = read(
        send(
            &app.app,
            Method::GET,
            "/api/v1/trash",
            None,
            Some(&cookie),
            false,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = trash["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "只列出 batch 顶层项");
    assert_eq!(items[0]["id"], a.to_string());
    let _ = (b, c);

    let (status, _headers, children) = read(
        send(
            &app.app,
            Method::GET,
            &format!("/api/v1/nodes/{root}/children"),
            None,
            Some(&cookie),
            false,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(children["items"].as_array().expect("items").is_empty());
}

#[tokio::test]
async fn restore_returns_item_and_descendants_to_original_parent() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let a = insert_node(&app, user_id, root, "folder", &token(0xd1), &meta(0xd2)).await;
    let b = insert_node(&app, user_id, a, "file", &token(0xd3), &meta(0xd4)).await;

    let (status, _headers, _body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/nodes/{a}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _headers, restored) = read(
        send(
            &app.app,
            Method::POST,
            &format!("/api/v1/trash/{a}/restore"),
            Some(json!({})),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["id"], a.to_string());
    assert!(restored["deleted_at"].is_null());

    let a_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1")
        .bind(a)
        .fetch_one(app.database.pool())
        .await
        .expect("读取父节点");
    let b_parent: Option<Uuid> = sqlx::query_scalar("SELECT parent_id FROM nodes WHERE id = $1")
        .bind(b)
        .fetch_one(app.database.pool())
        .await
        .expect("读取父节点");
    assert_eq!(a_parent, Some(root));
    assert_eq!(b_parent, Some(a));
}

#[tokio::test]
async fn restore_reports_unavailable_original_parent_and_accepts_new_parent() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let original_parent =
        insert_node(&app, user_id, root, "folder", &token(0xe1), &meta(0xe2)).await;
    let other_parent = insert_node(&app, user_id, root, "folder", &token(0xe3), &meta(0xe4)).await;
    let item = insert_node(
        &app,
        user_id,
        original_parent,
        "folder",
        &token(0xe5),
        &meta(0xe6),
    )
    .await;

    // 先把 item 放入回收站。
    let (status, _headers, _body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/nodes/{item}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // 再把原父节点永久删除。
    let (status, _headers, _body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/nodes/{original_parent}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _headers, _body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/trash/{original_parent}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::POST,
            &format!("/api/v1/trash/{item}/restore"),
            Some(json!({})),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "target_unavailable");

    let (status, _headers, restored) = read(
        send(
            &app.app,
            Method::POST,
            &format!("/api/v1/trash/{item}/restore"),
            Some(json!({
                "new_parent_id": other_parent,
                "name_token": token(0xe7),
            })),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["parent_id"], other_parent.to_string());
}

#[tokio::test]
async fn restore_name_conflict_returns_name_conflict() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let parent = insert_node(&app, user_id, root, "folder", &token(0xf1), &meta(0xf2)).await;
    let item = insert_node(&app, user_id, parent, "folder", &token(0xf3), &meta(0xf4)).await;

    let (status, _headers, _body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/nodes/{item}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let _conflicting =
        insert_node(&app, user_id, parent, "folder", &token(0xf3), &meta(0xf5)).await;

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::POST,
            &format!("/api/v1/trash/{item}/restore"),
            Some(json!({})),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "name_conflict");
}

#[tokio::test]
async fn permanent_delete_removes_tree_frees_quota_and_queues_objects() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let folder = insert_node(&app, user_id, root, "folder", &token(0x100), &meta(0x101)).await;
    let (file, _version, _content) =
        insert_file(&app, user_id, folder, 400, 0x102, "k-permanent-1").await;
    // 第二个 chunk 让删除时能验证整条 chunk 链。
    let content_id: Uuid =
        sqlx::query_scalar("SELECT content_id FROM file_versions WHERE node_id = $1")
            .bind(file)
            .fetch_one(app.database.pool())
            .await
            .expect("读取 content");
    sqlx::query(
        "UPDATE contents SET chunk_count = 2, ciphertext_size = 400 WHERE id = $1 AND user_id = $2",
    )
    .bind(content_id)
    .bind(user_id)
    .execute(app.database.pool())
    .await
    .expect("更新 content");
    sqlx::query(
        "INSERT INTO chunks (content_id, user_id, idx, object_key, ciphertext_size, sha256) VALUES ($1, $2, 1, $3, 200, $4)",
    )
    .bind(content_id)
    .bind(user_id)
    .bind("k-permanent-2")
    .bind([9_u8; 32].as_slice())
    .execute(app.database.pool())
    .await
    .expect("插入第二个 chunk");
    set_used_bytes(&app, user_id, 400).await;

    let (status, _headers, _body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/nodes/{folder}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/trash/{folder}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["deleted_nodes"], 2);
    assert_eq!(body["deleted_versions"], 1);
    assert_eq!(body["deleted_contents"], 1);
    assert_eq!(body["freed_bytes"], 400);
    assert_eq!(body["queued_objects"], 2);

    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM nodes WHERE kind = 'file' AND deleted_at IS NOT NULL"
        )
        .await,
        0
    );
    assert_eq!(count(&app, "SELECT count(*) FROM contents").await, 0);
    assert_eq!(count(&app, "SELECT count(*) FROM chunks").await, 0);
    assert_eq!(count(&app, "SELECT count(*) FROM file_versions").await, 0);
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM object_deletions WHERE object_key LIKE 'k-permanent-%'"
        )
        .await,
        2
    );
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("读取用量");
    assert_eq!(used, 0);

    // 重复永久删除：节点已不存在 → 404（与跨用户不可区分）。
    let (status, _headers, _body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/trash/{folder}"),
            None,
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// 7.5 版本
// ---------------------------------------------------------------------------

#[tokio::test]
async fn version_list_and_restore_reuse_content_without_changing_usage() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;
    let file = insert_node(&app, user_id, root, "file", &token(0x110), &meta(0x111)).await;

    let old_content = Uuid::new_v4();
    insert_content(
        &app,
        user_id,
        file,
        old_content,
        100,
        &[("k-version-old", 100)],
    )
    .await;
    let old_version = Uuid::new_v4();
    insert_version(
        &app,
        user_id,
        file,
        old_version,
        old_content,
        "-3 days",
        &meta(0x112),
    )
    .await;

    let new_content = Uuid::new_v4();
    insert_content(
        &app,
        user_id,
        file,
        new_content,
        200,
        &[("k-version-new", 200)],
    )
    .await;
    let current_version = Uuid::new_v4();
    insert_version(
        &app,
        user_id,
        file,
        current_version,
        new_content,
        "0 seconds",
        &meta(0x113),
    )
    .await;
    set_current_version(&app, user_id, file, current_version).await;
    set_used_bytes(&app, user_id, 300).await;

    let (status, _headers, versions) = read(
        send(
            &app.app,
            Method::GET,
            &format!("/api/v1/nodes/{file}/versions"),
            None,
            Some(&cookie),
            false,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let items = versions["items"].as_array().expect("items");
    assert_eq!(items.len(), 2);
    assert_eq!(
        items
            .iter()
            .filter(|item| item["is_current"] == true)
            .count(),
        1
    );

    let restored_version = Uuid::new_v4();
    let restore_body = json!({
        "source_version_id": old_version,
        "new_version_id": restored_version,
        "encrypted_meta": meta(0x114),
    });
    let (status, _headers, restored) = read(
        send(
            &app.app,
            Method::POST,
            &format!("/api/v1/nodes/{file}/versions/restore"),
            Some(restore_body.clone()),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["id"], restored_version.to_string());
    assert_eq!(restored["content_id"], old_content.to_string());
    assert_eq!(restored["is_current"], true);

    // 用量不变，content/chunks 不复制。
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("读取用量");
    assert_eq!(used, 300);
    assert_eq!(count(&app, "SELECT count(*) FROM contents").await, 2);
    assert_eq!(count(&app, "SELECT count(*) FROM chunks").await, 2);
    assert_eq!(count(&app, "SELECT count(*) FROM file_versions").await, 3);

    // 幂等：重复提交相同 new_version_id + 内容返回同一版本。
    let (status, _headers, again) = read(
        send(
            &app.app,
            Method::POST,
            &format!("/api/v1/nodes/{file}/versions/restore"),
            Some(restore_body),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, restored);
    assert_eq!(count(&app, "SELECT count(*) FROM file_versions").await, 3);

    // 同一 new_version_id 不同内容 → conflict。
    let (status, _headers, body) = read(
        send(
            &app.app,
            Method::POST,
            &format!("/api/v1/nodes/{file}/versions/restore"),
            Some(json!({
                "source_version_id": old_version,
                "new_version_id": restored_version,
                "encrypted_meta": meta(0x115),
            })),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "conflict");
}

// ---------------------------------------------------------------------------
// 7.6 扫描任务
// ---------------------------------------------------------------------------

#[tokio::test]
async fn version_cleanup_removes_only_old_versions_beyond_latest_ten() {
    let app = setup().await;
    let (user_id, root, _cookie) = user_with_session(&app).await;
    let file = insert_node(&app, user_id, root, "file", &token(0x120), &meta(0x121)).await;

    let mut versions = Vec::new();
    for index in 0..11_u64 {
        let content = Uuid::new_v4();
        let key = format!("k-retention-{index}");
        insert_content(&app, user_id, file, content, 100, &[(&key, 100)]).await;
        let version = Uuid::new_v4();
        let offset = if index == 0 { "-40 days" } else { "-5 days" };
        insert_version(&app, user_id, file, version, content, offset, &meta(0x122)).await;
        versions.push((version, content));
    }
    // 最后一个版本是当前版本；它即使很旧也不能被清理。
    set_current_version(&app, user_id, file, versions[10].0).await;
    set_used_bytes(&app, user_id, 1100).await;

    let outcome = run_once(&app.database, &VersionRetentionCleanup, 100)
        .await
        .expect("版本清理");
    assert!(matches!(outcome, ScanOutcome::Executed(1)));

    assert_eq!(count(&app, "SELECT count(*) FROM file_versions").await, 10);
    let oldest_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM file_versions WHERE id = $1)")
            .bind(versions[0].0)
            .fetch_one(app.database.pool())
            .await
            .expect("查询最旧版本");
    assert!(!oldest_exists);
    let content_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM contents WHERE id = $1)")
            .bind(versions[0].1)
            .fetch_one(app.database.pool())
            .await
            .expect("查询 content");
    assert!(!content_exists);
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM object_deletions WHERE object_key = 'k-retention-0'"
        )
        .await,
        1
    );
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("读取用量");
    assert_eq!(used, 1000);
}

#[tokio::test]
async fn version_cleanup_keeps_recent_versions_and_current_version() {
    let app = setup().await;
    let (user_id, root, _cookie) = user_with_session(&app).await;
    let file = insert_node(&app, user_id, root, "file", &token(0x130), &meta(0x131)).await;

    // 11 个版本里最旧的只有 5 天：未过保留期，不清理。
    let mut versions = Vec::new();
    for index in 0..11_u64 {
        let content = Uuid::new_v4();
        let key = format!("k-recent-{index}");
        insert_content(&app, user_id, file, content, 100, &[(&key, 100)]).await;
        let version = Uuid::new_v4();
        insert_version(
            &app,
            user_id,
            file,
            version,
            content,
            "-5 days",
            &meta(0x132),
        )
        .await;
        versions.push(version);
    }
    set_current_version(&app, user_id, file, versions[10]).await;

    let outcome = run_once(&app.database, &VersionRetentionCleanup, 100)
        .await
        .expect("版本清理");
    assert!(matches!(outcome, ScanOutcome::Executed(0)));
    assert_eq!(count(&app, "SELECT count(*) FROM file_versions").await, 11);
}

#[tokio::test]
async fn version_cleanup_never_removes_the_current_version() {
    let app = setup().await;
    let (user_id, root, _cookie) = user_with_session(&app).await;
    let file = insert_node(&app, user_id, root, "file", &token(0x135), &meta(0x136)).await;

    // 11 个版本都早于 30 天；把最旧的版本设为当前版本，它仍然不能被清理。
    let mut versions = Vec::new();
    for index in 0..11_u64 {
        let content = Uuid::new_v4();
        let key = format!("k-current-{index}");
        insert_content(&app, user_id, file, content, 100, &[(&key, 100)]).await;
        let version = Uuid::new_v4();
        let offset = if index == 0 { "-50 days" } else { "-40 days" };
        insert_version(&app, user_id, file, version, content, offset, &meta(0x137)).await;
        versions.push(version);
    }
    set_current_version(&app, user_id, file, versions[0]).await;

    let outcome = run_once(&app.database, &VersionRetentionCleanup, 100)
        .await
        .expect("版本清理");
    assert!(matches!(outcome, ScanOutcome::Executed(0)));
    assert_eq!(count(&app, "SELECT count(*) FROM file_versions").await, 11);
    assert_eq!(count(&app, "SELECT count(*) FROM contents").await, 11);
}

#[tokio::test]
async fn trash_cleanup_permanently_deletes_only_expired_batches() {
    let app = setup().await;
    let (user_id, root, cookie) = user_with_session(&app).await;

    let expired = insert_node(&app, user_id, root, "folder", &token(0x140), &meta(0x141)).await;
    let (_expired_file, _version, _content) =
        insert_file(&app, user_id, expired, 300, 0x142, "k-trash-expired").await;
    let recent = insert_node(&app, user_id, root, "folder", &token(0x143), &meta(0x144)).await;
    set_used_bytes(&app, user_id, 300).await;

    for node in [expired, recent] {
        let (status, _headers, _body) = read(
            send(
                &app.app,
                Method::DELETE,
                &format!("/api/v1/nodes/{node}"),
                None,
                Some(&cookie),
                true,
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    sqlx::query(
        "UPDATE nodes SET deleted_at = now() - interval '31 days' WHERE trash_batch_id = (SELECT trash_batch_id FROM nodes WHERE id = $1)",
    )
    .bind(expired)
    .execute(app.database.pool())
    .await
    .expect("设置过期回收站时间");

    let outcome = run_once(&app.database, &TrashCleanup, 100)
        .await
        .expect("回收站清理");
    assert!(matches!(outcome, ScanOutcome::Executed(2)));

    let expired_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM nodes WHERE id = $1)")
            .bind(expired)
            .fetch_one(app.database.pool())
            .await
            .expect("查询过期节点");
    assert!(!expired_exists);
    let recent_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM nodes WHERE id = $1)")
            .bind(recent)
            .fetch_one(app.database.pool())
            .await
            .expect("查询近期节点");
    assert!(recent_exists);
    assert_eq!(
        count(
            &app,
            "SELECT count(*) FROM object_deletions WHERE object_key = 'k-trash-expired'"
        )
        .await,
        1
    );
    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("读取用量");
    assert_eq!(used, 0);
}

#[tokio::test]
async fn quota_reconciliation_reports_but_never_corrects() {
    let app = setup().await;
    let (user_id, root, _cookie) = user_with_session(&app).await;
    let (_file, _version, _content) =
        insert_file(&app, user_id, root, 500, 0x150, "k-quota-1").await;
    set_used_bytes(&app, user_id, 623).await;

    let outcome = run_once(&app.database, &QuotaReconciliation, 100)
        .await
        .expect("配额对账");
    assert!(matches!(outcome, ScanOutcome::Executed(1)));

    let used: i64 = sqlx::query_scalar("SELECT used_bytes FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(app.database.pool())
        .await
        .expect("读取用量");
    assert_eq!(used, 623, "对账只能告警，不得自动修正");
}

// ---------------------------------------------------------------------------
// 7.7 跨用户隔离
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_file_endpoint_returns_404_for_another_users_node() {
    let app = setup().await;
    let (owner_id, owner_root, _owner_cookie) = user_with_session(&app).await;
    let (_attacker_id, attacker_root, attacker_cookie) = user_with_session(&app).await;

    let other_folder = insert_node(
        &app,
        owner_id,
        owner_root,
        "folder",
        &token(0x160),
        &meta(0x161),
    )
    .await;
    let (other_file, _version, _content) =
        insert_file(&app, owner_id, other_folder, 120, 0x162, "k-cross-1").await;
    let (attacker_file, _av, _ac) =
        insert_file(&app, _attacker_id, attacker_root, 90, 0x163, "k-cross-2").await;

    // 把 other_file 放入回收站，供 restore/permanent delete 使用。
    let (status, _headers, _body) = read(
        send(
            &app.app,
            Method::DELETE,
            &format!("/api/v1/nodes/{other_file}"),
            None,
            Some(&_owner_cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let requests = vec![
        (
            Method::GET,
            format!("/api/v1/nodes/{other_folder}/children"),
            None,
        ),
        (
            Method::POST,
            "/api/v1/folders".to_owned(),
            Some(json!({
                "id": Uuid::new_v4(),
                "parent_id": other_folder,
                "wrapped_key": wrapped(0x164),
                "encrypted_meta": meta(0x165),
                "name_token": token(0x166),
            })),
        ),
        (
            Method::PATCH,
            format!("/api/v1/nodes/{other_file}/rename"),
            Some(json!({
                "row_version": 1,
                "encrypted_meta": meta(0x167),
                "name_token": token(0x168),
            })),
        ),
        (
            Method::PATCH,
            format!("/api/v1/nodes/{other_file}/move"),
            Some(json!({
                "new_parent_id": attacker_root,
                "row_version": 1,
                "name_token": token(0x169),
            })),
        ),
        (
            Method::PATCH,
            format!("/api/v1/nodes/{attacker_file}/move"),
            Some(json!({
                "new_parent_id": other_folder,
                "row_version": 1,
                "name_token": token(0x16a),
            })),
        ),
        (Method::DELETE, format!("/api/v1/nodes/{other_file}"), None),
        (
            Method::POST,
            format!("/api/v1/trash/{other_file}/restore"),
            Some(json!({})),
        ),
        (Method::DELETE, format!("/api/v1/trash/{other_file}"), None),
        (
            Method::GET,
            format!("/api/v1/nodes/{other_file}/versions"),
            None,
        ),
        (
            Method::POST,
            format!("/api/v1/nodes/{other_file}/versions/restore"),
            Some(json!({
                "source_version_id": Uuid::new_v4(),
                "new_version_id": Uuid::new_v4(),
                "encrypted_meta": meta(0x16b),
            })),
        ),
    ];

    for (method, uri, body) in requests {
        let (status, _headers, response) = read(
            send(
                &app.app,
                method.clone(),
                &uri,
                body,
                Some(&attacker_cookie),
                method != Method::GET,
            )
            .await,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {uri} 必须返回 404，实际 {status} {response}"
        );
    }

    // 跨用户请求不得修改任何数据。
    let still_deleted: bool =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM nodes WHERE id = $1")
            .bind(other_file)
            .fetch_one(app.database.pool())
            .await
            .expect("读取节点状态");
    assert!(still_deleted);
}

#[tokio::test]
async fn cross_user_relative_foreign_keys_are_rejected_by_the_database() {
    let app = setup().await;
    let (owner, owner_root, _owner_cookie) = user_with_session(&app).await;
    let (attacker, attacker_root, _attacker_cookie) = user_with_session(&app).await;

    let owner_folder = insert_node(
        &app,
        owner,
        owner_root,
        "folder",
        &token(0x170),
        &meta(0x171),
    )
    .await;
    let attacker_folder = insert_node(
        &app,
        attacker,
        attacker_root,
        "folder",
        &token(0x172),
        &meta(0x173),
    )
    .await;

    let wrapped = server::auth::decode_hex_vec(&wrapped(0x174)).expect("包裹密钥");
    let encrypted_meta = server::auth::decode_hex_vec(&meta(0x175)).expect("元数据");
    let name_token = server::auth::decode_hex_vec(&token(0x176)).expect("令牌");

    // nodes.parent_id 必须与 user_id 指向同一用户。
    let cross_parent = sqlx::query(
        r#"
        INSERT INTO nodes (
            id, user_id, parent_id, kind, wrapped_key, encrypted_meta, name_token
        )
        VALUES ($1, $2, $3, 'folder', $4, $5, $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(attacker)
    .bind(owner_folder)
    .bind(&wrapped)
    .bind(&encrypted_meta)
    .bind(&name_token)
    .execute(app.database.pool())
    .await;
    assert!(
        cross_parent.is_err(),
        "引用他人父节点的 nodes 行必须被复合外键拒绝"
    );

    // contents.node_id 必须与 user_id 指向同一用户。
    let owner_content = Uuid::new_v4();
    insert_content(
        &app,
        owner,
        owner_folder,
        owner_content,
        10,
        &[("k-fk-owner", 10)],
    )
    .await;
    let cross_content = sqlx::query(
        r#"
        INSERT INTO contents (id, user_id, node_id, chunk_count, ciphertext_size)
        VALUES ($1, $2, $3, 1, 10)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(attacker)
    .bind(owner_folder)
    .execute(app.database.pool())
    .await;
    assert!(
        cross_content.is_err(),
        "引用他人节点的 contents 行必须被复合外键拒绝"
    );

    // chunks.content_id 必须与 user_id 指向同一用户。
    let cross_chunk = sqlx::query(
        r#"
        INSERT INTO chunks (content_id, user_id, idx, object_key, ciphertext_size, sha256)
        VALUES ($1, $2, 0, $3, 10, $4)
        "#,
    )
    .bind(owner_content)
    .bind(attacker)
    .bind("k-fk-cross-chunk")
    .bind([3_u8; 32].as_slice())
    .execute(app.database.pool())
    .await;
    assert!(
        cross_chunk.is_err(),
        "引用他人 content 的 chunks 行必须被复合外键拒绝"
    );

    // 攻击者自己的 content，供“content 归属正确但 node 属于他人”的版本测试使用。
    let attacker_content = Uuid::new_v4();
    insert_content(
        &app,
        attacker,
        attacker_folder,
        attacker_content,
        10,
        &[("k-fk-attacker", 10)],
    )
    .await;

    // file_versions.node_id 必须与 user_id 指向同一用户。
    let cross_version_node = sqlx::query(
        r#"
        INSERT INTO file_versions (id, user_id, node_id, content_id, encrypted_meta)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(attacker)
    .bind(owner_folder)
    .bind(attacker_content)
    .bind(&encrypted_meta)
    .execute(app.database.pool())
    .await;
    assert!(
        cross_version_node.is_err(),
        "引用他人节点的 file_versions 行必须被复合外键拒绝"
    );

    // file_versions.content_id 必须与 user_id 指向同一用户。
    let cross_version_content = sqlx::query(
        r#"
        INSERT INTO file_versions (id, user_id, node_id, content_id, encrypted_meta)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(attacker)
    .bind(attacker_folder)
    .bind(owner_content)
    .bind(&encrypted_meta)
    .execute(app.database.pool())
    .await;
    assert!(
        cross_version_content.is_err(),
        "引用他人 content 的 file_versions 行必须被复合外键拒绝"
    );

    // nodes.current_version_id 必须与 user_id 指向同一用户。
    let owner_version = Uuid::new_v4();
    insert_version(
        &app,
        owner,
        owner_folder,
        owner_version,
        owner_content,
        "0 seconds",
        &meta(0x177),
    )
    .await;
    let cross_current =
        sqlx::query("UPDATE nodes SET current_version_id = $1 WHERE id = $2 AND user_id = $3")
            .bind(owner_version)
            .bind(attacker_folder)
            .bind(attacker)
            .execute(app.database.pool())
            .await;
    assert!(
        cross_current.is_err(),
        "current_version_id 指向他人版本必须被复合外键拒绝"
    );
}
