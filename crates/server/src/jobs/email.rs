//! 邮件 outbox 与发送 worker（design D13）。
//!
//! outbox 只保存 `user_id / purpose / params`，**不保存任何令牌**。
//! 每封邮件在发送前生成新令牌，只把 SHA-256 哈希写入 `email_tokens`，再渲染并发送；
//! 发送失败退避重试，重试会重新生成令牌（旧未使用令牌先删除，从而失效）。
//!
//! 收件人由 [`RecipientResolver`] 解析：本组还没有 `users` 表，生产实现留给第 6 组；
//! 测试注入固定收件人。

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rand::{Rng, TryRngCore};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::LeaseConfig;
use super::metrics;
use super::retry::backoff_delay;
use crate::Database;
use crate::mail::{
    Mailer, RenderedMail, is_supported_purpose, purpose_requires_token, render_mail,
};

pub use crate::mail::purpose::{
    EXISTING_ACCOUNT, PASSWORD_CHANGED, RECOVERY_KEY_CHANGED, VERIFY_EMAIL,
};

/// 一批领取的最大行数。
///
/// 邮件发送是慢操作（含网络往返），批次比删除队列小，避免一批占用租约过久。
pub const BATCH_SIZE: i64 = 20;
/// 邮件令牌有效期（与 [`crate::auth::EMAIL_TOKEN_TTL_HOURS`] 一致）。
const TOKEN_TTL_HOURS: i32 = 24;
/// 测试模板常量（第 5 组测试使用）。
pub const PURPOSE_TEST: &str = crate::mail::purpose::TEST;

/// 收件人解析抽象（测试可注入固定收件人）。
#[async_trait]
pub trait RecipientResolver: Send + Sync {
    async fn resolve(&self, pool: &PgPool, user_id: Uuid) -> Result<Option<String>, sqlx::Error>;
}

/// 生产实现：按 `users.id` 查询规范化后的邮箱。
pub struct SqlRecipientResolver;

#[async_trait]
impl RecipientResolver for SqlRecipientResolver {
    async fn resolve(&self, pool: &PgPool, user_id: Uuid) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar!(
            r#"SELECT email::text AS "email!" FROM users WHERE id = $1"#,
            user_id,
        )
        .fetch_optional(pool)
        .await
    }
}

#[derive(Debug, Clone)]
pub struct ClaimedEmail {
    pub id: i64,
    pub user_id: Uuid,
    pub purpose: String,
    pub params: serde_json::Value,
    pub attempts: i32,
    pub claim_token: Uuid,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EmailStats {
    pub claimed: usize,
    pub sent: usize,
    pub failed: usize,
    pub lease_lost: usize,
    /// 因租约即将到期而主动释放、留给其他实例处理的数量。
    pub released: usize,
}

/// 在业务事务内登记一封邮件（只接受 `&mut Transaction`）。
pub async fn enqueue_email(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    purpose: &str,
    params: &serde_json::Value,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"
        INSERT INTO email_outbox (user_id, purpose, params)
        VALUES ($1, $2, $3)
        RETURNING id
        "#,
        user_id,
        purpose,
        params,
    )
    .fetch_one(&mut **tx)
    .await
}

/// 领取一批到期邮件（生产默认租约）。
pub async fn claim_batch(pool: &PgPool, limit: i64) -> Result<Vec<ClaimedEmail>, sqlx::Error> {
    claim_batch_with_lease(pool, limit, LeaseConfig::DEFAULT.lease).await
}

