//! 服务端配置：从环境变量（统一 `BACKUP_` 前缀）加载并校验。
//!
//! 硬性要求（AGENTS.md / service-operations spec）：
//! - 所有配置项使用 `BACKUP_` 前缀；密钥类配置用 [`Secret`]（内部 `secrecy::SecretString`），
//!   `Debug` 输出永远脱敏。
//! - 缺项或非法值时返回 [`ConfigError`]，错误信息只包含**配置项名称**与原因，
//!   绝不包含配置值本身。二进制入口据此以非零退出码退出。
//!
//! 本组只声明实际用到的配置项；pepper 等留到使用它们的功能组再加入。

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::PathBuf;

use figment::Figment;
use figment::providers::Serialized;
use ipnet::IpNet;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde::de::Deserializer;

/// 所有配置环境变量的统一前缀。
pub const ENV_PREFIX: &str = "BACKUP_";

/// 每个已认证用户每分钟的写接口默认上限。
///
/// 估算（批量接口、4 MiB 块、100 MB/s 持续上传）：
/// `100_000_000 / 4 MiB ≈ 24 块/秒`；每 32 块需要 1 次批量 presign + 1 次
/// 批量 confirm，即约 `24 × 2 / 32 × 60 ≈ 90 请求/分钟`。默认 240 留出约
/// 2.6 倍余量（约可支撑 260 MB/s 的持续上传）。README 记录完整推导。
pub const DEFAULT_USER_WRITE_RATE_LIMIT_PER_MINUTE: u32 = 240;

/// 配置项名称。错误信息与日志只使用这里的名字，不使用内部键名或配置值。
pub mod keys {
    pub const BIND_ADDR: &str = "BACKUP_BIND_ADDR";
    pub const METRICS_ADDR: &str = "BACKUP_METRICS_ADDR";
    pub const STATIC_DIR: &str = "BACKUP_STATIC_DIR";
    pub const DATABASE_URL: &str = "BACKUP_DATABASE_URL";
    pub const DATABASE_MAX_CONNECTIONS: &str = "BACKUP_DATABASE_MAX_CONNECTIONS";
    pub const STORAGE_KIND: &str = "BACKUP_STORAGE_KIND";
    pub const STORAGE_BUCKET: &str = "BACKUP_STORAGE_BUCKET";
    pub const STORAGE_REGION: &str = "BACKUP_STORAGE_REGION";
    pub const STORAGE_INTERNAL_ENDPOINT: &str = "BACKUP_STORAGE_INTERNAL_ENDPOINT";
    pub const STORAGE_PUBLIC_ENDPOINT: &str = "BACKUP_STORAGE_PUBLIC_ENDPOINT";
    pub const STORAGE_ACCESS_KEY_ID: &str = "BACKUP_STORAGE_ACCESS_KEY_ID";
    pub const STORAGE_SECRET_ACCESS_KEY: &str = "BACKUP_STORAGE_SECRET_ACCESS_KEY";
    pub const STORAGE_ALLOW_HTTP: &str = "BACKUP_STORAGE_ALLOW_HTTP";
    pub const JOBS_ENABLED: &str = "BACKUP_JOBS_ENABLED";
    pub const MAIL_MODE: &str = "BACKUP_MAIL_MODE";
    pub const MAIL_DIR: &str = "BACKUP_MAIL_DIR";
    pub const SMTP_HOST: &str = "BACKUP_SMTP_HOST";
    pub const SMTP_PORT: &str = "BACKUP_SMTP_PORT";
    pub const SMTP_FROM: &str = "BACKUP_SMTP_FROM";
    pub const SMTP_USERNAME: &str = "BACKUP_SMTP_USERNAME";
    pub const SMTP_PASSWORD: &str = "BACKUP_SMTP_PASSWORD";
    pub const SMTP_TLS: &str = "BACKUP_SMTP_TLS";
    pub const SMTP_ALLOW_INSECURE: &str = "BACKUP_SMTP_ALLOW_INSECURE";
    pub const AUTH_PEPPER_VERSION: &str = "BACKUP_AUTH_PEPPER_VERSION";
    pub const AUTH_PEPPER: &str = "BACKUP_AUTH_PEPPER";
    pub const AUTH_OLD_PEPPERS: &str = "BACKUP_AUTH_OLD_PEPPERS";
    pub const AUTH_FAKE_SALT_KEY: &str = "BACKUP_AUTH_FAKE_SALT_KEY";
    pub const PUBLIC_APP_URL: &str = "BACKUP_PUBLIC_APP_URL";
    pub const TRUSTED_PROXIES: &str = "BACKUP_TRUSTED_PROXIES";
    pub const AUTH_RATE_LIMIT_PER_MINUTE: &str = "BACKUP_AUTH_RATE_LIMIT_PER_MINUTE";
    pub const USER_WRITE_RATE_LIMIT_PER_MINUTE: &str = "BACKUP_USER_WRITE_RATE_LIMIT_PER_MINUTE";
}

/// 单个 pepper 的最小长度（字节）。
///
/// pepper 是 HMAC-SHA256 的密钥，本身不需要固定长度；这里只拒绝明显过短、
/// 可被低成本穷举的配置值，避免生产环境误用 `secret` 之类的字符串。
const MIN_PEPPER_BYTES: usize = 32;

/// 认证相关配置：pepper 版本链 + prelogin 伪造盐密钥。
///
/// `Debug` 派生是安全的：[`Secret`] 的 `Debug` 固定输出 `[REDACTED]`。
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// 当前 pepper 版本（新账号、以及升级成功后写入的版本）。
    pub pepper_version: i32,
    /// 当前 pepper。
    pub pepper: Secret,
    /// 旧 pepper 版本 → 值，用于登录/恢复校验后的渐进式升级。
    pub old_peppers: Vec<(i32, Secret)>,
    /// prelogin 对不存在邮箱生成确定性伪造盐所用的服务端密钥。
    /// 必须与 pepper 分开配置。
    pub fake_salt_key: Secret,
}

impl AuthConfig {
    /// 按版本取 pepper；版本不存在时返回 `None`。
    pub fn pepper_for(&self, version: i32) -> Option<&Secret> {
        if version == self.pepper_version {
            return Some(&self.pepper);
        }
        self.old_peppers
            .iter()
            .find(|(candidate, _)| *candidate == version)
            .map(|(_, secret)| secret)
    }
}

/// 启动配置错误。
///
/// 只携带配置项名称与静态原因字符串，因此可以安全地写日志。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("配置项 {key} 缺失")]
    Missing { key: &'static str },
    #[error("配置项 {key} 非法：{reason}")]
    Invalid {
        key: &'static str,
        reason: &'static str,
    },
}

impl ConfigError {
    fn invalid(key: &'static str, reason: &'static str) -> Self {
        Self::Invalid { key, reason }
    }
}

/// 密钥类配置。
///
/// 内部使用 `secrecy::SecretString`（Drop 时清零），不实现 `Serialize`/`Display`，
/// `Debug` 输出固定为 `[REDACTED]`，避免配置被意外写进日志或错误信息。
#[derive(Clone)]
pub struct Secret(SecretString);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(SecretString::from(value.into()))
    }

    /// 显式取出明文，只允许在真正需要建立连接的地方调用。
    pub fn expose(&self) -> &str {
        self.0.expose_secret()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::new(value))
    }
}

