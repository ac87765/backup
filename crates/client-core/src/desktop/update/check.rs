//! 检查更新：请求清单与签名、验签、防回滚与强制更新判断。

use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use tracing::warn;

use crate::release::{
    AppVersion, MinSupportVerdict, RELEASE_PUBLIC_KEY, ReleaseError, ReleaseManifest,
    below_min_supported, normalize_app_version, parse_stable_version, verify_manifest,
};
use crate::state::StateStore;

use super::{UpdateError, build_update_http, downloads_url, parse_server_url, unix_now};

/// 清单请求超时（D5）。
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// 清单/签名响应体上限；正常清单只有几 KiB。
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// 已见过的最高清单版本（防回滚水位）。
pub const HIGHEST_SEEN_KEY: &str = "update.highest_seen_version";

/// 用户关闭横幅的版本（每个版本只提示一次）。
pub const BANNER_DISMISSED_KEY: &str = "update.banner_dismissed_version";

const MANIFEST_NAME: &str = "latest.json";
const SIGNATURE_NAME: &str = "latest.json.sig";

/// 检查结果。
///
/// `Available` 直接携带完整清单（约 240 字节）；每次检查只产生一个值，不进入
/// 热路径，因此保留直观的结构而不额外装箱。
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheck {
    /// 当前已是最新版本。
    UpToDate,
    /// 有更高版本；`forced` 只在最低支持版本高于当前版本时为真。
    Available {
        manifest: ReleaseManifest,
        forced: bool,
    },
    /// 当前是开发构建：检查照常执行，但从不提示、从不强制更新。
    DevelopmentBuild,
    /// 无法检查；按“没有更新”处理，不提示、不下载。
    Unavailable { reason: UpdateUnavailableReason },
}

/// 无法检查更新的原因（稳定字符串供桥接层/界面使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateUnavailableReason {
    /// 网络错误。
    Network,
    /// HTTP 非 200。
    HttpStatus(u16),
    /// 签名无效（清单被篡改或不是维护者签发的）。
    InvalidSignature,
    /// 签名有效但 JSON 不符合清单契约，或响应体超过上限。
    InvalidManifest,
    /// 签名有效的旧清单低于已见最高版本（D1 防回滚）。
    RollbackRejected,
    /// 本地状态库读写失败。
    Storage,
}

impl UpdateUnavailableReason {
    /// 稳定字符串。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::HttpStatus(_) => "http_status",
            Self::InvalidSignature => "invalid_signature",
            Self::InvalidManifest => "invalid_manifest",
            Self::RollbackRejected => "rollback_rejected",
            Self::Storage => "storage",
        }
    }
}

/// 一次检查的结果与时间（Unix 秒）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckOutcome {
    pub check: UpdateCheck,
    pub checked_at: i64,
}

/// 更新检查器。
///
/// `current_version` 是 Dart 在初始化时传入的 `APP_VERSION`（可能是
/// `v0.1.0`、`v0.1.0-3-gabc1234`、`c22934c` 或 `dev`），由
/// [`normalize_app_version`] 统一归一化。
pub struct UpdateChecker {
    server_url: reqwest::Url,
    current_version: String,
    state: Arc<StateStore>,
    http: reqwest::Client,
    public_key: [u8; 32],
}

impl UpdateChecker {
    /// 创建检查器；生产代码使用 [`RELEASE_PUBLIC_KEY`]。
    pub fn new(
        server_url: &str,
        current_version: impl Into<String>,
        state: Arc<StateStore>,
    ) -> Result<Self, UpdateError> {
        let server_url = parse_server_url(server_url)?;
        let current_version = current_version.into();
        let http = build_update_http(&current_version, Some(REQUEST_TIMEOUT))?;
        Ok(Self {
            server_url,
            current_version,
            state,
            http,
            public_key: RELEASE_PUBLIC_KEY,
        })
    }

    /// 注入测试公钥（生产代码不要调用）。
    #[must_use]
    pub fn with_public_key(mut self, public_key: [u8; 32]) -> Self {
        self.public_key = public_key;
        self
    }

    /// 当前版本（Dart 传入的原始字符串）。
    #[must_use]
    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    /// 执行一次检查。
    ///
    /// 网络/签名/格式/防回滚问题都归为 [`UpdateCheck::Unavailable`]，界面按
    /// “没有更新”处理；本方法不返回错误，也不会因为在检查期间网络失败而阻断
    /// 应用的其他功能。
    pub async fn check(&self) -> UpdateCheckOutcome {
        let checked_at = unix_now();
        let check = match self.check_inner().await {
            Ok(check) => check,
            Err(error) => {
                warn!(reason = error.code(), "检查客户端更新失败，按没有更新处理");
                UpdateCheck::Unavailable {
                    reason: reason_for(&error),
                }
            }
        };
        UpdateCheckOutcome { check, checked_at }
    }

