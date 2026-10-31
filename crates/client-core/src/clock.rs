//! 可注入的时钟：429 退避等待与抖动。
//!
//! 生产环境使用 [`SystemClock`]（Tokio 定时器 + CSPRNG 抖动）；测试注入
//! 立即返回的假时钟即可断言等待时长，而不必真的睡满 `Retry-After`。

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

/// 退避等待的 future 类型（对象安全，便于注入 `Arc<dyn Clock>`）。
pub type SleepFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// 退避时钟。
pub trait Clock: Send + Sync {
    /// 等待指定时长。
    fn sleep(&self, duration: Duration) -> SleepFuture<'_>;

    /// 0–1 的随机抖动系数；默认使用操作系统 CSPRNG。
    fn jitter(&self) -> f64 {
        random_unit()
    }
}

/// 生产时钟：Tokio 定时器 + 操作系统 CSPRNG。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn sleep(&self, duration: Duration) -> SleepFuture<'_> {
        Box::pin(tokio::time::sleep(duration))
    }
}

fn random_unit() -> f64 {
    let mut bytes = [0_u8; 8];
    if getrandom::getrandom(&mut bytes).is_err() {
        // 抖动只影响等待时长，不是安全边界；取不到随机数时退化为无抖动。
        return 0.0;
    }
    let value = u64::from_le_bytes(bytes);
    value as f64 / (u64::MAX as f64 + 1.0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn jitter_stays_in_unit_interval() {
        for _ in 0..64 {
            let value = SystemClock.jitter();
            assert!((0.0..1.0).contains(&value), "抖动必须在 [0,1)：{value}");
        }
    }
}
