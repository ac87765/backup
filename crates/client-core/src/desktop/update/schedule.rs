//! 自动检查更新的调度辅助（D5）：启动后 10 秒，之后每 24 小时。

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::Clock;

/// 启动后首次检查的延迟。
pub const STARTUP_DELAY: Duration = Duration::from_secs(10);

/// 自动检查间隔。
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// 可注入时钟的调度器；手动检查后调用 [`Self::mark_checked`] 重置 24 小时窗口。
pub struct UpdateScheduler {
    clock: Arc<dyn Clock>,
    last_check: Option<SystemTime>,
}

impl UpdateScheduler {
    /// 创建调度器（尚未检查过）。
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            last_check: None,
        }
    }

    /// 下一次自动检查前需要等待的时长。
    ///
    /// 启动后第一次为 [`STARTUP_DELAY`]；之后为 24 小时减去距上次检查已经过去
    /// 的时间（至少为 0，手动检查后立即重算）。
    #[must_use]
    pub fn next_delay(&self) -> Duration {
        let Some(last_check) = self.last_check else {
            return STARTUP_DELAY;
        };
        let elapsed = self
            .clock
            .now()
            .duration_since(last_check)
            .unwrap_or(Duration::ZERO);
        CHECK_INTERVAL.saturating_sub(elapsed)
    }

    /// 记录一次检查（自动或手动）。
    pub fn mark_checked(&mut self) {
        self.last_check = Some(self.clock.now());
    }

    /// 等待下一次自动检查并记录时间；返回后调用方执行检查。
    pub async fn wait_for_next(&mut self) {
        let delay = self.next_delay();
        self.clock.sleep(delay).await;
        self.mark_checked();
    }
}
