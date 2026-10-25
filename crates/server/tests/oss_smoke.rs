//! 真实阿里云 OSS 冒烟测试（task 9.3）。
//!
//! 默认 `#[ignore]`：需要用户本人的测试 bucket 与 RAM AccessKey，仓库/CI 不持有凭据。
//! 凭据缺失时打印“跳过”原因并成功返回——这是仓库中唯一允许因缺少外部依赖而跳过的测试。
//!
//! 流程：预签名 PUT → 预签名 GET 并逐字节比对 → 删除 → stat 确认对象已不存在。
//! 失败路径也会尽力删除测试对象，避免在测试 bucket 留下垃圾。
//!
//! 运行方式见 `deploy/oss/README.md` 第 9 节：
//! `cargo test -p server --test oss_smoke -- --ignored --nocapture`

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use opendal::Operator;
use reqwest::Client;
use server::Storage;
use server::config::{Secret, StorageConfig, StorageKind};
use server::storage::ObjectKey;
use uuid::Uuid;

/// 冒烟对象只使用 `v1/` 前缀，与生产 RAM 最小权限策略完全一致。
const PRESIGN_TTL: Duration = Duration::from_secs(5 * 60);

struct SmokeConfig {
    bucket: String,
    region: String,
    internal_endpoint: String,
    public_endpoint: String,
    access_key_id: String,
    secret_access_key: String,
}

impl SmokeConfig {
    /// 读取环境变量；缺失时返回缺失项清单（不打印任何值）。
    fn from_env() -> Result<Self, Vec<&'static str>> {
        fn required(name: &'static str, missing: &mut Vec<&'static str>) -> String {
            match std::env::var(name) {
                Ok(value) if !value.trim().is_empty() => value,
                _ => {
                    missing.push(name);
                    String::new()
                }
            }
        }

        let mut missing = Vec::new();
        let bucket = required("OSS_SMOKE_BUCKET", &mut missing);
        let region = required("OSS_SMOKE_REGION", &mut missing);
        let public_endpoint = required("OSS_SMOKE_PUBLIC_ENDPOINT", &mut missing);
        let access_key_id = required("OSS_SMOKE_ACCESS_KEY_ID", &mut missing);
        let secret_access_key = required("OSS_SMOKE_SECRET_ACCESS_KEY", &mut missing);
        if !missing.is_empty() {
            return Err(missing);
        }
        // GitHub Actions runner 没有 VPC 内网，允许只配置公网 endpoint 兼作数据面 endpoint。
        let internal_endpoint = std::env::var("OSS_SMOKE_INTERNAL_ENDPOINT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| public_endpoint.clone());

        Ok(Self {
            bucket,
            region,
            internal_endpoint,
            public_endpoint,
            access_key_id,
            secret_access_key,
        })
    }

    fn storage_config(&self) -> StorageConfig {
        StorageConfig {
            kind: StorageKind::Oss,
            bucket: self.bucket.clone(),
            region: self.region.clone(),
            internal_endpoint: self.internal_endpoint.clone(),
            public_endpoint: self.public_endpoint.clone(),
            access_key_id: Secret::new(self.access_key_id.clone()),
            secret_access_key: Secret::new(self.secret_access_key.clone()),
            allow_http: self.public_endpoint.starts_with("http://")
                || self.internal_endpoint.starts_with("http://"),
        }
    }
}

fn direct_operator(config: &SmokeConfig) -> Result<Operator, String> {
    let builder = opendal::services::Oss::default()
        .bucket(&config.bucket)
        .endpoint(&config.internal_endpoint)
        .access_key_id(&config.access_key_id)
        .access_key_secret(&config.secret_access_key);
    Operator::new(builder).map_err(|error| format!("构造 OSS operator 失败：{error}"))
}

