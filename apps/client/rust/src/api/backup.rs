//! 备份引擎 API（design D6）：文件夹配置、预检、暂停/恢复、立即备份、设置、
//! 状态/汇总/最近活动与事件流。

use client_core::desktop::{
    BackupActivity, BackupError, BackupPreflight, BackupRunReport, BackupSettings, BackupSummary,
};

use crate::api::error::BridgeError;
use crate::api::types::{
    BackupActivityDto, BackupEventDto, BackupFolderDraftDto, BackupFolderDto, BackupFolderStateDto,
    BackupFolderUpdateDto, BackupPreflightDto, BackupRunReportDto, BackupSettingsDto,
    BackupSummaryDto,
};
use crate::convert::{
    backup_folder_draft, backup_folder_update, backup_preflight as preflight_dto,
    backup_run_report, backup_settings as settings_dto, parse_uuid, u32_from_i64,
};
use crate::frb_generated::StreamSink;
use crate::state;

/// 启动调度器：首次启动全量核对，之后监听文件变化并每 6 小时全量核对。
/// 已在运行时视为成功（幂等）。
pub async fn backup_start() -> Result<(), BridgeError> {
    let app = state::app()?;
    match app.engine.start().await {
        Ok(()) | Err(BackupError::AlreadyRunning) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// 停止调度器与文件监听；已上传内容与本地索引保持不变。
pub async fn backup_stop() -> Result<(), BridgeError> {
    let app = state::app()?;
    app.engine.stop().await;
    Ok(())
}

/// 当前用户的备份文件夹列表。
pub async fn backup_list_folders() -> Result<Vec<BackupFolderDto>, BridgeError> {
    let app = state::app()?;
    let folders = app.engine.list_folders().await?;
    Ok(folders.iter().map(BackupFolderDto::from).collect())
}

/// 读取一个备份文件夹。
pub async fn backup_folder(folder_id: String) -> Result<BackupFolderDto, BridgeError> {
    let app = state::app()?;
    let folder_id = parse_uuid(&folder_id, "备份文件夹")?;
    let folder = app.engine.folder(folder_id).await?;
    Ok((&folder).into())
}

/// 添加备份文件夹（不会立即上传；由调度器或“立即备份”触发）。
pub async fn backup_add_folder(
    draft: BackupFolderDraftDto,
) -> Result<BackupFolderDto, BridgeError> {
    let app = state::app()?;
    let draft = backup_folder_draft(draft)?;
    let folder = app.engine.add_folder(draft).await?;
    Ok((&folder).into())
}

/// 修改备份文件夹（本地路径、云端目标、排除规则、启用/暂停）。
pub async fn backup_update_folder(
    folder_id: String,
    update: BackupFolderUpdateDto,
) -> Result<BackupFolderDto, BridgeError> {
    let app = state::app()?;
    let folder_id = parse_uuid(&folder_id, "备份文件夹")?;
    let update = backup_folder_update(update)?;
    let folder = app.engine.update_folder(folder_id, update).await?;
    Ok((&folder).into())
}

/// 移除备份文件夹：只删除本机配置与索引，云端内容保留。
pub async fn backup_remove_folder(folder_id: String) -> Result<(), BridgeError> {
    let app = state::app()?;
    let folder_id = parse_uuid(&folder_id, "备份文件夹")?;
    Ok(app.engine.remove_folder(folder_id).await?)
}

/// 添加前预检：文件数、首次需上传字节数与剩余配额。
pub async fn backup_preflight(
    draft: BackupFolderDraftDto,
) -> Result<BackupPreflightDto, BridgeError> {
    let app = state::app()?;
    let draft = backup_folder_draft(draft)?;
    let preflight: BackupPreflight = app.engine.preflight(&draft).await?;
    Ok(preflight_dto(&preflight))
}

/// 暂停全部备份（保留已上传内容；恢复后继续）。
pub async fn backup_pause_all() -> Result<(), BridgeError> {
    let app = state::app()?;
    Ok(app.engine.pause_all().await?)
}

/// 恢复全部备份。
pub async fn backup_resume_all() -> Result<(), BridgeError> {
    let app = state::app()?;
    Ok(app.engine.resume_all().await?)
}

/// 暂停单个备份文件夹。
pub async fn backup_pause_folder(folder_id: String) -> Result<(), BridgeError> {
    let app = state::app()?;
    let folder_id = parse_uuid(&folder_id, "备份文件夹")?;
    Ok(app.engine.pause_folder(folder_id).await?)
}

/// 恢复单个备份文件夹。
pub async fn backup_resume_folder(folder_id: String) -> Result<(), BridgeError> {
    let app = state::app()?;
    let folder_id = parse_uuid(&folder_id, "备份文件夹")?;
    Ok(app.engine.resume_folder(folder_id).await?)
}

/// 全局备份设置（含隐藏文件、上传限速、全局暂停）。
pub async fn backup_settings() -> Result<BackupSettingsDto, BridgeError> {
    let app = state::app()?;
    let settings: BackupSettings = app.engine.settings().await?;
    Ok((&settings).into())
}

/// 保存全局备份设置。
pub async fn backup_set_settings(settings: BackupSettingsDto) -> Result<(), BridgeError> {
    let app = state::app()?;
    let settings = settings_dto(settings)?;
    Ok(app.engine.set_settings(settings).await?)
}

/// 最近一次运行状态快照（按文件夹），供界面初次渲染。
pub async fn backup_folder_states() -> Result<Vec<BackupFolderStateDto>, BridgeError> {
    let app = state::app()?;
    let states = app.engine.folder_states();
    Ok(states.iter().map(BackupFolderStateDto::from).collect())
}

/// 全局汇总（托盘/状态栏用）。
pub async fn backup_summary() -> Result<BackupSummaryDto, BridgeError> {
    let app = state::app()?;
    let summary: BackupSummary = app.engine.summary();
    Ok((&summary).into())
}

/// 当前缓冲的全局设置（不发网络请求）。
pub async fn backup_cached_settings() -> Result<BackupSettingsDto, BridgeError> {
    let app = state::app()?;
    let settings = app.engine.cached_settings();
    Ok((&settings).into())
}

/// 最近活动（只来自本机状态库）。
pub async fn backup_recent_activity(limit: i64) -> Result<Vec<BackupActivityDto>, BridgeError> {
    let app = state::app()?;
    let limit = u32_from_i64(limit, "活动条数")?;
    let activities: Vec<BackupActivity> = app.engine.recent_activity(limit).await?;
    Ok(activities.iter().map(BackupActivityDto::from).collect())
}

/// 立即备份一个文件夹并返回统计。
pub async fn backup_scan_folder_now(folder_id: String) -> Result<BackupRunReportDto, BridgeError> {
    let app = state::app()?;
    let folder_id = parse_uuid(&folder_id, "备份文件夹")?;
    let report: BackupRunReport = app.engine.scan_folder_now(folder_id).await?;
    Ok(backup_run_report(&report))
}

/// 立即备份全部文件夹并返回每个文件夹的统计。
pub async fn backup_scan_all_now() -> Result<Vec<BackupRunReportDto>, BridgeError> {
    let app = state::app()?;
    let reports: Vec<BackupRunReport> = app.engine.scan_all_now().await?;
    Ok(reports.iter().map(backup_run_report).collect())
}

/// 备份状态/活动/汇总事件流。
pub fn backup_events_stream(sink: StreamSink<BackupEventDto>) {
    let Ok(app) = state::app() else {
        return;
    };
    let receiver = app.engine.subscribe();
    state::forward_backup_stream(sink, receiver);
}
