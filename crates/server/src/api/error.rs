//! 统一 API 错误。
//!
//! 所有 API 错误返回 JSON `{code, message, request_id}`（service-operations spec）。
//! 500 只返回 `code=internal_error` 与通用文案；详细原因只写入日志，
//! 客户端响应中不得出现 SQL、堆栈或内部标识。

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

use super::request_id;

/// API 错误响应体（对外契约）。
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErrorBody {
    /// 稳定的机器可读错误码。
    pub code: String,
    /// 面向用户的通用文案。
    pub message: String,
    /// 服务端为本请求生成的 ID，便于运维检索日志。
    pub request_id: String,
}

/// 领域错误 → HTTP 响应的统一封装。
#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    retry_after: Option<u64>,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
            retry_after: None,
        }
    }

    /// 未预期的内部错误：记录详细原因，只向客户端返回通用信息。
    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "请求处理失败");
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "服务器内部错误",
        )
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "资源不存在")
    }

    pub fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "请求方法不被允许",
        )
    }

    /// 请求体超过上限。注意：此错误在读取请求体之前产生。
    pub fn payload_too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "请求体过大",
        )
    }

    /// 请求格式或字段校验失败（包括出现禁止字段）。
    pub fn invalid_request() -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", "请求参数无效")
    }

    /// 分页游标格式非法（只编码排序键，服务端不会用游标绕过 user_id 过滤）。
    pub fn invalid_cursor() -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_cursor", "分页游标无效")
    }

    /// 乐观锁版本号不匹配：客户端应刷新后重试。
    pub fn conflict() -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "conflict",
            "资源已被修改，请刷新后重试",
        )
    }

    /// 同一目录内已存在相同名称令牌（服务端看不到明文名称）。
    pub fn name_conflict() -> Self {
        Self::new(StatusCode::CONFLICT, "name_conflict", "同名节点已存在")
    }

    /// 目标父文件夹不存在、已删除或不可作为父节点。
    pub fn target_unavailable() -> Self {
        Self::new(StatusCode::CONFLICT, "target_unavailable", "目标位置不可用")
    }

    /// 把文件夹移动到自身或其后代。
    pub fn invalid_move() -> Self {
        Self::new(StatusCode::CONFLICT, "invalid_move", "不能移动到自身或后代")
    }

    /// 对根文件夹或非法的节点类型执行了不允许的操作。
    pub fn operation_not_allowed() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "operation_not_allowed",
            "当前资源不允许该操作",
        )
    }

    /// KDF 参数缺失、算法未知或超出允许范围。
    pub fn invalid_kdf_params() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_kdf_params",
            "KDF 参数无效",
        )
    }

    /// 密钥/包裹材料的编码、长度或版本字节不合法。
    pub fn invalid_key_material() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_key_material",
            "密钥材料无效",
        )
    }

    /// 登录失败：邮箱不存在与认证密钥错误共用同一个错误。
    pub fn invalid_credentials() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "邮箱或密码错误",
        )
    }

    /// 恢复密钥校验失败（与客户端解包失败使用同一错误码）。
    pub fn invalid_recovery_key() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "invalid_recovery_key",
            "恢复密钥错误",
        )
    }

    /// 未认证/会话已失效。
    pub fn unauthenticated() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthenticated", "需要登录")
    }

    /// 邮箱验证令牌无效、过期或已使用。
    pub fn invalid_token() -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_token", "链接无效或已过期")
    }

    /// 缺少/错误的 CSRF 自定义头。
    pub fn csrf_rejected() -> Self {
        Self::new(StatusCode::FORBIDDEN, "csrf_rejected", "缺少必要的请求头")
    }

    /// 账号因连续登录失败被临时锁定。
    pub fn account_locked(retry_after: u64) -> Self {
        let mut error = Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "account_temporarily_locked",
            "账号已被临时锁定，请稍后重试",
        );
        error.retry_after = Some(retry_after.max(1));
        error
    }

    /// IP 维度限流。
    pub fn too_many_requests(retry_after: u64) -> Self {
        let mut error = Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "请求过于频繁，请稍后重试",
        );
        error.retry_after = Some(retry_after.max(1));
        error
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn retry_after(&self) -> Option<u64> {
        self.retry_after
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            code: self.code.to_owned(),
            message: self.message.to_owned(),
            request_id: request_id::current(),
        };
        let mut response = (self.status, Json(body)).into_response();
        if let Some(seconds) = self.retry_after
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_error_exposes_only_generic_information() {
        let error = ApiError::internal("SELECT * FROM users WHERE email = 'secret@example.com'");
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        // 结构体本身不保存原始错误文本，从类型上避免泄露。
        let debug = format!("{error:?}");
        assert!(!debug.contains("SELECT"));
        assert!(!debug.contains("secret@example.com"));
    }
}
