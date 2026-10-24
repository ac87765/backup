//! 健康检查与指标集成测试（tasks 4.4）。
//!
//! 覆盖 service-operations spec 的「数据库不可用」场景，以及每项检查的超时上限；
//! 指标部分覆盖本组要求的 HTTP 请求指标与连接池指标（后台任务指标留给第 5 组）。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use server::api::build_router;
use server::{AppState, Database, Storage};
use tokio::io::AsyncReadExt;
use tower::ServiceExt;

use common::{
    TestDb, environment, integration_config, integration_state, migrated_database, static_fixture,
    test_config, test_state,
};

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
async fn healthz_and_readyz_report_ok_when_dependencies_are_available() {
    let env = environment().await;
    let (_db, database) = migrated_database(env).await;
    let (_dir, root) = static_fixture();
    let app = build_router(integration_state(env, database, root));

    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(body_json(health).await["status"], "ok");

    let ready = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(ready.status(), StatusCode::OK);
    let body = body_json(ready).await;
    assert_eq!(body["status"], "ready");
    assert_eq!(body["checks"]["database"], "ok");
    assert_eq!(body["checks"]["migrations"], "ok");
    assert_eq!(body["checks"]["storage"], "ok");
}

#[tokio::test]
async fn readyz_returns_503_while_healthz_stays_ok_when_database_is_down() {
    let env = environment().await;
    let (_dir, root) = static_fixture();
    let mut config = integration_config(env, root);
    // 关闭的端口：连接会立刻失败（不依赖 Docker）。
    config.database.url =
        server::config::Secret::new("postgres://backup:test@127.0.0.1:1/backup_test");
    let database = Database::connect_lazy(&config.database).expect("延迟连接池");
    let storage = Storage::new(&config.storage).expect("构造 storage");
    let app = build_router(AppState::new(config, database, storage));

    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(health.status(), StatusCode::OK, "存活检查不依赖数据库");

    let ready = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(ready).await;
    assert_eq!(body["status"], "not_ready");
    assert_ne!(body["checks"]["database"], "ok");
    assert_ne!(body["checks"]["migrations"], "ok");
}

#[tokio::test]
async fn readyz_returns_503_when_migrations_are_behind() {
    let env = environment().await;
    let test_db = TestDb::create(env).await;
    let pool = test_db.connect().await;
    let database = Database::from_pool(pool);
    let (_dir, root) = static_fixture();
    let app = build_router(integration_state(env, database, root));

    let ready = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(ready).await;
    assert_eq!(body["checks"]["database"], "ok");
    assert_eq!(body["checks"]["migrations"], "error");
    assert_eq!(body["checks"]["storage"], "ok");
}

#[tokio::test]
async fn readyz_storage_check_is_bounded_by_its_own_timeout() {
    let env = environment().await;
    let (_db, database) = migrated_database(env).await;
    let (_dir, root) = static_fixture();

    // 接受连接但永不响应的 TCP 服务：模拟对象存储挂起。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定挂起服务");
    let address = listener.local_addr().expect("本地地址");
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buffer = [0_u8; 1024];
                while socket.read(&mut buffer).await.unwrap_or(0) > 0 {}
            });
        }
    });

    let mut config = integration_config(env, root);
    config.storage.internal_endpoint = format!("http://{address}");
    config.storage.public_endpoint = format!("http://{address}");
    let storage = Storage::new(&config.storage).expect("构造 storage");
    let app = build_router(AppState::new(config, database, storage));

    let started = Instant::now();
    let ready = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    let elapsed = started.elapsed();

    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        elapsed < Duration::from_secs(5),
        "readyz 必须被单项超时限制在约 2 秒内，实际 {elapsed:?}"
    );
    let body = body_json(ready).await;
    assert_eq!(body["checks"]["storage"], "timeout");
    assert_eq!(body["checks"]["database"], "ok");
    assert_eq!(body["checks"]["migrations"], "ok");
}

#[tokio::test]
async fn prometheus_metrics_expose_http_and_pool_metrics() {
    let env = environment().await;
    let (_db, database) = migrated_database(env).await;
    let (_dir, root) = static_fixture();
    let handle = server::metrics::install().expect("安装指标记录器");

    let app = build_router(integration_state(env, database.clone(), root));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .expect("构造请求"),
        )
        .await
        .expect("请求应当完成");
    assert_eq!(response.status(), StatusCode::OK);

    server::metrics::record_pool_metrics(database.pool());

    let metrics_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定指标端口");
    let metrics_address = metrics_listener.local_addr().expect("指标地址");
    let metrics_task = tokio::spawn(async move {
        axum::serve(metrics_listener, server::metrics::router(handle)).await
    });

    let scrape = reqwest::get(format!("http://{metrics_address}/metrics"))
        .await
        .expect("抓取指标")
        .text()
        .await
        .expect("读取指标");
    metrics_task.abort();

    let request_line = scrape
        .lines()
        .find(|line| line.starts_with("http_requests_total"))
        .expect("必须导出 HTTP 请求指标");
    assert!(request_line.contains("method=\"GET\""), "{request_line}");
    assert!(
        request_line.contains("route=\"/healthz\""),
        "{request_line}"
    );
    assert!(request_line.contains("status=\"200\""), "{request_line}");
    assert!(
        scrape.contains("http_request_duration_seconds_bucket"),
        "必须导出请求延迟直方图"
    );
    let pool_line = scrape
        .lines()
        .find(|line| line.starts_with("db_pool_connections{state=\"idle\"}"))
        .expect("必须导出连接池空闲连接指标");
    assert!(pool_line.contains("db_pool_connections"), "{pool_line}");
    assert!(
        scrape.contains("db_pool_connections_max"),
        "必须导出连接池上限指标"
    );
}

#[tokio::test]
async fn metrics_default_address_binds_loopback_only() {
    // 配置默认值即“只绑定回环地址”；显式断言避免回归到 0.0.0.0。
    let (_dir, root) = static_fixture();
    let config = test_config(root);
    assert!(
        config.metrics_addr.ip().is_loopback(),
        "默认指标监听地址必须是回环地址，实际 {}",
        config.metrics_addr
    );
    let _ = test_state(config);
}
