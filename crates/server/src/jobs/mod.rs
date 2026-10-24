//! 后台任务框架（design D10）。
//!
//! 两类任务：
//! - 队列型：`object_deletions`、`email_outbox`。在一个短事务里用
//!   `FOR UPDATE SKIP LOCKED` 领取一批并设置 `claimed_until + claim_token`，
//!   事务提交后才在事务外执行副作用；失败按带抖动的指数退避重试，永不放弃。
//! - 扫描型：每个任务有固定 `job_id`，每批在独立事务中先
//!   `pg_try_advisory_xact_lock(job_id)`，拿不到就跳过；每批处理行数有上限。
//!
//! 所有后台任务都通过 [`crate::shutdown::Shutdown`] 的 TaskTracker 跟踪：
//! 收到取消信号后不再领取新工作，当前批次要么在统一停机期限内完成，要么被放弃
//! （队列型任务由租约过期兜底）。

pub mod deletion;
pub mod email;
pub mod metrics;
pub mod retry;
pub mod scan;

use std::sync::Arc;

use crate::Database;
use crate::Storage;
use crate::mail::Mailer;
use crate::shutdown::Shutdown;

/// 扫描型任务的固定数字 ID。
///
/// 这些 ID 用作 PostgreSQL advisory lock 的 key，必须全仓库唯一且不可复用。
/// 本组只实现框架与测试任务；业务任务在各个后续组接入时使用这里的常量。
pub mod job_ids {
    /// 测试专用扫描任务（仅测试构造）。
    pub const TEST_SCAN: i32 = 9_001;
    /// 上传会话过期清理（第 8 组）。
    pub const UPLOAD_SESSION_EXPIRY: i32 = 1_001;
    /// 版本保留清理（第 7 组）。
    pub const VERSION_CLEANUP: i32 = 1_002;
    /// 回收站 30 天清理（第 7 组）。
    pub const TRASH_CLEANUP: i32 = 1_003;
    /// 过期会话与邮件令牌清理（第 6、7 组）。
    pub const EXPIRED_TOKEN_CLEANUP: i32 = 1_004;
    /// 孤儿对象巡检（第 8 组）。
    pub const ORPHAN_OBJECT_SCAN: i32 = 1_005;
    /// 配额对账（第 7 组）。
    pub const QUOTA_RECONCILIATION: i32 = 1_006;
}

/// 启动本进程内的后台任务，并交由停机协调器跟踪。
///
/// `jobs_enabled = false` 时不启动任何 worker（D10：便于拆分为独立 worker 进程）。
pub fn start(
    database: Database,
    storage: Arc<Storage>,
    mailer: Arc<dyn Mailer>,
    recipient_resolver: Arc<dyn email::RecipientResolver>,
    shutdown: &Shutdown,
    jobs_enabled: bool,
) {
    if !jobs_enabled {
        tracing::info!("BACKUP_JOBS_ENABLED=false，本实例不运行后台任务");
        return;
    }

    let token = shutdown.token();
    drop(shutdown.spawn(deletion::worker_loop(
        database.clone(),
        storage,
        token.clone(),
    )));
    drop(shutdown.spawn(email::worker_loop(
        database.clone(),
        mailer,
        recipient_resolver,
        token.clone(),
    )));
    // 本组没有业务扫描任务；第 6 组起把任务注册到这里。
    drop(shutdown.spawn(scan::run_loop(database.clone(), Vec::new(), token.clone())));
    drop(shutdown.spawn(metrics::sampler_loop(database, token)));
    tracing::info!("后台任务已启动");
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::job_ids;

    #[test]
    fn advisory_lock_job_ids_are_unique() {
        let mut ids = vec![
            job_ids::TEST_SCAN,
            job_ids::UPLOAD_SESSION_EXPIRY,
            job_ids::VERSION_CLEANUP,
            job_ids::TRASH_CLEANUP,
            job_ids::EXPIRED_TOKEN_CLEANUP,
            job_ids::ORPHAN_OBJECT_SCAN,
            job_ids::QUOTA_RECONCILIATION,
        ];
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 7, "job_id 必须集中定义且不得重复");
    }
}
