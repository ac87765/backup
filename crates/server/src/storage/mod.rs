//! 对象存储访问层（OpenDAL）。
//!
//! design D3：服务端配置两个 operator——
//! - 内网 endpoint：服务端自身 `stat` / `list` / `delete`；
//! - 公网 endpoint：**只**用于生成预签名 URL（4.5 接入）。
//!
//! 本文件当前提供 operator 构造与 `/readyz` 使用的轻量探针（4.4），
//! 预签名、对象键与故障注入在 4.5 加入。
//!
//! # 删除入口
//! 对象删除只能由后台删除队列（`jobs` 模块）调用，**请求处理路径禁止删除对象**。
//! 因此 `delete` 在本模块中保持 `pub(crate)`，并附有明确注释；API 层只拿到
//! 不包含删除能力的 [`Storage`] 公共方法。

use opendal::Operator;

use crate::config::{StorageConfig, StorageKind};

/// 健康检查探测的固定对象键。
///
/// 不属于任何用户数据，也永远不会被写入业务对象；只用于确认 bucket 可访问。
/// `stat` 返回 NotFound 同样说明存储可达。
pub const PROBE_OBJECT_KEY: &str = "v1/.health-probe";

/// 对象存储错误。
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("对象存储配置无效")]
    Config(#[source] opendal::Error),
    #[error("对象存储操作失败")]
    Operation(#[source] opendal::Error),
}

/// 对象存储访问句柄。
#[derive(Clone)]
pub struct Storage {
    internal: Operator,
    bucket: String,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 不输出凭据或 endpoint 细节。
        f.debug_struct("Storage")
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

impl Storage {
    /// 根据配置构造内网 operator。
    pub fn new(config: &StorageConfig) -> Result<Self, StorageError> {
        let internal = build_operator(config, &config.internal_endpoint)?;
        Ok(Self {
            internal,
            bucket: config.bucket.clone(),
        })
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    /// `/readyz` 的对象存储检查：`stat` 一个固定探针键。
    ///
    /// 对象不存在（NotFound）也视为存储可达；其他错误视为不可用。
    pub async fn probe(&self) -> Result<(), StorageError> {
        match self.internal.stat(PROBE_OBJECT_KEY).await {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StorageError::Operation(error)),
        }
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
