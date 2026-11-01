//! 无需 Docker 的备份引擎集成测试：本地假服务端实现认证、节点树、上传与版本。
//!
//! 覆盖 spec“桌面文件夹备份”“备份排除与控制”的全部 Scenario（macOS NFD 场景
//! 在任意平台用 NFD 字符串构造），以及暂停、未解锁、上传中被修改、大目录索引
//! 快速跳过等控制行为。真实 PostgreSQL/MinIO 的测试见 `tests/backup.rs`。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use client_core::desktop::wechat::{
    WechatAccount, WechatCategory, WechatLocator, WechatProcessProbe, WechatSystemLocations,
};
use client_core::desktop::{
    BackupActivityKind, BackupEngine, BackupEngineConfig, BackupFolderDraft, BackupFolderStatus,
    BackupFolderUpdate, BackupPauseReason, BackupSummaryStatus, WechatBackupSource,
    WechatSnapshotOutcome,
};
use client_core::{
    Client, ClientBuilder, Clock, ConflictPolicy, MemorySecretStore, NodeKind, SleepFuture,
};
use crypto_core::{
    ContentHasher, ContentId, KdfParams, MasterKey, NodeId, NodeKey, NodeMetadata, VersionId,
    VersionMetadata, encrypt_node_metadata, encrypt_version_metadata, wrap_node_key,
};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const TOKEN: &str = "fake-session-token-0123456789abcdef";
const CHUNK: i64 = crypto_core::MAX_CHUNK_CIPHERTEXT_SIZE as i64;

struct NoWaitClock {
    sleeps: Mutex<Vec<Duration>>,
    /// 相对真实时间的偏移；`advance` 只影响 `now()`，不影响 `sleep` 立即返回。
    offset: Mutex<Duration>,
}

impl NoWaitClock {
    fn new() -> Self {
        Self {
            sleeps: Mutex::new(Vec::new()),
            offset: Mutex::new(Duration::ZERO),
        }
    }

    fn advance(&self, duration: Duration) {
        if let Ok(mut offset) = self.offset.lock() {
            *offset = offset.saturating_add(duration);
        }
    }
}

impl Clock for NoWaitClock {
    fn sleep(&self, duration: Duration) -> SleepFuture<'_> {
        self.sleeps.lock().unwrap().push(duration);
        Box::pin(async {})
    }

    fn now(&self) -> SystemTime {
        self.offset
            .lock()
            .map(|offset| SystemTime::now() + *offset)
            .unwrap_or_else(|_| SystemTime::now())
    }

    fn jitter(&self) -> f64 {
        0.0
    }
}

/// 可切换的假微信进程探测（测试绝不枚举真实进程）。
#[derive(Debug)]
struct FakeProbe {
    running: AtomicBool,
}

impl FakeProbe {
    fn new(running: bool) -> Self {
        Self {
            running: AtomicBool::new(running),
        }
    }

    fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::SeqCst);
    }
}

impl WechatProcessProbe for FakeProbe {
    fn is_running(&self, _version: client_core::desktop::wechat::WechatVersion) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// 假微信系统位置：只提供测试指定的“文档”目录，不读取注册表或真实微信目录。
struct FakeWechatLocations {
    documents: Option<PathBuf>,
}

impl WechatSystemLocations for FakeWechatLocations {
    fn documents_dir(&self) -> Result<Option<PathBuf>, client_core::desktop::wechat::WechatError> {
        Ok(self.documents.clone())
    }

    fn appdata_dir(&self) -> Result<Option<PathBuf>, client_core::desktop::wechat::WechatError> {
        Ok(None)
    }

    fn wechat_3_file_save_path(
        &self,
    ) -> Result<Option<String>, client_core::desktop::wechat::WechatError> {
        Ok(None)
    }
}

#[derive(Debug, Clone)]
struct Chunk {
    declared_size: i64,
    sha256: String,
}

#[derive(Debug, Clone)]
struct Version {
    id: Uuid,
    content_id: Uuid,
    encrypted_meta: String,
    is_current: bool,
    chunks: Vec<Chunk>,
}

#[derive(Debug, Clone)]
struct Node {
    id: Uuid,
    parent_id: Option<Uuid>,
    kind: String,
    name_token: Option<String>,
    wrapped_key: Option<String>,
    encrypted_meta: Option<String>,
    row_version: i32,
    current_version: Option<Version>,
    versions: Vec<Version>,
}

struct Session {
    id: Uuid,
    content_id: Uuid,
    kind: String,
    parent_id: Option<Uuid>,
    node_id: Uuid,
    version_id: Uuid,
    chunk_count: u32,
    declared_size: i64,
    wrapped_key: Option<String>,
    node_meta: Option<String>,
    name_token: Option<String>,
    version_meta: String,
    confirmed: HashMap<u32, Chunk>,
    status: String,
}

struct FakeState {
    email: String,
    user_id: Uuid,
    root_node_id: Uuid,
    salt: [u8; 16],
    wrapped_mk: Vec<u8>,
    master_key: MasterKey,
    expected_auth_key: String,
    base_url: Mutex<String>,
    session_valid: Mutex<bool>,
    nodes: Mutex<HashMap<Uuid, Node>>,
    sessions: Mutex<HashMap<Uuid, Session>>,
    objects: Mutex<HashMap<String, Vec<u8>>>,
    upload_sessions_created: Mutex<u64>,
    fail_upload_session_remaining: Mutex<u32>,
    fail_create_folder_once: Mutex<bool>,
    mutate_path_on_first_put: Mutex<Option<PathBuf>>,
    lock_client_on_first_put: Mutex<Option<Client>>,
    flip_probe_on_first_put: Mutex<Option<Arc<FakeProbe>>>,
    trashed: Mutex<HashSet<Uuid>>,
    requests: Mutex<u64>,
    quota_bytes: Mutex<i64>,
}

impl FakeState {
    fn new() -> Arc<Self> {
        let salt = [0x42_u8; 16];
        let params = KdfParams::RECOMMENDED;
        let keys = crypto_core::derive_keys_from_password(PASSWORD, &salt, &params).unwrap();
        let master_key = MasterKey::generate().unwrap();
        let wrapped_mk = crypto_core::wrap_mk_with_password(&keys.pwd_wrap, &master_key).unwrap();
        let root_node_id = Uuid::new_v4();
        let root = Node {
            id: root_node_id,
            parent_id: None,
            kind: "folder".to_owned(),
            name_token: None,
            wrapped_key: None,
            encrypted_meta: None,
            row_version: 1,
            current_version: None,
            versions: Vec::new(),
        };
        let mut nodes = HashMap::new();
        nodes.insert(root_node_id, root);
        Arc::new(Self {
            email: format!("backup-{}@example.com", Uuid::new_v4()),
            user_id: Uuid::new_v4(),
            root_node_id,
            salt,
            wrapped_mk,
            master_key,
            expected_auth_key: encode_hex(keys.auth_key.expose_secret()),
            base_url: Mutex::new(String::new()),
            session_valid: Mutex::new(false),
            nodes: Mutex::new(nodes),
            sessions: Mutex::new(HashMap::new()),
            objects: Mutex::new(HashMap::new()),
            upload_sessions_created: Mutex::new(0),
            fail_upload_session_remaining: Mutex::new(0),
            fail_create_folder_once: Mutex::new(false),
            mutate_path_on_first_put: Mutex::new(None),
            lock_client_on_first_put: Mutex::new(None),
            flip_probe_on_first_put: Mutex::new(None),
            trashed: Mutex::new(HashSet::new()),
            requests: Mutex::new(0),
            quota_bytes: Mutex::new(10 * 1024 * 1024 * 1024),
        })
    }

    fn requests(&self) -> u64 {
        *self.requests.lock().unwrap()
    }

    fn set_quota(&self, quota_bytes: i64) {
        *self.quota_bytes.lock().unwrap() = quota_bytes;
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

    fn upload_sessions(&self) -> u64 {
        *self.upload_sessions_created.lock().unwrap()
    }

    fn node(&self, id: Uuid) -> Option<Node> {
        self.nodes.lock().unwrap().get(&id).cloned()
    }

    fn children(&self, parent: Uuid) -> Vec<Node> {
        let nodes = self.nodes.lock().unwrap();
        let trashed = self.trashed.lock().unwrap();
        let mut items: Vec<Node> = nodes
            .values()
            .filter(|node| node.parent_id == Some(parent) && !trashed.contains(&node.id))
            .cloned()
            .collect();
        items.sort_by_key(|node| node.id);
        items
    }

    /// 已移入回收站（仍保留在状态中）的节点，供测试断言目录被回收。
    fn trashed_ids(&self) -> Vec<Uuid> {
        let mut ids: Vec<Uuid> = self.trashed.lock().unwrap().iter().copied().collect();
        ids.sort();
        ids
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

fn api_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({"code":code,"message":message,"request_id":"fake"})),
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

fn version_json(version: &Version) -> Value {
    json!({
        "id": version.id,
        "content_id": version.content_id,
        "encrypted_meta": version.encrypted_meta,
        "is_current": version.is_current,
        "created_at": "2026-01-01T00:00:00Z",
    })
}

fn node_json(node: &Node) -> Value {
    json!({
        "id": node.id,
        "kind": node.kind,
        "parent_id": node.parent_id,
        "wrapped_key": node.wrapped_key,
        "encrypted_meta": node.encrypted_meta,
        "row_version": node.row_version,
        "current_version": node.current_version.as_ref().map(version_json),
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "deleted_at": null,
        "trash_batch_id": null,
    })
}

async fn create_folder(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let id = Uuid::parse_str(body["id"].as_str().unwrap()).unwrap();
    let parent_id = Uuid::parse_str(body["parent_id"].as_str().unwrap()).unwrap();
    if *state.fail_create_folder_once.lock().unwrap() {
        // 模拟并发创建：节点实际上已存在，但本次请求返回冲突，客户端必须重新列出复用。
        state.nodes.lock().unwrap().insert(
            id,
            Node {
                id,
                parent_id: Some(parent_id),
                kind: "folder".to_owned(),
                name_token: body["name_token"].as_str().map(str::to_owned),
                wrapped_key: body["wrapped_key"].as_str().map(str::to_owned),
                encrypted_meta: body["encrypted_meta"].as_str().map(str::to_owned),
                row_version: 1,
                current_version: None,
                versions: Vec::new(),
            },
        );
        *state.fail_create_folder_once.lock().unwrap() = false;
        return api_error(StatusCode::CONFLICT, "name_conflict", "同名节点已存在");
    }
    let node = Node {
        id,
        parent_id: Some(parent_id),
        kind: "folder".to_owned(),
        name_token: body["name_token"].as_str().map(str::to_owned),
        wrapped_key: body["wrapped_key"].as_str().map(str::to_owned),
        encrypted_meta: body["encrypted_meta"].as_str().map(str::to_owned),
        row_version: 1,
        current_version: None,
        versions: Vec::new(),
    };
    state.nodes.lock().unwrap().insert(id, node.clone());
    Json(node_json(&node)).into_response()
}

async fn list_children(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<Uuid>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    if state.node(id).is_none() {
        return api_error(StatusCode::NOT_FOUND, "not_found", "节点不存在");
    }
    let items: Vec<Value> = state.children(id).iter().map(node_json).collect();
    Json(json!({"items": items, "next_cursor": null})).into_response()
}

async fn delete_node(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<Uuid>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let nodes = state.nodes.lock().unwrap();
    if !nodes.contains_key(&id) {
        return api_error(StatusCode::NOT_FOUND, "not_found", "节点不存在");
    }
    let mut trashed = state.trashed.lock().unwrap();
    let mut stack = vec![id];
    while let Some(current) = stack.pop() {
        if trashed.insert(current) {
            for child in nodes
                .values()
                .filter(|node| node.parent_id == Some(current))
            {
                stack.push(child.id);
            }
        }
    }
    Json(json!({"trash_batch_id": Uuid::new_v4()})).into_response()
}

async fn list_versions(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<Uuid>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let Some(node) = state.node(id) else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "节点不存在");
    };
    let mut versions = node.versions;
    if let Some(current) = node.current_version {
        versions.push(current);
    }
    versions.sort_by_key(|version| version.id);
    let items: Vec<Value> = versions.iter().map(version_json).collect();
    Json(json!({"items": items})).into_response()
}

async fn create_session(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    {
        let mut remaining = state.fail_upload_session_remaining.lock().unwrap();
        if *remaining > 0 {
            *remaining -= 1;
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "internal_error",
                "暂时不可用",
            );
        }
    }
    *state.upload_sessions_created.lock().unwrap() += 1;
    let content_id = Uuid::parse_str(body["content_id"].as_str().unwrap()).unwrap();
    let node_id = Uuid::parse_str(body["node_id"].as_str().unwrap()).unwrap();
    let version_id = Uuid::parse_str(body["version_id"].as_str().unwrap()).unwrap();
    let kind = body["kind"].as_str().unwrap().to_owned();
    let parent_id = body["parent_id"]
        .as_str()
        .and_then(|value| Uuid::parse_str(value).ok());
    let wrapped_key = body["wrapped_key"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| state.node(node_id).and_then(|node| node.wrapped_key));
    let session = Session {
        id: Uuid::new_v4(),
        content_id,
        kind: kind.clone(),
        parent_id,
        node_id,
        version_id,
        chunk_count: body["chunk_count"].as_u64().unwrap() as u32,
        declared_size: body["declared_size"].as_i64().unwrap(),
        wrapped_key: wrapped_key.clone(),
        node_meta: body["encrypted_meta"].as_str().map(str::to_owned),
        name_token: body["name_token"].as_str().map(str::to_owned),
        version_meta: body["version_meta"].as_str().unwrap().to_owned(),
        confirmed: HashMap::new(),
        status: "open".to_owned(),
    };
    let id = session.id;
    let chunk_count = session.chunk_count;
    let declared_size = session.declared_size;
    state.sessions.lock().unwrap().insert(id, session);
    Json(json!({
        "id": id,
        "content_id": content_id,
        "kind": kind,
        "status": "open",
        "node_id": node_id,
        "parent_id": parent_id,
        "version_id": version_id,
        "chunk_count": chunk_count,
        "declared_size": declared_size,
        "confirmed_chunks": 0,
        "wrapped_key": wrapped_key,
        "encrypted_meta": null,
        "expires_at": "2099-01-01T00:00:00Z",
        "created_at": "2026-01-01T00:00:00Z",
    }))
    .into_response()
}

