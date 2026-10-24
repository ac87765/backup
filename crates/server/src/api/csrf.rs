//! 跨站请求防护（user-accounts spec）。
//!
//! 所有改变状态的 API 请求（POST/PUT/PATCH/DELETE）必须携带
//! `X-Requested-With: backup-web`。该头无法被简单跨源表单构造，
//! 配合 SameSite=Strict Cookie 与 CORS 拒绝构成多层防护。

use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::error::ApiError;
use crate::auth::{CSRF_HEADER, CSRF_VALUE};

/// 对所有 `/api` 下的非安全方法检查自定义头。
pub async fn require_custom_header(request: Request, next: Next) -> Response {
    let changes_state = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if changes_state {
        let provided = request
            .headers()
            .get(CSRF_HEADER)
            .and_then(|value| value.to_str().ok());
        if provided != Some(CSRF_VALUE) {
            return ApiError::csrf_rejected().into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::routing::post;
    use tower::ServiceExt;

    async fn touch() -> &'static str {
        "ok"
    }

    fn app() -> Router {
        Router::new()
            .route("/x", post(touch))
            .layer(axum::middleware::from_fn(require_custom_header))
    }

    #[tokio::test]
    async fn post_without_header_is_rejected() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/x")
                    .body(Body::empty())
                    .expect("请求"),
            )
            .await
            .expect("响应");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn post_with_header_passes() {
        let response = app()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/x")
                    .header(CSRF_HEADER, CSRF_VALUE)
                    .body(Body::empty())
                    .expect("请求"),
            )
            .await
            .expect("响应");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
