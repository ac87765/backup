//! HTTP 层：会话 Cookie（本 crate 自己管理）、CSRF 头、User-Agent、
//! 错误码映射与 429 退避。
//!
//! 安全边界：
//! - 不启用 reqwest 的 cookies 特性，避免全局 Cookie store 被日志/调试输出；
//! - 日志只记录方法、路径、状态码与退避时长，绝不记录 Cookie、请求体或令牌；
//! - 所有写请求（以及 GET）都带与 Web 相同的 `X-Requested-With: backup-web`。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{
    ACCEPT, CONTENT_TYPE, COOKIE, HeaderMap, HeaderValue, RETRY_AFTER, SET_COOKIE, USER_AGENT,
};
use reqwest::{Method, Response, StatusCode, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;
use zeroize::Zeroizing;

use crate::clock::Clock;
use crate::error::{ApiErrorCode, ApiFailure, ClientError};
use crate::secret_store::SessionToken;

/// 改变状态请求必须携带的自定义头（user-accounts spec：跨站请求防护）。
pub(crate) const CSRF_HEADER: &str = "x-requested-with";
pub(crate) const CSRF_VALUE: &str = "backup-web";
/// 服务端会话 Cookie 名。
pub(crate) const SESSION_COOKIE_NAME: &str = "__Host-session";

/// 429 最多重试次数（不含第一次请求），与 Web 的 `RATE_LIMIT_MAX_RETRIES` 一致。
pub(crate) const RATE_LIMIT_MAX_RETRIES: u32 = 8;
/// 429 没有 `Retry-After` 时的默认等待秒数。
pub(crate) const RATE_LIMIT_DEFAULT_RETRY_AFTER_SECONDS: f64 = 5.0;
/// 429 等待抖动上限（0–20%，只向上加），与 Web 一致。
pub(crate) const RATE_LIMIT_JITTER_RATIO: f64 = 0.2;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 服务端错误体（`api::error::ErrorBody` 的客户端表示）。
#[derive(Debug, serde::Deserialize)]
struct ErrorBody {
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    request_id: Option<String>,
}

/// 客户端 HTTP 传输层。
pub(crate) struct HttpClient {
    http: reqwest::Client,
    base_url: Url,
    clock: Arc<dyn Clock>,
    user_agent: HeaderValue,
    session: Mutex<Option<SessionToken>>,
}

impl HttpClient {
    pub(crate) fn new(base_url: Url, clock: Arc<dyn Clock>) -> Result<Self, ClientError> {
        let mut base_url = base_url;
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }

        let user_agent = HeaderValue::from_str(&format!(
            "EncryptedBackup/{} ({}; desktop)",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        ))
        .map_err(|_| ClientError::Internal("User-Agent 构造失败"))?;

        let http = reqwest::Client::builder()
            // 不跟随重定向：避免把 Cookie 发给另一个源，也让错误码保持可预测。
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(ClientError::Network)?;

        Ok(Self {
            http,
            base_url,
            clock,
            user_agent,
            session: Mutex::new(None),
        })
    }

    pub(crate) fn set_session(&self, token: Option<SessionToken>) {
        if let Ok(mut slot) = self.session.lock() {
            *slot = token;
        }
    }

    pub(crate) fn current_session(&self) -> Option<SessionToken> {
        self.session.lock().ok().and_then(|slot| slot.clone())
    }

    pub(crate) fn clear_session(&self) {
        self.set_session(None);
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        let response = self.send(Method::GET, path, None).await?;
        Self::parse_json(response).await
    }

    /// 通用 JSON 请求（文件管理/传输模块使用）。
    ///
    /// 请求体只接受 `serde_json::Value`，避免把密钥类型误传给本层；429 退避
    /// 仍由 [`Self::send`] 统一处理。
    pub(crate) async fn request_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<T, ClientError> {
        let serialized = match body {
            Some(body) => Some(serialize_body(body)?),
            None => None,
        };
        let response = match serialized.as_ref() {
            Some(body) => self.send(method, path, Some(body.as_slice())).await?,
            None => self.send(method, path, None).await?,
        };
        Self::parse_json(response).await
    }

    pub(crate) async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ClientError> {
        let body = serialize_body(body)?;
        let response = self.send(Method::POST, path, Some(&body)).await?;
        Self::parse_json(response).await
    }

    /// POST 并在响应体中返回 JSON，同时读取 `Set-Cookie` 中的会话令牌。
    pub(crate) async fn post_json_capture_session<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<(T, Option<SessionToken>), ClientError> {
        let body = serialize_body(body)?;
        let response = self.send(Method::POST, path, Some(&body)).await?;
        let token = session_cookie(response.headers());
        let value = Self::parse_json(response).await?;
        Ok((value, token))
    }

    pub(crate) async fn post_empty<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, ClientError> {
        let response = self.send(Method::POST, path, None).await?;
        Self::parse_json(response).await
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<Response, ClientError> {
        let url = self.url_for(path)?;
        let mut retries: u32 = 0;
        loop {
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .header(USER_AGENT, self.user_agent.clone())
                .header(CSRF_HEADER, CSRF_VALUE)
                .header(ACCEPT, "application/json");

            if let Some(token) = self.current_session() {
                if !token.is_valid_header_value() {
                    return Err(ClientError::InvalidSession);
                }
                let cookie =
                    Zeroizing::new(format!("{SESSION_COOKIE_NAME}={}", token.expose_secret()));
                let value =
                    HeaderValue::from_str(&cookie).map_err(|_| ClientError::InvalidSession)?;
                request = request.header(COOKIE, value);
            }
            if let Some(body) = body {
                request = request
                    .header(CONTENT_TYPE, "application/json")
                    .body(body.to_vec());
            }

            let response = request.send().await.map_err(ClientError::Network)?;
            let status = response.status();

            if status == StatusCode::TOO_MANY_REQUESTS {
                let failure = Self::failure_from_response(response).await?;
                // 账号锁定不是瞬时拥塞：把 Retry-After 交给界面显示，不在这里
                // 连等 8 次（否则 15 分钟锁定会变成两小时的静默等待）。
                if failure.code == ApiErrorCode::AccountTemporarilyLocked {
                    return Err(ClientError::Api(failure));
                }
                if retries >= RATE_LIMIT_MAX_RETRIES {
                    return Err(ClientError::Api(failure));
                }
                retries += 1;
                let delay = rate_limit_delay(failure.retry_after, self.clock.jitter());
                tracing::warn!(
                    attempt = retries,
                    delay_ms = delay.as_millis(),
                    "服务器限流，退避后重试"
                );
                self.clock.sleep(delay).await;
                continue;
            }

            if status.is_success() {
                return Ok(response);
            }
            return Err(ClientError::Api(
                Self::failure_from_response(response).await?,
            ));
        }
    }

    fn url_for(&self, path: &str) -> Result<Url, ClientError> {
        self.base_url
            .join(path.trim_start_matches('/'))
            .map_err(|_| ClientError::InvalidBaseUrl)
    }

    async fn parse_json<T: DeserializeOwned>(response: Response) -> Result<T, ClientError> {
        let bytes = response.bytes().await.map_err(ClientError::Network)?;
        serde_json::from_slice(&bytes).map_err(|_| ClientError::InvalidResponse("JSON 解析失败"))
    }

    async fn failure_from_response(response: Response) -> Result<ApiFailure, ClientError> {
        let status = response.status().as_u16();
        let retry_after = retry_after(response.headers());
        let bytes = response.bytes().await.map_err(ClientError::Network)?;
        let body = serde_json::from_slice::<ErrorBody>(&bytes).ok();
        Ok(match body {
            Some(body) => ApiFailure {
                status,
                code: ApiErrorCode::from_server_code(&body.code),
                message: if body.message.is_empty() {
                    generic_message(status).to_owned()
                } else {
                    body.message
                },
                request_id: body.request_id,
                retry_after,
            },
            None => ApiFailure {
                status,
                code: fallback_code(status),
                message: generic_message(status).to_owned(),
                request_id: None,
                retry_after,
            },
        })
    }
}

fn serialize_body<B: Serialize>(body: &B) -> Result<Zeroizing<Vec<u8>>, ClientError> {
    serde_json::to_vec(body)
        .map(Zeroizing::new)
        .map_err(|_| ClientError::Internal("请求体序列化失败"))
}

/// 从 `Set-Cookie` 读取会话令牌；没有时返回 `None`。
pub(crate) fn session_cookie(headers: &HeaderMap) -> Option<SessionToken> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| {
            let (name, rest) = value.split_once('=')?;
            if name.trim() != SESSION_COOKIE_NAME {
                return None;
            }
            let token = rest.split(';').next()?.trim();
            if token.is_empty() {
                return None;
            }
            Some(SessionToken::new(token))
        })
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    let seconds = value.parse::<u64>().ok()?;
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

