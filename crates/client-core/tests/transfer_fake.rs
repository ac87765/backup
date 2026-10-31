//! 不需要 Docker 的传输测试：本地假服务端模拟 429、确认失败与进程重启续传。
//!
//! 覆盖：
//! - 429 使用第 1 组的退避（注入不等待的时钟）且不计为失败；
//! - 两块文件在第二块确认失败后，本地状态库保留记录；
//! - 新进程用同一应用数据目录登录后，只重新预签名/上传缺失块。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use client_core::{
    Client, ClientBuilder, Clock, ConflictPolicy, MemorySecretStore, SleepFuture, UploadTask,
};
use crypto_core::{KdfParams, MasterKey};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const TOKEN: &str = "fake-session-token-0123456789abcdef";
const CHUNK: i64 = crypto_core::MAX_CHUNK_CIPHERTEXT_SIZE as i64;

struct NoWaitClock {
    sleeps: Mutex<Vec<Duration>>,
}

impl NoWaitClock {
    fn new() -> Self {
        Self {
            sleeps: Mutex::new(Vec::new()),
        }
    }
}

impl Clock for NoWaitClock {
    fn sleep(&self, duration: Duration) -> SleepFuture<'_> {
        self.sleeps.lock().unwrap().push(duration);
        Box::pin(async {})
    }

    fn jitter(&self) -> f64 {
        0.0
    }
}

#[derive(Debug, Clone)]
struct UploadChunk {
    declared_size: i64,
    sha256: String,
}

struct Session {
    id: Uuid,
    content_id: Uuid,
    kind: String,
    node_id: Uuid,
    version_id: Uuid,
    chunk_count: u32,
    declared_size: i64,
    wrapped_key: Option<String>,
    version_meta: String,
    confirmed: HashMap<u32, UploadChunk>,
    status: String,
}

struct FakeState {
    email: String,
    user_id: Uuid,
    root_node_id: Uuid,
    salt: [u8; 16],
    wrapped_mk: Vec<u8>,
    expected_auth_key: String,
    base_url: Mutex<String>,
    session_valid: Mutex<bool>,
    rate_limit_presign_once: Mutex<bool>,
    fail_confirm_idx_one_once: Mutex<bool>,
    expire_get_idx_one_once: Mutex<bool>,
    presign_calls: Mutex<Vec<Vec<u32>>>,
    object_gets: Mutex<HashMap<String, u32>>,
    sessions: Mutex<HashMap<Uuid, Session>>,
    objects: Mutex<HashMap<String, Vec<u8>>>,
}

impl FakeState {
    fn new() -> Arc<Self> {
        let salt = [0x42_u8; 16];
        let params = KdfParams::RECOMMENDED;
        let keys = crypto_core::derive_keys_from_password(PASSWORD, &salt, &params).unwrap();
        let master_key = MasterKey::generate().unwrap();
        let wrapped_mk = crypto_core::wrap_mk_with_password(&keys.pwd_wrap, &master_key).unwrap();
        Arc::new(Self {
            email: format!("transfer-{}@example.com", Uuid::new_v4()),
            user_id: Uuid::new_v4(),
            root_node_id: Uuid::new_v4(),
            salt,
            wrapped_mk,
            expected_auth_key: encode_hex(keys.auth_key.expose_secret()),
            base_url: Mutex::new(String::new()),
            session_valid: Mutex::new(false),
            rate_limit_presign_once: Mutex::new(false),
            fail_confirm_idx_one_once: Mutex::new(false),
            expire_get_idx_one_once: Mutex::new(false),
            presign_calls: Mutex::new(Vec::new()),
            object_gets: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            objects: Mutex::new(HashMap::new()),
        })
    }

