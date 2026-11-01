//! 客户端更新（add-client-releases D1/D5）。
//!
//! 职责划分：
//! - [`UpdateChecker`]：用内置公钥验证 `/downloads/latest.json` 与 `.sig`，
//!   比较版本、记录防回滚水位、判断是否强制更新；
//! - [`UpdateDownloader`]：下载对应平台安装包到应用数据目录，流式计算
//!   SHA-256 并校验大小，校验通过才把 `*.partial` 改名为最终文件；
//! - [`install_update`]：Windows 静默安装 / macOS 替换应用并重启；
//! - [`UpdateScheduler`]：启动后 10 秒、之后每 24 小时的调度辅助。
//!
//! 更新检查不携带任何用户凭据：检查与下载各自使用独立的 `reqwest::Client`，
//! 不保存 Cookie、不设置 Authorization。日志只记录 URL/状态码，不记录响应体。

mod check;
mod download;
mod install;
mod schedule;

use std::time::{SystemTime, UNIX_EPOCH};

pub use check::{
    BANNER_DISMISSED_KEY, HIGHEST_SEEN_KEY, MAX_MANIFEST_BYTES, REQUEST_TIMEOUT, UpdateCheck,
    UpdateCheckOutcome, UpdateChecker, UpdateUnavailableReason,
};
pub use download::{
    UpdateCancellation, UpdateDownloadStatus, UpdateDownloader, UpdateFailure, UpdatePlatform,
    UpdateProgress, UpdateProgressSink, VerifiedUpdate,
};
pub use install::{InstallOutcome, install_update, open_update_dmg};
pub use schedule::{CHECK_INTERVAL, STARTUP_DELAY, UpdateScheduler};

use crate::state::StateError;

/// 更新流程错误。
///
/// 错误信息会直接展示给用户，因此只包含稳定文案，不含本地路径、响应体或
/// 服务端返回的任意文本。
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// 网络不可用或读取响应失败。
    #[error("网络连接失败，无法检查或下载更新，请稍后重试")]
    Network,

    /// 服务器返回非 200。
    #[error("服务器暂时无法提供更新（HTTP {0}）")]
    HttpStatus(u16),

    /// 清单签名无效（清单被篡改，或不是维护者签发的）。
    #[error("更新清单签名无效，已忽略")]
    InvalidSignature,

    /// 清单签名有效但格式不符合契约。
    #[error("更新清单无效，已忽略")]
    InvalidManifest,

    /// 下载内容的大小或 SHA-256 与已验签清单不一致。
    #[error("下载的文件不完整或被篡改，已删除，没有运行。可能是网络中断或被安全软件修改。")]
    Tampered,

    /// 用户取消。
    #[error("已取消下载")]
    Cancelled,

    /// 服务器地址不是有效的 HTTP(S) 地址。
    #[error("服务器地址无效")]
    InvalidServerUrl,

    /// 当前平台没有自动安装实现。
    #[error("当前平台不支持自动安装更新")]
    Unsupported,

    /// 安装/替换应用失败（文案必须是静态的，不能拼接本机路径）。
    #[error("安装更新失败：{0}")]
    InstallFailed(&'static str),

    /// 本地状态库错误。
    #[error(transparent)]
    State(#[from] StateError),

    /// 本地文件读写错误。
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl UpdateError {
    /// 桥接层使用的稳定错误码。
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Network | Self::HttpStatus(_) => "network",
            Self::InvalidSignature => "invalid_signature",
            Self::InvalidManifest => "invalid_manifest",
            Self::Tampered => "tampered",
            Self::Cancelled => "cancelled",
            Self::InvalidServerUrl => "invalid_server_url",
            Self::Unsupported => "unsupported",
            Self::InstallFailed(_) => "install_failed",
            Self::State(_) => "state_error",
            Self::Io(_) => "local_io_error",
        }
    }

    /// 面向用户的中文提示（与 `design/desktop/update.html` 的失败文案一致）。
    #[must_use]
    pub fn user_message(&self) -> String {
        match self {
            Self::State(_) => "本地状态库操作失败".to_owned(),
            Self::Io(_) => "无法读写本地更新文件，请检查磁盘空间与权限".to_owned(),
            other => other.to_string(),
        }
    }

    /// 转换为进度事件里的失败信息。
    #[must_use]
    pub fn failure(&self) -> UpdateFailure {
        UpdateFailure {
            code: self.code().to_owned(),
            message: self.user_message(),
        }
    }
}

/// 当前 Unix 秒数；系统时间早于 1970 时返回 0。
pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// 构造更新检查/下载共用的 HTTP 客户端。
///
/// - 不跟随重定向、无 Cookie、无默认 Authorization 头；
/// - User-Agent 只含应用名与版本；
/// - `timeout` 只用于清单请求（下载大文件不设总超时）。
pub(crate) fn build_update_http(
    user_agent: &str,
    timeout: Option<std::time::Duration>,
) -> Result<reqwest::Client, UpdateError> {
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(20))
        .user_agent(user_agent_header(user_agent));
    if let Some(timeout) = timeout {
        builder = builder.timeout(timeout);
    }
    builder.build().map_err(|_| UpdateError::Network)
}

/// User-Agent 值：`EncryptedBackup/<版本>`；过滤掉不能放进请求头的字符。
fn user_agent_header(version: &str) -> String {
    let safe: String = version
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+' | '_')
        })
        .collect();
    if safe.is_empty() {
        "EncryptedBackup/dev".to_owned()
    } else {
        format!("EncryptedBackup/{safe}")
    }
}

/// 把下载路径拼到服务器根上（`<server>/downloads/...`）。
pub(crate) fn downloads_url(
    server: &reqwest::Url,
    segments: &[&str],
) -> Result<reqwest::Url, UpdateError> {
    let mut url = server.clone();
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| UpdateError::InvalidServerUrl)?;
        // 服务器地址通常带根路径 `/`；过滤空片段避免拼出 `//downloads`。
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
    }
    Ok(url)
}

pub(crate) fn parse_server_url(server: &str) -> Result<reqwest::Url, UpdateError> {
    let url = reqwest::Url::parse(server.trim()).map_err(|_| UpdateError::InvalidServerUrl)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(UpdateError::InvalidServerUrl);
    }
    // 更新请求绝不能携带凭据（包括 URL 里的 userinfo）。
    if !url.username().is_empty() || url.password().is_some() {
        return Err(UpdateError::InvalidServerUrl);
    }
    Ok(url)
}

#[cfg(test)]
mod tests;
