//! 系统位置来源（design D1）。
//!
//! 注册表值、APPDATA 与“文档”目录都通过 [`WechatSystemLocations`] 注入。
//! 生产实现只在 Windows 编译；其他平台（含 Linux CI）返回“没有系统位置”，
//! 手动目录与账号识别逻辑仍然完全可用、可测试。
//!
//! trait 方法返回 `Result` 而不是直接 `Option`：注册表 / Known Folder 调用
//! 失败时调用方可以选择忽略，而 Windows 冒烟测试可以断言“调用返回 Ok”。

use std::path::PathBuf;

use super::WechatError;

/// 微信位置发现需要的系统信息。
pub trait WechatSystemLocations: Send + Sync {
    /// Windows Known Folder `FOLDERID_Documents`（“文档”）。
    fn documents_dir(&self) -> Result<Option<PathBuf>, WechatError>;
    /// `%APPDATA%`（4.x 的 `Tencent/xwechat/config` 位于其下）。
    fn appdata_dir(&self) -> Result<Option<PathBuf>, WechatError>;
    /// 3.x 注册表 `HKCU\Software\Tencent\WeChat` 的 `FileSavePath` 原始值。
    /// 可能是 `MyDocument:`，也可能是绝对路径。
    fn wechat_3_file_save_path(&self) -> Result<Option<String>, WechatError>;
}

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub(crate) use windows::PlatformWechatSystemLocations;

/// 其他平台没有可用的系统位置；手动目录仍然可用。
#[cfg(not(windows))]
pub(crate) struct PlatformWechatSystemLocations;

#[cfg(not(windows))]
impl WechatSystemLocations for PlatformWechatSystemLocations {
    fn documents_dir(&self) -> Result<Option<PathBuf>, WechatError> {
        Ok(None)
    }

    fn appdata_dir(&self) -> Result<Option<PathBuf>, WechatError> {
        Ok(None)
    }

    fn wechat_3_file_save_path(&self) -> Result<Option<String>, WechatError> {
        Ok(None)
    }
}
