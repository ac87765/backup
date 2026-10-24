//! 优雅停机。
//!
//! - Unix：监听 SIGTERM 与 Ctrl+C；Windows：只监听 Ctrl+C（`cfg` 区分）。
//! - 停止接受新连接，并等待进行中的请求完成（最长 [`SHUTDOWN_GRACE`]）。
//! - 通过 [`TaskTracker`] 等待后台任务结束；停机后不再领取新任务（第 5 组接入后生效）。

use std::future::{Future, IntoFuture};
use std::time::Duration;

use axum::Router;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// 优雅停机的等待上限。
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

/// 停机协调器：取消令牌 + 后台任务跟踪。
#[derive(Clone)]
pub struct Shutdown {
    token: CancellationToken,
    tracker: TaskTracker,
}

impl Shutdown {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tracker: TaskTracker::new(),
        }
    }

    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// 触发停机（信号处理或测试调用）。
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// 启动一个受跟踪的后台任务。
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.tracker.spawn(future)
    }

    /// 关闭任务跟踪并等待所有后台任务结束。
    ///
    /// 返回 `false` 表示超过 `grace` 仍未结束（调用方应记录告警后退出）。
    pub async fn wait_background(&self, grace: Duration) -> bool {
        self.tracker.close();
        tokio::time::timeout(grace, self.tracker.wait())
            .await
            .is_ok()
    }
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

/// 监听停机信号，收到后取消 token。
pub async fn wait_for_shutdown_signal(token: CancellationToken) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = terminate.recv() => {
                        tracing::info!(signal = "SIGTERM", "收到停机信号");
                    }
                    result = tokio::signal::ctrl_c() => {
                        match result {
                            Ok(()) => tracing::info!(signal = "SIGINT", "收到停机信号"),
                            Err(error) => tracing::error!(error = %error, "无法监听 Ctrl+C"),
                        }
                    }
                }
            }
            Err(error) => {
                // 拿不到 SIGTERM 监听时仍保留 Ctrl+C，避免完全没有停机手段。
                tracing::error!(error = %error, "无法监听 SIGTERM，仅监听 Ctrl+C");
                match tokio::signal::ctrl_c().await {
                    Ok(()) => tracing::info!(signal = "SIGINT", "收到停机信号"),
                    Err(error) => tracing::error!(error = %error, "无法监听 Ctrl+C"),
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        match tokio::signal::ctrl_c().await {
            Ok(()) => tracing::info!(signal = "CTRL_C", "收到停机信号"),
            Err(error) => tracing::error!(error = %error, "无法监听 Ctrl+C"),
        }
    }

    token.cancel();
}

/// 运行 HTTP 服务，直到收到停机信号并完成在途请求（或超过 30 秒强制退出）。
pub async fn serve_with_shutdown(
    listener: TcpListener,
    router: Router,
    shutdown: &Shutdown,
) -> std::io::Result<()> {
    let token = shutdown.token();
    let server = axum::serve(listener, router)
        .with_graceful_shutdown(async move { token.cancelled().await })
        .into_future();
    tokio::pin!(server);
    let select_token = shutdown.token();

    tokio::select! {
        result = &mut server => result,
        _ = select_token.cancelled() => {
            match tokio::time::timeout(SHUTDOWN_GRACE, &mut server).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!(
                        grace_secs = SHUTDOWN_GRACE.as_secs(),
                        "优雅停机超时，强制退出"
                    );
                    Ok(())
                }
            }
        }
    }
}
