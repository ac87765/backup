//! 对象删除队列（design D10 / file-transfer spec「存储对象删除的可靠性」）。
//!
//! 领取：短事务内 `FOR UPDATE SKIP LOCKED`，设置 5 分钟租约与新的 `claim_token`；
//! 事务提交后才会调用对象存储。完成/失败更新都必须匹配 `claim_token`，
//! 避免租约过期后旧 worker 误删或误更新新 worker 仍然拥有的行。
//!
//! 业务事务登记删除的唯一入口是 [`enqueue_object_deletions`]：它只接受
//! `&mut Transaction`，从类型上保证不会在事务外直接登记。

use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use sqlx::{PgPool, Postgres, Transaction};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::metrics;
use super::retry::{backoff_delay, classify_storage_error};
use crate::{Database, Storage};

/// 一批领取的最大行数。
pub const BATCH_SIZE: i64 = 100;
/// 领取租约：处理太慢时由其他实例重新领取。
pub const CLAIM_LEASE: Duration = Duration::from_secs(5 * 60);

/// 已领取的待删除对象。
#[derive(Debug, Clone)]
pub struct ClaimedDeletion {
    pub id: i64,
    pub object_key: String,
    pub attempts: i32,
    pub claim_token: Uuid,
}

/// 一批处理结果（测试与日志使用）。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DeletionStats {
    pub claimed: usize,
    pub deleted: usize,
    pub failed: usize,
    pub lease_lost: usize,
}

/// 在业务事务内登记待删除对象。
///
/// 只接受 `&mut Transaction`：没有连接池参数，因此不可能在业务事务之外调用。
pub async fn enqueue_object_deletions(
    tx: &mut Transaction<'_, Postgres>,
    object_keys: &[String],
) -> Result<u64, sqlx::Error> {
    if object_keys.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query!(
        "INSERT INTO object_deletions (object_key) SELECT unnest($1::text[])",
        object_keys,
    )
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected())
}

/// 领取一批到期任务（短事务，提交后才返回）。
pub async fn claim_batch(pool: &PgPool, limit: i64) -> Result<Vec<ClaimedDeletion>, sqlx::Error> {
    let claim_token = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    let claimed = sqlx::query_as!(
        ClaimedDeletion,
        r#"
        UPDATE object_deletions
        SET claimed_until = now() + make_interval(secs => $1),
            claim_token = $2
        WHERE id IN (
            SELECT id
            FROM object_deletions
            WHERE not_before <= now()
              AND (claimed_until IS NULL OR claimed_until < now())
            ORDER BY id
            FOR UPDATE SKIP LOCKED
            LIMIT $3
        )
        RETURNING id, object_key, attempts, claim_token AS "claim_token!"
        "#,
        CLAIM_LEASE.as_secs_f64(),
        claim_token,
        limit,
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(claimed)
}

/// 删除成功后移除队列行（必须匹配 claim_token）。
pub async fn complete(pool: &PgPool, id: i64, claim_token: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM object_deletions WHERE id = $1 AND claim_token = $2",
        id,
        claim_token,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 失败后按退避重试（必须匹配 claim_token）。
pub async fn fail(
    pool: &PgPool,
    id: i64,
    claim_token: Uuid,
    attempts: i32,
    error_kind: &'static str,
) -> Result<bool, sqlx::Error> {
    let delay = backoff_delay(attempts);
    let result = sqlx::query!(
        r#"
        UPDATE object_deletions
        SET attempts = attempts + 1,
            not_before = now() + make_interval(secs => $3),
            claimed_until = NULL,
            claim_token = NULL,
            last_error = $4
        WHERE id = $1 AND claim_token = $2
        "#,
        id,
        claim_token,
        delay.as_secs_f64(),
        error_kind,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// 领取并处理一批删除任务。
pub async fn run_batch(
    database: &Database,
    storage: &Arc<Storage>,
    limit: i64,
) -> Result<DeletionStats, sqlx::Error> {
    let claimed = claim_batch(database.pool(), limit).await?;
    let deletion = storage.deletion_handle();
    let mut stats = DeletionStats {
        claimed: claimed.len(),
        ..DeletionStats::default()
    };

    for item in claimed {
        match deletion.delete(&item.object_key).await {
            Ok(()) => match complete(database.pool(), item.id, item.claim_token).await {
                Ok(true) => {
                    stats.deleted += 1;
                    metrics::record_execution("object_deletions");
                }
                Ok(false) => {
                    stats.lease_lost += 1;
                    tracing::warn!(
                        outbox_id = item.id,
                        "对象删除完成但租约已被其他实例接管，跳过队列行更新"
                    );
                }
                Err(error) => return Err(error),
            },
            Err(error) => {
                let kind = classify_storage_error(&error);
                metrics::record_failure("object_deletions");
                match fail(
                    database.pool(),
                    item.id,
                    item.claim_token,
                    item.attempts + 1,
                    kind,
                )
                .await
                {
                    Ok(true) => {
                        stats.failed += 1;
                        tracing::warn!(
                            outbox_id = item.id,
                            error_kind = kind,
                            attempts = item.attempts + 1,
                            "对象删除失败，稍后重试"
                        );
                    }
                    Ok(false) => {
                        stats.lease_lost += 1;
                        tracing::warn!(
                            outbox_id = item.id,
                            "对象删除失败但租约已被其他实例接管，跳过队列行更新"
                        );
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(stats)
}

/// 删除队列 worker：收到取消信号后不再领取新批次。
pub async fn worker_loop(database: Database, storage: Arc<Storage>, shutdown: CancellationToken) {
    loop {
        if shutdown.is_cancelled() {
            break;
        }
        match run_batch(&database, &storage, BATCH_SIZE).await {
            Ok(stats) if stats.claimed > 0 => {
                tracing::debug!(
                    claimed = stats.claimed,
                    deleted = stats.deleted,
                    failed = stats.failed,
                    "对象删除批次完成"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(error = %error, "对象删除队列处理失败");
                metrics::record_failure("object_deletions");
            }
        }
        let delay = Duration::from_millis(rand::rng().random_range(1_500..=2_500));
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(delay) => {}
        }
    }
}