/// 对象存储后端类型：本地/测试用 S3 兼容 API（MinIO），生产用阿里云 OSS。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    S3,
    Oss,
}

impl StorageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::S3 => "s3",
            Self::Oss => "oss",
        }
    }
}

impl fmt::Display for StorageKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 数据库配置。
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: Secret,
    pub max_connections: u32,
}

/// `server migrate` 的配置：只加载并校验数据库部分。
///
/// 迁移不会访问对象存储、不会发邮件、也不会监听端口，因此不应要求这些
/// 与迁移无关的配置项存在；否则发布流程必须先准备一整套运行期配置。
#[derive(Debug, Clone)]
pub struct MigrateConfig {
    pub database: DatabaseConfig,
}

impl MigrateConfig {
    /// 从进程环境变量加载迁移所需配置。
    pub fn load() -> Result<Self, ConfigError> {
        Self::from_figment(&env_figment())
    }

    /// 从任意 figment 数据源加载（测试用）。
    pub fn from_figment(figment: &Figment) -> Result<Self, ConfigError> {
        Ok(Self {
            database: parse_database_config(figment)?,
        })
    }
}

/// 对象存储配置。
///
/// 内网 endpoint 供服务端自身 stat/list/delete 使用；公网 endpoint 只用于生成预签名 URL
/// （design D3）。两者可以是同一个地址（本地 MinIO），但语义必须分开配置。
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub kind: StorageKind,
    pub bucket: String,
    pub region: String,
    pub internal_endpoint: String,
    pub public_endpoint: String,
    pub access_key_id: Secret,
    pub secret_access_key: Secret,
    pub allow_http: bool,
}

impl StorageConfig {
    /// 浏览器直接访问对象存储时实际使用的 origin（`scheme://authority`）。
    ///
    /// OSS 的预签名 URL 使用虚拟主机风格：`https://<bucket>.<endpoint host>/...`，
    /// 因此浏览器访问 origin 必须带 bucket 子域名；S3/MinIO 使用 path-style，
    /// 浏览器访问 origin 就是公网 endpoint 的 origin。
    ///
    /// 这里是该 origin 的唯一推导入口：CSP 使用 [`crate::storage::Storage::browser_origin`]，
    /// 而 Storage 在启动时缓存本函数的结果，避免两处各自猜测。
    ///
    /// 若公网 endpoint 已带 `<bucket>.` 前缀，这里选择**启动期拒绝**而不是静默规范化：
    /// 自定义域名无法可靠判断该前缀是否真是 bucket 子域名，静默去重会掩盖配置错误；
    /// 要求填写标准 `https://oss-<region>.aliyuncs.com` 更可预测。
    pub(crate) fn browser_origin(&self) -> Result<String, ConfigError> {
        let uri: axum::http::Uri = self.public_endpoint.parse().map_err(|_| {
            ConfigError::invalid(keys::STORAGE_PUBLIC_ENDPOINT, "必须是合法的 http(s) URL")
        })?;
        let scheme = match uri.scheme_str() {
            Some("http") => "http",
            Some("https") => "https",
            _ => {
                return Err(ConfigError::invalid(
                    keys::STORAGE_PUBLIC_ENDPOINT,
                    "只支持 http:// 或 https:// 的 endpoint",
                ));
            }
        };
        let authority = uri
            .authority()
            .ok_or_else(|| ConfigError::invalid(keys::STORAGE_PUBLIC_ENDPOINT, "缺少主机名"))?;
        if self.kind == StorageKind::S3 {
            return Ok(origin_of(&self.public_endpoint));
        }

        let host = authority.host().to_ascii_lowercase();
        let bucket_prefix = format!("{}.", self.bucket.to_ascii_lowercase());
        if host.starts_with(&bucket_prefix) {
            return Err(ConfigError::invalid(
                keys::STORAGE_PUBLIC_ENDPOINT,
                "OSS endpoint 不能包含 bucket 子域名；请填写 https://oss-<region>.aliyuncs.com，由服务端自动拼接 bucket",
            ));
        }
        let host = format!("{}.{}", self.bucket, authority.host());
        match authority.port_u16() {
            Some(port) => Ok(format!("{scheme}://{host}:{port}")),
            None => Ok(format!("{scheme}://{host}")),
        }
    }
}

/// 邮件发送模式（design D13）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailMode {
    /// 写入本地目录，不真正发送（开发/测试）。
    File,
    /// 通过 SMTP 发送。
    Smtp,
}

impl MailMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Smtp => "smtp",
        }
    }
}

/// SMTP 连接加密方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpTls {
    /// 先明文连接，再用 STARTTLS 升级（通常 587）。
    Starttls,
    /// 从连接建立起就是 TLS（隐式 TLS / SMTPS，通常 465）。
    Implicit,
    /// 完全明文（只允许本地 mailpit 等测试环境，需显式开启 allow_insecure）。
    None,
}

impl SmtpTls {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starttls => "starttls",
            Self::Implicit => "implicit",
            Self::None => "none",
        }
    }

    /// 该模式下未显式配置端口时的默认端口。
    fn default_port(self) -> &'static str {
        match self {
            Self::Starttls => "587",
            Self::Implicit => "465",
            Self::None => "25",
        }
    }
}

/// SMTP 配置。
///
/// TLS 模式（`starttls` / `implicit`）始终要求用户名与密码；只有显式设置
/// `BACKUP_SMTP_ALLOW_INSECURE=true` 才允许明文连接（可无认证，本地 mailpit 测试用）。
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub from: String,
    pub username: Option<Secret>,
    pub password: Option<Secret>,
    pub tls: SmtpTls,
    /// 是否显式允许明文或无认证连接。
    pub allow_insecure: bool,
}

/// 邮件配置。
#[derive(Debug, Clone)]
pub enum MailConfig {
    File { dir: PathBuf },
    Smtp(SmtpConfig),
}

impl MailConfig {
    pub fn mode(&self) -> MailMode {
        match self {
            Self::File { .. } => MailMode::File,
            Self::Smtp(_) => MailMode::Smtp,
        }
    }
}

/// 服务端完整配置。
#[derive(Debug, Clone)]
pub struct Config {
    /// HTTP API 与前端静态文件的监听地址。
    pub bind_addr: SocketAddr,
    /// Prometheus 指标监听地址（默认只绑定回环地址）。
    pub metrics_addr: SocketAddr,
    /// 前端构建产物目录（SPA 回退到其中的 index.html）。
    pub static_dir: PathBuf,
    pub database: DatabaseConfig,
    pub storage: StorageConfig,
    /// 是否在本进程内运行后台任务（design D10；关闭后可拆分为独立 worker）。
    pub jobs_enabled: bool,
    pub mail: MailConfig,
    /// pepper 版本链与 prelogin 伪造盐密钥。
    pub auth: AuthConfig,
    /// 邮件中链接使用的前端外部地址（令牌只放在 `#token=` 片段里）。
    pub public_app_url: String,
    /// 只信任这些代理网段传来的 `X-Forwarded-For`（design D14）。
    pub trusted_proxies: Vec<IpNet>,
    /// 每个 IP 每分钟允许的认证类请求数（进程内令牌桶）。
    pub auth_rate_limit_per_minute: NonZeroU32,
    /// 每个已认证用户每分钟允许的写接口请求数（进程内令牌桶）。
    /// 批量 presign/confirm 各算一次请求，因此默认值按高吞吐上传估算。
    pub user_write_rate_limit_per_minute: NonZeroU32,
}

