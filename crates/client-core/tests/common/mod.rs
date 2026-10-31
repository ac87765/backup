//! `client-core` 集成测试共享框架。
//!
//! 参照 `crates/server/tests/common`：
//! - testcontainers 启动真实 PostgreSQL 16 与 MinIO；
//! - 每个测试独立数据库，迁移后在本进程内用 `axum::serve` 启动真实服务端路由；
//! - 客户端通过 HTTP 访问该服务端，覆盖会话 Cookie、CSRF 与错误码映射。
//!
//! 没有 Docker 时测试必须失败并给出明确提示（与 server 测试一致，不静默跳过）。

#![allow(dead_code)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::Path;
use std::process::Command;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crypto_core::{KdfParams, MasterKey};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::net::TcpListener;
use tokio::sync::OnceCell;
use uuid::Uuid;

/// 与 server 集成测试相同的容器标签，便于统一清理。
pub const CONTAINER_LABEL: &str = "backup-test";
pub const CONTAINER_PROCESS_LABEL: &str = "backup-test-process";

/// 服务端测试使用的 pepper；测试用户必须用同一个值计算 auth_hash。
pub const PEPPER: &str = "test-pepper-value-0123456789abcdef";

pub const POSTGRES_IMAGE: &str = "postgres";
pub const POSTGRES_TAG: &str = "16.15-alpine";

/// 与 `deploy/docker-compose.dev.yml` / `crates/server/tests/common` 保持同一 digest。
pub const MINIO_IMAGE_NAME: &str = "bitnamilegacy/minio@sha256";
pub const MINIO_IMAGE_DIGEST: &str =
    "451fe6858cb770cc9d0e77ba811ce287420f781c7c1b806a386f6896471a349c";

pub const MINIO_ACCESS_KEY: &str = "backup-test-access";
pub const MINIO_SECRET_KEY: &str = "backup-test-secret";
pub const MINIO_BUCKET: &str = "backup-test";

const POSTGRES_USER: &str = "backup";
const POSTGRES_PASSWORD: &str = "backup-test-password";
const POSTGRES_DB: &str = "backup_test";

static CONTAINERS_STARTED: AtomicBool = AtomicBool::new(false);
static CLEANUP_REGISTERED: Once = Once::new();

#[allow(unsafe_code)]
unsafe extern "C" {
    fn atexit(callback: extern "C" fn()) -> i32;
}

#[allow(unsafe_code)]
fn register_test_container_cleanup() {
    CLEANUP_REGISTERED.call_once(|| {
        // SAFETY: 回调是 extern "C" fn，不跨 FFI 传播 panic。
        let _ = unsafe { atexit(cleanup_test_containers) };
    });
}

extern "C" fn cleanup_test_containers() {
    if !CONTAINERS_STARTED.load(Ordering::SeqCst) {
        return;
    }
    let process_filter = format!("label={CONTAINER_PROCESS_LABEL}={}", std::process::id());
    let label_filter = format!("label={CONTAINER_LABEL}");
    let Ok(list) = Command::new("docker")
        .args([
            "ps",
            "-aq",
            "--filter",
            &label_filter,
            "--filter",
            &process_filter,
        ])
        .output()
    else {
        return;
    };
    if !list.status.success() {
        return;
    }
    let ids: Vec<String> = String::from_utf8_lossy(&list.stdout)
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if ids.is_empty() {
        return;
    }
    let _ = Command::new("docker")
        .args(["rm", "-f", "-v"])
        .args(&ids)
        .output();
}

pub struct TestEnvironment {
    pub database_url: String,
    pub storage_endpoint: String,
    pub storage_bucket: &'static str,
    pub storage_access_key: &'static str,
    pub storage_secret_key: &'static str,
    _minio: &'static ContainerAsync<GenericImage>,
    _postgres: &'static ContainerAsync<GenericImage>,
}

static ENVIRONMENT: OnceCell<TestEnvironment> = OnceCell::const_new();

pub async fn environment() -> &'static TestEnvironment {
    ENVIRONMENT.get_or_init(start_environment).await
}

fn docker_unavailable(context: &str, error: &dyn std::fmt::Display) -> ! {
    panic!(
        "client-core 集成测试需要可用的 Docker（PostgreSQL/MinIO）。\n\
         {context} 失败：{error}\n\
         本测试不会在缺少 Docker 时跳过；请在 WSL 中运行。"
    );
}

