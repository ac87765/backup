//! 桥接层进程级状态。
//!
//! 一个应用进程只有一个已初始化的 [`AppState`]：Rust 核心 `Client`、备份引擎、
//! 传输任务注册表与事件广播通道。界面通过 API 函数操作它；Dart 永远拿不到
//! 内部的 `Client` / `BackupEngine` / 密钥。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, RwLock};

use client_core::desktop::update::{
    UpdateCancellation, UpdateChecker, UpdateDownloadStatus, UpdateDownloader, UpdatePlatform,
    UpdateProgress, UpdateProgressSink, VerifiedUpdate,
};
use client_core::desktop::{BackupEngine, BackupEvent, WechatRestoreTask};
use client_core::release::ReleaseManifest;
use client_core::{
    Client, DownloadTask, FolderDownloadTask, FolderJobStatus, FolderProgress, FolderUploadTask,
    TransferProgress, TransferStatus, UploadTask,
};
use tokio::sync::broadcast;

use crate::api::error::BridgeError;
use crate::api::types::{
    BackupEventDto, FolderProgressDto, TransferFailureDto, TransferProgressDto, TransferResultDto,
    TransferStatusDto, UpdateDownloadStatusDto, UpdateProgressDto, WechatRestoreOutcomeDto,
};
use crate::frb_generated::StreamSink;

/// 传输/文件夹进度广播容量；界面来不及消费时只保留最近事件，不阻塞传输。
const EVENT_BUFFER: usize = 512;

/// 已初始化的应用状态。
pub(crate) struct AppState {
    pub(crate) client: Client,
    pub(crate) engine: BackupEngine,
    pub(crate) transfer_events: broadcast::Sender<TransferProgressDto>,
    pub(crate) folder_events: broadcast::Sender<FolderProgressDto>,
    pub(crate) transfers: Mutex<TransferRegistry>,
    pub(crate) updates: UpdateState,
}

impl AppState {
    pub(crate) fn subscribe_transfer_progress(&self) -> broadcast::Receiver<TransferProgressDto> {
        self.transfer_events.subscribe()
    }

    pub(crate) fn subscribe_folder_progress(&self) -> broadcast::Receiver<FolderProgressDto> {
        self.folder_events.subscribe()
    }

    pub(crate) fn registry(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, TransferRegistry>, BridgeError> {
        self.transfers
            .lock()
            .map_err(|_| BridgeError::internal("传输任务锁已损坏"))
    }
}

/// 桥接层持有的更新状态。
pub(crate) struct UpdateState {
    pub(crate) checker: UpdateChecker,
    pub(crate) downloader: UpdateDownloader,
    /// 最近一次检查发现的可下载更新。
    pub(crate) available: Mutex<Option<AvailableUpdate>>,
    /// 最近一次下载并通过校验的安装包。
    pub(crate) verified: Mutex<Option<VerifiedUpdate>>,
    /// 正在进行的下载（同一时间只允许一个）。
    pub(crate) active: Mutex<Option<ActiveUpdateDownload>>,
    pub(crate) events: broadcast::Sender<UpdateProgressDto>,
}

impl UpdateState {
    pub(crate) fn subscribe_progress(&self) -> broadcast::Receiver<UpdateProgressDto> {
        self.events.subscribe()
    }

    pub(crate) fn available(&self) -> Result<MutexGuard<'_, Option<AvailableUpdate>>, BridgeError> {
        self.available
            .lock()
            .map_err(|_| BridgeError::internal("更新状态锁已损坏"))
    }

    pub(crate) fn verified(&self) -> Result<MutexGuard<'_, Option<VerifiedUpdate>>, BridgeError> {
        self.verified
            .lock()
            .map_err(|_| BridgeError::internal("更新安装锁已损坏"))
    }

    pub(crate) fn active(
        &self,
    ) -> Result<MutexGuard<'_, Option<ActiveUpdateDownload>>, BridgeError> {
        self.active
            .lock()
            .map_err(|_| BridgeError::internal("更新下载锁已损坏"))
    }
}

/// 已发现、等待安装的更新。
#[derive(Clone)]
pub(crate) struct AvailableUpdate {
    pub(crate) manifest: ReleaseManifest,
}

/// 正在进行的更新下载。
pub(crate) struct ActiveUpdateDownload {
    pub(crate) cancel: UpdateCancellation,
}

/// 传输任务注册表。
#[derive(Default)]
pub(crate) struct TransferRegistry {
    tasks: HashMap<String, TransferTask>,
    results: HashMap<String, TransferResultDto>,
}

