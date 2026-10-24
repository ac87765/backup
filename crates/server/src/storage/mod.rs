//! 对象存储访问层（OpenDAL）。
//!
//! design D3：服务端配置两个 operator——
//! - 内网 endpoint：服务端自身 `stat` / `list` / `delete`；
//! - 公网 endpoint：**只**用于生成预签名 URL。
//!
//! # 删除能力与请求处理隔离
//!
//! 请求处理路径禁止直接删除对象（AGENTS.md：删除只能经过 outbox 队列）。
//! 因此 [`Storage`] 的公共 API **不提供删除方法**；唯一入口是
//! `pub(crate) Storage::deletion_handle()` 返回的 `DeletionHandle`，它只能被
//! 第 5 组的 `jobs` 删除队列调用。生产实现 `OpenDalBackend` 不导出；
//! 公开的 [`ObjectBackend`] trait 仅用于故障注入测试。
//!
//! # 故障注入
//!
//! [`FaultInjector`] 可以包装任意 [`ObjectBackend`]，按配置让“前 N 次”指定操作
//! 返回指定错误类型，供第 5、8 组测试重试/退避逻辑使用。

use std::fmt;
use std::sync::Arc;
use std::sync::Once;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use opendal::Operator;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::config::{StorageConfig, StorageKind};

/// 预签名 URL 的最长有效期（15 分钟）。
pub const MAX_PRESIGN_TTL: Duration = Duration::from_secs(15 * 60);

/// 健康检查探测使用的前缀。
///
/// 用 `list`（limit 1）而不是 `stat` 固定键：对象不存在与 bucket 不存在在 HEAD
/// 请求上都表现为 404，无法区分；list 在 bucket 不存在或凭据错误时会返回错误。
/// 前缀 `v1/` 是所有业务对象的根前缀，空 bucket 也能正常返回空列表。
pub const PROBE_PREFIX: &str = "v1/";

/// 对象存储错误。
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("对象存储配置无效")]
    Config(#[source] opendal::Error),
    #[error("对象存储操作失败")]
    Operation(#[source] opendal::Error),
    #[error("预签名有效期非法：必须在 1 秒到 15 分钟之间")]
    InvalidPresignTtl,
    #[error("对象存储故障注入：{0}")]
    Injected(InjectedErrorKind),
}

/// 服务端生成的对象键。
///
/// 格式固定为 `v1/{user_id}/{uuid}`，只能通过 [`ObjectKey::generate`] 生成。
/// 不提供从任意字符串构造的公共入口，避免把客户端提供的值当作对象键。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectKey(String);

impl ObjectKey {
    /// 为指定用户生成新的对象键（UUIDv4 由服务端 CSPRNG 生成）。
    pub fn generate(user_id: Uuid) -> Self {
        Self(format!("v1/{user_id}/{}", Uuid::new_v4()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 对象元数据（`stat` 结果）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMetadata {
    pub size: u64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
}

/// `list` 返回的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectEntry {
    pub key: String,
    pub size: u64,
    pub is_dir: bool,
}

/// 预签名请求。
///
/// `Debug` 故意脱敏：完整 URL 含签名，绝不能进日志。
#[derive(Clone, PartialEq, Eq)]
pub struct PresignedUrl {
    method: String,
    url: String,
}

impl PresignedUrl {
    pub fn method(&self) -> &str {
        &self.method
    }

    /// 仅在把 URL 返回给客户端时调用；禁止写入日志。
    pub fn url(&self) -> &str {
        &self.url
    }
}

impl fmt::Debug for PresignedUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PresignedUrl")
            .field("method", &self.method)
            .field("url", &"[REDACTED]")
            .finish()
    }
}

/// 可被故障注入的操作类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectOperation {
    Stat,
    List,
    Delete,
    PresignWrite,
    PresignRead,
}

/// 故障注入产生的错误类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectedErrorKind {
    Unavailable,
    PermissionDenied,
    Timeout,
}