async fn start_environment() -> TestEnvironment {
    register_test_container_cleanup();
    let process_label = std::process::id().to_string();

    let postgres = GenericImage::new(POSTGRES_IMAGE, POSTGRES_TAG)
        .with_exposed_port(5432.tcp())
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_label(CONTAINER_LABEL, "1")
        .with_label(CONTAINER_PROCESS_LABEL, process_label.clone())
        .with_env_var("POSTGRES_DB", POSTGRES_DB)
        .with_env_var("POSTGRES_USER", POSTGRES_USER)
        .with_env_var("POSTGRES_PASSWORD", POSTGRES_PASSWORD)
        .start()
        .await
        .unwrap_or_else(|error| docker_unavailable("启动 PostgreSQL 容器", &error));
    CONTAINERS_STARTED.store(true, Ordering::SeqCst);
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
        .with_wait_for(WaitFor::message_on_either_std("MinIO setup finished"))
        .with_label(CONTAINER_LABEL, "1")
        .with_label(CONTAINER_PROCESS_LABEL, process_label)
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

    TestEnvironment {
        database_url,
        storage_endpoint: format!("http://127.0.0.1:{minio_port}"),
        storage_bucket: MINIO_BUCKET,
        storage_access_key: MINIO_ACCESS_KEY,
        storage_secret_key: MINIO_SECRET_KEY,
        _minio: minio,
        _postgres: postgres,
    }
}

/// 每测试独立的数据库。
pub struct TestDb {
    pub url: String,
    #[allow(dead_code)]
    pub name: String,
}

