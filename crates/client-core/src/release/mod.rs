//! 客户端发布清单：严格解析、Ed25519 验签与版本判断。
//!
//! 本模块不受 `desktop` feature 限制，也不做任何 IO：发布工具用它生成清单，
//! 客户端用它验签。签名私钥只由 `release-tool` 在维护者电脑上使用，本模块
//! 只包含验证逻辑。
//!
//! 约束（add-client-releases design D1/D2）：
//! - 只接受 `schema = 1`；未知字段、未知平台、未知 schema 一律拒绝。
//! - `version` / `min_supported` 必须是不带预发布与构建元数据的稳定 semver。
//! - `sha256` 必须是 64 位小写十六进制；文件名不得包含路径分隔符、`..` 或控制字符。
//! - `history` 最多 2 项（加上清单自身共 3 个版本的更新日志）。
//! - 验签在解析之前完成：签名无效时不解析 JSON，调用方拿不到部分结果。

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};

/// 当前唯一的清单 schema。
pub const MANIFEST_SCHEMA: u32 = 1;

/// `history` 最多保留的旧版本数量。
pub const MAX_HISTORY: usize = 2;

/// 编译进客户端的发布公钥。
///
/// 与仓库根目录 `deploy/release-signing.pub`（32 字节标准 Base64）一致；
/// 单元测试读取该文件并逐字节比较，轮换公钥时必须同时更新两者。
pub const RELEASE_PUBLIC_KEY: [u8; 32] = [
    0xdf, 0x2a, 0x37, 0xd3, 0x1d, 0x56, 0xeb, 0xc8, 0x42, 0xbd, 0x5e, 0x97, 0x65, 0x26, 0x61, 0x76,
    0x14, 0xbe, 0xc2, 0x79, 0x81, 0xfa, 0x13, 0x77, 0xd6, 0x67, 0x29, 0x1a, 0xdc, 0x27, 0x71, 0xfe,
];

/// 发布清单（`latest.json`）。
///
/// 字段顺序固定：序列化结果就是发布工具写入磁盘的顺序。反序列化经由
/// [`ReleaseManifestWire`] 严格校验，构造后也可调用 [`Self::validate`] 复查。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ReleaseManifestWire")]
pub struct ReleaseManifest {
    /// 清单 schema，当前只允许 [`MANIFEST_SCHEMA`]。
    pub schema: u32,
    /// 稳定 semver（例如 `0.2.0`）。
    pub version: String,
    /// 发布时间的 RFC3339 字符串（发布工具负责校验格式）。
    pub released_at: String,
    /// 低于该版本的客户端必须更新。
    pub min_supported: String,
    /// 当前版本的更新内容，每项一行。
    pub notes: Vec<String>,
    /// 之前最多 2 个版本（最新在前）。
    pub history: Vec<ReleaseHistoryEntry>,
    /// 各平台安装包。
    pub platforms: ReleasePlatforms,
}

impl ReleaseManifest {
    /// 校验清单的所有不变量；反序列化与发布工具写盘前都会调用。
    pub fn validate(&self) -> Result<(), ReleaseError> {
        if self.schema != MANIFEST_SCHEMA {
            return Err(ReleaseError::UnsupportedSchema(self.schema));
        }
        parse_stable_version(&self.version)?;
        parse_stable_version(&self.min_supported)?;
        validate_artifact(&self.platforms.windows_x64)?;
        validate_artifact(&self.platforms.macos_universal)?;
        if self.history.len() > MAX_HISTORY {
            return Err(ReleaseError::TooMuchHistory);
        }
        for entry in &self.history {
            parse_stable_version(&entry.version)?;
        }
        Ok(())
    }
}

/// 清单中的一个历史版本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseHistoryEntry {
    /// 稳定 semver。
    pub version: String,
    /// 发布时间的 RFC3339 字符串。
    pub released_at: String,
    /// 该版本的更新内容。
    pub notes: Vec<String>,
}

/// 清单中的两个平台。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasePlatforms {
    /// Windows x64 安装包。
    #[serde(rename = "windows-x64")]
    pub windows_x64: ReleaseArtifact,
    /// macOS universal .dmg。
    #[serde(rename = "macos-universal")]
    pub macos_universal: ReleaseArtifact,
}

/// 单个平台的安装包信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseArtifact {
    /// 不含路径的安装包文件名。
    pub file: String,
    /// 字节大小。
    pub size: u64,
    /// 64 位小写十六进制 SHA-256。
    pub sha256: String,
}

