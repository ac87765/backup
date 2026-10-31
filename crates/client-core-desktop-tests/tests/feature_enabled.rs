//! 工作区构建的 desktop feature 哨兵测试。
//!
//! 这个 crate 不测试业务逻辑；它存在的作用是让 `cargo test --workspace` 与
//! `cargo clippy --workspace --all-targets` 在构建图里启用 `client-core/desktop`。
//! 真正的备份引擎测试在 `client-core`：
//! - 单元测试：`desktop` 模块内的 `#[cfg(test)]`；
//! - 无 Docker 集成测试：`crates/client-core/tests/backup_fake.rs`；
//! - Docker 集成测试：`crates/client-core/tests/backup.rs`。

#[test]
fn desktop_feature_is_enabled_for_workspace_builds() {
    // 引用一个只有 desktop feature 才存在的公开常量；若 feature 未启用，本测试
    // 会编译失败，从而保证 CI 不会静默跳过备份引擎。
    assert!(client_core::desktop::DEFAULT_DEBOUNCE.as_secs() >= 1);
}
