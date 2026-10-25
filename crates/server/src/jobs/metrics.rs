//! 后台任务指标（service-operations spec：指标）。
//!
//! - `jobs_executions_total{job}` / `jobs_failures_total{job}`；
//! - `queue_backlog{queue}` / `queue_oldest_age_seconds{queue}`（定时采样）；
//! - `upload_sessions{status}`（按状态采样的上传会话数量，第 8 组）。

use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

use crate::Database;

/// 队列积压指标的采样间隔。
const QUEUE_SAMPLE_INTERVAL: Duration = Duration::from_secs(15);

pub fn record_execution(job: &str) {
    metrics::counter!("jobs_executions_total", "job" => job.to_owned()).increment(1);
}

pub fn record_failure(job: &str) {
    metrics::counter!("jobs_failures_total", "job" => job.to_owned()).increment(1);
}

/// 写入一次对象删除/邮件 outbox 的积压与最老等待时长。
pub async fn record_queue_metrics(pool: &PgPool) -> Result<(), sqlx::Error> {
    let row = sqlx::query!(
        r#"
        SELECT
            (SELECT count(*) FROM object_deletions) AS "deletions_count!",
            (SELECT COALESCE(EXTRACT(EPOCH FROM now() - min(created_at)), 0)::float8
               FROM object_deletions) AS "deletions_oldest!",
            (SELECT count(*) FROM email_outbox) AS "mail_count!",
            (SELECT COALESCE(EXTRACT(EPOCH FROM now() - min(created_at)), 0)::float8
               FROM email_outbox) AS "mail_oldest!"
        "#
    )
    .fetch_one(pool)
    .await?;

    metrics::gauge!("queue_backlog", "queue" => "object_deletions").set(row.deletions_count as f64);
    metrics::gauge!("queue_oldest_age_seconds", "queue" => "object_deletions")
        .set(row.deletions_oldest);
    metrics::gauge!("queue_backlog", "queue" => "email_outbox").set(row.mail_count as f64);
    metrics::gauge!("queue_oldest_age_seconds", "queue" => "email_outbox").set(row.mail_oldest);

    // 上传会话按状态采样；状态集合固定（open/committed/aborted/expired）。
    let sessions = sqlx::query!(
        r#"
        SELECT status, count(*) AS "count!"
        FROM upload_sessions
        GROUP BY status
        "#
    )
    .fetch_all(pool)
    .await?;
    for session in sessions {
        metrics::gauge!("upload_sessions", "status" => session.status).set(session.count as f64);
    }
    Ok(())
}

/// 周期性采样队列指标，直到停机。
pub async fn sampler_loop(database: Database, shutdown: CancellationToken) {
    loop {
        if let Err(error) = record_queue_metrics(database.pool()).await {
            tracing::warn!(error = %error, "采样队列指标失败");
        }
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(QUEUE_SAMPLE_INTERVAL) => {}
        }
    }
}
