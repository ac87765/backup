//! 数据库目录 `before-restore-` 后缀的时间戳。
//!
//! Windows 使用本地时间（`GetLocalTime`），与 design D4 的示例一致；其他平台
//! 没有额外的日期/时区依赖（AGENTS.md 不新增依赖），使用 UTC 并按同样格式
//! 输出。后缀只用于区分保留目录，不参与任何加密或一致性判断。
#![allow(unsafe_code)]

use std::time::SystemTime;

#[cfg(not(windows))]
use std::time::UNIX_EPOCH;

#[cfg(not(windows))]
use super::snapshot::format_compact_timestamp;

pub(crate) fn compact_now(now: SystemTime) -> String {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        let _ = now;
        let mut time = windows_sys::Win32::Foundation::SYSTEMTIME {
            wYear: 0,
            wMonth: 0,
            wDayOfWeek: 0,
            wDay: 0,
            wHour: 0,
            wMinute: 0,
            wSecond: 0,
            wMilliseconds: 0,
        };
        // SAFETY: GetLocalTime 只写入调用方提供的 SYSTEMTIME，不涉及指针所有权。
        unsafe { GetLocalTime(&mut time) };
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond
        )
    }
    #[cfg(not(windows))]
    {
        let seconds = now
            .duration_since(UNIX_EPOCH)
            .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
            .unwrap_or_default();
        format_compact_timestamp(seconds)
    }
}