fn expected_size(session: &Session, idx: u32) -> i64 {
    if idx + 1 == session.chunk_count {
        session.declared_size - i64::from(session.chunk_count - 1) * CHUNK
    } else {
        CHUNK
    }
}

async fn list_confirmed(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<Uuid>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions.get(&id) else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "会话不存在");
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
    AxumPath(id): AxumPath<Uuid>,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let indices: Vec<u32> = body["indices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_u64().unwrap() as u32)
        .collect();
    let sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions.get(&id) else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "会话不存在");
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
    AxumPath((id, idx)): AxumPath<(Uuid, u32)>,
    body: Bytes,
) -> Response {
    state
        .objects
        .lock()
        .unwrap()
        .insert(format!("{id}/{idx}"), body.to_vec());
    if let Some(path) = state.mutate_path_on_first_put.lock().unwrap().take() {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&path) {
            let _ = file.write_all(b"changed-during-upload");
        }
    }
    if let Some(client) = state.lock_client_on_first_put.lock().unwrap().take() {
        let _ = client.lock();
    }
    if let Some(probe) = state.flip_probe_on_first_put.lock().unwrap().take() {
        probe.set_running(true);
    }
    StatusCode::OK.into_response()
}

async fn get_object(
    State(state): State<Arc<FakeState>>,
    AxumPath((version_id, idx)): AxumPath<(Uuid, u32)>,
) -> Response {
    match state
        .objects
        .lock()
        .unwrap()
        .get(&format!("{version_id}/{idx}"))
        .cloned()
    {
        Some(bytes) => bytes.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn confirm(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<Uuid>,
    Json(body): Json<Value>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let mut sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions.get_mut(&id) else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "会话不存在");
    };
    let mut items = Vec::new();
    for chunk in body["chunks"].as_array().unwrap() {
        let idx = chunk["idx"].as_u64().unwrap() as u32;
        let declared_size = chunk["declared_size"].as_i64().unwrap();
        let sha256 = chunk["sha256"].as_str().unwrap().to_owned();
        session.confirmed.insert(
            idx,
            Chunk {
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
    Json(json!({"session_id": id, "items": items})).into_response()
}

async fn commit(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<Uuid>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let mut sessions = state.sessions.lock().unwrap();
    let Some(session) = sessions.get_mut(&id) else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "会话不存在");
    };
    if session.confirmed.len() as u32 != session.chunk_count {
        return api_error(StatusCode::CONFLICT, "upload_incomplete", "缺块");
    }
    let mut chunks: Vec<Chunk> = Vec::with_capacity(session.chunk_count as usize);
    for idx in 0..session.chunk_count {
        let Some(chunk) = session.confirmed.get(&idx) else {
            return api_error(StatusCode::CONFLICT, "upload_incomplete", "缺块");
        };
        chunks.push(chunk.clone());
    }
    // 会话对象移动到版本对象，旧版本仍可下载。
    {
        let mut objects = state.objects.lock().unwrap();
        for idx in 0..session.chunk_count {
            if let Some(bytes) = objects.remove(&format!("{id}/{idx}")) {
                objects.insert(format!("{}/{idx}", session.version_id), bytes);
            }
        }
    }

    let mut nodes = state.nodes.lock().unwrap();
    if session.kind == "new_file" {
        let parent_id = session.parent_id.unwrap();
        let conflict = nodes
            .values()
            .any(|node| node.parent_id == Some(parent_id) && node.name_token == session.name_token);
        if conflict {
            return api_error(StatusCode::CONFLICT, "name_conflict", "同名文件已存在");
        }
        let version = Version {
            id: session.version_id,
            content_id: session.content_id,
            encrypted_meta: session.version_meta.clone(),
            is_current: true,
            chunks,
        };
        let node = Node {
            id: session.node_id,
            parent_id: Some(parent_id),
            kind: "file".to_owned(),
            name_token: session.name_token.clone(),
            wrapped_key: session.wrapped_key.clone(),
            encrypted_meta: session.node_meta.clone(),
            row_version: 1,
            current_version: Some(version),
            versions: Vec::new(),
        };
        nodes.insert(node.id, node);
    } else {
        let Some(node) = nodes.get_mut(&session.node_id) else {
            return api_error(StatusCode::NOT_FOUND, "not_found", "目标文件不存在");
        };
        if let Some(current) = node.current_version.take() {
            node.versions.push(Version {
                is_current: false,
                ..current
            });
        }
        node.current_version = Some(Version {
            id: session.version_id,
            content_id: session.content_id,
            encrypted_meta: session.version_meta.clone(),
            is_current: true,
            chunks,
        });
        node.row_version += 1;
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

async fn download_manifest(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    AxumPath((node_id, version_id)): AxumPath<(Uuid, Uuid)>,
) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let Some(node) = state.node(node_id) else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "节点不存在");
    };
    let mut versions = node.versions;
    if let Some(current) = node.current_version {
        versions.push(current);
    }
    let Some(version) = versions.iter().find(|version| version.id == version_id) else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "版本不存在");
    };
    let base = state.base_url.lock().unwrap().clone();
    let mut chunks = Vec::new();
    let mut ciphertext_size = 0_i64;
    for (idx, chunk) in version.chunks.iter().enumerate() {
        ciphertext_size += chunk.declared_size;
        chunks.push(json!({
            "idx": idx,
            "ciphertext_size": chunk.declared_size,
            "sha256": chunk.sha256,
            "method": "GET",
            "url": format!("{base}/objects/{version_id}/{idx}"),
        }));
    }
    Json(json!({
        "node_id": node_id,
        "version_id": version_id,
        "content_id": version.content_id,
        "encrypted_meta": version.encrypted_meta,
        "ciphertext_size": ciphertext_size,
        "chunk_count": version.chunks.len(),
        "wrapped_key": node.wrapped_key,
        "expires_at": "2099-01-01T00:00:00Z",
        "chunks": chunks,
    }))
    .into_response()
}

async fn usage(State(state): State<Arc<FakeState>>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let quota_bytes = *state.quota_bytes.lock().unwrap();
    Json(json!({
        "used_bytes": 0,
        "reserved_bytes": 0,
        "quota_bytes": quota_bytes,
    }))
    .into_response()
}

/// 统计所有经过假服务端的请求，供“未变时零请求”的断言使用。
async fn count_requests(
    State(state): State<Arc<FakeState>>,
    request: Request,
    next: Next,
) -> Response {
    *state.requests.lock().unwrap() += 1;
    next.run(request).await
}

async fn spawn(state: Arc<FakeState>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    *state.base_url.lock().unwrap() = format!("http://{address}");
    let app = Router::new()
        .route("/api/v1/auth/prelogin", post(prelogin))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/folders", post(create_folder))
        .route("/api/v1/nodes/{id}/children", get(list_children))
        .route("/api/v1/nodes/{id}", delete(delete_node))
        .route("/api/v1/nodes/{id}/versions", get(list_versions))
        .route(
            "/api/v1/nodes/{node_id}/versions/{version_id}/download",
            get(download_manifest),
        )
        .route("/api/v1/upload-sessions", post(create_session))
        .route("/api/v1/upload-sessions/{id}/chunks", get(list_confirmed))
        .route("/api/v1/upload-sessions/{id}/chunks/presign", post(presign))
        .route("/api/v1/upload-sessions/{id}/chunks/confirm", post(confirm))
        .route("/api/v1/upload-sessions/{id}/commit", post(commit))
        .route("/api/v1/me/usage", get(usage))
        .route("/objects/{id}/{idx}", put(put_object).get(get_object))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            count_requests,
        ))
        .with_state(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}")
}

fn build_client_with_clock(base_url: &str, app_data: &Path, clock: Arc<NoWaitClock>) -> Client {
    ClientBuilder::new(base_url)
        .secret_store(Arc::new(MemorySecretStore::new()))
        .app_data_dir(app_data)
        .clock(clock)
        .build()
        .unwrap()
}

