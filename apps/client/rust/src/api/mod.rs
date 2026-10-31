//! 暴露给 Dart 的全部 API。
//!
//! 本模块是 flutter_rust_bridge 的输入命名空间（`flutter_rust_bridge.yaml` 的
//! `rust_input: crate::api`）。所有公开函数都是薄层：解析参数、调用
//! `client-core`、把结果/错误转成 DTO；不实现业务逻辑。
//!
//! 模块划分与 design D1 对应：认证、文件、传输、备份引擎、初始化。iOS 之后
//! 复用 auth/files/transfer 与 DTO 中平台无关的部分。

pub mod auth;
pub mod backup;
pub mod error;
pub mod files;
pub mod init;
pub mod transfer;
pub mod types;

/// flutter_rust_bridge 启动钩子（`RustLib.init()` 时调用一次）。
#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
}
