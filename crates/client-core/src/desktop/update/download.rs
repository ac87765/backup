//! 下载更新安装包并校验大小与 SHA-256。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crypto_core::ContentHasher;
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Notify;

use crate::release::{ReleaseArtifact, ReleaseManifest};

use super::{UpdateError, build_update_http, downloads_url, parse_server_url};

/// 安装包平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePlatform {
    WindowsX64,
    MacOsUniversal,
}

impl UpdatePlatform {
    /// 当前编译目标对应的平台；其他平台返回 `None`（自动安装返回 Unsupported）。
    #[must_use]
    pub fn current() -> Option<Self> {
        #[cfg(target_os = "windows")]
        {
            Some(Self::WindowsX64)
        }
        #[cfg(target_os = "macos")]
        {
            Some(Self::MacOsUniversal)
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            None
        }
    }

    /// 稳定字符串（与清单 `platforms` 的键一致）。
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::WindowsX64 => "windows-x64",
            Self::MacOsUniversal => "macos-universal",
        }
    }

    /// 取清单中该平台的安装包信息。
    #[must_use]
    pub fn artifact(self, manifest: &ReleaseManifest) -> &ReleaseArtifact {
        match self {
            Self::WindowsX64 => &manifest.platforms.windows_x64,
            Self::MacOsUniversal => &manifest.platforms.macos_universal,
        }
    }
}

/// 下载状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateDownloadStatus {
    Downloading,
    Completed,
    Cancelled,
    Failed,
}

/// 进度事件里的失败信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFailure {
    pub code: String,
    pub message: String,
}

/// 下载进度事件；与传输进度事件同风格，不包含文件路径或响应体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateProgress {
    pub status: UpdateDownloadStatus,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    /// 自本次下载开始的平均速度（字节/秒）。
    pub speed_bytes_per_second: u64,
    pub error: Option<UpdateFailure>,
}

/// 进度接收器。
pub type UpdateProgressSink = Arc<dyn Fn(UpdateProgress) + Send + Sync>;

/// 取消句柄；可跨线程克隆。
#[derive(Clone, Default)]
pub struct UpdateCancellation {
    inner: Arc<CancelInner>,
}

#[derive(Default)]
struct CancelInner {
    cancelled: AtomicBool,
    notify: Notify,
}

impl UpdateCancellation {
    /// 新建未取消的句柄。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求取消；下载会在下一个数据块/等待点退出并删除 partial。
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    /// 是否已取消。
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

impl std::fmt::Debug for UpdateCancellation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("UpdateCancellation")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// 已通过大小与 SHA-256 校验的安装包。
///
/// 只有 [`UpdateDownloader::download_update`] 能构造它；安装函数只接受该类型，
/// 从类型上避免运行未校验的文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedUpdate {
    pub version: String,
    pub platform: UpdatePlatform,
    pub file_name: String,
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
}

/// 下载器：持有服务器地址与应用数据目录。
pub struct UpdateDownloader {
    server_url: reqwest::Url,
    app_data_dir: PathBuf,
    http: reqwest::Client,
}

impl UpdateDownloader {
    /// 创建下载器；`app_data_dir` 是状态库所在的应用数据目录，
    /// `app_version` 用于 User-Agent（不携带用户信息）。
    pub fn new(
        server_url: &str,
        app_data_dir: impl Into<PathBuf>,
        app_version: &str,
    ) -> Result<Self, UpdateError> {
        let server_url = parse_server_url(server_url)?;
        let app_data_dir = app_data_dir.into();
        let http = build_update_http(app_version, None)?;
        Ok(Self {
            server_url,
            app_data_dir,
            http,
        })
    }

    /// 下载并校验安装包。
    ///
    /// 流程：
    /// 1. 清理 `updates/` 下其他版本的目录；
    /// 2. 若同版本最终文件已存在，重新计算大小与 SHA-256，通过则直接复用；
    /// 3. 否则写入 `<file>.partial`，边下边算 SHA-256；超过清单大小立即中止；
    /// 4. 大小与 SHA-256 都一致才原子改名为最终文件；否则删除并返回
    ///    [`UpdateError::Tampered`]。
    pub async fn download_update(
        &self,
        manifest: &ReleaseManifest,
        platform: UpdatePlatform,
        cancel: &UpdateCancellation,
        progress: &UpdateProgressSink,
    ) -> Result<VerifiedUpdate, UpdateError> {
        let artifact = platform.artifact(manifest);
        let mut session = Session::new(artifact.size, progress);
        let result = self
            .download_inner(manifest, platform, cancel, &mut session)
            .await;
        match &result {
            Ok(_) => session.emit(UpdateDownloadStatus::Completed, None),
            Err(error) => {
                let status = if matches!(error, UpdateError::Cancelled) {
                    UpdateDownloadStatus::Cancelled
                } else {
                    UpdateDownloadStatus::Failed
                };
                session.emit(status, Some(error.failure()));
            }
        }
        result
    }

