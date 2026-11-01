//! 微信备份桥接 API（add-wechat-backup 第 1–4 组）。
//!
//! 薄层：解析 DTO、调用 `client-core` 的 [`BackupEngine`]，把结果转成展示 DTO。
//! 发现、类别备份、数据库快照与恢复的业务逻辑全部在 `client-core`；本模块不
//! 读取微信文件、不解密、不实现路径规则。
//!
//! 恢复启动前由核心检查微信进程与快照完整性；错误码统一由
//! [`BridgeError`](crate::api::error::BridgeError) 给出（`wechat_running`、
//! `snapshot_incomplete`、`snapshot_corrupted` 等）。

use std::path::PathBuf;

use crate::api::error::BridgeError;
use crate::api::types::{
    WechatAccountBackupStatusDto, WechatAccountDto, WechatAccountRefDto, WechatBackupPreflightDto,
    WechatCategoryDto, WechatDatabaseSnapshotDto, WechatRestoreCategoryDto,
    WechatRestoreRequestDto, WechatSnapshotReportDto,
};
use crate::convert::{
    wechat_account, wechat_account_dto, wechat_categories, wechat_restore_request,
};
use crate::state::{self, AppState, TransferTask};

/// 发现本机微信账号（展示字段 + 本地备注名）。
pub async fn wechat_list_accounts() -> Result<Vec<WechatAccountDto>, BridgeError> {
    let app = state::app()?;
    let accounts = app.engine.discover_wechat_accounts().await?;
    Ok(accounts.iter().map(wechat_account_dto).collect())
}

/// 手动添加微信数据目录；返回重新发现后的账号列表。
pub async fn wechat_add_manual_directory(
    path: String,
) -> Result<Vec<WechatAccountDto>, BridgeError> {
    let app = state::app()?;
    app.engine
        .add_wechat_manual_directory(PathBuf::from(path))
        .await?;
    let accounts = app.engine.discover_wechat_accounts().await?;
    Ok(accounts.iter().map(wechat_account_dto).collect())
}

/// 移除手动添加的微信数据目录；返回重新发现后的账号列表。
pub async fn wechat_remove_manual_directory(
    path: String,
) -> Result<Vec<WechatAccountDto>, BridgeError> {
    let app = state::app()?;
    app.engine
        .remove_wechat_manual_directory(&PathBuf::from(path))
        .await?;
    let accounts = app.engine.discover_wechat_accounts().await?;
    Ok(accounts.iter().map(wechat_account_dto).collect())
}

/// 当前手动添加的微信数据目录（供界面回显）。
pub async fn wechat_manual_directories() -> Result<Vec<String>, BridgeError> {
    let app = state::app()?;
    app.engine.ensure_wechat_manual_directories_loaded().await;
    Ok(app
        .engine
        .wechat_manual_directories()
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect())
}

/// 设置账号备注名（只存本地）。
pub async fn wechat_set_note(
    account: WechatAccountRefDto,
    note: String,
) -> Result<(), BridgeError> {
    let app = state::app()?;
    let account = wechat_account(account);
    Ok(app.engine.set_wechat_account_note(&account, &note).await?)
}

/// 清除账号备注名。
pub async fn wechat_clear_note(account: WechatAccountRefDto) -> Result<(), BridgeError> {
    let app = state::app()?;
    let account = wechat_account(account);
    Ok(app.engine.clear_wechat_account_note(&account).await?)
}

/// 设置一个账号勾选的类别（design D2）。
pub async fn wechat_set_categories(
    account: WechatAccountRefDto,
    categories: Vec<WechatCategoryDto>,
) -> Result<(), BridgeError> {
    let app = state::app()?;
    let account = wechat_account(account);
    let categories = wechat_categories(categories);
    Ok(app
        .engine
        .set_wechat_categories(&account, &categories)
        .await?)
}

/// 微信备份首次上传预检（文件数、明文/密文字节数、剩余配额）。
pub async fn wechat_preflight(
    account: WechatAccountRefDto,
    categories: Vec<WechatCategoryDto>,
) -> Result<WechatBackupPreflightDto, BridgeError> {
    let app = state::app()?;
    let account = wechat_account(account);
    let categories = wechat_categories(categories);
    let preflight = app.engine.preflight_wechat(&account, &categories).await?;
    Ok((&preflight).into())
}

/// 每个微信账号每个类别的备份状态。
pub async fn wechat_account_statuses() -> Result<Vec<WechatAccountBackupStatusDto>, BridgeError> {
    let app = state::app()?;
    let statuses = app.engine.list_wechat_accounts().await?;
    Ok(statuses.iter().map(Into::into).collect())
}

/// 立即为一个账号做数据库快照（微信运行时返回 `waiting_for_wechat_exit`）。
pub async fn wechat_snapshot_now(
    account: WechatAccountRefDto,
) -> Result<WechatSnapshotReportDto, BridgeError> {
    let app = state::app()?;
    let account = wechat_account(account);
    let report = app.engine.snapshot_wechat_databases_now(&account).await?;
    Ok((&report).into())
}

/// 列出某账号可用的完整数据库快照（只返回带完成标记的）。
pub async fn wechat_database_snapshots(
    account: WechatAccountRefDto,
) -> Result<Vec<WechatDatabaseSnapshotDto>, BridgeError> {
    let app = state::app()?;
    let snapshots = app
        .engine
        .wechat_database_snapshots(&account.account_name)
        .await?;
    Ok(snapshots.iter().map(Into::into).collect())
}

/// 列出云端存在哪些文件类别可恢复（含大小）。
pub async fn wechat_restore_categories(
    account: WechatAccountRefDto,
) -> Result<Vec<WechatRestoreCategoryDto>, BridgeError> {
    let app = state::app()?;
    let account = wechat_account(account);
    let categories = app.engine.wechat_restore_categories(&account).await?;
    Ok(categories.iter().map(Into::into).collect())
}

/// 开始恢复；返回传输 ID，进度通过 `transfer_progress_stream` 推送。
///
/// 微信运行中、快照不完整或不存在时直接返回错误，不创建任务、不写任何文件。
pub async fn wechat_start_restore(request: WechatRestoreRequestDto) -> Result<String, BridgeError> {
    let app = state::app()?;
    let request = wechat_restore_request(request)?;
    let task = app.engine.restore_wechat(request).await?;
    register_restore(&app, task)
}

fn register_restore(
    app: &std::sync::Arc<AppState>,
    task: client_core::desktop::WechatRestoreTask,
) -> Result<String, BridgeError> {
    let receiver = task.subscribe();
    let transfer = TransferTask::WechatRestore(task);
    let id = transfer.id();
    {
        let mut registry = app.registry()?;
        registry.insert(transfer.clone());
    }
    state::forward_transfer_progress(app.clone(), receiver);
    state::spawn_transfer(app.clone(), transfer);
    Ok(id)
}
