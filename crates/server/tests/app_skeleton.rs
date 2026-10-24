//! 应用骨架集成测试（tasks 4.2）：
//! 请求 ID、统一错误、413、安全响应头/CSP、静态资源缓存、日志脱敏。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::{Arc, Mutex, OnceLock};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use http_body_util::BodyExt;
use server::AppState;
use server::api::build_router_with;
use server::api::error::ApiError;
use tower::ServiceExt;
use tracing_subscriber::fmt::MakeWriter;

use common::{static_fixture, test_config, test_state};

struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("日志缓冲锁").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Buffer {
    type Writer = Buffer;

    fn make_writer(&'a self) -> Self::Writer {
        Buffer(self.0.clone())
    }
}

static LOGS: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();

/// 安装（一次）一个把 JSON 日志写入内存缓冲的全局订阅器。
fn captured_logs() -> Arc<Mutex<Vec<u8>>> {
    LOGS.get_or_init(|| {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::INFO)
            .with_writer(Buffer(buffer.clone()))
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        buffer
    })
    .clone()
}

fn app_with_extra(extra: Router<AppState>) -> (Router, tempfile::TempDir) {
    let (dir, root) = static_fixture();
    let router = build_router_with(test_state(test_config(root)), extra);
    (router, dir)
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("读取响应体")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("响应体必须是 JSON")
}

#[tokio::test]
async fn request_id_is_generated_by_server_and_returned_everywhere() {
    let (app, _dir) = app_with_extra(Router::new());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/does-not-exist")
                // 客户端伪造的请求 ID 必须被忽略。
                .header("x-request-id", "client-forged-request-id")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let header_id = response
        .headers()
        .get("x-request-id")
        .expect("响应必须带 X-Request-Id")
        .to_str()
        .expect("请求 ID 必须是 ASCII")
        .to_owned();
    assert_ne!(header_id, "client-forged-request-id");
    assert!(
        uuid::Uuid::parse_str(&header_id).is_ok(),
        "请求 ID 必须是服务端生成的 UUID，实际: {header_id}"
    );

    let body = body_json(response).await;
    assert_eq!(body["code"], "not_found");
    assert_eq!(body["request_id"], header_id);
}

#[tokio::test]
async fn unknown_api_route_returns_json_404_not_spa() {
    let (app, _dir) = app_with_extra(Router::new());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/nope")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .is_some_and(|value| value
                .to_str()
                .unwrap_or_default()
                .starts_with("application/json"))
    );
    let body = body_json(response).await;
    assert_eq!(body["code"], "not_found");
}

#[tokio::test]
async fn non_api_paths_fall_back_to_index_html() {
    let (app, _dir) = app_with_extra(Router::new());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/files/some/deep/link")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-cache"),
        "SPA 回退的 index.html 必须 no-cache"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("读取响应体")
        .to_bytes();
    assert!(String::from_utf8_lossy(&bytes).contains("backup-spa"));
}

#[tokio::test]
async fn missing_asset_returns_404_instead_of_spa_index() {
    let (app, _dir) = app_with_extra(Router::new());

    for uri in [
        "/assets/missing-DiwrgTda.js",
        "/assets/nested/missing-file.wasm",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("构造请求"),
            )
            .await
            .expect("请求应当完成");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "/assets 下缺失文件必须 404: {uri}"
        );
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("读取响应体")
            .to_bytes();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("backup-spa"),
            "/assets 缺失文件不得回退到 index.html: {uri}"
        );
    }
}

#[tokio::test]
async fn oversized_body_is_rejected_without_being_parsed() {
    let (app, _dir) = app_with_extra(Router::new());
    let ten_mib = 10 * 1024 * 1024;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/uploads")
                .header(header::CONTENT_LENGTH, ten_mib.to_string())
                .body(Body::from(vec![b'a'; ten_mib]))
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let request_id = response
        .headers()
        .get("x-request-id")
        .expect("413 也必须带请求 ID")
        .to_str()
        .expect("请求 ID 为 ASCII")
        .to_owned();
    let body = body_json(response).await;
    assert_eq!(body["code"], "payload_too_large");
    assert_eq!(body["request_id"], request_id);
}

