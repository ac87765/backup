//! 传输 API：启动上传/下载、暂停/继续/取消/重试、断点续传、进度事件。
//!
//! 文件内容不经过 Dart：上传由 `client-core` 直接读取本地路径，下载直接写入
//! 用户选择的本地目录（design D2）。Dart 只拿到任务 ID 与展示进度。

use std::path::PathBuf;

use client_core::ConflictPolicy;

use crate::api::error::BridgeError;
use crate::api::types::{
    ConflictPolicyDto, FolderProgressDto, PendingUploadDto, TransferProgressDto, TransferResultDto,
};
use crate::convert::parse_uuid;
use crate::frb_generated::StreamSink;
use crate::state::{self, AppState, TransferTask};

/// 上传新文件到指定文件夹；返回传输 ID，进度通过 `transfer_progress_stream` 推送。
pub async fn start_upload_file(
    parent_id: String,
    local_path: String,
) -> Result<String, BridgeError> {
    let app = state::app()?;
    let parent_id = parse_uuid(&parent_id, "文件夹")?;
    let task = app
        .client
        .upload_file(parent_id, PathBuf::from(local_path))?;
    register_upload(&app, task)
}

/// 为已有文件上传新版本。
pub async fn start_upload_new_version(
    node_id: String,
    local_path: String,
) -> Result<String, BridgeError> {
    let app = state::app()?;
    let node_id = parse_uuid(&node_id, "节点")?;
    let task = app
        .client
        .upload_new_version(node_id, PathBuf::from(local_path))?;
    register_upload(&app, task)
}

/// 上传整个本地文件夹；逐层与云端同名文件夹合并。
pub async fn start_upload_folder(
    parent_id: String,
    local_dir: String,
) -> Result<String, BridgeError> {
    let app = state::app()?;
    let parent_id = parse_uuid(&parent_id, "文件夹")?;
    let task = app
        .client
        .upload_folder(parent_id, PathBuf::from(local_dir))?;
    register_folder_upload(&app, task)
}

/// 下载文件（可指定历史版本）到本地目录。
pub async fn start_download_file(
    node_id: String,
    version_id: String,
    file_name: String,
    destination_dir: String,
    policy: ConflictPolicyDto,
) -> Result<String, BridgeError> {
    let app = state::app()?;
    let node_id = parse_uuid(&node_id, "节点")?;
    let version_id = parse_uuid(&version_id, "版本")?;
    let task = app.client.download_file(
        node_id,
        version_id,
        &file_name,
        PathBuf::from(destination_dir),
        ConflictPolicy::from(policy),
    )?;
    register_download(&app, task)
}

/// 下载云端文件夹到本地目录（保留目录结构，不打包 zip）。
pub async fn start_download_folder(
    folder_id: String,
    folder_name: String,
    destination_root: String,
    policy: ConflictPolicyDto,
) -> Result<String, BridgeError> {
    let app = state::app()?;
    let folder_id = parse_uuid(&folder_id, "文件夹")?;
    let task = app.client.download_folder(
        folder_id,
        &folder_name,
        PathBuf::from(destination_root),
        ConflictPolicy::from(policy),
    )?;
    register_folder_download(&app, task)
}

/// 本机状态库中未完成的上传（应用重启后可续传）。
pub async fn list_pending_uploads() -> Result<Vec<PendingUploadDto>, BridgeError> {
    let app = state::app()?;
    let records = app.client.list_pending_uploads().await?;
    Ok(records.iter().map(PendingUploadDto::from).collect())
}

/// 按会话 ID 恢复未完成上传；返回新的传输 ID。
pub async fn resume_pending_upload(session_id: String) -> Result<String, BridgeError> {
    let app = state::app()?;
    let session_id = parse_uuid(&session_id, "上传会话")?;
    let task = app.client.resume_pending_upload(session_id).await?;
    register_upload(&app, task)
}

/// 放弃一个未完成上传：尽力 abort 服务端会话并删除本地记录。
pub async fn abort_pending_upload(session_id: String) -> Result<(), BridgeError> {
    let app = state::app()?;
    let session_id = parse_uuid(&session_id, "上传会话")?;
    Ok(app.client.abort_pending_upload(session_id).await?)
}

/// 暂停传输（下一个块/检查点生效）。
pub async fn pause_transfer(transfer_id: String) -> Result<(), BridgeError> {
    let app = state::app()?;
    let task = task_by_id(&app, &transfer_id)?;
    task.pause();
    Ok(())
}

/// 继续传输。
pub async fn resume_transfer(transfer_id: String) -> Result<(), BridgeError> {
    let app = state::app()?;
    let task = task_by_id(&app, &transfer_id)?;
    task.resume();
    Ok(())
}

