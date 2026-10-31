//! `client-core` —— Windows / macOS 原生客户端的 Rust 核心。
//!
//! 本 crate 不依赖 Flutter，可单独测试；界面层只拿到展示数据
//! （邮箱、状态、是否管理员等），永远拿不到密钥字节。
//!
//! 第 1 组实现 API 客户端、认证与凭据存储：
//! - [`Client`]：登录、解锁、`/auth/me`、退出登录；
//! - [`SecretStore`]：系统钥匙串（macOS Keychain / Windows DPAPI + 凭据管理器）
//!   与内存实现；
//! - 429 退避、会话 Cookie、CSRF 头与强类型错误码。
//!
//! 平台无关代码与桌面专有能力通过 `desktop` feature 分开；第 2 组起的文件监听、
//! 备份调度等才会放入该 feature。

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

mod api;
mod auth;
mod client;
mod clock;
mod files;
mod hex;
mod io_path;
mod model;
mod secret_store;
mod state;
mod transfer;

pub mod error;

pub use client::{Account, AccountStatus, Client, ClientBuilder, Clock, SleepFuture, SystemClock};
pub use error::{ApiErrorCode, ApiFailure, ClientError, TransferError};
pub use io_path::{sanitize_local_name, validate_name};
pub use model::{
    ConflictPolicy, DownloadOutcome, FolderCounts, FolderDownloadOutcome, FolderJobStatus,
    FolderProgress, FolderUploadOutcome, NodeKind, NodeSort, NodeSortField, NodeView,
    PendingUpload, PendingUploadStatus, PermanentDeleteOutcome, RestoreTarget, SkippedItem,
    TransferDirection, TransferErrorCode, TransferFailure, TransferProgress, TransferStatus,
    TrashItem, UploadOutcome, UsageView, VersionView,
};
pub use secret_store::{
    MemorySecretStore, SecretStore, SecretStoreError, SessionToken, platform_secret_store,
};
pub use state::{DATABASE_FILE_NAME, SCHEMA_VERSION, StateError, StateStore};
pub use transfer::{DownloadTask, FolderDownloadTask, FolderUploadTask, UploadTask};

/// 桌面专有能力的入口（design D1）。
///
/// 只在 `desktop` feature 下编译：文件监听、备份调度与本地备份索引。iOS 构建
/// 不启用该 feature，因此不会引入 `notify` / `globset`，也不会编译这些代码。
#[cfg(feature = "desktop")]
pub mod desktop;
