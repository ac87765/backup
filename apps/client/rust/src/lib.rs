//! 加密备份桌面客户端的 Flutter ↔ Rust 桥接层。
//!
//! 本 crate 只做两件事：
//! 1. 把 `client-core` 的公开 API 包装成异步函数 / Stream；
//! 2. 把 `client-core` 的展示类型转换成 DTO，把错误转换成稳定的错误码 + 中文提示。
//!
//! 密钥、`Client`、`BackupEngine` 都留在 Rust 内；Dart 只能看到
//! [`api::types`] 中的展示数据。边界由 `tests/boundary.rs` 自动校验。

pub mod api;
mod convert;
// 生成的 FFI 胶水层必须使用 unsafe；手写模块仍然由 workspace 的
// `unsafe_code = "deny"` 约束。生成文件同时跳过 rustfmt（见 flutter_rust_bridge.yaml）。
//
// 生成代码内部使用 unwrap/unimplemented（序列化分支的“不可达”写法）且会随
// 代码生成重写，因此只在这一个模块上放行这些 restriction lint；workspace
// lints 不放宽，手写代码继续禁止。
#[rustfmt::skip]
#[allow(
    unsafe_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented
)]
mod frb_generated;
mod state;
