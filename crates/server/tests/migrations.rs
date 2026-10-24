//! 数据库迁移集成测试（tasks 4.3）。
//!
//! 覆盖 service-operations spec「未执行迁移即启动」场景，以及
//! 落后 / 校验和不一致 / 数据库领先 三种启动判定。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use common::{
    TestDb, apply_migrate_env, environment, integration_serve_env, server_command, static_fixture,
};
use server::db::{Database, MigrationState};

#[tokio::test]
async fn fresh_database_is_behind_and_refuses_to_serve() {
    let env = environment().await;
    let test_db = TestDb::create(env).await;
    let pool = test_db.connect().await;
    let database = Database::from_pool(pool);

    let state = database.migration_state().await.expect("读取迁移状态");
    let mut expected: Vec<i64> = server::db::MIGRATOR
        .migrations
        .iter()
        .map(|migration| migration.version)
        .collect();
    expected.sort_unstable();
    assert_eq!(state, MigrationState::Behind { missing: expected });
    assert!(!state.is_ready());
}

#[tokio::test]
async fn migrate_command_applies_migrations_and_is_idempotent() {
    let env = environment().await;
    let test_db = TestDb::create(env).await;
    let pool = test_db.connect().await;
    let database = Database::from_pool(pool.clone());

    database.migrate().await.expect("第一次迁移应当成功");
    database.migrate().await.expect("重复迁移应当幂等");

    let state = database.migration_state().await.expect("读取迁移状态");
    assert_eq!(
        state,
        MigrationState::Ready {
            database_ahead: vec![]
        }
    );

    // 最小迁移确实创建了 citext 扩展（证明迁移内容被执行）。
    let extension_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_extension WHERE extname = 'citext'")
            .fetch_one(&pool)
            .await
            .expect("查询扩展");
    assert_eq!(extension_count, 1);
}

#[tokio::test]
async fn checksum_mismatch_is_rejected() {
    let env = environment().await;
    let test_db = TestDb::create(env).await;
    let pool = test_db.connect().await;
    let database = Database::from_pool(pool.clone());
    database.migrate().await.expect("迁移应当成功");

    sqlx::query("UPDATE _sqlx_migrations SET checksum = $1 WHERE version = 1")
        .bind(vec![0x42_u8; 32])
        .execute(&pool)
        .await
        .expect("篡改校验和");

    let state = database.migration_state().await.expect("读取迁移状态");
    assert_eq!(
        state,
        MigrationState::ChecksumMismatch { versions: vec![1] }
    );
    assert!(!state.is_ready());
}

#[tokio::test]
async fn database_ahead_of_code_is_allowed() {
    let env = environment().await;
    let test_db = TestDb::create(env).await;
    let pool = test_db.connect().await;
    let database = Database::from_pool(pool.clone());
    database.migrate().await.expect("迁移应当成功");

    // 模拟“新版本代码先发布、旧版本实例回滚”的场景：数据库里有当前代码不认识的迁移版本。
    sqlx::query(
        "INSERT INTO _sqlx_migrations \
         (version, description, success, checksum, execution_time) \
         VALUES ($1, $2, true, $3, 0)",
    )
    .bind(999_999_i64)
    .bind("future_migration")
    .bind(vec![0x7f_u8; 32])
    .execute(&pool)
    .await
    .expect("插入未来迁移版本");

    let state = database.migration_state().await.expect("读取迁移状态");
    assert_eq!(
        state,
        MigrationState::Ready {
            database_ahead: vec![999_999]
        }
    );
    assert!(state.is_ready(), "数据库领先必须允许启动");
}

#[tokio::test]
async fn serve_binary_refuses_to_start_before_migration() {
    let env = environment().await;
    let test_db = TestDb::create(env).await;
    let (_dir, static_dir) = static_fixture();

    let serve_env = integration_serve_env(env, &test_db.url, &static_dir);
    let mut command = server_command();
    command.arg("serve");
    serve_env.apply(&mut command);
    let output = command.output().expect("启动 server 二进制");

    assert!(
        !output.status.success(),
        "未迁移的数据库必须导致 serve 非零退出"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("缺少迁移版本") && stderr.contains("server migrate"),
        "错误日志必须说明迁移落后，实际 stderr: {stderr}"
    );
}

#[tokio::test]
async fn migrate_binary_applies_migrations() {
    let env = environment().await;
    let test_db = TestDb::create(env).await;

    let mut command = server_command();
    command.arg("migrate");
    apply_migrate_env(&mut command, &test_db.url);
    let output = command.output().expect("启动 server 二进制");

    assert!(
        output.status.success(),
        "migrate 应当成功，stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let pool = test_db.connect().await;
    let database = Database::from_pool(pool);
    let state = database.migration_state().await.expect("读取状态");
    assert!(state.is_ready(), "migrate 后应当 ready");
}
