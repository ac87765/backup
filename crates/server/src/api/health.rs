//! 健康检查。
//!
//! - `/healthz`：进程可响应即成功（存活检查）。
//! - `/readyz`：数据库可连接、迁移版本匹配、对象存储可访问三项全部成功才 200，
//!   否则 503。每项检查独立超时（2 秒）并并发执行，保证 readyz 本身不会挂住。

use std::future::Future;
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;
use utoipa::ToSchema;

use crate::{AppState, MigrationState};

/// 单项就绪检查的超时时间。
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// 存活检查响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
}

/// 就绪检查各子项状态。
#[derive(Debug, Serialize, ToSchema)]
pub struct ReadyChecks {
    pub database: String,
    pub migrations: String,
    pub storage: String,
}

/// 就绪检查响应。
#[derive(Debug, Serialize, ToSchema)]
pub struct ReadyResponse {
    pub status: String,
    pub checks: ReadyChecks,
}

/// 存活检查。
#[utoipa::path(get, path = "/healthz", responses(
    (status = 200, description = "进程存活", body = HealthResponse)
))]
pub async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_owned(),
    })
}

/// 就绪检查。
#[utoipa::path(get, path = "/readyz", responses(
    (status = 200, description = "服务就绪", body = ReadyResponse),
    (status = 503, description = "依赖不可用或迁移版本不匹配", body = ReadyResponse)
))]
pub async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<ReadyResponse>) {
    let database = state.database.clone();
    let migrations = state.database.clone();
    let storage = state.storage.clone();

    let (database, migrations, storage) = tokio::join!(
        check("database", async move {
            database.ping().await.map_err(|error| error.to_string())
        }),
        check("migrations", async move {
            match migrations.migration_state().await {
                Ok(MigrationState::Ready { .. }) => Ok(()),
                Ok(state) => Err(state.to_string()),
                Err(error) => Err(error.to_string()),
            }
        }),
        check("storage", async move {
            storage.probe().await.map_err(|error| error.to_string())
        }),
    );

    let ready = database == "ok" && migrations == "ok" && storage == "ok";
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(ReadyResponse {
            status: if ready { "ready" } else { "not_ready" }.to_owned(),
            checks: ReadyChecks {
                database: database.to_owned(),
                migrations: migrations.to_owned(),
                storage: storage.to_owned(),
            },
        }),
    )
}

/// 执行单项检查，独立超时；失败只记录分类（不向客户端暴露内部细节）。
async fn check<F>(name: &'static str, future: F) -> &'static str
where
    F: Future<Output = Result<(), String>>,
{
    match tokio::time::timeout(CHECK_TIMEOUT, future).await {
        Ok(Ok(())) => "ok",
        Ok(Err(reason)) => {
            tracing::warn!(check = name, reason = %reason, "就绪检查失败");
            "error"
        }
        Err(_) => {
            tracing::warn!(
                check = name,
                timeout_secs = CHECK_TIMEOUT.as_secs(),
                "就绪检查超时"
            );
            "timeout"
        }
    }
}
