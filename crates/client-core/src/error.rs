//! 强类型错误。
//!
//! 服务端的 `{code, message, request_id}` 错误体在这里被映射成
//! [`ApiErrorCode`]，调用方不需要比较字符串；未知/新增的 code 仍会以
//! [`ApiErrorCode::Other`] 保留，便于向前兼容。

use std::time::Duration;

use crate::client::AccountStatus;
use crate::model::TransferErrorCode;

/// 传输/文件操作的结构化错误。
///
/// `message` 只用于界面展示；日志不得输出可能包含本地路径或文件夹名称的
/// 详细错误（调用方应按错误码记录）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct TransferError {
    pub code: TransferErrorCode,
    pub message: String,
}

impl TransferError {
    #[must_use]
    pub fn new(code: TransferErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// 服务端稳定错误码。
///
/// 与 `crates/server/src/api/error.rs` 的 `ApiError` 构造函数一一对应。
/// 新增服务端错误码时只需在 [`ApiErrorCode::from_server_code`] 中追加映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiErrorCode {
    InternalError,
    NotFound,
    MethodNotAllowed,
    PayloadTooLarge,
    InvalidRequest,
    InvalidCursor,
    Conflict,
    NameConflict,
    TargetUnavailable,
    EmailNotVerified,
    AccountPending,
    AccountSuspended,
    AccountRejected,
    RegistrationClosed,
    ReauthRequired,
    CannotModifySelf,
    InvalidAccountState,
    QuotaExceeded,
    UploadSessionClosed,
    ChunkNotFound,
    ChunkSizeMismatch,
    UploadIncomplete,
    InvalidMove,
    OperationNotAllowed,
    InvalidKdfParams,
    InvalidKeyMaterial,
    InvalidCredentials,
    InvalidRecoveryKey,
    Unauthenticated,
    InvalidToken,
    CsrfRejected,
    AccountTemporarilyLocked,
    RateLimited,
    /// 服务端新增、客户端尚未识别的错误码。
    Other(String),
}

impl ApiErrorCode {
    /// 服务端使用的稳定字符串。
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::InternalError => "internal_error",
            Self::NotFound => "not_found",
            Self::MethodNotAllowed => "method_not_allowed",
            Self::PayloadTooLarge => "payload_too_large",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidCursor => "invalid_cursor",
            Self::Conflict => "conflict",
            Self::NameConflict => "name_conflict",
            Self::TargetUnavailable => "target_unavailable",
            Self::EmailNotVerified => "email_not_verified",
            Self::AccountPending => "account_pending",
            Self::AccountSuspended => "account_suspended",
            Self::AccountRejected => "account_rejected",
            Self::RegistrationClosed => "registration_closed",
            Self::ReauthRequired => "reauth_required",
            Self::CannotModifySelf => "cannot_modify_self",
            Self::InvalidAccountState => "invalid_account_state",
            Self::QuotaExceeded => "quota_exceeded",
            Self::UploadSessionClosed => "upload_session_closed",
            Self::ChunkNotFound => "chunk_not_found",
            Self::ChunkSizeMismatch => "chunk_size_mismatch",
            Self::UploadIncomplete => "upload_incomplete",
            Self::InvalidMove => "invalid_move",
            Self::OperationNotAllowed => "operation_not_allowed",
            Self::InvalidKdfParams => "invalid_kdf_params",
            Self::InvalidKeyMaterial => "invalid_key_material",
            Self::InvalidCredentials => "invalid_credentials",
            Self::InvalidRecoveryKey => "invalid_recovery_key",
            Self::Unauthenticated => "unauthenticated",
            Self::InvalidToken => "invalid_token",
            Self::CsrfRejected => "csrf_rejected",
            Self::AccountTemporarilyLocked => "account_temporarily_locked",
            Self::RateLimited => "rate_limited",
            Self::Other(code) => code,
        }
    }

    pub(crate) fn from_server_code(code: &str) -> Self {
        match code {
            "internal_error" => Self::InternalError,
            "not_found" => Self::NotFound,
            "method_not_allowed" => Self::MethodNotAllowed,
            "payload_too_large" => Self::PayloadTooLarge,
            "invalid_request" => Self::InvalidRequest,
            "invalid_cursor" => Self::InvalidCursor,
            "conflict" => Self::Conflict,
            "name_conflict" => Self::NameConflict,
            "target_unavailable" => Self::TargetUnavailable,
            "email_not_verified" => Self::EmailNotVerified,
            "account_pending" => Self::AccountPending,
            "account_suspended" => Self::AccountSuspended,
            "account_rejected" => Self::AccountRejected,
            "registration_closed" => Self::RegistrationClosed,
            "reauth_required" => Self::ReauthRequired,
            "cannot_modify_self" => Self::CannotModifySelf,
            "invalid_account_state" => Self::InvalidAccountState,
            "quota_exceeded" => Self::QuotaExceeded,
            "upload_session_closed" => Self::UploadSessionClosed,
            "chunk_not_found" => Self::ChunkNotFound,
            "chunk_size_mismatch" => Self::ChunkSizeMismatch,
            "upload_incomplete" => Self::UploadIncomplete,
            "invalid_move" => Self::InvalidMove,
            "operation_not_allowed" => Self::OperationNotAllowed,
            "invalid_kdf_params" => Self::InvalidKdfParams,
            "invalid_key_material" => Self::InvalidKeyMaterial,
            "invalid_credentials" => Self::InvalidCredentials,
            "invalid_recovery_key" => Self::InvalidRecoveryKey,
            "unauthenticated" => Self::Unauthenticated,
            "invalid_token" => Self::InvalidToken,
            "csrf_rejected" => Self::CsrfRejected,
            "account_temporarily_locked" => Self::AccountTemporarilyLocked,
            "rate_limited" => Self::RateLimited,
            other => Self::Other(other.to_owned()),
        }
    }
}

