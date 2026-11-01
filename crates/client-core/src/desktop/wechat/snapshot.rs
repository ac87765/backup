//! 聊天记录数据库快照（add-wechat-backup 第 3 组，design D3）。
//!
//! 只在微信未运行时，把整个数据库目录（4.x 的 `db_storage/`、3.x 的 `Msg/`）
//! 原样上传到一个带 UTC 时间戳的云端目录，全部文件完成后写入 `快照完成` 标记。
//! 快照不解析、不解密任何数据库内容；读取一律共享读，不修改源文件。
//!
//! 一致性控制：
//! - 开始前记录整个目录的清单（相对路径、大小、修改时间）；
//! - 上传每个文件前检查微信进程与文件指纹，上传过程中按已确认分块检查进程；
//! - 上传完成后、写标记前再次核对整个清单（任何文件变化都放弃）；
//! - 放弃时把本次时间戳目录移入回收站（不彻底删除）。
//!
//! 内容未变跳过：先用本地缓存比较大小/修改时间（不读文件），不一致时逐文件
//! 计算 SHA-256，再与**上一份完整快照的云端节点元数据**常量时间比较。
//!
//! 云端目录始终是权威来源：完整快照、保留份数与不完整快照清理都按时间戳目录名
//! 解析后以云端列表为准；本地状态库只是缓存。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Client;
use crate::clock::Clock;
use crate::error::ClientError;
use crate::io_path::{FileFingerprint, normalize_cloud_name, path_for_io};
use crate::model::{NodeKind, UploadOutcome};
use crate::state::StateStore;
use crate::transfer::{TokenBucket, UploadTask};

use super::super::error::BackupError;
use super::super::filter::{hash_file_stable, name_key};
use super::super::paths::ensure_cloud_folder;
use super::super::types::{
    WECHAT_SNAPSHOT_RETENTION_MAX, WECHAT_SNAPSHOT_RETENTION_MIN, WechatSnapshotOutcome,
    WechatSnapshotReport,
};
use super::layout::{WechatCategory, WechatVersion, layout_for};
use super::process::WechatProcessProbe;

/// 完整快照的标记文件名。清单 JSON 的内容就写在这个文件里（加密上传）。
pub(crate) const SNAPSHOT_DONE_FILE: &str = "快照完成";
/// 云端 `微信备份` 根目录名。
const WECHAT_CLOUD_ROOT: &str = "微信备份";
/// 上传时每 N 个已确认分块检查一次微信进程（上传任务一批最多确认 32 块）。
const PROBE_CHECK_CHUNKS: u32 = 8;
/// 生成时间戳目录名时，若冲突则向后寻找的秒数上限。
const MAX_TIMESTAMP_SHIFT: i64 = 120;

/// 一次快照涉及的账号信息（只含本地路径与版本，不含任何聊天内容）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotAccount {
    pub data_root: PathBuf,
    pub account_name: String,
    pub version: WechatVersion,
}

impl SnapshotAccount {
    /// 数据库目录：版本布局描述中的“聊天记录数据库”来源。
    fn database_directory(&self) -> Result<PathBuf, BackupError> {
        let layout = layout_for(self.version);
        let category = layout
            .category(WechatCategory::Databases)
            .ok_or(BackupError::InvalidConfig("微信数据库布局缺失"))?;
        let relative = category
            .account_paths
            .first()
            .ok_or(BackupError::InvalidConfig("微信数据库路径缺失"))?;
        Ok(self.data_root.join(&self.account_name).join(relative))
    }
}

/// 快照运行所需的引擎能力（与 `BackupEngine` 解耦，便于单元测试）。
pub(crate) struct SnapshotContext {
    pub client: Client,
    pub clock: std::sync::Arc<dyn Clock>,
    pub state: std::sync::Arc<StateStore>,
    pub device_name: String,
    pub probe: std::sync::Arc<dyn WechatProcessProbe>,
    pub upload_limit_bytes_per_second: Option<u64>,
    pub retention: u8,
}

/// 一个本地数据库文件（尚未计算内容摘要）。
#[derive(Debug, Clone)]
struct ScannedFile {
    relative_path: String,
    path: PathBuf,
    size: u64,
    mtime_ns: i64,
}

/// 计算摘要后的清单项。
#[derive(Debug, Clone)]
struct SnapshotFile {
    relative_path: String,
    path: PathBuf,
    size: u64,
    mtime_ns: i64,
    content_sha256: String,
}

/// 云端 `快照完成` 文件里的清单（不含修改时间与本地路径）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct MarkerManifest {
    /// 快照时间（Unix 秒），与所在时间戳目录名一致。
    pub snapshot_time: i64,
    pub files: Vec<MarkerFile>,
}

