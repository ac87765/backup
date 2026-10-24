//! 静态文件响应头。
//!
//! - 带内容哈希的文件名（Vite 产物，如 `index-abc123.js`）设置长期不可变缓存。
//! - `index.html`（以及 SPA 回退命中的 HTML）设置 `no-cache`，保证发布后立即生效。
//! - `.wasm` 明确返回 `application/wasm`，避免浏览器用错 MIME 拒绝流式编译。
//!
//! 只处理非 `/api` 路径；API 响应不写静态缓存头。

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::middleware::Next;
use axum::response::Response;

/// 静态文件响应头中间件。
pub async fn add_static_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let mut response = next.run(request).await;

    if path.starts_with("/api") {
        return response;
    }

    let is_html = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));

    let headers = response.headers_mut();
    if path.to_ascii_lowercase().ends_with(".wasm") {
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/wasm"));
    }
    let cache_control = if is_html {
        HeaderValue::from_static("no-cache")
    } else if has_content_hash(&path) {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    } else {
        // 无哈希的静态文件（favicon 等）也不能长期缓存，避免发布后陈旧。
        HeaderValue::from_static("no-cache")
    };
    headers.insert(CACHE_CONTROL, cache_control);

    response
}

/// 判断文件名是否带 Vite 风格的内容哈希：`name-<8+ 位 [A-Za-z0-9_-]>.ext`。
fn has_content_hash(path: &str) -> bool {
    let Some(file_name) = path.rsplit('/').next() else {
        return false;
    };
    let Some((stem, _extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    let Some((_name, hash)) = stem.rsplit_once('-') else {
        return false;
    };
    hash.len() >= 8
        && hash.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

#[cfg(test)]
mod tests {
    use super::has_content_hash;

    #[test]
    fn detects_hashed_asset_names() {
        assert!(has_content_hash("/assets/index-DiwrgTda.js"));
        assert!(has_content_hash("/assets/index-DiwrgTda.css"));
        assert!(has_content_hash("/assets/crypto_wasm_bg-abcDEF12.wasm"));
        assert!(!has_content_hash("/"));
        assert!(!has_content_hash("/index.html"));
        assert!(!has_content_hash("/favicon.svg"));
        assert!(!has_content_hash("/assets/app.js"));
        assert!(!has_content_hash("/assets/app-123.js"));
    }
}