impl Config {
    /// 从进程环境变量加载配置。
    ///
    /// 这里不用 figment 的 `Env` provider：它会把 `true`、`123456` 之类的值
    /// 自动解析成 bool/数字，既可能把数字型密钥的前导零吃掉，也会让布尔配置
    /// 的读取路径变得不可预测。我们先把环境变量原样收集成字符串，再交给
    /// figment 的 `Serialized` provider 合并——配置解析仍然是 figment。
    pub fn load() -> Result<Self, ConfigError> {
        Self::from_figment(&env_figment())
    }

    /// 从任意 figment 数据源加载（测试用 `Serialized` 注入，避免依赖进程环境）。
    pub fn from_figment(figment: &Figment) -> Result<Self, ConfigError> {
        let bind_addr = parse_socket_addr(
            &string_with_default(figment, keys::BIND_ADDR, "127.0.0.1:8080")?,
            keys::BIND_ADDR,
        )?;
        let metrics_addr = parse_socket_addr(
            &string_with_default(figment, keys::METRICS_ADDR, "127.0.0.1:9091")?,
            keys::METRICS_ADDR,
        )?;
        let static_dir = PathBuf::from(string_with_default(figment, keys::STATIC_DIR, "web/dist")?);

        let database = parse_database_config(figment)?;

        let kind = parse_storage_kind(&required_string(figment, keys::STORAGE_KIND)?)?;
        let bucket = required_string(figment, keys::STORAGE_BUCKET)?;
        let region = match optional_string(figment, keys::STORAGE_REGION)? {
            Some(region) => region,
            None if kind == StorageKind::Oss => {
                return Err(ConfigError::Missing {
                    key: keys::STORAGE_REGION,
                });
            }
            None => "us-east-1".to_owned(),
        };
        let (internal_endpoint, internal_scheme) =
            parse_endpoint(figment, keys::STORAGE_INTERNAL_ENDPOINT)?;
        let (public_endpoint, public_scheme) =
            parse_endpoint(figment, keys::STORAGE_PUBLIC_ENDPOINT)?;
        let access_key_id = required_secret(figment, keys::STORAGE_ACCESS_KEY_ID)?;
        let secret_access_key = required_secret(figment, keys::STORAGE_SECRET_ACCESS_KEY)?;
        let allow_http = parse_bool(
            &string_with_default(figment, keys::STORAGE_ALLOW_HTTP, "false")?,
            keys::STORAGE_ALLOW_HTTP,
        )?;
        if !allow_http && internal_scheme == "http" {
            return Err(ConfigError::invalid(
                keys::STORAGE_INTERNAL_ENDPOINT,
                "http 端点必须显式设置 BACKUP_STORAGE_ALLOW_HTTP=true",
            ));
        }
        if !allow_http && public_scheme == "http" {
            return Err(ConfigError::invalid(
                keys::STORAGE_PUBLIC_ENDPOINT,
                "http 端点必须显式设置 BACKUP_STORAGE_ALLOW_HTTP=true",
            ));
        }

        let jobs_enabled = parse_bool(
            &string_with_default(figment, keys::JOBS_ENABLED, "true")?,
            keys::JOBS_ENABLED,
        )?;
        let mail = parse_mail_config(figment)?;
        let auth = parse_auth_config(figment)?;
        let public_app_url = parse_public_app_url(figment)?;
        let trusted_proxies = parse_trusted_proxies(figment)?;
        let auth_rate_limit_per_minute = parse_auth_rate_limit(figment)?;
        let user_write_rate_limit_per_minute = parse_user_write_rate_limit(figment)?;

        // 启动期校验浏览器访问 origin：OSS 的 public endpoint 必须是不带 bucket
        // 的标准 endpoint，否则预签名 URL 的 host 会变成 bucket.bucket.host。
        let storage = StorageConfig {
            kind,
            bucket,
            region,
            internal_endpoint,
            public_endpoint,
            access_key_id,
            secret_access_key,
            allow_http,
        };
        storage.browser_origin()?;

        Ok(Self {
            bind_addr,
            metrics_addr,
            static_dir,
            database,
            storage,
            jobs_enabled,
            mail,
            auth,
            public_app_url,
            trusted_proxies,
            auth_rate_limit_per_minute,
            user_write_rate_limit_per_minute,
        })
    }
}

/// 解析 pepper 版本链与 prelogin 伪造盐密钥。
fn parse_auth_config(figment: &Figment) -> Result<AuthConfig, ConfigError> {
    let pepper_version = parse_i32(
        &required_string(figment, keys::AUTH_PEPPER_VERSION)?,
        keys::AUTH_PEPPER_VERSION,
        1..=i32::MAX,
    )?;
    let pepper = required_secret(figment, keys::AUTH_PEPPER)?;
    if pepper.expose().len() < MIN_PEPPER_BYTES {
        return Err(ConfigError::invalid(
            keys::AUTH_PEPPER,
            "pepper 至少需要 32 字节",
        ));
    }

    let fake_salt_key = required_secret(figment, keys::AUTH_FAKE_SALT_KEY)?;
    if fake_salt_key.expose().len() < MIN_PEPPER_BYTES {
        return Err(ConfigError::invalid(
            keys::AUTH_FAKE_SALT_KEY,
            "伪造盐密钥至少需要 32 字节",
        ));
    }
    if fake_salt_key.expose() == pepper.expose() {
        return Err(ConfigError::invalid(
            keys::AUTH_FAKE_SALT_KEY,
            "伪造盐密钥必须与 pepper 分开",
        ));
    }

    let mut old_peppers = Vec::new();
    if let Some(raw) = optional_string(figment, keys::AUTH_OLD_PEPPERS)? {
        let parsed: BTreeMap<String, String> = serde_json::from_str(&raw).map_err(|_| {
            ConfigError::invalid(
                keys::AUTH_OLD_PEPPERS,
                "必须是 {\"版本\":\"pepper\"} 形式的 JSON 对象",
            )
        })?;
        for (version, value) in parsed {
            let version: i32 = version.parse().map_err(|_| {
                ConfigError::invalid(keys::AUTH_OLD_PEPPERS, "旧 pepper 版本必须是正整数")
            })?;
            if version <= 0 {
                return Err(ConfigError::invalid(
                    keys::AUTH_OLD_PEPPERS,
                    "旧 pepper 版本必须是正整数",
                ));
            }
            if version == pepper_version {
                return Err(ConfigError::invalid(
                    keys::AUTH_OLD_PEPPERS,
                    "旧 pepper 版本不能与当前版本相同",
                ));
            }
            if value.len() < MIN_PEPPER_BYTES {
                return Err(ConfigError::invalid(
                    keys::AUTH_OLD_PEPPERS,
                    "旧 pepper 至少需要 32 字节",
                ));
            }
            old_peppers.push((version, Secret::new(value)));
        }
        old_peppers.sort_by_key(|(version, _)| *version);
    }

    Ok(AuthConfig {
        pepper_version,
        pepper,
        old_peppers,
        fake_salt_key,
    })
}

