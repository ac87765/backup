//! 恢复到微信（add-wechat-backup 第 4 组，design D4）。
//!
//! 恢复是**唯一**会写入微信目录的操作（design D5），且只在用户确认后调用：
//!
//! - 文件类别：合并恢复，云端相对目录结构原样落到本地对应位置；本机已有同名
//!   文件按“跳过”策略保留，不覆盖。
//! - 数据库：只接受带 `快照完成` 标记的快照。先把快照下载到数据库目录同一父
//!   目录下的临时目录，逐文件核对清单中的大小与 `content_sha256`；全部通过后
//!   才把现有数据库目录改名为 `<原名>.before-restore-<时间>` 保留，再把临时
//!   目录原子改名成数据库目录。任何一步失败都删除临时目录并把旧目录改回。
//! - 微信运行时绝不写入：开始前检测到运行中直接返回
//!   [`BackupError::WechatRunning`]；过程中每个文件写入前再检测，检测到微信
//!   启动就把任务暂停并发出“需要退出微信”的进度事件。
//!
//! 本模块不读取、不解密或解析任何数据库 / 图片内容；清单是快照上传时写入的
//! JSON，只含相对路径、大小与 SHA-256。日志与错误信息不包含本机路径或文件名。

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use crypto_core::ContentHasher;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::Client;
use crate::clock::Clock;
use crate::error::ClientError;
use crate::io_path::{normalize_cloud_name, path_for_io, sanitize_local_name};
use crate::model::{
    ConflictPolicy, NodeKind, TransferDirection, TransferErrorCode, TransferFailure,
    TransferProgress, TransferStatus,
};
use crate::transfer::{TransferControl, download_file_silent, failure};

use super::super::error::BackupError;
use super::super::filter::name_key;
use super::super::types::{
    WechatDatabaseSnapshot, WechatRestoreCategoryInfo, WechatRestoreReport, WechatRestoreRequest,
    WechatSnapshotSelection,
};
use super::layout::{WechatCategory, WechatVersion, layout_for};
use super::process::WechatProcessProbe;
use super::restore_time;
use super::snapshot::{MarkerManifest, SNAPSHOT_DONE_FILE, WECHAT_CLOUD_ROOT, parse_timestamp};

/// 进度事件通道容量；界面来不及消费时只保留最近事件，不阻塞恢复。
const EVENT_BUFFER: usize = 256;
/// `before-restore-<时间>` 名称冲突时最多尝试的序号。
const MAX_BACKUP_NAME_SUFFIX: u32 = 100;

/// 已准备好的一个云端文件。
#[derive(Debug, Clone)]
struct RestoreFile {
    name: String,
    node_id: Uuid,
    version_id: Uuid,
    size: u64,
    local_parent: PathBuf,
}

/// 文件类别恢复计划。
#[derive(Debug, Default)]
struct CategoryPlan {
    files: Vec<RestoreFile>,
    total_bytes: u64,
}

/// 数据库快照恢复计划。
#[derive(Debug)]
struct SnapshotPlan {
    database_dir: PathBuf,
    files: Vec<SnapshotFilePlan>,
    total_bytes: u64,
}

#[derive(Debug)]
struct SnapshotFilePlan {
    relative_path: String,
    name: String,
    node_id: Uuid,
    version_id: Uuid,
    size: u64,
    content_sha256: String,
}

/// 一次恢复的完整计划。
#[derive(Debug)]
struct RestorePlan {
    category: CategoryPlan,
    snapshot: Option<SnapshotPlan>,
}

/// 云端一个文件。
#[derive(Debug, Clone)]
struct CloudFile {
    relative_path: String,
    name: String,
    node_id: Uuid,
    version_id: Uuid,
    size: u64,
}

/// 一次可恢复的数据库快照的位置。
#[derive(Debug, Clone)]
struct SnapshotLocation {
    device_name: String,
    timestamp: String,
    snapshot_at: i64,
    node_id: Uuid,
}

/// 微信恢复任务。`Clone` 共享暂停/取消状态与进度通道，可在另一个任务里控制。
#[derive(Clone)]
pub struct WechatRestoreTask {
    id: Uuid,
    client: Client,
    clock: Arc<dyn Clock>,
    probe: Arc<dyn WechatProcessProbe>,
    request: WechatRestoreRequest,
    plan: Arc<RestorePlan>,
    control: Arc<TransferControl>,
    events: broadcast::Sender<TransferProgress>,
}

impl WechatRestoreTask {
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

    /// 暂停；在下一个文件检查点生效。
    pub fn pause(&self) {
        self.control.pause();
    }

