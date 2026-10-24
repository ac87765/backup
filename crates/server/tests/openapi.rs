//! OpenAPI 子命令测试（tasks 4.7）：不需要配置或数据库，stdout 必须是纯 JSON 规范。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::server_command;

#[test]
fn openapi_command_prints_spec_without_configuration() {
    // 清空环境：openapi 子命令不应依赖任何 BACKUP_* 配置。
    let output = server_command()
        .arg("openapi")
        .output()
        .expect("启动 server 二进制");

    assert!(
        output.status.success(),
        "openapi 应当成功，stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let spec: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout 必须是合法 JSON 规范");
    assert!(
        spec["openapi"]
            .as_str()
            .is_some_and(|version| version.starts_with("3.")),
        "必须是 OpenAPI 3.x"
    );
    assert!(spec["paths"]["/healthz"].is_object());
    assert!(spec["paths"]["/readyz"].is_object());
    assert!(spec["components"]["schemas"]["ErrorBody"].is_object());
    assert!(spec["components"]["schemas"]["ReadyResponse"].is_object());

    // stdout 只能有规范本身，不能混入日志。
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("BACKUP_"),
        "规范输出中不应出现配置项名称"
    );
}
