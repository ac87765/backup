//! `server` 二进制入口。
//!
//! 子命令：
//! - `serve`：校验配置与迁移版本后启动 HTTP 服务；迁移落后/校验和不一致时拒绝启动。
//! - `migrate`：显式执行数据库迁移（发布流程中先跑这一步）。
//! - `openapi`：把 OpenAPI 规范输出到 stdout（不需要配置，也不连接任何依赖）。

use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use server::{
    AppState, Config, Database, MigrateConfig, StartupError, Storage, build_router, config, jobs,
    mail, metrics, shutdown,
};

#[derive(Debug, Parser)]
#[command(name = "server", version, about = "端对端加密备份服务")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 启动 HTTP API 与前端静态文件服务。
    Serve,
    /// 执行数据库迁移。
    Migrate,
    /// 输出 OpenAPI 规范到 stdout。
    Openapi,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // 日志固定写 stderr：`openapi` 的 stdout 必须只有规范 JSON。
    server::telemetry::init_bootstrap();
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // 错误链可能包含底层原因（例如迁移版本号），但不包含配置值。
            tracing::error!(error = %format!("{error:#}"), "服务启动失败");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> anyhow::Result<()> {
    match command {
        Command::Openapi => {
            let json = server::openapi::spec_json_pretty()?;
            println!("{json}");
            Ok(())
        }
        Command::Serve => runtime()?.block_on(serve()),
        Command::Migrate => runtime()?.block_on(migrate()),
    }
}

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}

async fn serve() -> anyhow::Result<()> {
    let config = Config::load()?;
    let database = Database::connect(&config.database).await?;
    let migration_state = database.migration_state().await?;
    if !migration_state.is_ready() {
        return Err(StartupError::Migration(migration_state).into());
    }
    if !migration_state.database_ahead().is_empty() {
        tracing::warn!(
            database_ahead = ?migration_state.database_ahead(),
            "数据库迁移版本领先于当前代码（允许启动，保持向前兼容）"
        );
    }
    let storage = Arc::new(Storage::new(&config.storage)?);
    let mailer = mail::build_mailer(&config.mail)?;
    if let config::MailConfig::File { dir } = &config.mail {
        tracing::warn!(
            dir = %dir.display(),
            "BACKUP_MAIL_MODE=file：邮件不会真正发送，将写入本地目录"
        );
    }

    let shutdown = shutdown::Shutdown::new();
    let signal_task = tokio::spawn(shutdown::wait_for_shutdown_signal(shutdown.clone()));

    // Prometheus 指标使用独立端口，默认只绑定回环地址（配置可覆盖）。
    let metrics_handle = metrics::install()?;
    let metrics_addr = config.metrics_addr;
    let metrics_listener = tokio::net::TcpListener::bind(metrics_addr).await?;
    let metrics_router = metrics::router(metrics_handle);
    let metrics_shutdown = shutdown.clone();
    let _metrics_task = shutdown.spawn(async move {
        let token = metrics_shutdown.token();
        let server = axum::serve(metrics_listener, metrics_router)
            .with_graceful_shutdown(async move { token.cancelled().await });
        if let Err(error) = server.await {
            tracing::error!(error = %error, "指标服务异常退出");
        }
    });
    let _sampler = shutdown.spawn(metrics::pool_sampler(
        database.pool().clone(),
        shutdown.token(),
    ));

    jobs::start(
        database.clone(),
        storage.clone(),
        mailer,
        Arc::new(jobs::email::SqlRecipientResolver),
        config.public_app_url.clone(),
        &shutdown,
        config.jobs_enabled,
    );

    let bind_addr = config.bind_addr;
    let router = build_router(AppState::new(config, database, Storage::clone(&storage)));
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(bind_addr = %bind_addr, metrics_addr = %metrics_addr, "HTTP 服务已启动");
    let result = shutdown::serve_with_shutdown(listener, router, &shutdown).await;

    // 无论服务正常结束还是出错，都通知后台任务退出。
    // 在途请求与后台任务共享同一个总期限（默认 30 秒），不会先后各等 30 秒。
    shutdown.cancel();
    if !shutdown.wait_background_remaining().await {
        tracing::warn!(
            grace_secs = shutdown.grace().as_secs(),
            "后台任务未在停机总期限内结束"
        );
    }
    signal_task.abort();
    result?;
    Ok(())
}

async fn migrate() -> anyhow::Result<()> {
    // 迁移只依赖数据库；不要求存储、邮件或监听地址等运行期配置。
    let config = MigrateConfig::load()?;
    let database = Database::connect(&config.database).await?;
    database.migrate().await?;
    let state = database.migration_state().await?;
    tracing::info!(state = %state, "数据库迁移完成");
    Ok(())
}
