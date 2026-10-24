//! `server` 二进制入口。
//!
//! 子命令：
//! - `serve`：校验配置与迁移版本后启动 HTTP 服务；迁移落后/校验和不一致时拒绝启动。
//! - `migrate`：显式执行数据库迁移（发布流程中先跑这一步）。
//!
//! OpenAPI 子命令在 4.7 加入。

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use server::{AppState, Config, Database, StartupError, Storage, build_router, metrics};

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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match command {
        Command::Serve => runtime.block_on(serve()),
        Command::Migrate => runtime.block_on(migrate()),
    }
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
    let storage = Storage::new(&config.storage)?;

    let shutdown = tokio_util::sync::CancellationToken::new();

    // Prometheus 指标使用独立端口，默认只绑定回环地址（配置可覆盖）。
    let metrics_handle = metrics::install()?;
    let metrics_addr = config.metrics_addr;
    let metrics_listener = tokio::net::TcpListener::bind(metrics_addr).await?;
    let metrics_router = metrics::router(metrics_handle);
    let metrics_task = tokio::spawn(async move {
        if let Err(error) = axum::serve(metrics_listener, metrics_router).await {
            tracing::error!(error = %error, "指标服务异常退出");
        }
    });
    let sampler = metrics::spawn_pool_sampler(database.pool().clone(), shutdown.clone());

    let bind_addr = config.bind_addr;
    let router = build_router(AppState::new(config, database, storage));
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!(bind_addr = %bind_addr, metrics_addr = %metrics_addr, "HTTP 服务已启动");
    axum::serve(listener, router).await?;

    // 优雅停机在 4.6 接入；这里保证后台采样任务能正常退出。
    shutdown.cancel();
    let _ = sampler.await;
    metrics_task.abort();
    Ok(())
}

async fn migrate() -> anyhow::Result<()> {
    let config = Config::load()?;
    let database = Database::connect(&config.database).await?;
    database.migrate().await?;
    let state = database.migration_state().await?;
    tracing::info!(state = %state, "数据库迁移完成");
    Ok(())
}
