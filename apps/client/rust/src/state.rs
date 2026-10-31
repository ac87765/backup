//! 桥接层进程级状态。
//!
//! 一个应用进程只有一个已初始化的 [`AppState`]：Rust 核心 `Client`、备份引擎、
//! 传输任务注册表与事件广播通道。界面通过 API 函数操作它；Dart 永远拿不到
//! 内部的 `Client` / `BackupEngine` / 密钥。

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, RwLock};

use client_core::desktop::{BackupEngine, BackupEvent};
use client_core::{
    Client, DownloadTask, FolderDownloadTask, FolderJobStatus, FolderProgress, FolderUploadTask,
    TransferProgress, TransferStatus, UploadTask,
};
use tokio::sync::broadcast;

use crate::api::error::BridgeError;
use crate::api::types::{
    BackupEventDto, FolderProgressDto, TransferFailureDto, TransferProgressDto, TransferResultDto,
    TransferStatusDto,
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
}

impl TransferTask {
    pub(crate) fn id(&self) -> String {
        match self {
            Self::Upload(task) => task.id().to_string(),
            Self::Download(task) => task.id().to_string(),
            Self::FolderUpload(task) => task.id().to_string(),
            Self::FolderDownload(task) => task.id().to_string(),
        }
    }

    pub(crate) fn pause(&self) {
        match self {
            Self::Upload(task) => task.pause(),
            Self::Download(task) => task.pause(),
            Self::FolderUpload(task) => task.pause(),
            Self::FolderDownload(task) => task.pause(),
        }
    }

    pub(crate) fn resume(&self) {
        match self {
            Self::Upload(task) => task.resume(),
            Self::Download(task) => task.resume(),
            Self::FolderUpload(task) => task.resume(),
            Self::FolderDownload(task) => task.resume(),
        }
    }

    pub(crate) fn cancel(&self) {
        match self {
            Self::Upload(task) => task.cancel(),
            Self::Download(task) => task.cancel(),
            Self::FolderUpload(task) => task.cancel(),
            Self::FolderDownload(task) => task.cancel(),
        }
    }

    /// 执行任务并转换成终态 DTO；错误统一为 `Failed` / `Cancelled` 结果。
    pub(crate) async fn run(self) -> TransferResultDto {
        let transfer_id = self.id();
        let outcome = match self {
            Self::Upload(task) => task.run().await.map(|outcome| TransferResultDto {
                transfer_id: transfer_id.clone(),
                status: TransferStatusDto::Completed,
                error: None,
                upload: Some((&outcome).into()),
                download: None,
                folder_upload: None,
                folder_download: None,
            }),
            Self::Download(task) => task.run().await.map(|outcome| TransferResultDto {
                transfer_id: transfer_id.clone(),
                status: TransferStatusDto::Completed,
                error: None,
                upload: None,
                download: Some((&outcome).into()),
                folder_upload: None,
                folder_download: None,
            }),
            Self::FolderUpload(task) => task.run().await.map(|outcome| TransferResultDto {
                transfer_id: transfer_id.clone(),
                status: TransferStatusDto::Completed,
                error: None,
                upload: None,
                download: None,
                folder_upload: Some((&outcome).into()),
                folder_download: None,
            }),
            Self::FolderDownload(task) => task.run().await.map(|outcome| TransferResultDto {
                transfer_id: transfer_id.clone(),
                status: TransferStatusDto::Completed,
                error: None,
                upload: None,
                download: None,
                folder_upload: None,
                folder_download: Some((&outcome).into()),
            }),
        };
        match outcome {
            Ok(result) => result,
            Err(error) => {
                let error = BridgeError::from(error);
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
                }
            }
        }
    }
}

static STATE: LazyLock<RwLock<Option<Arc<AppState>>>> = LazyLock::new(|| RwLock::new(None));

/// 安装（或替换）应用状态。
pub(crate) fn install(client: Client, engine: BackupEngine) -> Result<Arc<AppState>, BridgeError> {
    let (transfer_events, _) = broadcast::channel(EVENT_BUFFER);
    let (folder_events, _) = broadcast::channel(EVENT_BUFFER);
    let state = Arc::new(AppState {
        client,
        engine,
        transfer_events,
        folder_events,
        transfers: Mutex::new(TransferRegistry::default()),
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
