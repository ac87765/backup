//! 后台任务集成测试（tasks 5.1、5.2）。
//!
//! 覆盖队列领取/租约/退避、真实 MinIO 删除、扫描任务的 advisory lock 互斥与指标；
//! 以及 `deletion_handle` 只允许出现在 storage/jobs 模块的源码扫描。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use server::Database;
use server::jobs::LeaseConfig;
use server::jobs::deletion::{
    claim_batch, complete, enqueue_object_deletions, run_batch, run_batch_with_lease,
};
use server::jobs::scan::{ScanJob, ScanOutcome, run_once};
use server::storage::{FaultInjectionConfig, InjectedErrorKind, ObjectOperation, Storage};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use common::{environment, integration_storage_config, migrated_database};

async fn enqueue(database: &Database, keys: &[String]) {
    let mut tx = database.pool().begin().await.expect("开始事务");
    enqueue_object_deletions(&mut tx, keys)
        .await
        .expect("登记删除");
    tx.commit().await.expect("提交事务");
}

#[tokio::test]
async fn two_workers_never_claim_the_same_row() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let keys: Vec<String> = (0..5)
        .map(|_| format!("v1/{}/{}", Uuid::new_v4(), Uuid::new_v4()))
        .collect();
    enqueue(&database, &keys).await;

    let (first, second) = tokio::join!(
        claim_batch(database.pool(), 10),
        claim_batch(database.pool(), 10)
    );
    let first = first.expect("worker 1 领取");
    let second = second.expect("worker 2 领取");
    let mut ids: Vec<i64> = first
        .iter()
        .chain(second.iter())
        .map(|row| row.id)
        .collect();
    ids.sort_unstable();
    let unique = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), unique, "同一行不能被两个 worker 同时领取");
    assert_eq!(ids.len(), 5, "所有到期行都应被领取");
}

#[tokio::test]
async fn expired_lease_can_be_reclaimed_and_old_token_cannot_complete() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    enqueue(
        &database,
        &[format!("v1/{}/{}", Uuid::new_v4(), Uuid::new_v4())],
    )
    .await;

    let first = claim_batch(database.pool(), 1).await.expect("第一次领取");
    assert_eq!(first.len(), 1);
    let item = &first[0];

    // 模拟 worker 崩溃/处理过慢：租约过期。
    sqlx::query(
        "UPDATE object_deletions SET claimed_until = now() - interval '1 second' WHERE id = $1",
    )
    .bind(item.id)
    .execute(database.pool())
    .await
    .expect("过期租约");

    let reclaimed = claim_batch(database.pool(), 1).await.expect("重新领取");
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].id, item.id);
    assert_ne!(reclaimed[0].claim_token, item.claim_token);

    assert!(
        !complete(database.pool(), item.id, item.claim_token)
            .await
            .expect("旧 token 完成"),
        "旧 worker 不能删除已被重新领取的行"
    );
    assert!(
        complete(database.pool(), item.id, reclaimed[0].claim_token)
            .await
            .expect("新 token 完成"),
        "新 worker 应能完成"
    );
}

#[tokio::test]
async fn deleting_a_missing_object_is_success() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let storage = Arc::new(real_storage_async().await);
    let key = format!("v1/{}/{}", Uuid::new_v4(), Uuid::new_v4());
    enqueue(&database, &[key]).await;

    let stats = run_batch(&database, &storage, 10).await.expect("处理删除");
    assert_eq!(stats.claimed, 1);
    assert_eq!(stats.deleted, 1);
    assert_eq!(stats.failed, 0);

    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM object_deletions")
        .fetch_one(database.pool())
        .await
        .expect("统计队列");
    assert_eq!(remaining, 0);
}

async fn real_storage_async() -> Storage {
    let env = environment().await;
    Storage::new(&integration_storage_config(env)).expect("构造 storage")
}