impl TransferRegistry {
    pub(crate) fn insert(&mut self, task: TransferTask) {
        self.tasks.insert(task.id(), task);
    }

    pub(crate) fn get(&self, transfer_id: &str) -> Option<&TransferTask> {
        self.tasks.get(transfer_id)
    }

    pub(crate) fn set_result(&mut self, transfer_id: String, result: TransferResultDto) {
        self.results.insert(transfer_id, result);
    }

    pub(crate) fn result(&self, transfer_id: &str) -> Option<TransferResultDto> {
        self.results.get(transfer_id).cloned()
    }

    pub(crate) fn clear_result(&mut self, transfer_id: &str) {
        self.results.remove(transfer_id);
    }
}

/// 一个进行中或已创建的传输任务。
#[derive(Clone)]
pub(crate) enum TransferTask {
    Upload(UploadTask),
    Download(DownloadTask),
    FolderUpload(FolderUploadTask),
    FolderDownload(FolderDownloadTask),
    WechatRestore(WechatRestoreTask),
}

impl TransferTask {
    pub(crate) fn id(&self) -> String {
        match self {
            Self::Upload(task) => task.id().to_string(),
            Self::Download(task) => task.id().to_string(),
            Self::FolderUpload(task) => task.id().to_string(),
            Self::FolderDownload(task) => task.id().to_string(),
            Self::WechatRestore(task) => task.id().to_string(),
        }
    }

    pub(crate) fn pause(&self) {
        match self {
            Self::Upload(task) => task.pause(),
            Self::Download(task) => task.pause(),
            Self::FolderUpload(task) => task.pause(),
            Self::FolderDownload(task) => task.pause(),
            Self::WechatRestore(task) => task.pause(),
        }
    }

    pub(crate) fn resume(&self) {
        match self {
            Self::Upload(task) => task.resume(),
            Self::Download(task) => task.resume(),
            Self::FolderUpload(task) => task.resume(),
            Self::FolderDownload(task) => task.resume(),
            Self::WechatRestore(task) => task.resume(),
        }
    }

    pub(crate) fn cancel(&self) {
        match self {
            Self::Upload(task) => task.cancel(),
            Self::Download(task) => task.cancel(),
            Self::FolderUpload(task) => task.cancel(),
            Self::FolderDownload(task) => task.cancel(),
            Self::WechatRestore(task) => task.cancel(),
        }
    }

    /// 执行任务并转换成终态 DTO；错误统一为 `Failed` / `Cancelled` 结果。
    pub(crate) async fn run(self) -> TransferResultDto {
        let transfer_id = self.id();
        let outcome: Result<TransferResultDto, BridgeError> = match self {
            Self::Upload(task) => task
                .run()
                .await
                .map(|outcome| TransferResultDto {
                    transfer_id: transfer_id.clone(),
                    status: TransferStatusDto::Completed,
                    error: None,
                    upload: Some((&outcome).into()),
                    download: None,
                    folder_upload: None,
                    folder_download: None,
                    wechat_restore: None,
                })
                .map_err(BridgeError::from),
            Self::Download(task) => task
                .run()
                .await
                .map(|outcome| TransferResultDto {
                    transfer_id: transfer_id.clone(),
                    status: TransferStatusDto::Completed,
                    error: None,
                    upload: None,
                    download: Some((&outcome).into()),
                    folder_upload: None,
                    folder_download: None,
                    wechat_restore: None,
                })
                .map_err(BridgeError::from),
            Self::FolderUpload(task) => task
                .run()
                .await
                .map(|outcome| TransferResultDto {
                    transfer_id: transfer_id.clone(),
                    status: TransferStatusDto::Completed,
                    error: None,
                    upload: None,
                    download: None,
                    folder_upload: Some((&outcome).into()),
                    folder_download: None,
                    wechat_restore: None,
                })
                .map_err(BridgeError::from),
            Self::FolderDownload(task) => task
                .run()
                .await
                .map(|outcome| TransferResultDto {
                    transfer_id: transfer_id.clone(),
                    status: TransferStatusDto::Completed,
                    error: None,
                    upload: None,
                    download: None,
                    folder_upload: None,
                    folder_download: Some((&outcome).into()),
                    wechat_restore: None,
                })
                .map_err(BridgeError::from),
            Self::WechatRestore(task) => task
                .run()
                .await
                .map(|outcome| TransferResultDto {
                    transfer_id: transfer_id.clone(),
                    status: TransferStatusDto::Completed,
                    error: None,
                    upload: None,
                    download: None,
                    folder_upload: None,
                    folder_download: None,
                    wechat_restore: Some(WechatRestoreOutcomeDto::from(&outcome)),
                })
                .map_err(BridgeError::from),
        };
        match outcome {
            Ok(result) => result,
            Err(error) => {
                let status = if error.code == "cancelled" {
                    TransferStatusDto::Cancelled
                } else {
                    TransferStatusDto::Failed
                };
                TransferResultDto {
                    transfer_id,
                    status,
                    error: Some(crate::api::types::TransferFailureDto {
                        code: error.code,
                        message: error.message,
                    }),
                    upload: None,
                    download: None,
                    folder_upload: None,
                    folder_download: None,
                    wechat_restore: None,
                }
            }
        }
    }
}