    /// 继续。
    pub fn resume(&self) {
        self.control.resume();
    }

    /// 取消；已写入的文件保留（文件类别是合并恢复），数据库恢复会走失败回滚。
    pub fn cancel(&self) {
        self.control.cancel();
    }

    /// 在后台启动。
    #[must_use]
    pub fn start(&self) -> tokio::task::JoinHandle<Result<WechatRestoreReport, BackupError>> {
        let task = self.clone();
        tokio::spawn(async move { task.run().await })
    }

    /// 执行恢复。
    pub async fn run(&self) -> Result<WechatRestoreReport, BackupError> {
        let result = self.run_inner().await;
        if let Err(error) = &result {
            let cancelled = self.control.is_cancelled()
                || matches!(error, BackupError::Client(ClientError::Cancelled));
            let failure = restore_failure(error);
            self.emit(
                if cancelled {
                    TransferStatus::Cancelled
                } else {
                    TransferStatus::Failed
                },
                0,
                0,
                0,
                0,
                if cancelled { None } else { Some(failure) },
            );
        }
        result
    }

    async fn run_inner(&self) -> Result<WechatRestoreReport, BackupError> {
        let total_files = self.plan.category.files.len() as u64 + self.plan.snapshot_files_count();
        let total_bytes = self.plan.category.total_bytes + self.plan.snapshot_bytes();
        let mut completed_files = 0_u64;
        let mut completed_bytes = 0_u64;
        let mut restored_files = 0_u64;
        let mut restored_bytes = 0_u64;
        let mut skipped_files = 0_u64;

        self.emit(
            TransferStatus::Downloading,
            0,
            total_bytes,
            0,
            total_files,
            None,
        );

        for file in &self.plan.category.files {
            self.wait_for_wechat().await?;
            self.ensure_local_directory(&file.local_parent).await?;
            let outcome = download_file_silent(
                &self.client,
                file.node_id,
                file.version_id,
                &file.name,
                &file.local_parent,
                ConflictPolicy::Skip,
                &self.control,
            )
            .await
            .map_err(BackupError::Client)?;
            completed_files = completed_files.saturating_add(1);
            completed_bytes = completed_bytes.saturating_add(file.size);
            if outcome.skipped {
                skipped_files = skipped_files.saturating_add(1);
            } else {
                restored_files = restored_files.saturating_add(1);
                restored_bytes = restored_bytes.saturating_add(file.size);
            }
            self.emit(
                TransferStatus::Downloading,
                completed_bytes,
                total_bytes,
                completed_files,
                total_files,
                None,
            );
        }

        let mut database_restored = false;
        if let Some(snapshot) = &self.plan.snapshot {
            self.wait_for_wechat().await?;
            self.restore_snapshot(snapshot).await?;
            database_restored = true;
            completed_files = completed_files.saturating_add(snapshot.files.len() as u64);
            completed_bytes = completed_bytes.saturating_add(snapshot.total_bytes);
            restored_files = restored_files.saturating_add(snapshot.files.len() as u64);
            restored_bytes = restored_bytes.saturating_add(snapshot.total_bytes);
            self.emit(
                TransferStatus::Downloading,
                completed_bytes,
                total_bytes,
                completed_files,
                total_files,
                None,
            );
        }

        self.emit(
            TransferStatus::Completed,
            completed_bytes,
            total_bytes,
            completed_files,
            total_files,
            None,
        );
        Ok(WechatRestoreReport {
            transfer_id: self.id,
            restored_files,
            restored_bytes,
            skipped_files,
            database_restored,
        })
    }

    /// 每个文件写入前检测微信；检测到运行中就先暂停并报告原因。
    async fn wait_for_wechat(&self) -> Result<(), BackupError> {
        loop {
            if self.control.is_cancelled() {
                return Err(BackupError::Client(ClientError::Cancelled));
            }
            if !self.probe.is_running(self.request.version) {
                return Ok(());
            }
            self.control.pause();
            self.emit(
                TransferStatus::Paused,
                0,
                0,
                0,
                0,
                Some(TransferFailure {
                    code: TransferErrorCode::WechatRunning,
                    message: "检测到微信已启动，恢复已暂停；请退出微信后继续".to_owned(),
                }),
            );
            // 等用户“继续”；醒来后重新检测，微信仍在运行就再次暂停。
            self.control.checkpoint().await?;
        }
    }

