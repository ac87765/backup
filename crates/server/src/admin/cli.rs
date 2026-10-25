//! `server admin grant|revoke|list`：唯一可以修改管理员身份的命令行入口。
//!
//! 只连接数据库，不需要 OSS/SMTP 等运行期配置；所有变更与审计记录同事务提交。
//! 输出只写 stdout（面向运维），不通过 tracing 记录邮箱。

use anyhow::Context;
use clap::Subcommand;

use super::{CLI_SOURCE_IP, SYSTEM_ADMIN_ID, action, format_quota, insert_audit};
use crate::{Database, MigrateConfig};

/// 管理员相关子命令。
#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// 授予管理员身份。
    Grant {
        /// 要授予的账号邮箱。
        email: String,
    },
    /// 撤销管理员身份。
    Revoke {
        /// 要撤销的账号邮箱。
        email: String,
    },
    /// 列出当前全部管理员。
    List,
}

/// 执行 `server admin` 子命令。
pub async fn run(command: AdminCommand) -> anyhow::Result<()> {
    let config = MigrateConfig::load().context("加载数据库配置失败")?;
    let database = Database::connect(&config.database)
        .await
        .context("连接数据库失败")?;
    let result = match command {
        AdminCommand::Grant { email } => grant(&database, &email).await,
        AdminCommand::Revoke { email } => revoke(&database, &email).await,
        AdminCommand::List => list(&database).await,
    };
    database.close().await;
    result
}

fn normalize_email(value: &str) -> anyhow::Result<String> {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty()
        || !normalized.contains('@')
        || normalized.contains(char::is_whitespace)
    {
        anyhow::bail!("邮箱格式无效");
    }
    Ok(normalized)
}

async fn find_user(database: &Database, email: &str) -> anyhow::Result<Option<(uuid::Uuid, bool)>> {
    let row = sqlx::query!(
        r#"
        SELECT id, is_admin
        FROM users
        WHERE email = $1::text::citext
        "#,
        email,
    )
    .fetch_optional(database.pool())
    .await
    .context("查询账号失败")?;
    Ok(row.map(|row| (row.id, row.is_admin)))
}

async fn grant(database: &Database, email: &str) -> anyhow::Result<()> {
    let email = normalize_email(email)?;
    let Some((user_id, is_admin)) = find_user(database, &email).await? else {
        anyhow::bail!("未找到该邮箱对应的账号");
    };
    if is_admin {
        println!("{email} 已经是管理员，未做修改");
        return Ok(());
    }

    let mut tx = database.pool().begin().await.context("开始事务失败")?;
    sqlx::query!(
        "UPDATE users SET is_admin = true, updated_at = now() WHERE id = $1",
        user_id,
    )
    .execute(&mut *tx)
    .await
    .context("授予管理员失败")?;
    insert_audit(
        &mut tx,
        SYSTEM_ADMIN_ID,
        action::GRANT_ADMIN,
        Some(user_id),
        &serde_json::json!({ "via": "cli" }),
        CLI_SOURCE_IP,
    )
    .await
    .context("写入审计日志失败")?;
    tx.commit().await.context("提交事务失败")?;
    println!("{email} 已成为管理员");
    Ok(())
}

async fn revoke(database: &Database, email: &str) -> anyhow::Result<()> {
    let email = normalize_email(email)?;
    let Some((user_id, is_admin)) = find_user(database, &email).await? else {
        anyhow::bail!("未找到该邮箱对应的账号");
    };
    if !is_admin {
        println!("{email} 不是管理员，未做修改");
        return Ok(());
    }

    let mut tx = database.pool().begin().await.context("开始事务失败")?;
    sqlx::query!(
        "UPDATE users SET is_admin = false, updated_at = now() WHERE id = $1",
        user_id,
    )
    .execute(&mut *tx)
    .await
    .context("撤销管理员失败")?;
    insert_audit(
        &mut tx,
        SYSTEM_ADMIN_ID,
        action::REVOKE_ADMIN,
        Some(user_id),
        &serde_json::json!({ "via": "cli" }),
        CLI_SOURCE_IP,
    )
    .await
    .context("写入审计日志失败")?;
    tx.commit().await.context("提交事务失败")?;
    println!("{email} 已不再是管理员");
    Ok(())
}

async fn list(database: &Database) -> anyhow::Result<()> {
    let rows = sqlx::query!(
        r#"
        SELECT email::text AS "email!", status, quota_bytes
        FROM users
        WHERE is_admin = true
        ORDER BY email
        "#,
    )
    .fetch_all(database.pool())
    .await
    .context("查询管理员列表失败")?;
    if rows.is_empty() {
        println!("当前没有管理员");
        return Ok(());
    }
    for row in rows {
        println!(
            "{}\t{}\t{}",
            row.email,
            row.status,
            format_quota(row.quota_bytes)
        );
    }
    Ok(())
}