impl fmt::Display for InjectedErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Unavailable => "unavailable",
            Self::PermissionDenied => "permission_denied",
            Self::Timeout => "timeout",
        };
        f.write_str(name)
    }
}

/// 故障注入配置：前 `fail_first` 次匹配操作失败，之后恢复正常。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultInjectionConfig {
    pub fail_first: usize,
    pub kind: InjectedErrorKind,
    /// `None` 表示所有操作都注入；`Some(op)` 只注入指定操作。
    pub operation: Option<ObjectOperation>,
}

impl FaultInjectionConfig {
    pub fn new(fail_first: usize, kind: InjectedErrorKind) -> Self {
        Self {
            fail_first,
            kind,
            operation: None,
        }
    }

    pub fn only(mut self, operation: ObjectOperation) -> Self {
        self.operation = Some(operation);
        self
    }
}

/// 存储后端抽象（生产实现为 OpenDAL；测试可包装故障注入）。
#[async_trait]
pub trait ObjectBackend: Send + Sync {
    async fn stat(&self, object_key: &str) -> Result<Option<ObjectMetadata>, StorageError>;
    async fn list(&self, prefix: &str, limit: usize) -> Result<Vec<ObjectEntry>, StorageError>;
    /// 仅供后台删除队列使用；请求处理路径不得调用。
    async fn delete(&self, object_key: &str) -> Result<(), StorageError>;
    async fn presign_write(
        &self,
        object_key: &str,
        ttl: Duration,
    ) -> Result<PresignedUrl, StorageError>;
    async fn presign_read(
        &self,
        object_key: &str,
        ttl: Duration,
    ) -> Result<PresignedUrl, StorageError>;
}

/// 对象存储访问句柄。
#[derive(Clone)]
pub struct Storage {
    backend: Arc<dyn ObjectBackend>,
    bucket: String,
}

impl fmt::Debug for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Storage")
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

impl Storage {
    /// 根据配置构造内网 + 公网两个 operator。
    pub fn new(config: &StorageConfig) -> Result<Self, StorageError> {
        ensure_http_stack();
        let backend = OpenDalBackend::new(config)?;
        Ok(Self {
            backend: Arc::new(backend),
            bucket: config.bucket.clone(),
        })
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// 返回包装了故障注入的新句柄（共享同一后端）。
    pub fn with_fault_injection(&self, config: FaultInjectionConfig) -> Self {
        Self {
            backend: Arc::new(FaultInjector::new(self.backend.clone(), config)),
            bucket: self.bucket.clone(),
        }
    }

    /// `/readyz` 的对象存储检查：对 `v1/` 前缀做 limit 1 的 list。
    ///
    /// bucket 不存在、凭据错误、网络不可达都会返回错误，因此不会把“bucket 不存在”
    /// 误判为可达；空 bucket 返回空列表则视为正常。
    pub async fn probe(&self) -> Result<(), StorageError> {
        self.backend.list(PROBE_PREFIX, 1).await.map(|_| ())
    }

    pub async fn stat(&self, object_key: &str) -> Result<Option<ObjectMetadata>, StorageError> {
        self.backend.stat(object_key).await
    }

    pub async fn list_prefix(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<ObjectEntry>, StorageError> {
        self.backend.list(prefix, limit).await
    }

    /// 生成预签名 PUT URL；有效期必须在 `(0, 15 分钟]` 内。
    pub async fn presign_write(
        &self,
        object_key: &ObjectKey,
        ttl: Duration,
    ) -> Result<PresignedUrl, StorageError> {
        validate_presign_ttl(ttl)?;
        self.backend.presign_write(object_key.as_str(), ttl).await
    }

    /// 生成预签名 GET URL；有效期必须在 `(0, 15 分钟]` 内。
    pub async fn presign_read(
        &self,
        object_key: &str,
        ttl: Duration,
    ) -> Result<PresignedUrl, StorageError> {
        validate_presign_ttl(ttl)?;
        self.backend.presign_read(object_key, ttl).await
    }

    /// 删除能力入口：只有后台删除队列（第 5 组 `jobs`）可以调用。
    // 第 5 组的 jobs 模块是唯一调用方；在此之前没有生产调用点。
    #[allow(dead_code)]
    pub(crate) fn deletion_handle(&self) -> DeletionHandle {
        DeletionHandle {
            backend: self.backend.clone(),
        }
    }
}

fn validate_presign_ttl(ttl: Duration) -> Result<(), StorageError> {
    if ttl.is_zero() || ttl > MAX_PRESIGN_TTL {
        return Err(StorageError::InvalidPresignTtl);
    }
    Ok(())
}

/// 安装进程级 HTTP/TLS 运行时。
///
/// OpenDAL 使用 `rustls-no-provider` 的 reqwest transport（避免 aws-lc-rs），
/// 因此需要：
/// 1. 把 rustls 的默认 CryptoProvider 统一为 ring；
/// 2. 显式安装 OpenDAL 的默认 reqwest transport。
///
/// 两者都是进程级、幂等的；在其他库已经安装 provider 时保留现有设置。
fn ensure_http_stack() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
        opendal_http_transport_reqwest::install_default();
    });
}