#[tokio::test]
async fn internal_errors_do_not_leak_details() {
    async fn boom() -> Result<&'static str, ApiError> {
        Err(ApiError::internal(
            "database error: SELECT password FROM users WHERE email = 'leaked@example.com'",
        ))
    }

    let (app, _dir) = app_with_extra(Router::<AppState>::new().route("/test/boom", get(boom)));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/test/boom")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_json(response).await;
    assert_eq!(body["code"], "internal_error");
    let serialized = body.to_string();
    assert!(!serialized.contains("SELECT"));
    assert!(!serialized.contains("leaked@example.com"));
    assert!(!serialized.contains("database error"));
    assert!(serialized.contains("服务器内部错误"));
}

#[tokio::test]
async fn security_headers_are_present_on_pages_and_api_errors() {
    let (app, _dir) = app_with_extra(Router::new());

    for uri in ["/", "/api/v1/missing"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("构造请求"),
            )
            .await
            .expect("请求应当完成");
        let headers = response.headers();
        let csp = headers
            .get("content-security-policy")
            .expect("必须设置 CSP")
            .to_str()
            .expect("CSP 为 ASCII");
        assert!(csp.contains("default-src 'self'"), "实际 CSP: {csp}");
        assert!(
            csp.contains("connect-src 'self' http://127.0.0.1:9000"),
            "CSP 必须追加对象存储公网 origin，实际: {csp}"
        );
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(!csp.contains("unsafe-inline"));
        assert_eq!(
            headers
                .get("x-content-type-options")
                .and_then(|value| value.to_str().ok()),
            Some("nosniff")
        );
        assert_eq!(
            headers
                .get("referrer-policy")
                .and_then(|value| value.to_str().ok()),
            Some("no-referrer")
        );
        assert!(
            headers
                .get("strict-transport-security")
                .is_some_and(|value| value.to_str().unwrap_or_default().contains("max-age=")),
            "必须设置 HSTS"
        );
    }
}

#[tokio::test]
async fn static_assets_use_long_cache_while_html_is_no_cache() {
    let (app, _dir) = app_with_extra(Router::new());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/index-DiwrgTda.js")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("public, max-age=31536000, immutable")
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-cache")
    );

    let response = app
        .oneshot(
            Request::builder()
                .uri("/assets/app.wasm")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/wasm")
    );
}

#[tokio::test]
async fn logs_never_contain_secrets_bodies_or_presigned_urls() {
    let logs = captured_logs();
    logs.lock().expect("日志缓冲锁").clear();

    let (app, _dir) = app_with_extra(Router::new());
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                // 查询串里的预签名签名绝不能被记录。
                .uri("/api/v1/unknown?X-Amz-Signature=presign-signature-marker")
                .header("x-requested-with", "backup-web")
                .header(header::COOKIE, "__Host-session=session-token-marker")
                .header(
                    header::AUTHORIZATION,
                    "Bearer authorization-header-marker",
                )
                .header("x-request-id", "client-request-id-marker")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"secret":"request-body-marker","presigned":"https://bucket.example/obj?X-Amz-Signature=body-signature-marker"}"#,
                ))
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let text = String::from_utf8(logs.lock().expect("日志缓冲锁").clone()).expect("日志是 UTF-8");
    assert!(
        text.contains("http_request"),
        "必须记录请求日志，实际日志: {text}"
    );
    assert!(
        text.contains("\"/api/v1/unknown\""),
        "日志只记录不含查询串的 path"
    );
    for marker in [
        "session-token-marker",
        "authorization-header-marker",
        "presign-signature-marker",
        "client-request-id-marker",
        "request-body-marker",
        "body-signature-marker",
    ] {
        assert!(
            !text.contains(marker),
            "日志中不得出现敏感内容 {marker}，实际日志: {text}"
        );
    }
}
