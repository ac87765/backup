//! 备份服务库：配置、HTTP 应用骨架、数据库迁移、对象存储与健康检查。
//!
//! 二进制入口只做参数解析与生命周期管理，具体能力都在本 crate 内实现，
//! 以便集成测试直接构造应用（不启动真实进程也能覆盖中间件与错误映射）。

pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod jobs;
pub mod mail;
pub mod metrics;
pub mod openapi;
pub mod shutdown;
pub mod storage;
pub mod telemetry;

pub use api::AppState;
pub use api::build_router;
pub use config::{Config, MigrateConfig};
pub use db::{Database, MigrationState};
pub use error::StartupError;
pub use storage::Storage;
