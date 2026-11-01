//! 传输引擎（design D4）。
//!
//! - 上传：4 MiB 分块、批量预签名/确认（每批最多 32 块）、提交；文件级并发 3。
//! - 下载：预签名 URL 直连对象存储（不带会话 Cookie），逐块校验 SHA-256 后解密；
//!   先写同目录临时文件，全部校验通过后再重命名。
//! - 控制：暂停 / 继续 / 取消 / 重试；取消上传时调用 abort 释放配额预留。
//! - 事件：`tokio::sync::broadcast` 推送 [`TransferProgress`]，只含 ID、计数、
//!   状态与稳定错误码，不含路径、密钥、令牌或预签名 URL。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use reqwest::Method;
use tokio::sync::{Notify, broadcast};
use uuid::Uuid;

use crate::Client;
use crate::error::{ClientError, TransferError};
use crate::io_path::{ensure_valid_name, open_for_shared_read, path_for_io};
use crate::model::{
    ConflictPolicy, DownloadOutcome, FolderDownloadOutcome, FolderProgress, FolderUploadOutcome,
    PendingUpload, TransferDirection, TransferErrorCode, TransferFailure, TransferProgress,
    TransferStatus, UploadOutcome,
};

mod download;
mod folder;
mod limiter;
mod scan;
mod upload;

/// 微信恢复等桌面任务复用“无进度”单文件下载（内部逐块校验、临时文件原子落盘）。
pub(crate) use download::run_silent as download_file_silent;
pub(crate) use limiter::TokenBucket;

/// 默认进度通道容量；界面来不及消费时只保留最近事件，不阻塞传输。
const EVENT_BUFFER: usize = 256;
/// 单个文件上传时同时进行的块 PUT 数（文件级并发 3）。
pub(crate) const CHUNK_CONCURRENCY: usize = 3;
/// 批量预签名/确认的最大块数（服务端契约）。
pub(crate) const MAX_BATCH_CHUNKS: usize = 32;

/// 传输控制状态。内部用 `Arc`，克隆后仍共享同一暂停/取消状态，
/// 因此可以在另一个任务里调用 `pause` / `resume` / `cancel`。
#[derive(Debug, Clone)]
pub(crate) struct TransferControl {
    inner: Arc<ControlInner>,
}

#[derive(Debug)]
pub(crate) struct ControlInner {
    pub(crate) paused: AtomicBool,
    pub(crate) cancelled: AtomicBool,
    notify: Notify,
}

