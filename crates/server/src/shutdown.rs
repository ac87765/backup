//! 优雅停机。
//!
//! - Unix：监听 SIGTERM 与 Ctrl+C；Windows：只监听 Ctrl+C（`cfg` 区分）。
//! - 触发停机后停止接受新连接，并等待进行中的请求与后台任务；
//!   **两者共用一个总期限**（默认 [`SHUTDOWN_GRACE`]，30 秒），不是各自等 30 秒。
//! - 通过 [`TaskTracker`] 跟踪后台任务；停机后不再领取新任务（第 5 组接入后生效）。

use std::future::{Future, IntoFuture};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use axum::Router;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// 优雅停机的总期限。
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(30);

struct Inner {
    token: CancellationToken,
    tracker: TaskTracker,
    grace: Duration,
    cancelled_at: OnceLock<Instant>,
}

/// 停机协调器：取消令牌 + 后台任务跟踪 + 统一截止时间。
#[derive(Clone)]
pub struct Shutdown {
    inner: Arc<Inner>,
}

impl Shutdown {
    pub fn new() -> Self {
        Self::with_grace(SHUTDOWN_GRACE)
    }

    /// 使用自定义总期限（测试用；生产使用 [`SHUTDOWN_GRACE`]）。
    pub fn with_grace(grace: Duration) -> Self {
        Self {
            inner: Arc::new(Inner {
                token: CancellationToken::new(),
                tracker: TaskTracker::new(),
                grace,
                cancelled_at: OnceLock::new(),
            }),
        }
    }

    pub fn token(&self) -> CancellationToken {
        self.inner.token.clone()
    }

    /// 配置的总停机期限。
    pub fn grace(&self) -> Duration {
        self.inner.grace
    }

    /// 触发停机（信号处理或测试调用），并记录截止时间的起点。
    pub fn cancel(&self) {
        let _ = self.inner.cancelled_at.set(Instant::now());
        self.inner.token.cancel();
    }

    /// 距离总截止时间还剩多久；尚未触发停机时返回完整期限。
    pub fn remaining_grace(&self) -> Duration {
        if self.inner.cancelled_at.get().is_none() && self.inner.token.is_cancelled() {
            // 有人直接取消了 token（例如测试）；从第一次观察到取消时开始计时。
            let _ = self.inner.cancelled_at.set(Instant::now());
        }
        match self.inner.cancelled_at.get() {
            Some(cancelled_at) => self.inner.grace.saturating_sub(cancelled_at.elapsed()),
            None => self.inner.grace,
        }
    }

    /// 启动一个受跟踪的后台任务。
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.inner.tracker.spawn(future)
    }

    /// 关闭任务跟踪并在给定预算内等待所有后台任务结束。
    ///
    /// 返回 `false` 表示超过预算仍未结束（调用方应记录告警后退出）。
    pub async fn wait_background(&self, grace: Duration) -> bool {
        self.inner.tracker.close();
        tokio::time::timeout(
            grace.max(Duration::from_millis(1)),
            self.inner.tracker.wait(),
        )
        .await
        .is_ok()
    }

    /// 使用统一截止时间剩余的部分等待后台任务结束。
    ///
    /// 必须在 HTTP 服务完成在途请求之后调用；它与在途请求共享同一个总期限。
    pub async fn wait_background_remaining(&self) -> bool {
        self.wait_background(self.remaining_grace()).await
    }
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

/// 监听停机信号，收到后调用 [`Shutdown::cancel`]（记录统一截止时间）。
pub async fn wait_for_shutdown_signal(shutdown: Shutdown) {
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

    shutdown.cancel();
}

/// 运行 HTTP 服务，直到收到停机信号并完成在途请求（受统一总期限限制）。
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
            let remaining = shutdown.remaining_grace();
            match tokio::time::timeout(remaining.max(Duration::from_millis(1)), &mut server).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!(
                        grace_secs = shutdown.grace().as_secs(),
                        "优雅停机超过总期限，强制退出"
                    );
                    Ok(())
                }
            }
        }
    }
}