    async fn ensure_local_directory(&self, directory: &Path) -> Result<(), BackupError> {
        let directory = directory.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let path = path_for_io(&directory)
                .map_err(|_| BackupError::InvalidConfig("恢复目标路径无效"))?;
            std::fs::create_dir_all(path)
                .map_err(|_| BackupError::InvalidConfig("无法创建恢复目标目录"))
        })
        .await
        .map_err(|_| BackupError::InvalidConfig("创建恢复目录任务失败"))?
    }

    /// 数据库快照的“下载到临时目录 → 校验 → 改名保留旧目录 → 原子替换”流程。
    async fn restore_snapshot(&self, snapshot: &SnapshotPlan) -> Result<(), BackupError> {
        let parent = snapshot
            .database_dir
            .parent()
            .ok_or(BackupError::InvalidConfig("微信数据库目录路径无效"))?
            .to_path_buf();
        self.ensure_local_directory(&parent).await?;
        let database_name = snapshot
            .database_dir
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .ok_or(BackupError::InvalidConfig("微信数据库目录名无效"))?;
        let temp_name = format!(".{database_name}.restore-tmp-{}", Uuid::new_v4().simple());
        let temp_dir = parent.join(temp_name);
        self.ensure_local_directory(&temp_dir).await?;

        // 临时目录由本函数负责清理；成功改名后路径已不存在，remove_dir_all 返回
        // NotFound 属于正常情况。
        let temp_guard = TempDirGuard::new(temp_dir.clone());

        for file in &snapshot.files {
            self.wait_for_wechat().await?;
            let destination = snapshot_destination(&temp_dir, &file.relative_path)?;
            if let Some(destination_parent) = destination.parent() {
                self.ensure_local_directory(destination_parent).await?;
            }
            let download = download_file_silent(
                &self.client,
                file.node_id,
                file.version_id,
                &file.name,
                destination
                    .parent()
                    .ok_or(BackupError::InvalidConfig("快照文件路径无效"))?,
                ConflictPolicy::Replace,
                &self.control,
            )
            .await
            .map_err(snapshot_client_error)?;
            verify_snapshot_file(&download.path, file).await?;
        }

        // 提交前再检测一次微信，避免下载期间启动后仍然替换数据库。
        self.wait_for_wechat().await?;

        let backup_dir = next_backup_directory(&snapshot.database_dir, self.clock.now());
        let existing = path_for_io(&snapshot.database_dir)
            .map_err(|_| BackupError::InvalidConfig("微信数据库目录路径无效"))?
            .is_dir();
        commit_database(&temp_dir, &snapshot.database_dir, &backup_dir, existing)?;
        // 成功改名后临时目录已不存在；guard 的清理是无操作。
        drop(temp_guard);
        Ok(())
    }

    fn emit(
        &self,
        status: TransferStatus,
        completed_bytes: u64,
        total_bytes: u64,
        completed_files: u64,
        total_files: u64,
        error: Option<TransferFailure>,
    ) {
        // 没有订阅者时 send 返回错误，属于正常情况。
        let _ = self.events.send(TransferProgress {
            transfer_id: self.id,
            direction: TransferDirection::Download,
            status,
            completed_bytes,
            total_bytes,
            completed_chunks: u32::try_from(completed_files).unwrap_or(u32::MAX),
            total_chunks: u32::try_from(total_files).unwrap_or(u32::MAX),
            error,
        });
    }
}

impl RestorePlan {
    fn snapshot_files_count(&self) -> u64 {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.files.len() as u64)
            .unwrap_or_default()
    }

    fn snapshot_bytes(&self) -> u64 {
        self.snapshot
            .as_ref()
            .map(|snapshot| snapshot.total_bytes)
            .unwrap_or_default()
    }
}

/// 准备一次恢复：定位云端文件、校验快照清单；不写任何本地文件。
///
/// 开始前检测微信；检测到运行中直接返回 [`BackupError::WechatRunning`]。
pub(crate) async fn prepare_restore(
    client: &Client,
    clock: Arc<dyn Clock>,
    probe: Arc<dyn WechatProcessProbe>,
    request: WechatRestoreRequest,
) -> Result<WechatRestoreTask, BackupError> {
    if !super::backup::validate_account_name(&request.account_name) {
        return Err(BackupError::InvalidConfig("微信账号目录名无效"));
    }
    if probe.is_running(request.version) {
        return Err(BackupError::WechatRunning);
    }
    if request
        .categories
        .iter()
        .any(|category| !category.is_file_source())
    {
        return Err(BackupError::InvalidConfig("恢复文件类别无效"));
    }
    if request.restore_database && request.snapshot.is_none() {
        return Err(BackupError::InvalidConfig("恢复数据库需要选择快照"));
    }
    if !request.restore_database && request.snapshot.is_some() {
        return Err(BackupError::InvalidConfig("未选择恢复数据库"));
    }

    let account_dir = request.data_root.join(&request.account_name);
    let category = prepare_categories(client, &request, &account_dir).await?;
    let snapshot = if request.restore_database {
        let selection = request
            .snapshot
            .as_ref()
            .ok_or(BackupError::InvalidConfig("恢复数据库需要选择快照"))?;
        Some(prepare_snapshot(client, &request, &account_dir, selection).await?)
    } else {
        None
    };
    let plan = Arc::new(RestorePlan { category, snapshot });
    let (events, _) = broadcast::channel(EVENT_BUFFER);
    Ok(WechatRestoreTask {
        id: Uuid::new_v4(),
        client: client.clone(),
        clock,
        probe,
        request,
        plan,
        control: Arc::new(TransferControl::new()),
        events,
    })
}

