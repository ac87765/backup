//! 微信进程检测（design D3）。
//!
//! 聊天记录数据库只在微信**未运行**时做快照。检测是只读的：
//! - Windows 用 ToolHelp 进程快照（`CreateToolhelp32Snapshot`），只读取进程
//!   可执行文件名，不打开进程、不读取内存、不取命令行；存在
//!   `WeChat.exe` / `Weixin.exe` / `WeChatAppEx.exe` 任一即视为运行中
//!   （大小写不敏感；4.x 的内置浏览器子进程也在其中）。
//! - 其他平台返回 `false`；macOS 在第 5 组用 bundle id 实现。
//!
//! 测试通过 [`WechatProcessProbe`] 注入假实现，绝不依赖本机是否安装微信。

use super::WechatVersion;

/// 微信主进程 / 内置浏览器子进程名（大小写不敏感）。
#[cfg(any(windows, test))]
pub(crate) const WECHAT_PROCESS_NAMES: [&str; 3] = ["WeChat.exe", "Weixin.exe", "WeChatAppEx.exe"];

/// 可注入的微信进程探测器。
pub trait WechatProcessProbe: Send + Sync {
    /// 指定版本的微信是否正在运行。实现必须只读且不产生可见副作用。
    fn is_running(&self, version: WechatVersion) -> bool;
}

/// 当前平台的默认探测器。
#[must_use]
pub fn default_process_probe() -> std::sync::Arc<dyn WechatProcessProbe> {
    std::sync::Arc::new(PlatformWechatProcessProbe)
}

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub(crate) use windows::PlatformWechatProcessProbe;

#[cfg(not(windows))]
pub(crate) struct PlatformWechatProcessProbe;

#[cfg(not(windows))]
impl WechatProcessProbe for PlatformWechatProcessProbe {
    fn is_running(&self, _version: WechatVersion) -> bool {
        false
    }
}

/// 进程名是否是需要检测的微信进程（大小写不敏感）。
#[cfg(any(windows, test))]
pub(crate) fn is_wechat_process_name(name: &str) -> bool {
    WECHAT_PROCESS_NAMES
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn process_names_match_case_insensitively() {
        for name in [
            "WeChat.exe",
            "weixin.exe",
            "WECHATAPPEX.EXE",
            "WeChatAppEx.exe",
        ] {
            assert!(is_wechat_process_name(name), "{name}");
        }
        for name in [
            "WeChat",
            "Weixin.exe.bak",
            "WeChatAppEx",
            "notwechat.exe",
            "WeChat.exe.tmp",
        ] {
            assert!(!is_wechat_process_name(name), "{name}");
        }
    }

    /// 冒烟测试：只断言调用不 panic 且返回布尔值；不关心本机是否安装微信，
    /// 也不输出枚举到的任何进程名。
    #[cfg(windows)]
    #[test]
    fn windows_probe_returns_without_panicking() {
        let probe = PlatformWechatProcessProbe;
        let _ = probe.is_running(WechatVersion::Wechat3);
        let _ = probe.is_running(WechatVersion::Wechat4);
    }
}