#[test]
#[ignore = "需要真实阿里云 OSS 测试 bucket 与凭据；按 deploy/oss/README.md 配置后手动运行"]
fn oss_presign_upload_download_delete_roundtrip() {
    let config = match SmokeConfig::from_env() {
        Ok(config) => config,
        Err(missing) => {
            eprintln!(
                "跳过 OSS 冒烟测试：缺少或为空的凭据环境变量：{}",
                missing.join(", ")
            );
            eprintln!(
                "需要配置：OSS_SMOKE_BUCKET、OSS_SMOKE_REGION、OSS_SMOKE_PUBLIC_ENDPOINT、\
                 OSS_SMOKE_ACCESS_KEY_ID、OSS_SMOKE_SECRET_ACCESS_KEY（可选 OSS_SMOKE_INTERNAL_ENDPOINT）"
            );
            return;
        }
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("创建 tokio runtime");
    runtime.block_on(async move {
        let storage = Storage::new(&config.storage_config()).expect("构造 OSS 存储");
        let operator = direct_operator(&config).expect("构造清理用 operator");

        let key = ObjectKey::generate(Uuid::new_v4());
        let payload = format!(
            "backup-oss-smoke-{}-{}",
            Uuid::new_v4(),
            "0123456789abcdef".repeat(64)
        )
        .into_bytes();
        let client = Client::new();

        // 把全部步骤放进 result，确保失败时仍然执行清理。
        let outcome: Result<(), String> = async {
            let put = storage
                .presign_write(&key, PRESIGN_TTL)
                .await
                .map_err(|error| format!("生成预签名 PUT 失败：{error}"))?;
            let response = client
                .put(put.url())
                .body(payload.clone())
                .send()
                .await
                .map_err(|error| format!("PUT 请求失败：{error}"))?;
            if !response.status().is_success() {
                return Err(format!("预签名 PUT 返回 HTTP {}", response.status()));
            }

            let stat = storage
                .stat(key.as_str())
                .await
                .map_err(|error| format!("stat 上传对象失败：{error}"))?
                .ok_or_else(|| "上传成功后 stat 不到对象".to_owned())?;
            if stat.size != payload.len() as u64 {
                return Err(format!(
                    "对象大小不一致：期望 {}，实际 {}",
                    payload.len(),
                    stat.size
                ));
            }

            let get = storage
                .presign_read(key.as_str(), PRESIGN_TTL)
                .await
                .map_err(|error| format!("生成预签名 GET 失败：{error}"))?;
            let response = client
                .get(get.url())
                .send()
                .await
                .map_err(|error| format!("GET 请求失败：{error}"))?;
            if !response.status().is_success() {
                return Err(format!("预签名 GET 返回 HTTP {}", response.status()));
            }
            let downloaded = response
                .bytes()
                .await
                .map_err(|error| format!("读取下载内容失败：{error}"))?;
            if downloaded.as_ref() != payload.as_slice() {
                return Err("下载内容与上传内容不一致".to_owned());
            }
            Ok(())
        }
        .await;

        // 无论上传/下载是否成功，都尽力删除本次生成的对象（删除幂等）。
        let cleanup = operator
            .delete(key.as_str())
            .await
            .map_err(|error| format!("清理测试对象失败：{error}"));

        // 删除后确认对象不存在；OSS 强一致，但留少量重试窗口。
        let mut gone = false;
        for _ in 0..20 {
            match storage.stat(key.as_str()).await {
                Ok(None) => {
                    gone = true;
                    break;
                }
                Ok(Some(_)) => tokio::time::sleep(Duration::from_millis(100)).await,
                Err(error) => {
                    if let Err(message) = &outcome {
                        panic!("{message}");
                    }
                    panic!("删除后 stat 失败：{error}");
                }
            }
        }

        if let Err(error) = cleanup {
            panic!("{error}");
        }
        if let Err(error) = outcome {
            panic!("{error}");
        }
        assert!(gone, "删除后对象仍然存在（key={}）", key.as_str());
        eprintln!("OSS 冒烟测试通过：预签名上传、下载比对、删除与 stat 确认均成功");
    });
}
