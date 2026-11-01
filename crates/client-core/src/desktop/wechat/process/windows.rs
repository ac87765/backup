//! Windows 进程检测（ToolHelp，只读）。
//!
//! 只枚举进程快照里的可执行文件名；不打开进程句柄、不读内存、不取命令行，
//! 也不记录任何进程名到日志。快照句柄无论成功与否都会关闭。
#![allow(unsafe_code)]

use std::mem::size_of;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};

use super::{WechatProcessProbe, WechatVersion, is_wechat_process_name};

/// Windows 平台的生产实现（零大小）。
pub(crate) struct PlatformWechatProcessProbe;

impl WechatProcessProbe for PlatformWechatProcessProbe {
    fn is_running(&self, _version: WechatVersion) -> bool {
        // 3.x / 4.x 共用同一组可执行文件名，版本参数不影响检测结果。
        wechat_process_running()
    }
}

fn wechat_process_running() -> bool {
    // SAFETY: 只创建进程快照；没有待遍历进程时返回快照句柄或失败句柄。
    let snapshot: HANDLE = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
        // 枚举失败时无法证明微信未运行。数据库一致性优先：按“运行中”处理，
        // 让快照等待而不是复制可能处于写入中的数据库；下一次检查会重试。
        return true;
    }

    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..PROCESSENTRY32W::default()
    };
    // SAFETY: snapshot 是有效快照句柄；entry 已设置 dwSize 且指向可写结构。
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    let mut found = false;
    while has_entry {
        let length = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if is_wechat_process_name(&name) {
            found = true;
            break;
        }
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: 同上；Process32NextW 在遍历结束后返回 0。
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }

    // SAFETY: snapshot 是 CreateToolhelp32Snapshot 返回且尚未关闭的句柄。
    unsafe {
        CloseHandle(snapshot);
    }
    found
}
