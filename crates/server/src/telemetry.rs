//! 日志初始化。
//!
//! 日志必须是 JSON（service-operations spec），且写入 **stderr**：
//! `server openapi` 会把 OpenAPI JSON 写到 stdout，日志不能污染该输出。

use tracing_subscriber::EnvFilter;

/// 安装默认的 JSON 日志订阅器。
///
/// 重复调用（例如测试中已经安装过）时静默返回，不 panic。
pub fn init_bootstrap() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // 全局订阅器只能安装一次；测试或重复初始化时保持已有订阅器。
    let _ = tracing_subscriber::fmt()
        .json()
        .with_current_span(true)
        .with_span_list(false)
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .try_init();
}
