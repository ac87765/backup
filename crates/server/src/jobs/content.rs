//! 文件管理相关的扫描任务（task 7.6）。
//!
//! - 版本保留清理：每个文件保留最近 10 个版本；超出的版本只有早于 30 天才
//!   会被清理，当前版本永不清理；
//! - 回收站 30 天清理：复用永久删除逻辑；
//! - 配额对账：只写指标与 WARN 日志，绝不自动修正 `used_bytes`。

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::job_ids;
use super::scan::ScanJob;
use crate::files::trash::permanently_delete_batch;

/// 每个文件保留的最近版本数。
pub const RETAIN_LATEST_VERSIONS: i64 = 10;
/// 超出保留数后仍继续保留的天数。
pub const VERSION_RETENTION_DAYS: i32 = 30;
/// 回收站保留天数。
pub const TRASH_RETENTION_DAYS: i32 = 30;

/// 版本保留清理。
pub struct VersionRetentionCleanup;

#[async_trait]
impl ScanJob for VersionRetentionCleanup {
    fn job_id(&self) -> i32 {
        job_ids::VERSION_CLEANUP
    }

    fn name(&self) -> &'static str {
        "version_cleanup"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(6 * 60 * 60)
    }

    async fn run_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        limit: i64,
    ) -> Result<u64, sqlx::Error> {
        // 最近 10 个版本无条件保留；排名之外的版本还要早于 30 天。
        let candidates = sqlx::query!(
            r#"
            SELECT candidate.id, candidate.user_id
            FROM (
                SELECT
                    fv.id,
                    fv.user_id,
                    fv.node_id,
                    fv.created_at,
                    row_number() OVER (
                        PARTITION BY fv.node_id
                        ORDER BY fv.created_at DESC, fv.id DESC
                    ) AS rank
                FROM file_versions fv
            ) candidate
            JOIN nodes n
                ON n.id = candidate.node_id AND n.user_id = candidate.user_id
            WHERE candidate.rank > $1
              AND candidate.created_at < now() - make_interval(days => $2)
              AND candidate.id IS DISTINCT FROM n.current_version_id
            ORDER BY candidate.id
            LIMIT $3
            "#,
            RETAIN_LATEST_VERSIONS,
            VERSION_RETENTION_DAYS,
            limit,
        )
        .fetch_all(&mut **tx)
        .await?;

        let mut freed_by_user: HashMap<Uuid, i64> = HashMap::new();
        let mut deleted_versions: u64 = 0;

        for candidate in candidates {
            let removed = sqlx::query_scalar!(
                r#"
                DELETE FROM file_versions
                WHERE id = $1 AND user_id = $2
                RETURNING content_id
                "#,
                candidate.id,
                candidate.user_id,
            )
            .fetch_optional(&mut **tx)
            .await?;
            let Some(content_id) = removed else {
                continue;
            };
            deleted_versions += 1;

            let still_referenced = sqlx::query_scalar!(
                r#"
                SELECT EXISTS (
                    SELECT 1 FROM file_versions
                    WHERE user_id = $1 AND content_id = $2
                ) AS "exists!"
                "#,
                candidate.user_id,
                content_id,
            )
            .fetch_one(&mut **tx)
            .await?;
            if still_referenced {
                continue;
            }

            let content = sqlx::query!(
                "SELECT ciphertext_size FROM contents WHERE user_id = $1 AND id = $2",
                candidate.user_id,
                content_id,
            )
            .fetch_optional(&mut **tx)
            .await?;
            let Some(content) = content else {
                continue;
            };

            let object_keys: Vec<String> = sqlx::query_scalar!(
                r#"
                DELETE FROM chunks
                WHERE user_id = $1 AND content_id = $2
                RETURNING object_key
                "#,
                candidate.user_id,
                content_id,
            )
            .fetch_all(&mut **tx)
            .await?;
            crate::jobs::deletion::enqueue_object_deletions(tx, &object_keys).await?;

            let deleted_contents = sqlx::query!(
                "DELETE FROM contents WHERE user_id = $1 AND id = $2",
                candidate.user_id,
                content_id,
            )
            .execute(&mut **tx)
            .await?;
            if deleted_contents.rows_affected() == 1 {
                *freed_by_user.entry(candidate.user_id).or_default() += content.ciphertext_size;
            }
        }

        for (user_id, bytes) in freed_by_user {
            sqlx::query!(
                r#"
                UPDATE users
                SET used_bytes = GREATEST(used_bytes - $2, 0),
                    updated_at = now()
                WHERE id = $1
                "#,
                user_id,
                bytes,
            )
            .execute(&mut **tx)
            .await?;
        }

        Ok(deleted_versions)
    }
}

