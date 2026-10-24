//! 统一 API 错误。
//!
//! 所有 API 错误返回 JSON `{code, message, request_id}`（service-operations spec）。
//! 500 只返回 `code=internal_error` 与通用文案；详细原因只写入日志，
//! 客户端响应中不得出现 SQL、堆栈或内部标识。

use axum::Json;
use axum::http::StatusCode;
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
}

impl ApiError {
    /// 未预期的内部错误：记录详细原因，只向客户端返回通用信息。
    pub fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(error = %error, "请求处理失败");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: "服务器内部错误",
        }
    }

    pub fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: "资源不存在",
        }
    }

    pub fn method_not_allowed() -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            code: "method_not_allowed",
            message: "请求方法不被允许",
        }
    }

    /// 请求体超过上限。注意：此错误在读取请求体之前产生。
    pub fn payload_too_large() -> Self {
        Self {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            code: "payload_too_large",
            message: "请求体过大",
        }
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            code: self.code.to_owned(),
            message: self.message.to_owned(),
            request_id: request_id::current(),
        };
        (self.status, Json(body)).into_response()
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
