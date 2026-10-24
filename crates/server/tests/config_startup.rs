//! 配置加载的进程级验证：缺项/非法值必须以非零退出码退出，且只指出配置项名。
//!
//! 这里通过真实启动二进制并断言 stderr，覆盖 main → config → 日志这条链路。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::process::Command;

/// 一组合法的完整配置（用于 isolate 单个非法项）。
fn valid_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("BACKUP_BIND_ADDR", "127.0.0.1:18080"),
        (
            "BACKUP_DATABASE_URL",
            "postgres://backup:secret-db-password@127.0.0.1:5432/backup",
        ),
        ("BACKUP_STORAGE_KIND", "s3"),
        ("BACKUP_STORAGE_BUCKET", "backup"),
        ("BACKUP_STORAGE_REGION", "us-east-1"),
        ("BACKUP_STORAGE_INTERNAL_ENDPOINT", "http://127.0.0.1:9000"),
        ("BACKUP_STORAGE_PUBLIC_ENDPOINT", "http://127.0.0.1:9000"),
        ("BACKUP_STORAGE_ACCESS_KEY_ID", "access-key-value"),
        ("BACKUP_STORAGE_SECRET_ACCESS_KEY", "secret-key-value"),
        ("BACKUP_STORAGE_ALLOW_HTTP", "true"),
        ("BACKUP_MAIL_MODE", "file"),
    ]
}

#[test]
fn missing_config_exits_non_zero_and_names_the_item() {
    let output = Command::new(env!("CARGO_BIN_EXE_server"))
        .arg("serve")
        // 清空环境，避免开发机上已有的 BACKUP_* 影响结果。
        .env_clear()
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
    let mut command = Command::new(env!("CARGO_BIN_EXE_server"));
    command.arg("serve").env_clear();
    for (key, value) in valid_env() {
        if key != "BACKUP_MAIL_MODE" {
            command.env(key, value);
        }
    }
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
    let mut command = Command::new(env!("CARGO_BIN_EXE_server"));
    command.arg("serve").env_clear();
    for (key, value) in valid_env() {
        command.env(key, value);
    }
    command.env("BACKUP_BIND_ADDR", "this-is-not-a-socket-address");
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
