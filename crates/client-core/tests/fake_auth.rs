//! 认证流程的本地假服务器测试（不需要 Docker）。
//!
//! 真实服务端集成测试在 `tests/auth.rs`；本文件让本机也能验证：
//! Cookie 捕获与回传、CSRF 头、登录 → `/auth/me`、解锁、退出、会话失效清理。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use client_core::{AccountStatus, Client, ClientError, MemorySecretStore, SecretStore};
use crypto_core::{KdfParams, MasterKey};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";

struct FakeState {
    email: String,
    user_id: Uuid,
    root_node_id: Uuid,
    salt: [u8; 16],
    wrapped_mk: Vec<u8>,
    expected_auth_key: String,
    token: String,
    session_valid: AtomicBool,
    auth_key_ok: AtomicBool,
    prelogin_calls: AtomicUsize,
    me_calls: AtomicUsize,
    login_calls: AtomicUsize,
    logout_calls: AtomicUsize,
    cookies: Mutex<Vec<Option<String>>>,
    csrf_headers: Mutex<Vec<Option<String>>>,
}

impl FakeState {
    fn new() -> Arc<Self> {
        let email = format!("fake-{}@example.com", Uuid::new_v4());
        let salt = [0x5a_u8; 16];
        let params = KdfParams::RECOMMENDED;
        let keys = crypto_core::derive_keys_from_password(PASSWORD, &salt, &params)
            .expect("派生假服务器测试密钥");
        let master_key = MasterKey::generate().expect("生成假服务器主密钥");
        let wrapped_mk = crypto_core::wrap_mk_with_password(&keys.pwd_wrap, &master_key)
            .expect("包裹假服务器主密钥");
        Arc::new(Self {
            email,
            user_id: Uuid::new_v4(),
            root_node_id: Uuid::new_v4(),
            salt,
            wrapped_mk,
            expected_auth_key: encode_hex(keys.auth_key.expose_secret()),
            token: "a".repeat(64),
            session_valid: AtomicBool::new(false),
            auth_key_ok: AtomicBool::new(false),
            prelogin_calls: AtomicUsize::new(0),
            me_calls: AtomicUsize::new(0),
            login_calls: AtomicUsize::new(0),
            logout_calls: AtomicUsize::new(0),
            cookies: Mutex::new(Vec::new()),
            csrf_headers: Mutex::new(Vec::new()),
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

fn record_headers(state: &FakeState, headers: &HeaderMap) {
    state.cookies.lock().unwrap().push(
        headers
            .get(header::COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    );
    state.csrf_headers.lock().unwrap().push(
        headers
            .get("x-requested-with")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    );
}

fn cookie_is_valid(state: &FakeState, headers: &HeaderMap) -> bool {
    if !state.session_valid.load(Ordering::SeqCst) {
        return false;
    }
    let expected = format!("__Host-session={}", state.token);
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "code": "unauthenticated",
            "message": "需要登录",
            "request_id": "fake-auth",
        })),
    )
        .into_response()
}

async fn prelogin(State(state): State<Arc<FakeState>>, headers: HeaderMap) -> Response {
    state.prelogin_calls.fetch_add(1, Ordering::SeqCst);
    record_headers(&state, &headers);
    Json(json!({
        "kdf_params": KdfParams::RECOMMENDED,
        "kdf_salt": encode_hex(&state.salt),
    }))
    .into_response()
}

async fn login(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    state.login_calls.fetch_add(1, Ordering::SeqCst);
    record_headers(&state, &headers);
    let presented = body
        .get("auth_key")
        .and_then(Value::as_str)
        .unwrap_or_default();
    state
        .auth_key_ok
        .store(presented == state.expected_auth_key, Ordering::SeqCst);
    state.session_valid.store(true, Ordering::SeqCst);

    let mut response = Json(state.profile()).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "__Host-session={}; Path=/; HttpOnly; Secure; SameSite=Strict",
            state.token
        ))
        .expect("构造 Set-Cookie"),
    );
    response
}

async fn me(State(state): State<Arc<FakeState>>, headers: HeaderMap) -> Response {
    state.me_calls.fetch_add(1, Ordering::SeqCst);
    record_headers(&state, &headers);
    if !cookie_is_valid(&state, &headers) {
        return unauthorized();
    }
    Json(state.profile()).into_response()
}