/// 使用指定租约领取一批到期邮件。
pub async fn claim_batch_with_lease(
    pool: &PgPool,
    limit: i64,
    lease: Duration,
) -> Result<Vec<ClaimedEmail>, sqlx::Error> {
    let claim_token = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    let claimed = sqlx::query_as!(
        ClaimedEmail,
        r#"
        UPDATE email_outbox
        SET claimed_until = now() + make_interval(secs => $1),
            claim_token = $2
        WHERE id IN (
            SELECT id
            FROM email_outbox
            WHERE not_before <= now()
              AND (claimed_until IS NULL OR claimed_until < now())
            ORDER BY id
            FOR UPDATE SKIP LOCKED
            LIMIT $3
        )
        RETURNING id, user_id, purpose, params, attempts, claim_token AS "claim_token!"
        "#,
        lease.as_secs_f64(),
        claim_token,
        limit,
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(claimed)
}

/// 立即释放本批尚未开始的邮件（必须匹配 claim_token）。
pub async fn release(pool: &PgPool, ids: &[i64], claim_token: Uuid) -> Result<u64, sqlx::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query!(
        r#"
        UPDATE email_outbox
        SET claimed_until = NULL,
            claim_token = NULL
        WHERE id = ANY($1::bigint[]) AND claim_token = $2
        "#,
        ids,
        claim_token,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn complete(pool: &PgPool, id: i64, claim_token: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM email_outbox WHERE id = $1 AND claim_token = $2",
        id,
        claim_token,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

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
        UPDATE email_outbox
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

/// 处理一批邮件（生产默认租约）：生成令牌 → 写哈希 → 渲染 → 发送 → 删除 outbox 行。
pub async fn run_batch(
    database: &Database,
    mailer: &Arc<dyn Mailer>,
    resolver: &Arc<dyn RecipientResolver>,
    public_app_url: &str,
    limit: i64,
) -> Result<EmailStats, sqlx::Error> {
    run_batch_with_lease(
        database,
        mailer,
        resolver,
        public_app_url,
        limit,
        LeaseConfig::DEFAULT,
    )
    .await
}

/// 使用指定租约处理一批邮件。
pub async fn run_batch_with_lease(
    database: &Database,
    mailer: &Arc<dyn Mailer>,
    resolver: &Arc<dyn RecipientResolver>,
    public_app_url: &str,
    limit: i64,
    lease: LeaseConfig,
) -> Result<EmailStats, sqlx::Error> {
    let claimed = claim_batch_with_lease(database.pool(), limit, lease.lease).await?;
    // 从领取成功时开始计时；这是计算租约剩余时间的基准。
    let claimed_at = Instant::now();
    let mut stats = EmailStats {
        claimed: claimed.len(),
        ..EmailStats::default()
    };

    let mut index = 0;
    while index < claimed.len() {
        if !lease.allows_new_item(claimed_at) {
            let remaining_ids: Vec<i64> = claimed[index..].iter().map(|item| item.id).collect();
            let released =
                release(database.pool(), &remaining_ids, claimed[index].claim_token).await?;
            stats.released += released as usize;
            tracing::warn!(
                released,
                remaining_ms = lease.remaining(claimed_at).as_millis() as u64,
                "邮件租约即将到期，已释放本批剩余邮件"
            );
            break;
        }

        let item = &claimed[index];
        index += 1;
        if !is_supported_purpose(&item.purpose) {
            // 未知模板属于配置/代码错误，但仍按失败重试，避免静默丢邮件。
            if fail(
                database.pool(),
                item.id,
                item.claim_token,
                item.attempts + 1,
                "unsupported_purpose",
            )
            .await?
            {
                stats.failed += 1;
                metrics::record_failure("email_outbox");
            } else {
                stats.lease_lost += 1;
            }
            continue;
        }

        let recipient = match resolver.resolve(database.pool(), item.user_id).await {
            Ok(Some(recipient)) => recipient,
            Ok(None) => {
                if fail(
                    database.pool(),
                    item.id,
                    item.claim_token,
                    item.attempts + 1,
                    "recipient_unknown",
                )
                .await?
                {
                    stats.failed += 1;
                    metrics::record_failure("email_outbox");
                } else {
                    stats.lease_lost += 1;
                }
                continue;
            }
            Err(error) => return Err(error),
        };

        // 只有验证/测试邮件需要令牌；通知邮件不产生无意义的令牌行。
        let token = if purpose_requires_token(&item.purpose) {
            Some(generate_token())
        } else {
            None
        };
        if let Some(token) = token.as_deref() {
            let token_hash = hash_token(token);
            let mut tx = database.pool().begin().await?;
            // 重试时让旧令牌失效：只保留本次生成的未使用令牌。
            sqlx::query!(
                "DELETE FROM email_tokens WHERE user_id = $1 AND purpose = $2 AND used_at IS NULL",
                item.user_id,
                item.purpose.as_str(),
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query!(
                r#"
                INSERT INTO email_tokens (user_id, purpose, token_hash, expires_at)
                VALUES ($1, $2, $3, now() + make_interval(hours => $4))
                "#,
                item.user_id,
                item.purpose.as_str(),
                token_hash,
                TOKEN_TTL_HOURS,
            )
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
        }

        let Some((subject, body)) = render_mail(
            &item.purpose,
            &recipient,
            token.as_deref(),
            public_app_url,
            &item.params,
        ) else {
            // 理论上已被 is_supported_purpose 拦截；防御性处理。
            if fail(
                database.pool(),
                item.id,
                item.claim_token,
                item.attempts + 1,
                "unsupported_purpose",
            )
            .await?
            {
                stats.failed += 1;
                metrics::record_failure("email_outbox");
            } else {
                stats.lease_lost += 1;
            }
            continue;
        };
        let mail = RenderedMail {
            outbox_id: item.id,
            to: recipient,
            subject,
            body,
        };

        match mailer.send(&mail).await {
            Ok(()) => match complete(database.pool(), item.id, item.claim_token).await {
                Ok(true) => {
                    stats.sent += 1;
                    metrics::record_execution("email_outbox");
                }
                Ok(false) => {
                    stats.lease_lost += 1;
                    tracing::warn!(outbox_id = item.id, "邮件已发送但租约已被接管");
                }
                Err(error) => return Err(error),
            },
            Err(error) => {
                let kind = super::retry::classify_mail_error(&error);
                metrics::record_failure("email_outbox");
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
                            "邮件发送失败，稍后重试"
                        );
                    }
                    Ok(false) => stats.lease_lost += 1,
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(stats)
}

/// 邮件 worker：收到取消信号后不再领取新批次。
pub async fn worker_loop(
    database: Database,
    mailer: Arc<dyn Mailer>,
    resolver: Arc<dyn RecipientResolver>,
    public_app_url: String,
    shutdown: CancellationToken,
) {
    loop {
        if shutdown.is_cancelled() {
            break;
        }
        match run_batch(&database, &mailer, &resolver, &public_app_url, BATCH_SIZE).await {
            Ok(stats) if stats.claimed > 0 => {
                tracing::debug!(
                    claimed = stats.claimed,
                    sent = stats.sent,
                    failed = stats.failed,
                    released = stats.released,
                    "邮件批次完成"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::error!(error = %error, "邮件 outbox 处理失败");
                metrics::record_failure("email_outbox");
            }
        }
        let delay = Duration::from_millis(rand::rng().random_range(1_500..=2_500));
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

/// 生成 256 位令牌（hex，64 字符）。
///
/// 令牌直接来自操作系统 CSPRNG；取随机数失败（极罕见）时回退到 rand 的
/// 进程 CSPRNG（ThreadRng 同样由 OsRng 播种），绝不使用固定值或计数器。
fn generate_token() -> String {
    let mut bytes = [0_u8; 32];
    if let Err(error) = rand::rngs::OsRng.try_fill_bytes(&mut bytes) {
        tracing::warn!(error = %error, "操作系统随机数不可用，回退到进程 CSPRNG 生成邮件令牌");
        rand::rng().fill(&mut bytes);
    }
    let mut token = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write;
        // 写入 String 不会失败。
        let _ = write!(token, "{byte:02x}");
    }
    token
}

/// 只保存令牌哈希。
fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn email_batch_is_about_twenty() {
        assert_eq!(BATCH_SIZE, 20);
    }

    #[test]
    fn tokens_are_random_and_hashed() {
        let first = generate_token();
        let second = generate_token();
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
        let hash = hash_token(&first);
        assert_eq!(hash.len(), 32);
        assert_ne!(hash, first.as_bytes());
    }
}
