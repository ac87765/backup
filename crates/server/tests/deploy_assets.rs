//! 部署资产的自检：生产环境变量模板、RAM 策略、CORS、生命周期与镜像约束。
//!
//! 这些测试不需要 Docker/阿里云，随 `cargo test -p server` 运行，防止 9.1/9.2 的
//! 模板在后续修改中漏项或退化成过宽权限。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use server::config::keys;

const ENV_TEMPLATE: &str = include_str!("../../../deploy/oss/production.env.example");
const RAM_POLICY: &str = include_str!("../../../deploy/oss/ram-policy.json");
const CORS: &str = include_str!("../../../deploy/oss/cors.xml");
const LIFECYCLE: &str = include_str!("../../../deploy/oss/lifecycle.xml");
const DOCKERFILE: &str = include_str!("../../../Dockerfile");
const RUST_TOOLCHAIN: &str = include_str!("../../../rust-toolchain.toml");
const NVMRC: &str = include_str!("../../../.nvmrc");
const DOCKERIGNORE: &str = include_str!("../../../.dockerignore");

/// 所有 `BACKUP_*` 配置项（新增配置时必须同步更新生产模板与清单）。
const ALL_CONFIG_KEYS: &[&str] = &[
    keys::BIND_ADDR,
    keys::METRICS_ADDR,
    keys::STATIC_DIR,
    keys::DATABASE_URL,
    keys::DATABASE_MAX_CONNECTIONS,
    keys::STORAGE_KIND,
    keys::STORAGE_BUCKET,
    keys::STORAGE_REGION,
    keys::STORAGE_INTERNAL_ENDPOINT,
    keys::STORAGE_PUBLIC_ENDPOINT,
    keys::STORAGE_ACCESS_KEY_ID,
    keys::STORAGE_SECRET_ACCESS_KEY,
    keys::STORAGE_ALLOW_HTTP,
    keys::JOBS_ENABLED,
    keys::MAIL_MODE,
    keys::MAIL_DIR,
    keys::SMTP_HOST,
    keys::SMTP_PORT,
    keys::SMTP_FROM,
    keys::SMTP_USERNAME,
    keys::SMTP_PASSWORD,
    keys::SMTP_TLS,
    keys::SMTP_ALLOW_INSECURE,
    keys::AUTH_PEPPER_VERSION,
    keys::AUTH_PEPPER,
    keys::AUTH_OLD_PEPPERS,
    keys::AUTH_FAKE_SALT_KEY,
    keys::PUBLIC_APP_URL,
    keys::TRUSTED_PROXIES,
    keys::AUTH_RATE_LIMIT_PER_MINUTE,
    keys::USER_WRITE_RATE_LIMIT_PER_MINUTE,
];

#[test]
fn production_env_template_contains_every_backup_key() {
    for key in ALL_CONFIG_KEYS {
        assert!(
            ENV_TEMPLATE.contains(&format!("{key}=")),
            "deploy/oss/production.env.example 缺少配置项 {key}"
        );
    }
}

#[test]
fn ram_policy_is_scoped_to_v1_prefix() {
    let policy: serde_json::Value =
        serde_json::from_str(RAM_POLICY).expect("RAM 策略必须是合法 JSON");
    let statements = policy["Statement"]
        .as_array()
        .expect("RAM 策略必须包含 Statement 数组");
    assert!(!statements.is_empty());

    let mut actions = Vec::new();
    for statement in statements {
        assert_eq!(statement["Effect"], "Allow");
        let resources = statement["Resource"]
            .as_array()
            .expect("每条 statement 必须有 Resource");
        assert!(!resources.is_empty());
        for resource in resources {
            let resource = resource.as_str().expect("Resource 必须是字符串");
            assert!(
                resource.starts_with("acs:oss:"),
                "Resource 必须是 OSS ARN：{resource}"
            );
            assert!(
                resource.contains("<BUCKET_NAME>"),
                "Resource 必须使用 bucket 占位符：{resource}"
            );
            assert!(
                resource.contains("/v1/*") || resource.ends_with("<BUCKET_NAME>"),
                "Resource 必须限制在 bucket 或 v1/ 前缀：{resource}"
            );
        }
        for action in statement["Action"].as_array().expect("Action 必须是数组") {
            actions.push(action.as_str().expect("Action 必须是字符串"));
        }
    }
    actions.sort_unstable();
    assert_eq!(
        actions,
        vec![
            "oss:DeleteObject",
            "oss:GetObject",
            "oss:GetObjectMeta",
            "oss:ListObjects",
            "oss:PutObject",
        ]
    );
    assert!(!RAM_POLICY.contains("\"oss:*\""), "禁止使用 oss:* 通配权限");
}