fn build_engine_with_retry(
    client: &Client,
    debounce: Duration,
    watch_files: bool,
    retry_interval: Duration,
    wechat_locator: Option<WechatLocator>,
    wechat_process_probe: Arc<dyn WechatProcessProbe>,
    wechat_snapshot_interval: Duration,
) -> BackupEngine {
    BackupEngine::new(
        client.clone(),
        BackupEngineConfig {
            device_name: "TEST-PC".to_owned(),
            debounce,
            full_scan_interval: Duration::from_secs(3600),
            retry_interval,
            watch_files,
            wechat_locator,
            wechat_manual_directories: Vec::new(),
            wechat_process_probe: Some(wechat_process_probe),
            wechat_snapshot_interval,
        },
    )
    .unwrap()
}

async fn cloud_children(client: &Client, folder_id: Uuid) -> Vec<client_core::NodeView> {
    client
        .list_folder(folder_id, Default::default())
        .await
        .unwrap()
}

async fn default_target(client: &Client, local_name: &str) -> Uuid {
    let root = client.account().unwrap().root_node_id;
    let mut parent = root;
    for name in ["电脑备份", "TEST-PC", local_name] {
        let children = cloud_children(client, parent).await;
        parent = children
            .into_iter()
            .find(|node| node.name == name && node.kind == NodeKind::Folder)
            .unwrap_or_else(|| panic!("缺少云端文件夹 {name}"))
            .id;
    }
    parent
}

fn write(path: &Path, content: &[u8]) {
    std::fs::write(path, content).unwrap();
}

fn set_mtime_forward(path: &Path) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    let future = SystemTime::now() + Duration::from_secs(120);
    file.set_modified(future).unwrap();
}

async fn add_and_scan(
    engine: &BackupEngine,
    draft: BackupFolderDraft,
) -> (Uuid, client_core::desktop::BackupRunReport) {
    let folder = engine.add_folder(draft).await.unwrap();
    let report = engine.scan_folder_now(folder.id).await.unwrap();
    (folder.id, report)
}

struct Harness {
    state: Arc<FakeState>,
    client: Client,
    engine: BackupEngine,
    clock: Arc<NoWaitClock>,
    probe: Arc<FakeProbe>,
    _app_data: tempfile::TempDir,
    files: tempfile::TempDir,
}

async fn harness() -> Harness {
    harness_with_options(false, Duration::from_millis(50), Duration::from_secs(60)).await
}

async fn harness_with_watch(watch_files: bool, debounce: Duration) -> Harness {
    harness_with_options(watch_files, debounce, Duration::from_secs(60)).await
}

async fn harness_with_retry(retry_interval: Duration) -> Harness {
    harness_with_options(false, Duration::from_millis(50), retry_interval).await
}

async fn harness_with_options(
    watch_files: bool,
    debounce: Duration,
    retry_interval: Duration,
) -> Harness {
    harness_full(watch_files, debounce, retry_interval, None).await
}

/// 带微信系统位置注入的假服务端夹具（“文档”目录由测试指定）。
async fn harness_with_wechat(documents: PathBuf) -> Harness {
    harness_full(
        false,
        Duration::from_millis(50),
        Duration::from_secs(60),
        Some(documents),
    )
    .await
}

async fn harness_full(
    watch_files: bool,
    debounce: Duration,
    retry_interval: Duration,
    wechat_documents: Option<PathBuf>,
) -> Harness {
    harness_full_with(
        watch_files,
        debounce,
        retry_interval,
        wechat_documents,
        Arc::new(FakeProbe::new(false)),
        Duration::from_secs(10),
    )
    .await
}

async fn harness_full_with(
    watch_files: bool,
    debounce: Duration,
    retry_interval: Duration,
    wechat_documents: Option<PathBuf>,
    probe: Arc<FakeProbe>,
    wechat_snapshot_interval: Duration,
) -> Harness {
    let state = FakeState::new();
    let base_url = spawn(state.clone()).await;
    let app_data = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let clock = Arc::new(NoWaitClock::new());
    let client = build_client_with_clock(&base_url, app_data.path(), clock.clone());
    client.login(&state.email, PASSWORD).await.unwrap();
    // 始终注入假系统位置与假进程探测：测试绝不读取注册表、真实微信目录或进程。
    let wechat_locator = Some(WechatLocator::with_system_locations(Arc::new(
        FakeWechatLocations {
            documents: wechat_documents,
        },
    )));
    let engine = build_engine_with_retry(
        &client,
        debounce,
        watch_files,
        retry_interval,
        wechat_locator,
        probe.clone(),
        wechat_snapshot_interval,
    );
    Harness {
        state,
        client,
        engine,
        clock,
        probe,
        _app_data: app_data,
        files,
    }
}

async fn wait_for_added(engine: &BackupEngine, relative_path: &str) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let activity = engine.recent_activity(50).await.unwrap();
            if activity.iter().any(|item| {
                item.relative_path == relative_path && item.kind == BackupActivityKind::Added
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("等待 {relative_path} 自动备份超时"));
}

#[tokio::test]
async fn new_file_is_backed_up_and_reports_status() {
    let h = harness().await;
    let work = h.files.path().join("工作资料");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("a.txt"), b"hello");

    let preflight = h
        .engine
        .preflight(&BackupFolderDraft::new(&work))
        .await
        .unwrap();
    assert_eq!(preflight.file_count, 1);
    assert_eq!(preflight.total_bytes, 5);
    assert!(preflight.upload_bytes > preflight.total_bytes);
    assert!(preflight.remaining_quota_bytes.is_some());

    let (folder_id, report) = add_and_scan(&h.engine, BackupFolderDraft::new(&work)).await;
    assert_eq!(report.status, BackupFolderStatus::IdleOk);
    assert_eq!(report.uploaded_files, 1);
    assert_eq!(report.hashed_files, 1);
    assert_eq!(report.unchanged_files, 0);

    let target = default_target(&h.client, "工作资料").await;
    let children = cloud_children(&h.client, target).await;
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "a.txt");
    assert_eq!(children[0].size, Some(5));

    assert_eq!(h.engine.summary().status, BackupSummaryStatus::AllBackedUp);
    let states = h.engine.folder_states();
    assert_eq!(states[0].folder_id, folder_id);
    assert_eq!(states[0].status, BackupFolderStatus::IdleOk);
    assert!(states[0].last_backup_at.is_some());
    let activity = h.engine.recent_activity(10).await.unwrap();
    assert!(
        activity
            .iter()
            .any(|item| item.kind == BackupActivityKind::Added && item.relative_path == "a.txt")
    );
    let stored = h.engine.folder(folder_id).await.unwrap();
    assert_eq!(stored.cloud_folder_id, Some(target));
    assert!(stored.cloud_path.is_some());
}

#[tokio::test]
async fn modified_file_creates_new_version_and_old_version_is_restorable() {
    let h = harness().await;
    let work = h.files.path().join("docs");
    std::fs::create_dir_all(&work).unwrap();
    let file = work.join("report.txt");
    write(&file, b"version-one");

    let (_, first) = add_and_scan(&h.engine, BackupFolderDraft::new(&work)).await;
    assert_eq!(first.uploaded_files, 1);
    let target = default_target(&h.client, "docs").await;
    let node = cloud_children(&h.client, target).await.remove(0);

    write(&file, b"version-two-longer");
    set_mtime_forward(&file);
    let second = h
        .engine
        .scan_folder_now(h.engine.list_folders().await.unwrap()[0].id)
        .await
        .unwrap();
    assert_eq!(second.updated_files, 1);
    assert_eq!(second.uploaded_files, 0);

    let children = cloud_children(&h.client, target).await;
    assert_eq!(children.len(), 1, "修改必须生成新版本而不是新文件");
    let versions = h.client.list_versions(node.id).await.unwrap();
    assert_eq!(versions.len(), 2);
    let old = versions.iter().find(|version| !version.is_current).unwrap();
    let output = tempfile::tempdir().unwrap();
    let downloaded = h
        .client
        .download_file(
            node.id,
            old.id,
            "report.txt",
            output.path(),
            ConflictPolicy::Replace,
        )
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_eq!(std::fs::read(downloaded.path).unwrap(), b"version-one");
}

#[tokio::test]
async fn local_delete_does_not_delete_cloud_content() {
    let h = harness().await;
    let work = h.files.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("keep.txt"), b"keep");
    write(&work.join("deleted.txt"), b"delete me");
    let (folder_id, first) = add_and_scan(&h.engine, BackupFolderDraft::new(&work)).await;
    assert_eq!(first.uploaded_files, 2);
    let target = default_target(&h.client, "work").await;

    std::fs::remove_file(work.join("deleted.txt")).unwrap();
    let sessions_before = h.state.upload_sessions();
    let report = h.engine.scan_folder_now(folder_id).await.unwrap();
    assert_eq!(report.uploaded_files, 0);
    assert_eq!(
        h.state.upload_sessions(),
        sessions_before,
        "删除不得触发任何上传"
    );
    let children = cloud_children(&h.client, target).await;
    assert_eq!(children.len(), 2, "云端文件必须保留");
}

#[tokio::test]
async fn mtime_change_with_same_content_does_not_create_version() {
    let h = harness().await;
    let work = h.files.path().join("same");
    std::fs::create_dir_all(&work).unwrap();
    let file = work.join("same.txt");
    write(&file, b"same-content");
    let (folder_id, _) = add_and_scan(&h.engine, BackupFolderDraft::new(&work)).await;
    let target = default_target(&h.client, "same").await;
    let node = cloud_children(&h.client, target).await.remove(0);
    let sessions_before = h.state.upload_sessions();

    set_mtime_forward(&file);
    let report = h.engine.scan_folder_now(folder_id).await.unwrap();
    assert_eq!(report.hashed_files, 1, "修改时间变化必须重新计算摘要");
    assert_eq!(report.unchanged_files, 1);
    assert_eq!(report.updated_files, 0);
    assert_eq!(
        h.state.upload_sessions(),
        sessions_before,
        "内容相同不得上传"
    );
    assert_eq!(
        h.client.list_versions(node.id).await.unwrap().len(),
        1,
        "内容相同不得产生新版本"
    );
    let activity = h.engine.recent_activity(10).await.unwrap();
    assert!(
        activity
            .iter()
            .any(|item| item.kind == BackupActivityKind::Unchanged)
    );
}

#[tokio::test]
async fn default_and_custom_exclusions_apply_and_hidden_can_be_enabled() {
    let h = harness().await;
    let work = h.files.path().join("mixed");
    std::fs::create_dir_all(work.join("skipdir")).unwrap();
    std::fs::create_dir_all(work.join("node_modules")).unwrap();
    for name in [
        ".DS_Store",
        "Thumbs.db",
        "desktop.ini",
        "._resource",
        "~$report.docx",
        ".~lock.report.docx#",
        "a.tmp",
        "a.part",
        "a.crdownload",
        "photo.psd",
        ".hidden",
    ] {
        write(&work.join(name), b"x");
    }
    write(&work.join("skipdir/inside.txt"), b"x");
    write(&work.join("node_modules/pkg.js"), b"x");
    write(&work.join("keep.txt"), b"keep");
    write(&work.join("shortcut.lnk"), b"shortcut-bytes");

    let mut draft = BackupFolderDraft::new(&work);
    draft.exclude_globs = vec!["*.psd".to_owned(), "node_modules".to_owned()];
    draft.exclude_subfolders = vec!["skipdir".to_owned()];
    let preflight = h.engine.preflight(&draft).await.unwrap();
    assert_eq!(
        preflight.file_count, 2,
        "预检必须应用用户排除规则（keep.txt 与 shortcut.lnk）"
    );
    let (folder_id, report) = add_and_scan(&h.engine, draft).await;
    assert_eq!(report.status, BackupFolderStatus::IdleOk);
    let target = default_target(&h.client, "mixed").await;
    let mut names: Vec<String> = cloud_children(&h.client, target)
        .await
        .into_iter()
        .map(|node| node.name)
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["keep.txt".to_owned(), "shortcut.lnk".to_owned()],
        "默认/自定义排除与隐藏文件规则"
    );

    let mut settings = h.engine.settings().await.unwrap();
    settings.include_hidden = true;
    h.engine.set_settings(settings).await.unwrap();
    h.engine.scan_folder_now(folder_id).await.unwrap();
    let names: Vec<String> = cloud_children(&h.client, target)
        .await
        .into_iter()
        .map(|node| node.name)
        .collect();
    assert!(
        names.contains(&".hidden".to_owned()),
        "开启后应备份隐藏文件"
    );
}