async fn logout(State(state): State<Arc<FakeState>>, headers: HeaderMap) -> Response {
    state.logout_calls.fetch_add(1, Ordering::SeqCst);
    record_headers(&state, &headers);
    if !cookie_is_valid(&state, &headers) {
        return unauthorized();
    }
    state.session_valid.store(false, Ordering::SeqCst);
    Json(json!({ "ok": true })).into_response()
}

async fn spawn_server(state: Arc<FakeState>) -> String {
    let app = Router::new()
        .route("/api/v1/auth/prelogin", post(prelogin))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/me", get(me))
        .route("/api/v1/auth/logout", post(logout))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}")
}

fn client(base_url: String, store: Arc<MemorySecretStore>) -> Client {
    Client::new(base_url, store).unwrap()
}

#[tokio::test]
async fn login_me_and_logout_use_the_managed_cookie() {
    let state = FakeState::new();
    let base_url = spawn_server(state.clone()).await;
    let store = Arc::new(MemorySecretStore::new());
    let client = client(base_url, store.clone());

    let account = client.login(&state.email, PASSWORD).await.unwrap();
    assert_eq!(account.email, state.email);
    assert_eq!(account.status, AccountStatus::Active);
    assert!(
        state.auth_key_ok.load(Ordering::SeqCst),
        "auth_key 派生不一致"
    );
    assert!(store.load_session().unwrap().is_some());
    assert!(store.load_master_key().unwrap().is_none());

    let refreshed = client.current_account().await.unwrap();
    assert_eq!(refreshed.user_id, state.user_id);
    assert_eq!(state.me_calls.load(Ordering::SeqCst), 2);

    client.logout().await.unwrap();
    assert_eq!(state.logout_calls.load(Ordering::SeqCst), 1);
    assert!(!state.session_valid.load(Ordering::SeqCst));
    assert!(store.load_session().unwrap().is_none());
    assert!(store.load_master_key().unwrap().is_none());
    assert!(!client.is_unlocked());

    // 所有请求都带与 Web 相同的 CSRF 头。
    let csrf = state.csrf_headers.lock().unwrap();
    assert!(
        csrf.iter()
            .all(|value| value.as_deref() == Some("backup-web"))
    );
}

#[tokio::test]
async fn session_invalid_clears_store_and_memory() {
    let state = FakeState::new();
    let base_url = spawn_server(state.clone()).await;
    let store = Arc::new(MemorySecretStore::new());
    let client = Client::builder(base_url)
        .secret_store(store.clone())
        .keep_unlocked(true)
        .build()
        .unwrap();

    client.login(&state.email, PASSWORD).await.unwrap();
    assert!(store.load_session().unwrap().is_some());
    assert!(store.load_master_key().unwrap().is_some());

    state.session_valid.store(false, Ordering::SeqCst);
    let error = client.current_account().await.unwrap_err();
    assert!(error.is_unauthenticated());
    assert!(store.load_session().unwrap().is_none());
    assert!(store.load_master_key().unwrap().is_none());
    assert!(!client.is_unlocked());
}

#[tokio::test]
async fn reopen_unlocks_with_password_only_and_wrong_password_keeps_session() {
    let state = FakeState::new();
    let base_url = spawn_server(state.clone()).await;
    let store = Arc::new(MemorySecretStore::new());

    let first = client(base_url.clone(), store.clone());
    first.login(&state.email, PASSWORD).await.unwrap();
    drop(first);

    let reopened = client(base_url.clone(), store.clone());
    let restored = reopened.restore_session().await.unwrap().unwrap();
    assert_eq!(restored.email, state.email);
    assert!(!reopened.is_unlocked());

    let unlocked = reopened.unlock(PASSWORD).await.unwrap();
    assert_eq!(unlocked.user_id, state.user_id);
    assert!(reopened.is_unlocked());

    let reopened_again = client(base_url, store.clone());
    let error = reopened_again.unlock("wrong password").await.unwrap_err();
    assert!(matches!(error, ClientError::UnlockFailed));
    assert!(!reopened_again.is_unlocked());
    assert!(store.load_session().unwrap().is_some());
}