#[tokio::test]
async fn storage_failure_backs_off_and_retries_until_success() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let base = real_storage_async().await;
    let faulty = Arc::new(base.with_fault_injection(
        FaultInjectionConfig::new(1, InjectedErrorKind::Unavailable).only(ObjectOperation::Delete),
    ));
    let key = format!("v1/{}/{}", Uuid::new_v4(), Uuid::new_v4());
    enqueue(&database, &[key]).await;

    let stats = run_batch(&database, &faulty, 10).await.expect("第一次处理");
    assert_eq!(stats.failed, 1);
    let row = sqlx::query(
        "SELECT attempts, last_error, not_before > now() AS future FROM object_deletions",
    )
    .fetch_one(database.pool())
    .await
    .expect("读取队列行");
    let attempts: i32 = row.get("attempts");
    let last_error: Option<String> = row.get("last_error");
    let future: bool = row.get("future");
    assert_eq!(attempts, 1);
    assert_eq!(last_error.as_deref(), Some("unavailable"));
    assert!(future, "失败后必须退避到未来");

    // not_before 未到：不会再次领取。
    let early = run_batch(&database, &faulty, 10).await.expect("提前处理");
    assert_eq!(early.claimed, 0);

    // 到期后同一故障注入已耗尽，重试成功。
    sqlx::query("UPDATE object_deletions SET not_before = now()")
        .execute(database.pool())
        .await
        .expect("将任务置为到期");
    let stats = run_batch(&database, &faulty, 10).await.expect("重试处理");
    assert_eq!(stats.deleted, 1);
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM object_deletions")
        .fetch_one(database.pool())
        .await
        .expect("统计队列");
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn one_item_failure_does_not_block_other_items() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let base = real_storage_async().await;
    let faulty = Arc::new(base.with_fault_injection(
        FaultInjectionConfig::new(1, InjectedErrorKind::Unavailable).only(ObjectOperation::Delete),
    ));
    let keys = vec![
        format!("v1/{}/{}", Uuid::new_v4(), Uuid::new_v4()),
        format!("v1/{}/{}", Uuid::new_v4(), Uuid::new_v4()),
    ];
    enqueue(&database, &keys).await;

    let stats = run_batch(&database, &faulty, 10).await.expect("处理批次");
    assert_eq!(stats.claimed, 2);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.deleted, 1, "单个工作项失败不得阻塞其他项");

    let rows = sqlx::query("SELECT attempts FROM object_deletions")
        .fetch_all(database.pool())
        .await
        .expect("读取剩余队列");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<i32, _>("attempts"), 1);
}

#[tokio::test]
async fn lease_aware_worker_releases_remaining_items_for_another_worker() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    let base = real_storage_async().await;
    // 慢存储：每次删除耗时 1.8 秒，用来触发“剩余时间不足安全余量”。
    let slow = Arc::new(
        base.with_fault_injection(
            FaultInjectionConfig::new(0, InjectedErrorKind::Unavailable)
                .only(ObjectOperation::Delete)
                .with_delay(Duration::from_millis(1_800)),
        ),
    );
    let keys: Vec<String> = (0..5)
        .map(|_| format!("v1/{}/{}", Uuid::new_v4(), Uuid::new_v4()))
        .collect();
    enqueue(&database, &keys).await;

    let lease = LeaseConfig::new(Duration::from_secs(3), Duration::from_millis(1_500));
    let stats = run_batch_with_lease(&database, &slow, 10, lease)
        .await
        .expect("处理删除批次");
    assert_eq!(stats.claimed, 5);
    assert_eq!(stats.deleted, 1, "只应开始处理第一个项目");
    assert_eq!(stats.released, 4, "租约接近到期时必须释放剩余项目");

    let released: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_deletions \
         WHERE claimed_until IS NULL AND claim_token IS NULL",
    )
    .fetch_one(database.pool())
    .await
    .expect("统计已释放行");
    assert_eq!(released, 4);

    // 另一个 worker 可以立即领取（无需等待租约到期）。
    let reclaimed = claim_batch(database.pool(), 10)
        .await
        .expect("另一个 worker 立即领取");
    assert_eq!(reclaimed.len(), 4, "被释放的项目应可被立即重新领取");
}

struct ProbeJob {
    delay: Duration,
}

#[async_trait]
impl ScanJob for ProbeJob {
    fn job_id(&self) -> i32 {
        server::jobs::job_ids::TEST_SCAN
    }

    fn name(&self) -> &'static str {
        "scan_probe"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(1)
    }

    async fn run_batch(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        _limit: i64,
    ) -> Result<u64, sqlx::Error> {
        tokio::time::sleep(self.delay).await;
        sqlx::query("INSERT INTO scan_probe DEFAULT VALUES")
            .execute(&mut **tx)
            .await
            .map(|result| result.rows_affected())
    }
}