/// 清单中的一个文件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct MarkerFile {
    /// 相对数据库目录的路径（`/` 分隔、NFC）。
    pub relative_path: String,
    pub size: u64,
    pub content_sha256: String,
}

/// 本地状态库缓存的清单（额外保存修改时间，用于不读文件的快速跳过）。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedManifest {
    snapshot_time: i64,
    files: Vec<CachedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedFile {
    relative_path: String,
    size: u64,
    mtime_ns: i64,
    content_sha256: String,
}

/// 一个云端时间戳目录。
#[derive(Debug, Clone)]
struct SnapshotDir {
    name: String,
    node_id: Uuid,
    at: i64,
    complete: bool,
}

/// 上传阶段的中止原因。
#[derive(Debug)]
enum UploadAbort {
    /// 微信启动了（探测到运行中）。
    WechatStarted,
    /// 文件在上传过程中被修改。
    FileChanged,
    /// 其他错误（网络、服务端、配额等）。
    Failed(BackupError),
}

/// 执行一次账号的数据库快照。`progress(done, total)` 用于界面显示上传进度。
///
/// 不修改微信目录；所有读取都是共享读。返回结果不会包含任何路径或文件名。
pub(crate) async fn run_snapshot(
    ctx: &SnapshotContext,
    account: &SnapshotAccount,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<WechatSnapshotReport, BackupError> {
    let user_id = ctx.client.ensure_writable()?.user_id;
    if ctx.probe.is_running(account.version) {
        return Ok(report(WechatSnapshotOutcome::WaitingForWechatExit, None, 0));
    }
    let database_dir = account.database_directory()?;
    if !path_for_io(&database_dir).is_ok_and(|path| path.is_dir()) {
        // 该账号没有数据库目录：没有可备份的内容，不创建空快照。
        return Ok(report(WechatSnapshotOutcome::Unchanged, None, 0));
    }

    let scanned = scan_manifest_blocking(database_dir.clone()).await?;
    if scanned.is_empty() {
        return Ok(report(WechatSnapshotOutcome::Unchanged, None, 0));
    }

    let cached = ctx
        .state
        .wechat_snapshot(
            user_id,
            data_root_text(&account.data_root),
            account.account_name.clone(),
        )
        .await?;

    // 本地快速路径：大小 / 修改时间与本地缓存的上一份完整快照全部一致时，
    // 不读文件、不发任何云端请求。云端目录仍是保留/清理的权威来源，但它们
    // 由独立的低频维护流程处理。
    if let Some(cached) = &cached
        && cache_matches(cached, &scanned)
    {
        return Ok(report(
            WechatSnapshotOutcome::Unchanged,
            Some(cached.snapshot_at),
            scanned.len() as u64,
        ));
    }

    // 计算摘要前再检查一次（目录遍历可能耗时）。
    if ctx.probe.is_running(account.version) {
        return Ok(report(
            WechatSnapshotOutcome::WaitingForWechatExit,
            cached.as_ref().map(|record| record.snapshot_at),
            scanned.len() as u64,
        ));
    }
    let files = match hash_manifest(ctx, account, scanned).await? {
        HashOutcome::Files(files) => files,
        HashOutcome::Waiting => {
            return Ok(report(
                WechatSnapshotOutcome::WaitingForWechatExit,
                cached.as_ref().map(|record| record.snapshot_at),
                0,
            ));
        }
        HashOutcome::Changed => {
            return Ok(report(
                WechatSnapshotOutcome::Aborted,
                cached.as_ref().map(|record| record.snapshot_at),
                0,
            ));
        }
    };

    // 只有本地清单变化或没有本地缓存时才访问云端。
    let parent = ensure_snapshot_parent(ctx, account).await?;
    let mut directories = list_snapshot_dirs(ctx, parent).await?;
    // 不完整快照（没有 `快照完成`）在下一次快照开始前移入回收站。清理失败时
    // 不继续上传，避免与同名时间戳目录混用；下一次检查会重试。
    for directory in directories.iter().filter(|dir| !dir.complete) {
        if let Err(error) = ctx.client.delete_to_trash(directory.node_id).await {
            return Err(BackupError::Client(error));
        }
    }
    directories.retain(|dir| dir.complete);
    sort_directories(&mut directories);
    let latest = directories.first().cloned();

    // 与上一份完整快照的云端节点元数据逐文件比较（常量时间比较摘要）。
    if let Some(latest) = &latest {
        let remote = list_snapshot_files(&ctx.client, latest.node_id).await?;
        if same_content(&files, &remote) {
            cache_snapshot(ctx, user_id, account, latest.at, latest.node_id, &files).await?;
            return Ok(report(
                WechatSnapshotOutcome::Unchanged,
                Some(latest.at),
                files.len() as u64,
            ));
        }
    }

    if ctx.probe.is_running(account.version) {
        return Ok(report(
            WechatSnapshotOutcome::WaitingForWechatExit,
            latest.as_ref().map(|dir| dir.at),
            files.len() as u64,
        ));
    }

    let baseline = unix_seconds(ctx.clock.now()).max(
        latest
            .as_ref()
            .map(|directory| directory.at.saturating_add(1))
            .unwrap_or(i64::MIN),
    );
    let Some((snapshot_at, directory_name)) = next_timestamp(&directories, baseline) else {
        return Err(BackupError::InvalidConfig("微信快照时间戳目录不可用"));
    };
    let directory_id = ensure_cloud_folder(&ctx.client, parent, &directory_name).await?;
    let limiter = TokenBucket::new(ctx.upload_limit_bytes_per_second.unwrap_or(0));
    let total = files.len() as u64;
    let mut uploaded_files = 0_u64;
    let mut uploaded_bytes = 0_u64;

    let result = upload_all(
        ctx,
        account,
        directory_id,
        &files,
        limiter,
        &mut uploaded_files,
        &mut uploaded_bytes,
        progress,
    )
    .await;

    match result {
        Ok(()) => {}
        Err(UploadAbort::WechatStarted) => {
            discard_snapshot(ctx, directory_id).await;
            return Ok(report(
                WechatSnapshotOutcome::WaitingForWechatExit,
                latest.as_ref().map(|dir| dir.at),
                total,
            ));
        }
        Err(UploadAbort::FileChanged) => {
            discard_snapshot(ctx, directory_id).await;
            return Ok(report(
                WechatSnapshotOutcome::Aborted,
                latest.as_ref().map(|dir| dir.at),
                total,
            ));
        }
        Err(UploadAbort::Failed(error)) => {
            discard_snapshot(ctx, directory_id).await;
            return Err(error);
        }
    }

    // 全部文件上传完成后，再核对一次整个清单，任何变化都放弃。
    let after = match scan_manifest_blocking(database_dir.clone()).await {
        Ok(after) => after,
        Err(error) => {
            discard_snapshot(ctx, directory_id).await;
            return Err(error);
        }
    };
    if !same_fingerprints(&files, &after) {
        discard_snapshot(ctx, directory_id).await;
        return Ok(report(
            WechatSnapshotOutcome::Aborted,
            latest.as_ref().map(|dir| dir.at),
            total,
        ));
    }
    if ctx.probe.is_running(account.version) {
        discard_snapshot(ctx, directory_id).await;
        return Ok(report(
            WechatSnapshotOutcome::WaitingForWechatExit,
            latest.as_ref().map(|dir| dir.at),
            total,
        ));
    }

    // 写 `快照完成` 标记；有这个文件的快照才算完整、才可用于恢复。
    match write_marker(ctx, account, directory_id, snapshot_at, &files).await {
        Ok(()) => {}
        Err(UploadAbort::WechatStarted) => {
            discard_snapshot(ctx, directory_id).await;
            return Ok(report(
                WechatSnapshotOutcome::WaitingForWechatExit,
                latest.as_ref().map(|dir| dir.at),
                total,
            ));
        }
        Err(UploadAbort::FileChanged) => {
            discard_snapshot(ctx, directory_id).await;
            return Ok(report(
                WechatSnapshotOutcome::Aborted,
                latest.as_ref().map(|dir| dir.at),
                total,
            ));
        }
        Err(UploadAbort::Failed(error)) => {
            discard_snapshot(ctx, directory_id).await;
            return Err(error);
        }
    }
    // 标记写入期间如果微信启动，仍按放弃处理（不能把可能变化的数据库当成完整）。
    if ctx.probe.is_running(account.version) {
        discard_snapshot(ctx, directory_id).await;
        return Ok(report(
            WechatSnapshotOutcome::WaitingForWechatExit,
            latest.as_ref().map(|dir| dir.at),
            total,
        ));
    }
    cache_snapshot(ctx, user_id, account, snapshot_at, directory_id, &files).await?;

    // 保留最近 N 份完整快照；更早的移入回收站。清理失败不影响本次已完成的
    // 快照（下次快照或设置变化时仍会重试），只记录不包含路径的警告。
    if enforce_retention(ctx, parent, ctx.retention).await.is_err() {
        tracing::warn!("微信数据库快照保留清理失败，稍后重试");
    }

    Ok(WechatSnapshotReport {
        outcome: WechatSnapshotOutcome::Completed,
        snapshot_at: Some(snapshot_at),
        total_files: total,
        uploaded_files,
        uploaded_bytes,
    })
}

/// 只清理不完整快照与超出保留份数的完整快照；用于保留设置变化时立即生效。
///
/// 云端快照目录不存在时什么也不做（不创建空目录）。
pub(crate) async fn maintain_snapshots(
    ctx: &SnapshotContext,
    account: &SnapshotAccount,
) -> Result<(), BackupError> {
    let Some(parent) = find_snapshot_parent(ctx, account).await? else {
        return Ok(());
    };
    let directories = list_snapshot_dirs(ctx, parent).await?;
    for directory in directories.iter().filter(|dir| !dir.complete) {
        discard_snapshot(ctx, directory.node_id).await;
    }
    enforce_retention(ctx, parent, ctx.retention).await?;
    Ok(())
}

fn report(
    outcome: WechatSnapshotOutcome,
    snapshot_at: Option<i64>,
    total_files: u64,
) -> WechatSnapshotReport {
    WechatSnapshotReport {
        outcome,
        snapshot_at,
        total_files,
        uploaded_files: 0,
        uploaded_bytes: 0,
    }
}

fn data_root_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// 在阻塞线程中遍历数据库目录，避免大目录阻塞异步运行时。
async fn scan_manifest_blocking(database_dir: PathBuf) -> Result<Vec<ScannedFile>, BackupError> {
    tokio::task::spawn_blocking(move || scan_manifest(&database_dir))
        .await
        .map_err(|_| BackupError::InvalidConfig("微信数据库扫描任务失败"))?
}

/// 遍历数据库目录，记录全部普通文件（包含隐藏文件；不跟随符号链接）。
fn scan_manifest(database_dir: &Path) -> Result<Vec<ScannedFile>, BackupError> {
    let mut files = Vec::new();
    scan_directory(database_dir, "", &mut files)?;
    let mut seen = HashSet::with_capacity(files.len());
    for file in &files {
        if !seen.insert(file.relative_path.clone()) {
            return Err(BackupError::InvalidConfig("微信数据库文件名冲突"));
        }
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn scan_directory(
    directory: &Path,
    prefix: &str,
    files: &mut Vec<ScannedFile>,
) -> Result<(), BackupError> {
    let io_path =
        path_for_io(directory).map_err(|_| BackupError::InvalidConfig("微信数据库路径无效"))?;
    let entries = std::fs::read_dir(&io_path)
        .map_err(|_| BackupError::InvalidConfig("微信数据库无法读取"))?;
    let mut children = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| BackupError::InvalidConfig("微信数据库无法读取"))?;
        children.push(entry);
    }
    children.sort_by_key(|entry| entry.file_name());

    for entry in children {
        let file_type = entry
            .file_type()
            .map_err(|_| BackupError::InvalidConfig("微信数据库无法读取"))?;
        if file_type.is_symlink() {
            // 不跟随符号链接：避免把数据库目录之外的文件带进快照。
            continue;
        }
        let raw_name = entry
            .file_name()
            .into_string()
            .map_err(|_| BackupError::InvalidConfig("微信数据库文件名无效"))?;
        let name = normalize_cloud_name(&raw_name);
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let path = directory.join(&raw_name);
        if file_type.is_dir() {
            scan_directory(&path, &relative, files)?;
        } else if file_type.is_file() {
            let metadata = entry
                .metadata()
                .map_err(|_| BackupError::InvalidConfig("微信数据库无法读取"))?;
            files.push(ScannedFile {
                relative_path: relative,
                path,
                size: metadata.len(),
                mtime_ns: crate::desktop::filter::mtime_ns(&metadata),
            });
        }
    }
    Ok(())
}

enum HashOutcome {
    Files(Vec<SnapshotFile>),
    Waiting,
    Changed,
}

async fn hash_manifest(
    ctx: &SnapshotContext,
    account: &SnapshotAccount,
    scanned: Vec<ScannedFile>,
) -> Result<HashOutcome, BackupError> {
    let mut files = Vec::with_capacity(scanned.len());
    for file in scanned {
        if ctx.probe.is_running(account.version) {
            return Ok(HashOutcome::Waiting);
        }
        let expected = FileFingerprint {
            size: file.size,
            mtime_ns: file.mtime_ns,
        };
        let path = file.path.clone();
        let hash = tokio::task::spawn_blocking(move || hash_file_stable(&path, expected))
            .await
            .map_err(|_| BackupError::InvalidConfig("微信数据库哈希任务失败"))?
            .map_err(BackupError::Client)?;
        let Some(hash) = hash else {
            return Ok(HashOutcome::Changed);
        };
        files.push(SnapshotFile {
            relative_path: file.relative_path,
            path: file.path,
            size: file.size,
            mtime_ns: file.mtime_ns,
            content_sha256: hash,
        });
    }
    Ok(HashOutcome::Files(files))
}

fn same_fingerprints(files: &[SnapshotFile], scanned: &[ScannedFile]) -> bool {
    files.len() == scanned.len()
        && files.iter().zip(scanned).all(|(left, right)| {
            left.relative_path == right.relative_path
                && left.size == right.size
                && left.mtime_ns == right.mtime_ns
        })
}

/// 上一份完整快照的云端文件（只用展示元数据里的摘要）。
#[derive(Debug, Clone)]
struct RemoteFile {
    content_sha256: Option<String>,
}

fn same_content(files: &[SnapshotFile], remote: &BTreeMap<String, RemoteFile>) -> bool {
    if files.len() != remote.len() {
        return false;
    }
    files.iter().all(|file| {
        remote.get(&file.relative_path).is_some_and(|remote| {
            remote
                .content_sha256
                .as_deref()
                .is_some_and(|hash| Client::hashes_equal(&file.content_sha256, hash))
        })
    })
}

fn cache_matches(cached: &crate::state::WechatSnapshotRecord, scanned: &[ScannedFile]) -> bool {
    let Ok(manifest) = serde_json::from_str::<CachedManifest>(&cached.manifest_json) else {
        return false;
    };
    manifest.snapshot_time == cached.snapshot_at
        && manifest.files.len() == scanned.len()
        && manifest.files.iter().zip(scanned).all(|(cached, local)| {
            cached.relative_path == local.relative_path
                && cached.size == local.size
                && cached.mtime_ns == local.mtime_ns
        })
}

async fn cache_snapshot(
    ctx: &SnapshotContext,
    user_id: Uuid,
    account: &SnapshotAccount,
    snapshot_at: i64,
    cloud_dir_node_id: Uuid,
    files: &[SnapshotFile],
) -> Result<(), BackupError> {
    let manifest = CachedManifest {
        snapshot_time: snapshot_at,
        files: files
            .iter()
            .map(|file| CachedFile {
                relative_path: file.relative_path.clone(),
                size: file.size,
                mtime_ns: file.mtime_ns,
                content_sha256: file.content_sha256.clone(),
            })
            .collect(),
    };
    let manifest_json = serde_json::to_string(&manifest)
        .map_err(|_| BackupError::InvalidConfig("微信快照清单序列化失败"))?;
    ctx.state
        .upsert_wechat_snapshot(crate::state::WechatSnapshotRecord {
            user_id,
            data_root: data_root_text(&account.data_root),
            account_name: account.account_name.clone(),
            snapshot_at,
            cloud_dir_node_id,
            manifest_json,
            updated_at: unix_seconds(ctx.clock.now()),
        })
        .await?;
    Ok(())
}

/// 云端快照根目录：`微信备份/<设备名>/<账号目录名>/聊天记录数据库`
/// （账号根节点在界面上显示为“全部文件”）。
async fn ensure_snapshot_parent(
    ctx: &SnapshotContext,
    account: &SnapshotAccount,
) -> Result<Uuid, BackupError> {
    let mut parent = ctx.client.ensure_writable()?.root_node_id;
    for name in snapshot_path_names(ctx, account) {
        parent = ensure_cloud_folder(&ctx.client, parent, &name).await?;
    }
    Ok(parent)
}

/// 已存在的快照根目录；不存在时返回 `None`（不创建）。
async fn find_snapshot_parent(
    ctx: &SnapshotContext,
    account: &SnapshotAccount,
) -> Result<Option<Uuid>, BackupError> {
    let mut parent = ctx.client.ensure_writable()?.root_node_id;
    for name in snapshot_path_names(ctx, account) {
        let key = name_key(&name);
        let nodes = ctx.client.list_folder_decrypted(parent).await?;
        let Some(node) = nodes
            .iter()
            .find(|node| node.kind() == NodeKind::Folder && name_key(node.name()) == key)
        else {
            return Ok(None);
        };
        parent = node.view.id;
    }
    Ok(Some(parent))
}

fn snapshot_path_names(ctx: &SnapshotContext, account: &SnapshotAccount) -> Vec<String> {
    vec![
        WECHAT_CLOUD_ROOT.to_owned(),
        ctx.device_name.clone(),
        account.account_name.clone(),
        WechatCategory::Databases.label().to_owned(),
    ]
}

/// 列出快照根目录下名称可解析的时间戳目录，并逐个检查是否有 `快照完成`。
///
/// 名称无法解析的子目录完全不动（可能是用户自己的目录）。
async fn list_snapshot_dirs(
    ctx: &SnapshotContext,
    parent: Uuid,
) -> Result<Vec<SnapshotDir>, BackupError> {
    let nodes = ctx.client.list_folder_decrypted(parent).await?;
    let mut directories = Vec::new();
    for node in nodes {
        if node.kind() != NodeKind::Folder {
            continue;
        }
        let Some(at) = parse_timestamp(node.name()) else {
            continue;
        };
        let complete = ctx
            .client
            .list_folder_decrypted(node.view.id)
            .await?
            .iter()
            .any(|child| child.kind() == NodeKind::File && child.name() == SNAPSHOT_DONE_FILE);
        directories.push(SnapshotDir {
            name: node.name().to_owned(),
            node_id: node.view.id,
            at,
            complete,
        });
    }
    sort_directories(&mut directories);
    Ok(directories)
}

fn sort_directories(directories: &mut [SnapshotDir]) {
    directories.sort_by(|left, right| {
        right
            .at
            .cmp(&left.at)
            .then_with(|| right.name.cmp(&left.name))
    });
}

/// 列出一份完整快照里的全部文件（不含 `快照完成`），用于内容比较。
async fn list_snapshot_files(
    client: &Client,
    root: Uuid,
) -> Result<BTreeMap<String, RemoteFile>, BackupError> {
    let mut output = BTreeMap::new();
    list_files_recursive(client, root, "", &mut output).await?;
    Ok(output)
}

async fn list_files_recursive(
    client: &Client,
    folder_id: Uuid,
    prefix: &str,
    output: &mut BTreeMap<String, RemoteFile>,
) -> Result<(), BackupError> {
    for node in client.list_folder_decrypted(folder_id).await? {
        let relative = if prefix.is_empty() {
            node.name().to_owned()
        } else {
            format!("{prefix}/{}", node.name())
        };
        match node.kind() {
            NodeKind::Folder => {
                Box::pin(list_files_recursive(
                    client,
                    node.view.id,
                    &relative,
                    output,
                ))
                .await?;
            }
            NodeKind::File if prefix.is_empty() && node.name() == SNAPSHOT_DONE_FILE => {}
            NodeKind::File => {
                output.insert(
                    relative,
                    RemoteFile {
                        content_sha256: node.content_sha256.clone(),
                    },
                );
            }
        }
    }
    Ok(())
}

/// 把本次时间戳目录移入回收站。失败不掩盖原始错误，下次快照前还会再清理。
async fn discard_snapshot(ctx: &SnapshotContext, directory_id: Uuid) {
    if ctx.client.delete_to_trash(directory_id).await.is_err() {
        // 不记录路径、账号目录名或文件名。
        tracing::warn!("微信数据库快照放弃时移入回收站失败，下次快照前会重试清理");
    }
}

#[allow(clippy::too_many_arguments)]
async fn upload_all(
    ctx: &SnapshotContext,
    account: &SnapshotAccount,
    directory_id: Uuid,
    files: &[SnapshotFile],
    limiter: Option<Arc<TokenBucket>>,
    uploaded_files: &mut u64,
    uploaded_bytes: &mut u64,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), UploadAbort> {
    let total = files.len() as u64;
    // 同一快照内缓存已解析/创建的云端子目录，避免每个文件重复列出同一目录。
    let mut folders: HashMap<String, Uuid> = HashMap::new();
    for (index, file) in files.iter().enumerate() {
        if ctx.probe.is_running(account.version) {
            return Err(UploadAbort::WechatStarted);
        }
        let expected = FileFingerprint {
            size: file.size,
            mtime_ns: file.mtime_ns,
        };
        let current = tokio::task::spawn_blocking({
            let path = file.path.clone();
            move || FileFingerprint::of_path(&path)
        })
        .await
        .map_err(|_| UploadAbort::Failed(BackupError::InvalidConfig("微信数据库文件检查失败")))?
        .map_err(|_| UploadAbort::Failed(BackupError::InvalidConfig("微信数据库文件无法读取")))?;
        if current.changed_from(expected) {
            return Err(UploadAbort::FileChanged);
        }

        let parent = ensure_relative_folder(ctx, directory_id, &file.relative_path, &mut folders)
            .await
            .map_err(UploadAbort::Failed)?;
        let task = ctx
            .client
            .upload_file_with_limiter(parent, file.path.clone(), limiter.clone())
            .map_err(|error| UploadAbort::Failed(BackupError::Client(error)))?;
        match upload_with_probe(ctx, account.version, &task).await {
            Ok(_) => {}
            Err(abort) => return Err(abort),
        }
        *uploaded_files = uploaded_files.saturating_add(1);
        *uploaded_bytes = uploaded_bytes.saturating_add(file.size);
        progress((index + 1) as u64, total);
    }
    Ok(())
}

/// 在文件所在的子目录下创建目录结构（保留数据库目录内的相对路径）。
async fn ensure_relative_folder(
    ctx: &SnapshotContext,
    root: Uuid,
    relative_path: &str,
    folders: &mut HashMap<String, Uuid>,
) -> Result<Uuid, BackupError> {
    let mut parent = root;
    let mut segments: Vec<&str> = relative_path.split('/').collect();
    // 最后一段是文件名，其余是目录。
    segments.pop();
    let mut key = String::new();
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        if !key.is_empty() {
            key.push('/');
        }
        key.push_str(segment);
        if let Some(known) = folders.get(&key) {
            parent = *known;
            continue;
        }
        parent = ensure_cloud_folder(&ctx.client, parent, &normalize_cloud_name(segment))
            .await
            .map_err(BackupError::Client)?;
        folders.insert(key.clone(), parent);
    }
    Ok(parent)
}