/// 反序列化中转结构：先按宽松类型读入，再在 `TryFrom` 中做全部严格校验。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseManifestWire {
    schema: u32,
    version: String,
    released_at: String,
    min_supported: String,
    notes: Vec<String>,
    history: Vec<ReleaseHistoryEntry>,
    platforms: ReleasePlatforms,
}

impl TryFrom<ReleaseManifestWire> for ReleaseManifest {
    type Error = ReleaseError;

    fn try_from(wire: ReleaseManifestWire) -> Result<Self, Self::Error> {
        let manifest = Self {
            schema: wire.schema,
            version: wire.version,
            released_at: wire.released_at,
            min_supported: wire.min_supported,
            notes: wire.notes,
            history: wire.history,
            platforms: wire.platforms,
        };
        manifest.validate()?;
        Ok(manifest)
    }
}

/// 清单/签名/版本校验错误。
#[derive(Debug, thiserror::Error)]
pub enum ReleaseError {
    /// 清单 schema 不是当前客户端认识的版本。
    #[error("不支持的清单 schema：{0}")]
    UnsupportedSchema(u32),

    /// 版本号不是稳定 semver（含预发布或构建元数据），或是开发构建。
    #[error("版本号不是稳定的语义化版本：{0}")]
    InvalidVersion(String),

    /// SHA-256 不是 64 位小写十六进制。
    #[error("SHA-256 必须是 64 位小写十六进制")]
    InvalidSha256,

    /// 文件名包含路径分隔符、`..` 或控制字符，或为空。
    #[error("安装包文件名无效")]
    InvalidFileName,

    /// history 超过 [`MAX_HISTORY`] 项。
    #[error("清单 history 最多 2 项")]
    TooMuchHistory,

    /// 签名不是合法的标准 Base64。
    #[error("签名不是有效的 Base64")]
    InvalidSignatureEncoding,

    /// 签名解码后不是 64 字节。
    #[error("签名长度不是 64 字节")]
    InvalidSignatureLength,

    /// 公钥不是合法的 Ed25519 公钥。
    #[error("公钥无效")]
    InvalidPublicKey,

    /// Ed25519 严格验签失败。
    #[error("清单签名验证失败")]
    InvalidSignature,

    /// 验签通过但 JSON 不符合清单契约。
    #[error("清单格式无效")]
    InvalidManifest(#[source] serde_json::Error),
}

/// 验证 `latest.json` 原始字节的 Ed25519 签名，再严格解析清单。
///
/// 顺序固定：先 `verify_strict` 验证原始字节，成功后才解析 JSON。任何失败都
/// 返回错误，不暴露部分解析结果。
pub fn verify_manifest(
    bytes: &[u8],
    sig_b64: &str,
    public_key: &[u8; 32],
) -> Result<ReleaseManifest, ReleaseError> {
    let verifying_key =
        VerifyingKey::from_bytes(public_key).map_err(|_| ReleaseError::InvalidPublicKey)?;
    let sig_bytes = BASE64
        .decode(sig_b64.trim())
        .map_err(|_| ReleaseError::InvalidSignatureEncoding)?;
    if sig_bytes.len() != 64 {
        return Err(ReleaseError::InvalidSignatureLength);
    }
    let signature =
        Signature::from_slice(&sig_bytes).map_err(|_| ReleaseError::InvalidSignatureLength)?;
    verifying_key
        .verify_strict(bytes, &signature)
        .map_err(|_| ReleaseError::InvalidSignature)?;
    // 先按中转结构解析 JSON（未知字段在这里被拒绝），再做清单不变量校验；
    // 这样调用方能拿到具体的校验错误（schema、文件名、SHA-256、history 等）。
    let wire: ReleaseManifestWire =
        serde_json::from_slice(bytes).map_err(ReleaseError::InvalidManifest)?;
    ReleaseManifest::try_from(wire)
}

/// 解析稳定 semver：拒绝预发布（`-dev`）与构建元数据（`+abc`）。
pub fn parse_stable_version(value: &str) -> Result<Version, ReleaseError> {
    let version =
        Version::parse(value).map_err(|_| ReleaseError::InvalidVersion(value.to_owned()))?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(ReleaseError::InvalidVersion(value.to_owned()));
    }
    Ok(version)
}

