//! 优雅停机集成测试（tasks 4.6）。
//!
//! 覆盖 service-operations spec 的「滚动发布」场景：停机期间进行中的请求正常完成，
//! 且不再接受新连接。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use axum::Extension;
use axum::Router;
use axum::extract::Extension as ExtractExtension;
use axum::routing::get;
use server::AppState;
use server::api::build_router_with;
use server::shutdown::{Shutdown, serve_with_shutdown};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use common::{config_env, environment, migrated_database, static_fixture, test_config, test_state};

#[derive(Clone)]
struct SlowState {
    started: Arc<Notify>,
}

async fn slow_request(ExtractExtension(state): ExtractExtension<SlowState>) -> &'static str {
    state.started.notify_one();
    tokio::time::sleep(Duration::from_millis(600)).await;
    "slow-request-completed"
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn in_flight_request_completes_and_new_connections_are_rejected() {
    let (_dir, root) = static_fixture();
    let started = Arc::new(Notify::new());
    let extra = Router::<AppState>::new()
        .route("/test/slow", get(slow_request))
        .layer(Extension(SlowState {
            started: started.clone(),
        }));
    let app = build_router_with(test_state(test_config(root)), extra);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定测试端口");
    let address = listener.local_addr().expect("本地地址");
    let shutdown = Shutdown::new();
    let server_shutdown = shutdown.clone();
    let server =
        tokio::spawn(async move { serve_with_shutdown(listener, app, &server_shutdown).await });

    let client = tokio::spawn(async move {
        reqwest::get(format!("http://{address}/test/slow"))
            .await
            .expect("请求应当建立")
    });

    // 确认慢请求已经进入处理函数后再触发停机。
    started.notified().await;
    shutdown.cancel();

    // 停机开始后，新连接必须被拒绝（允许极短的 accept 竞态，最多轮询 5 秒）。
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match TcpStream::connect(address).await {
            Err(_) => break,
            Ok(connection) => {
                drop(connection);
                assert!(
                    Instant::now() < deadline,
                    "停机后仍然接受新连接（超过 5 秒）"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }

    // 进行中的请求必须正常完成，而不是被重置。
    let response = client.await.expect("客户端任务");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.text().await.expect("读取响应体"),
        "slow-request-completed"
    );

    let server_result = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .expect("服务应当在宽限期内退出")
        .expect("服务任务不应 panic");
    assert!(server_result.is_ok(), "服务退出应当无错误");
}

#[tokio::test]
async fn background_tasks_are_tracked_and_awaited() {
    let shutdown = Shutdown::new();
    let finished = Arc::new(AtomicBool::new(false));
    let task_finished = finished.clone();
    shutdown.spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        task_finished.store(true, Ordering::SeqCst);
    });

    assert!(
        shutdown.wait_background(Duration::from_secs(2)).await,
        "后台任务应当在宽限期内结束"
    );
    assert!(finished.load(Ordering::SeqCst));
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sigterm_terminates_the_real_binary_gracefully() {
    let env = environment().await;
    let (test_db, _database) = migrated_database(env).await;
    let (_dir, root) = static_fixture();

    // 先占用再释放一个端口，交给子进程使用。
    let probe = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("探测空闲端口");
    let address = probe.local_addr().expect("本地地址");
    drop(probe);

    let mut child = Command::new(env!("CARGO_BIN_EXE_server"))
        .arg("serve")
        .env_clear()
        .envs(config_env(env, &test_db.url, &root))
        .env("BACKUP_BIND_ADDR", address.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("启动 server 二进制");

    let ready_deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if reqwest::get(format!("http://{address}/healthz"))
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            break;
        }
        assert!(
            Instant::now() < ready_deadline,
            "服务未在 20 秒内通过 /healthz"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // 发送 SIGTERM（Unix 的停机信号）。
    let kill_status = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .expect("发送 SIGTERM");
    assert!(kill_status.success(), "kill 命令失败");

    let exit_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().expect("等待 server 退出") {
            assert!(
                status.success(),
                "SIGTERM 后应优雅退出（0），实际: {status}"
            );
            break;
        }
        assert!(
            Instant::now() < exit_deadline,
            "收到 SIGTERM 后 10 秒内未退出"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