    fn profile(&self) -> Value {
        json!({
            "user_id": self.user_id,
            "email": self.email,
            "email_verified": true,
            "kdf_params": KdfParams::RECOMMENDED,
            "kdf_salt": encode_hex(&self.salt),
            "wrapped_mk": encode_hex(&self.wrapped_mk),
            "root_node_id": self.root_node_id,
            "status": "active",
            "status_reason": null,
            "is_admin": false,
        })
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn authorized(state: &FakeState, headers: &HeaderMap) -> bool {
    *state.session_valid.lock().unwrap()
        && headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value == format!("__Host-session={TOKEN}"))
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"code":"unauthenticated","message":"需要登录","request_id":"fake"})),
    )
        .into_response()
}

async fn prelogin(State(state): State<Arc<FakeState>>) -> Response {
    Json(json!({
        "kdf_params": KdfParams::RECOMMENDED,
        "kdf_salt": encode_hex(&state.salt),
    }))
    .into_response()
}

async fn login(State(state): State<Arc<FakeState>>, Json(body): Json<Value>) -> Response {
    if body.get("auth_key").and_then(Value::as_str) != Some(state.expected_auth_key.as_str()) {
        return unauthorized();
    }
    *state.session_valid.lock().unwrap() = true;
    let mut response = Json(state.profile()).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "__Host-session={TOKEN}; Path=/; HttpOnly; Secure; SameSite=Strict"
        ))
        .unwrap(),
    );
    response
}

async fn me(State(state): State<Arc<FakeState>>, headers: HeaderMap) -> Response {
    if authorized(&state, &headers) {
        Json(state.profile()).into_response()
    } else {
        unauthorized()
    }
}

async fn logout(State(state): State<Arc<FakeState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    *state.session_valid.lock().unwrap() = false;
    Json(json!({"ok": true})).into_response()
}

fn expected_size(session: &Session, idx: u32) -> i64 {
    if idx + 1 == session.chunk_count {
        session.declared_size - i64::from(session.chunk_count - 1) * CHUNK
    } else {
        CHUNK
    }
}

async fn create_session(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let content_id = Uuid::parse_str(body["content_id"].as_str().unwrap()).unwrap();
    let node_id = Uuid::parse_str(body["node_id"].as_str().unwrap()).unwrap();
    let version_id = Uuid::parse_str(body["version_id"].as_str().unwrap()).unwrap();
    let session = Session {
        id: Uuid::new_v4(),
        content_id,
        kind: body["kind"].as_str().unwrap().to_owned(),
        node_id,
        version_id,
        chunk_count: body["chunk_count"].as_u64().unwrap() as u32,
        declared_size: body["declared_size"].as_i64().unwrap(),
        wrapped_key: body["wrapped_key"].as_str().map(str::to_owned),
        version_meta: body["version_meta"].as_str().unwrap_or_default().to_owned(),
        confirmed: HashMap::new(),
        status: "open".to_owned(),
    };
    let id = session.id;
    let chunk_count = session.chunk_count;
    let declared_size = session.declared_size;
    let wrapped_key = session.wrapped_key.clone();
    state.sessions.lock().unwrap().insert(id, session);
    Json(json!({
        "id": id,
        "content_id": content_id,
        "kind": body["kind"],
        "status": "open",
        "node_id": node_id,
        "parent_id": body["parent_id"],
        "version_id": version_id,
        "chunk_count": chunk_count,
        "declared_size": declared_size,
        "confirmed_chunks": 0,
        "wrapped_key": wrapped_key,
        "encrypted_meta": body["encrypted_meta"],
        "expires_at": "2099-01-01T00:00:00Z",
        "created_at": "2026-01-01T00:00:00Z",
    }))
    .into_response()
}

