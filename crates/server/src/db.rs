//! 数据库连接池与迁移校验。
//!
//! 迁移文件在编译期由 `sqlx::migrate!` 嵌入二进制（`migrations/` 位于仓库根目录）。
//! `serve` 启动时只校验版本，不执行迁移：
//! - 数据库落后于代码（有未应用的迁移）→ 拒绝启动；
//! - 已应用迁移的校验和与代码不一致 → 拒绝启动；
//! - 数据库领先于代码（存在代码不认识的已应用版本）→ 允许启动（向前兼容的发布要求）。
//!
//! 执行迁移只允许走显式的 `server migrate` 子命令（design D15）。

use std::collections::HashMap;
use std::time::Duration;

use sqlx::PgPool;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::postgres::PgPoolOptions;

use crate::config::DatabaseConfig;

/// 编译期嵌入的迁移集合。
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// 启动期数据库错误。
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("无法连接数据库")]
    Connect(#[source] sqlx::Error),
    #[error("数据库查询失败")]
    Query(#[source] sqlx::Error),
    #[error("执行数据库迁移失败")]
    Migrate(#[source] MigrateError),
}

/// `_sqlx_migrations` 中的一行。
#[derive(Debug, sqlx::FromRow)]
struct AppliedMigration {
    version: i64,
    checksum: Vec<u8>,
    success: bool,
}

/// 迁移版本检查结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationState {
    /// 代码中的迁移都已应用；`database_ahead` 是数据库比代码多的版本（允许）。
    Ready { database_ahead: Vec<i64> },
    /// 数据库缺少这些版本，必须拒绝启动。
    Behind { missing: Vec<i64> },
    /// 这些已应用迁移的校验和与代码不一致，必须拒绝启动。
    ChecksumMismatch { versions: Vec<i64> },
    /// 这些迁移在数据库中被标记为执行失败，必须拒绝启动。
    Failed { versions: Vec<i64> },
}

impl MigrationState {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    /// 数据库领先于代码的版本（允许启动，仅用于日志）。
    pub fn database_ahead(&self) -> &[i64] {
        match self {
            Self::Ready { database_ahead } => database_ahead,
            _ => &[],
        }
    }
}

impl std::fmt::Display for MigrationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready { database_ahead } if database_ahead.is_empty() => {
                f.write_str("迁移版本一致")
            }
            Self::Ready { database_ahead } => {
                write!(f, "迁移版本一致（数据库领先版本: {database_ahead:?}）")
            }
            Self::Behind { missing } => {
                write!(
                    f,
                    "数据库缺少迁移版本 {missing:?}，请先执行 `server migrate`"
                )
            }
            Self::ChecksumMismatch { versions } => {
                write!(f, "已应用迁移的校验和不一致（版本 {versions:?}）")
            }
            Self::Failed { versions } => {
                write!(f, "数据库中存在失败的迁移（版本 {versions:?}）")
            }
        }
    }
}

/// 数据库句柄（连接池）。
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// 按配置建立连接池（启动期调用）。
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(config.url.expose())
            .await
            .map_err(DatabaseError::Connect)?;
        Ok(Self { pool })
    }

    /// 直接使用已有连接池（测试或未来的连接复用入口）。
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 只创建连接池而不建立连接（测试构造 AppState、或延迟连接时使用）。
    pub fn connect_lazy(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect_lazy(config.url.expose())
            .map_err(DatabaseError::Connect)?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// 执行全部未应用的迁移（只允许 `server migrate` / 测试调用）。
    pub async fn migrate(&self) -> Result<(), DatabaseError> {
        MIGRATOR
            .run(&self.pool)
            .await
            .map_err(DatabaseError::Migrate)
    }

    /// 校验数据库迁移状态。
    pub async fn migration_state(&self) -> Result<MigrationState, DatabaseError> {
        let applied = self.applied_migrations().await?;
        let applied_by_version: HashMap<i64, AppliedMigration> = applied
            .into_iter()
            .map(|migration| (migration.version, migration))
            .collect();

        let embedded_versions: std::collections::HashSet<i64> =
            MIGRATOR.migrations.iter().map(|m| m.version).collect();

        let mut missing = Vec::new();
        let mut mismatched = Vec::new();
        let mut failed = Vec::new();
        for migration in MIGRATOR.migrations.iter() {
            match applied_by_version.get(&migration.version) {
                None => missing.push(migration.version),
                Some(row) if !row.success => failed.push(migration.version),
                Some(row) if row.checksum != migration.checksum.as_ref() => {
                    mismatched.push(migration.version);
                }
                Some(_) => {}
            }
        }
        let mut database_ahead: Vec<i64> = applied_by_version
            .keys()
            .filter(|version| !embedded_versions.contains(version))
            .copied()
            .collect();
        database_ahead.sort_unstable();

        if !failed.is_empty() {
            failed.sort_unstable();
            return Ok(MigrationState::Failed { versions: failed });
        }
        if !missing.is_empty() {
            missing.sort_unstable();
            return Ok(MigrationState::Behind { missing });
        }
        if !mismatched.is_empty() {
            mismatched.sort_unstable();
            return Ok(MigrationState::ChecksumMismatch {
                versions: mismatched,
            });
        }
        Ok(MigrationState::Ready { database_ahead })
    }

    async fn applied_migrations(&self) -> Result<Vec<AppliedMigration>, DatabaseError> {
        // 迁移表在从未执行过迁移的库中不存在：视为“落后于代码”。
        match sqlx::query_as!(
            AppliedMigration,
            "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version"
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => Ok(rows),
            Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("42P01") => {
                Ok(Vec::new())
            }
            Err(error) => Err(DatabaseError::Query(error)),
        }
    }

    /// 数据库可连接性检查（`/readyz`）。
    pub async fn ping(&self) -> Result<(), DatabaseError> {
        sqlx::query_scalar!("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(DatabaseError::Query)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