/// 删除能力句柄，仅在 crate 内部可见（`jobs` 模块使用）。
#[allow(dead_code)]
pub(crate) struct DeletionHandle {
    backend: Arc<dyn ObjectBackend>,
}

impl DeletionHandle {
    /// 删除对象。对象不存在视为成功（幂等），与后续删除队列语义一致。
    #[allow(dead_code)]
    pub(crate) async fn delete(&self, object_key: &str) -> Result<(), StorageError> {
        self.backend.delete(object_key).await
    }
}

/// 故障注入装饰器。
pub struct FaultInjector {
    inner: Arc<dyn ObjectBackend>,
    remaining_failures: AtomicUsize,
    config: FaultInjectionConfig,
}

impl FaultInjector {
    pub fn new(inner: Arc<dyn ObjectBackend>, config: FaultInjectionConfig) -> Self {
        Self {
            inner,
            remaining_failures: AtomicUsize::new(config.fail_first),
            config,
        }
    }

    fn maybe_fail(&self, operation: ObjectOperation) -> Result<(), StorageError> {
        if self
            .config
            .operation
            .is_some_and(|configured| configured != operation)
        {
            return Ok(());
        }
        let injected = self
            .remaining_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok();
        if injected {
            Err(StorageError::Injected(self.config.kind))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl ObjectBackend for FaultInjector {
    async fn stat(&self, object_key: &str) -> Result<Option<ObjectMetadata>, StorageError> {
        self.maybe_fail(ObjectOperation::Stat)?;
        self.inner.stat(object_key).await
    }

    async fn list(&self, prefix: &str, limit: usize) -> Result<Vec<ObjectEntry>, StorageError> {
        self.maybe_fail(ObjectOperation::List)?;
        self.inner.list(prefix, limit).await
    }

    async fn delete(&self, object_key: &str) -> Result<(), StorageError> {
        self.maybe_fail(ObjectOperation::Delete)?;
        self.inner.delete(object_key).await
    }

    async fn presign_write(
        &self,
        object_key: &str,
        ttl: Duration,
    ) -> Result<PresignedUrl, StorageError> {
        self.maybe_fail(ObjectOperation::PresignWrite)?;
        self.inner.presign_write(object_key, ttl).await
    }

    async fn presign_read(
        &self,
        object_key: &str,
        ttl: Duration,
    ) -> Result<PresignedUrl, StorageError> {
        self.maybe_fail(ObjectOperation::PresignRead)?;
        self.inner.presign_read(object_key, ttl).await
    }
}

/// OpenDAL 生产实现：内网 operator 做数据面操作，公网 operator 只做预签名。
struct OpenDalBackend {
    internal: Operator,
    public: Operator,
}

impl OpenDalBackend {
    fn new(config: &StorageConfig) -> Result<Self, StorageError> {
        Ok(Self {
            internal: build_operator(config, &config.internal_endpoint)?,
            public: build_operator(config, &config.public_endpoint)?,
        })
    }
}

#[async_trait]
impl ObjectBackend for OpenDalBackend {
    async fn stat(&self, object_key: &str) -> Result<Option<ObjectMetadata>, StorageError> {
        match self.internal.stat(object_key).await {
            Ok(metadata) => Ok(Some(ObjectMetadata {
                size: metadata.content_length(),
                content_type: metadata.content_type().map(str::to_owned),
                etag: metadata.etag().map(str::to_owned),
            })),
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StorageError::Operation(error)),
        }
    }

    async fn list(&self, prefix: &str, limit: usize) -> Result<Vec<ObjectEntry>, StorageError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        // 不能用 `list_with(..).limit(n)`：那只是每页大小的提示，底层仍会翻完所有分页
        // 并把结果全部物化到内存。`lister_with` 是流式的，读取到 n 条后立即停止。
        let mut lister = self
            .internal
            .lister_with(prefix)
            .limit(limit)
            .await
            .map_err(StorageError::Operation)?;
        let mut entries = Vec::with_capacity(limit.min(128));
        while let Some(entry) = lister.next().await {
            let entry = entry.map_err(StorageError::Operation)?;
            let metadata = entry.metadata();
            entries.push(ObjectEntry {
                key: entry.path().to_owned(),
                size: metadata.content_length(),
                is_dir: metadata.is_dir(),
            });
            if entries.len() >= limit {
                break;
            }
        }
        Ok(entries)
    }

    async fn delete(&self, object_key: &str) -> Result<(), StorageError> {
        // OpenDAL 对不存在的对象执行 delete 视为成功，满足删除队列的幂等要求。
        self.internal
            .delete(object_key)
            .await
            .map_err(StorageError::Operation)
    }

    async fn presign_write(
        &self,
        object_key: &str,
        ttl: Duration,
    ) -> Result<PresignedUrl, StorageError> {
        let request = self
            .public
            .presign_write(object_key, ttl)
            .await
            .map_err(StorageError::Operation)?;
        Ok(PresignedUrl {
            method: request.method().as_str().to_owned(),
            url: request.uri().to_string(),
        })
    }

    async fn presign_read(
        &self,
        object_key: &str,
        ttl: Duration,
    ) -> Result<PresignedUrl, StorageError> {
        let request = self
            .public
            .presign_read(object_key, ttl)
            .await
            .map_err(StorageError::Operation)?;
        Ok(PresignedUrl {
            method: request.method().as_str().to_owned(),
            url: request.uri().to_string(),
        })
    }
}

fn build_operator(config: &StorageConfig, endpoint: &str) -> Result<Operator, StorageError> {
    match config.kind {
        StorageKind::S3 => {
            let builder = opendal::services::S3::default()
                .bucket(&config.bucket)
                .region(&config.region)
                .endpoint(endpoint)
                .access_key_id(config.access_key_id.expose())
                .secret_access_key(config.secret_access_key.expose())
                // 不读取 ~/.aws/config 或 EC2 元数据：配置必须全部来自环境变量。
                .disable_config_load()
                .disable_ec2_metadata();
            Ok(Operator::new(builder).map_err(StorageError::Config)?)
        }
        StorageKind::Oss => {
            let builder = opendal::services::Oss::default()
                .bucket(&config.bucket)
                .endpoint(endpoint)
                .access_key_id(config.access_key_id.expose())
                .access_key_secret(config.secret_access_key.expose());
            Ok(Operator::new(builder).map_err(StorageError::Config)?)
        }
    }
}

#[cfg(test)]
mod tests {
    // 测试代码允许使用 expect/unwrap：断言失败时 panic 是预期行为。
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    struct CountingBackend {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ObjectBackend for CountingBackend {
        async fn stat(&self, _object_key: &str) -> Result<Option<ObjectMetadata>, StorageError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }

        async fn list(
            &self,
            _prefix: &str,
            _limit: usize,
        ) -> Result<Vec<ObjectEntry>, StorageError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }

