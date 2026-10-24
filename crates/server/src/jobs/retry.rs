//! 重试退避与错误分类。
//!
//! 退避：`min(1 小时, 5 秒 × 2^attempts)`，再在 [上限/2, 上限] 之间做抖动。
//! `last_error` 只保存稳定分类字符串，绝不保存原始错误文本（可能带 URL/签名）。

use std::time::Duration;

use rand::Rng;

use crate::mail::MailError;
use crate::storage::StorageError;

/// 首次重试的基础退避。
const BASE_BACKOFF: Duration = Duration::from_secs(5);
/// 退避上限（spec：最长不超过 1 小时）。
pub const MAX_BACKOFF: Duration = Duration::from_secs(60 * 60);

/// 计算下一次重试的延迟。
///
/// `attempts` 是**已经失败**的次数（第一次失败传 1）。
pub fn backoff_delay(attempts: i32) -> Duration {
    let attempts = attempts.max(1) as u32;
    let exponent = attempts.min(20);
    let base_ms = BASE_BACKOFF.as_millis() as u64;
    let cap_ms = MAX_BACKOFF.as_millis() as u64;
    let raw_ms = base_ms.saturating_mul(1_u64 << exponent).min(cap_ms).max(1);
    // 全抖动区间 [raw/2, raw]，避免多个实例同时重试。
    let low = (raw_ms / 2).max(1);
    let jittered = rand::rng().random_range(low..=raw_ms);
    Duration::from_millis(jittered)
}

/// 对象存储错误 → 稳定分类。
pub fn classify_storage_error(error: &StorageError) -> &'static str {
    match error {
        StorageError::Injected(kind) => match kind {
            crate::storage::InjectedErrorKind::Unavailable => "unavailable",
            crate::storage::InjectedErrorKind::PermissionDenied => "permission_denied",
            crate::storage::InjectedErrorKind::Timeout => "timeout",
        },
        StorageError::Config(_) => "config",
        StorageError::InvalidPresignTtl => "invalid_ttl",
        StorageError::Operation(error) => match error.kind() {
            opendal::ErrorKind::PermissionDenied => "permission_denied",
            opendal::ErrorKind::RateLimited => "unavailable",
            opendal::ErrorKind::NotFound => "not_found",
            opendal::ErrorKind::Unexpected => "unexpected",
            _ if error.is_temporary() => "unavailable",
            _ => "other",
        },
    }
}

/// 邮件错误 → 稳定分类。
pub fn classify_mail_error(error: &MailError) -> &'static str {
    error.kind()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn backoff_grows_and_is_capped_at_one_hour() {
        let first = backoff_delay(1);
        let fourth = backoff_delay(4);
        let huge = backoff_delay(1000);
        assert!(first >= Duration::from_secs(5));
        assert!(fourth > first);
        assert!(huge <= MAX_BACKOFF);
        assert!(huge >= MAX_BACKOFF / 2);
    }

    #[test]
    fn backoff_jitter_stays_within_half_to_full_bound() {
        for attempts in 1..=8 {
            for _ in 0..20 {
                let delay = backoff_delay(attempts);
                let exponent = (attempts as u32).min(20);
                let raw = (BASE_BACKOFF.as_millis() as u64)
                    .saturating_mul(1_u64 << exponent)
                    .min(MAX_BACKOFF.as_millis() as u64);
                assert!(delay.as_millis() as u64 <= raw);
                assert!(delay.as_millis() as u64 >= raw / 2);
            }
        }
    }
}
