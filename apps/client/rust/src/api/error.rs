//! 桥接层统一错误。
//!
//! 所有错误都转换为 [`BridgeError`]：稳定的英文错误码 + 中文提示。提示来自
//! `client-core` 已有的面向用户文案，不包含本地路径、文件名、令牌、密钥、
//! 包裹密钥或服务端请求体；错误码供界面做重试/跳转判断。

use client_core::desktop::BackupError;
use client_core::desktop::update::UpdateError;
use client_core::desktop::wechat::WechatError;
use client_core::{ClientError, StateError};

/// 桥接 API 的错误 DTO（Dart 侧实现 `FrbException`）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct BridgeError {
    /// 稳定错误码（与 `client-core` / 服务端错误码一致）。
    pub code: String,
    /// 面向用户的中文提示。
    pub message: String,
}

impl BridgeError {
    /// 构造错误。
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub(crate) fn invalid_id(kind: &str) -> Self {
        Self::new("invalid_id", format!("{kind} ID 无效"))
    }

    pub(crate) fn invalid_number(field: &str) -> Self {
        Self::new("invalid_number", format!("{field} 超出有效范围"))
    }

    pub(crate) fn internal(message: &str) -> Self {
        Self::new("internal_error", message)
    }
}

impl From<ClientError> for BridgeError {
    fn from(error: ClientError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.to_string(),
        }
    }
}

impl From<StateError> for BridgeError {
    fn from(_: StateError) -> Self {
        Self::new("state_error", "本地状态库操作失败")
    }
}

impl From<UpdateError> for BridgeError {
    fn from(error: UpdateError) -> Self {
        // `UpdateError` 自带稳定错误码与中文提示；`manual_install_required`
        // 由 `update_install` 在 macOS 无法替换应用时显式构造。
        Self::new(error.code(), error.user_message())
    }
}

impl From<BackupError> for BridgeError {
    fn from(error: BackupError) -> Self {
        match error {
            BackupError::Client(error) => error.into(),
            BackupError::State(_) => Self::new("state_error", "本地状态库操作失败"),
            BackupError::InvalidConfig(message) => {
                Self::new("invalid_config", format!("备份文件夹配置无效：{message}"))
            }
            BackupError::NotSignedIn => Self::new("not_signed_in", "需要登录并解锁后才能备份"),
            BackupError::FolderNotFound => Self::new("folder_not_found", "找不到备份文件夹"),
            BackupError::WatchUnavailable => {
                Self::new("watch_unavailable", "文件监听不可用，已降级为定时全量核对")
            }
            BackupError::AlreadyRunning => Self::new("already_running", "备份引擎已在运行"),
            BackupError::WechatRunning => Self::new(
                "wechat_running",
                "微信正在运行，请先完全退出微信（包括托盘图标）后再恢复",
            ),
            BackupError::SnapshotIncomplete => {
                Self::new("snapshot_incomplete", "所选数据库快照不完整，不能用于恢复")
            }
            BackupError::SnapshotCorrupted => Self::new(
                "snapshot_corrupted",
                "快照内容与清单不一致，恢复失败，原数据库已保留",
            ),
            BackupError::SnapshotNotFound => {
                Self::new("snapshot_not_found", "找不到所选的数据库快照")
            }
            BackupError::Wechat(error) => match error {
                WechatError::InvalidNote(message) => {
                    Self::new("invalid_note", format!("账号备注名无效：{message}"))
                }
                WechatError::InvalidAccount => Self::new("invalid_account", "微信账号目录名无效"),
                WechatError::State(_) => Self::new("state_error", "本地状态库操作失败"),
                _ => Self::new("wechat_error", error.to_string()),
            },
        }
    }
}
