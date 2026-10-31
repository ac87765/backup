//! 认证集成测试：真实 PostgreSQL + MinIO + 进程内服务端。
//!
//! 覆盖 native-clients spec 的三个核心 Scenario（重新打开应用、待审核账号、
//! 退出登录清除凭据），以及登录成功、凭据错误不可区分、会话失效清除存储。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;

use client_core::{
    AccountStatus, ApiErrorCode, Client, ClientError, MemorySecretStore, SecretStore,
};
use common::{
    active_session_count, create_user, environment, latest_user_agent, revoke_sessions,
    start_server,
};
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";

#[tokio::test]
async fn login_success_persists_session_and_sends_client_headers() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let store = Arc::new(MemorySecretStore::new());
    let client = Client::new(&server.base_url, store.clone()).unwrap();

    let account = client.login(&user.email, &user.password).await.unwrap();

    assert_eq!(account.user_id, user.user_id);
    assert_eq!(account.email, user.email);
    assert_eq!(account.root_node_id, user.root_node_id);
    assert_eq!(account.status, AccountStatus::Active);
    assert!(account.writes_allowed());
    assert!(account.email_verified);
    assert!(!account.is_admin);

    assert!(
        store.load_session().unwrap().is_some(),
        "登录成功后会话令牌必须写入凭据存储"
    );
    assert!(
        store.load_master_key().unwrap().is_none(),
        "默认不持久化主密钥"
    );
    assert!(client.is_unlocked());

    let user_agent = latest_user_agent(&server.database, user.user_id)
        .await
        .expect("登录必须写入会话 User-Agent");
    assert!(
        user_agent.starts_with("EncryptedBackup/"),
        "User-Agent 必须标识客户端：{user_agent}"
    );
    assert!(
        user_agent.ends_with("; desktop)"),
        "User-Agent 必须带 desktop 标记：{user_agent}"
    );

    let refreshed = client.current_account().await.unwrap();
    assert_eq!(refreshed.email, user.email);
    assert_eq!(refreshed.status, AccountStatus::Active);
}

#[tokio::test]
async fn admin_flag_is_reported_by_me() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", true, PASSWORD).await;
    let client = Client::new(&server.base_url, Arc::new(MemorySecretStore::new())).unwrap();

    let account = client.login(&user.email, &user.password).await.unwrap();
    assert!(account.is_admin, "/auth/me 必须返回 is_admin");
}

#[tokio::test]
async fn wrong_password_and_unknown_email_share_the_same_error() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let store = Arc::new(MemorySecretStore::new());
    let client = Client::new(&server.base_url, store.clone()).unwrap();

    let wrong_password = client
        .login(&user.email, "not-the-password")
        .await
        .unwrap_err();
    let unknown_email = client
        .login(
            &format!("nobody-{}@example.com", Uuid::new_v4()),
            "not-the-password",
        )
        .await
        .unwrap_err();

    assert_eq!(
        wrong_password.api_code(),
        Some(&ApiErrorCode::InvalidCredentials)
    );
    assert_eq!(
        unknown_email.api_code(),
        Some(&ApiErrorCode::InvalidCredentials)
    );
    assert_eq!(wrong_password.status(), Some(401));
    assert_eq!(unknown_email.status(), Some(401));
    assert_eq!(
        wrong_password.to_string(),
        unknown_email.to_string(),
        "错误密码与未知邮箱必须返回同一条消息"
    );
    assert!(store.load_session().unwrap().is_none());
    assert!(!client.is_unlocked());
}

#[tokio::test]
async fn pending_account_login_reports_pending_and_no_write_access() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "pending", false, PASSWORD).await;
    let client = Client::new(&server.base_url, Arc::new(MemorySecretStore::new())).unwrap();

    let account = client.login(&user.email, &user.password).await.unwrap();

    assert_eq!(account.status, AccountStatus::Pending);
    assert!(
        !account.writes_allowed(),
        "待审核账号在核心层必须被标记为不可写"
    );
    assert!(!account.is_admin);
}