    async fn download_inner(
        &self,
        manifest: &ReleaseManifest,
        platform: UpdatePlatform,
        cancel: &UpdateCancellation,
        session: &mut Session<'_>,
    ) -> Result<VerifiedUpdate, UpdateError> {
        let artifact = platform.artifact(manifest);
        let updates_dir = self.app_data_dir.join("updates");
        cleanup_other_versions(&updates_dir, &manifest.version)?;
        let version_dir = updates_dir.join(&manifest.version);
        tokio::fs::create_dir_all(&version_dir).await?;

        let final_path = version_dir.join(&artifact.file);
        if final_path.exists() {
            if verify_existing(&final_path, artifact).await? {
                session.downloaded = artifact.size;
                return Ok(VerifiedUpdate {
                    version: manifest.version.clone(),
                    platform,
                    file_name: artifact.file.clone(),
                    path: final_path,
                    size: artifact.size,
                    sha256: artifact.sha256.clone(),
                });
            }
            tokio::fs::remove_file(&final_path).await?;
        }

        let partial = version_dir.join(format!("{}.partial", artifact.file));
        remove_if_exists(&partial).await;
        let mut file = tokio::fs::File::create(&partial).await?;

        let result = self
            .stream_to_file(&mut file, manifest, artifact, cancel, session)
            .await;
        drop(file);
        if let Err(error) = result {
            remove_if_exists(&partial).await;
            return Err(error);
        }

        if session.downloaded != artifact.size {
            remove_if_exists(&partial).await;
            return Err(UpdateError::Tampered);
        }
        let actual = session.hasher.finish_hex();
        if !constant_time_hex_eq(&actual, &artifact.sha256) {
            remove_if_exists(&partial).await;
            return Err(UpdateError::Tampered);
        }

        if let Err(error) = tokio::fs::rename(&partial, &final_path).await {
            remove_if_exists(&partial).await;
            return Err(error.into());
        }
        Ok(VerifiedUpdate {
            version: manifest.version.clone(),
            platform,
            file_name: artifact.file.clone(),
            path: final_path,
            size: artifact.size,
            sha256: artifact.sha256.clone(),
        })
    }

    async fn stream_to_file(
        &self,
        file: &mut tokio::fs::File,
        manifest: &ReleaseManifest,
        artifact: &ReleaseArtifact,
        cancel: &UpdateCancellation,
        session: &mut Session<'_>,
    ) -> Result<(), UpdateError> {
        let url = downloads_url(
            &self.server_url,
            &["downloads", &manifest.version, &artifact.file],
        )?;
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(UpdateError::Cancelled),
            response = self.http.get(url).send() => response.map_err(|_| UpdateError::Network)?,
        };
        let mut response = response;
        let status = response.status();
        if !status.is_success() {
            return Err(UpdateError::HttpStatus(status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|length| length > artifact.size)
        {
            return Err(UpdateError::Tampered);
        }

        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(UpdateError::Cancelled),
                chunk = response.chunk() => chunk.map_err(|_| UpdateError::Network)?,
            };
            let Some(chunk) = chunk else {
                break;
            };
            if session.downloaded.saturating_add(chunk.len() as u64) > artifact.size {
                return Err(UpdateError::Tampered);
            }
            session.hasher.update(&chunk);
            file.write_all(&chunk).await?;
            session.downloaded = session.downloaded.saturating_add(chunk.len() as u64);
            session.emit(UpdateDownloadStatus::Downloading, None);
        }

        file.flush().await?;
        file.sync_all().await?;
        Ok(())
    }
}

/// 一次下载的可变状态（进度、哈希、速度）。
struct Session<'a> {
    total: u64,
    downloaded: u64,
    started: Instant,
    hasher: ContentHasher,
    sink: &'a UpdateProgressSink,
}

impl<'a> Session<'a> {
    fn new(total: u64, sink: &'a UpdateProgressSink) -> Self {
        Self {
            total,
            downloaded: 0,
            started: Instant::now(),
            hasher: ContentHasher::new(),
            sink,
        }
    }

    fn speed(&self) -> u64 {
        let seconds = self.started.elapsed().as_secs_f64();
        if seconds < 0.05 {
            0
        } else {
            (self.downloaded as f64 / seconds) as u64
        }
    }

    fn emit(&self, status: UpdateDownloadStatus, error: Option<UpdateFailure>) {
        let event = UpdateProgress {
            status,
            downloaded_bytes: self.downloaded,
            total_bytes: self.total,
            speed_bytes_per_second: self.speed(),
            error,
        };
        (self.sink)(event);
    }
}

/// 清理 `updates/` 下除当前版本外的其他版本子目录。
fn cleanup_other_versions(updates_dir: &Path, keep: &str) -> Result<(), UpdateError> {
    let entries = match std::fs::read_dir(updates_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if entry.file_name() == std::ffi::OsStr::new(keep) {
            continue;
        }
        std::fs::remove_dir_all(entry.path())?;
    }
    Ok(())
}

async fn remove_if_exists(path: &Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {}
    }
}

/// 重新校验已有文件；不存在或校验失败返回 `false`。
async fn verify_existing(path: &Path, artifact: &ReleaseArtifact) -> Result<bool, UpdateError> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if metadata.len() != artifact.size {
        return Ok(false);
    }
    let mut file = tokio::fs::File::open(path).await?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut hasher = ContentHasher::new();
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(constant_time_hex_eq(&hasher.finish_hex(), &artifact.sha256))
}

/// 常量时间比较两个十六进制摘要（长度不同直接返回 false）。
fn constant_time_hex_eq(left: &str, right: &str) -> bool {
    left.len() == right.len() && bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}