/// `Retry-After`（缺失按 5 秒）加 0–20% 抖动。
pub(crate) fn rate_limit_delay(retry_after: Option<Duration>, jitter: f64) -> Duration {
    let seconds = retry_after
        .map(|duration| duration.as_secs_f64())
        .filter(|seconds| *seconds > 0.0)
        .unwrap_or(RATE_LIMIT_DEFAULT_RETRY_AFTER_SECONDS);
    let jitter = if jitter.is_finite() {
        jitter.clamp(0.0, 1.0)
    } else {
        0.0
    };
    Duration::from_secs_f64(seconds * (1.0 + jitter * RATE_LIMIT_JITTER_RATIO))
}

fn fallback_code(status: u16) -> ApiErrorCode {
    match status {
        401 => ApiErrorCode::Unauthenticated,
        404 => ApiErrorCode::NotFound,
        429 => ApiErrorCode::RateLimited,
        500..=599 => ApiErrorCode::InternalError,
        other => ApiErrorCode::Other(format!("http_{other}")),
    }
}

fn generic_message(status: u16) -> &'static str {
    match status {
        401 => "需要登录",
        403 => "请求被拒绝",
        404 => "资源不存在",
        429 => "请求过于频繁，请稍后重试",
        500..=599 => "服务器内部错误",
        _ => "请求失败",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn cookie_is_parsed_from_set_cookie_headers() {
        let mut headers = HeaderMap::new();
        headers.append(SET_COOKIE, HeaderValue::from_static("theme=dark; Path=/"));
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("__Host-session=abc123; Path=/; HttpOnly; Secure"),
        );
        let token = session_cookie(&headers).expect("必须找到会话 Cookie");
        assert_eq!(token.expose_secret(), "abc123");
    }

    #[test]
    fn missing_or_empty_cookie_is_none() {
        let mut headers = HeaderMap::new();
        headers.append(SET_COOKIE, HeaderValue::from_static("theme=dark"));
        assert!(session_cookie(&headers).is_none());

        headers.clear();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("__Host-session=; Path=/"),
        );
        assert!(session_cookie(&headers).is_none());
    }

    #[test]
    fn retry_after_parses_positive_seconds_only() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("7"));
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(7)));

        headers.insert(RETRY_AFTER, HeaderValue::from_static("0"));
        assert_eq!(retry_after(&headers), None);

        headers.insert(RETRY_AFTER, HeaderValue::from_static("not-a-number"));
        assert_eq!(retry_after(&headers), None);
    }

    #[test]
    fn rate_limit_delay_defaults_to_five_seconds_and_applies_jitter() {
        assert_eq!(
            rate_limit_delay(None, 0.0),
            Duration::from_secs_f64(RATE_LIMIT_DEFAULT_RETRY_AFTER_SECONDS)
        );
        assert_eq!(
            rate_limit_delay(Some(Duration::from_secs(10)), 0.0),
            Duration::from_secs(10)
        );
        let with_jitter = rate_limit_delay(Some(Duration::from_secs(10)), 1.0);
        assert_eq!(with_jitter, Duration::from_secs_f64(12.0));
    }

    #[test]
    fn fallback_codes_are_strongly_typed() {
        assert_eq!(fallback_code(401), ApiErrorCode::Unauthenticated);
        assert_eq!(fallback_code(404), ApiErrorCode::NotFound);
        assert_eq!(fallback_code(429), ApiErrorCode::RateLimited);
        assert_eq!(fallback_code(503), ApiErrorCode::InternalError);
        assert_eq!(
            fallback_code(418),
            ApiErrorCode::Other("http_418".to_owned())
        );
    }
}