#[tokio::test]
async fn invalid_session_clears_session_and_master_key() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let store = Arc::new(MemorySecretStore::new());
    let client = Client::builder(&server.base_url)
        .secret_store(store.clone())
        .keep_unlocked(true)
        .build()
        .unwrap();

    client.login(&user.email, &user.password).await.unwrap();
    assert!(store.load_session().unwrap().is_some());
    assert!(
        store.load_master_key().unwrap().is_some(),
        "开启保持解锁后主密钥应写入凭据存储"
    );

    // 模拟管理员撤销会话（如停用账号）。
    revoke_sessions(&server.database, user.user_id).await;

    let error = client.current_account().await.unwrap_err();
    assert!(error.is_unauthenticated(), "撤销后必须返回 401：{error:?}");
    assert!(
        store.load_session().unwrap().is_none(),
        "会话失效必须清除会话令牌"
    );
    assert!(
        store.load_master_key().unwrap().is_none(),
        "会话失效必须清除主密钥"
    );
    assert!(!client.is_unlocked());
    assert!(client.account().is_none());

    assert!(matches!(
        client.current_account().await,
        Err(ClientError::NoSession)
    ));
}

#[tokio::test]
async fn logout_clears_credentials_and_revokes_server_session() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let store = Arc::new(MemorySecretStore::new());
    let client = Client::builder(&server.base_url)
        .secret_store(store.clone())
        .keep_unlocked(true)
        .build()
        .unwrap();

    client.login(&user.email, &user.password).await.unwrap();
    assert_eq!(
        active_session_count(&server.database, user.user_id).await,
        1
    );

    client.logout().await.unwrap();

    assert_eq!(
        active_session_count(&server.database, user.user_id).await,
        0,
        "服务端会话必须被撤销"
    );
    assert!(store.load_session().unwrap().is_none());
    assert!(store.load_master_key().unwrap().is_none());
    assert!(!client.is_unlocked());
    assert!(client.account().is_none());
    assert!(matches!(client.restore_session().await, Ok(None)));
}

#[tokio::test]
async fn reopen_with_stored_session_unlocks_with_password_only() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let store = Arc::new(MemorySecretStore::new());

    // 第一次打开应用：登录。默认不持久化主密钥。
    let first = Client::new(&server.base_url, store.clone()).unwrap();
    let login_account = first.login(&user.email, &user.password).await.unwrap();
    assert_eq!(login_account.email, user.email);
    drop(first);

    // 关闭并重新打开：新的 client 实例 + 已存会话令牌。
    let reopened = Client::new(&server.base_url, store.clone()).unwrap();
    let restored = reopened
        .restore_session()
        .await
        .unwrap()
        .expect("未开启保持解锁时应能用会话令牌恢复展示数据");
    assert_eq!(restored.email, user.email, "解锁页必须显示已存账号邮箱");
    assert!(!reopened.is_unlocked(), "尚未输入密码时不能有主密钥");

    // 只输入密码，不需要重新输入邮箱。
    let unlocked = reopened.unlock(&user.password).await.unwrap();
    assert_eq!(unlocked.user_id, user.user_id);
    assert!(reopened.is_unlocked());
    assert!(store.load_session().unwrap().is_some());

    // 错误密码：解包失败，提示密码错误；会话与主密钥都不应被误删。
    let reopened_again = Client::new(&server.base_url, store.clone()).unwrap();
    let error = reopened_again
        .unlock("this is not the password")
        .await
        .unwrap_err();
    assert!(
        matches!(error, ClientError::UnlockFailed),
        "错误密码必须映射为 UnlockFailed：{error:?}"
    );
    assert!(!reopened_again.is_unlocked());
    assert!(
        store.load_session().unwrap().is_some(),
        "解锁失败不应删除会话令牌"
    );
}
