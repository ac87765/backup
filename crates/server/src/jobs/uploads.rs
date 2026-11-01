//! 上传相关的扫描任务（task 8.5）。
//!
//! - [`UploadSessionExpiry`]：把超过 24 小时的 open 会话置为 expired，
//!   释放预留配额，并把当前对象键与被替换的旧键写入删除队列。
//! - [`OrphanObjectScan`]：分批流式列举 `v1/` 前缀下的对象，只处理
//!   `last_modified` 早于 48 小时、且不被 `chunks` 或未关闭会话的
//!   `upload_chunks` 引用的对象；进度游标持久化在 `job_state`，每次运行
//!   有处理上限，因此大 bucket 也能跨多次运行逐步扫完。
//!
//! 锁顺序：会话过期任务先锁 `users` 行、再锁会话行，与提交/中止路径一致，
//! 避免与请求处理互相死锁。孤儿巡检只读数据库并写删除队列，不持有会话锁。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use sqlx::{Postgres, Transaction};

use super::deletion::enqueue_object_deletions;
use super::job_ids;
use super::scan::ScanJob;
use crate::files::uploads::{collect_session_object_keys, load_expired_session_for_update};
use crate::storage::Storage;

/// 会话过期扫描的执行间隔。
const EXPIRY_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// 孤儿巡检的执行间隔（每次只处理 `limit` 个对象，逐步扫完）。
const ORPHAN_SCAN_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// 孤儿对象的最小年龄（design D10 / file-transfer spec：48 小时）。
pub const ORPHAN_MIN_AGE: Duration = Duration::from_secs(48 * 60 * 60);
/// 孤儿巡检的固定前缀（所有业务对象都在 `v1/{user_id}/{uuid}`）。
const OBJECT_PREFIX: &str = "v1/";

/// 上传会话过期清理任务。
pub struct UploadSessionExpiry;

#[async_trait]
impl ScanJob for UploadSessionExpiry {
    fn job_id(&self) -> i32 {
        job_ids::UPLOAD_SESSION_EXPIRY
    }

    fn name(&self) -> &'static str {
        "upload_session_expiry"
    }

    fn interval(&self) -> Duration {
        EXPIRY_INTERVAL
    }

    async fn run_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        limit: i64,
    ) -> Result<u64, sqlx::Error> {
        // 先只取候选 ID（不锁会话行）；随后按 user_id 顺序先锁用户行，
        // 再锁定并复核会话，保持与提交/中止路径相同的锁顺序。
        let candidates = sqlx::query!(
            r#"
            SELECT id, user_id
            FROM upload_sessions
            WHERE status = 'open' AND expires_at <= now()
            ORDER BY user_id, expires_at, id
            LIMIT $1
            "#,
            limit,
        )
        .fetch_all(&mut **tx)
        .await?;

        let mut expired: u64 = 0;
        for candidate in candidates {
            // 用户行可能已被其他事务删除；跳过并让外键级联处理会话。
            // 与 `files::lock_user` 一致使用 NO KEY UPDATE，避免与 presign 的外键检查死锁。
            let user = sqlx::query_scalar!(
                "SELECT id FROM users WHERE id = $1 FOR NO KEY UPDATE",
                candidate.user_id,
            )
            .fetch_optional(&mut **tx)
            .await?;
            if user.is_none() {
                continue;
            }

            let Some(row) = load_expired_session_for_update(tx, candidate.id).await? else {
                continue;
            };
            if row.user_id != candidate.user_id {
                continue;
            }

            let keys = collect_session_object_keys(tx, row.user_id, row.id).await?;
            enqueue_object_deletions(tx, &keys).await?;

            sqlx::query!(
                "DELETE FROM upload_chunks WHERE user_id = $1 AND session_id = $2",
                row.user_id,
                row.id,
            )
            .execute(&mut **tx)
            .await?;

            let updated = sqlx::query!(
                r#"
                UPDATE upload_sessions
                SET status = 'expired'
                WHERE id = $1 AND user_id = $2 AND status = 'open'
                "#,
                row.id,
                row.user_id,
            )
            .execute(&mut **tx)
            .await?;
            if updated.rows_affected() != 1 {
                continue;
            }

            sqlx::query!(
                r#"
                UPDATE users
                SET reserved_bytes = reserved_bytes - $2,
                    updated_at = now()
                WHERE id = $1
                "#,
                row.user_id,
                row.reserved_bytes,
            )
            .execute(&mut **tx)
            .await?;

            expired += 1;
        }
        Ok(expired)
    }
}

/// 孤儿对象巡检。
pub struct OrphanObjectScan {
    storage: Arc<Storage>,
    min_age: Duration,
    prefix: String,
}

