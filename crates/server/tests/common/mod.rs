//! 集成测试共享框架：真实 PostgreSQL 16 与 MinIO（testcontainers）。
//!
//! 设计要点（tasks 4.8）：
//! - 容器按测试进程共享，但**每个测试拿到独立的数据库与对象键前缀**，因此测试可以并行运行。
//! - 没有 Docker 时测试必须失败，并给出明确提示；这里刻意不提供任何“跳过”逻辑。
//! - MinIO 镜像与 `deploy/docker-compose.dev.yml` 使用同一个 digest，测试
//!   `tests/test_harness.rs` 会断言两处一致。
//!
//! 容器会泄漏到进程结束（`Box::leak`），因为 testcontainers 的容器在 Drop 时会停止；
//! 进程退出后由 Docker 侧的容器清理机制兜底。

#![allow(dead_code)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use testcontainers::core::{CmdWaitFor, ExecCommand, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::sync::OnceCell;
use uuid::Uuid;

/// 与 `deploy/docker-compose.dev.yml` 的 `postgres` 服务保持一致。
pub const POSTGRES_IMAGE: &str = "postgres";
pub const POSTGRES_TAG: &str = "16.15-alpine";

/// 与 `deploy/docker-compose.dev.yml` 中 MinIO 服务的镜像 **digest** 一致。
///
/// 来源：`deploy/docker-compose.dev.yml` 的
/// `image: ${MINIO_IMAGE:-bitnamilegacy/minio@sha256:451fe...}`。
/// testcontainers 0.28 的 `GenericImage` 用 `{name}:{tag}` 拼装镜像引用，
/// 因此把 `@sha256` 放进 name、把摘要放进 tag，最终得到标准的 digest 引用。
pub const MINIO_IMAGE_NAME: &str = "bitnamilegacy/minio@sha256";
pub const MINIO_IMAGE_DIGEST: &str =
    "451fe6858cb770cc9d0e77ba811ce287420f781c7c1b806a386f6896471a349c";

pub const MINIO_ACCESS_KEY: &str = "backup-test-access";
pub const MINIO_SECRET_KEY: &str = "backup-test-secret";
pub const MINIO_BUCKET: &str = "backup-test";

const POSTGRES_USER: &str = "backup";
const POSTGRES_PASSWORD: &str = "backup-test-password";
const POSTGRES_DB: &str = "backup_test";

/// 共享的容器与连接信息。
pub struct TestEnvironment {
    /// 指向容器内默认库 `backup_test` 的连接串（用于创建每测试独立数据库）。
    pub database_url: String,
    /// 对象存储 endpoint（公网/内网在本地测试中是同一个地址）。
    pub storage_endpoint: String,
    pub storage_bucket: &'static str,
    pub storage_access_key: &'static str,
    pub storage_secret_key: &'static str,
    minio: &'static ContainerAsync<GenericImage>,
    _postgres: &'static ContainerAsync<GenericImage>,
}

static ENVIRONMENT: OnceCell<TestEnvironment> = OnceCell::const_new();

/// 获取（首次调用时启动）共享测试环境。
pub async fn environment() -> &'static TestEnvironment {
    ENVIRONMENT.get_or_init(start_environment).await
}

fn docker_unavailable(context: &str, error: &dyn std::fmt::Display) -> ! {
    panic!(
        "集成测试需要可用的 Docker（PostgreSQL/MinIO）。\n\
         {context} 失败：{error}\n\
         本测试不会在缺少 Docker 时跳过（tasks 4.8 明确禁止静默通过）。\
         请启动 Docker（本机 Windows 请在 WSL 中运行测试）后重试。"
    )
}

async fn start_environment() -> TestEnvironment {
    let postgres = GenericImage::new(POSTGRES_IMAGE, POSTGRES_TAG)
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_env_var("POSTGRES_DB", POSTGRES_DB)
        .with_env_var("POSTGRES_USER", POSTGRES_USER)
        .with_env_var("POSTGRES_PASSWORD", POSTGRES_PASSWORD)
        .start()
        .await
        .unwrap_or_else(|error| docker_unavailable("启动 PostgreSQL 容器", &error));
    let postgres = Box::leak(Box::new(postgres));
    let postgres_port = postgres
        .get_host_port_ipv4(5432.tcp())
        .await
        .unwrap_or_else(|error| docker_unavailable("解析 PostgreSQL 端口", &error));
    let database_url = format!(
        "postgres://{POSTGRES_USER}:{POSTGRES_PASSWORD}@127.0.0.1:{postgres_port}/{POSTGRES_DB}"
    );

    let minio = GenericImage::new(MINIO_IMAGE_NAME, MINIO_IMAGE_DIGEST)
        .with_exposed_port(9000.tcp())
        // Bitnami 镜像在 setup 阶段先后台启动 MinIO、建默认 bucket，再停掉并前台启动；
        // 等到 setup 完成，避免测试命中“后台实例即将停止”的窗口。
        .with_wait_for(WaitFor::message_on_either_std("MinIO setup finished"))
        .with_env_var("MINIO_ROOT_USER", MINIO_ACCESS_KEY)
        .with_env_var("MINIO_ROOT_PASSWORD", MINIO_SECRET_KEY)
        .with_env_var("MINIO_DEFAULT_BUCKETS", MINIO_BUCKET)
        .with_env_var("MINIO_BROWSER", "off")
        .start()
        .await
        .unwrap_or_else(|error| docker_unavailable("启动 MinIO 容器", &error));
    let minio = Box::leak(Box::new(minio));
    let minio_port = minio
        .get_host_port_ipv4(9000.tcp())
        .await
        .unwrap_or_else(|error| docker_unavailable("解析 MinIO 端口", &error));

    wait_for_bucket(minio, MINIO_BUCKET).await;

    TestEnvironment {
        database_url,
        storage_endpoint: format!("http://127.0.0.1:{minio_port}"),
        storage_bucket: MINIO_BUCKET,
        storage_access_key: MINIO_ACCESS_KEY,
        storage_secret_key: MINIO_SECRET_KEY,
        minio,
        _postgres: postgres,
    }
}