/// 邮件链接使用的前端外部地址。
fn parse_public_app_url(figment: &Figment) -> Result<String, ConfigError> {
    let value = string_with_default(figment, keys::PUBLIC_APP_URL, "http://127.0.0.1:8080")?;
    let uri: axum::http::Uri = value
        .parse()
        .map_err(|_| ConfigError::invalid(keys::PUBLIC_APP_URL, "必须是合法的 http(s) URL"))?;
    if !matches!(uri.scheme_str(), Some("http") | Some("https")) || uri.authority().is_none() {
        return Err(ConfigError::invalid(
            keys::PUBLIC_APP_URL,
            "必须是合法的 http(s) URL",
        ));
    }
    if value.contains('?') || value.contains('#') {
        return Err(ConfigError::invalid(
            keys::PUBLIC_APP_URL,
            "不能包含查询串或片段",
        ));
    }
    Ok(value.trim_end_matches('/').to_owned())
}

/// 解析可信代理网段列表（逗号分隔的 CIDR，也接受裸 IP）。
fn parse_trusted_proxies(figment: &Figment) -> Result<Vec<IpNet>, ConfigError> {
    let Some(raw) = optional_string(figment, keys::TRUSTED_PROXIES)? else {
        return Ok(Vec::new());
    };
    let mut networks = Vec::new();
    for part in raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let candidate = if part.contains('/') {
            part.to_owned()
        } else {
            // 裸 IP 视为单个地址（IPv4 /32、IPv6 /128）。
            let ip: std::net::IpAddr = part.parse().map_err(|_| {
                ConfigError::invalid(keys::TRUSTED_PROXIES, "必须是逗号分隔的 CIDR 或 IP 地址")
            })?;
            match ip {
                std::net::IpAddr::V4(_) => format!("{ip}/32"),
                std::net::IpAddr::V6(_) => format!("{ip}/128"),
            }
        };
        let network = candidate.parse::<IpNet>().map_err(|_| {
            ConfigError::invalid(keys::TRUSTED_PROXIES, "必须是逗号分隔的 CIDR 或 IP 地址")
        })?;
        networks.push(network);
    }
    Ok(networks)
}

/// 每个 IP 每分钟的认证类请求上限。
fn parse_auth_rate_limit(figment: &Figment) -> Result<NonZeroU32, ConfigError> {
    let value = parse_u32(
        &string_with_default(figment, keys::AUTH_RATE_LIMIT_PER_MINUTE, "60")?,
        keys::AUTH_RATE_LIMIT_PER_MINUTE,
        1..=100_000,
    )?;
    NonZeroU32::new(value).ok_or(ConfigError::invalid(
        keys::AUTH_RATE_LIMIT_PER_MINUTE,
        "必须是正整数",
    ))
}

/// 每个已认证用户每分钟的写接口上限。
fn parse_user_write_rate_limit(figment: &Figment) -> Result<NonZeroU32, ConfigError> {
    let default = DEFAULT_USER_WRITE_RATE_LIMIT_PER_MINUTE.to_string();
    let value = parse_u32(
        &string_with_default(figment, keys::USER_WRITE_RATE_LIMIT_PER_MINUTE, &default)?,
        keys::USER_WRITE_RATE_LIMIT_PER_MINUTE,
        1..=1_000_000,
    )?;
    NonZeroU32::new(value).ok_or(ConfigError::invalid(
        keys::USER_WRITE_RATE_LIMIT_PER_MINUTE,
        "必须是正整数",
    ))
}

/// 只解析数据库配置（`serve` 与 `migrate` 共用）。
fn parse_database_config(figment: &Figment) -> Result<DatabaseConfig, ConfigError> {
    let url = required_secret(figment, keys::DATABASE_URL)?;
    if !(url.expose().starts_with("postgres://") || url.expose().starts_with("postgresql://")) {
        return Err(ConfigError::invalid(
            keys::DATABASE_URL,
            "必须是 postgres:// 或 postgresql:// 连接串",
        ));
    }
    let max_connections = parse_u32(
        &string_with_default(figment, keys::DATABASE_MAX_CONNECTIONS, "10")?,
        keys::DATABASE_MAX_CONNECTIONS,
        1..=1024,
    )?;
    Ok(DatabaseConfig {
        url,
        max_connections,
    })
}

/// 解析邮件配置。
///
/// `BACKUP_MAIL_MODE` 没有默认值：显式选择 `file` 或 `smtp`，缺失时启动失败并指出项名。
fn parse_mail_config(figment: &Figment) -> Result<MailConfig, ConfigError> {
    let mode = required_string(figment, keys::MAIL_MODE)?;
    match mode.trim().to_ascii_lowercase().as_str() {
        "file" => Ok(MailConfig::File {
            dir: PathBuf::from(string_with_default(figment, keys::MAIL_DIR, "var/mail")?),
        }),
        "smtp" => {
            let host = required_string(figment, keys::SMTP_HOST)?;
            let tls = match string_with_default(figment, keys::SMTP_TLS, "starttls")?
                .trim()
                .to_ascii_lowercase()
                .as_str()
            {
                "starttls" => SmtpTls::Starttls,
                "implicit" => SmtpTls::Implicit,
                "none" => SmtpTls::None,
                _ => {
                    return Err(ConfigError::invalid(
                        keys::SMTP_TLS,
                        "必须是 starttls、implicit 或 none",
                    ));
                }
            };
            let port = parse_u16(
                &string_with_default(figment, keys::SMTP_PORT, tls.default_port())?,
                keys::SMTP_PORT,
            )?;
            let from = required_string(figment, keys::SMTP_FROM)?;
            if !from.contains('@') {
                return Err(ConfigError::invalid(keys::SMTP_FROM, "必须是邮件地址"));
            }
            let username = optional_secret(figment, keys::SMTP_USERNAME)?;
            let password = optional_secret(figment, keys::SMTP_PASSWORD)?;
            if username.is_some() != password.is_some() {
                return Err(ConfigError::invalid(
                    keys::SMTP_PASSWORD,
                    "SMTP 用户名与密码必须同时配置或同时省略",
                ));
            }
            let allow_insecure = parse_bool(
                &string_with_default(figment, keys::SMTP_ALLOW_INSECURE, "false")?,
                keys::SMTP_ALLOW_INSECURE,
            )?;
            // 安全默认：必须 TLS 且必须认证。
            // - TLS 模式下用户名/密码始终必填；
            // - 明文连接（含无认证）只在 `allow_insecure` 显式开启时允许（本地 mailpit）。
            if tls != SmtpTls::None && username.is_none() {
                return Err(ConfigError::invalid(
                    keys::SMTP_USERNAME,
                    "TLS 模式下用户名与密码必填",
                ));
            }
            if !allow_insecure && tls == SmtpTls::None {
                return Err(ConfigError::invalid(
                    keys::SMTP_ALLOW_INSECURE,
                    "明文 SMTP 必须显式设置 BACKUP_SMTP_ALLOW_INSECURE=true",
                ));
            }
            Ok(MailConfig::Smtp(SmtpConfig {
                host,
                port,
                from,
                username,
                password,
                tls,
                allow_insecure,
            }))
        }
        _ => Err(ConfigError::invalid(keys::MAIL_MODE, "必须是 file 或 smtp")),
    }
}

