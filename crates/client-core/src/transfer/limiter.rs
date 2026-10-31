//! 后台备份上传限速（令牌桶）。
//!
//! 令牌桶只在**后台备份**的 `UploadTask` 上启用；手动上传通过
//! [`crate::Client::upload_file`] 创建的任务不带限速器，因此不会被备份设置影响。
//!
//! 桶容量取 `max(速率, 64 KiB)`：允许小文件突发，避免 1 KiB/s 时每个块都要等待
//! 数分钟；单个 4 MiB 分块会拆成多次令牌获取，长期平均速率仍等于配置值。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 桶的最小容量：允许一次小突发，避免极低速率下吞吐退化为零。
const MIN_CAPACITY_BYTES: u64 = 64 * 1024;

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

/// 按字节数限速的令牌桶。`Clone` 共享同一桶。
#[derive(Debug, Clone)]
pub(crate) struct TokenBucket {
    rate_bytes_per_second: f64,
    capacity: f64,
    bucket: Arc<Mutex<Bucket>>,
}

impl TokenBucket {
    /// 创建限速器；`bytes_per_second == 0` 表示不限制。
    pub(crate) fn new(bytes_per_second: u64) -> Option<Arc<Self>> {
        if bytes_per_second == 0 {
            return None;
        }
        let capacity = bytes_per_second.max(MIN_CAPACITY_BYTES) as f64;
        Some(Arc::new(Self {
            rate_bytes_per_second: bytes_per_second as f64,
            capacity,
            bucket: Arc::new(Mutex::new(Bucket {
                tokens: capacity,
                last: Instant::now(),
            })),
        }))
    }

    /// 等待直到可以发送 `bytes` 字节。桶锁只用于计算，等待期间不持锁。
    pub(crate) async fn acquire(&self, bytes: u64) {
        let mut remaining = bytes;
        while remaining > 0 {
            let (granted, wait) = match self.bucket.lock() {
                Ok(mut bucket) => reserve(
                    &mut bucket,
                    self.rate_bytes_per_second,
                    self.capacity,
                    remaining,
                ),
                // 锁损坏时放弃限速而不是让备份失败：限速不是数据安全边界。
                Err(_) => return,
            };
            remaining = remaining.saturating_sub(granted);
            if remaining == 0 {
                return;
            }
            match wait {
                Some(duration) if !duration.is_zero() => tokio::time::sleep(duration).await,
                // 理论上不会发生；避免忙等。
                _ => tokio::time::sleep(Duration::from_millis(1)).await,
            }
        }
    }
}

/// 在桶里取出至多 `wanted` 个令牌；返回 `(取出量, 还需等待的时长)`。
fn reserve(bucket: &mut Bucket, rate: f64, capacity: f64, wanted: u64) -> (u64, Option<Duration>) {
    let now = Instant::now();
    let elapsed = now.saturating_duration_since(bucket.last).as_secs_f64();
    bucket.last = now;
    bucket.tokens = (bucket.tokens + elapsed * rate).min(capacity);
    let wanted = wanted as f64;
    let taken = bucket.tokens.min(wanted);
    bucket.tokens -= taken;
    let missing = wanted - taken;
    let granted = taken as u64;
    if missing <= f64::EPSILON {
        (granted, None)
    } else {
        (granted, Some(Duration::from_secs_f64(missing / rate)))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn zero_rate_means_unlimited() {
        assert!(TokenBucket::new(0).is_none());
    }

    #[test]
    fn reserve_refills_at_rate_and_reports_wait() {
        let mut bucket = Bucket {
            tokens: 0.0,
            last: Instant::now() - Duration::from_secs(1),
        };
        // 1 秒后 100 B/s 的桶应累积 100 个令牌；取 100 个不需要等待。
        assert_eq!(reserve(&mut bucket, 100.0, 1000.0, 100), (100, None));
        // 桶空后再取 100 个，应等待约 1 秒。
        let (granted, wait) = reserve(&mut bucket, 100.0, 1000.0, 100);
        assert_eq!(granted, 0);
        let wait = wait.expect("需要等待");
        assert!(
            wait >= Duration::from_millis(900) && wait <= Duration::from_millis(1100),
            "等待时长异常：{wait:?}"
        );
    }

    #[test]
    fn capacity_caps_burst() {
        let mut bucket = Bucket {
            tokens: 0.0,
            last: Instant::now() - Duration::from_secs(3600),
        };
        // 一小时也只允许容量上限的突发。
        assert_eq!(reserve(&mut bucket, 10.0, 50.0, 50), (50, None));
        let (granted, _) = reserve(&mut bucket, 10.0, 50.0, 1);
        assert_eq!(granted, 0, "容量已用尽，必须等待补充");
    }
}