/// 回收站 30 天清理。
pub struct TrashCleanup;

#[async_trait]
impl ScanJob for TrashCleanup {
    fn job_id(&self) -> i32 {
        job_ids::TRASH_CLEANUP
    }

    fn name(&self) -> &'static str {
        "trash_cleanup"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(60 * 60)
    }

    async fn run_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        limit: i64,
    ) -> Result<u64, sqlx::Error> {
        let batches = sqlx::query!(
            r#"
            SELECT batch.user_id, batch.trash_batch_id
            FROM (
                SELECT DISTINCT ON (n.user_id, n.trash_batch_id)
                    n.user_id,
                    n.trash_batch_id
                FROM nodes n
                WHERE n.deleted_at IS NOT NULL
                  AND n.deleted_at < now() - make_interval(days => $1)
                  AND NOT EXISTS (
                      SELECT 1 FROM nodes parent
                      WHERE parent.user_id = n.user_id
                        AND parent.id = n.parent_id
                        AND parent.trash_batch_id = n.trash_batch_id
                        AND parent.deleted_at IS NOT NULL
                  )
                ORDER BY n.user_id, n.trash_batch_id
            ) batch
            ORDER BY batch.user_id, batch.trash_batch_id
            LIMIT $2
            "#,
            TRASH_RETENTION_DAYS,
            limit,
        )
        .fetch_all(&mut **tx)
        .await?;

        let mut deleted_nodes: u64 = 0;
        for batch in batches {
            let Some(trash_batch_id) = batch.trash_batch_id else {
                continue;
            };
            // 与请求路径一致：同一用户的配额/结构变更先锁用户行。
            sqlx::query!(
                "SELECT id FROM users WHERE id = $1 FOR UPDATE",
                batch.user_id,
            )
            .fetch_optional(&mut **tx)
            .await?;
            let summary = permanently_delete_batch(tx, batch.user_id, trash_batch_id).await?;
            deleted_nodes += summary.deleted_nodes;
        }
        Ok(deleted_nodes)
    }
}

/// 配额对账（只告警，不修正）。
pub struct QuotaReconciliation;

#[async_trait]
impl ScanJob for QuotaReconciliation {
    fn job_id(&self) -> i32 {
        job_ids::QUOTA_RECONCILIATION
    }

    fn name(&self) -> &'static str {
        "quota_reconciliation"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(24 * 60 * 60)
    }

    async fn run_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        _limit: i64,
    ) -> Result<u64, sqlx::Error> {
        // 已用量 = 被至少一个保留版本引用的 content 的密文字节数之和；
        // 同一 content 被多个版本引用只计一次。
        //
        // 该任务只做一次只读聚合，不逐行修改任何表，因此没有“批次游标”；
        // 指标与 WARN 日志覆盖全部用户，避免按 id 截断导致永远漏掉后面的用户。
        let totals = sqlx::query!(
            r#"
            WITH actual AS (
                SELECT
                    u.id,
                    u.used_bytes,
                    COALESCE((
                        SELECT SUM(c.ciphertext_size)
                        FROM contents c
                        WHERE c.user_id = u.id
                          AND EXISTS (
                              SELECT 1
                              FROM file_versions fv
                              WHERE fv.content_id = c.id AND fv.user_id = u.id
                          )
                    ), 0)::bigint AS actual
                FROM users u
            )
            SELECT
                count(*)::bigint AS "mismatched!",
                COALESCE(SUM(used_bytes - actual), 0)::bigint AS "delta!"
            FROM actual
            WHERE used_bytes <> actual
            "#
        )
        .fetch_one(&mut **tx)
        .await?;

        metrics::gauge!("quota_reconciliation_mismatched_users").set(totals.mismatched as f64);
        metrics::gauge!("quota_reconciliation_delta_bytes").set(totals.delta as f64);

        if totals.mismatched > 0 {
            // 只记录汇总，避免把用户 ID 与用量细节写进日志；不自动修正。
            tracing::warn!(
                mismatched_users = totals.mismatched,
                delta_bytes = totals.delta,
                "配额对账发现偏差（仅告警，不自动修正）"
            );
        }

        Ok(totals.mismatched.max(0) as u64)
    }
}