/// 列出某账号在全部设备上的完整数据库快照（只返回带完成标记的快照）。
pub(crate) async fn list_database_snapshots(
    client: &Client,
    account_name: &str,
) -> Result<Vec<WechatDatabaseSnapshot>, BackupError> {
    if !super::backup::validate_account_name(account_name) {
        return Err(BackupError::InvalidConfig("微信账号目录名无效"));
    }
    let locations = snapshot_locations(client, account_name).await?;
    let mut snapshots = Vec::with_capacity(locations.len());
    for location in locations {
        let cloud = collect_cloud_files(client, location.node_id).await?;
        if cloud
            .iter()
            .any(|file| file.relative_path == SNAPSHOT_DONE_FILE)
        {
            let (file_count, total_bytes) = cloud
                .iter()
                .filter(|file| file.relative_path != SNAPSHOT_DONE_FILE)
                .fold((0_u64, 0_u64), |(count, bytes), file| {
                    (count + 1, bytes.saturating_add(file.size))
                });
            snapshots.push(WechatDatabaseSnapshot {
                device_name: location.device_name,
                timestamp: location.timestamp,
                snapshot_at: location.snapshot_at,
                total_bytes,
                file_count,
                complete: true,
            });
        }
    }
    snapshots.sort_by(|left, right| {
        right
            .snapshot_at
            .cmp(&left.snapshot_at)
            .then_with(|| left.device_name.cmp(&right.device_name))
            .then_with(|| left.timestamp.cmp(&right.timestamp))
    });
    Ok(snapshots)
}

/// 列出云端存在哪些文件类别可恢复（跨设备去重，按本地相对路径计一次）。
pub(crate) async fn list_restore_categories(
    client: &Client,
    account_name: &str,
    version: WechatVersion,
) -> Result<Vec<WechatRestoreCategoryInfo>, BackupError> {
    if !super::backup::validate_account_name(account_name) {
        return Err(BackupError::InvalidConfig("微信账号目录名无效"));
    }
    let layout = layout_for(version);
    let devices = device_folders(client).await?;
    let mut result = Vec::with_capacity(WechatCategory::ALL.len());
    for category in WechatCategory::ALL {
        if !category.is_file_source() {
            continue;
        }
        let Some(category_layout) = layout.category(category) else {
            continue;
        };
        let mut seen = HashSet::new();
        let mut file_count = 0_u64;
        let mut total_bytes = 0_u64;
        for (_, device_id) in &devices {
            for leaf in category_leaf_names(category_layout, account_name) {
                let Some(folder) =
                    find_category_folder(client, *device_id, account_name, category.label(), &leaf)
                        .await?
                else {
                    continue;
                };
                for file in collect_cloud_files(client, folder).await? {
                    if seen.insert(file.relative_path.clone()) {
                        file_count = file_count.saturating_add(1);
                        total_bytes = total_bytes.saturating_add(file.size);
                    }
                }
            }
        }
        result.push(WechatRestoreCategoryInfo {
            category,
            file_count,
            total_bytes,
            available: file_count > 0,
        });
    }
    Ok(result)
}