static STATE: LazyLock<RwLock<Option<Arc<AppState>>>> = LazyLock::new(|| RwLock::new(None));

/// 安装（或替换）应用状态。
pub(crate) fn install(
    client: Client,
    engine: BackupEngine,
    checker: UpdateChecker,
    downloader: UpdateDownloader,
) -> Result<Arc<AppState>, BridgeError> {
    let (transfer_events, _) = broadcast::channel(EVENT_BUFFER);
    let (folder_events, _) = broadcast::channel(EVENT_BUFFER);
    let (update_events, _) = broadcast::channel(EVENT_BUFFER);
    let state = Arc::new(AppState {
        client,
        engine,
        transfer_events,
        folder_events,
        transfers: Mutex::new(TransferRegistry::default()),
        updates: UpdateState {
            checker,
            downloader,
            available: Mutex::new(None),
            verified: Mutex::new(None),
            active: Mutex::new(None),
            events: update_events,
        },
    });
    let mut guard = STATE
        .write()
        .map_err(|_| BridgeError::internal("客户端状态锁已损坏"))?;
    *guard = Some(state.clone());
    Ok(state)
}

/// 取当前应用状态；未初始化时返回可展示错误。
pub(crate) fn app() -> Result<Arc<AppState>, BridgeError> {
    STATE
        .read()
        .map_err(|_| BridgeError::internal("客户端状态锁已损坏"))?
        .clone()
        .ok_or_else(|| BridgeError::new("not_initialized", "客户端尚未初始化"))
}

/// 为一次数据传输启动独立的当前线程运行时。
///
/// 不用 `tokio::spawn` 是为了让长时间传输不依赖 flutter_rust_bridge 的异步
/// 执行器生命周期；每个传输一个线程（界面级并发是个位数）。
pub(crate) fn spawn_transfer(app: Arc<AppState>, task: TransferTask) {
    let transfer_id = task.id();
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => {
                if let Ok(mut registry) = app.transfers.lock() {
                    registry.set_result(
                        transfer_id.clone(),
                        failed_result(transfer_id, "无法启动传输运行时"),
                    );
                }
                return;
            }
        };
        let result = runtime.block_on(task.run());
        if let Ok(mut registry) = app.transfers.lock() {
            registry.set_result(transfer_id, result);
        }
    });
}

/// 在后台线程下载并校验更新；进度通过 `updates.events` 广播。
pub(crate) fn spawn_update_download(
    app: Arc<AppState>,
    download_id: String,
    update: AvailableUpdate,
    platform: UpdatePlatform,
    cancel: UpdateCancellation,
) {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => {
                let _ = app.updates.events.send(failed_update_progress(
                    &download_id,
                    "internal_error",
                    "无法启动更新下载运行时",
                ));
                if let Ok(mut active) = app.updates.active.lock() {
                    *active = None;
                }
                return;
            }
        };
        let sink: UpdateProgressSink = {
            let app = app.clone();
            let download_id = download_id.clone();
            Arc::new(move |progress: UpdateProgress| {
                // Completed 由本线程在写入 verified 之后发送，避免界面收到
                // completed 后立刻调用 update_install 时还拿不到已校验文件。
                if progress.status == UpdateDownloadStatus::Completed {
                    return;
                }
                let dto = crate::convert::update_progress(&download_id, &progress);
                let _ = app.updates.events.send(dto);
            })
        };
        let result = runtime.block_on(app.updates.downloader.download_update(
            &update.manifest,
            platform,
            &cancel,
            &sink,
        ));
        if let Ok(verified) = result {
            let total = verified.size;
            if let Ok(mut guard) = app.updates.verified.lock() {
                *guard = Some(verified);
            }
            let _ = app.updates.events.send(UpdateProgressDto {
                download_id: download_id.clone(),
                status: UpdateDownloadStatusDto::Completed,
                downloaded_bytes: crate::convert::i64_from_u64(total),
                total_bytes: crate::convert::i64_from_u64(total),
                speed_bytes_per_second: 0,
                error: None,
            });
        }
        if let Ok(mut active) = app.updates.active.lock() {
            *active = None;
        }
    });
}