/// 通过容器内的 `mc` 轮询确认 bucket 已创建（Bitnami 的默认 bucket 创建在后台完成）。
async fn wait_for_bucket(minio: &ContainerAsync<GenericImage>, bucket: &str) {
    let host = format!("http://{MINIO_ACCESS_KEY}:{MINIO_SECRET_KEY}@127.0.0.1:9000");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let command = ExecCommand::new([
            "/opt/bitnami/minio-client/bin/mc",
            "--config-dir",
            "/tmp/mc",
            "stat",
            &format!("local/{bucket}"),
        ])
        .with_env_vars([
            ("HOME".to_owned(), "/tmp".to_owned()),
            ("MC_HOST_local".to_owned(), host.clone()),
        ])
        .with_cmd_ready_condition(CmdWaitFor::exit());
        let ready = match minio.exec(command).await {
            Ok(result) => result.exit_code().await.ok().flatten() == Some(0),
            Err(_) => false,
        };
        if ready {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            docker_unavailable(
                "等待 MinIO bucket 创建",
                &format!("超过 60 秒仍无法访问 bucket {bucket}"),
            );
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// 每测试独立的数据库。
///
/// 每个测试创建一个全新 database，因此迁移、`_sqlx_migrations` 与业务表完全隔离，
/// 多个测试可以并行执行且互不干扰。
pub struct TestDb {
    pub url: String,
    pub name: String,
}

impl TestDb {
    pub async fn create(env: &TestEnvironment) -> Self {
        let name = format!("test_{}", Uuid::new_v4().simple());
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&env.database_url)
            .await
            .unwrap_or_else(|error| docker_unavailable("连接 PostgreSQL 管理库", &error));
        // 库名由本函数用 UUID 生成（只含 ASCII 十六进制与下划线），不存在注入面。
        sqlx::query(&format!("CREATE DATABASE \"{name}\""))
            .execute(&admin)
            .await
            .unwrap_or_else(|error| panic!("创建测试数据库 {name} 失败：{error}"));
        admin.close().await;

        let base = env
            .database_url
            .rsplit_once('/')
            .map(|(prefix, _)| prefix)
            .expect("测试连接串必须包含数据库名");
        Self {
            url: format!("{base}/{name}"),
            name,
        }
    }

    pub async fn connect(&self) -> PgPool {
        PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&self.url)
            .await
            .unwrap_or_else(|error| docker_unavailable("连接测试数据库", &error))
    }
}

/// 每测试独立的对象键前缀（全部测试共享同一个 bucket）。
pub struct TestBucket {
    pub bucket: &'static str,
    pub prefix: String,
}

impl TestBucket {
    pub fn create(env: &TestEnvironment) -> Self {
        Self {
            bucket: env.storage_bucket,
            prefix: format!("tests/{}/", Uuid::new_v4().simple()),
        }
    }

    pub fn key(&self, name: &str) -> String {
        format!("{}{name}", self.prefix)
    }
}

/// 供测试直接复用的 MinIO 容器句柄（例如需要在容器内执行命令时）。
pub fn minio_container(env: &TestEnvironment) -> &'static ContainerAsync<GenericImage> {
    env.minio
}

