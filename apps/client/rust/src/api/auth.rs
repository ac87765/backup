//! 认证 API：登录、解锁、会话恢复、保持解锁、退出。
//!
//! 密码只作为函数参数传入 Rust，在 Rust 内完成 Argon2id 派生与主密钥解包；
//! 返回给 Dart 的只有 [`crate::api::types::AccountDto`] 展示数据。

use crate::api::error::BridgeError;
use crate::api::types::{AccountDto, KdfParamsDto};
use crate::convert::account_dto;
use crate::state;

/// 登录前查询服务端 KDF 参数（展示数据，不含盐或密钥）。
pub async fn prelogin(email: String) -> Result<KdfParamsDto, BridgeError> {
    let app = state::app()?;
    let params = app.client.prelogin(&email).await?;
    Ok(params.into())
}

/// 邮箱 + 密码登录；成功后会话令牌写入系统钥匙串。
pub async fn login(email: String, password: String) -> Result<AccountDto, BridgeError> {
    let app = state::app()?;
    let account = app.client.login(&email, &password).await?;
    start_engine_after_auth(&app).await;
    Ok(account_dto(&account, app.client.is_unlocked()))
}

/// 仅用密码解锁（已保存会话）。密码错误返回 `unlock_failed`。
pub async fn unlock(password: String) -> Result<AccountDto, BridgeError> {
    let app = state::app()?;
    let account = app.client.unlock(&password).await?;
    start_engine_after_auth(&app).await;
    Ok(account_dto(&account, app.client.is_unlocked()))
}

/// “在这台电脑上保持解锁”：钥匙串里已有主密钥时直接恢复。
///
/// 返回 `None` 表示没有可用凭据，界面应显示解锁页。
pub async fn unlock_with_stored_master_key() -> Result<Option<AccountDto>, BridgeError> {
    let app = state::app()?;
    let account = app.client.unlock_with_stored_master_key().await?;
    if account.is_some() {
        start_engine_after_auth(&app).await;
    }
    Ok(account.map(|account| account_dto(&account, app.client.is_unlocked())))
}

/// 重新打开应用时用已保存会话恢复展示数据（不输入密码）。
pub async fn restore_session() -> Result<Option<AccountDto>, BridgeError> {
    let app = state::app()?;
    let account = app.client.restore_session().await?;
    if account.is_some() {
        start_engine_after_auth(&app).await;
    }
    Ok(account.map(|account| account_dto(&account, app.client.is_unlocked())))
}

/// 请求 `/auth/me` 刷新账号状态（含 `status` / `is_admin`）。
pub async fn current_account() -> Result<AccountDto, BridgeError> {
    let app = state::app()?;
    let account = app.client.current_account().await?;
    Ok(account_dto(&account, app.client.is_unlocked()))
}

/// 是否已有保存的会话令牌（无需网络）。
pub async fn has_stored_session() -> Result<bool, BridgeError> {
    let app = state::app()?;
    Ok(app.client.has_stored_session()?)
}

/// 当前是否已解锁（内存中有主密钥）。
pub async fn is_unlocked() -> Result<bool, BridgeError> {
    let app = state::app()?;
    Ok(app.client.is_unlocked())
}

/// “在这台电脑上保持解锁”当前是否开启。
pub async fn is_keep_unlocked() -> Result<bool, BridgeError> {
    let app = state::app()?;
    Ok(app.client.keep_unlocked())
}

/// 修改“保持解锁”。关闭时立即删除钥匙串中的主密钥。
pub async fn set_keep_unlocked(enabled: bool) -> Result<(), BridgeError> {
    let app = state::app()?;
    Ok(app.client.set_keep_unlocked(enabled)?)
}

/// 临时锁定：只清除内存主密钥，保留会话令牌与钥匙串内容。
pub async fn lock() -> Result<(), BridgeError> {
    let app = state::app()?;
    Ok(app.client.lock()?)
}

/// 退出登录：撤销服务端会话，并清除本地会话令牌与主密钥；停止备份引擎。
pub async fn logout() -> Result<(), BridgeError> {
    let app = state::app()?;
    app.engine.stop().await;
    Ok(app.client.logout().await?)
}

/// 清除本地全部凭据（修改密码、会话失效时使用）；停止备份引擎。
pub async fn forget_credentials() -> Result<(), BridgeError> {
    let app = state::app()?;
    app.engine.stop().await;
    Ok(app.client.forget_credentials()?)
}

/// 登录/解锁成功后尽力启动备份引擎。
///
/// 待审核账号会返回 `AccountNotWritable`，此时界面不应提供备份入口；账号
/// 变为 active 后由界面调用 `backup_start` 重试。引擎已在运行时返回的错误
/// 也可以安全忽略（启动是幂等的）。
async fn start_engine_after_auth(app: &state::AppState) {
    let _ = app.engine.start().await;
}