/// 把进程环境变量中的 `BACKUP_*` 原样收集成 figment 数据源。
///
/// 不用 figment 的 `Env` provider：它会把 `true`、`123456` 之类的值自动解析成
/// bool/数字，既可能吃掉数字型密钥的前导零，也让布尔配置的读取路径不可预测。
fn env_figment() -> Figment {
    let values: std::collections::HashMap<String, String> = std::env::vars()
        .filter_map(|(key, value)| {
            key.strip_prefix(ENV_PREFIX)
                .map(|name| (name.to_ascii_lowercase(), value))
        })
        .collect();
    Figment::new().merge(Serialized::defaults(values))
}

/// 环境变量名 → figment 内部键：去掉 `BACKUP_` 前缀并小写。
///
/// 集中在这里换算，保证环境变量名与读取逻辑不会漂移。
fn figment_key(env_key: &'static str) -> String {
    env_key
        .strip_prefix(ENV_PREFIX)
        .unwrap_or(env_key)
        .to_ascii_lowercase()
}

fn optional_string(
    figment: &Figment,
    env_key: &'static str,
) -> Result<Option<String>, ConfigError> {
    match figment.find_value(&figment_key(env_key)) {
        Ok(value) => value
            .deserialize::<String>()
            .map(Some)
            .map_err(|_| ConfigError::invalid(env_key, "不是合法的字符串")),
        // 配置不存在（或其他 provider 层面的缺失）都按缺项处理，
        // 不要向上传递 figment 的原始错误文本，避免把值带进日志。
        Err(_) => Ok(None),
    }
}

fn required_string(figment: &Figment, env_key: &'static str) -> Result<String, ConfigError> {
    let value = optional_string(figment, env_key)?.ok_or(ConfigError::Missing { key: env_key })?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::invalid(env_key, "不能为空"));
    }
    Ok(trimmed.to_owned())
}

fn required_secret(figment: &Figment, env_key: &'static str) -> Result<Secret, ConfigError> {
    let value = optional_string(figment, env_key)?.ok_or(ConfigError::Missing { key: env_key })?;
    if value.trim().is_empty() {
        return Err(ConfigError::invalid(env_key, "不能为空"));
    }
    Ok(Secret::new(value))
}

fn optional_secret(
    figment: &Figment,
    env_key: &'static str,
) -> Result<Option<Secret>, ConfigError> {
    match optional_string(figment, env_key)? {
        Some(value) if value.trim().is_empty() => Ok(None),
        Some(value) => Ok(Some(Secret::new(value))),
        None => Ok(None),
    }
}

fn string_with_default(
    figment: &Figment,
    env_key: &'static str,
    default: &str,
) -> Result<String, ConfigError> {
    match optional_string(figment, env_key)? {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Err(ConfigError::invalid(env_key, "不能为空"))
            } else {
                Ok(trimmed.to_owned())
            }
        }
        None => Ok(default.to_owned()),
    }
}

fn parse_socket_addr(value: &str, env_key: &'static str) -> Result<SocketAddr, ConfigError> {
    value.parse().map_err(|_| {
        ConfigError::invalid(
            env_key,
            "必须是 host:port 形式的监听地址（例如 127.0.0.1:8080）",
        )
    })
}

fn parse_i32(
    value: &str,
    env_key: &'static str,
    range: std::ops::RangeInclusive<i32>,
) -> Result<i32, ConfigError> {
    let parsed = value
        .parse::<i32>()
        .map_err(|_| ConfigError::invalid(env_key, "必须是正整数"))?;
    if !range.contains(&parsed) {
        return Err(ConfigError::invalid(env_key, "超出允许范围"));
    }
    Ok(parsed)
}

fn parse_u32(
    value: &str,
    env_key: &'static str,
    range: std::ops::RangeInclusive<u32>,
) -> Result<u32, ConfigError> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| ConfigError::invalid(env_key, "必须是正整数"))?;
    if !range.contains(&parsed) {
        return Err(ConfigError::invalid(env_key, "超出允许范围"));
    }
    Ok(parsed)
}

fn parse_u16(value: &str, env_key: &'static str) -> Result<u16, ConfigError> {
    let parsed = value
        .parse::<u16>()
        .map_err(|_| ConfigError::invalid(env_key, "必须是 1-65535 的端口号"))?;
    if parsed == 0 {
        return Err(ConfigError::invalid(env_key, "必须是 1-65535 的端口号"));
    }
    Ok(parsed)
}

fn parse_bool(value: &str, env_key: &'static str) -> Result<bool, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(ConfigError::invalid(env_key, "必须是 true 或 false")),
    }
}

fn parse_storage_kind(value: &str) -> Result<StorageKind, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "s3" => Ok(StorageKind::S3),
        "oss" => Ok(StorageKind::Oss),
        _ => Err(ConfigError::invalid(keys::STORAGE_KIND, "必须是 s3 或 oss")),
    }
}

/// 校验并规范化 endpoint，返回 `(去掉末尾斜杠的 URL, scheme)`。
fn parse_endpoint(
    figment: &Figment,
    env_key: &'static str,
) -> Result<(String, String), ConfigError> {
    let value = required_string(figment, env_key)?;
    let uri: axum::http::Uri = value
        .parse()
        .map_err(|_| ConfigError::invalid(env_key, "必须是合法的 http(s) URL"))?;
    let scheme = match uri.scheme_str() {
        Some("http") => "http",
        Some("https") => "https",
        _ => {
            return Err(ConfigError::invalid(
                env_key,
                "只支持 http:// 或 https:// 的 endpoint",
            ));
        }
    };
    let authority = uri
        .authority()
        .ok_or(ConfigError::invalid(env_key, "缺少主机名"))?;
    if !uri.path().is_empty() && uri.path() != "/" {
        return Err(ConfigError::invalid(env_key, "endpoint 不能包含路径部分"));
    }
    Ok((format!("{scheme}://{authority}"), scheme.to_owned()))
}

/// 从已校验的 endpoint 中取出 origin（`scheme://authority`）。
fn origin_of(endpoint: &str) -> String {
    match endpoint.split_once("://") {
        Some((scheme, rest)) => {
            let authority = rest.split('/').next().unwrap_or(rest);
            format!("{scheme}://{authority}")
        }
        // endpoint 一定经过 parse_endpoint 校验；这里保底返回原值。
        None => endpoint.to_owned(),
    }
}

/// 测试专用的配置输入：键为不带前缀的环境变量名（与 figment 内部键相同）。
#[cfg(test)]
fn figment_from(pairs: &[(&str, &str)]) -> Figment {
    use figment::providers::Serialized;
    use std::collections::HashMap;

    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    Figment::new().merge(Serialized::defaults(map))
}

