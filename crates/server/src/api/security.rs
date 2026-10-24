//! 安全响应头与 CSP。
//!
//! CSP 的基础策略只有一份：仓库根目录的 `csp/base-policy.txt`。
//! - 服务端用 `include_str!` 编进二进制；
//! - `web/vite.config.ts` 在构建/预览时读取同一个文件；
//! - 服务端启动时从配置读出对象存储公网 origin，追加到 `connect-src`。

use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

/// 共享 CSP 基础策略（编译期嵌入，测试会校验它与磁盘文件一致）。
pub const BASE_CONTENT_SECURITY_POLICY: &str = include_str!("../../../../csp/base-policy.txt");

/// 规范化后的基础策略（去掉首尾空白）。
pub fn base_policy() -> &'static str {
    BASE_CONTENT_SECURITY_POLICY.trim()
}

/// 在基础策略的 `connect-src` 追加对象存储公网 origin。
///
/// 基础策略必须包含 `connect-src`（`csp/base-policy.txt` 中已有），
/// 因此这里只做追加，不生成新的指令集合。
pub fn content_security_policy(storage_origin: &str) -> String {
    let origin = storage_origin.trim().trim_end_matches('/');
    let mut directives: Vec<String> = base_policy()
        .split(';')
        .map(|directive| directive.trim().to_owned())
        .filter(|directive| !directive.is_empty())
        .map(|directive| {
            let is_connect_src = directive
                .split_whitespace()
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("connect-src"));
            if is_connect_src && !origin.is_empty() {
                format!("{directive} {origin}")
            } else {
                directive
            }
        })
        .collect();
    if !directives.iter().any(|directive| {
        directive
            .split_whitespace()
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("connect-src"))
    }) {
        directives.push(format!("connect-src 'self' {origin}"));
    }
    directives.join("; ")
}

/// 预构建的安全响应头集合。
#[derive(Debug, Clone)]
pub struct SecurityHeaders {
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl SecurityHeaders {
    /// 按对象存储公网 origin 构建安全头。
    pub fn new(storage_origin: &str) -> Self {
        let csp = content_security_policy(storage_origin);
        Self {
            headers: vec![
                (
                    HeaderName::from_static("content-security-policy"),
                    HeaderValue::from_str(&csp)
                        .unwrap_or_else(|_| HeaderValue::from_static("default-src 'self'")),
                ),
                // HSTS 只对 HTTPS 生效；反向代理终止 TLS 后浏览器仍会看到该头。
                (
                    HeaderName::from_static("strict-transport-security"),
                    HeaderValue::from_static("max-age=63072000; includeSubDomains"),
                ),
                (
                    HeaderName::from_static("x-content-type-options"),
                    HeaderValue::from_static("nosniff"),
                ),
                (
                    HeaderName::from_static("referrer-policy"),
                    HeaderValue::from_static("no-referrer"),
                ),
            ],
        }
    }

    fn insert_into(&self, headers: &mut HeaderMap) {
        for (name, value) in &self.headers {
            headers.insert(name.clone(), value.clone());
        }
    }
}

/// 安全头中间件：对包括错误响应在内的所有响应生效。
pub async fn apply(headers: SecurityHeaders, request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    headers.insert_into(response.headers_mut());
    response
}

#[cfg(test)]
mod tests {
    // 测试代码允许使用 expect/unwrap：断言失败时 panic 是预期行为。
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn embedded_policy_matches_repository_file() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../csp/base-policy.txt");
        let on_disk = std::fs::read_to_string(path).expect("应当能读取共享 CSP 文件");
        assert_eq!(
            BASE_CONTENT_SECURITY_POLICY, on_disk,
            "编译期嵌入的 CSP 与仓库文件不一致，请重新构建"
        );
        // Web 端必须读取同一个文件，避免维护两份策略。
        let vite_config = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/vite.config.ts"
        ))
        .expect("应当能读取 web/vite.config.ts");
        assert!(
            vite_config.contains("csp/base-policy.txt"),
            "web/vite.config.ts 必须读取同一份 csp/base-policy.txt"
        );
    }

    #[test]
    fn storage_origin_is_appended_to_connect_src() {
        let policy = content_security_policy("https://bucket.oss-cn-hangzhou.aliyuncs.com/");
        assert!(
            policy.contains("connect-src 'self' https://bucket.oss-cn-hangzhou.aliyuncs.com"),
            "实际策略: {policy}"
        );
        // 只应该出现一次 connect-src。
        assert_eq!(policy.matches("connect-src").count(), 1);
        assert!(policy.contains("frame-ancestors 'none'"));
        assert!(policy.contains("script-src 'self' 'wasm-unsafe-eval'"));
        assert!(!policy.contains("unsafe-inline"));
    }

    #[test]
    fn empty_origin_keeps_base_policy_unchanged() {
        assert_eq!(content_security_policy(""), base_policy());
    }
}