/// 运行上传任务，并在每个确认批次检查一次微信进程；检测到启动就取消上传。
async fn upload_with_probe(
    ctx: &SnapshotContext,
    version: WechatVersion,
    task: &UploadTask,
) -> Result<UploadOutcome, UploadAbort> {
    let runner = task.clone();
    let run = runner.run();
    tokio::pin!(run);
    let mut events = task.subscribe();
    let mut events_open = true;
    let mut last_checked_chunks = 0_u32;
    let mut started = false;

    loop {
        tokio::select! {
            result = &mut run => {
                return match result {
                    Ok(outcome) => Ok(outcome),
                    Err(_) if started => Err(UploadAbort::WechatStarted),
                    Err(ClientError::FileChanged) => Err(UploadAbort::FileChanged),
                    Err(error) => Err(UploadAbort::Failed(BackupError::Client(error))),
                };
            }
            event = events.recv(), if events_open => {
                match event {
                    Ok(progress) => {
                        if progress.completed_chunks.saturating_sub(last_checked_chunks)
                            >= PROBE_CHECK_CHUNKS
                        {
                            last_checked_chunks = progress.completed_chunks;
                            if ctx.probe.is_running(version) {
                                started = true;
                                task.cancel();
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        events_open = false;
                    }
                }
            }
        }
    }
}

/// 在时间戳目录写入 `快照完成` 文件（内容为清单 JSON）。
async fn write_marker(
    ctx: &SnapshotContext,
    account: &SnapshotAccount,
    directory_id: Uuid,
    snapshot_at: i64,
    files: &[SnapshotFile],
) -> Result<(), UploadAbort> {
    let manifest = MarkerManifest {
        snapshot_time: snapshot_at,
        files: files
            .iter()
            .map(|file| MarkerFile {
                relative_path: file.relative_path.clone(),
                size: file.size,
                content_sha256: file.content_sha256.clone(),
            })
            .collect(),
    };
    let json = serde_json::to_string(&manifest)
        .map_err(|_| UploadAbort::Failed(BackupError::InvalidConfig("微信快照清单序列化失败")))?;
    let marker = MarkerFileGuard::create(&json).map_err(UploadAbort::Failed)?;
    let task = ctx
        .client
        .upload_file_with_limiter(directory_id, marker.path().to_path_buf(), None)
        .map_err(|error| UploadAbort::Failed(BackupError::Client(error)))?;
    upload_with_probe(ctx, account.version, &task)
        .await
        .map(|_| ())
}

/// 保留最近 `retention` 份完整快照，更早的移入回收站。
async fn enforce_retention(
    ctx: &SnapshotContext,
    parent: Uuid,
    retention: u8,
) -> Result<(), BackupError> {
    let mut directories = list_snapshot_dirs(ctx, parent).await?;
    let mut complete: Vec<SnapshotDir> = directories
        .drain(..)
        .filter(|directory| directory.complete)
        .collect();
    sort_directories(&mut complete);
    let keep =
        usize::from(retention.clamp(WECHAT_SNAPSHOT_RETENTION_MIN, WECHAT_SNAPSHOT_RETENTION_MAX));
    for directory in complete.into_iter().skip(keep) {
        discard_snapshot(ctx, directory.node_id).await;
    }
    Ok(())
}

/// 生成唯一的时间戳目录名；同一秒内重复快照时向后顺延。
fn next_timestamp(directories: &[SnapshotDir], baseline: i64) -> Option<(i64, String)> {
    let existing: HashSet<&str> = directories
        .iter()
        .map(|directory| directory.name.as_str())
        .collect();
    for shift in 0..=MAX_TIMESTAMP_SHIFT {
        let candidate = baseline.saturating_add(shift);
        let name = format_timestamp(candidate);
        if !existing.contains(name.as_str()) {
            return Some((candidate, name));
        }
    }
    None
}

/// 临时 `快照完成` 文件：放在系统临时目录，上传后由 Drop 清理，不留在微信目录。
struct MarkerFileGuard {
    directory: PathBuf,
}

impl MarkerFileGuard {
    fn create(content: &str) -> Result<Self, BackupError> {
        let directory = std::env::temp_dir().join(format!(
            "backup-wechat-snapshot-{}",
            Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&directory)
            .map_err(|_| BackupError::InvalidConfig("无法创建快照标记临时目录"))?;
        std::fs::write(directory.join(SNAPSHOT_DONE_FILE), content.as_bytes())
            .map_err(|_| BackupError::InvalidConfig("无法写入快照标记"))?;
        Ok(Self { directory })
    }

    fn path(&self) -> PathBuf {
        self.directory.join(SNAPSHOT_DONE_FILE)
    }
}

impl Drop for MarkerFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

/// `2026-11-01T153000Z` 形式的 UTC 时间戳（与 design D3 的目录名一致）。
pub(crate) fn format_timestamp(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let within = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = within / 3_600;
    let minute = (within % 3_600) / 60;
    let second = within % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// 解析时间戳目录名；格式不合法或日期越界时返回 `None`（目录不动）。
pub(crate) fn parse_timestamp(name: &str) -> Option<i64> {
    let bytes = name.as_bytes();
    if bytes.len() != 18 || bytes[17] != b'Z' {
        return None;
    }
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let year = parse_number(&bytes[0..4])? as i64;
    let month = parse_number(&bytes[5..7])?;
    let day = parse_number(&bytes[8..10])?;
    let hour = parse_number(&bytes[11..13])? as i64;
    let minute = parse_number(&bytes[13..15])? as i64;
    let second = parse_number(&bytes[15..17])? as i64;
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn parse_number(bytes: &[u8]) -> Option<u32> {
    let mut value = 0_u32;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))?;
    }
    Some(value)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Howard Hinnant 的 civil date 算法；不引入日期库。
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = i64::from((month + 9) % 12);
    let day_of_year = (153 * month_index + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn timestamp_roundtrip_and_validation() {
        let example = 1_793_000_600;
        assert_eq!(format_timestamp(example), "2026-10-26T074320Z");
        // design D3 的目录名示例：2026-11-01T153000Z。
        assert_eq!(format_timestamp(1_793_547_000), "2026-11-01T153000Z");
        assert_eq!(parse_timestamp("2026-11-01T153000Z"), Some(1_793_547_000));
        let text = format_timestamp(example);
        assert_eq!(text.len(), 18, "{text}");
        assert_eq!(parse_timestamp(&text), Some(example));
        assert_eq!(format_timestamp(0), "1970-01-01T000000Z");
        assert_eq!(parse_timestamp("1970-01-01T000000Z"), Some(0));

        for invalid in [
            "2026-11-01T153000",
            "2026-11-01t153000Z",
            "2026-13-01T153000Z",
            "2026-02-30T153000Z",
            "2026-11-01T240000Z",
            "2026-11-01T156000Z",
            "abcd-11-01T153000Z",
            "2026-11-01T153000z",
            "2026-11-01T153000Z/",
        ] {
            assert_eq!(parse_timestamp(invalid), None, "{invalid}");
        }
        // 闰年 2 月 29 日合法。
        assert!(parse_timestamp("2024-02-29T000000Z").is_some());
        assert!(parse_timestamp("2025-02-29T000000Z").is_none());
    }

    #[test]
    fn next_timestamp_avoids_collision() {
        let mut directories = vec![SnapshotDir {
            name: format_timestamp(100),
            node_id: Uuid::new_v4(),
            at: 100,
            complete: true,
        }];
        let (at, name) = next_timestamp(&directories, 100).unwrap();
        assert_eq!(at, 101);
        assert_eq!(name, format_timestamp(101));
        directories.push(SnapshotDir {
            name: name.clone(),
            node_id: Uuid::new_v4(),
            at,
            complete: true,
        });
        let (at, _) = next_timestamp(&directories, 100).unwrap();
        assert_eq!(at, 102);
    }

    #[test]
    fn marker_manifest_contains_no_local_paths_or_mtime() {
        let manifest = MarkerManifest {
            snapshot_time: 42,
            files: vec![MarkerFile {
                relative_path: "message/message_0.db".to_owned(),
                size: 3,
                content_sha256: "a".repeat(64),
            }],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("relative_path"));
        assert!(json.contains("content_sha256"));
        assert!(json.contains("snapshot_time"));
        assert!(!json.contains("mtime"));
        assert!(!json.contains("path\":\"/"));
    }
}
