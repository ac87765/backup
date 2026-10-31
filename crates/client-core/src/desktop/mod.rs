//! 桌面备份引擎（design D6）。
//!
//! 只在 `desktop` feature 下编译；iOS 构建不包含 `notify`/`globset` 与本模块。
//! 对外 API 分成三类：
//! - 配置：添加 / 修改 / 移除备份文件夹、预检、全局设置；
//! - 控制：暂停 / 恢复、立即备份、启动 / 停止调度器；
//! - 查询：每个文件夹状态、全局汇总、最近活动，以及 [`BackupEngine::subscribe`]
//!   的事件流。
//!
//! 微信备份（add-wechat-backup）复用同一引擎：`wechat` 模块负责只读发现与布局
//! 描述，[`BackupEngine::set_wechat_categories`] 把勾选的类别注册为微信来源，
//! [`BackupEngine::list_wechat_sources`] / [`BackupEngine::list_wechat_accounts`]
//! 提供状态，[`BackupEngine::preflight_wechat`] 提供开启前的大小与配额检查。
//!
//! 典型用法（界面层）：
//! ```no_run
//! # async fn example(client: client_core::Client) -> Result<(), client_core::desktop::BackupError> {
//! use client_core::desktop::{BackupEngine, BackupEngineConfig, BackupFolderDraft};
//!
//! let engine = BackupEngine::new(client, BackupEngineConfig::default())?;
//! engine.start().await?;
//! let folder = engine.add_folder(BackupFolderDraft::new("D:/工作资料")).await?;
//! let _ = folder;
//! # Ok(())
//! # }
//! ```

mod engine;
mod error;
mod filter;
mod paths;
mod types;
mod watcher;

pub mod wechat;

pub use engine::BackupEngine;
pub use error::BackupError;
pub use types::{
    BackupActivity, BackupActivityKind, BackupEngineConfig, BackupEvent, BackupFolder,
    BackupFolderDraft, BackupFolderState, BackupFolderStatus, BackupFolderUpdate,
    BackupPauseReason, BackupPreflight, BackupRunReport, BackupSettings, BackupSummary,
    BackupSummaryStatus, DEFAULT_DEBOUNCE, DEFAULT_FULL_SCAN_INTERVAL, DEFAULT_RETRY_INTERVAL,
    WechatAccountBackupStatus, WechatBackupPreflight, WechatBackupSource,
    WechatCategoryBackupStatus,
};