#[tokio::test]
async fn nfd_local_name_merges_with_existing_nfc_cloud_file() {
    let h = harness().await;
    let first_dir = h.files.path().join("mac");
    std::fs::create_dir_all(&first_dir).unwrap();
    write(&first_dir.join("caf\u{e9}.txt"), b"same-content");
    let (_, first) = add_and_scan(&h.engine, BackupFolderDraft::new(&first_dir)).await;
    assert_eq!(first.uploaded_files, 1);
    let target = default_target(&h.client, "mac").await;
    let node = cloud_children(&h.client, target).await.remove(0);
    let sessions_before = h.state.upload_sessions();

    let second_dir = h.files.path().join("windows");
    std::fs::create_dir_all(&second_dir).unwrap();
    write(&second_dir.join("cafe\u{301}.txt"), b"same-content");
    let mut draft = BackupFolderDraft::new(&second_dir);
    draft.cloud_target_id = Some(target);
    let (_, second) = add_and_scan(&h.engine, draft).await;
    assert_eq!(second.unchanged_files, 1, "NFD 必须与 NFC 视为同一文件");
    assert_eq!(second.uploaded_files, 0);
    assert_eq!(second.updated_files, 0);
    assert_eq!(h.state.upload_sessions(), sessions_before);
    assert_eq!(cloud_children(&h.client, target).await.len(), 1);
    assert_eq!(h.client.list_versions(node.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn large_directory_second_scan_reads_no_files_and_sends_no_uploads() {
    const FILES: usize = 20_000;
    let h = harness().await;
    let work = h.files.path().join("large");
    std::fs::create_dir_all(&work).unwrap();
    let mut local = Vec::with_capacity(FILES);
    for index in 0..FILES {
        let name = format!("f{index:05}.txt");
        let path = work.join(&name);
        std::fs::write(&path, b"x").unwrap();
        let mtime_ns = std::fs::metadata(&path)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64;
        local.push((name, mtime_ns));
    }

    // 预先在云端与本地索引中放入 20,000 个“已备份且未变化”的文件。这样测试
    // 只验证索引快速跳过，不会把时间花在 20,000 次真实上传上。
    let root = h.client.account().unwrap().root_node_id;
    let cloud_folder = h.client.create_folder(root, "large-cloud").await.unwrap();
    let hash = {
        let mut hasher = ContentHasher::new();
        hasher.update(b"x");
        hasher.finish_hex()
    };
    let mut index_rows = Vec::with_capacity(FILES);
    {
        let mut nodes = h.state.nodes.lock().unwrap();
        for (name, mtime_ns) in &local {
            let node_id = Uuid::new_v4();
            let node_typed = NodeId::parse(&node_id.to_string()).unwrap();
            let node_key = NodeKey::generate().unwrap();
            let wrapped = wrap_node_key(&h.state.master_key, &node_typed, &node_key).unwrap();
            let node_meta =
                encrypt_node_metadata(&node_key, &node_typed, &NodeMetadata { name: name.clone() })
                    .unwrap();
            let version_id = Uuid::new_v4();
            let version_typed = VersionId::parse(&version_id.to_string()).unwrap();
            let content_id = Uuid::new_v4();
            let content_typed = ContentId::parse(&content_id.to_string()).unwrap();
            let version_meta = encrypt_version_metadata(
                &node_key,
                &version_typed,
                &VersionMetadata {
                    content_id: content_typed,
                    size: 1,
                    mtime: *mtime_ns / 1_000_000_000,
                    mime: "text/plain".to_owned(),
                    content_sha256: Some(hash.clone()),
                },
            )
            .unwrap();
            nodes.insert(
                node_id,
                Node {
                    id: node_id,
                    parent_id: Some(cloud_folder.id),
                    kind: "file".to_owned(),
                    name_token: None,
                    wrapped_key: Some(encode_hex(&wrapped)),
                    encrypted_meta: Some(encode_hex(&node_meta)),
                    row_version: 1,
                    current_version: Some(Version {
                        id: version_id,
                        content_id,
                        encrypted_meta: encode_hex(&version_meta),
                        is_current: true,
                        chunks: Vec::new(),
                    }),
                    versions: Vec::new(),
                },
            );
            index_rows.push((name.clone(), *mtime_ns, node_id));
        }
    }

    let mut draft = BackupFolderDraft::new(&work);
    draft.cloud_target_id = Some(cloud_folder.id);
    let folder = h.engine.add_folder(draft).await.unwrap();
    let user_id = h.client.account().unwrap().user_id;
    {
        let db =
            rusqlite::Connection::open(h._app_data.path().join(client_core::DATABASE_FILE_NAME))
                .unwrap();
        let tx = db.unchecked_transaction().unwrap();
        {
            let mut statement = tx
                .prepare(
                    r#"
                    INSERT INTO backup_index (
                        user_id, folder_id, relative_path, size, mtime_ns,
                        cloud_node_id, content_sha256, updated_at
                    )
                    VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, 0)
                    "#,
                )
                .unwrap();
            for (name, mtime_ns, node_id) in &index_rows {
                statement
                    .execute(rusqlite::params![
                        user_id.to_string(),
                        folder.id.to_string(),
                        name,
                        *mtime_ns,
                        node_id.to_string(),
                        hash,
                    ])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
    }

    let sessions = h.state.upload_sessions();
    let report = h.engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(report.scanned_files, FILES as u64);
    assert_eq!(report.unchanged_files, FILES as u64);
    assert_eq!(report.hashed_files, 0, "未变文件不得被读取");
    assert_eq!(
        h.state.upload_sessions(),
        sessions,
        "未变文件不得发上传请求"
    );
}

#[tokio::test]
async fn file_changed_during_upload_is_retried_on_next_round() {
    let h = harness().await;
    let work = h.files.path().join("changing");
    std::fs::create_dir_all(&work).unwrap();
    let file = work.join("a.txt");
    write(&file, b"initial");
    *h.state.mutate_path_on_first_put.lock().unwrap() = Some(file.clone());
    let (folder_id, first) = add_and_scan(&h.engine, BackupFolderDraft::new(&work)).await;
    assert_eq!(first.uploaded_files, 0);
    assert!(first.retry_later >= 1, "上传中被修改必须记录稍后重试");
    assert!(
        h.engine
            .recent_activity(10)
            .await
            .unwrap()
            .iter()
            .any(|item| item.kind == BackupActivityKind::RetryLater)
    );

    let second = h.engine.scan_folder_now(folder_id).await.unwrap();
    assert_eq!(second.uploaded_files, 1, "下一轮应成功");
    let target = default_target(&h.client, "changing").await;
    assert_eq!(cloud_children(&h.client, target).await.len(), 1);
}

#[tokio::test]
async fn pause_prevents_upload_and_resume_continues() {
    let h = harness().await;
    let work = h.files.path().join("paused");
    std::fs::create_dir_all(&work).unwrap();
    let folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();
    h.engine.pause_all().await.unwrap();
    write(&work.join("a.txt"), b"a");
    let report = h.engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(report.status, BackupFolderStatus::Paused);
    assert_eq!(report.uploaded_files, 0);
    assert_eq!(h.state.upload_sessions(), 0);

    h.engine.resume_all().await.unwrap();
    let report = h.engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(report.status, BackupFolderStatus::IdleOk);
    assert_eq!(report.uploaded_files, 1);

    h.engine.pause_folder(folder.id).await.unwrap();
    write(&work.join("b.txt"), b"b");
    let report = h.engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(report.status, BackupFolderStatus::Paused);
    assert_eq!(report.uploaded_files, 0);
    h.engine.resume_folder(folder.id).await.unwrap();
    let report = h.engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(report.uploaded_files, 1);
}

#[tokio::test]
async fn locked_client_reports_needs_unlock_and_resumes_after_unlock() {
    let h = harness().await;
    let work = h.files.path().join("locked");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("a.txt"), b"a");
    let folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();
    h.client.lock().unwrap();
    let report = h.engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(report.status, BackupFolderStatus::NeedsUnlock);
    assert_eq!(h.state.upload_sessions(), 0);
    let state = h.engine.folder_states().remove(0);
    assert_eq!(state.pause_reason, Some(BackupPauseReason::NeedsUnlock));
    assert_eq!(h.engine.summary().status, BackupSummaryStatus::NeedsUnlock);

    h.client.unlock(PASSWORD).await.unwrap();
    let report = h.engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(report.status, BackupFolderStatus::IdleOk);
    assert_eq!(report.uploaded_files, 1);
}

#[tokio::test]
async fn server_unavailable_pauses_then_retry_succeeds() {
    let h = harness().await;
    let work = h.files.path().join("network");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("a.txt"), b"a");
    *h.state.fail_upload_session_remaining.lock().unwrap() = 1;
    let (folder_id, first) = add_and_scan(&h.engine, BackupFolderDraft::new(&work)).await;
    assert_eq!(first.status, BackupFolderStatus::Paused);
    assert_eq!(
        h.engine.folder_states().remove(0).pause_reason,
        Some(BackupPauseReason::Network)
    );
    assert!(
        h.engine
            .recent_activity(10)
            .await
            .unwrap()
            .iter()
            .any(|item| item.kind == BackupActivityKind::Failed)
    );

    let second = h.engine.scan_folder_now(folder_id).await.unwrap();
    assert_eq!(second.status, BackupFolderStatus::IdleOk);
    assert_eq!(second.uploaded_files, 1);
}

#[tokio::test]
async fn folder_name_conflict_race_relists_and_reuses() {
    let h = harness().await;
    let work = h.files.path().join("race");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("a.txt"), b"a");
    *h.state.fail_create_folder_once.lock().unwrap() = true;
    let (_, report) = add_and_scan(&h.engine, BackupFolderDraft::new(&work)).await;
    assert_eq!(report.status, BackupFolderStatus::IdleOk);
    assert_eq!(report.uploaded_files, 1);
    let target = default_target(&h.client, "race").await;
    assert_eq!(cloud_children(&h.client, target).await.len(), 1);
}

#[tokio::test]
async fn file_watcher_triggers_backup_with_debounce() {
    let h = harness_with_watch(true, Duration::from_millis(100)).await;
    let work = h.files.path().join("watched");
    std::fs::create_dir_all(&work).unwrap();
    let folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();
    h.engine.start().await.unwrap();
    write(&work.join("new.txt"), b"watched");

    let uploaded = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let activity = h.engine.recent_activity(20).await.unwrap();
            if activity.iter().any(|item| item.relative_path == "new.txt") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    h.engine.stop().await;
    assert!(uploaded.is_ok(), "文件监听应在防抖后触发备份");
    assert!(h.engine.folder(folder.id).await.is_ok());
}

#[tokio::test]
async fn folder_config_can_be_updated_and_removed() {
    let h = harness().await;
    let work = h.files.path().join("config");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("a.log"), b"log");
    write(&work.join("b.txt"), b"text");
    let folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();

    let updated = h
        .engine
        .update_folder(
            folder.id,
            BackupFolderUpdate {
                exclude_globs: Some(vec!["*.log".to_owned()]),
                enabled: Some(false),
                ..BackupFolderUpdate::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.exclude_globs, vec!["*.log".to_owned()]);
    assert!(!updated.enabled);
    let paused = h.engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(paused.status, BackupFolderStatus::Paused);
    assert_eq!(h.state.upload_sessions(), 0);

    h.engine
        .update_folder(
            folder.id,
            BackupFolderUpdate {
                enabled: Some(true),
                ..BackupFolderUpdate::default()
            },
        )
        .await
        .unwrap();
    h.engine.scan_folder_now(folder.id).await.unwrap();
    let target = default_target(&h.client, "config").await;
    let names: Vec<String> = cloud_children(&h.client, target)
        .await
        .into_iter()
        .map(|node| node.name)
        .collect();
    assert_eq!(names, vec!["b.txt".to_owned()]);

    // 更换本地路径：清空索引、重新解析默认云端目标（旧云端内容保留）。
    let other = h.files.path().join("config2");
    std::fs::create_dir_all(&other).unwrap();
    write(&other.join("c.txt"), b"new");
    let updated = h
        .engine
        .update_folder(
            folder.id,
            BackupFolderUpdate {
                local_path: Some(other.clone()),
                ..BackupFolderUpdate::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.local_path, other);
    h.engine.scan_folder_now(folder.id).await.unwrap();
    let new_target = default_target(&h.client, "config2").await;
    let names: Vec<String> = cloud_children(&h.client, new_target)
        .await
        .into_iter()
        .map(|node| node.name)
        .collect();
    assert_eq!(names, vec!["c.txt".to_owned()]);
    assert_eq!(
        cloud_children(&h.client, target).await.len(),
        1,
        "更换本地路径不得删除旧云端内容"
    );

    h.engine.remove_folder(folder.id).await.unwrap();
    assert!(h.engine.list_folders().await.unwrap().is_empty());
    assert_eq!(
        cloud_children(&h.client, target).await.len(),
        1,
        "移除备份文件夹不得删除云端内容"
    );
}

#[tokio::test]
async fn unreadable_file_is_counted_but_does_not_block_others() {
    let h = harness().await;
    let work = h.files.path().join("unreadable");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("good.txt"), b"good");
    let locked = work.join("locked.txt");
    write(&locked, b"locked");

    #[cfg(windows)]
    let _lock = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .unwrap()
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    }

    let (_, report) = add_and_scan(&h.engine, BackupFolderDraft::new(&work)).await;
    if report.unreadable_files == 0 {
        // root 或具有 CAP_DAC_OVERRIDE 的运行环境仍能读取权限 0 的文件。
        // 这不是引擎缺陷；恢复权限后跳过断言，避免 CI 误报。
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600));
        }
        eprintln!("当前用户可读取权限 0 的文件（可能以 root 运行），跳过不可读断言");
        return;
    }
    assert!(
        report.unreadable_files >= 1,
        "被占用文件必须计入无法读取：{report:?}"
    );
    assert_eq!(report.uploaded_files, 1, "其他文件不得被阻塞");
    match report.status {
        BackupFolderStatus::Error { unreadable_count } => assert!(unreadable_count >= 1),
        other => panic!("期望 error 状态，实际 {other:?}"),
    }
    let target = default_target(&h.client, "unreadable").await;
    let names: Vec<String> = cloud_children(&h.client, target)
        .await
        .into_iter()
        .map(|node| node.name)
        .collect();
    assert_eq!(names, vec!["good.txt".to_owned()]);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600));
    }
}

