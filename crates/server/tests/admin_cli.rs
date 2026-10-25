//! `server admin grant|revoke|list` 集成测试（add-admin-console task 1.2）。
//!
//! 命令行只依赖数据库；授予/撤销与审计记录同事务，重复操作幂等且不重复审计。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{apply_migrate_env, environment, insert_test_user, migrated_database, server_command};
use uuid::Uuid;

fn run_admin(database_url: &str, args: &[&str]) -> std::process::Output {
    let mut command = server_command();
    command.arg("admin").args(args);
    apply_migrate_env(&mut command, database_url);
    command.output().expect("启动 server admin")
}

async fn audit_count(database: &server::Database, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM admin_audit_log WHERE action = $1")
        .bind(action)
        .fetch_one(database.pool())
        .await
        .expect("统计审计记录")
}

#[tokio::test]
async fn grant_revoke_and_list_are_audited_and_idempotent() {
    let env = environment().await;
    let (test_db, database) = migrated_database(env).await;
    let user_id = Uuid::new_v4();
    insert_test_user(&database, user_id).await;
    let email = format!("test-{user_id}@example.com");

    // 授予管理员：is_admin 变为 true，并写入一条 via=cli 的审计。
    let output = run_admin(&test_db.url, &["grant", &email]);
    assert!(
        output.status.success(),
        "grant 应当成功，stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(database.pool())
        .await
        .expect("读取 is_admin");
    assert!(is_admin);
    let audit = sqlx::query!(
        r#"
        SELECT admin_id, action, target_user_id, details, ip
        FROM admin_audit_log
        ORDER BY id DESC
        LIMIT 1
        "#
    )
    .fetch_one(database.pool())
    .await
    .expect("读取审计记录");
    assert_eq!(audit.admin_id, Uuid::nil());
    assert_eq!(audit.action, "grant_admin");
    assert_eq!(audit.target_user_id, Some(user_id));
    assert_eq!(audit.details["via"], serde_json::json!("cli"));
    assert_eq!(audit.ip, "cli");
    assert_eq!(audit_count(&database, "grant_admin").await, 1);

    // 重复授予幂等：不重复写审计。
    let output = run_admin(&test_db.url, &["grant", &email]);
    assert!(output.status.success());
    assert_eq!(audit_count(&database, "grant_admin").await, 1);

    // list 只输出管理员邮箱。
    let output = run_admin(&test_db.url, &["list"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&email),
        "list 输出应包含管理员邮箱: {stdout}"
    );

    // 撤销管理员：is_admin 变回 false，并追加一条 revoke 审计。
    let output = run_admin(&test_db.url, &["revoke", &email]);
    assert!(
        output.status.success(),
        "revoke 应当成功，stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let is_admin: bool = sqlx::query_scalar("SELECT is_admin FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(database.pool())
        .await
        .expect("读取 is_admin");
    assert!(!is_admin);
    assert_eq!(audit_count(&database, "revoke_admin").await, 1);

    // 重复撤销幂等。
    let output = run_admin(&test_db.url, &["revoke", &email]);
    assert!(output.status.success());
    assert_eq!(audit_count(&database, "revoke_admin").await, 1);
}

#[tokio::test]
async fn granting_unknown_email_fails_without_writes() {
    let env = environment().await;
    let (test_db, database) = migrated_database(env).await;

    let output = run_admin(&test_db.url, &["grant", "missing@example.com"]);
    assert!(!output.status.success(), "未知邮箱必须失败");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit_log")
        .fetch_one(database.pool())
        .await
        .expect("统计审计记录");
    assert_eq!(count, 0);
}