async fn prepare_categories(
    client: &Client,
    request: &WechatRestoreRequest,
    account_dir: &Path,
) -> Result<CategoryPlan, BackupError> {
    let layout = layout_for(request.version);
    let devices = device_folders(client).await?;
    let mut plan = CategoryPlan::default();
    let mut seen = HashSet::new();
    for category in WechatCategory::ALL {
        if !request.categories.contains(&category) || !category.is_file_source() {
            continue;
        }
        let Some(category_layout) = layout.category(category) else {
            continue;
        };
        let destinations = category_destinations(category_layout, account_dir, &request.data_root)?;
        for (leaf, local_base) in destinations {
            for (_, device_id) in &devices {
                let Some(cloud_folder) = find_category_folder(
                    client,
                    *device_id,
                    &request.account_name,
                    category.label(),
                    &leaf,
                )
                .await?
                else {
                    continue;
                };
                for file in collect_cloud_files(client, cloud_folder).await? {
                    let local_path = local_file_path(&local_base, &file.relative_path, &file.name)?;
                    let key = crate::desktop::paths::comparable_path(&local_path);
                    if !seen.insert(key) {
                        continue;
                    }
                    plan.total_bytes = plan.total_bytes.saturating_add(file.size);
                    plan.files.push(RestoreFile {
                        name: file.name,
                        node_id: file.node_id,
                        version_id: file.version_id,
                        size: file.size,
                        local_parent: local_path
                            .parent()
                            .ok_or(BackupError::InvalidConfig("恢复目标路径无效"))?
                            .to_path_buf(),
                    });
                }
            }
        }
    }
    plan.files.sort_by(|left, right| {
        left.local_parent
            .cmp(&right.local_parent)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(plan)
}

async fn prepare_snapshot(
    client: &Client,
    request: &WechatRestoreRequest,
    account_dir: &Path,
    selection: &WechatSnapshotSelection,
) -> Result<SnapshotPlan, BackupError> {
    let layout = layout_for(request.version);
    let database_relative = layout
        .category(WechatCategory::Databases)
        .and_then(|category| category.account_paths.first())
        .ok_or(BackupError::InvalidConfig("微信数据库布局缺失"))?;
    let database_dir = account_dir.join(database_relative);

    let device_id = find_device_folder(client, &selection.device_name)
        .await?
        .ok_or(BackupError::SnapshotNotFound)?;
    let account_id = find_folder(client, device_id, &request.account_name)
        .await?
        .ok_or(BackupError::SnapshotNotFound)?;
    let database_category_id = find_folder(client, account_id, WechatCategory::Databases.label())
        .await?
        .ok_or(BackupError::SnapshotNotFound)?;
    let snapshot_id = find_folder(client, database_category_id, &selection.timestamp)
        .await?
        .ok_or(BackupError::SnapshotNotFound)?;

    let cloud = collect_cloud_files(client, snapshot_id).await?;
    let Some(marker) = cloud
        .iter()
        .find(|file| file.relative_path == SNAPSHOT_DONE_FILE)
    else {
        return Err(BackupError::SnapshotIncomplete);
    };
    let manifest = download_manifest(client, marker).await?;
    let expected_at = parse_timestamp(&selection.timestamp).ok_or(BackupError::SnapshotNotFound)?;
    if manifest.snapshot_time != expected_at {
        return Err(BackupError::SnapshotCorrupted);
    }
    let cloud_files: BTreeMap<&str, &CloudFile> = cloud
        .iter()
        .filter(|file| file.relative_path != SNAPSHOT_DONE_FILE)
        .map(|file| (file.relative_path.as_str(), file))
        .collect();
    if cloud_files.len() != manifest.files.len() {
        return Err(BackupError::SnapshotCorrupted);
    }
    let mut files = Vec::with_capacity(manifest.files.len());
    let mut seen = HashSet::with_capacity(manifest.files.len());
    let mut total_bytes = 0_u64;
    for file in &manifest.files {
        if !seen.insert(file.relative_path.clone()) {
            return Err(BackupError::SnapshotCorrupted);
        }
        let cloud_file = cloud_files
            .get(file.relative_path.as_str())
            .ok_or(BackupError::SnapshotCorrupted)?;
        if cloud_file.size != file.size {
            return Err(BackupError::SnapshotCorrupted);
        }
        total_bytes = total_bytes.saturating_add(file.size);
        files.push(SnapshotFilePlan {
            relative_path: file.relative_path.clone(),
            name: cloud_file.name.clone(),
            node_id: cloud_file.node_id,
            version_id: cloud_file.version_id,
            size: file.size,
            content_sha256: file.content_sha256.clone(),
        });
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(SnapshotPlan {
        database_dir,
        files,
        total_bytes,
    })
}

async fn download_manifest(
    client: &Client,
    marker: &CloudFile,
) -> Result<MarkerManifest, BackupError> {
    if marker.name != SNAPSHOT_DONE_FILE {
        return Err(BackupError::SnapshotCorrupted);
    }
    let guard = TempDirGuard::create().map_err(BackupError::InvalidConfig)?;
    let control = TransferControl::new();
    let outcome = download_file_silent(
        client,
        marker.node_id,
        marker.version_id,
        &marker.name,
        guard.path(),
        ConflictPolicy::Replace,
        &control,
    )
    .await
    .map_err(snapshot_client_error)?;
    let bytes = tokio::fs::read(&outcome.path)
        .await
        .map_err(|_| BackupError::SnapshotCorrupted)?;
    serde_json::from_slice::<MarkerManifest>(&bytes).map_err(|_| BackupError::SnapshotCorrupted)
}

fn category_leaf_names(
    category: &super::layout::WechatCategoryLayout,
    account_name: &str,
) -> Vec<String> {
    let mut names = Vec::new();
    for relative in category.account_paths {
        if let Some(last) = last_segment(relative) {
            names.push(normalize_cloud_name(last));
        }
    }
    for relative in category.data_root_paths {
        if let Some(last) = last_segment(&relative.replace("{account}", account_name)) {
            names.push(normalize_cloud_name(last));
        }
    }
    names
}

fn category_destinations(
    category: &super::layout::WechatCategoryLayout,
    account_dir: &Path,
    data_root: &Path,
) -> Result<Vec<(String, PathBuf)>, BackupError> {
    let mut destinations = Vec::new();
    for relative in category.account_paths {
        let leaf = last_segment(relative).ok_or(BackupError::InvalidConfig("微信来源路径无效"))?;
        destinations.push((normalize_cloud_name(leaf), account_dir.join(relative)));
    }
    for relative in category.data_root_paths {
        let leaf = last_segment(relative).ok_or(BackupError::InvalidConfig("微信来源路径无效"))?;
        destinations.push((
            normalize_cloud_name(leaf),
            data_root.join(
                relative.replace(
                    "{account}",
                    account_dir
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default(),
                ),
            ),
        ));
    }
    Ok(destinations)
}

fn last_segment(relative: &str) -> Option<&str> {
    relative.rsplit('/').find(|part| !part.is_empty())
}

/// 本地完整文件路径（用于按路径去重）；父目录由调用方取 `parent()`。
fn local_file_path(
    local_base: &Path,
    relative_path: &str,
    name: &str,
) -> Result<PathBuf, BackupError> {
    let mut destination = local_base.to_path_buf();
    let mut segments: Vec<&str> = relative_path.split('/').collect();
    let _ = segments
        .pop()
        .ok_or(BackupError::InvalidConfig("恢复文件路径无效"))?;
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        destination.push(sanitize_local_name(segment));
    }
    destination.push(sanitize_local_name(name));
    Ok(destination)
}

fn snapshot_destination(temp_dir: &Path, relative_path: &str) -> Result<PathBuf, BackupError> {
    let mut destination = temp_dir.to_path_buf();
    let mut segments: Vec<&str> = relative_path.split('/').collect();
    let name = segments
        .pop()
        .ok_or(BackupError::InvalidConfig("快照文件路径无效"))?;
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        destination.push(sanitize_local_name(segment));
    }
    destination.push(sanitize_local_name(name));
    Ok(destination)
}

async fn verify_snapshot_file(path: &Path, expected: &SnapshotFilePlan) -> Result<(), BackupError> {
    let path = path.to_path_buf();
    let expected_size = expected.size;
    let expected_hash = expected.content_sha256.clone();
    let result = tokio::task::spawn_blocking(move || {
        let path = path_for_io(&path).map_err(|_| BackupError::SnapshotCorrupted)?;
        let mut file = std::fs::File::open(path).map_err(|_| BackupError::SnapshotCorrupted)?;
        let mut hasher = ContentHasher::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut size = 0_u64;
        loop {
            let read = std::io::Read::read(&mut file, &mut buffer)
                .map_err(|_| BackupError::SnapshotCorrupted)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            size = size.saturating_add(read as u64);
        }
        Ok::<_, BackupError>((size, hasher.finish_hex()))
    })
    .await
    .map_err(|_| BackupError::SnapshotCorrupted)??;
    let (size, hash) = result;
    if size != expected_size || !Client::hashes_equal(&hash, &expected_hash) {
        return Err(BackupError::SnapshotCorrupted);
    }
    Ok(())
}

fn snapshot_client_error(error: ClientError) -> BackupError {
    match error {
        ClientError::Integrity(_) | ClientError::Crypto(_) => BackupError::SnapshotCorrupted,
        error => BackupError::Client(error),
    }
}

fn restore_failure(error: &BackupError) -> TransferFailure {
    match error {
        BackupError::WechatRunning => TransferFailure {
            code: TransferErrorCode::WechatRunning,
            message: error.to_string(),
        },
        BackupError::SnapshotIncomplete => TransferFailure {
            code: TransferErrorCode::SnapshotIncomplete,
            message: error.to_string(),
        },
        BackupError::SnapshotCorrupted => TransferFailure {
            code: TransferErrorCode::SnapshotCorrupted,
            message: error.to_string(),
        },
        BackupError::SnapshotNotFound => TransferFailure {
            code: TransferErrorCode::SnapshotNotFound,
            message: error.to_string(),
        },
        BackupError::Client(error) => failure(error),
        other => TransferFailure {
            code: TransferErrorCode::Other,
            message: other.to_string(),
        },
    }
}

// ---------------------------------------------------------------------------
// 云端遍历
// ---------------------------------------------------------------------------

async fn root_folder(client: &Client) -> Result<Uuid, BackupError> {
    Ok(client.ensure_writable()?.root_node_id)
}

async fn wechat_root(client: &Client) -> Result<Option<Uuid>, BackupError> {
    let root = root_folder(client).await?;
    find_folder(client, root, WECHAT_CLOUD_ROOT).await
}

async fn device_folders(client: &Client) -> Result<Vec<(String, Uuid)>, BackupError> {
    let Some(root) = wechat_root(client).await? else {
        return Ok(Vec::new());
    };
    let mut devices = client
        .list_folder_decrypted(root)
        .await?
        .into_iter()
        .filter(|node| node.kind() == NodeKind::Folder)
        .map(|node| (node.name().to_owned(), node.view.id))
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(devices)
}

async fn find_device_folder(
    client: &Client,
    device_name: &str,
) -> Result<Option<Uuid>, BackupError> {
    let key = name_key(device_name);
    Ok(device_folders(client)
        .await?
        .into_iter()
        .find(|(name, _)| name_key(name) == key)
        .map(|(_, id)| id))
}

async fn find_folder(
    client: &Client,
    parent: Uuid,
    name: &str,
) -> Result<Option<Uuid>, BackupError> {
    let key = name_key(name);
    Ok(client
        .list_folder_decrypted(parent)
        .await?
        .into_iter()
        .find(|node| node.kind() == NodeKind::Folder && name_key(node.name()) == key)
        .map(|node| node.view.id))
}

async fn find_category_folder(
    client: &Client,
    device_id: Uuid,
    account_name: &str,
    category_label: &str,
    leaf: &str,
) -> Result<Option<Uuid>, BackupError> {
    let Some(account_id) = find_folder(client, device_id, account_name).await? else {
        return Ok(None);
    };
    let Some(category_id) = find_folder(client, account_id, category_label).await? else {
        return Ok(None);
    };
    find_folder(client, category_id, leaf).await
}

async fn snapshot_locations(
    client: &Client,
    account_name: &str,
) -> Result<Vec<SnapshotLocation>, BackupError> {
    let mut locations = Vec::new();
    for (device_name, device_id) in device_folders(client).await? {
        let Some(account_id) = find_folder(client, device_id, account_name).await? else {
            continue;
        };
        let Some(database_id) =
            find_folder(client, account_id, WechatCategory::Databases.label()).await?
        else {
            continue;
        };
        for node in client.list_folder_decrypted(database_id).await? {
            if node.kind() != NodeKind::Folder {
                continue;
            }
            let Some(snapshot_at) = parse_timestamp(node.name()) else {
                continue;
            };
            locations.push(SnapshotLocation {
                device_name: device_name.clone(),
                timestamp: node.name().to_owned(),
                snapshot_at,
                node_id: node.view.id,
            });
        }
    }
    Ok(locations)
}

fn collect_cloud_files<'a>(
    client: &'a Client,
    folder_id: Uuid,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Vec<CloudFile>, BackupError>> + Send + 'a>,
> {
    Box::pin(async move {
        let mut files = Vec::new();
        collect_cloud_files_inner(client, folder_id, "", &mut files).await?;
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(files)
    })
}

fn collect_cloud_files_inner<'a>(
    client: &'a Client,
    folder_id: Uuid,
    prefix: &'a str,
    output: &'a mut Vec<CloudFile>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), BackupError>> + Send + 'a>> {
    Box::pin(async move {
        for node in client.list_folder_decrypted(folder_id).await? {
            let sanitized = sanitize_local_name(node.name());
            let relative = if prefix.is_empty() {
                sanitized.clone()
            } else {
                format!("{prefix}/{sanitized}")
            };
            match node.kind() {
                NodeKind::Folder => {
                    Box::pin(collect_cloud_files_inner(
                        client,
                        node.view.id,
                        &relative,
                        output,
                    ))
                    .await?;
                }
                NodeKind::File => {
                    let version_id = node
                        .view
                        .current_version_id
                        .ok_or(BackupError::SnapshotCorrupted)?;
                    output.push(CloudFile {
                        relative_path: relative,
                        name: sanitized,
                        node_id: node.view.id,
                        version_id,
                        size: node.view.size.unwrap_or_default(),
                    });
                }
            }
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// 本地文件系统
// ---------------------------------------------------------------------------

fn next_backup_directory(database_dir: &Path, now: SystemTime) -> PathBuf {
    let name = database_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("database");
    let suffix = restore_time::compact_now(now);
    for attempt in 0..=MAX_BACKUP_NAME_SUFFIX {
        let candidate_name = if attempt == 0 {
            format!("{name}.before-restore-{suffix}")
        } else {
            format!("{name}.before-restore-{suffix}-{attempt}")
        };
        let candidate = database_dir
            .parent()
            .map(|parent| parent.join(&candidate_name))
            .unwrap_or_else(|| PathBuf::from(&candidate_name));
        if !path_for_io(&candidate).is_ok_and(|path| path.exists()) {
            return candidate;
        }
    }
    // 极端情况下沿用第一个候选；rename 会失败并触发回滚，不会丢数据。
    database_dir.with_file_name(format!("{name}.before-restore-{suffix}"))
}

fn rename(from: &Path, to: &Path) -> Result<(), BackupError> {
    let from = path_for_io(from).map_err(|_| BackupError::InvalidConfig("恢复路径无效"))?;
    let to = path_for_io(to).map_err(|_| BackupError::InvalidConfig("恢复路径无效"))?;
    std::fs::rename(from, to).map_err(|error| BackupError::Client(ClientError::Io(error)))
}

/// 把已验证的临时目录提交为数据库目录；旧目录先改名保留，失败时改回。
fn commit_database(
    temp_dir: &Path,
    database_dir: &Path,
    backup_dir: &Path,
    existing: bool,
) -> Result<(), BackupError> {
    if existing {
        rename(database_dir, backup_dir)?;
    }
    if let Err(error) = rename(temp_dir, database_dir) {
        if existing && rename(backup_dir, database_dir).is_err() {
            tracing::warn!("微信数据库恢复回滚失败，旧数据库仍保留在 before-restore 目录");
        }
        let _ = remove_dir_all(temp_dir);
        return Err(error);
    }
    Ok(())
}

fn remove_dir_all(path: &Path) -> std::io::Result<()> {
    let Ok(path) = path_for_io(path) else {
        return Ok(());
    };
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// 系统临时目录（用于下载 `快照完成` 清单，绝不写微信目录）。
struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn create() -> Result<Self, &'static str> {
        let path =
            std::env::temp_dir().join(format!("backup-wechat-restore-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&path).map_err(|_| "无法创建恢复临时目录")?;
        Ok(Self { path })
    }

    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn compact_timestamp_matches_database_suffix_format() {
        assert_eq!(
            format_compact_timestamp_for_test(1_793_547_000),
            "20261101-153000"
        );
        assert_eq!(format_compact_timestamp_for_test(0), "19700101-000000");
    }

    fn format_compact_timestamp_for_test(seconds: i64) -> String {
        super::super::snapshot::format_compact_timestamp(seconds)
    }

    #[test]
    fn next_backup_directory_avoids_existing_names() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("db_storage");
        std::fs::create_dir_all(&database).unwrap();
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_793_547_000);
        let first = next_backup_directory(&database, now);
        let name = first.file_name().and_then(|name| name.to_str()).unwrap();
        assert!(
            name.starts_with("db_storage.before-restore-"),
            "unexpected backup name: {name}"
        );
        std::fs::create_dir_all(&first).unwrap();
        let second = next_backup_directory(&database, now);
        assert_ne!(first, second);
    }

    #[test]
    fn snapshot_destination_preserves_relative_structure() {
        let temp = Path::new("C:/tmp/restore");
        let path = snapshot_destination(temp, "message/message_0.db").unwrap();
        assert!(path.ends_with("message/message_0.db"));
        let nested = snapshot_destination(temp, "MMKV/mmkv.bin").unwrap();
        assert!(nested.ends_with("MMKV/mmkv.bin"));
    }

    #[test]
    fn commit_database_rolls_back_when_temp_rename_fails() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("db_storage");
        std::fs::create_dir_all(database.join("message")).unwrap();
        std::fs::write(database.join("message/message_0.db"), b"old").unwrap();
        let backup = dir.path().join("db_storage.before-restore-test");
        let missing_temp = dir.path().join("missing-temp");
        let error = commit_database(&missing_temp, &database, &backup, true).unwrap_err();
        assert!(matches!(error, BackupError::Client(_)));
        assert_eq!(
            std::fs::read(database.join("message/message_0.db")).unwrap(),
            b"old",
            "提交失败必须把旧目录改回原名"
        );
        assert!(!backup.exists());
    }
}