#[tokio::test]
async fn concurrent_manual_scans_upload_only_once() {
    let h = harness().await;
    let work = h.files.path().join("concurrent");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("a.txt"), b"a");
    let folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();

    let (first, second) = tokio::join!(
        h.engine.scan_folder_now(folder.id),
        h.engine.scan_folder_now(folder.id)
    );
    first.unwrap();
    second.unwrap();

    assert_eq!(h.state.upload_sessions(), 1, "并发扫描不得重复上传");
    let target = default_target(&h.client, "concurrent").await;
    let children = cloud_children(&h.client, target).await;
    assert_eq!(children.len(), 1);
    assert_eq!(
        h.client.list_versions(children[0].id).await.unwrap().len(),
        1,
        "并发扫描不得多出一个版本"
    );
}

#[tokio::test]
async fn scheduler_retries_network_failures_until_success() {
    let h = harness_with_retry(Duration::from_millis(100)).await;
    let work = h.files.path().join("retry-network");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("a.txt"), b"a");
    h.engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();
    // 连续两轮不可用，第三轮恢复；全程不手动调用 scan。
    *h.state.fail_upload_session_remaining.lock().unwrap() = 2;

    h.engine.start().await.unwrap();
    wait_for_added(&h.engine, "a.txt").await;
    h.engine.stop().await;

    let target = default_target(&h.client, "retry-network").await;
    assert_eq!(cloud_children(&h.client, target).await.len(), 1);
}

#[tokio::test]
async fn scheduler_retries_file_changed_later_until_success() {
    let h = harness_with_retry(Duration::from_millis(100)).await;
    let work = h.files.path().join("retry-changed");
    std::fs::create_dir_all(&work).unwrap();
    let file = work.join("a.txt");
    write(&file, b"initial");
    *h.state.mutate_path_on_first_put.lock().unwrap() = Some(file);
    let _folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();

    h.engine.start().await.unwrap();
    wait_for_added(&h.engine, "a.txt").await;
    h.engine.stop().await;

    let target = default_target(&h.client, "retry-changed").await;
    assert_eq!(cloud_children(&h.client, target).await.len(), 1);
}

#[tokio::test]
async fn engine_can_restart_after_stop() {
    let h = harness().await;
    let work = h.files.path().join("restart");
    std::fs::create_dir_all(&work).unwrap();
    let _folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();

    write(&work.join("a.txt"), b"a");
    h.engine.start().await.unwrap();
    wait_for_added(&h.engine, "a.txt").await;
    h.engine.stop().await;

    write(&work.join("b.txt"), b"b");
    h.engine.start().await.unwrap();
    wait_for_added(&h.engine, "b.txt").await;
    h.engine.stop().await;

    let target = default_target(&h.client, "restart").await;
    assert_eq!(cloud_children(&h.client, target).await.len(), 2);
}

#[tokio::test]
async fn lock_during_upload_reports_needs_unlock_and_resumes() {
    let h = harness().await;
    let work = h.files.path().join("lock-mid-run");
    std::fs::create_dir_all(&work).unwrap();
    write(&work.join("a.txt"), b"a");
    write(&work.join("b.txt"), b"b");
    let _folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();
    // 第一次 PUT 时锁定客户端：第一个文件上传完成后，process_tree 必须
    // 在下一个文件前发现未解锁并返回 needs_unlock。
    *h.state.lock_client_on_first_put.lock().unwrap() = Some(h.client.clone());

    let first = h
        .engine
        .scan_folder_now(h.engine.list_folders().await.unwrap()[0].id)
        .await
        .unwrap();
    assert_eq!(first.status, BackupFolderStatus::NeedsUnlock);
    assert_eq!(first.uploaded_files, 1);

    h.client.unlock(PASSWORD).await.unwrap();
    let second = h
        .engine
        .scan_folder_now(h.engine.list_folders().await.unwrap()[0].id)
        .await
        .unwrap();
    assert_eq!(second.status, BackupFolderStatus::IdleOk);
    assert_eq!(second.uploaded_files, 1);

    let target = default_target(&h.client, "lock-mid-run").await;
    assert_eq!(cloud_children(&h.client, target).await.len(), 2);
}

#[tokio::test]
async fn folder_path_conflicts_are_rejected() {
    let h = harness().await;
    let base = h.files.path().join("paths");
    let outer = base.join("outer");
    let child = outer.join("child");
    let sibling = base.join("sibling");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();

    let _ = h
        .engine
        .add_folder(BackupFolderDraft::new(&outer))
        .await
        .unwrap();

    let duplicate = h
        .engine
        .add_folder(BackupFolderDraft::new(&outer))
        .await
        .unwrap_err()
        .to_string();
    assert!(duplicate.contains("已经添加"), "{duplicate}");

    let descendant = h
        .engine
        .add_folder(BackupFolderDraft::new(&child))
        .await
        .unwrap_err()
        .to_string();
    assert!(descendant.contains("不能互相包含"), "{descendant}");

    let parent = h
        .engine
        .add_folder(BackupFolderDraft::new(&base))
        .await
        .unwrap_err()
        .to_string();
    assert!(parent.contains("不能互相包含"), "{parent}");

    let sibling_folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&sibling))
        .await
        .unwrap();

    // update_folder 更换本地路径时也必须做同样的包含检查。
    let nested_under_outer = outer.join("sibling");
    std::fs::create_dir_all(&nested_under_outer).unwrap();
    let update_error = h
        .engine
        .update_folder(
            sibling_folder.id,
            BackupFolderUpdate {
                local_path: Some(nested_under_outer),
                ..BackupFolderUpdate::default()
            },
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(update_error.contains("不能互相包含"), "{update_error}");

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let upper = PathBuf::from(outer.to_string_lossy().to_uppercase());
        if upper.exists() {
            let error = h
                .engine
                .add_folder(BackupFolderDraft::new(upper))
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains("已经添加"), "{error}");
        } else {
            eprintln!("文件系统区分大小写，跳过大小写重复路径断言");
        }
    }
}

// ---------------------------------------------------------------------------
// 微信文件类别备份（add-wechat-backup 第 2 组）
// ---------------------------------------------------------------------------

const WECHAT_ACCOUNT: &str = "wxid_test0001";
const WECHAT_ATTACH_HASH: &str = "0123456789abcdef0123456789abcdef";