#[cfg(test)]
mod tests {
    // 测试代码允许使用 expect/unwrap：断言失败时 panic 是预期行为。
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn complete_pairs() -> Vec<(&'static str, &'static str)> {
        vec![
            ("database_url", "postgres://backup:pw@127.0.0.1:5432/backup"),
            ("storage_kind", "s3"),
            ("storage_bucket", "backup"),
            ("storage_region", "us-east-1"),
            ("storage_internal_endpoint", "http://127.0.0.1:9000/"),
            ("storage_public_endpoint", "http://127.0.0.1:9000"),
            ("storage_access_key_id", "minio-access-key-value"),
            ("storage_secret_access_key", "minio-secret-key-value"),
            ("storage_allow_http", "true"),
            ("mail_mode", "file"),
            ("auth_pepper_version", "1"),
            ("auth_pepper", "test-pepper-value-0123456789abcdef"),
            ("auth_fake_salt_key", "test-fake-salt-key-0123456789abcdef"),
        ]
    }

    fn load(pairs: &[(&'static str, &'static str)]) -> Result<Config, ConfigError> {
        Config::from_figment(&figment_from(pairs))
    }

    #[test]
    fn loads_complete_config_with_defaults() {
        let config = load(&complete_pairs()).expect("配置应当合法");
        assert_eq!(config.bind_addr.to_string(), "127.0.0.1:8080");
        assert_eq!(config.metrics_addr.to_string(), "127.0.0.1:9091");
        assert_eq!(config.database.max_connections, 10);
        assert_eq!(config.storage.kind, StorageKind::S3);
        assert_eq!(
            config.storage.browser_origin().expect("S3 origin 应合法"),
            "http://127.0.0.1:9000"
        );
        assert_eq!(config.storage.internal_endpoint, "http://127.0.0.1:9000");
    }

    #[test]
    fn migrate_config_loads_with_only_database_url() {
        let pairs = [("database_url", "postgres://backup:pw@127.0.0.1:5432/backup")];
        let config =
            MigrateConfig::from_figment(&figment_from(&pairs)).expect("迁移配置只要求数据库连接串");
        assert_eq!(config.database.max_connections, 10);
        assert_eq!(
            config.database.url.expose(),
            "postgres://backup:pw@127.0.0.1:5432/backup"
        );
    }

    #[test]
    fn migrate_config_still_requires_database_url() {
        let err = MigrateConfig::from_figment(&figment_from(&[]))
            .expect_err("缺少数据库连接串时 migrate 配置必须失败");
        assert_eq!(
            err,
            ConfigError::Missing {
                key: keys::DATABASE_URL
            }
        );
    }

    #[test]
    fn missing_database_url_reports_item_name() {
        let pairs: Vec<_> = complete_pairs()
            .into_iter()
            .filter(|(key, _)| *key != "database_url")
            .collect();
        let err = load(&pairs).expect_err("缺少数据库连接串必须失败");
        assert_eq!(
            err,
            ConfigError::Missing {
                key: keys::DATABASE_URL
            }
        );
        assert!(err.to_string().contains("BACKUP_DATABASE_URL"));
    }

    #[test]
    fn missing_storage_credentials_report_item_name() {
        let pairs: Vec<_> = complete_pairs()
            .into_iter()
            .filter(|(key, _)| *key != "storage_access_key_id")
            .collect();
        let err = load(&pairs).expect_err("缺少对象存储凭据必须失败");
        assert_eq!(
            err,
            ConfigError::Missing {
                key: keys::STORAGE_ACCESS_KEY_ID
            }
        );
        assert!(err.to_string().contains("BACKUP_STORAGE_ACCESS_KEY_ID"));
    }

