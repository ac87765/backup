//! `app_settings` 读写（add-admin-console design D1）。
//!
//! 两个键：
//! - `registration_mode`：`approval` / `open` / `closed`；
//! - `default_quota_bytes`：新用户默认额度；行不存在时回退到
//!   `BACKUP_DEFAULT_QUOTA_BYTES`，所以配置项在管理员尚未改过设置时仍然生效。
//!
//! 所有函数接收 `&mut PgConnection`：既能在请求事务内读（保证注册模式与
//! 注册写入一致），也能从连接池取一条连接读出。

use sqlx::PgConnection;
use uuid::Uuid;

pub const REGISTRATION_MODE_KEY: &str = "registration_mode";
pub const DEFAULT_QUOTA_KEY: &str = "default_quota_bytes";

pub const MODE_APPROVAL: &str = "approval";
pub const MODE_OPEN: &str = "open";
pub const MODE_CLOSED: &str = "closed";

/// 是否为合法的注册模式。
pub fn is_valid_registration_mode(mode: &str) -> bool {
    matches!(mode, MODE_APPROVAL | MODE_OPEN | MODE_CLOSED)
}

/// 读取注册模式；行缺失或值非法时回退到 `approval`（最安全的默认）。
pub async fn registration_mode(conn: &mut PgConnection) -> Result<String, sqlx::Error> {
    let value: Option<serde_json::Value> = sqlx::query_scalar!(
        "SELECT value FROM app_settings WHERE key = $1",
        REGISTRATION_MODE_KEY,
    )
    .fetch_optional(conn)
    .await?;
    Ok(match value {
        Some(serde_json::Value::String(mode)) if is_valid_registration_mode(&mode) => mode,
        _ => MODE_APPROVAL.to_owned(),
    })
}

/// 读取默认额度；行缺失或不是非负整数时返回 `None`，由调用方回退到配置。
pub async fn default_quota(conn: &mut PgConnection) -> Result<Option<i64>, sqlx::Error> {
    let value: Option<serde_json::Value> = sqlx::query_scalar!(
        "SELECT value FROM app_settings WHERE key = $1",
        DEFAULT_QUOTA_KEY,
    )
    .fetch_optional(conn)
    .await?;
    Ok(value
        .and_then(|value| value.as_i64())
        .filter(|value| *value >= 0))
}

/// 写入一个设置（调用方负责事务与审计）。
pub async fn set(
    conn: &mut PgConnection,
    key: &str,
    value: &serde_json::Value,
    updated_by: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
        INSERT INTO app_settings (key, value, updated_at, updated_by)
        VALUES ($1, $2, now(), $3)
        ON CONFLICT (key) DO UPDATE
        SET value = EXCLUDED.value,
            updated_at = now(),
            updated_by = EXCLUDED.updated_by
        "#,
        key,
        value,
        updated_by,
    )
    .execute(conn)
    .await?;
    Ok(())
}