/// 构造一个微信 4.x 夹具（全部为虚构数据，不读取真实微信目录）。
fn create_wechat_fixture(base: &Path) -> PathBuf {
    let documents = base.join("文档");
    let account = documents.join("xwechat_files").join(WECHAT_ACCOUNT);
    let mut directories: Vec<String> = [
        "msg/file/2026-10",
        "msg/file/cache",
        "msg/file/temp",
        "msg/video/2026-10",
        "db_storage/message",
        "cache/2026-10",
        "temp",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    directories.push(format!("msg/attach/{WECHAT_ATTACH_HASH}/2026-10"));
    for directory in directories {
        std::fs::create_dir_all(account.join(directory)).unwrap();
    }
    write(&account.join("msg/file/2026-10/报告.pdf"), b"pdf-report");
    write(&account.join("msg/file/cache/cache.bin"), b"cache-file");
    write(&account.join("msg/file/temp/temp.bin"), b"temp-file");
    write(
        &account
            .join("msg/attach")
            .join(WECHAT_ATTACH_HASH)
            .join("2026-10/pic.dat"),
        b"image-bytes",
    );
    write(&account.join("msg/video/2026-10/clip.mp4"), b"video-bytes");
    write(
        &account.join("db_storage/message/message_0.db"),
        b"database-bytes",
    );
    write(&account.join("cache/2026-10/cache.bin"), b"account-cache");
    write(&account.join("temp/temp.bin"), b"account-temp");
    documents
}

async fn discover_wechat_account(documents: &Path) -> WechatAccount {
    let locator = WechatLocator::with_system_locations(Arc::new(FakeWechatLocations {
        documents: Some(documents.to_path_buf()),
    }));
    locator
        .discover(&[])
        .await
        .unwrap()
        .into_iter()
        .find(|account| account.account_name == WECHAT_ACCOUNT)
        .expect("夹具中应有微信账号")
}

async fn cloud_folder_by_path(client: &Client, names: &[&str]) -> Uuid {
    let mut parent = client.account().unwrap().root_node_id;
    for name in names {
        let children = cloud_children(client, parent).await;
        parent = children
            .into_iter()
            .find(|node| node.kind == NodeKind::Folder && node.name == *name)
            .unwrap_or_else(|| panic!("缺少云端文件夹 {name}"))
            .id;
    }
    parent
}

async fn cloud_file_names(client: &Client, folder_id: Uuid) -> Vec<String> {
    let mut names: Vec<String> = cloud_children(client, folder_id)
        .await
        .into_iter()
        .map(|node| node.name)
        .collect();
    names.sort();
    names
}

async fn wechat_source(engine: &BackupEngine, category: WechatCategory) -> WechatBackupSource {
    engine
        .list_wechat_sources()
        .await
        .unwrap()
        .into_iter()
        .find(|source| source.category == category)
        .expect("应有对应类别的微信来源")
}

fn projected_upload_bytes(size: u64) -> u64 {
    let chunks = crypto_core::chunk_count_for_size(size).unwrap_or(1);
    size + u64::from(chunks) * crypto_core::CHUNK_CIPHERTEXT_OVERHEAD as u64
}

fn tree_snapshot(root: &Path) -> Vec<(PathBuf, u64, i64)> {
    fn walk(directory: &Path, base: &Path, output: &mut Vec<(PathBuf, u64, i64)>) {
        let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let metadata = entry.metadata().unwrap();
            let path = entry.path();
            if metadata.is_dir() {
                walk(&path, base, output);
            } else {
                let modified = metadata
                    .modified()
                    .unwrap()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as i64;
                output.push((
                    path.strip_prefix(base).unwrap().to_path_buf(),
                    metadata.len(),
                    modified,
                ));
            }
        }
    }
    let mut output = Vec::new();
    walk(root, root, &mut output);
    output.sort();
    output
}

fn tree_totals(root: &Path) -> (u64, u64) {
    tree_snapshot(root)
        .into_iter()
        .fold((0_u64, 0_u64), |(files, bytes), (_, size, _)| {
            (files + 1, bytes + size)
        })
}

#[tokio::test]
async fn wechat_chat_files_incremental_and_uncheck_keeps_cloud() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_wechat_fixture(fixture.path());
    // create_wechat_fixture 的父目录是临时目录，需要让它活得比测试久。
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::ChatFiles])
        .await
        .unwrap();

    let sources = h.engine.list_wechat_sources().await.unwrap();
    assert_eq!(sources.len(), 1, "聊天文件只有一个来源路径");
    assert_eq!(sources[0].category, WechatCategory::ChatFiles);
    assert_eq!(sources[0].relative_path, "wxid_test0001/msg/file");
    // 微信来源不出现在备份文件夹列表。
    assert!(h.engine.list_folders().await.unwrap().is_empty());

    let report = h.engine.scan_all_now().await.unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(
        report[0].uploaded_files, 1,
        "只上传聊天文件，缓存与临时目录不传"
    );

    let source = wechat_source(&h.engine, WechatCategory::ChatFiles).await;
    assert_eq!(
        source.cloud_path.as_deref(),
        Some("全部文件/微信备份/TEST-PC/wxid_test0001/聊天文件/file")
    );
    let target = cloud_folder_by_path(
        &h.client,
        &["微信备份", "TEST-PC", WECHAT_ACCOUNT, "聊天文件", "file"],
    )
    .await;
    let pdf_dir = cloud_folder_by_path(
        &h.client,
        &[
            "微信备份",
            "TEST-PC",
            WECHAT_ACCOUNT,
            "聊天文件",
            "file",
            "2026-10",
        ],
    )
    .await;
    assert_eq!(cloud_file_names(&h.client, pdf_dir).await, vec!["报告.pdf"]);
    assert!(
        !cloud_file_names(&h.client, target)
            .await
            .contains(&"cache".to_owned()),
        "来源内的缓存目录不得上传"
    );

    // 新增文件后增量只上传新文件。
    let later = documents.join("xwechat_files").join(WECHAT_ACCOUNT);
    std::fs::create_dir_all(later.join("msg/file/2026-11")).unwrap();
    write(&later.join("msg/file/2026-11/新文件.docx"), b"new-doc");
    let sessions_before = h.state.upload_sessions();
    let report = h.engine.scan_folder_now(source.folder_id).await.unwrap();
    assert_eq!(report.uploaded_files, 1, "只上传新增文件");
    assert_eq!(report.unchanged_files, 1, "已有文件不重复上传");
    assert_eq!(h.state.upload_sessions(), sessions_before + 1);

    // 取消勾选：停止备份、云端保留。
    h.engine.set_wechat_categories(&account, &[]).await.unwrap();
    assert!(h.engine.list_wechat_sources().await.unwrap().is_empty());
    let accounts = h.engine.list_wechat_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert!(
        accounts[0]
            .categories
            .iter()
            .all(|category| !category.selected),
        "取消勾选后类别选择状态必须持久化为未勾选"
    );
    assert!(
        accounts[0]
            .categories
            .iter()
            .all(|category| category.status.is_none())
    );
    std::fs::create_dir_all(later.join("msg/file/2026-12")).unwrap();
    write(&later.join("msg/file/2026-12/之后.pdf"), b"after-uncheck");
    let sessions_before = h.state.upload_sessions();
    h.engine.scan_all_now().await.unwrap();
    assert_eq!(
        h.state.upload_sessions(),
        sessions_before,
        "取消勾选后不得再上传"
    );
    assert_eq!(
        cloud_file_names(&h.client, pdf_dir).await,
        vec!["报告.pdf"],
        "云端内容必须保留"
    );
    let november = cloud_folder_by_path(
        &h.client,
        &[
            "微信备份",
            "TEST-PC",
            WECHAT_ACCOUNT,
            "聊天文件",
            "file",
            "2026-11",
        ],
    )
    .await;
    assert_eq!(
        cloud_file_names(&h.client, november).await,
        vec!["新文件.docx"]
    );
}

#[tokio::test]
async fn wechat_images_and_videos_map_to_attach_and_video_folders() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_wechat_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::ImagesAndVideos])
        .await
        .unwrap();
    assert_eq!(
        h.engine.list_wechat_sources().await.unwrap().len(),
        2,
        "图片与视频有两个来源路径"
    );

    let report = h.engine.scan_all_now().await.unwrap();
    assert_eq!(report.len(), 2);
    assert_eq!(
        report
            .iter()
            .map(|report| report.uploaded_files)
            .sum::<u64>(),
        2
    );

    let attach_dir = cloud_folder_by_path(
        &h.client,
        &[
            "微信备份",
            "TEST-PC",
            WECHAT_ACCOUNT,
            "图片与视频",
            "attach",
            WECHAT_ATTACH_HASH,
            "2026-10",
        ],
    )
    .await;
    assert_eq!(
        cloud_file_names(&h.client, attach_dir).await,
        vec!["pic.dat"]
    );
    let video_dir = cloud_folder_by_path(
        &h.client,
        &[
            "微信备份",
            "TEST-PC",
            WECHAT_ACCOUNT,
            "图片与视频",
            "video",
            "2026-10",
        ],
    )
    .await;
    assert_eq!(
        cloud_file_names(&h.client, video_dir).await,
        vec!["clip.mp4"]
    );

    let accounts = h.engine.list_wechat_accounts().await.unwrap();
    assert_eq!(accounts.len(), 1);
    let images = accounts[0]
        .categories
        .iter()
        .find(|category| category.category == WechatCategory::ImagesAndVideos)
        .unwrap();
    assert!(images.selected);
    assert_eq!(images.status, Some(BackupFolderStatus::IdleOk));
    assert!(images.last_backup_at.is_some());

    // 暂停一个来源后，类别汇总状态为“已暂停”。
    let attach_source = h
        .engine
        .list_wechat_sources()
        .await
        .unwrap()
        .into_iter()
        .find(|source| source.relative_path.ends_with("msg/attach"))
        .unwrap();
    h.engine
        .pause_folder(attach_source.folder_id)
        .await
        .unwrap();
    let accounts = h.engine.list_wechat_accounts().await.unwrap();
    let images = accounts[0]
        .categories
        .iter()
        .find(|category| category.category == WechatCategory::ImagesAndVideos)
        .unwrap();
    assert_eq!(images.status, Some(BackupFolderStatus::Paused));
    assert_eq!(images.pause_reason, Some(BackupPauseReason::User));
    h.engine
        .resume_folder(attach_source.folder_id)
        .await
        .unwrap();

    // 重复应用同一勾选不会产生重复来源（同一路径只注册一次）。
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::ImagesAndVideos])
        .await
        .unwrap();
    assert_eq!(h.engine.list_wechat_sources().await.unwrap().len(), 2);
}

#[tokio::test]
async fn wechat_source_cache_temp_and_default_exclusions_apply_inside_source() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_wechat_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::ChatFiles])
        .await
        .unwrap();
    let account_dir = documents.join("xwechat_files").join(WECHAT_ACCOUNT);
    write(&account_dir.join("msg/file/2026-10/Thumbs.db"), b"system");
    write(&account_dir.join("msg/file/2026-10/草稿.tmp"), b"temp");

    h.engine.scan_all_now().await.unwrap();
    let target = cloud_folder_by_path(
        &h.client,
        &["微信备份", "TEST-PC", WECHAT_ACCOUNT, "聊天文件", "file"],
    )
    .await;
    let names = cloud_file_names(&h.client, target).await;
    assert_eq!(
        names,
        vec!["2026-10".to_owned()],
        "来源内的 cache/temp 与默认系统临时文件都不上传"
    );
    let month = cloud_folder_by_path(
        &h.client,
        &[
            "微信备份",
            "TEST-PC",
            WECHAT_ACCOUNT,
            "聊天文件",
            "file",
            "2026-10",
        ],
    )
    .await;
    assert_eq!(cloud_file_names(&h.client, month).await, vec!["报告.pdf"]);
}