    #[test]
    fn invalid_values_are_rejected_without_echoing_them() {
        let mut pairs = complete_pairs();
        pairs.push(("database_max_connections", "0"));
        let err = load(&pairs).expect_err("连接数 0 必须被拒绝");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::DATABASE_MAX_CONNECTIONS,
                reason: "超出允许范围"
            }
        );
        // 错误只包含配置项名与原因，不包含非法值。
        assert_eq!(
            err.to_string(),
            "配置项 BACKUP_DATABASE_MAX_CONNECTIONS 非法：超出允许范围"
        );

        let mut pairs = complete_pairs();
        pairs.push(("storage_kind", "ftp"));
        let err = load(&pairs).expect_err("未知存储类型必须被拒绝");
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: keys::STORAGE_KIND,
                ..
            }
        ));

        let mut pairs = complete_pairs();
        pairs.push(("bind_addr", "not-a-socket-addr"));
        let err = load(&pairs).expect_err("非法监听地址必须被拒绝");
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: keys::BIND_ADDR,
                ..
            }
        ));
    }

    #[test]
    fn http_endpoint_requires_explicit_opt_in() {
        let mut pairs = complete_pairs();
        pairs.push(("storage_allow_http", "false"));
        let err = load(&pairs).expect_err("http endpoint 必须显式允许");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::STORAGE_INTERNAL_ENDPOINT,
                reason: "http 端点必须显式设置 BACKUP_STORAGE_ALLOW_HTTP=true"
            }
        );
    }

    #[test]
    fn oss_requires_region() {
        let pairs: Vec<_> = complete_pairs()
            .into_iter()
            .filter(|(key, _)| *key != "storage_region")
            .chain([("storage_kind", "oss")])
            .collect();
        let err = load(&pairs).expect_err("OSS 必须显式配置 region");
        assert_eq!(
            err,
            ConfigError::Missing {
                key: keys::STORAGE_REGION
            }
        );
    }

    fn oss_pairs() -> Vec<(&'static str, &'static str)> {
        let mut pairs = complete_pairs();
        pairs.push(("storage_kind", "oss"));
        pairs.push(("storage_region", "oss-cn-hangzhou"));
        pairs.push((
            "storage_public_endpoint",
            "https://oss-cn-hangzhou.aliyuncs.com",
        ));
        pairs
    }

    #[test]
    fn oss_browser_origin_uses_bucket_subdomain() {
        let config = load(&oss_pairs()).expect("OSS 配置应当合法");
        assert_eq!(
            config.storage.browser_origin().expect("OSS origin 应合法"),
            "https://backup.oss-cn-hangzhou.aliyuncs.com"
        );
    }

    #[test]
    fn oss_rejects_public_endpoint_with_bucket_prefix() {
        let mut pairs = oss_pairs();
        pairs.push((
            "storage_public_endpoint",
            "https://backup.oss-cn-hangzhou.aliyuncs.com",
        ));
        let err = load(&pairs).expect_err("带 bucket 子域名的 endpoint 必须被拒绝");
        match err {
            ConfigError::Invalid { key, reason } => {
                assert_eq!(key, keys::STORAGE_PUBLIC_ENDPOINT);
                assert!(
                    reason.contains("bucket 子域名"),
                    "错误原因应说明 bucket 子域名问题：{reason}"
                );
            }
            other => panic!("应为配置错误，实际: {other:?}"),
        }
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let config = load(&complete_pairs()).expect("配置应当合法");
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("minio-access-key-value"));
        assert!(!debug.contains("minio-secret-key-value"));
        assert!(!debug.contains("backup:pw@"));
        // 非密钥字段仍然可见，便于排查配置问题。
        assert!(debug.contains("127.0.0.1:9000"));
    }

    #[test]
    fn config_error_display_never_contains_values() {
        let mut pairs = complete_pairs();
        pairs.retain(|(key, _)| *key != "storage_secret_access_key");
        pairs.push(("storage_secret_access_key", ""));
        let err = load(&pairs).expect_err("空密钥必须失败");
        let text = err.to_string();
        assert!(text.contains("BACKUP_STORAGE_SECRET_ACCESS_KEY"));
        assert!(!text.contains("minio-secret-key-value"));
    }

    #[test]
    fn env_prefix_maps_to_internal_keys() {
        assert_eq!(figment_key(keys::DATABASE_URL), "database_url");
        assert_eq!(
            figment_key(keys::STORAGE_SECRET_ACCESS_KEY),
            "storage_secret_access_key"
        );
    }

    #[test]
    fn numeric_looking_secrets_are_preserved_verbatim() {
        let mut pairs = complete_pairs();
        pairs.push(("storage_access_key_id", "000123456"));
        pairs.push(("storage_secret_access_key", "1234567890"));
        let config = load(&pairs).expect("配置应当合法");
        assert_eq!(config.storage.access_key_id.expose(), "000123456");
        assert_eq!(config.storage.secret_access_key.expose(), "1234567890");
    }

    #[test]
    fn jobs_are_enabled_by_default_and_can_be_disabled() {
        let config = load(&complete_pairs()).expect("配置应当合法");
        assert!(config.jobs_enabled);

        let mut pairs = complete_pairs();
        pairs.push(("jobs_enabled", "false"));
        let config = load(&pairs).expect("配置应当合法");
        assert!(!config.jobs_enabled);
    }

    #[test]
    fn file_mode_uses_default_mail_dir() {
        let config = load(&complete_pairs()).expect("配置应当合法");
        match config.mail {
            MailConfig::File { dir } => assert_eq!(dir, PathBuf::from("var/mail")),
            MailConfig::Smtp(_) => panic!("应为 file 模式"),
        }
    }

    #[test]
    fn missing_mail_mode_names_the_item() {
        let pairs: Vec<_> = complete_pairs()
            .into_iter()
            .filter(|(key, _)| *key != "mail_mode")
            .collect();
        let err = load(&pairs).expect_err("缺少邮件模式必须启动失败");
        assert_eq!(
            err,
            ConfigError::Missing {
                key: keys::MAIL_MODE
            }
        );
        assert!(err.to_string().contains("BACKUP_MAIL_MODE"));
    }

    #[test]
    fn unknown_mail_mode_is_rejected() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "sendmail"));
        let err = load(&pairs).expect_err("未知邮件模式必须失败");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::MAIL_MODE,
                reason: "必须是 file 或 smtp"
            }
        );
    }

    #[test]
    fn smtp_mode_requires_host_and_from() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        let err = load(&pairs).expect_err("缺少 SMTP_HOST 必须失败");
        assert_eq!(
            err,
            ConfigError::Missing {
                key: keys::SMTP_HOST
            }
        );

        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "smtp.example.com"));
        let err = load(&pairs).expect_err("缺少 SMTP_FROM 必须失败");
        assert_eq!(
            err,
            ConfigError::Missing {
                key: keys::SMTP_FROM
            }
        );
    }

    #[test]
    fn smtp_credentials_must_be_paired() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "smtp.example.com"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_username", "user"));
        let err = load(&pairs).expect_err("只有用户名必须失败");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::SMTP_PASSWORD,
                reason: "SMTP 用户名与密码必须同时配置或同时省略"
            }
        );
    }

    #[test]
    fn smtp_mode_accepts_secure_full_configuration() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "smtp.example.com"));
        pairs.push(("smtp_port", "2525"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_username", "user"));
        pairs.push(("smtp_password", "smtp-secret-value"));
        pairs.push(("smtp_tls", "implicit"));
        let config = load(&pairs).expect("配置应当合法");
        match &config.mail {
            MailConfig::Smtp(smtp) => {
                assert_eq!(smtp.host, "smtp.example.com");
                assert_eq!(smtp.port, 2525);
                assert_eq!(smtp.tls, SmtpTls::Implicit);
                assert!(!smtp.allow_insecure);
                assert!(smtp.username.is_some());
                assert!(smtp.password.is_some());
            }
            MailConfig::File { .. } => panic!("应为 smtp 模式"),
        }
        let debug = format!("{config:?}");
        assert!(!debug.contains("smtp-secret-value"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn smtp_tls_defaults_to_starttls_port_587() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "smtp.example.com"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_username", "user"));
        pairs.push(("smtp_password", "smtp-secret-value"));
        let config = load(&pairs).expect("配置应当合法");
        match &config.mail {
            MailConfig::Smtp(smtp) => {
                assert_eq!(smtp.tls, SmtpTls::Starttls);
                assert_eq!(smtp.port, 587);
            }
            MailConfig::File { .. } => panic!("应为 smtp 模式"),
        }
    }

    #[test]
    fn smtp_implicit_tls_defaults_to_port_465() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "smtp.example.com"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_username", "user"));
        pairs.push(("smtp_password", "smtp-secret-value"));
        pairs.push(("smtp_tls", "implicit"));
        let config = load(&pairs).expect("配置应当合法");
        match &config.mail {
            MailConfig::Smtp(smtp) => {
                assert_eq!(smtp.tls, SmtpTls::Implicit);
                assert_eq!(smtp.port, 465);
            }
            MailConfig::File { .. } => panic!("应为 smtp 模式"),
        }
    }

    #[test]
    fn smtp_unknown_tls_mode_is_rejected() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "smtp.example.com"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_username", "user"));
        pairs.push(("smtp_password", "smtp-secret-value"));
        pairs.push(("smtp_tls", "ssl"));
        let err = load(&pairs).expect_err("未知 TLS 模式必须失败");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::SMTP_TLS,
                reason: "必须是 starttls、implicit 或 none"
            }
        );
    }

    #[test]
    fn smtp_tls_requires_credentials_by_default() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "smtp.example.com"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_tls", "starttls"));
        let err = load(&pairs).expect_err("TLS 模式下无认证必须失败");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::SMTP_USERNAME,
                reason: "TLS 模式下用户名与密码必填"
            }
        );
    }

    #[test]
    fn smtp_plaintext_requires_explicit_opt_in() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "127.0.0.1"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_username", "user"));
        pairs.push(("smtp_password", "smtp-secret-value"));
        pairs.push(("smtp_tls", "none"));
        let err = load(&pairs).expect_err("明文 SMTP 默认必须失败");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::SMTP_ALLOW_INSECURE,
                reason: "明文 SMTP 必须显式设置 BACKUP_SMTP_ALLOW_INSECURE=true"
            }
        );
        assert!(err.to_string().contains("BACKUP_SMTP_ALLOW_INSECURE"));
    }

    #[test]
    fn smtp_insecure_opt_in_allows_plaintext_and_missing_credentials() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "127.0.0.1"));
        pairs.push(("smtp_port", "1025"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_tls", "none"));
        pairs.push(("smtp_allow_insecure", "true"));
        let config = load(&pairs).expect("显式允许不安全连接时应当合法");
        match &config.mail {
            MailConfig::Smtp(smtp) => {
                assert_eq!(smtp.tls, SmtpTls::None);
                assert_eq!(smtp.port, 1025);
                assert!(smtp.allow_insecure);
                assert!(smtp.username.is_none());
                assert!(smtp.password.is_none());
            }
            MailConfig::File { .. } => panic!("应为 smtp 模式"),
        }
    }

    #[test]
    fn smtp_tls_never_allows_missing_credentials_even_with_insecure_flag() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "smtp.example.com"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_tls", "starttls"));
        pairs.push(("smtp_allow_insecure", "true"));
        let err = load(&pairs).expect_err("TLS 模式始终要求用户名与密码");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::SMTP_USERNAME,
                reason: "TLS 模式下用户名与密码必填"
            }
        );
    }

    #[test]
    fn smtp_allow_insecure_rejects_invalid_boolean() {
        let mut pairs = complete_pairs();
        pairs.push(("mail_mode", "smtp"));
        pairs.push(("smtp_host", "127.0.0.1"));
        pairs.push(("smtp_from", "backup@example.com"));
        pairs.push(("smtp_tls", "none"));
        pairs.push(("smtp_allow_insecure", "maybe"));
        let err = load(&pairs).expect_err("非法布尔值必须失败");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::SMTP_ALLOW_INSECURE,
                reason: "必须是 true 或 false"
            }
        );
    }

    #[test]
    fn auth_pepper_is_required_and_redacted() {
        let pairs: Vec<_> = complete_pairs()
            .into_iter()
            .filter(|(key, _)| *key != "auth_pepper")
            .collect();
        let err = load(&pairs).expect_err("缺少 pepper 必须失败");
        assert_eq!(
            err,
            ConfigError::Missing {
                key: keys::AUTH_PEPPER
            }
        );

        let config = load(&complete_pairs()).expect("配置应当合法");
        let debug = format!("{config:?}");
        assert!(!debug.contains("test-pepper-value-0123456789abcdef"));
        assert!(!debug.contains("test-fake-salt-key-0123456789abcdef"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn short_pepper_is_rejected() {
        let mut pairs = complete_pairs();
        pairs.retain(|(key, _)| *key != "auth_pepper");
        pairs.push(("auth_pepper", "too-short"));
        let err = load(&pairs).expect_err("过短 pepper 必须失败");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::AUTH_PEPPER,
                reason: "pepper 至少需要 32 字节"
            }
        );
    }

    #[test]
    fn fake_salt_key_must_be_configured_separately() {
        let mut pairs = complete_pairs();
        pairs.retain(|(key, _)| *key != "auth_fake_salt_key");
        pairs.push(("auth_fake_salt_key", "test-pepper-value-0123456789abcdef"));
        let err = load(&pairs).expect_err("伪造盐密钥不得与 pepper 相同");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::AUTH_FAKE_SALT_KEY,
                reason: "伪造盐密钥必须与 pepper 分开"
            }
        );
    }

    #[test]
    fn old_peppers_are_parsed_and_looked_up_by_version() {
        let mut pairs = complete_pairs();
        pairs.push((
            "auth_old_peppers",
            r#"{"1":"old-pepper-value-0123456789abcdef"}"#,
        ));
        pairs.push(("auth_pepper_version", "2"));
        let config = load(&pairs).expect("配置应当合法");
        assert_eq!(config.auth.pepper_version, 2);
        assert!(config.auth.pepper_for(1).is_some());
        assert!(config.auth.pepper_for(2).is_some());
        assert!(config.auth.pepper_for(3).is_none());
        let debug = format!("{config:?}");
        assert!(!debug.contains("old-pepper-value-0123456789abcdef"));
    }

    #[test]
    fn old_pepper_version_cannot_equal_current_version() {
        let mut pairs = complete_pairs();
        pairs.push((
            "auth_old_peppers",
            r#"{"1":"old-pepper-value-0123456789abcdef"}"#,
        ));
        let err = load(&pairs).expect_err("旧 pepper 版本不得与当前版本相同");
        assert_eq!(
            err,
            ConfigError::Invalid {
                key: keys::AUTH_OLD_PEPPERS,
                reason: "旧 pepper 版本不能与当前版本相同"
            }
        );
    }

    #[test]
    fn invalid_old_peppers_json_is_rejected() {
        let mut pairs = complete_pairs();
        pairs.push(("auth_old_peppers", "not-json"));
        let err = load(&pairs).expect_err("非法旧 pepper JSON 必须失败");
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: keys::AUTH_OLD_PEPPERS,
                ..
            }
        ));
    }

    #[test]
    fn trusted_proxies_accept_cidrs_and_bare_ips() {
        let mut pairs = complete_pairs();
        pairs.push(("trusted_proxies", "127.0.0.1/32, 10.0.0.0/8, 2001:db8::1"));
        let config = load(&pairs).expect("配置应当合法");
        assert_eq!(config.trusted_proxies.len(), 3);
        let private: std::net::IpAddr = "10.1.2.3".parse().expect("IP");
        let v6: std::net::IpAddr = "2001:db8::1".parse().expect("IP");
        assert!(
            config
                .trusted_proxies
                .iter()
                .any(|network| network.contains(&private))
        );
        assert!(
            config
                .trusted_proxies
                .iter()
                .any(|network| network.contains(&v6))
        );

        let mut pairs = complete_pairs();
        pairs.push(("trusted_proxies", "not-an-ip"));
        let err = load(&pairs).expect_err("非法代理网段必须失败");
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: keys::TRUSTED_PROXIES,
                ..
            }
        ));
    }

    #[test]
    fn public_app_url_defaults_and_rejects_invalid_scheme() {
        let config = load(&complete_pairs()).expect("配置应当合法");
        assert_eq!(config.public_app_url, "http://127.0.0.1:8080");

        let mut pairs = complete_pairs();
        pairs.push(("public_app_url", "ftp://example.com"));
        let err = load(&pairs).expect_err("非 http(s) URL 必须失败");
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: keys::PUBLIC_APP_URL,
                ..
            }
        ));
    }

    #[test]
    fn auth_rate_limit_defaults_and_rejects_zero() {
        let config = load(&complete_pairs()).expect("配置应当合法");
        assert_eq!(config.auth_rate_limit_per_minute.get(), 60);

        let mut pairs = complete_pairs();
        pairs.push(("auth_rate_limit_per_minute", "0"));
        let err = load(&pairs).expect_err("限流为 0 必须失败");
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: keys::AUTH_RATE_LIMIT_PER_MINUTE,
                ..
            }
        ));
    }

    #[test]
    fn user_write_rate_limit_defaults_and_rejects_zero() {
        let config = load(&complete_pairs()).expect("配置应当合法");
        assert_eq!(
            config.user_write_rate_limit_per_minute.get(),
            DEFAULT_USER_WRITE_RATE_LIMIT_PER_MINUTE
        );

        let mut pairs = complete_pairs();
        pairs.push(("user_write_rate_limit_per_minute", "2400"));
        let config = load(&pairs).expect("显式配置应当生效");
        assert_eq!(config.user_write_rate_limit_per_minute.get(), 2400);

        let mut pairs = complete_pairs();
        pairs.push(("user_write_rate_limit_per_minute", "0"));
        let err = load(&pairs).expect_err("限流为 0 必须失败");
        assert!(matches!(
            err,
            ConfigError::Invalid {
                key: keys::USER_WRITE_RATE_LIMIT_PER_MINUTE,
                ..
            }
        ));
    }
}
