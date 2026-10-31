//! 429 退避测试：本地假服务器 + 注入时钟，不需要 Docker。
//!
//! 覆盖：
//! - 有 `Retry-After` 时按该秒数等待；
//! - 缺失 `Retry-After` 时默认 5 秒；
//! - 最多重试 8 次（共 9 次请求）；
//! - 每次请求都带 User-Agent 与 CSRF 头。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use client_core::{Client, ClientError, Clock, MemorySecretStore, SleepFuture};
use serde_json::json;
use tokio::net::TcpListener;

#[derive(Clone, Copy)]
enum Mode {
    /// 前两次 429（Retry-After: 3），随后返回被降级的 KDF 参数。
    RetryThenDowngraded,
    /// 永远 429（Retry-After: 3）。
    AlwaysRateLimited,
    /// 第一次 429 且不带 Retry-After，随后返回被降级的 KDF 参数。
    MissingRetryAfter,
}

#[derive(Clone)]
struct ServerState {
    mode: Mode,
    prelogin_calls: Arc<AtomicUsize>,
    login_calls: Arc<AtomicUsize>,
    user_agents: Arc<Mutex<Vec<Option<String>>>>,
    csrf_headers: Arc<Mutex<Vec<Option<String>>>>,
}

impl ServerState {
    fn new(mode: Mode) -> Self {
        Self {
            mode,
            prelogin_calls: Arc::new(AtomicUsize::new(0)),
            login_calls: Arc::new(AtomicUsize::new(0)),
            user_agents: Arc::new(Mutex::new(Vec::new())),
            csrf_headers: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[derive(Default)]
struct FakeClock {
    sleeps: Mutex<Vec<Duration>>,
}

impl Clock for FakeClock {
    fn sleep(&self, duration: Duration) -> SleepFuture<'_> {
        self.sleeps.lock().unwrap().push(duration);
        Box::pin(async {})
    }

    fn jitter(&self) -> f64 {
        0.0
    }
}

fn rate_limited(retry_after: Option<u64>) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({
            "code": "rate_limited",
            "message": "请求过于频繁，请稍后重试",
            "request_id": "retry-test",
        })),
    )
        .into_response();
    if let Some(seconds) = retry_after {
        response.headers_mut().insert(
            header::RETRY_AFTER,
            HeaderValue::from_str(&seconds.to_string()).unwrap(),
        );
    }
    response
}

fn downgraded_prelogin() -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "kdf_params": {
                "algorithm": "argon2id",
                "m_cost_kib": 8192,
                "t_cost": 1,
                "p_cost": 1,
            },
            "kdf_salt": "000102030405060708090a0b0c0d0e0f",
        })),
    )
        .into_response()
}

async fn prelogin(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    state.prelogin_calls.fetch_add(1, Ordering::SeqCst);
    state.user_agents.lock().unwrap().push(
        headers
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    );
    state.csrf_headers.lock().unwrap().push(
        headers
            .get("x-requested-with")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    );

    let call = state.prelogin_calls.load(Ordering::SeqCst);
    match state.mode {
        Mode::RetryThenDowngraded if call <= 2 => rate_limited(Some(3)),
        Mode::AlwaysRateLimited => rate_limited(Some(3)),
        Mode::MissingRetryAfter if call == 1 => rate_limited(None),
        _ => downgraded_prelogin(),
    }
}

async fn login(State(state): State<ServerState>) -> Response {
    state.login_calls.fetch_add(1, Ordering::SeqCst);
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "code": "invalid_request",
            "message": "不应在 KDF 校验失败后发送登录请求",
            "request_id": "retry-test",
        })),
    )
        .into_response()
}

async fn spawn_server(state: ServerState) -> String {
    let app = Router::new()
        .route("/api/v1/auth/prelogin", post(prelogin))
        .route("/api/v1/auth/login", post(login))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}")
}

fn client(base_url: String, clock: Arc<FakeClock>) -> Client {
    Client::builder(base_url)
        .secret_store(Arc::new(MemorySecretStore::new()))
        .clock(clock)
        .build()
        .unwrap()
}

#[tokio::test]
async fn retries_429_with_retry_after_before_reporting_bad_kdf_params() {
    let state = ServerState::new(Mode::RetryThenDowngraded);
    let base_url = spawn_server(state.clone()).await;
    let clock = Arc::new(FakeClock::default());

    let error = client(base_url, clock.clone())
        .login("user@example.com", "correct horse battery staple")
        .await
        .unwrap_err();

    assert!(
        matches!(error, ClientError::InvalidKdfParams),
        "降级参数必须在发送认证密钥前被拒绝，实际：{error:?}"
    );
    assert_eq!(state.prelogin_calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        state.login_calls.load(Ordering::SeqCst),
        0,
        "参数校验失败后不得发送 login"
    );
    assert_eq!(
        *clock.sleeps.lock().unwrap(),
        vec![Duration::from_secs(3), Duration::from_secs(3)]
    );

    let user_agents = state.user_agents.lock().unwrap();
    assert!(!user_agents.is_empty());
    for value in user_agents.iter() {
        let value = value.as_deref().expect("必须带 User-Agent");
        assert!(
            value.starts_with("EncryptedBackup/"),
            "User-Agent 前缀不对：{value}"
        );
        assert!(
            value.ends_with("; desktop)"),
            "User-Agent 后缀不对：{value}"
        );
    }
    let csrf = state.csrf_headers.lock().unwrap();
    assert!(
        csrf.iter()
            .all(|value| value.as_deref() == Some("backup-web"))
    );
}

#[tokio::test]
async fn defaults_to_five_seconds_when_retry_after_is_missing() {
    let state = ServerState::new(Mode::MissingRetryAfter);
    let base_url = spawn_server(state.clone()).await;
    let clock = Arc::new(FakeClock::default());

    let error = client(base_url, clock.clone())
        .login("user@example.com", "correct horse battery staple")
        .await
        .unwrap_err();

    assert!(matches!(error, ClientError::InvalidKdfParams));
    assert_eq!(state.prelogin_calls.load(Ordering::SeqCst), 2);
    assert_eq!(*clock.sleeps.lock().unwrap(), vec![Duration::from_secs(5)]);
}

#[tokio::test]
async fn stops_after_eight_rate_limit_retries() {
    let state = ServerState::new(Mode::AlwaysRateLimited);
    let base_url = spawn_server(state.clone()).await;
    let clock = Arc::new(FakeClock::default());

    let error = client(base_url, clock.clone())
        .login("user@example.com", "correct horse battery staple")
        .await
        .unwrap_err();

    assert!(
        error.is_rate_limited(),
        "8 次重试后应返回 rate_limited，实际：{error:?}"
    );
    assert_eq!(state.prelogin_calls.load(Ordering::SeqCst), 9);
    assert_eq!(clock.sleeps.lock().unwrap().len(), 8);
}