#[tokio::test]
async fn wechat_source_and_ordinary_folder_can_share_a_path() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_wechat_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;
    let source_path = documents
        .join("xwechat_files")
        .join(WECHAT_ACCOUNT)
        .join("msg/file");

    // 同一路径先作为普通备份文件夹，再作为微信来源：不做嵌套/重复冲突拒绝。
    let folder = h
        .engine
        .add_folder(BackupFolderDraft::new(&source_path))
        .await
        .unwrap();
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::ChatFiles])
        .await
        .unwrap();
    assert_eq!(h.engine.list_folders().await.unwrap().len(), 1);
    assert_eq!(h.engine.list_wechat_sources().await.unwrap().len(), 1);

    // 普通文件夹本身位于微信数据根内：全部由微信备份处理，不上传。
    let user_report = h.engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(user_report.uploaded_files, 0);
    assert_eq!(user_report.status, BackupFolderStatus::IdleOk);

    h.engine.scan_all_now().await.unwrap();
    let wechat_target = cloud_folder_by_path(
        &h.client,
        &["微信备份", "TEST-PC", WECHAT_ACCOUNT, "聊天文件", "file"],
    )
    .await;
    assert_eq!(
        cloud_file_names(&h.client, wechat_target).await,
        vec!["2026-10".to_owned()]
    );
}

#[tokio::test]
async fn ordinary_folder_excludes_wechat_data_and_reports_excluded_size() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_wechat_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    write(&documents.join("笔记.txt"), b"notes");

    let data_root = documents.join("xwechat_files");
    let (wechat_files, wechat_bytes) = tree_totals(&data_root);
    assert!(wechat_files > 0 && wechat_bytes > 0);

    // 微信备份未开启时同样排除。
    let preflight = h
        .engine
        .preflight(&BackupFolderDraft::new(&documents))
        .await
        .unwrap();
    assert_eq!(preflight.file_count, 1, "只统计非微信数据");
    assert_eq!(preflight.total_bytes, 5);
    assert_eq!(preflight.excluded_wechat_file_count, wechat_files);
    assert_eq!(preflight.excluded_wechat_bytes, wechat_bytes);

    let (folder_id, report) = add_and_scan(&h.engine, BackupFolderDraft::new(&documents)).await;
    assert_eq!(report.uploaded_files, 1);
    let target = default_target(&h.client, "文档").await;
    assert_eq!(
        cloud_file_names(&h.client, target).await,
        vec!["笔记.txt"],
        "xwechat_files 不得出现在普通备份中"
    );

    // 微信备份开启后仍然排除（两个来源互不冲突，也不做嵌套拒绝）。
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::ChatFiles])
        .await
        .unwrap();
    h.engine.scan_all_now().await.unwrap();
    h.engine.scan_folder_now(folder_id).await.unwrap();
    assert_eq!(cloud_file_names(&h.client, target).await, vec!["笔记.txt"]);
    let wechat_target = cloud_folder_by_path(
        &h.client,
        &["微信备份", "TEST-PC", WECHAT_ACCOUNT, "聊天文件", "file"],
    )
    .await;
    assert!(
        !cloud_file_names(&h.client, wechat_target).await.is_empty(),
        "微信来源仍由微信备份单独处理"
    );
}

#[tokio::test]
async fn wechat_preflight_counts_bytes_quota_and_without_images() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_wechat_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;

    let categories = [
        WechatCategory::ChatFiles,
        WechatCategory::ImagesAndVideos,
        WechatCategory::Databases,
    ];
    let preflight = h
        .engine
        .preflight_wechat(&account, &categories)
        .await
        .unwrap();
    let chat = b"pdf-report".len() as u64;
    let image = b"image-bytes".len() as u64;
    let video = b"video-bytes".len() as u64;
    let database = b"database-bytes".len() as u64;
    assert_eq!(preflight.file_count, 4);
    assert_eq!(preflight.total_bytes, chat + image + video + database);
    assert_eq!(preflight.database_bytes, database);
    assert_eq!(
        preflight.upload_bytes,
        projected_upload_bytes(chat)
            + projected_upload_bytes(image)
            + projected_upload_bytes(video)
            + projected_upload_bytes(database),
        "块开销必须逐文件计算"
    );
    assert_eq!(
        preflight.upload_bytes_without_images_and_videos,
        projected_upload_bytes(chat) + projected_upload_bytes(database)
    );
    assert!(!preflight.over_quota, "默认配额足够");

    // 剩余空间不足 → over_quota；取消图片与视频后仍按同一口径计算。
    h.state.set_quota(1);
    let tight = h
        .engine
        .preflight_wechat(&account, &categories)
        .await
        .unwrap();
    assert!(tight.over_quota);
    assert_eq!(tight.remaining_quota_bytes, Some(1));

    let without_images = h
        .engine
        .preflight_wechat(
            &account,
            &[WechatCategory::ChatFiles, WechatCategory::Databases],
        )
        .await
        .unwrap();
    assert_eq!(
        without_images.upload_bytes, tight.upload_bytes_without_images_and_videos,
        "界面第三格与取消图片视频后的实际预检一致"
    );
}

#[tokio::test]
async fn wechat_backup_does_not_modify_source_directories() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_wechat_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;
    let account_dir = documents.join("xwechat_files").join(WECHAT_ACCOUNT);
    let before = tree_snapshot(&account_dir);

    h.engine
        .set_wechat_categories(
            &account,
            &[
                WechatCategory::ChatFiles,
                WechatCategory::ImagesAndVideos,
                WechatCategory::Databases,
                WechatCategory::PhoneMigration,
            ],
        )
        .await
        .unwrap();
    h.engine.scan_all_now().await.unwrap();
    h.engine.scan_all_now().await.unwrap();
    let _ = h
        .engine
        .preflight_wechat(
            &account,
            &[
                WechatCategory::ChatFiles,
                WechatCategory::ImagesAndVideos,
                WechatCategory::Databases,
            ],
        )
        .await
        .unwrap();
    let snapshot = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(snapshot.outcome, WechatSnapshotOutcome::Completed);

    let after = tree_snapshot(&account_dir);
    assert_eq!(
        before, after,
        "备份期间不得修改微信目录（路径/大小/修改时间）"
    );
}

// ---------------------------------------------------------------------------
// 微信聊天记录数据库快照（add-wechat-backup 第 3 组）
// ---------------------------------------------------------------------------

/// 完整的 4.x 数据库夹具：包含 `.db`、`-wal`/`-shm`、`.kvdb`/`-shm`、
/// `.material` 与 `MMKV/`（design D3 要求的整个目录，而不是只取 `.db`）。
fn create_snapshot_fixture(base: &Path) -> PathBuf {
    let documents = create_wechat_fixture(base);
    let database = documents
        .join("xwechat_files")
        .join(WECHAT_ACCOUNT)
        .join("db_storage");
    for directory in ["message", "contact", "MMKV"] {
        std::fs::create_dir_all(database.join(directory)).unwrap();
    }
    write(&database.join("message/message_0.db"), b"database-bytes");
    write(&database.join("message/message_0.db-wal"), b"wal-bytes");
    write(&database.join("message/message_0.db-shm"), b"shm-bytes");
    write(&database.join("contact/contact.kvdb"), b"kvdb-bytes");
    write(
        &database.join("contact/contact.kvdb-shm"),
        b"kvdb-shm-bytes",
    );
    write(
        &database.join("contact/contact.material"),
        b"material-bytes",
    );
    write(&database.join("MMKV/mmkv.bin"), b"mmkv-bytes");
    documents
}

fn primary_database_file(documents: &Path) -> PathBuf {
    documents
        .join("xwechat_files")
        .join(WECHAT_ACCOUNT)
        .join("db_storage/message/message_0.db")
}

fn is_timestamp_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 18
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[17] == b'Z'
}

async fn try_cloud_folder_by_path(client: &Client, names: &[&str]) -> Option<Uuid> {
    let mut parent = client.account()?.root_node_id;
    for name in names {
        let node = cloud_children(client, parent)
            .await
            .into_iter()
            .find(|node| node.kind == NodeKind::Folder && node.name == *name)?;
        parent = node.id;
    }
    Some(parent)
}

async fn snapshot_parent(client: &Client) -> Option<Uuid> {
    try_cloud_folder_by_path(
        client,
        &["微信备份", "TEST-PC", WECHAT_ACCOUNT, "聊天记录数据库"],
    )
    .await
}

async fn timestamp_dirs(client: &Client, parent: Uuid) -> Vec<client_core::NodeView> {
    cloud_children(client, parent)
        .await
        .into_iter()
        .filter(|node| node.kind == NodeKind::Folder && is_timestamp_name(&node.name))
        .collect()
}

async fn marker_exists(client: &Client, directory: Uuid) -> bool {
    cloud_children(client, directory)
        .await
        .iter()
        .any(|node| node.kind == NodeKind::File && node.name == "快照完成")
}

async fn wait_for_db_status(engine: &BackupEngine, expected: BackupFolderStatus) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let accounts = engine.list_wechat_accounts().await.unwrap();
            let status = accounts
                .iter()
                .flat_map(|account| account.categories.iter())
                .find(|category| category.category == WechatCategory::Databases)
                .and_then(|category| category.status);
            if status == Some(expected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("等待数据库类别状态 {expected:?} 超时"));
}

/// 等待自动快照完成，返回快照根目录。
async fn wait_for_complete_snapshot(client: &Client) -> Uuid {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Some(parent) = snapshot_parent(client).await {
                for directory in timestamp_dirs(client, parent).await {
                    if marker_exists(client, directory.id).await {
                        return parent;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("快照应在微信退出后自动完成")
}

async fn wait_for_db_pause_reason(engine: &BackupEngine, expected: BackupPauseReason) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let accounts = engine.list_wechat_accounts().await.unwrap();
            let category = accounts
                .iter()
                .flat_map(|account| account.categories.iter())
                .find(|category| category.category == WechatCategory::Databases);
            if category.is_some_and(|category| {
                category.status == Some(BackupFolderStatus::Paused)
                    && category.pause_reason == Some(expected)
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("等待数据库类别暂停原因 {expected:?} 超时"));
}

async fn wait_until_trashed(state: &Arc<FakeState>, node_id: Uuid) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if state.trashed_ids().contains(&node_id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("快照目录应被移入回收站");
}

async fn wait_for_requests_above(state: &Arc<FakeState>, baseline: u64) -> u64 {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let current = state.requests();
            if current > baseline {
                return current;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("应产生云端请求")
}

async fn wait_for_complete_snapshot_count(client: &Client, parent: Uuid, expected: usize) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let mut complete = 0;
            for directory in timestamp_dirs(client, parent).await {
                if marker_exists(client, directory.id).await {
                    complete += 1;
                }
            }
            if complete == expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("等待 {expected} 份完整快照超时"));
}

/// 等待请求计数在连续两个 100ms 窗口内保持不变。
async fn wait_for_request_quiescence(state: &Arc<FakeState>) -> u64 {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut last = state.requests();
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let current = state.requests();
            if current == last {
                return current;
            }
            last = current;
        }
    })
    .await
    .expect("请求应在启动维护后静默")
}

