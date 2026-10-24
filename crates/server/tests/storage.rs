//! 存储层集成测试（tasks 4.5）：预签名 PUT/GET 往返、TTL 上限、公网 endpoint
//! 只用于预签名、故障注入。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::time::Duration;

use server::storage::{
    FaultInjectionConfig, InjectedErrorKind, MAX_PRESIGN_TTL, ObjectKey, ObjectOperation, Storage,
    StorageError,
};
use uuid::Uuid;

use common::{environment, integration_storage_config};

async fn async_storage() -> Storage {
    let env = environment().await;
    let config = integration_storage_config(env);
    Storage::new(&config).expect("构造 storage")
}

#[tokio::test]
async fn presigned_put_and_get_roundtrip() {
    let storage = async_storage().await;
    let bucket = Uuid::new_v4(); // 每个测试用独立用户前缀
    let key = ObjectKey::generate(bucket);
    assert!(key.as_str().starts_with(&format!("v1/{bucket}/")));

    let payload = b"ciphertext-frame-bytes".to_vec();
    let put = storage
        .presign_write(&key, Duration::from_secs(300))
        .await
        .expect("生成预签名 PUT");
    assert_eq!(put.method(), "PUT");

    let client = reqwest::Client::new();
    let response = client
        .put(put.url())
        .body(payload.clone())
        .send()
        .await
        .expect("预签名 PUT 应当成功");
    assert!(
        response.status().is_success(),
        "预签名 PUT 失败: {}",
        response.status()
    );

    let metadata = storage
        .stat(key.as_str())
        .await
        .expect("stat 应当成功")
        .expect("对象应当存在");
    assert_eq!(metadata.size, payload.len() as u64);

    let entries = storage
        .list_prefix(&format!("v1/{bucket}/"), 10)
        .await
        .expect("list 应当成功");
    assert!(
        entries.iter().any(|entry| entry.key == key.as_str()),
        "list 应包含刚上传的对象，实际: {entries:?}"
    );

    let get = storage
        .presign_read(key.as_str(), Duration::from_secs(300))
        .await
        .expect("生成预签名 GET");
    assert_eq!(get.method(), "GET");
    let response = client
        .get(get.url())
        .send()
        .await
        .expect("预签名 GET 应当成功");
    assert!(response.status().is_success());
    let downloaded = response.bytes().await.expect("读取下载内容");
    assert_eq!(downloaded.as_ref(), payload.as_slice());
}

#[tokio::test]
async fn list_prefix_stops_after_limit() {
    let storage = async_storage().await;
    let user = Uuid::new_v4();
    let prefix = format!("v1/{user}/");
    let client = reqwest::Client::new();

    // 同一前缀下放 3 个对象，验证 limit 是真正的截断，而不是“每页大小提示”。
    for index in 0..3 {
        let key = ObjectKey::generate(user);
        let url = storage
            .presign_write(&key, Duration::from_secs(60))
            .await
            .expect("生成预签名 PUT");
        let response = client
            .put(url.url())
            .body(format!("payload-{index}"))
            .send()
            .await
            .expect("上传对象");
        assert!(response.status().is_success());
    }

    let limited = storage.list_prefix(&prefix, 1).await.expect("list limit 1");
    assert_eq!(
        limited.len(),
        1,
        "limit 1 必须立即停止，实际返回 {} 条: {limited:?}",
        limited.len()
    );

    let all = storage
        .list_prefix(&prefix, 10)
        .await
        .expect("list limit 10");
    assert_eq!(all.len(), 3, "应能看到同前缀下的全部 3 个对象");

    storage.probe().await.expect("probe 行为保持不变");
}

#[tokio::test]
async fn missing_object_stats_as_none() {
    let storage = async_storage().await;
    let key = ObjectKey::generate(Uuid::new_v4());
    assert!(
        storage
            .stat(key.as_str())
            .await
            .expect("stat 应当成功")
            .is_none(),
        "不存在的对象必须返回 None"
    );
}

#[tokio::test]
async fn presign_ttl_is_limited_to_fifteen_minutes() {
    let storage = async_storage().await;
    let key = ObjectKey::generate(Uuid::new_v4());

    for ttl in [
        Duration::ZERO,
        MAX_PRESIGN_TTL + Duration::from_secs(1),
        Duration::from_secs(3600),
    ] {
        assert!(
            matches!(
                storage.presign_write(&key, ttl).await,
                Err(StorageError::InvalidPresignTtl)
            ),
            "超过上限或为零的 TTL 必须被拒绝: {ttl:?}"
        );
    }

    let url = storage
        .presign_write(&key, MAX_PRESIGN_TTL)
        .await
        .expect("15 分钟应当允许");
    assert!(
        url.url().contains("X-Amz-Expires=900"),
        "预签名 URL 的有效期应为 900 秒，实际: {}",
        url.url()
    );
}

#[tokio::test]
async fn presign_uses_public_endpoint_only_for_urls() {
    let env = environment().await;
    let mut config = integration_storage_config(env);
    // 内网 endpoint 仍是可访问的 MinIO；公网 endpoint 故意不可访问。
    config.public_endpoint = "https://public.example.invalid".to_owned();
    let storage = Storage::new(&config).expect("构造 storage");
    let key = ObjectKey::generate(Uuid::new_v4());

    let put = storage
        .presign_write(&key, Duration::from_secs(60))
        .await
        .expect("生成预签名 URL 不应访问网络");
    assert!(
        put.url().starts_with("https://public.example.invalid/"),
        "预签名 URL 必须使用公网 endpoint，实际: {}",
        put.url()
    );

    // 数据面操作（stat/list）使用内网 endpoint，因此仍能正常工作。
    assert!(
        storage
            .stat(key.as_str())
            .await
            .expect("stat 走内网 endpoint")
            .is_none()
    );
}

#[tokio::test]
async fn fault_injection_can_fail_presign_calls() {
    let storage = async_storage().await;
    let faulty = storage.with_fault_injection(
        FaultInjectionConfig::new(2, InjectedErrorKind::Unavailable)
            .only(ObjectOperation::PresignWrite),
    );
    let key = ObjectKey::generate(Uuid::new_v4());

    for _ in 0..2 {
        assert!(matches!(
            faulty.presign_write(&key, Duration::from_secs(60)).await,
            Err(StorageError::Injected(InjectedErrorKind::Unavailable))
        ));
    }

    let url = faulty
        .presign_write(&key, Duration::from_secs(60))
        .await
        .expect("第三次应当恢复");
    let response = reqwest::Client::new()
        .put(url.url())
        .body(b"fault-injection-roundtrip".to_vec())
        .send()
        .await
        .expect("恢复后预签名 PUT 应当成功");
    assert!(response.status().is_success());
}
