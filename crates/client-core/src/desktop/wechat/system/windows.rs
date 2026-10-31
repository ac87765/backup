//! Windows 系统位置读取（只读）。
//!
//! - 3.x 自定义位置：`HKCU\Software\Tencent\WeChat` 的 `FileSavePath`；
//! - “文档”：`SHGetKnownFolderPath(FOLDERID_Documents)`，不硬编码用户目录；
//! - APPDATA：环境变量。
//!
//! 注册表值可能包含用户路径，绝不写日志；调用失败时由上层决定忽略。
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, HANDLE,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW,
};
use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

use super::super::WechatError;
use super::WechatSystemLocations;

const WECHAT_REGISTRY_KEY: &str = r"Software\Tencent\WeChat";
const FILE_SAVE_PATH_VALUE: &str = "FileSavePath";

/// Windows 平台的生产实现（零大小）。
pub(crate) struct PlatformWechatSystemLocations;

impl WechatSystemLocations for PlatformWechatSystemLocations {
    fn documents_dir(&self) -> Result<Option<PathBuf>, WechatError> {
        documents_directory()
    }

    fn appdata_dir(&self) -> Result<Option<PathBuf>, WechatError> {
        Ok(std::env::var_os("APPDATA").map(PathBuf::from))
    }

    fn wechat_3_file_save_path(&self) -> Result<Option<String>, WechatError> {
        registry_file_save_path()
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn system_location_error() -> WechatError {
    WechatError::SystemLocation("读取 Windows 系统位置失败")
}

/// 读取 3.x 的 `FileSavePath`。键或值不存在时返回 `Ok(None)`。
fn registry_file_save_path() -> Result<Option<String>, WechatError> {
    let key = wide(WECHAT_REGISTRY_KEY);
    let value = wide(FILE_SAVE_PATH_VALUE);
    let mut size: u32 = 0;

    // SAFETY: key/value 都是以 NUL 结尾的 UTF-16；pvdata 为 null 时
    // RegGetValueW 只查询所需字节数，不写入缓冲区。
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
        return Err(system_location_error());
    }
    if size == 0 {
        return Ok(None);
    }

    // 注册表字符串按 UTF-16 存；向上取整分配 u16 缓冲区，保证指针对齐。
    let mut buffer = vec![0_u16; (size as usize).div_ceil(2)];
    let mut bytes = size;
    // SAFETY: buffer 至少可写 bytes 字节；bytes 是入参/出参，成功时更新为
    // 实际写入的字节数，不超过缓冲区大小。
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(system_location_error());
    }
    let units = (bytes as usize / 2).min(buffer.len());
    buffer.truncate(units);
    let value = String::from_utf16_lossy(&buffer);
    let value = value.trim_end_matches('\0').trim();
    if value.is_empty() || value.contains('\0') {
        return Ok(None);
    }
    Ok(Some(value.to_owned()))
}

/// 通过 Known Folder API 获取“文档”目录。
fn documents_directory() -> Result<Option<PathBuf>, WechatError> {
    let mut raw: *mut u16 = ptr::null_mut();
    // SAFETY: FOLDERID_Documents 是有效的 GUID；raw 是出参，成功时由 Shell
    // 通过 CoTaskMemAlloc 分配，读取后必须用 CoTaskMemFree 释放。
    let result = unsafe {
        SHGetKnownFolderPath(
            &FOLDERID_Documents,
            KF_FLAG_DEFAULT as u32,
            ptr::null_mut::<c_void>() as HANDLE,
            &mut raw,
        )
    };
    if result < 0 {
        // 文档要求：无论成功与否都要用 CoTaskMemFree 释放出参缓冲区。
        if !raw.is_null() {
            // SAFETY: raw 由 SHGetKnownFolderPath 分配（可能为空，已检查）。
            unsafe {
                CoTaskMemFree(raw.cast());
            }
        }
        return Err(system_location_error());
    }
    if raw.is_null() {
        return Ok(None);
    }

    // SAFETY: SHGetKnownFolderPath 成功时 raw 指向以 NUL 结尾的 UTF-16 字符串。
    let length = unsafe {
        let mut length = 0_usize;
        while *raw.add(length) != 0 {
            length += 1;
        }
        length
    };
    // SAFETY: raw 在 [0, length) 范围内有效；复制后立即释放原缓冲区。
    let value = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(raw, length)) };
    unsafe {
        CoTaskMemFree(raw.cast());
    }

    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(value)))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    /// 冒烟测试：只断言 API 不 panic、返回 Ok；不依赖本机是否安装微信，
    /// 也不打印读取到的任何路径或注册表值。
    #[test]
    fn windows_system_locations_read_without_panicking() {
        let locations = PlatformWechatSystemLocations;
        assert!(locations.documents_dir().is_ok());
        assert!(locations.appdata_dir().is_ok());
        assert!(locations.wechat_3_file_save_path().is_ok());
    }
}
