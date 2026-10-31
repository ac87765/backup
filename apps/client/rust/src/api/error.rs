//! 桥接层统一错误。
//!
//! 所有错误都转换为 [`BridgeError`]：稳定的英文错误码 + 中文提示。提示来自
//! `client-core` 已有的面向用户文案，不包含本地路径、文件名、令牌、密钥、
//! 包裹密钥或服务端请求体；错误码供界面做重试/跳转判断。

use client_core::desktop::BackupError;
use client_core::{ApiErrorCode, ClientError};

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
        let code = match &error {
            ClientError::Api(failure) => api_code(&failure.code),
            ClientError::Network(_) => "network_error",
            ClientError::Crypto(_) => "crypto_error",
            ClientError::UnsupportedKdfAlgorithm => "unsupported_kdf_algorithm",
            ClientError::InvalidKdfParams => "invalid_kdf_params",
            ClientError::InvalidResponse(_) => "invalid_response",
            ClientError::InvalidBaseUrl => "invalid_server_url",
            ClientError::NoSession => "no_session",
            ClientError::UnlockFailed => "unlock_failed",
            ClientError::InvalidSession => "invalid_session",
            ClientError::SecretStore(_) => "secret_store_error",
            ClientError::State(_) => "state_error",
            ClientError::Io(_) => "local_io_error",
            ClientError::Locked => "locked",
            ClientError::AccountNotWritable(_) => "account_not_writable",
            ClientError::NodeKeyUnavailable => "node_key_unavailable",
            ClientError::FileChanged => "file_changed",
            ClientError::Transfer(transfer) => transfer.code.as_str(),
            ClientError::Integrity(_) => "integrity_error",
            ClientError::OperationNotAllowed => "operation_not_allowed",
            ClientError::Cancelled => "cancelled",
            ClientError::Internal(_) => "internal_error",
        };
        Self {
            code: code.to_owned(),
            message: error.to_string(),
        }
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
        }
    }
}

fn api_code(code: &ApiErrorCode) -> &str {
    code.as_str()
}