impl TestDb {
    pub async fn create(env: &TestEnvironment) -> Self {
        let name = format!("client_test_{}", Uuid::new_v4().simple());
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&env.database_url)
            .await
            .unwrap_or_else(|error| docker_unavailable("连接 PostgreSQL 管理库", &error));
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

pub async fn migrated_database(env: &TestEnvironment) -> (TestDb, server::Database) {
    let test_db = TestDb::create(env).await;
    let pool = test_db.connect().await;
    server::db::MIGRATOR
        .run(&pool)
        .await
        .expect("测试数据库迁移应当成功");
    (test_db, server::Database::from_pool(pool))
}

fn test_config(
    env: &TestEnvironment,
    database_url: String,
    static_dir: &Path,
    mail_dir: &Path,
) -> server::Config {
    server::Config {
        bind_addr: "127.0.0.1:0".parse().expect("测试监听地址"),
        metrics_addr: "127.0.0.1:0".parse().expect("测试指标地址"),
        static_dir: static_dir.to_path_buf(),
        database: server::config::DatabaseConfig {
            url: server::config::Secret::new(database_url),
            max_connections: 5,
        },
        storage: server::config::StorageConfig {
            kind: server::config::StorageKind::S3,
            bucket: env.storage_bucket.to_owned(),
            region: "us-east-1".to_owned(),
            internal_endpoint: env.storage_endpoint.clone(),
            public_endpoint: env.storage_endpoint.clone(),
            access_key_id: server::config::Secret::new(env.storage_access_key),
            secret_access_key: server::config::Secret::new(env.storage_secret_key),
            allow_http: true,
        },
        jobs_enabled: false,
        mail: server::config::MailConfig::File {
            dir: mail_dir.to_path_buf(),
        },
        auth: server::config::AuthConfig {
            pepper_version: 1,
            pepper: server::config::Secret::new(PEPPER),
            old_peppers: Vec::new(),
            fake_salt_key: server::config::Secret::new("test-fake-salt-key-0123456789abcdef"),
        },
        public_app_url: "http://127.0.0.1:8080".to_owned(),
        trusted_proxies: Vec::new(),
        auth_rate_limit_per_minute: NonZeroU32::new(600).expect("600"),
        user_write_rate_limit_per_minute: NonZeroU32::new(600).expect("600"),
        registration_rate_limit_per_hour: NonZeroU32::new(5).expect("5"),
        default_quota_bytes: server::config::DEFAULT_NEW_USER_QUOTA_BYTES as i64,
        max_quota_bytes: server::config::DEFAULT_MAX_QUOTA_BYTES as i64,
    }
}

/// 进程内启动真实服务端（随机端口）。
pub struct TestServer {
    pub base_url: String,
    pub database: server::Database,
    _db: TestDb,
    _dir: tempfile::TempDir,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn start_server(env: &TestEnvironment) -> TestServer {
    let (test_db, database) = migrated_database(env).await;
    let dir = tempfile::tempdir().expect("创建测试临时目录");
    let static_dir = dir.path().join("static");
    let mail_dir = dir.path().join("mail");
    std::fs::create_dir_all(static_dir.join("assets")).expect("创建静态目录");
    std::fs::write(
        static_dir.join("index.html"),
        "<!doctype html><html><body>client-core test</body></html>",
    )
    .expect("写入 index.html");
    std::fs::create_dir_all(&mail_dir).expect("创建邮件目录");

    let config = test_config(env, test_db.url.clone(), &static_dir, &mail_dir);
    let storage = server::Storage::new(&config.storage).expect("构造 storage");
    let state = server::AppState::new(config, database.clone(), storage);
    let app = server::build_router(state);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定测试端口");
    let address = listener.local_addr().expect("读取测试端口");
    let task = tokio::spawn(async move {
        let make_service = app.into_make_service_with_connect_info::<SocketAddr>();
        let _ = axum::serve(listener, make_service).await;
    });

    TestServer {
        base_url: format!("http://{address}"),
        database,
        _db: test_db,
        _dir: dir,
        task,
    }
}

/// 测试用户及其登录密码。
pub struct TestUser {
    pub user_id: Uuid,
    pub email: String,
    pub password: String,
    pub root_node_id: Uuid,
    /// 测试账号的主密钥；仅测试用于独立验证客户端上传的密文。
    pub master_key: MasterKey,
}

/// 直接写库创建一个账号（不经过注册接口；注册不在第 1 组范围）。
///
/// KDF 参数使用生产推荐值，登录路径与真实用户完全一致。
pub async fn create_user(
    database: &server::Database,
    status: &str,
    is_admin: bool,
    password: &str,
) -> TestUser {
    let user_id = Uuid::new_v4();
    let root_node_id = Uuid::new_v4();
    let email = format!("client-test-{user_id}@example.com");
    let salt = crypto_core::generate_salt().expect("生成 KDF 盐");
    let params = KdfParams::RECOMMENDED;
    let keys =
        crypto_core::derive_keys_from_password(password, &salt, &params).expect("派生测试用户密钥");
    let master_key = MasterKey::generate().expect("生成测试主密钥");
    let wrapped_mk =
        crypto_core::wrap_mk_with_password(&keys.pwd_wrap, &master_key).expect("包裹测试主密钥");
    let auth_hash = server::auth::hmac_sha256(PEPPER.as_bytes(), keys.auth_key.expose_secret())
        .expect("计算测试 auth_hash");
    let recovery_hash = [0_u8; 32];
    let mut recovery_wrapped = vec![crypto_core::CIPHERTEXT_FORMAT_VERSION];
    recovery_wrapped.extend(std::iter::repeat_n(
        0x33_u8,
        crypto_core::FRAME_OVERHEAD + crypto_core::KEY_LEN - 1,
    ));
    let kdf_params = serde_json::to_value(params).expect("序列化 KDF 参数");

    let mut tx = database.pool().begin().await.expect("开始测试用户事务");
    sqlx::query(
        r#"
        INSERT INTO users (
            id, email, kdf_params, kdf_salt, auth_hash, pepper_version,
            recovery_auth_hash, recovery_pepper_version, wrapped_mk, recovery_wrapped_mk,
            root_node_id, status, is_admin, approved_at, email_verified_at
        )
        VALUES ($1, $2::text::citext, $3, $4, $5, 1, $6, 1, $7, $8, $9, $10, $11,
                CASE WHEN $10 = 'active' THEN now() ELSE NULL END, now())
        "#,
    )
    .bind(user_id)
    .bind(&email)
    .bind(kdf_params)
    .bind(salt.as_slice())
    .bind(auth_hash.as_slice())
    .bind(recovery_hash.as_slice())
    .bind(wrapped_mk.as_slice())
    .bind(recovery_wrapped.as_slice())
    .bind(root_node_id)
    .bind(status)
    .bind(is_admin)
    .execute(&mut *tx)
    .await
    .expect("插入测试用户");
    sqlx::query(
        r#"
        INSERT INTO nodes (id, user_id, parent_id, kind, row_version)
        VALUES ($1, $2, NULL, 'folder', 1)
        "#,
    )
    .bind(root_node_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .expect("插入测试根节点");
    tx.commit().await.expect("提交测试用户事务");

    TestUser {
        user_id,
        email,
        password: password.to_owned(),
        root_node_id,
        master_key,
    }
}

/// 撤销某用户全部会话（模拟管理员停用/撤销设备）。
pub async fn revoke_sessions(database: &server::Database, user_id: Uuid) {
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user_id)
        .execute(database.pool())
        .await
        .expect("撤销测试用户会话");
}

/// 当前仍有效的会话数。
pub async fn active_session_count(database: &server::Database, user_id: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(user_id)
        .fetch_one(database.pool())
        .await
        .expect("统计有效会话")
}

/// 最近一次登录写入的 User-Agent。
pub async fn latest_user_agent(database: &server::Database, user_id: Uuid) -> Option<String> {
    sqlx::query_scalar(
        "SELECT user_agent FROM sessions WHERE user_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_one(database.pool())
    .await
    .expect("读取会话 User-Agent")
}