impl std::ops::Deref for TransferControl {
    type Target = ControlInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl ControlInner {
    pub(crate) fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

impl TransferControl {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(ControlInner {
                paused: AtomicBool::new(false),
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    pub(crate) fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub(crate) fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// 暂停检查点：暂停时等待“继续”；取消时返回 [`ClientError::Cancelled`]。
    pub(crate) async fn checkpoint(&self) -> Result<(), ClientError> {
        loop {
            if self.is_cancelled() {
                return Err(ClientError::Cancelled);
            }
            if !self.paused.load(Ordering::SeqCst) {
                return Ok(());
            }
            self.notify.notified().await;
        }
    }
}

/// 进度接收器：上传/下载使用 broadcast；文件夹任务把它转成聚合进度。
pub(crate) type ProgressSink = Arc<dyn Fn(TransferProgress) + Send + Sync>;
/// 文件夹聚合进度接收器。
pub(crate) type FolderSink = Arc<dyn Fn(FolderProgress) + Send + Sync>;

fn broadcast_sink(sender: broadcast::Sender<TransferProgress>) -> ProgressSink {
    Arc::new(move |progress| {
        // 没有订阅者时 send 返回错误，属于正常情况，不能中断传输。
        let _ = sender.send(progress);
    })
}

#[allow(clippy::too_many_arguments)]
fn emit(
    sink: &ProgressSink,
    transfer_id: Uuid,
    direction: TransferDirection,
    status: TransferStatus,
    completed_bytes: u64,
    total_bytes: u64,
    completed_chunks: u32,
    total_chunks: u32,
    error: Option<TransferFailure>,
) {
    sink(TransferProgress {
        transfer_id,
        direction,
        status,
        completed_bytes,
        total_bytes,
        completed_chunks,
        total_chunks,
        error,
    });
}

/// 把任意 [`ClientError`] 转成进度事件里的稳定错误码。
pub(crate) fn failure(error: &ClientError) -> TransferFailure {
    TransferFailure {
        code: error.transfer_code(),
        message: error.to_string(),
    }
}

pub(crate) fn transfer_error(code: TransferErrorCode, message: impl Into<String>) -> ClientError {
    ClientError::Transfer(TransferError::new(code, message))
}

pub(crate) fn invalid_name() -> ClientError {
    transfer_error(TransferErrorCode::InvalidName, "文件或文件夹名称不合法")
}

/// 上传任务。`Clone` 只共享控制状态与进度通道；调用方可以在另一个任务里
/// `pause` / `resume` / `cancel`。
#[derive(Clone)]
pub struct UploadTask {
    id: Uuid,
    client: Client,
    request: UploadRequest,
    control: Arc<TransferControl>,
    events: broadcast::Sender<TransferProgress>,
    /// 仅后台备份任务携带限速器；手动上传为 `None`（design D6）。
    limiter: Option<Arc<TokenBucket>>,
}

#[derive(Clone, Debug)]
pub(crate) enum UploadRequest {
    NewFile { parent_id: Uuid, path: PathBuf },
    NewVersion { node_id: Uuid, path: PathBuf },
    Resume { session_id: Uuid, path: PathBuf },
}

impl UploadTask {
    fn new(client: Client, request: UploadRequest, limiter: Option<Arc<TokenBucket>>) -> Self {
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Self {
            id: Uuid::new_v4(),
            client,
            request,
            control: Arc::new(TransferControl::new()),
            events,
            limiter,
        }
    }

    /// 传输 ID（进度事件中的 `transfer_id`）。
    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// 订阅进度事件。
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<TransferProgress> {
        self.events.subscribe()
    }

    /// 暂停；在下一个块/检查点生效。已确认块不会丢失。
    pub fn pause(&self) {
        self.control.pause();
    }

    /// 继续。
    pub fn resume(&self) {
        self.control.resume();
    }

    /// 取消；运行中的任务会调用 abort 释放服务端配额预留并删除本地续传记录。
    pub fn cancel(&self) {
        self.control.cancel();
    }

    /// 在后台启动。
    #[must_use]
    pub fn start(&self) -> tokio::task::JoinHandle<Result<UploadOutcome, ClientError>> {
        let task = self.clone();
        tokio::spawn(async move { task.run().await })
    }

    /// 执行（或重试）上传。失败后再次调用会从本地记录的已确认块继续；
    /// 会话过期时自动从头开始。
    pub async fn run(&self) -> Result<UploadOutcome, ClientError> {
        let sink = broadcast_sink(self.events.clone());
        upload::run(
            &self.client,
            &self.request,
            self.id,
            &self.control,
            &sink,
            self.limiter.clone(),
        )
        .await
    }
}

/// 单文件下载任务。
#[derive(Clone)]
pub struct DownloadTask {
    id: Uuid,
    client: Client,
    node_id: Uuid,
    version_id: Uuid,
    file_name: String,
    destination_dir: PathBuf,
    policy: ConflictPolicy,
    control: Arc<TransferControl>,
    events: broadcast::Sender<TransferProgress>,
}

impl DownloadTask {
    fn new(
        client: Client,
        node_id: Uuid,
        version_id: Uuid,
        file_name: String,
        destination_dir: PathBuf,
        policy: ConflictPolicy,
    ) -> Self {
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Self {
            id: Uuid::new_v4(),
            client,
            node_id,
            version_id,
            file_name,
            destination_dir,
            policy,
            control: Arc::new(TransferControl::new()),
            events,
        }
    }

    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<TransferProgress> {
        self.events.subscribe()
    }

    pub fn pause(&self) {
        self.control.pause();
    }

    pub fn resume(&self) {
        self.control.resume();
    }

    pub fn cancel(&self) {
        self.control.cancel();
    }

    #[must_use]
    pub fn start(&self) -> tokio::task::JoinHandle<Result<DownloadOutcome, ClientError>> {
        let task = self.clone();
        tokio::spawn(async move { task.run().await })
    }

    pub async fn run(&self) -> Result<DownloadOutcome, ClientError> {
        let sink = broadcast_sink(self.events.clone());
        download::run(
            &self.client,
            self.node_id,
            self.version_id,
            &self.file_name,
            &self.destination_dir,
            self.policy,
            self.id,
            &self.control,
            Some(&sink),
        )
        .await
    }
}

/// 文件夹上传任务。
#[derive(Clone)]
pub struct FolderUploadTask {
    id: Uuid,
    client: Client,
    parent_id: Uuid,
    local_dir: PathBuf,
    control: Arc<TransferControl>,
    events: broadcast::Sender<FolderProgress>,
    /// 第一次运行做配额预检；重试失败项时不再用整目录总量重复预检。
    started: Arc<AtomicBool>,
}

impl FolderUploadTask {
    fn new(client: Client, parent_id: Uuid, local_dir: PathBuf) -> Self {
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Self {
            id: Uuid::new_v4(),
            client,
            parent_id,
            local_dir,
            control: Arc::new(TransferControl::new()),
            events,
            started: Arc::new(AtomicBool::new(false)),
        }
    }

    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<FolderProgress> {
        self.events.subscribe()
    }

    pub fn pause(&self) {
        self.control.pause();
    }

    pub fn resume(&self) {
        self.control.resume();
    }

    /// 取消：不再派发新文件，进行中的上传会 abort 释放预留。
    pub fn cancel(&self) {
        self.control.cancel();
    }

    #[must_use]
    pub fn start(&self) -> tokio::task::JoinHandle<Result<FolderUploadOutcome, ClientError>> {
        let task = self.clone();
        tokio::spawn(async move { task.run().await })
    }

    /// 执行或重试。再次调用时，已成功的文件会因内容相同被跳过，只会上传
    /// 失败项与变化项。
    pub async fn run(&self) -> Result<FolderUploadOutcome, ClientError> {
        let sink: Arc<dyn Fn(FolderProgress) + Send + Sync> = {
            let sender = self.events.clone();
            Arc::new(move |progress| {
                let _ = sender.send(progress);
            })
        };
        folder::run_upload(
            &self.client,
            self.id,
            self.parent_id,
            &self.local_dir,
            &self.started,
            &self.control,
            &sink,
        )
        .await
    }
}

/// 云端文件夹下载到本地目录的任务。
#[derive(Clone)]
pub struct FolderDownloadTask {
    id: Uuid,
    client: Client,
    folder_id: Uuid,
    folder_name: String,
    destination_root: PathBuf,
    policy: ConflictPolicy,
    control: Arc<TransferControl>,
    events: broadcast::Sender<FolderProgress>,
}

impl FolderDownloadTask {
    fn new(
        client: Client,
        folder_id: Uuid,
        folder_name: String,
        destination_root: PathBuf,
        policy: ConflictPolicy,
    ) -> Self {
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Self {
            id: Uuid::new_v4(),
            client,
            folder_id,
            folder_name,
            destination_root,
            policy,
            control: Arc::new(TransferControl::new()),
            events,
        }
    }

    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<FolderProgress> {
        self.events.subscribe()
    }

    pub fn pause(&self) {
        self.control.pause();
    }

    pub fn resume(&self) {
        self.control.resume();
    }

    pub fn cancel(&self) {
        self.control.cancel();
    }

    #[must_use]
    pub fn start(&self) -> tokio::task::JoinHandle<Result<FolderDownloadOutcome, ClientError>> {
        let task = self.clone();
        tokio::spawn(async move { task.run().await })
    }

    pub async fn run(&self) -> Result<FolderDownloadOutcome, ClientError> {
        let sink: Arc<dyn Fn(FolderProgress) + Send + Sync> = {
            let sender = self.events.clone();
            Arc::new(move |progress| {
                let _ = sender.send(progress);
            })
        };
        folder::run_download(
            &self.client,
            self.id,
            self.folder_id,
            &self.folder_name,
            &self.destination_root,
            self.policy,
            &self.control,
            &sink,
        )
        .await
    }
}

impl Client {
    /// 上传新文件。真正开始前不会创建服务端会话；调用 [`UploadTask::run`]。
    pub fn upload_file(
        &self,
        parent_id: Uuid,
        local_path: impl Into<PathBuf>,
    ) -> Result<UploadTask, ClientError> {
        self.upload_file_inner(parent_id, local_path.into(), None)
    }

    /// 后台备份专用：带令牌桶限速的 `upload_file`；手动上传不受限速影响。
    pub(crate) fn upload_file_with_limiter(
        &self,
        parent_id: Uuid,
        local_path: impl Into<PathBuf>,
        limiter: Option<Arc<TokenBucket>>,
    ) -> Result<UploadTask, ClientError> {
        self.upload_file_inner(parent_id, local_path.into(), limiter)
    }

    fn upload_file_inner(
        &self,
        parent_id: Uuid,
        path: PathBuf,
        limiter: Option<Arc<TokenBucket>>,
    ) -> Result<UploadTask, ClientError> {
        let name = file_name(&path)?;
        ensure_valid_name(name)?;
        ensure_readable_file(&path)?;
        Ok(UploadTask::new(
            self.clone(),
            UploadRequest::NewFile { parent_id, path },
            limiter,
        ))
    }

    /// 为已有文件上传新版本。
    pub fn upload_new_version(
        &self,
        node_id: Uuid,
        local_path: impl Into<PathBuf>,
    ) -> Result<UploadTask, ClientError> {
        self.upload_new_version_inner(node_id, local_path.into(), None)
    }

    /// 后台备份专用：带令牌桶限速的新版本上传。
    pub(crate) fn upload_new_version_with_limiter(
        &self,
        node_id: Uuid,
        local_path: impl Into<PathBuf>,
        limiter: Option<Arc<TokenBucket>>,
    ) -> Result<UploadTask, ClientError> {
        self.upload_new_version_inner(node_id, local_path.into(), limiter)
    }

    fn upload_new_version_inner(
        &self,
        node_id: Uuid,
        path: PathBuf,
        limiter: Option<Arc<TokenBucket>>,
    ) -> Result<UploadTask, ClientError> {
        let name = file_name(&path)?;
        ensure_valid_name(name)?;
        ensure_readable_file(&path)?;
        Ok(UploadTask::new(
            self.clone(),
            UploadRequest::NewVersion { node_id, path },
            limiter,
        ))
    }

    /// 列出本机状态库中的未完成上传（只含元数据，不含密钥）。
    pub async fn list_pending_uploads(&self) -> Result<Vec<PendingUpload>, ClientError> {
        let account = self.ensure_writable()?;
        let records = self.state_store.list_uploads(account.user_id).await?;
        Ok(records
            .into_iter()
            .map(|record| record.into_pending())
            .collect())
    }

    /// 按会话 ID 恢复上传任务。文件指纹不符时不会续传，任务执行会报错。
    pub async fn resume_pending_upload(&self, session_id: Uuid) -> Result<UploadTask, ClientError> {
        let record = self
            .state_store
            .get_upload(session_id)
            .await?
            .ok_or_else(|| transfer_error(TransferErrorCode::Other, "找不到待续传的上传会话"))?;
        Ok(UploadTask::new(
            self.clone(),
            UploadRequest::Resume {
                session_id,
                path: record.local_path,
            },
            None,
        ))
    }

    /// 放弃一个本地记录的上传会话：尽力 abort 服务端会话，然后删除本地记录。
    pub async fn abort_pending_upload(&self, session_id: Uuid) -> Result<(), ClientError> {
        let _ = self.abort_upload_session(session_id).await;
        self.state_store.delete_upload(session_id).await?;
        Ok(())
    }

    /// 上传整个本地文件夹；逐层与云端同名文件夹合并，文件级并发 3。
    pub fn upload_folder(
        &self,
        parent_id: Uuid,
        local_dir: impl Into<PathBuf>,
    ) -> Result<FolderUploadTask, ClientError> {
        let local_dir = local_dir.into();
        let name = local_dir
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or_else(invalid_name)?;
        ensure_valid_name(name)?;
        let metadata = std::fs::metadata(
            path_for_io(&local_dir)
                .map_err(|_| transfer_error(TransferErrorCode::LocalIo, "路径不是有效 UTF-8"))?,
        )?;
        if !metadata.is_dir() {
            return Err(transfer_error(
                TransferErrorCode::LocalIo,
                "所选路径不是文件夹",
            ));
        }
        Ok(FolderUploadTask::new(self.clone(), parent_id, local_dir))
    }

    /// 下载任意保留版本到指定目录（文件名来自节点展示数据）。
    pub fn download_file(
        &self,
        node_id: Uuid,
        version_id: Uuid,
        file_name: &str,
        destination_dir: impl Into<PathBuf>,
        policy: ConflictPolicy,
    ) -> Result<DownloadTask, ClientError> {
        if file_name.trim().is_empty() {
            return Err(invalid_name());
        }
        Ok(DownloadTask::new(
            self.clone(),
            node_id,
            version_id,
            file_name.to_owned(),
            destination_dir.into(),
            policy,
        ))
    }

    /// 下载云端文件夹到本地目录；会创建以 `folder_name` 命名的子目录并保留结构。
    pub fn download_folder(
        &self,
        folder_id: Uuid,
        folder_name: &str,
        destination_root: impl Into<PathBuf>,
        policy: ConflictPolicy,
    ) -> Result<FolderDownloadTask, ClientError> {
        if folder_name.trim().is_empty() {
            return Err(invalid_name());
        }
        Ok(FolderDownloadTask::new(
            self.clone(),
            folder_id,
            folder_name.to_owned(),
            destination_root.into(),
            policy,
        ))
    }

    /// 内部：调用服务端 abort。会话已关闭/不存在视为成功（调用方仍应清理本地记录）。
    pub(crate) async fn abort_upload_session(&self, session_id: Uuid) -> Result<(), ClientError> {
        let result = self
            .api
            .request_json::<serde_json::Value>(
                Method::POST,
                &format!("/api/v1/upload-sessions/{session_id}/abort"),
                None,
            )
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(error) => match error.api_code() {
                Some(
                    crate::error::ApiErrorCode::NotFound
                    | crate::error::ApiErrorCode::UploadSessionClosed,
                ) => Ok(()),
                _ => Err(error),
            },
        }
    }
}

pub(crate) fn file_name(path: &Path) -> Result<&str, ClientError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(invalid_name)
}

fn ensure_readable_file(path: &Path) -> Result<(), ClientError> {
    let io_path = path_for_io(path)
        .map_err(|_| transfer_error(TransferErrorCode::LocalIo, "路径不是有效 UTF-8"))?;
    let file = open_for_shared_read(&io_path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(transfer_error(
            TransferErrorCode::LocalIo,
            "所选路径不是普通文件",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn control_pause_resume_and_cancel() {
        let control = TransferControl::new();
        control.pause();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), control.checkpoint())
                .await
                .is_err()
        );
        control.resume();
        control.checkpoint().await.unwrap();
        control.cancel();
        assert!(matches!(
            control.checkpoint().await,
            Err(ClientError::Cancelled)
        ));
    }

    #[test]
    fn failure_maps_cancel_and_invalid_name_codes() {
        assert_eq!(
            failure(&ClientError::Cancelled).code,
            TransferErrorCode::Cancelled
        );
        assert_eq!(
            failure(&invalid_name()).code,
            TransferErrorCode::InvalidName
        );
    }
}
