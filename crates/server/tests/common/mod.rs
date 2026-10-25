//! 集成测试共享框架：真实 PostgreSQL 16 与 MinIO（testcontainers）。
//!
//! 设计要点（tasks 4.8）：
//! - 容器按测试进程共享，但**每个测试拿到独立的数据库与对象键前缀**，因此测试可以并行运行。
//! - 没有 Docker 时测试必须失败，并给出明确提示；这里刻意不提供任何“跳过”逻辑。
//! - MinIO 镜像与 `deploy/docker-compose.dev.yml` 使用同一个 digest，测试
//!   `tests/test_harness.rs` 会断言两处一致。
//!
//! 容器在进程内共享（`static OnceCell`），因此不会依赖 `Drop`：进程退出时由
//! `atexit` 钩子按 `backup-test` + `backup-test-process=<pid>` 标签删除本进程
//! 启动的容器。多个测试二进制并行时，各进程只删自己的容器；异常崩溃的残留
//! 容器见 README 的手动清理命令。

#![allow(dead_code)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::path::Path;
use std::process::Command;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use testcontainers::core::{CmdWaitFor, ExecCommand, IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use tokio::sync::OnceCell;
use uuid::Uuid;

/// 所有测试容器的统一标签（`docker ps -a --filter label=backup-test`）。
pub const CONTAINER_LABEL: &str = "backup-test";
/// 记录启动容器的测试进程 ID，用于进程退出时只删除自己的容器。
pub const CONTAINER_PROCESS_LABEL: &str = "backup-test-process";

/// 是否已经成功启动过容器；没有 Docker 的进程不执行清理命令。
static CONTAINERS_STARTED: AtomicBool = AtomicBool::new(false);
/// 只注册一次 atexit 回调（每个测试二进制一个进程）。
static CLEANUP_REGISTERED: Once = Once::new();

#[allow(unsafe_code)]
unsafe extern "C" {
    /// libc / UCRT 的退出钩子；只在测试进程退出时安排一次 best-effort 清理。
    fn atexit(callback: extern "C" fn()) -> i32;
}

/// 注册进程退出清理钩子。
///
/// 使用 `atexit` 而不是 `Drop`：容器句柄被 `Box::leak` 到 static，
/// `ContainerAsync` 的 Drop 永远不会运行。回调只按本进程 PID 标签删除
/// 自己启动的容器。
#[allow(unsafe_code)]
fn register_test_container_cleanup() {
    CLEANUP_REGISTERED.call_once(|| {
        // SAFETY: 回调是 `extern "C" fn`，不跨 FFI 传播 panic；atexit 是
        // C 运行时提供的线程安全注册函数，失败时只是不执行清理。
        let _ = unsafe { atexit(cleanup_test_containers) };
    });
}

/// 进程退出时删除本进程创建的测试容器（含匿名卷）。
///
/// 容器句柄被 `Box::leak`，`ContainerAsync` 的 Drop 不会运行；这里按标签
/// 列出并 `docker rm -f -v`。只匹配本进程 PID 标签，不碰其他测试进程的容器。
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
    // PostgreSQL 已经启动：无论后续 MinIO 是否失败，退出时都要清理。
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
        // Bitnami 镜像在 setup 阶段先后台启动 MinIO、建默认 bucket，再停掉并前台启动；
        // 等到 setup 完成，避免测试命中“后台实例即将停止”的窗口。
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

/// 读取 `.eml` 文件并返回解码后的正文。
pub fn read_mail_body(path: &Path) -> String {
    let raw = std::fs::read_to_string(path).expect("读取邮件文件");
    decode_mail_body(&raw)
}

/// 从原始邮件（含头部）中取正文并按 `Content-Transfer-Encoding` 解码。
///
/// lettre 在正文含中文时使用 base64，较旧的简单正文可能使用
/// quoted-printable；测试统一通过本函数读取正文，避免直接匹配编码后的文本。
pub fn decode_mail_body(raw: &str) -> String {
    let (headers, body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .unwrap_or(("", raw));
    let encoding = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-transfer-encoding")
            .then(|| value.trim().to_ascii_lowercase())
    });

    match encoding.as_deref() {
        Some("base64") => {
            use base64::Engine;
            let cleaned: String = body
                .chars()
                .filter(|character| !character.is_ascii_whitespace())
                .collect();
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(cleaned.as_bytes())
                .expect("邮件正文必须是合法 base64");
            String::from_utf8(decoded).expect("邮件正文必须是 UTF-8")
        }
        Some("quoted-printable") => decode_quoted_printable(body),
        _ => body.to_owned(),
    }
}