/// 构造一个不连接任何真实依赖的配置（只用于应用骨架/中间件测试）。
pub fn test_config(static_dir: std::path::PathBuf) -> server::Config {
    server::Config {
        bind_addr: "127.0.0.1:0".parse().expect("测试监听地址"),
        metrics_addr: "127.0.0.1:0".parse().expect("测试指标地址"),
        static_dir,
        database: server::config::DatabaseConfig {
            url: server::config::Secret::new(
                "postgres://backup:test-password@127.0.0.1:5432/backup_test",
            ),
            max_connections: 2,
        },
        storage: server::config::StorageConfig {
            kind: server::config::StorageKind::S3,
            bucket: MINIO_BUCKET.to_owned(),
            region: "us-east-1".to_owned(),
            internal_endpoint: "http://127.0.0.1:9000".to_owned(),
            public_endpoint: "http://127.0.0.1:9000".to_owned(),
            access_key_id: server::config::Secret::new(MINIO_ACCESS_KEY),
            secret_access_key: server::config::Secret::new(MINIO_SECRET_KEY),
            allow_http: true,
        },
    }
}

/// 连接真实 PostgreSQL/MinIO 的测试配置。
pub fn integration_config(env: &TestEnvironment, static_dir: std::path::PathBuf) -> server::Config {
    let mut config = test_config(static_dir);
    config.database.url = server::config::Secret::new(env.database_url.clone());
    config.storage.internal_endpoint = env.storage_endpoint.clone();
    config.storage.public_endpoint = env.storage_endpoint.clone();
    config
}

/// 创建并迁移一个独立测试数据库，返回连接池句柄。
pub async fn migrated_database(env: &TestEnvironment) -> (TestDb, server::Database) {
    let test_db = TestDb::create(env).await;
    let pool = test_db.connect().await;
    server::db::MIGRATOR
        .run(&pool)
        .await
        .expect("测试数据库迁移应当成功");
    (test_db, server::Database::from_pool(pool))
}

/// 构造连接真实依赖的 [`server::AppState`]。
pub fn integration_state(
    env: &TestEnvironment,
    database: server::Database,
    static_dir: std::path::PathBuf,
) -> server::AppState {
    let config = integration_config(env, static_dir);
    let storage = server::Storage::new(&config.storage).expect("构造 storage");
    server::AppState::new(config, database, storage)
}

/// 构造指向测试 MinIO 的对象存储配置。
pub fn integration_storage_config(env: &TestEnvironment) -> server::config::StorageConfig {
    integration_config(env, std::env::temp_dir()).storage
}

/// 构造一组完整、合法的 `BACKUP_*` 环境变量，供启动二进制的测试使用。
pub fn config_env(
    env: &TestEnvironment,
    database_url: &str,
    static_dir: &std::path::Path,
) -> Vec<(String, String)> {
    vec![
        ("BACKUP_BIND_ADDR".to_owned(), "127.0.0.1:0".to_owned()),
        ("BACKUP_METRICS_ADDR".to_owned(), "127.0.0.1:0".to_owned()),
        (
            "BACKUP_STATIC_DIR".to_owned(),
            static_dir.display().to_string(),
        ),
        ("BACKUP_DATABASE_URL".to_owned(), database_url.to_owned()),
        ("BACKUP_STORAGE_KIND".to_owned(), "s3".to_owned()),
        (
            "BACKUP_STORAGE_BUCKET".to_owned(),
            env.storage_bucket.to_owned(),
        ),
        ("BACKUP_STORAGE_REGION".to_owned(), "us-east-1".to_owned()),
        (
            "BACKUP_STORAGE_INTERNAL_ENDPOINT".to_owned(),
            env.storage_endpoint.clone(),
        ),
        (
            "BACKUP_STORAGE_PUBLIC_ENDPOINT".to_owned(),
            env.storage_endpoint.clone(),
        ),
        (
            "BACKUP_STORAGE_ACCESS_KEY_ID".to_owned(),
            env.storage_access_key.to_owned(),
        ),
        (
            "BACKUP_STORAGE_SECRET_ACCESS_KEY".to_owned(),
            env.storage_secret_key.to_owned(),
        ),
        ("BACKUP_STORAGE_ALLOW_HTTP".to_owned(), "true".to_owned()),
    ]
}

/// 构造一个不连接任何真实依赖的 [`server::AppState`]（只用于应用骨架/中间件测试）。
pub fn test_state(config: server::Config) -> server::AppState {
    let database = server::Database::connect_lazy(&config.database).expect("延迟连接池不应失败");
    let storage = server::Storage::new(&config.storage).expect("构造内存/网络无关的 storage");
    server::AppState::new(config, database, storage)
}

/// 创建带静态产物的临时目录，模拟 `web/dist`（含哈希资源、index.html 与 wasm）。
pub fn static_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("创建临时静态目录");
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("assets")).expect("创建 assets 目录");
    std::fs::write(
        root.join("index.html"),
        "<!doctype html><html><body><div id=\"root\">backup-spa</div></body></html>",
    )
    .expect("写入 index.html");
    std::fs::write(
        root.join("assets/index-DiwrgTda.js"),
        "console.log('hashed asset')",
    )
    .expect("写入带哈希的 JS");
    std::fs::write(root.join("assets/app.wasm"), b"\0asm").expect("写入 wasm");
    std::fs::write(root.join("favicon.svg"), "<svg></svg>").expect("写入 favicon");
    (dir, root)
}