/// 归一化后的应用版本（add-client-releases D1）。
///
/// CI 用 `git describe --tags --always --dirty` 注入 `APP_VERSION`：标签构建是
/// `v0.1.0`，非标签构建可能是 `v0.1.0-3-gabc1234`、`c22934c` 或 `dev`。
/// 只有去掉前导 `v` 后仍是稳定 semver 的才是可升级的发布构建。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppVersion {
    /// 稳定的发布版本（`v0.1.0` 或 `0.1.0`）。
    Release(Version),
    /// 开发构建（提交号、带预发布/构建元数据的 describe 结果、`dev` 等）。
    Development,
}

impl AppVersion {
    /// 是否是可升级的发布构建。
    #[must_use]
    pub fn is_release(&self) -> bool {
        matches!(self, Self::Release(_))
    }
}

/// 归一化应用版本：去掉前导 `v`（与空白），再判断是否为稳定 semver。
///
/// 清单里的版本仍然由 [`parse_stable_version`] 严格校验（不允许 `v` 前缀）；
/// 本函数只用于客户端自身由 `git describe` 注入的版本。
#[must_use]
pub fn normalize_app_version(raw: &str) -> AppVersion {
    let trimmed = raw.trim();
    let without_prefix = trimmed.strip_prefix('v').unwrap_or(trimmed);
    match parse_stable_version(without_prefix) {
        Ok(version) => AppVersion::Release(version),
        Err(_) => AppVersion::Development,
    }
}

/// 版本比较结果。
///
/// 当前版本解析失败（开发构建，例如 `c22934c` 或 `0.1.0-dev+abc`）时返回
/// [`VersionVerdict::DevelopmentBuild`]，客户端不应提示升级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionVerdict {
    /// 清单版本高于当前版本。
    Newer,
    /// 清单版本等于或低于当前版本；或清单版本本身不可解析（防御性处理）。
    NotNewer,
    /// 当前版本是开发构建，无法比较。
    DevelopmentBuild,
}

/// 判断 `candidate`（清单版本）是否高于 `current`（当前客户端版本）。
///
/// `current` 会先经过 [`normalize_app_version`]（去掉标签版本的前导 `v`）。
pub fn is_newer(candidate: &str, current: &str) -> VersionVerdict {
    let AppVersion::Release(current) = normalize_app_version(current) else {
        return VersionVerdict::DevelopmentBuild;
    };
    let Ok(candidate) = parse_stable_version(candidate) else {
        return VersionVerdict::NotNewer;
    };
    if candidate > current {
        VersionVerdict::Newer
    } else {
        VersionVerdict::NotNewer
    }
}

/// 最低支持版本判断结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinSupportVerdict {
    /// 当前版本达到最低支持版本。
    Supported,
    /// 当前版本低于最低支持版本，必须更新。
    BelowMinimum,
    /// 当前版本是开发构建，无法判断，也不应提示强制更新。
    DevelopmentBuild,
}

/// 判断当前版本是否低于清单的 `min_supported`。
///
/// `manifest` 已经过 [`verify_manifest`] 校验，`min_supported` 正常不会解析失败；
/// 若调用方自行构造了非法清单，这里按“不强制更新”处理，避免误伤用户。
/// `current` 会先经过 [`normalize_app_version`]（去掉标签版本的前导 `v`）。
pub fn below_min_supported(current: &str, manifest: &ReleaseManifest) -> MinSupportVerdict {
    let AppVersion::Release(current) = normalize_app_version(current) else {
        return MinSupportVerdict::DevelopmentBuild;
    };
    let Ok(minimum) = parse_stable_version(&manifest.min_supported) else {
        return MinSupportVerdict::Supported;
    };
    if current < minimum {
        MinSupportVerdict::BelowMinimum
    } else {
        MinSupportVerdict::Supported
    }
}

fn validate_artifact(artifact: &ReleaseArtifact) -> Result<(), ReleaseError> {
    validate_file_name(&artifact.file)?;
    if !is_lower_hex_sha256(&artifact.sha256) {
        return Err(ReleaseError::InvalidSha256);
    }
    Ok(())
}

fn validate_file_name(file: &str) -> Result<(), ReleaseError> {
    let invalid = file.is_empty()
        || file.contains('/')
        || file.contains('\\')
        // Windows 盘符相对路径（`C:x`）与 NTFS 备用数据流（`a.exe:s`）。
        || file.contains(':')
        || file.contains("..")
        || file.chars().any(char::is_control);
    if invalid {
        Err(ReleaseError::InvalidFileName)
    } else {
        Ok(())
    }
}

fn is_lower_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests;