impl OrphanObjectScan {
    /// 生产配置：只处理早于 48 小时的对象。
    pub fn new(storage: Arc<Storage>) -> Self {
        Self {
            storage,
            min_age: ORPHAN_MIN_AGE,
            prefix: OBJECT_PREFIX.to_owned(),
        }
    }

    /// 测试入口：可缩短最小年龄、缩小前缀，用真实 MinIO 验证巡检逻辑。
    /// 生产路径始终使用 `v1/` 全前缀与 48 小时。
    pub fn with_options(storage: Arc<Storage>, min_age: Duration, prefix: String) -> Self {
        Self {
            storage,
            min_age,
            prefix,
        }
    }
}

#[async_trait]
impl ScanJob for OrphanObjectScan {
    fn job_id(&self) -> i32 {
        job_ids::ORPHAN_OBJECT_SCAN
    }

    fn name(&self) -> &'static str {
        "orphan_object_scan"
    }

    fn interval(&self) -> Duration {
        ORPHAN_SCAN_INTERVAL
    }

    async fn run_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        limit: i64,
    ) -> Result<u64, sqlx::Error> {
        if limit <= 0 {
            return Ok(0);
        }

        let stored = sqlx::query_scalar!(
            "SELECT cursor FROM job_state WHERE job_id = $1 FOR UPDATE",
            job_ids::ORPHAN_OBJECT_SCAN,
        )
        .fetch_optional(&mut **tx)
        .await?;
        let cursor = stored.flatten();

        let entries = self
            .storage
            .list_prefix_page(&self.prefix, cursor.as_deref(), limit as usize)
            .await
            .map_err(|error| {
                // StorageError 的 Display 不含对象键、URL 或凭据。
                sqlx::Error::Io(std::io::Error::other(format!("列举对象存储失败: {error}")))
            })?;

        let now = SystemTime::now();
        let candidates: Vec<String> = entries
            .iter()
            .filter(|entry| !entry.is_dir)
            .filter(|entry| {
                entry
                    .last_modified
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age >= self.min_age)
            })
            .map(|entry| entry.key.clone())
            .collect();

        let mut to_delete = Vec::new();
        if !candidates.is_empty() {
            // 被任何 content 的块引用 → 不是孤儿。
            let referenced_by_chunks: HashSet<String> = sqlx::query_scalar!(
                r#"
                SELECT object_key
                FROM chunks
                WHERE object_key = ANY($1::text[])
                "#,
                &candidates,
            )
            .fetch_all(&mut **tx)
            .await?
            .into_iter()
            .collect();

            // 被未关闭会话的当前键或已被替换的旧键引用 → 仍在进行中，跳过。
            let mut protected_by_sessions: HashSet<String> = HashSet::new();
            let open_rows = sqlx::query!(
                r#"
                SELECT uc.object_key, uc.superseded_object_keys
                FROM upload_chunks uc
                JOIN upload_sessions us
                    ON us.id = uc.session_id AND us.user_id = uc.user_id
                WHERE us.status = 'open'
                  AND (
                      uc.object_key = ANY($1::text[])
                      OR uc.superseded_object_keys && $1::text[]
                  )
                "#,
                &candidates,
            )
            .fetch_all(&mut **tx)
            .await?;
            for row in open_rows {
                protected_by_sessions.insert(row.object_key);
                protected_by_sessions.extend(row.superseded_object_keys);
            }

            // 已在删除队列中的对象不重复入队。
            let already_queued: HashSet<String> = sqlx::query_scalar!(
                r#"
                SELECT object_key
                FROM object_deletions
                WHERE object_key = ANY($1::text[])
                "#,
                &candidates,
            )
            .fetch_all(&mut **tx)
            .await?
            .into_iter()
            .collect();

            to_delete = candidates
                .iter()
                .filter(|key| !referenced_by_chunks.contains(*key))
                .filter(|key| !protected_by_sessions.contains(*key))
                .filter(|key| !already_queued.contains(*key))
                .cloned()
                .collect();
        }

        let enqueued = enqueue_object_deletions(tx, &to_delete).await?;

        // 游标推进到最后一条“已列举”的对象（无论它是否被删除），
        // 保证对象很多时每轮都有进展；不足一页即回到开头。
        let next_cursor = if entries.len() >= limit as usize {
            entries.last().map(|entry| entry.key.clone())
        } else {
            None
        };
        sqlx::query!(
            r#"
            INSERT INTO job_state (job_id, cursor, updated_at)
            VALUES ($1, $2, now())
            ON CONFLICT (job_id) DO UPDATE
            SET cursor = EXCLUDED.cursor,
                updated_at = now()
            "#,
            job_ids::ORPHAN_OBJECT_SCAN,
            next_cursor,
        )
        .execute(&mut **tx)
        .await?;

        if enqueued > 0 {
            tracing::info!(
                scanned = entries.len(),
                enqueued,
                "孤儿对象巡检发现了待删除对象"
            );
        }
        Ok(enqueued)
    }
}