#[tokio::test]
async fn wechat_snapshot_waits_for_exit_then_starts_automatically() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_snapshot_fixture(fixture.path());
    let h = harness_full_with(
        false,
        Duration::from_millis(50),
        Duration::from_secs(60),
        Some(documents.clone()),
        Arc::new(FakeProbe::new(true)),
        Duration::from_millis(100),
    )
    .await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();

    h.engine.start().await.unwrap();
    wait_for_db_status(&h.engine, BackupFolderStatus::WaitingForWechatExit).await;
    assert_eq!(h.state.upload_sessions(), 0, "微信运行中不得上传");
    assert!(
        snapshot_parent(&h.client).await.is_none(),
        "微信运行中不得创建时间戳目录"
    );

    // 微信退出后（探测切换为未运行），调度器在下一次检查自动开始快照。
    h.probe.set_running(false);
    h.clock.advance(Duration::from_millis(500));
    let parent = wait_for_complete_snapshot(&h.client).await;
    let directories = timestamp_dirs(&h.client, parent).await;
    assert_eq!(directories.len(), 1, "应新建一份完整快照");
    assert!(marker_exists(&h.client, directories[0].id).await);
    h.engine.stop().await;
}

#[tokio::test]
async fn wechat_snapshot_aborts_when_wechat_starts_mid_upload() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_snapshot_fixture(fixture.path());
    let h = harness_full_with(
        false,
        Duration::from_millis(50),
        Duration::from_secs(60),
        Some(documents.clone()),
        Arc::new(FakeProbe::new(false)),
        Duration::from_secs(10),
    )
    .await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();

    // 第一个文件 PUT 之后把探测切为“运行中”：剩余文件停止上传并放弃本次快照。
    *h.state.flip_probe_on_first_put.lock().unwrap() = Some(h.probe.clone());
    let first = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    // 探测到微信启动时状态回到“等微信退出后备份”，本次上传已放弃。
    assert_eq!(first.outcome, WechatSnapshotOutcome::WaitingForWechatExit);
    let parent = snapshot_parent(&h.client).await.unwrap();
    assert!(
        timestamp_dirs(&h.client, parent).await.is_empty(),
        "放弃的时间戳目录必须移入回收站"
    );
    assert!(
        !h.state.trashed_ids().is_empty(),
        "放弃的目录必须进入回收站而不是彻底删除"
    );

    // 微信退出后重试成功，且只有一份完整快照。
    h.probe.set_running(false);
    h.clock.advance(Duration::from_secs(1));
    let second = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(second.outcome, WechatSnapshotOutcome::Completed);
    let directories = timestamp_dirs(&h.client, parent).await;
    assert_eq!(directories.len(), 1);
    assert!(marker_exists(&h.client, directories[0].id).await);
}

#[tokio::test]
async fn wechat_snapshot_aborts_when_a_database_file_changes_during_upload() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_snapshot_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();

    // 第一个文件 PUT 时修改该文件：上传任务报告 FileChanged，本次快照放弃。
    *h.state.mutate_path_on_first_put.lock().unwrap() = Some(primary_database_file(&documents));
    let report = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(report.outcome, WechatSnapshotOutcome::Aborted);
    let parent = snapshot_parent(&h.client).await.unwrap();
    assert!(
        timestamp_dirs(&h.client, parent).await.is_empty(),
        "文件变化时本次目录必须移入回收站"
    );
    assert!(!h.state.trashed_ids().is_empty());

    // 文件稳定后重试成功，且只有一份完整快照。
    h.clock.advance(Duration::from_secs(1));
    let retried = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(retried.outcome, WechatSnapshotOutcome::Completed);
    let directories = timestamp_dirs(&h.client, parent).await;
    assert_eq!(directories.len(), 1);
    assert!(marker_exists(&h.client, directories[0].id).await);
}

#[tokio::test]
async fn wechat_snapshot_skips_when_content_unchanged() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_snapshot_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();

    let first = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(first.outcome, WechatSnapshotOutcome::Completed);
    let parent = snapshot_parent(&h.client).await.unwrap();
    assert_eq!(timestamp_dirs(&h.client, parent).await.len(), 1);
    let sessions = h.state.upload_sessions();
    assert!(sessions >= 1);

    // 大小与修改时间完全一致：不读文件、不发上传请求。
    let second = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(second.outcome, WechatSnapshotOutcome::Unchanged);
    assert_eq!(h.state.upload_sessions(), sessions, "未变不得上传");
    assert_eq!(
        timestamp_dirs(&h.client, parent).await.len(),
        1,
        "不得新建目录"
    );

    // 只改修改时间不改内容：重新计算摘要后与云端一致，同样不新建。
    set_mtime_forward(&primary_database_file(&documents));
    let third = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(third.outcome, WechatSnapshotOutcome::Unchanged);
    assert_eq!(h.state.upload_sessions(), sessions, "内容未变不得上传");
    assert_eq!(timestamp_dirs(&h.client, parent).await.len(), 1);
}

#[tokio::test]
async fn wechat_snapshot_retention_keeps_latest_and_applies_new_setting() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_snapshot_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();
    let first = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(first.outcome, WechatSnapshotOutcome::Completed);
    let parent = snapshot_parent(&h.client).await.unwrap();

    // 默认保留 3 份：第 2–4 份完成后，最早一份进入回收站。
    for index in 1..=3 {
        let file = primary_database_file(&documents);
        write(&file, format!("database-content-{index}").as_bytes());
        set_mtime_forward(&file);
        h.clock.advance(Duration::from_secs(1));
        let report = h
            .engine
            .snapshot_wechat_databases_now(&account)
            .await
            .unwrap();
        assert_eq!(report.outcome, WechatSnapshotOutcome::Completed);
    }
    assert_eq!(
        timestamp_dirs(&h.client, parent).await.len(),
        3,
        "默认只保留最近 3 份完整快照"
    );
    assert!(
        !h.state.trashed_ids().is_empty(),
        "最早的一份必须移入回收站"
    );

    // 保留数改为 1：立即清理到 1 份。
    let mut settings = h.engine.settings().await.unwrap();
    settings.wechat_snapshot_retention = 1;
    h.engine.set_settings(settings).await.unwrap();
    assert_eq!(
        timestamp_dirs(&h.client, parent).await.len(),
        1,
        "保留数改为 1 后只应剩 1 份"
    );
}

#[tokio::test]
async fn wechat_snapshot_cleans_incomplete_before_next_snapshot() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_snapshot_fixture(fixture.path());
    let h = harness_with_wechat(documents.clone()).await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();

    // 预先在云端放一个没有 `快照完成` 的时间戳目录（模拟上次中断）。
    let root = h.client.account().unwrap().root_node_id;
    let mut parent = root;
    let mut incomplete_id = root;
    for name in [
        "微信备份",
        "TEST-PC",
        WECHAT_ACCOUNT,
        "聊天记录数据库",
        "2020-01-01T000000Z",
    ] {
        incomplete_id = h.client.create_folder(parent, name).await.unwrap().id;
        parent = incomplete_id;
    }
    assert!(!marker_exists(&h.client, incomplete_id).await);

    let report = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(report.outcome, WechatSnapshotOutcome::Completed);
    assert!(
        h.state.trashed_ids().contains(&incomplete_id),
        "不完整快照必须在下次快照前移入回收站"
    );
    let parent = snapshot_parent(&h.client).await.unwrap();
    let directories = timestamp_dirs(&h.client, parent).await;
    assert_eq!(directories.len(), 1);
    assert!(marker_exists(&h.client, directories[0].id).await);
}

#[tokio::test]
async fn wechat_snapshot_unchanged_makes_no_cloud_requests() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_snapshot_fixture(fixture.path());
    let h = harness_full_with(
        false,
        Duration::from_millis(50),
        Duration::from_secs(60),
        Some(documents.clone()),
        Arc::new(FakeProbe::new(false)),
        Duration::from_millis(100),
    )
    .await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();
    let first = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(first.outcome, WechatSnapshotOutcome::Completed);
    let parent = snapshot_parent(&h.client).await.unwrap();

    // 启动后的第一次检查会做一次云端维护；之后本地未变时应完全静默。
    h.engine.start().await.unwrap();
    wait_for_db_status(&h.engine, BackupFolderStatus::IdleOk).await;
    let baseline = wait_for_request_quiescence(&h.state).await;

    for _ in 0..5 {
        h.clock.advance(Duration::from_millis(300));
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            h.state.requests(),
            baseline,
            "数据库未变时连续检查不得产生任何云端请求"
        );
    }

    // 本地变化后才访问云端并新建快照。
    let file = primary_database_file(&documents);
    write(&file, b"changed-content-after-quiescence");
    set_mtime_forward(&file);
    h.clock.advance(Duration::from_millis(300));
    let after = wait_for_requests_above(&h.state, baseline).await;
    assert!(after > baseline);
    wait_for_complete_snapshot_count(&h.client, parent, 2).await;
    h.engine.stop().await;
}

#[tokio::test]
async fn wechat_snapshot_cleanup_runs_at_startup_and_full_scan() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_snapshot_fixture(fixture.path());
    let h = harness_full_with(
        false,
        Duration::from_millis(50),
        Duration::from_secs(60),
        Some(documents.clone()),
        Arc::new(FakeProbe::new(false)),
        Duration::from_millis(100),
    )
    .await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();
    let first = h
        .engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(first.outcome, WechatSnapshotOutcome::Completed);
    let parent = snapshot_parent(&h.client).await.unwrap();

    // 启动后第一次检查：即使本地未变，也要清理不完整快照。
    let startup_incomplete = h
        .client
        .create_folder(parent, "2020-01-01T000000Z")
        .await
        .unwrap();
    let before_start = h.state.requests();
    h.engine.start().await.unwrap();
    wait_until_trashed(&h.state, startup_incomplete.id).await;
    assert!(
        h.state.requests() > before_start,
        "启动后的第一次检查必须列云端目录做清理"
    );

    // 每 6 小时（测试用 1 小时全量核对周期）再清理一次，即使本地未变。
    let full_scan_incomplete = h
        .client
        .create_folder(parent, "2020-01-02T000000Z")
        .await
        .unwrap();
    let before_full = h.state.requests();
    h.clock.advance(Duration::from_secs(3_601));
    wait_until_trashed(&h.state, full_scan_incomplete.id).await;
    assert!(
        h.state.requests() > before_full,
        "全量核对必须再次列云端目录做清理"
    );
    h.engine.stop().await;
}

#[tokio::test]
async fn wechat_snapshot_network_error_waits_for_retry_interval() {
    let fixture = tempfile::tempdir().unwrap();
    let documents = create_snapshot_fixture(fixture.path());
    let h = harness_full_with(
        false,
        Duration::from_millis(50),
        Duration::from_secs(10),
        Some(documents.clone()),
        Arc::new(FakeProbe::new(false)),
        Duration::from_millis(50),
    )
    .await;
    let account = discover_wechat_account(&documents).await;
    h.engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();
    // 第一次上传会话创建返回 503：网络/服务端类错误。
    *h.state.fail_upload_session_remaining.lock().unwrap() = 1;

    h.engine.start().await.unwrap();
    wait_for_db_pause_reason(&h.engine, BackupPauseReason::Network).await;
    let baseline = h.state.requests();

    // 未到 retry_interval 前，调度器继续做本地检查但不得重试云端上传。
    h.clock.advance(Duration::from_secs(1));
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        h.state.requests(),
        baseline,
        "网络错误在 retry_interval 之前不得再次尝试"
    );

    // 超过 retry_interval 后自动重试并成功。
    h.clock.advance(Duration::from_secs(12));
    let after = wait_for_requests_above(&h.state, baseline).await;
    assert!(after > baseline);
    wait_for_db_status(&h.engine, BackupFolderStatus::IdleOk).await;
    h.engine.stop().await;
}