        async fn delete(&self, _object_key: &str) -> Result<(), StorageError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn presign_write(
            &self,
            _object_key: &str,
            _ttl: Duration,
        ) -> Result<PresignedUrl, StorageError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(PresignedUrl {
                method: "PUT".to_owned(),
                url: "https://example.invalid/object?X-Amz-Signature=secret".to_owned(),
            })
        }

        async fn presign_read(
            &self,
            _object_key: &str,
            _ttl: Duration,
        ) -> Result<PresignedUrl, StorageError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(PresignedUrl {
                method: "GET".to_owned(),
                url: "https://example.invalid/object?X-Amz-Signature=secret".to_owned(),
            })
        }
    }

    #[test]
    fn object_keys_are_server_generated_and_scoped_to_user() {
        let user_id = Uuid::new_v4();
        let first = ObjectKey::generate(user_id);
        let second = ObjectKey::generate(user_id);
        assert!(first.as_str().starts_with(&format!("v1/{user_id}/")));
        assert_ne!(first, second);
        assert_eq!(first.as_str().split('/').count(), 3);
        let object_id = first.as_str().rsplit('/').next().expect("对象 UUID");
        let parsed = Uuid::parse_str(object_id).expect("对象键必须包含 UUID");
        assert_eq!(parsed.get_version_num(), 4);
    }

    #[test]
    fn debug_output_redacts_presigned_urls() {
        let url = PresignedUrl {
            method: "PUT".to_owned(),
            url: "https://bucket.example/object?X-Amz-Signature=super-secret".to_owned(),
        };
        let debug = format!("{url:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("super-secret"));
    }

    #[tokio::test]
    async fn fault_injection_fails_first_n_calls_only() {
        let backend = Arc::new(CountingBackend {
            calls: AtomicUsize::new(0),
        });
        let injected = FaultInjector::new(
            backend.clone(),
            FaultInjectionConfig::new(2, InjectedErrorKind::Unavailable),
        );

        assert!(matches!(
            injected.stat("key").await,
            Err(StorageError::Injected(InjectedErrorKind::Unavailable))
        ));
        assert!(matches!(
            injected.stat("key").await,
            Err(StorageError::Injected(InjectedErrorKind::Unavailable))
        ));
        assert!(injected.stat("key").await.expect("第三次应成功").is_none());
        assert_eq!(
            backend.calls.load(Ordering::SeqCst),
            1,
            "只有成功那次穿透到底层"
        );
    }

    #[tokio::test]
    async fn fault_injection_can_target_a_specific_operation() {
        let backend = Arc::new(CountingBackend {
            calls: AtomicUsize::new(0),
        });
        let injected = FaultInjector::new(
            backend,
            FaultInjectionConfig::new(1, InjectedErrorKind::Timeout)
                .only(ObjectOperation::PresignWrite),
        );

        // 非目标操作不受影响。
        assert!(
            injected
                .list("v1/", 1)
                .await
                .expect("list 不受影响")
                .is_empty()
        );
        // 目标操作第一次失败，第二次成功。
        let key = ObjectKey::generate(Uuid::new_v4());
        assert!(matches!(
            injected.presign_write(key.as_str(), MAX_PRESIGN_TTL).await,
            Err(StorageError::Injected(InjectedErrorKind::Timeout))
        ));
        assert!(
            injected
                .presign_write(key.as_str(), MAX_PRESIGN_TTL)
                .await
                .is_ok()
        );
    }
}