async fn get_session(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions.get(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    Json(json!({
        "id": session.id,
        "content_id": session.content_id,
        "kind": session.kind,
        "status": session.status,
        "node_id": session.node_id,
        "parent_id": null,
        "version_id": session.version_id,
        "chunk_count": session.chunk_count,
        "declared_size": session.declared_size,
        "confirmed_chunks": session.confirmed.len(),
        "wrapped_key": session.wrapped_key,
        "encrypted_meta": null,
        "expires_at": "2099-01-01T00:00:00Z",
        "created_at": "2026-01-01T00:00:00Z",
    }))
    .into_response()
}

async fn list_confirmed(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions.get(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut items: Vec<Value> = session
        .confirmed
        .iter()
        .map(|(idx, chunk)| {
            json!({
                "idx": idx,
                "declared_size": chunk.declared_size,
                "sha256": chunk.sha256,
                "confirmed_at": "2026-01-01T00:00:00Z",
            })
        })
        .collect();
    items.sort_by_key(|item| item["idx"].as_u64());
    Json(json!({
        "session_id": id,
        "chunk_count": session.chunk_count,
        "confirmed_count": items.len(),
        "items": items,
    }))
    .into_response()
}

async fn presign(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    if *state.rate_limit_presign_once.lock().unwrap() {
        *state.rate_limit_presign_once.lock().unwrap() = false;
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"code":"rate_limited","message":"稍后重试","request_id":"fake"})),
        )
            .into_response();
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
        return response;
    }
    let indices: Vec<u32> = body["indices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_u64().unwrap() as u32)
        .collect();
    state.presign_calls.lock().unwrap().push(indices.clone());
    let sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions.get(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let base = state.base_url.lock().unwrap().clone();
    let items: Vec<Value> = indices
        .into_iter()
        .map(|idx| {
            json!({
                "idx": idx,
                "ok": true,
                "url": format!("{base}/objects/{id}/{idx}"),
                "method": "PUT",
                "expires_at": "2099-01-01T00:00:00Z",
                "expected_size": expected_size(session, idx),
                "code": null,
                "message": null,
            })
        })
        .collect();
    Json(json!({"session_id": id, "items": items})).into_response()
}

async fn put_object(
    State(state): State<Arc<FakeState>>,
    Path((id, idx)): Path<(Uuid, u32)>,
    body: Bytes,
) -> Response {
    state
        .objects
        .lock()
        .unwrap()
        .insert(format!("{id}/{idx}"), body.to_vec());
    StatusCode::OK.into_response()
}

async fn get_object(
    State(state): State<Arc<FakeState>>,
    Path((id, idx)): Path<(Uuid, u32)>,
) -> Response {
    let key = format!("{id}/{idx}");
    *state
        .object_gets
        .lock()
        .unwrap()
        .entry(key.clone())
        .or_default() += 1;
    if idx == 1 && *state.expire_get_idx_one_once.lock().unwrap() {
        *state.expire_get_idx_one_once.lock().unwrap() = false;
        return StatusCode::FORBIDDEN.into_response();
    }
    match state.objects.lock().unwrap().get(&key).cloned() {
        Some(bytes) => bytes.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn confirm(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let mut sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions.get_mut(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let fail_idx_one = *state.fail_confirm_idx_one_once.lock().unwrap();
    let mut items = Vec::new();
    let mut failed_idx_one = false;
    for chunk in body["chunks"].as_array().unwrap() {
        let idx = chunk["idx"].as_u64().unwrap() as u32;
        let declared_size = chunk["declared_size"].as_i64().unwrap();
        let sha256 = chunk["sha256"].as_str().unwrap().to_owned();
        if fail_idx_one && idx == 1 && !failed_idx_one {
            failed_idx_one = true;
            items.push(json!({
                "idx": idx,
                "ok": false,
                "declared_size": declared_size,
                "sha256": sha256,
                "confirmed_at": null,
                "code": "upload_failed",
                "message": "模拟确认失败",
            }));
            continue;
        }
        session.confirmed.insert(
            idx,
            UploadChunk {
                declared_size,
                sha256: sha256.clone(),
            },
        );
        items.push(json!({
            "idx": idx,
            "ok": true,
            "declared_size": declared_size,
            "sha256": sha256,
            "confirmed_at": "2026-01-01T00:00:00Z",
            "code": null,
            "message": null,
        }));
    }
    if failed_idx_one {
        *state.fail_confirm_idx_one_once.lock().unwrap() = false;
    }
    Json(json!({"session_id": id, "items": items})).into_response()
}

async fn download_manifest(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Path((node_id, version_id)): Path<(Uuid, Uuid)>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions
        .values()
        .find(|session| session.node_id == node_id && session.version_id == version_id)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let base = state.base_url.lock().unwrap().clone();
    let mut chunks = Vec::new();
    let mut ciphertext_size = 0_i64;
    for idx in 0..session.chunk_count {
        let Some(chunk) = session.confirmed.get(&idx) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        ciphertext_size += chunk.declared_size;
        chunks.push(json!({
            "idx": idx,
            "ciphertext_size": chunk.declared_size,
            "sha256": chunk.sha256,
            "method": "GET",
            "url": format!("{base}/objects/{}/{idx}", session.id),
        }));
    }
    Json(json!({
        "node_id": node_id,
        "version_id": version_id,
        "content_id": session.content_id,
        "encrypted_meta": session.version_meta,
        "ciphertext_size": ciphertext_size,
        "chunk_count": session.chunk_count,
        "wrapped_key": session.wrapped_key,
        "expires_at": "2099-01-01T00:00:00Z",
        "chunks": chunks,
    }))
    .into_response()
}

async fn commit(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let mut sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions.get_mut(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if session.confirmed.len() as u32 != session.chunk_count {
        return (
            StatusCode::CONFLICT,
            Json(json!({"code":"upload_incomplete","message":"缺块","request_id":"fake"})),
        )
            .into_response();
    }
    session.status = "committed".to_owned();
    Json(json!({
        "session_id": id,
        "status": "committed",
        "node_id": session.node_id,
        "version_id": session.version_id,
        "content_id": session.content_id,
        "ciphertext_size": session.declared_size,
        "chunk_count": session.chunk_count,
        "committed_at": "2026-01-01T00:00:00Z",
    }))
    .into_response()
}

async fn spawn(state: Arc<FakeState>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    *state.base_url.lock().unwrap() = format!("http://{address}");
    let app = Router::new()
        .route("/api/v1/auth/prelogin", post(prelogin))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/upload-sessions", post(create_session))
        .route("/api/v1/upload-sessions/{id}", get(get_session))
        .route("/api/v1/upload-sessions/{id}/chunks", get(list_confirmed))
        .route("/api/v1/upload-sessions/{id}/chunks/presign", post(presign))
        .route("/api/v1/upload-sessions/{id}/chunks/confirm", post(confirm))
        .route("/api/v1/upload-sessions/{id}/commit", post(commit))
        .route(
            "/api/v1/nodes/{node_id}/versions/{version_id}/download",
            get(download_manifest),
        )
        .route("/objects/{id}/{idx}", put(put_object).get(get_object))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}")
}

fn client(base_url: &str, app_data_dir: &std::path::Path, clock: Arc<NoWaitClock>) -> Client {
    ClientBuilder::new(base_url)
        .secret_store(Arc::new(MemorySecretStore::new()))
        .app_data_dir(app_data_dir)
        .clock(clock)
        .build()
        .unwrap()
}

fn write_file(path: &std::path::Path, size: usize) {
    let data: Vec<u8> = (0..size).map(|index| (index % 251) as u8).collect();
    std::fs::write(path, data).unwrap();
}

#[tokio::test]
async fn rate_limited_presign_is_retried_and_upload_succeeds() {
    let state = FakeState::new();
    *state.rate_limit_presign_once.lock().unwrap() = true;
    let base_url = spawn(state.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(NoWaitClock::new());
    let client = client(&base_url, dir.path(), clock.clone());
    client.login(&state.email, PASSWORD).await.unwrap();

    let path = dir.path().join("rate-limit.bin");
    write_file(&path, 1024);
    let task = client.upload_file(state.root_node_id, &path).unwrap();
    let mut events = task.subscribe();
    task.run().await.unwrap();

    let mut saw_rate_limit = false;
    while let Ok(event) = events.try_recv() {
        if event
            .error
            .as_ref()
            .is_some_and(|error| error.code == client_core::TransferErrorCode::RateLimited)
        {
            saw_rate_limit = true;
        }
    }
    // 429 由第 1 组 API 层静默退避，界面不一定看到错误；关键是上传成功且不计失败。
    let _ = saw_rate_limit;
    assert!(
        clock
            .sleeps
            .lock()
            .unwrap()
            .iter()
            .any(|duration| *duration >= Duration::from_secs(1)),
        "429 必须按 Retry-After 等待"
    );
    assert!(client.list_pending_uploads().await.unwrap().is_empty());
}

#[tokio::test]
async fn restart_resumes_only_missing_chunks_after_failed_confirmation() {
    let state = FakeState::new();
    *state.fail_confirm_idx_one_once.lock().unwrap() = true;
    let base_url = spawn(state.clone()).await;
    let app_data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("resume.bin");
    // 5 MiB = 2 个 4 MiB 分块；用小块模拟大文件的多块续传，不生成 2 GB。
    write_file(&path, 5 * 1024 * 1024);

    let clock = Arc::new(NoWaitClock::new());
    let first = client(&base_url, app_data.path(), clock.clone());
    first.login(&state.email, PASSWORD).await.unwrap();
    let task: UploadTask = first.upload_file(state.root_node_id, &path).unwrap();
    let error = task.run().await.unwrap_err();
    assert!(
        error.to_string().contains("模拟确认失败") || error.is_retryable(),
        "第一次上传应因确认失败而中止：{error:?}"
    );

    let pending = first.list_pending_uploads().await.unwrap();
    assert_eq!(pending.len(), 1, "失败后必须保留续传记录");
    let session_id = pending[0].session_id;
    drop(first);

    // 模拟应用重启：新 Client 使用同一应用数据目录。
    let second = client(&base_url, app_data.path(), clock);
    second.login(&state.email, PASSWORD).await.unwrap();
    assert_eq!(second.list_pending_uploads().await.unwrap().len(), 1);
    let resume = second.resume_pending_upload(session_id).await.unwrap();
    resume.run().await.unwrap();
    assert!(
        second.list_pending_uploads().await.unwrap().is_empty(),
        "完成后必须清理本地记录"
    );

    let calls = state.presign_calls.lock().unwrap().clone();
    assert_eq!(
        calls.last(),
        Some(&vec![1]),
        "重启后只应重新预签名缺失的第 1 块，实际调用：{calls:?}"
    );
    let confirmed = state
        .sessions
        .lock()
        .unwrap()
        .get(&session_id)
        .map(|session| session.confirmed.len())
        .unwrap_or_default();
    assert_eq!(confirmed, 2);
}

#[tokio::test]
async fn download_verifies_hashes_and_cleans_up_after_tampering() {
    let state = FakeState::new();
    let base_url = spawn(state.clone()).await;
    let app_data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("download.bin");
    write_file(&path, 1024);
    let client = client(&base_url, app_data.path(), Arc::new(NoWaitClock::new()));
    client.login(&state.email, PASSWORD).await.unwrap();
    let uploaded = client
        .upload_file(state.root_node_id, &path)
        .unwrap()
        .run()
        .await
        .unwrap();

    let output = tempfile::tempdir().unwrap();
    let outcome = client
        .download_file(
            uploaded.node_id,
            uploaded.version_id,
            "download.bin",
            output.path(),
            ConflictPolicy::KeepBoth,
        )
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(&outcome.path).unwrap(),
        std::fs::read(&path).unwrap()
    );

    // 篡改对象存储中的密文：下载必须失败，且目标目录不出现半成品。
    let session_id = state
        .sessions
        .lock()
        .unwrap()
        .values()
        .find(|session| session.node_id == uploaded.node_id)
        .map(|session| session.id)
        .unwrap();
    {
        let mut objects = state.objects.lock().unwrap();
        let object = objects
            .get_mut(&format!("{session_id}/0"))
            .expect("对象必须存在");
        let last = object.len() - 1;
        object[last] ^= 0x01;
    }
    let clean = tempfile::tempdir().unwrap();
    let error = client
        .download_file(
            uploaded.node_id,
            uploaded.version_id,
            "download.bin",
            clean.path(),
            ConflictPolicy::KeepBoth,
        )
        .unwrap()
        .run()
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("完整性") || error.to_string().contains("SHA"),
        "篡改必须被识别：{error:?}"
    );
    assert!(
        std::fs::read_dir(clean.path()).unwrap().next().is_none(),
        "失败后不得留下目标或临时文件"
    );
}

#[tokio::test]
async fn expired_download_url_refreshes_manifest_without_redownloading_written_chunks() {
    let state = FakeState::new();
    let base_url = spawn(state.clone()).await;
    let app_data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("expiry.bin");
    write_file(&path, 5 * 1024 * 1024);
    let client = client(&base_url, app_data.path(), Arc::new(NoWaitClock::new()));
    client.login(&state.email, PASSWORD).await.unwrap();
    let uploaded = client
        .upload_file(state.root_node_id, &path)
        .unwrap()
        .run()
        .await
        .unwrap();
    *state.expire_get_idx_one_once.lock().unwrap() = true;

    let output = tempfile::tempdir().unwrap();
    let outcome = client
        .download_file(
            uploaded.node_id,
            uploaded.version_id,
            "expiry.bin",
            output.path(),
            ConflictPolicy::KeepBoth,
        )
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(&outcome.path).unwrap(),
        std::fs::read(&path).unwrap(),
        "刷新清单后必须得到与源文件一致的内容"
    );

    let session_id = state
        .sessions
        .lock()
        .unwrap()
        .values()
        .find(|session| session.node_id == uploaded.node_id)
        .map(|session| session.id)
        .unwrap();
    let gets = state.object_gets.lock().unwrap().clone();
    assert_eq!(
        gets.get(&format!("{session_id}/0")).copied(),
        Some(1),
        "已写入的第 0 块不得重新下载：{gets:?}"
    );
    assert_eq!(
        gets.get(&format!("{session_id}/1")).copied(),
        Some(2),
        "第 1 块应首次 403、刷新清单后重试成功：{gets:?}"
    );
}

#[tokio::test]
async fn download_conflict_policies_keep_both_replace_and_skip() {
    let state = FakeState::new();
    let base_url = spawn(state.clone()).await;
    let app_data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let path = files.path().join("conflict.txt");
    std::fs::write(&path, b"cloud").unwrap();
    let client = client(&base_url, app_data.path(), Arc::new(NoWaitClock::new()));
    client.login(&state.email, PASSWORD).await.unwrap();
    let uploaded = client
        .upload_file(state.root_node_id, &path)
        .unwrap()
        .run()
        .await
        .unwrap();

    let target = tempfile::tempdir().unwrap();
    std::fs::write(target.path().join("conflict.txt"), b"local").unwrap();
    let keep = client
        .download_file(
            uploaded.node_id,
            uploaded.version_id,
            "conflict.txt",
            target.path(),
            ConflictPolicy::KeepBoth,
        )
        .unwrap()
        .run()
        .await
        .unwrap();
    assert!(keep.path.ends_with("conflict (1).txt"));
    assert_eq!(
        std::fs::read(target.path().join("conflict.txt")).unwrap(),
        b"local"
    );
    assert_eq!(std::fs::read(keep.path).unwrap(), b"cloud");

    let skip = client
        .download_file(
            uploaded.node_id,
            uploaded.version_id,
            "conflict.txt",
            target.path(),
            ConflictPolicy::Skip,
        )
        .unwrap()
        .run()
        .await
        .unwrap();
    assert!(skip.skipped);
    assert_eq!(
        std::fs::read(target.path().join("conflict.txt")).unwrap(),
        b"local"
    );

    let replace = client
        .download_file(
            uploaded.node_id,
            uploaded.version_id,
            "conflict.txt",
            target.path(),
            ConflictPolicy::Replace,
        )
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_eq!(std::fs::read(replace.path).unwrap(), b"cloud");
    assert_eq!(
        std::fs::read(target.path().join("conflict.txt")).unwrap(),
        b"cloud"
    );
}
