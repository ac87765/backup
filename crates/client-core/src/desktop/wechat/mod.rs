//! 微信备份来源发现（`desktop` feature，design D1/D2）。
//!
//! 本模块只做**只读发现**：定位微信 3.x / 4.x 的数据根目录、识别账号目录、
//! 统计各类别的大小与最近修改时间，并提供仅为本地展示的备注名。它不读取
//! 任何文件内容（4.x 的 `.ini` 例外，只取第一行路径）、不解密数据库、不写
//! 微信目录。
//!
//! 位置来源按 design D1 的顺序：手动目录 → 注册表（3.x）/ 4.x 配置 →
//! 默认位置。同一目录只出现一次，来源以最先命中的那个为准。系统位置通过
//! [`WechatSystemLocations`] 注入，测试使用假实现。
//!
//! 大目录的遍历在 [`tokio::task::spawn_blocking`] 中执行，调用方的异步
//! 运行时不会被阻塞。

mod layout;
mod notes;
mod process;
mod scan;
mod system;

pub(crate) mod backup;
pub(crate) mod exclude;
pub(crate) mod snapshot;

#[cfg(test)]
mod tests;

pub use layout::{
    MACOS_LAYOUTS, WECHAT_3_LAYOUT, WECHAT_4_LAYOUT, WINDOWS_LAYOUTS, WechatCategory,
    WechatCategoryLayout, WechatLayout, WechatVersion, layout_for,
};
pub use notes::{WECHAT_NOTE_MAX_CHARS, WechatAccountNotes};
pub use process::{WechatProcessProbe, default_process_probe};
pub use system::WechatSystemLocations;

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::state::StateError;

/// 账号位置的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WechatSource {
    /// 用户手动添加的目录。
    Manual,
    /// Windows 3.x 注册表 `FileSavePath`。
    Registry,
    /// Windows 4.x `%APPDATA%\Tencent\xwechat\config\*.ini`。
    ConfigFile,
    /// “文档”下的默认位置。
    DefaultLocation,
}

impl WechatSource {
    /// 界面展示用的来源名称。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Manual => "手动添加",
            Self::Registry => "注册表",
            Self::ConfigFile => "微信配置",
            Self::DefaultLocation => "默认位置",
        }
    }
}

/// 一个类别的发现结果（只包含展示与后续备份所需的元数据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatCategoryInfo {
    pub category: WechatCategory,
    /// 该类别在本机上的绝对来源路径；可能为空（例如没有迁移备份）。
    pub paths: Vec<PathBuf>,
    pub size_bytes: u64,
    pub file_count: u64,
    /// 类别内最新文件修改时间（Unix 纳秒）；没有文件时为 `None`。
    pub last_modified_ns: Option<i64>,
    /// 来源路径是否存在（空类别目录也算存在）。
    pub exists: bool,
}

/// 发现到的一个微信账号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatAccount {
    /// 账号目录名（例如 `wxid_test0001`）。
    pub account_name: String,
    pub version: WechatVersion,
    /// 数据根目录（`...\WeChat Files` 或 `...\xwechat_files`）。
    pub data_root: PathBuf,
    /// 账号目录本身。
    pub account_dir: PathBuf,
    pub source: WechatSource,
    pub categories: Vec<WechatCategoryInfo>,
    pub total_size_bytes: u64,
    pub total_file_count: u64,
    /// 各类别文件中最新的修改时间（Unix 纳秒）。
    pub last_modified_ns: Option<i64>,
    /// 最近修改的账号标为“可能是当前登录的账号”。
    pub is_current_login_candidate: bool,
    /// 手机迁移备份是否有内容（文件数 > 0）。
    pub has_phone_migration_backup: bool,
}

impl WechatAccount {
    /// 按类别读取发现结果。
    #[must_use]
    pub fn category(&self, category: WechatCategory) -> Option<&WechatCategoryInfo> {
        self.categories
            .iter()
            .find(|info| info.category == category)
    }
}

/// 微信来源发现错误。错误信息不包含本地路径或账号目录名。
#[derive(Debug, thiserror::Error)]
pub enum WechatError {
    #[error("微信目录扫描失败")]
    ScanFailed,
    #[error("读取微信系统位置失败：{0}")]
    SystemLocation(&'static str),
    #[error("微信账号目录名无效")]
    InvalidAccount,
    #[error("账号备注名无效：{0}")]
    InvalidNote(&'static str),
    #[error(transparent)]
    State(#[from] StateError),
}

/// 微信来源发现入口。
///
/// 生产环境用 [`WechatLocator::new`]（Windows 上读取注册表 / Known Folder）；
/// 测试与上层可以用 [`WechatLocator::with_system_locations`] 注入假实现。
#[derive(Clone)]
pub struct WechatLocator {
    system: Arc<dyn WechatSystemLocations>,
}

impl fmt::Debug for WechatLocator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WechatLocator")
            .finish_non_exhaustive()
    }
}

impl WechatLocator {
    /// 使用当前平台的系统位置来源。
    #[must_use]
    pub fn new() -> Self {
        Self {
            system: Arc::new(system::PlatformWechatSystemLocations),
        }
    }

    /// 注入自定义系统位置来源（测试或宿主环境提供）。
    #[must_use]
    pub fn with_system_locations(system: Arc<dyn WechatSystemLocations>) -> Self {
        Self { system }
    }

    /// 发现所有微信账号。
    ///
    /// `manual_directories` 可以是数据根目录、其父目录，或单个账号目录。
    /// 无账号时返回空列表（界面据此显示“没有找到微信数据”）。遍历在
    /// `spawn_blocking` 中执行，不阻塞异步调用方。
    pub async fn discover(
        &self,
        manual_directories: &[PathBuf],
    ) -> Result<Vec<WechatAccount>, WechatError> {
        let system = Arc::clone(&self.system);
        let manual = manual_directories.to_vec();
        tokio::task::spawn_blocking(move || scan::discover_blocking(system.as_ref(), &manual))
            .await
            .map_err(|_| WechatError::ScanFailed)?
    }

    /// 只发现微信数据根目录（`xwechat_files` / `WeChat Files` 的实际路径）。
    ///
    /// 普通备份文件夹自动排除微信数据（design D2）只需要根目录，不需要统计
    /// 每个账号的类别大小，因此这里不遍历账号内的文件。
    pub async fn discover_data_roots(
        &self,
        manual_directories: &[PathBuf],
    ) -> Result<Vec<PathBuf>, WechatError> {
        let system = Arc::clone(&self.system);
        let manual = manual_directories.to_vec();
        tokio::task::spawn_blocking(move || scan::discover_roots_blocking(system.as_ref(), &manual))
            .await
            .map_err(|_| WechatError::ScanFailed)?
    }
}

impl Default for WechatLocator {
    fn default() -> Self {
        Self::new()
    }
}