fn decode_quoted_printable(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'=' {
            // 软换行：`=\r\n` 或 `=\n`
            if bytes.get(index + 1) == Some(&b'\r') && bytes.get(index + 2) == Some(&b'\n') {
                index += 3;
                continue;
            }
            if bytes.get(index + 1) == Some(&b'\n') {
                index += 2;
                continue;
            }
            if let (Some(high), Some(low)) = (
                bytes.get(index + 1).and_then(|byte| hex_value(*byte)),
                bytes.get(index + 2).and_then(|byte| hex_value(*byte)),
            ) {
                output.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).expect("quoted-printable 解码后必须是 UTF-8")
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// 插入一条最小的 users 行与根节点，返回根节点 ID。
///
/// `email_outbox` / `email_tokens` 在第 6 组补了指向 `users` 的外键；
/// `users.root_node_id` 在第 7 组改为 NOT NULL，因此这里必须同时创建根节点。
/// 两个外键都是 DEFERRABLE，可以在同一事务内互相引用。
pub async fn insert_test_user(database: &server::Database, user_id: Uuid) -> Uuid {
    let email = format!("test-{user_id}@example.com");
    let params = serde_json::json!({
        "algorithm": "argon2id",
        "m_cost_kib": 65536,
        "t_cost": 3,
        "p_cost": 1,
    });
    let salt = [0_u8; 16];
    let hash = [0_u8; 32];
    let wrapped = vec![1_u8; crypto_core::FRAME_OVERHEAD + crypto_core::KEY_LEN];
    let root_node_id = Uuid::new_v4();
    let mut tx = database.pool().begin().await.expect("开始插入测试用户事务");
    sqlx::query(
        r#"
        INSERT INTO users (
            id, email, kdf_params, kdf_salt, auth_hash, pepper_version,
            recovery_auth_hash, recovery_pepper_version, wrapped_mk, recovery_wrapped_mk,
            root_node_id
        )
        VALUES ($1, $2::text::citext, $3, $4, $5, 1, $6, 1, $7, $8, $9)
        "#,
    )
    .bind(user_id)
    .bind(email)
    .bind(params)
    .bind(salt.as_slice())
    .bind(hash.as_slice())
    .bind(hash.as_slice())
    .bind(wrapped.as_slice())
    .bind(wrapped.as_slice())
    .bind(root_node_id)
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
    root_node_id
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
        jobs_enabled: false,
        mail: server::config::MailConfig::File {
            dir: std::env::temp_dir().join("backup-test-mail"),
        },
        auth: server::config::AuthConfig {
            pepper_version: 1,
            pepper: server::config::Secret::new("test-pepper-value-0123456789abcdef"),
            old_peppers: Vec::new(),
            fake_salt_key: server::config::Secret::new("test-fake-salt-key-0123456789abcdef"),
        },
        public_app_url: "http://127.0.0.1:8080".to_owned(),
        trusted_proxies: Vec::new(),
        auth_rate_limit_per_minute: std::num::NonZeroU32::new(60).expect("60"),
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

/// 启动真实 server 二进制；默认清空环境，避免开发机上的 `BACKUP_*` 干扰结果。
pub fn server_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_server"));
    command.env_clear();
    command
}

/// 完整 `serve` 环境变量的输入。
///
/// 不依赖 [`TestEnvironment`]，因此不需要 Docker 的进程级配置测试也能复用。
pub struct ServeEnvSettings<'a> {
    pub database_url: &'a str,
    pub static_dir: &'a Path,
    pub storage_bucket: &'a str,
    pub storage_internal_endpoint: &'a str,
    pub storage_public_endpoint: &'a str,
    pub storage_access_key: &'a str,
    pub storage_secret_key: &'a str,
}

/// 一组完整、合法的 `serve` 环境变量，并持有临时邮件目录直到测试结束。
pub struct ServeBinaryEnv {
    vars: Vec<(String, String)>,
    _mail_dir: tempfile::TempDir,
}

impl ServeBinaryEnv {
    /// 把完整环境写入命令（先 `env_clear`）。
    pub fn apply(&self, command: &mut Command) {
        self.apply_with(command, &[], &[]);
    }

    /// 写入完整环境，但可排除某些项或覆盖某些项（供配置失败测试使用）。
    pub fn apply_with(&self, command: &mut Command, overrides: &[(&str, &str)], excluded: &[&str]) {
        command.env_clear();
        for (key, value) in &self.vars {
            if excluded.contains(&key.as_str()) {
                continue;
            }
            command.env(key, value);
        }
        for (key, value) in overrides {
            command.env(key, value);
        }
    }
}

/// 构造完整、合法的 `serve` 环境变量。
pub fn serve_binary_env(settings: ServeEnvSettings<'_>) -> ServeBinaryEnv {
    let mail_dir = tempfile::tempdir().expect("创建临时邮件目录");
    let vars = vec![
        ("BACKUP_BIND_ADDR".to_owned(), "127.0.0.1:0".to_owned()),
        ("BACKUP_METRICS_ADDR".to_owned(), "127.0.0.1:0".to_owned()),
        (
            "BACKUP_STATIC_DIR".to_owned(),
            settings.static_dir.display().to_string(),
        ),
        (
            "BACKUP_DATABASE_URL".to_owned(),
            settings.database_url.to_owned(),
        ),
        ("BACKUP_STORAGE_KIND".to_owned(), "s3".to_owned()),
        (
            "BACKUP_STORAGE_BUCKET".to_owned(),
            settings.storage_bucket.to_owned(),
        ),
        ("BACKUP_STORAGE_REGION".to_owned(), "us-east-1".to_owned()),
        (
            "BACKUP_STORAGE_INTERNAL_ENDPOINT".to_owned(),
            settings.storage_internal_endpoint.to_owned(),
        ),
        (
            "BACKUP_STORAGE_PUBLIC_ENDPOINT".to_owned(),
            settings.storage_public_endpoint.to_owned(),
        ),
        (
            "BACKUP_STORAGE_ACCESS_KEY_ID".to_owned(),
            settings.storage_access_key.to_owned(),
        ),
        (
            "BACKUP_STORAGE_SECRET_ACCESS_KEY".to_owned(),
            settings.storage_secret_key.to_owned(),
        ),
        ("BACKUP_STORAGE_ALLOW_HTTP".to_owned(), "true".to_owned()),
        ("BACKUP_MAIL_MODE".to_owned(), "file".to_owned()),
        (
            "BACKUP_MAIL_DIR".to_owned(),
            mail_dir.path().display().to_string(),
        ),
        ("BACKUP_AUTH_PEPPER_VERSION".to_owned(), "1".to_owned()),
        (
            "BACKUP_AUTH_PEPPER".to_owned(),
            "test-pepper-value-0123456789abcdef".to_owned(),
        ),
        (
            "BACKUP_AUTH_FAKE_SALT_KEY".to_owned(),
            "test-fake-salt-key-0123456789abcdef".to_owned(),
        ),
    ];
    ServeBinaryEnv {
        vars,
        _mail_dir: mail_dir,
    }
}

/// 连接真实测试环境的完整 `serve` 环境变量。
pub fn integration_serve_env(
    env: &TestEnvironment,
    database_url: &str,
    static_dir: &Path,
) -> ServeBinaryEnv {
    serve_binary_env(ServeEnvSettings {
        database_url,
        static_dir,
        storage_bucket: env.storage_bucket,
        storage_internal_endpoint: &env.storage_endpoint,
        storage_public_endpoint: &env.storage_endpoint,
        storage_access_key: env.storage_access_key,
        storage_secret_key: env.storage_secret_key,
    })
}

/// `migrate` 只需要数据库配置；刻意只设置 `BACKUP_DATABASE_URL`，
/// 用来验证迁移入口不会再要求存储、邮件或监听地址。
pub fn apply_migrate_env(command: &mut Command, database_url: &str) {
    command.env_clear().env("BACKUP_DATABASE_URL", database_url);
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
