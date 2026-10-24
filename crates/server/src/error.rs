//! 启动期错误类型。
//!
//! 这些错误只会在进程启动/退出路径上出现，Display 文本必须足以定位配置项或
//! 数据库状态，同时不得携带任何密钥值。运行期请求错误见 `api::error`。

use crate::db::MigrationState;

/// 进程启动失败的原因。
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(transparent)]
    Database(#[from] crate::db::DatabaseError),
    #[error(transparent)]
    Storage(#[from] crate::storage::StorageError),
    #[error("数据库迁移状态不允许启动：{0}")]
    Migration(MigrationState),
}