/// 取消传输；上传会调用 abort 释放服务端配额预留。
pub async fn cancel_transfer(transfer_id: String) -> Result<(), BridgeError> {
    let app = state::app()?;
    let task = task_by_id(&app, &transfer_id)?;
    task.cancel();
    Ok(())
}

/// 重试任务：上传从本地记录的已确认块继续，下载/文件夹任务重新开始。
pub async fn retry_transfer(transfer_id: String) -> Result<(), BridgeError> {
    let app = state::app()?;
    let task = task_by_id(&app, &transfer_id)?;
    {
        let mut registry = app.registry()?;
        registry.clear_result(&transfer_id);
    }
    resubscribe(&app, &task);
    state::spawn_transfer(app, task);
    Ok(())
}

/// 查询任务终态结果；任务仍在运行或 ID 不存在时返回 `None`。
pub async fn transfer_result(
    transfer_id: String,
) -> Result<Option<TransferResultDto>, BridgeError> {
    let app = state::app()?;
    let registry = app.registry()?;
    Ok(registry.result(&transfer_id))
}

/// 单文件传输进度事件流。
pub fn transfer_progress_stream(sink: StreamSink<TransferProgressDto>) {
    let Ok(app) = state::app() else {
        return;
    };
    let receiver = app.subscribe_transfer_progress();
    state::forward_transfer_stream(sink, receiver);
}

/// 文件夹任务进度事件流。
pub fn folder_progress_stream(sink: StreamSink<FolderProgressDto>) {
    let Ok(app) = state::app() else {
        return;
    };
    let receiver = app.subscribe_folder_progress();
    state::forward_folder_stream(sink, receiver);
}

fn task_by_id(app: &AppState, transfer_id: &str) -> Result<TransferTask, BridgeError> {
    let registry = app.registry()?;
    registry.get(transfer_id).cloned().ok_or_else(|| {
        BridgeError::new(
            "transfer_not_found",
            "找不到传输任务，可能已结束或应用已重启",
        )
    })
}

fn resubscribe(app: &std::sync::Arc<AppState>, task: &TransferTask) {
    match task {
        TransferTask::Upload(task) => {
            state::forward_transfer_progress(app.clone(), task.subscribe());
        }
        TransferTask::Download(task) => {
            state::forward_transfer_progress(app.clone(), task.subscribe());
        }
        TransferTask::FolderUpload(task) => {
            state::forward_folder_progress(app.clone(), task.subscribe());
        }
        TransferTask::FolderDownload(task) => {
            state::forward_folder_progress(app.clone(), task.subscribe());
        }
        TransferTask::WechatRestore(task) => {
            state::forward_transfer_progress(app.clone(), task.subscribe());
        }
    }
}

fn register_upload(
    app: &std::sync::Arc<AppState>,
    task: client_core::UploadTask,
) -> Result<String, BridgeError> {
    let receiver = task.subscribe();
    let transfer = TransferTask::Upload(task);
    let id = transfer.id();
    {
        let mut registry = app.registry()?;
        registry.insert(transfer.clone());
    }
    state::forward_transfer_progress(app.clone(), receiver);
    state::spawn_transfer(app.clone(), transfer);
    Ok(id)
}

fn register_download(
    app: &std::sync::Arc<AppState>,
    task: client_core::DownloadTask,
) -> Result<String, BridgeError> {
    let receiver = task.subscribe();
    let transfer = TransferTask::Download(task);
    let id = transfer.id();
    {
        let mut registry = app.registry()?;
        registry.insert(transfer.clone());
    }
    state::forward_transfer_progress(app.clone(), receiver);
    state::spawn_transfer(app.clone(), transfer);
    Ok(id)
}

fn register_folder_upload(
    app: &std::sync::Arc<AppState>,
    task: client_core::FolderUploadTask,
) -> Result<String, BridgeError> {
    let receiver = task.subscribe();
    let transfer = TransferTask::FolderUpload(task);
    let id = transfer.id();
    {
        let mut registry = app.registry()?;
        registry.insert(transfer.clone());
    }
    state::forward_folder_progress(app.clone(), receiver);
    state::spawn_transfer(app.clone(), transfer);
    Ok(id)
}

fn register_folder_download(
    app: &std::sync::Arc<AppState>,
    task: client_core::FolderDownloadTask,
) -> Result<String, BridgeError> {
    let receiver = task.subscribe();
    let transfer = TransferTask::FolderDownload(task);
    let id = transfer.id();
    {
        let mut registry = app.registry()?;
        registry.insert(transfer.clone());
    }
    state::forward_folder_progress(app.clone(), receiver);
    state::spawn_transfer(app.clone(), transfer);
    Ok(id)
}
