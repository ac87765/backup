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
    assert!(spec["paths"]["/api/v1/auth/prelogin"].is_object());
    assert!(spec["components"]["schemas"]["ErrorBody"].is_object());
    assert!(spec["components"]["schemas"]["ReadyResponse"].is_object());

    // stdout 只能有规范本身，不能混入日志。
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("BACKUP_"),
        "规范输出中不应出现配置项名称"
    );
}

/// 每个 `/api` 业务路由的每个操作都必须声明至少一个 `ErrorBody` 错误响应。
#[test]
fn every_api_operation_declares_an_error_body_response() {
    let spec = serde_json::to_value(server::openapi::spec()).expect("规范可序列化");
    let paths = spec["paths"].as_object().expect("paths 对象");
    let mut operation_count = 0;

    for (path, item) in paths {
        if !path.starts_with("/api") {
            continue;
        }
        let operations = item.as_object().expect("路径项必须是对象");
        let mut path_operations = 0;
        for (method, operation) in operations {
            if !matches!(
                method.as_str(),
                "get" | "post" | "put" | "patch" | "delete" | "head" | "options" | "trace"
            ) {
                continue;
            }
            path_operations += 1;
            operation_count += 1;
            let responses = operation["responses"].as_object().expect("responses 对象");
            let has_error_body = responses.iter().any(|(status, response)| {
                !status.starts_with('2')
                    && response["content"]["application/json"]["schema"]["$ref"]
                        .as_str()
                        .is_some_and(|reference| reference.ends_with("/ErrorBody"))
            });
            assert!(
                has_error_body,
                "{method} {path} 必须声明以 ErrorBody 表示的错误响应"
            );
        }
        assert!(path_operations > 0, "{path} 必须至少声明一个操作");
    }

    assert!(
        operation_count >= 12,
        "至少应覆盖 12 个账号 API 操作，实际 {operation_count}"
    );
}