#[test]
fn cors_is_limited_to_frontend_origin_and_safe_methods() {
    assert!(CORS.contains("<AllowedOrigin>https://backup.example.com</AllowedOrigin>"));
    assert!(!CORS.contains("<AllowedOrigin>*</AllowedOrigin>"));
    assert!(!CORS.contains("<AllowedMethod>DELETE</AllowedMethod>"));
    assert!(!CORS.contains("<AllowedMethod>POST</AllowedMethod>"));
    assert!(CORS.contains("<AllowedMethod>PUT</AllowedMethod>"));
    assert!(CORS.contains("<AllowedMethod>GET</AllowedMethod>"));
    assert!(CORS.contains("<AllowedMethod>HEAD</AllowedMethod>"));
    assert!(CORS.contains("<ExposeHeader>ETag</ExposeHeader>"));
}

#[test]
fn lifecycle_only_aborts_incomplete_multipart_uploads() {
    assert!(LIFECYCLE.contains("<AbortMultipartUpload>"));
    assert!(LIFECYCLE.contains("<Prefix>v1/</Prefix>"));
    assert!(!LIFECYCLE.contains("<Expiration>"));
    assert!(!LIFECYCLE.contains("<NoncurrentVersionExpiration>"));
}

#[test]
fn dockerfile_pins_external_bases_and_runs_non_root() {
    let external_bases: Vec<&str> = DOCKERFILE
        .lines()
        .filter(|line| {
            line.starts_with("FROM rust:")
                || line.starts_with("FROM node:")
                || line.starts_with("FROM debian:")
        })
        .collect();
    assert_eq!(
        external_bases.len(),
        4,
        "应有四个外部基础镜像（rust ×2 / node / debian）"
    );
    for line in external_bases {
        assert!(
            line.contains("@sha256:"),
            "基础镜像必须按 digest 固定：{line}"
        );
    }
    assert!(DOCKERFILE.contains("AS planner"));
    assert!(DOCKERFILE.contains("AS builder"));
    assert!(DOCKERFILE.contains("AS wasm-builder"));
    assert!(DOCKERFILE.contains("AS web-builder"));
    assert!(DOCKERFILE.contains("AS runtime"));
    assert!(DOCKERFILE.contains("USER 10001:10001"));
    assert!(DOCKERFILE.contains("apt-get install -y --no-install-recommends ca-certificates"));
}

#[test]
fn dockerfile_tool_versions_match_ci_pins() {
    let rust_channel = RUST_TOOLCHAIN
        .lines()
        .find_map(|line| line.trim().strip_prefix("channel = "))
        .map(|value| value.trim().trim_matches('"'))
        .expect("rust-toolchain.toml 必须有 channel");
    assert!(
        DOCKERFILE.contains(&format!("FROM rust:{rust_channel}-bookworm@sha256:")),
        "Dockerfile 的 Rust 版本必须与 rust-toolchain.toml 一致"
    );

    let node_version = NVMRC.trim();
    assert!(
        DOCKERFILE.contains(&format!("FROM node:{node_version}-bookworm-slim@sha256:")),
        "Dockerfile 的 Node 版本必须与 .nvmrc 一致"
    );

    assert!(
        DOCKERFILE.contains("wasm-pack/releases/download/v0.15.0/"),
        "Dockerfile 的 wasm-pack 版本必须与 CI 一致（0.15.0）"
    );
}

#[test]
fn dockerignore_excludes_large_and_generated_paths() {
    for required in [
        "target",
        "web/node_modules",
        ".git",
        "web/src/generated",
        "web/e2e-tmp",
        "deploy/e2e-out",
    ] {
        assert!(
            DOCKERIGNORE.lines().any(|line| line.trim() == required),
            ".dockerignore 必须排除 {required}"
        );
    }
}