async fn create_scan_probe(database: &Database) {
    sqlx::query("CREATE TABLE scan_probe (id BIGSERIAL PRIMARY KEY)")
        .execute(database.pool())
        .await
        .expect("创建扫描测试表");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_schedulers_execute_a_scan_job_only_once() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    create_scan_probe(&database).await;
    let job = ProbeJob {
        delay: Duration::from_millis(300),
    };

    let (first, second) =
        tokio::join!(run_once(&database, &job, 10), run_once(&database, &job, 10));
    let outcomes = [first.expect("第一次执行"), second.expect("第二次执行")];
    let executed = outcomes
        .iter()
        .filter(|outcome| matches!(**outcome, ScanOutcome::Executed(1)))
        .count();
    let skipped = outcomes
        .iter()
        .filter(|outcome| matches!(**outcome, ScanOutcome::SkippedLocked))
        .count();
    assert_eq!(executed, 1, "应当只有一个实例拿到 advisory lock");
    assert_eq!(skipped, 1, "另一个实例必须跳过");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM scan_probe")
        .fetch_one(database.pool())
        .await
        .expect("统计扫描结果");
    assert_eq!(rows, 1);
}

struct FailingJob;

#[async_trait]
impl ScanJob for FailingJob {
    fn job_id(&self) -> i32 {
        server::jobs::job_ids::TEST_SCAN + 1
    }

    fn name(&self) -> &'static str {
        "scan_fail"
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(1)
    }

    async fn run_batch(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        _limit: i64,
    ) -> Result<u64, sqlx::Error> {
        Err(sqlx::Error::RowNotFound)
    }
}

#[tokio::test]
async fn job_and_queue_metrics_are_exported() {
    let env = environment().await;
    let (_test_db, database) = migrated_database(env).await;
    create_scan_probe(&database).await;
    let handle = server::metrics::install().expect("安装指标记录器");

    let job = ProbeJob {
        delay: Duration::ZERO,
    };
    run_once(&database, &job, 10).await.expect("执行扫描任务");
    assert!(
        run_once(&database, &FailingJob, 10).await.is_err(),
        "失败任务应返回错误"
    );

    // 放一条删除任务与一封邮件，验证积压/最老年龄指标。
    let key = format!("v1/{}/{}", Uuid::new_v4(), Uuid::new_v4());
    enqueue(&database, &[key]).await;
    let mut tx = database.pool().begin().await.expect("开始事务");
    server::jobs::email::enqueue_email(
        &mut tx,
        Uuid::new_v4(),
        server::jobs::email::PURPOSE_TEST,
        &serde_json::json!({}),
    )
    .await
    .expect("登记邮件");
    tx.commit().await.expect("提交事务");
    server::jobs::metrics::record_queue_metrics(database.pool())
        .await
        .expect("采样队列指标");

    let text = handle.render();
    assert!(
        text.contains("jobs_executions_total"),
        "必须导出任务执行次数"
    );
    assert!(text.contains("job=\"scan_probe\""), "{text}");
    assert!(
        text.contains("jobs_failures_total{job=\"scan_fail\"}"),
        "必须导出任务失败次数: {text}"
    );
    assert!(
        text.contains("queue_backlog{queue=\"object_deletions\"}"),
        "{text}"
    );
    assert!(
        text.contains("queue_oldest_age_seconds{queue=\"object_deletions\"}"),
        "{text}"
    );
    assert!(
        text.contains("queue_backlog{queue=\"email_outbox\"}"),
        "{text}"
    );
}

#[test]
fn deletion_handle_is_only_used_by_storage_and_jobs() {
    fn visit(dir: &std::path::Path, hits: &mut usize) {
        for entry in std::fs::read_dir(dir).expect("读取源码目录") {
            let entry = entry.expect("目录项");
            let path = entry.path();
            if path.is_dir() {
                visit(&path, hits);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("读取源码");
            if !source.contains("deletion_handle") {
                continue;
            }
            *hits += source.matches("deletion_handle").count();
            let normalized = path.to_string_lossy().replace('\\', "/");
            let allowed =
                normalized.ends_with("src/storage/mod.rs") || normalized.contains("/src/jobs/");
            assert!(
                allowed,
                "deletion_handle 只能出现在 storage 与 jobs 模块，违规文件: {normalized}"
            );
        }
    }

    let mut hits = 0;
    visit(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut hits,
    );
    assert!(hits >= 2, "源码扫描应当至少命中 storage 与 jobs");
}
