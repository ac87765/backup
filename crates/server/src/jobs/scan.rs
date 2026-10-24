//! 扫描型任务框架（design D10）。
//!
//! 每个任务有固定数字 `job_id`。每批在独立事务内先
//! `pg_try_advisory_xact_lock(job_id)`，拿不到锁直接跳过；每批行数有上限。
//! 下一批时间 = interval + 随机抖动，避免多实例同时醒来。

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rand::Rng;
use sqlx::{Postgres, Transaction};
use tokio_util::sync::CancellationToken;

use super::metrics;
use crate::Database;

/// 每批最多处理的行数。
pub const BATCH_SIZE: i64 = 100;

/// 一个扫描型后台任务。
#[async_trait]
pub trait ScanJob: Send + Sync {
    fn job_id(&self) -> i32;
    fn name(&self) -> &'static str;
    fn interval(&self) -> Duration;
    /// 在调用方已持有 advisory lock 的事务中执行一批。
    async fn run_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        limit: i64,
    ) -> Result<u64, sqlx::Error>;
}

/// 单次执行结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanOutcome {
    Executed(u64),
    SkippedLocked,
}

/// 执行一次扫描任务（独立事务 + advisory lock）。
pub async fn run_once(
    database: &Database,
    job: &dyn ScanJob,
    limit: i64,
) -> Result<ScanOutcome, sqlx::Error> {
    let mut tx = database.pool().begin().await?;
    let locked = sqlx::query_scalar!("SELECT pg_try_advisory_xact_lock($1)", job.job_id() as i64,)
        .fetch_one(&mut *tx)
        .await?
        .unwrap_or(false);
    if !locked {
        tx.rollback().await?;
        return Ok(ScanOutcome::SkippedLocked);
    }

    let rows = match job.run_batch(&mut tx, limit).await {
        Ok(rows) => rows,
        Err(error) => {
            metrics::record_failure(job.name());
            tx.rollback().await?;
            return Err(error);
        }
    };
    tx.commit().await?;
    metrics::record_execution(job.name());
    Ok(ScanOutcome::Executed(rows))
}

/// 所有扫描任务的调度循环。
pub async fn run_loop(
    database: Database,
    jobs: Vec<Arc<dyn ScanJob>>,
    shutdown: CancellationToken,
) {
    if jobs.is_empty() {
        // 本组没有业务扫描任务：直接等待停机，避免空转。
        shutdown.cancelled().await;
        return;
    }

    let mut next_due: Vec<Instant> = jobs.iter().map(|_| Instant::now()).collect();
    loop {
        if shutdown.is_cancelled() {
            break;
        }

        let now = Instant::now();
        for (index, job) in jobs.iter().enumerate() {
            if next_due[index] > now {
                continue;
            }
            match run_once(&database, job.as_ref(), BATCH_SIZE).await {
                Ok(outcome) => {
                    tracing::debug!(
                        job = job.name(),
                        outcome = ?outcome,
                        "扫描任务批次完成"
                    );
                }
                Err(error) => {
                    tracing::error!(
                        job = job.name(),
                        error = %error,
                        "扫描任务失败"
                    );
                }
            }
            next_due[index] = instant_after(job.interval());
        }

        let wait = next_due
            .iter()
            .map(|due| due.saturating_duration_since(Instant::now()))
            .min()
            .unwrap_or(Duration::from_secs(1))
            .min(Duration::from_secs(1));
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(wait) => {}
        }
    }
}

fn instant_after(interval: Duration) -> Instant {
    let jitter = if interval.is_zero() {
        Duration::ZERO
    } else {
        let max_ms = (interval.as_millis() / 5).max(1) as u64;
        Duration::from_millis(rand::rng().random_range(0..=max_ms))
    };
    Instant::now() + interval + jitter
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn jitter_is_within_twenty_percent() {
        let interval = Duration::from_secs(10);
        for _ in 0..50 {
            let due = instant_after(interval);
            let remaining = due.saturating_duration_since(Instant::now());
            assert!(remaining >= interval);
            assert!(remaining <= interval + Duration::from_secs(2));
        }
    }
}