fn failed_update_progress(download_id: &str, code: &str, message: &str) -> UpdateProgressDto {
    UpdateProgressDto {
        download_id: download_id.to_owned(),
        status: UpdateDownloadStatusDto::Failed,
        downloaded_bytes: 0,
        total_bytes: 0,
        speed_bytes_per_second: 0,
        error: Some(TransferFailureDto {
            code: code.to_owned(),
            message: message.to_owned(),
        }),
    }
}

/// 广播单文件进度到全局事件流；终态事件发送后退出。
pub(crate) fn forward_transfer_progress(
    app: Arc<AppState>,
    mut receiver: broadcast::Receiver<TransferProgress>,
) {
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            loop {
                match receiver.recv().await {
                    Ok(progress) => {
                        let terminal = matches!(
                            progress.status,
                            TransferStatus::Completed
                                | TransferStatus::Failed
                                | TransferStatus::Cancelled
                        );
                        let dto = TransferProgressDto::from(&progress);
                        // 没有界面订阅者时 send 返回错误，属于正常情况，不能中断传输。
                        let _ = app.transfer_events.send(dto);
                        if terminal {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
}

/// 广播文件夹进度到全局事件流。
pub(crate) fn forward_folder_progress(
    app: Arc<AppState>,
    mut receiver: broadcast::Receiver<FolderProgress>,
) {
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            loop {
                match receiver.recv().await {
                    Ok(progress) => {
                        let terminal = matches!(
                            progress.status,
                            FolderJobStatus::Done
                                | FolderJobStatus::Cancelled
                                | FolderJobStatus::Failed
                        );
                        let dto = FolderProgressDto::from(&progress);
                        let _ = app.folder_events.send(dto);
                        if terminal {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
}

fn failed_result(transfer_id: String, message: &str) -> TransferResultDto {
    TransferResultDto {
        transfer_id,
        status: TransferStatusDto::Failed,
        error: Some(TransferFailureDto {
            code: "internal_error".to_owned(),
            message: message.to_owned(),
        }),
        upload: None,
        download: None,
        folder_upload: None,
        folder_download: None,
        wechat_restore: None,
    }
}

/// 把单文件进度广播转换成 Dart `Stream`。
///
/// 每个订阅一个 OS 线程 + current-thread runtime：订阅数量只有传输/文件夹/
/// 备份事件三条，代价可接受，同时避免依赖 flutter_rust_bridge 执行器是否
/// 处于 tokio 运行时上下文。
pub(crate) fn forward_transfer_stream(
    sink: StreamSink<TransferProgressDto>,
    mut receiver: broadcast::Receiver<TransferProgressDto>,
) {
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            loop {
                match receiver.recv().await {
                    Ok(progress) => {
                        // Dart 侧取消订阅后 add 返回错误，正常退出即可。
                        if sink.add(progress).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
}

/// 把文件夹进度广播转换成 Dart `Stream`。
pub(crate) fn forward_folder_stream(
    sink: StreamSink<FolderProgressDto>,
    mut receiver: broadcast::Receiver<FolderProgressDto>,
) {
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            loop {
                match receiver.recv().await {
                    Ok(progress) => {
                        if sink.add(progress).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
}

/// 把备份事件广播转换成 Dart `Stream`。
pub(crate) fn forward_backup_stream(
    sink: StreamSink<BackupEventDto>,
    mut receiver: broadcast::Receiver<BackupEvent>,
) {
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        if sink.add(BackupEventDto::from(&event)).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
}

/// 把更新下载进度广播转换成 Dart `Stream`。
pub(crate) fn forward_update_stream(
    sink: StreamSink<UpdateProgressDto>,
    mut receiver: broadcast::Receiver<UpdateProgressDto>,
) {
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async move {
            loop {
                match receiver.recv().await {
                    Ok(progress) => {
                        if sink.add(progress).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    });
}
