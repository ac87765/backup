//! Prometheus 指标（独立端口）。
//!
//! 本组实现：
//! - HTTP 请求数 / 延迟（按路由模板与状态码）；
//! - 数据库连接池使用量。
//!
//! 上传会话、删除队列积压、后台任务执行与失败指标分别在本项目第 5、8 组补充
//! （依赖相应的业务表与任务框架）。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{MatchedPath, Request};
use axum::http::{HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;

/// 连接池指标的采样间隔。
const POOL_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

/// 指标记录器安装失败。
#[derive(Debug, thiserror::Error)]
#[error("无法安装 Prometheus 指标记录器")]
pub struct MetricsError(#[source] metrics_exporter_prometheus::BuildError);

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
static INSTALL_LOCK: Mutex<()> = Mutex::new(());

/// 安装全局 Prometheus 记录器（进程内只安装一次；重复调用返回同一 handle）。
pub fn install() -> Result<PrometheusHandle, MetricsError> {
    let _guard = INSTALL_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(handle) = HANDLE.get() {
        return Ok(handle.clone());
    }
    // 延迟指标显式使用 Prometheus histogram buckets（而不是 summary），
    // 便于多实例聚合与 P50/P95/P99 告警。
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("http_request_duration_seconds".to_owned()),
            &[
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ],
        )
        .map_err(MetricsError)?
        .install_recorder()
        .map_err(MetricsError)?;
    let _ = HANDLE.set(handle.clone());
    Ok(handle)
}

/// 指标服务的路由（只暴露 `/metrics`，监听地址由配置决定，默认回环地址）。
pub fn router(handle: PrometheusHandle) -> Router {
    Router::new().route(
        "/metrics",
        get(move || {
            let handle = handle.clone();
            async move {
                (
                    [(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
                    )],
                    handle.render(),
                )
                    .into_response()
            }
        }),
    )
}

/// HTTP 请求指标中间件。
///
/// 标签使用路由模板（例如 `/api/v1/nodes/{id}`）而不是原始路径，避免高基数。
pub async fn track_http(request: Request, next: Next) -> Response {
    let method = request.method().as_str().to_owned();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_owned())
        .unwrap_or_else(|| "unmatched".to_owned());
    let started = Instant::now();

    let response = next.run(request).await;
    let status = response.status().as_u16().to_string();
    let elapsed = started.elapsed().as_secs_f64();

    metrics::counter!(
        "http_requests_total",
        "method" => method.clone(),
        "route" => route.clone(),
        "status" => status.clone(),
    )
    .increment(1);
    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method,
        "route" => route,
        "status" => status,
    )
    .record(elapsed);

    response
}

/// 写入一次连接池指标快照。
pub fn record_pool_metrics(pool: &PgPool) {
    let total = pool.size() as f64;
    let idle = pool.num_idle() as f64;
    metrics::gauge!("db_pool_connections", "state" => "idle").set(idle);
    metrics::gauge!("db_pool_connections", "state" => "in_use").set((total - idle).max(0.0));
    metrics::gauge!("db_pool_connections_max").set(pool.options().get_max_connections() as f64);
}

/// 周期性采样连接池指标，直到收到停机信号。
pub fn spawn_pool_sampler(
    pool: PgPool,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(POOL_SAMPLE_INTERVAL);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = ticker.tick() => record_pool_metrics(&pool),
            }
        }
    })
}
