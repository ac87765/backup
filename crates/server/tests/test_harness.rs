//! 验证集成测试框架本身：容器可用、每测试数据库/键前缀隔离、MinIO 镜像与 compose 一致。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{MINIO_IMAGE_DIGEST, MINIO_IMAGE_NAME, TestBucket, TestDb, environment};

#[tokio::test]
async fn postgres_and_minio_fixtures_are_usable() {
    let env = environment().await;
    let db = TestDb::create(env).await;
    let pool = db.connect().await;
    let one: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .expect("PostgreSQL 应当可以执行查询");
    assert_eq!(one, 1);

    let health = reqwest::get(format!("{}/minio/health/ready", env.storage_endpoint))
        .await
        .expect("MinIO 健康端点应当可访问");
    assert!(
        health.status().is_success(),
        "MinIO 健康检查返回 {}",
        health.status()
    );

    let bucket = TestBucket::create(env);
    assert!(bucket.key("probe.bin").starts_with("tests/"));
    assert_eq!(bucket.bucket, env.storage_bucket);
}

#[tokio::test]
async fn parallel_tests_get_isolated_databases() {
    let env = environment().await;
    let first = TestDb::create(env).await;
    let second = TestDb::create(env).await;
    assert_ne!(first.name, second.name);

    let (first_pool, second_pool) = tokio::join!(first.connect(), second.connect());
    for (pool, marker) in [(&first_pool, "first"), (&second_pool, "second")] {
        sqlx::query("CREATE TABLE isolation_probe (marker text NOT NULL)")
            .execute(pool)
            .await
            .expect("应当能在独立数据库中建表");
        sqlx::query("INSERT INTO isolation_probe (marker) VALUES ($1)")
            .bind(marker)
            .execute(pool)
            .await
            .expect("应当能写入独立数据库");
    }

    let first_markers: Vec<String> =
        sqlx::query_scalar("SELECT marker FROM isolation_probe ORDER BY marker")
            .fetch_all(&first_pool)
            .await
            .expect("查询第一个数据库");
    let second_markers: Vec<String> =
        sqlx::query_scalar("SELECT marker FROM isolation_probe ORDER BY marker")
            .fetch_all(&second_pool)
            .await
            .expect("查询第二个数据库");
    assert_eq!(first_markers, vec!["first".to_owned()]);
    assert_eq!(second_markers, vec!["second".to_owned()]);
}

#[test]
fn minio_image_digest_matches_dev_compose() {
    let compose = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/docker-compose.dev.yml"
    ))
    .expect("应当能读取 deploy/docker-compose.dev.yml");
    let reference = format!("{MINIO_IMAGE_NAME}:{MINIO_IMAGE_DIGEST}");
    assert!(
        compose.contains(&reference),
        "测试框架使用的 MinIO 镜像（{reference}）必须与 deploy/docker-compose.dev.yml 完全一致"
    );
}
