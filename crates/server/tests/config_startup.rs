//! 配置加载的进程级验证：缺项/非法值必须以非零退出码退出，且只指出配置项名。
//!
//! 这里通过真实启动二进制并断言 stderr，覆盖 main → config → 日志这条链路。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{ServeEnvSettings, serve_binary_env, server_command};

/// 一组完整、合法的配置（用于 isolate 单个缺失/非法项）。
///
/// 与 `tests/common` 共用同一个构造器，避免各测试自行拼环境变量时漏项。
fn startup_env() -> common::ServeBinaryEnv {
    let static_dir = std::env::temp_dir().join("backup-config-startup");
    serve_binary_env(ServeEnvSettings {
        database_url: "postgres://backup:secret-db-password@127.0.0.1:5432/backup",
        static_dir: &static_dir,
        storage_bucket: "backup",
        storage_internal_endpoint: "http://127.0.0.1:9000",
        storage_public_endpoint: "http://127.0.0.1:9000",
        storage_access_key: "access-key-value",
        storage_secret_key: "secret-key-value",
    })
}

#[test]
fn missing_config_exits_non_zero_and_names_the_item() {
    // 这个用例故意从空环境启动：它验证“完全没有配置”时的第一个缺失项。
    let output = server_command()
        .arg("serve")
        .output()
        .expect("应当能启动 server 二进制");
    assert!(!output.status.success(), "缺少配置时必须非零退出");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("BACKUP_DATABASE_URL"),
        "错误日志必须指出缺失的配置项，实际 stderr: {stderr}"
    );
}

#[test]
fn missing_mail_mode_exits_non_zero_and_names_the_item() {
    let startup = startup_env();
    let mut command = server_command();
    command.arg("serve");
    startup.apply_with(&mut command, &[], &["BACKUP_MAIL_MODE"]);
    let output = command.output().expect("应当能启动 server 二进制");
    assert!(
        !output.status.success(),
        "缺少 BACKUP_MAIL_MODE 必须非零退出"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("BACKUP_MAIL_MODE"),
        "错误日志必须指出缺失的配置项，实际 stderr: {stderr}"
    );
}

#[test]
fn invalid_config_exits_non_zero_and_never_echoes_values() {
    let startup = startup_env();
    let mut command = server_command();
    command.arg("serve");
    startup.apply_with(
        &mut command,
        &[("BACKUP_BIND_ADDR", "this-is-not-a-socket-address")],
        &[],
    );
    let output = command.output().expect("应当能启动 server 二进制");
    assert!(!output.status.success(), "非法配置必须非零退出");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("BACKUP_BIND_ADDR"),
        "错误日志必须指出非法配置项，实际 stderr: {stderr}"
    );
    assert!(
        !stderr.contains("this-is-not-a-socket-address"),
        "错误日志不得包含配置值，实际 stderr: {stderr}"
    );
    // 其他密钥值同样不得出现在日志里。
    assert!(!stderr.contains("secret-db-password"));
    assert!(!stderr.contains("secret-key-value"));
}