    async fn check_inner(&self) -> Result<UpdateCheck, UpdateError> {
        let manifest_url = downloads_url(&self.server_url, &["downloads", MANIFEST_NAME])?;
        let signature_url = downloads_url(&self.server_url, &["downloads", SIGNATURE_NAME])?;

        let manifest_bytes = self.fetch_limited(&manifest_url).await?;
        let signature = self.fetch_limited(&signature_url).await?;
        let signature =
            std::str::from_utf8(&signature).map_err(|_| UpdateError::InvalidManifest)?;

        let manifest =
            verify_manifest(&manifest_bytes, signature, &self.public_key).map_err(|error| {
                match error {
                    ReleaseError::InvalidSignature
                    | ReleaseError::InvalidSignatureEncoding
                    | ReleaseError::InvalidSignatureLength
                    | ReleaseError::InvalidPublicKey => UpdateError::InvalidSignature,
                    _ => UpdateError::InvalidManifest,
                }
            })?;

        // 清单已验签；版本在 verify_manifest 中已经校验为稳定 semver。
        let manifest_version =
            parse_stable_version(&manifest.version).map_err(|_| UpdateError::InvalidManifest)?;

        let highest = self
            .state
            .app_setting(HIGHEST_SEEN_KEY)
            .await?
            .and_then(|value| parse_stable_version(&value).ok());

        if highest
            .as_ref()
            .is_some_and(|highest| manifest_version < *highest)
        {
            warn!(
                manifest = %manifest.version,
                "拒绝低于已见最高版本的更新清单"
            );
            return Ok(UpdateCheck::Unavailable {
                reason: UpdateUnavailableReason::RollbackRejected,
            });
        }

        if highest
            .as_ref()
            .is_none_or(|highest| manifest_version > *highest)
        {
            self.state
                .set_app_setting(HIGHEST_SEEN_KEY, &manifest.version)
                .await?;
        }

        let AppVersion::Release(current) = normalize_app_version(&self.current_version) else {
            return Ok(UpdateCheck::DevelopmentBuild);
        };

        if manifest_version <= current {
            return Ok(UpdateCheck::UpToDate);
        }

        let forced = below_min_supported(&self.current_version, &manifest)
            == MinSupportVerdict::BelowMinimum;
        Ok(UpdateCheck::Available { manifest, forced })
    }

    /// 读取“用户已关闭横幅”的版本。
    pub async fn banner_dismissed_version(&self) -> Result<Option<String>, UpdateError> {
        Ok(self.state.app_setting(BANNER_DISMISSED_KEY).await?)
    }

    /// 记录用户已关闭某个版本的横幅。
    pub async fn dismiss_banner(&self, version: &str) -> Result<(), UpdateError> {
        self.state
            .set_app_setting(BANNER_DISMISSED_KEY, version)
            .await?;
        Ok(())
    }

    async fn fetch_limited(&self, url: &reqwest::Url) -> Result<Vec<u8>, UpdateError> {
        let response = self.http.get(url.clone()).send().await.map_err(|error| {
            warn!(url = %url, error = %error, "请求更新清单失败");
            UpdateError::Network
        })?;
        let status = response.status();
        if status != StatusCode::OK {
            warn!(url = %url, status = status.as_u16(), "更新清单返回非 200");
            return Err(UpdateError::HttpStatus(status.as_u16()));
        }
        let mut response = response;
        let mut body = Vec::new();
        loop {
            let chunk = response.chunk().await.map_err(|_| UpdateError::Network)?;
            let Some(chunk) = chunk else {
                break;
            };
            if body.len() as u64 + chunk.len() as u64 > MAX_MANIFEST_BYTES {
                warn!(url = %url, "更新清单超过响应体上限");
                return Err(UpdateError::InvalidManifest);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

fn reason_for(error: &UpdateError) -> UpdateUnavailableReason {
    match error {
        UpdateError::Network => UpdateUnavailableReason::Network,
        UpdateError::HttpStatus(status) => UpdateUnavailableReason::HttpStatus(*status),
        UpdateError::InvalidSignature => UpdateUnavailableReason::InvalidSignature,
        UpdateError::InvalidManifest => UpdateUnavailableReason::InvalidManifest,
        UpdateError::State(_) => UpdateUnavailableReason::Storage,
        _ => UpdateUnavailableReason::Network,
    }
}