/// 一次失败的服务端请求（不含任何请求体或密钥材料）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ApiFailure {
    /// HTTP 状态码。
    pub status: u16,
    /// 强类型错误码。
    pub code: ApiErrorCode,
    /// 服务端面向用户的文案。
    pub message: String,
    /// 服务端请求 ID，便于运维检索日志。
    pub request_id: Option<String>,
    /// 仅 429/锁定响应携带。
    pub retry_after: Option<Duration>,
}

/// 凭据存储错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretStoreError {
    /// 平台凭据存储暂时不可用（例如钥匙串被锁定）。
    #[error("系统凭据存储不可用")]
    Unavailable,
    /// 平台后端返回错误；消息不包含任何密钥或令牌。
    #[error("系统凭据存储操作失败：{0}")]
    Backend(String),
    /// 凭据存储中的数据不是本客户端写入的格式。
    #[error("系统凭据存储中的数据格式无效")]
    Corrupt,
}

/// 客户端错误。
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// 网络错误（DNS、连接、TLS、超时等）。
    #[error("网络请求失败")]
    Network(#[source] reqwest::Error),

    /// 服务端返回了非 2xx 响应。
    #[error(transparent)]
    Api(#[from] ApiFailure),

    /// crypto-core 的密码学错误（解包失败等）。
    #[error(transparent)]
    Crypto(#[from] crypto_core::CryptoError),

    /// 服务端下发的 KDF 算法未知，需要升级客户端。
    #[error("服务端使用了当前客户端不支持的 KDF 算法，请升级客户端")]
    UnsupportedKdfAlgorithm,

    /// KDF 参数越界（疑似被降级或过大）。
    #[error("服务端下发的 KDF 参数不安全，已拒绝派生")]
    InvalidKdfParams,

    /// 服务端响应结构不符合契约。
    #[error("服务端响应格式无效：{0}")]
    InvalidResponse(&'static str),

    /// base URL 无法解析。
    #[error("服务端地址无效")]
    InvalidBaseUrl,

    /// 本地没有可用的会话令牌。
    #[error("需要登录")]
    NoSession,

    /// 已持有有效会话，但密码无法解包主密钥。
    #[error("密码错误")]
    UnlockFailed,

    /// 会话令牌格式非法（例如被本地存储篡改）。
    #[error("会话令牌格式无效")]
    InvalidSession,

    /// 凭据存储失败。
    #[error(transparent)]
    SecretStore(#[from] SecretStoreError),

    /// 本地状态库失败。
    #[error(transparent)]
    State(#[from] crate::state::StateError),

    /// 本地文件系统失败（不附加路径，避免把文件名写进日志）。
    #[error("本地文件操作失败")]
    Io(#[from] std::io::Error),

    /// 主密钥尚未加载（用户尚未输入密码解锁）。
    #[error("密钥库已锁定，请先解锁")]
    Locked,

    /// 当前账号不是 active，写操作被客户端直接拒绝（服务端也会拒绝）。
    #[error("当前账号状态不可写：{0}")]
    AccountNotWritable(AccountStatus),

    /// 缺少节点密钥；需要先刷新目录或重新加载版本。
    #[error("节点密钥尚未加载，请刷新目录后重试")]
    NodeKeyUnavailable,

    /// 文件在上传过程中被修改；调用方应稍后重试。
    #[error("文件在上传过程中被修改，已放弃本次上传")]
    FileChanged,

    /// 传输/文件操作错误（带稳定错误码）。
    #[error(transparent)]
    Transfer(#[from] TransferError),

    /// 完整性校验失败（版本绑定、块哈希、明文大小等）。
    #[error("完整性校验失败：{0}")]
    Integrity(&'static str),

    /// 当前操作不被允许（例如重命名根节点）。
    #[error("当前操作不被允许")]
    OperationNotAllowed,

    /// 用户取消传输。
    #[error("传输已取消")]
    Cancelled,

    /// 内部错误；不携带敏感信息。
    #[error("客户端内部错误：{0}")]
    Internal(&'static str),
}

impl ClientError {
    /// 强类型服务端错误码（仅 [`ClientError::Api`]）。
    #[must_use]
    pub fn api_code(&self) -> Option<&ApiErrorCode> {
        match self {
            Self::Api(failure) => Some(&failure.code),
            _ => None,
        }
    }

    /// HTTP 状态码（仅 [`ClientError::Api`]）。
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api(failure) => Some(failure.status),
            _ => None,
        }
    }

    /// 服务端是否明确表示会话已失效。
    #[must_use]
    pub fn is_unauthenticated(&self) -> bool {
        matches!(
            self,
            Self::Api(failure)
                if failure.status == 401 || failure.code == ApiErrorCode::Unauthenticated
        )
    }

    /// 是否是可退避重试的限流错误。
    #[must_use]
    pub fn is_rate_limited(&self) -> bool {
        matches!(
            self,
            Self::Api(failure)
                if failure.status == 429 && failure.code == ApiErrorCode::RateLimited
        )
    }

    /// 稳定错误码（界面判断与日志用；不含路径、令牌或服务端文案）。
    #[must_use]
    pub fn code(&self) -> &str {
        match self {
            Self::Api(failure) => failure.code.as_str(),
            Self::Network(_) => "network_error",
            Self::Crypto(_) => "crypto_error",
            Self::UnsupportedKdfAlgorithm => "unsupported_kdf_algorithm",
            Self::InvalidKdfParams => "invalid_kdf_params",
            Self::InvalidResponse(_) => "invalid_response",
            Self::InvalidBaseUrl => "invalid_server_url",
            Self::NoSession => "no_session",
            Self::UnlockFailed => "unlock_failed",
            Self::InvalidSession => "invalid_session",
            Self::SecretStore(_) => "secret_store_error",
            Self::State(_) => "state_error",
            Self::Io(_) => "local_io_error",
            Self::Locked => "locked",
            Self::AccountNotWritable(_) => "account_not_writable",
            Self::NodeKeyUnavailable => "node_key_unavailable",
            Self::FileChanged => "file_changed",
            Self::Transfer(transfer) => transfer.code.as_str(),
            Self::Integrity(_) => "integrity_error",
            Self::OperationNotAllowed => "operation_not_allowed",
            Self::Cancelled => "cancelled",
            Self::Internal(_) => "internal_error",
        }
    }

    /// 稳定传输错误码（用于进度事件与界面）。
    #[must_use]
    pub fn transfer_code(&self) -> TransferErrorCode {
        match self {
            Self::Api(failure) => match &failure.code {
                ApiErrorCode::NameConflict => TransferErrorCode::NameConflict,
                ApiErrorCode::TargetUnavailable => TransferErrorCode::TargetUnavailable,
                ApiErrorCode::QuotaExceeded => TransferErrorCode::QuotaExceeded,
                ApiErrorCode::UploadSessionClosed => TransferErrorCode::UploadSessionClosed,
                ApiErrorCode::ChunkNotFound => TransferErrorCode::ChunkNotFound,
                ApiErrorCode::ChunkSizeMismatch => TransferErrorCode::ChunkSizeMismatch,
                ApiErrorCode::UploadIncomplete => TransferErrorCode::UploadIncomplete,
                ApiErrorCode::RateLimited => TransferErrorCode::RateLimited,
                _ if failure.status == 429 => TransferErrorCode::RateLimited,
                _ if failure.status >= 500 => TransferErrorCode::Server,
                _ => TransferErrorCode::Other,
            },
            Self::Network(_) => TransferErrorCode::Network,
            Self::AccountNotWritable(_) => TransferErrorCode::AccountNotWritable,
            Self::Locked => TransferErrorCode::Locked,
            Self::FileChanged => TransferErrorCode::FileChanged,
            Self::Io(_) => TransferErrorCode::LocalIo,
            Self::Integrity(_) | Self::Crypto(_) => TransferErrorCode::Integrity,
            Self::Cancelled => TransferErrorCode::Cancelled,
            Self::Transfer(error) => error.code,
            _ => TransferErrorCode::Other,
        }
    }

    /// 是否值得稍后重试（网络、429、5xx、本地临时失败、上传中被修改）。
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Network(_) | Self::Io(_) | Self::FileChanged => true,
            Self::Api(failure) => failure.status == 429 || failure.status >= 500,
            Self::Transfer(error) => matches!(
                error.code,
                TransferErrorCode::Network
                    | TransferErrorCode::RateLimited
                    | TransferErrorCode::Server
                    | TransferErrorCode::FileChanged
            ),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn known_codes_roundtrip() {
        for code in [
            ApiErrorCode::InvalidCredentials,
            ApiErrorCode::Unauthenticated,
            ApiErrorCode::AccountPending,
            ApiErrorCode::RateLimited,
            ApiErrorCode::AccountTemporarilyLocked,
            ApiErrorCode::CsrfRejected,
        ] {
            assert_eq!(ApiErrorCode::from_server_code(code.as_str()), code);
        }
    }

    #[test]
    fn unknown_code_is_preserved() {
        assert_eq!(
            ApiErrorCode::from_server_code("something_new"),
            ApiErrorCode::Other("something_new".to_owned())
        );
    }

    #[test]
    fn unauthenticated_detection_covers_401_and_code() {
        let by_status = ClientError::Api(ApiFailure {
            status: 401,
            code: ApiErrorCode::Other("http_401".to_owned()),
            message: "需要登录".to_owned(),
            request_id: None,
            retry_after: None,
        });
        assert!(by_status.is_unauthenticated());

        let by_code = ClientError::Api(ApiFailure {
            status: 403,
            code: ApiErrorCode::Unauthenticated,
            message: "需要登录".to_owned(),
            request_id: None,
            retry_after: None,
        });
        assert!(by_code.is_unauthenticated());
    }
}
