//! 桌面备份引擎（design D6）。
//!
//! 触发：`notify` 文件监听（防抖）+ 启动/每 6 小时全量核对 + 手动“立即备份”。
//! 增量：本地索引（相对路径、大小、修改时间、云端节点 ID、已备份摘要）快速跳过；
//! 否则计算 SHA-256 与云端当前版本元数据的 `content_sha256` 常量时间比较。
//! 目录逐层与云端合并；永不删除云端；监听失败/事件溢出时降级为全量核对。
//!
//! 密钥只在本 crate 内部使用：本模块通过 [`Client`] 的现有传输/文件 API 工作，
//! 对外事件只有状态、计数、相对路径与错误码。

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use crate::Client;
use crate::clock::Clock;
use crate::error::{ApiErrorCode, ClientError};
use crate::files::DecryptedNode;
use crate::io_path::{FileFingerprint, path_for_io, validate_name};
use crate::model::{NodeKind, TransferErrorCode};
use crate::state::{
    BackupActivityRecord, BackupFolderRecord, BackupFolderSourceKind, BackupIndexRecord,
    StateStore, WechatSourceMeta,
};
use crate::transfer::{TokenBucket, transfer_error};

use super::error::BackupError;
use super::filter::{
    Exclusions, LocalFile, LocalTree, WalkOptions, hash_file_stable, name_key, scan_tree,
};
use super::paths::{comparable_path, path_within};
use super::types::{
    BackupActivity, BackupActivityKind, BackupEngineConfig, BackupFolder, BackupFolderDraft,
    BackupFolderState, BackupFolderStatus, BackupFolderUpdate, BackupPauseReason, BackupPreflight,
    BackupRunReport, BackupSettings, BackupSummary, BackupSummaryStatus, WechatAccountBackupStatus,
    WechatBackupPreflight, WechatBackupSource, WechatCategoryBackupStatus,
};
use super::watcher::{FolderWatcher, WatchSignal, watch_folder};
use super::wechat::backup::{desired_sources, validate_account_name};
use super::wechat::exclude::{exclusions_for, measure_trees};
use super::wechat::{WechatAccount, WechatCategory, WechatLocator, WechatVersion, layout_for};

/// 事件通道容量；界面来不及消费时只保留最近事件，不阻塞备份。
const EVENT_BUFFER: usize = 512;
/// 调度器 tick：检查锁定状态、全量核对与网络重试时间。
const SCHEDULER_TICK: Duration = Duration::from_secs(5);

const SETTING_INCLUDE_HIDDEN: &str = "include_hidden";
const SETTING_UPLOAD_LIMIT: &str = "upload_limit_bytes_per_second";
const SETTING_PAUSED: &str = "paused";

/// 备份引擎。`Clone` 共享同一运行状态。
#[derive(Clone)]
pub struct BackupEngine {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client,
    clock: Arc<dyn Clock>,
    state: Arc<StateStore>,
    config: BackupEngineConfig,
    events: broadcast::Sender<super::types::BackupEvent>,
    commands: Mutex<mpsc::UnboundedSender<Command>>,
    watch_sender: Mutex<Option<mpsc::UnboundedSender<WatchSignal>>>,
    watchers: Mutex<HashMap<Uuid, FolderWatcher>>,
    scheduler: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// 全局串行化备份运行：界面“立即备份”与调度器可能同时触发。选择一把全局
    /// 锁而不是按文件夹锁，因为引擎本来就在单任务里逐文件夹顺序处理，全局锁
    /// 实现最简单，也不会让多个文件夹同时读取/哈希造成额外磁盘压力。
    run_lock: tokio::sync::Mutex<()>,
    paused_all: AtomicBool,
    runtime: Mutex<Runtime>,
    /// 微信数据根发现所用的系统位置来源。
    wechat_locator: WechatLocator,
    /// 缓存的微信数据根（普通备份文件夹自动排除用）。
    wechat_roots: Mutex<Vec<PathBuf>>,
    /// 是否已完成过一次数据根发现（避免每个文件夹都调用发现）。
    wechat_roots_ready: AtomicBool,
}

#[derive(Default)]
struct Runtime {
    settings: BackupSettings,
    statuses: HashMap<Uuid, BackupFolderState>,
    paused_folders: HashSet<Uuid>,
    summary: BackupSummary,
    last_checked_at: Option<i64>,
    retry_at: Option<SystemTime>,
}

#[derive(Debug)]
enum Command {
    Wake,
    Shutdown,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.scheduler.lock()
            && let Some(handle) = guard.take()
        {
            handle.abort();
        }
    }
}

impl BackupEngine {
    /// 创建引擎；不启动调度器。调用 [`BackupEngine::start`] 开始监听与定时核对。
    pub fn new(client: Client, config: BackupEngineConfig) -> Result<Self, BackupError> {
        let device_name = config.device_name.trim().to_owned();
        if device_name.is_empty() || validate_name(&device_name).is_some() {
            return Err(BackupError::InvalidConfig("设备名不可用"));
        }
        let mut normalized = config;
        normalized.device_name = crate::io_path::normalize_cloud_name(&device_name);
        let wechat_locator = normalized.wechat_locator.clone().unwrap_or_default();
        let clock = client.clock.clone();
        let state = client.state_store.clone();
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let (commands, _) = mpsc::unbounded_channel();
        Ok(Self {
            inner: Arc::new(Inner {
                client,
                clock,
                state,
                config: normalized,
                events,
                commands: Mutex::new(commands),
                watch_sender: Mutex::new(None),
                watchers: Mutex::new(HashMap::new()),
                scheduler: Mutex::new(None),
                run_lock: tokio::sync::Mutex::new(()),
                paused_all: AtomicBool::new(false),
                runtime: Mutex::new(Runtime::default()),
                wechat_locator,
                wechat_roots: Mutex::new(Vec::new()),
                wechat_roots_ready: AtomicBool::new(false),
            }),
        })
    }

    /// 订阅状态/活动/汇总事件。
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<super::types::BackupEvent> {
        self.inner.events.subscribe()
    }

    /// 引擎配置（设备名、防抖与核对周期）。
    #[must_use]
    pub fn config(&self) -> &BackupEngineConfig {
        &self.inner.config
    }

    /// 最近一次运行状态快照（按文件夹），供界面初次渲染。
    #[must_use]
    pub fn folder_states(&self) -> Vec<BackupFolderState> {
        let runtime = match self.inner.runtime.lock() {
            Ok(runtime) => runtime,
            Err(_) => return Vec::new(),
        };
        let mut states: Vec<BackupFolderState> = runtime.statuses.values().cloned().collect();
        states.sort_by_key(|state| state.folder_id);
        states
    }

    /// 全局汇总（托盘用）。
    #[must_use]
    pub fn summary(&self) -> BackupSummary {
        self.inner
            .runtime
            .lock()
            .map(|runtime| runtime.summary)
            .unwrap_or_default()
    }

    /// 当前缓冲的全局设置。
    #[must_use]
    pub fn cached_settings(&self) -> BackupSettings {
        self.inner
            .runtime
            .lock()
            .map(|runtime| runtime.settings)
            .unwrap_or_default()
    }

    /// 读取并缓存某用户的全局设置。
    pub async fn settings(&self) -> Result<BackupSettings, BackupError> {
        let user_id = self.user_id()?;
        let settings = self.load_settings(user_id).await?;
        self.inner
            .paused_all
            .store(settings.paused, Ordering::SeqCst);
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.settings = settings;
            runtime.summary = summarize(&runtime);
        }
        self.emit_summary();
        Ok(settings)
    }

    /// 保存全局设置（隐藏文件、上传限速、暂停）。
    pub async fn set_settings(&self, settings: BackupSettings) -> Result<(), BackupError> {
        let user_id = self.user_id()?;
        self.save_settings(user_id, settings).await?;
        self.inner
            .paused_all
            .store(settings.paused, Ordering::SeqCst);
        {
            if let Ok(mut runtime) = self.inner.runtime.lock() {
                runtime.settings = settings;
                runtime.summary = summarize(&runtime);
            }
        }
        self.refresh_statuses_for_pause().await;
        self.emit_summary();
        self.wake();
        Ok(())
    }

    /// 列出当前用户的普通备份文件夹。
    ///
    /// 微信来源不在这里返回（design D2）：“备份文件夹”页面只显示用户添加的
    /// 文件夹；微信来源使用 [`BackupEngine::list_wechat_sources`]。
    pub async fn list_folders(&self) -> Result<Vec<BackupFolder>, BackupError> {
        let user_id = self.user_id()?;
        let records = self.inner.state.list_user_backup_folders(user_id).await?;
        Ok(records.into_iter().map(BackupFolder::from).collect())
    }

    /// 读取一个普通备份文件夹；微信来源不通过这个 API 暴露。
    pub async fn folder(&self, folder_id: Uuid) -> Result<BackupFolder, BackupError> {
        let user_id = self.user_id()?;
        let record = self
            .inner
            .state
            .get_backup_folder(user_id, folder_id)
            .await?
            .ok_or(BackupError::FolderNotFound)?;
        if record.source_kind != BackupFolderSourceKind::User {
            return Err(BackupError::FolderNotFound);
        }
        Ok(record.into())
    }

    /// 添加备份文件夹。不会立即上传；由调度器或“立即备份”触发。
    pub async fn add_folder(&self, draft: BackupFolderDraft) -> Result<BackupFolder, BackupError> {
        let user_id = self.user_id()?;
        Exclusions::compile(&draft.exclude_globs, &draft.exclude_subfolders)?;
        let local_path = absolute_path(&draft.local_path)?;
        // 目录 metadata 只做一次快速 stat，保持在 async 函数内；真正遍历目录
        // 的 scan_tree 仍在 spawn_blocking 中执行。
        let io_path =
            path_for_io(&local_path).map_err(|_| BackupError::InvalidConfig("备份路径无效"))?;
        let metadata = std::fs::metadata(&io_path)
            .map_err(|_| BackupError::InvalidConfig("备份路径不存在或无法读取"))?;
        if !metadata.is_dir() {
            return Err(BackupError::InvalidConfig("备份路径不是文件夹"));
        }
        let local_name = local_name(&local_path)?;
        let now = unix_seconds(self.inner.clock.now());
        let existing = self.inner.state.list_user_backup_folders(user_id).await?;
        ensure_no_path_conflict(&existing, &local_path, None)?;
        let record = BackupFolderRecord {
            id: Uuid::new_v4(),
            user_id,
            local_path,
            local_name,
            cloud_target_id: draft.cloud_target_id,
            cloud_folder_id: None,
            cloud_path: None,
            exclude_globs: draft.exclude_globs,
            exclude_subfolders: draft.exclude_subfolders,
            enabled: true,
            last_backup_at: None,
            created_at: now,
            updated_at: now,
            source_kind: BackupFolderSourceKind::User,
            wechat: None,
        };
        self.inner
            .state
            .insert_backup_folder(record.clone())
            .await?;
        self.upsert_runtime_folder(&record);
        self.sync_watchers().await;
        self.wake();
        Ok(record.into())
    }

    /// 修改备份文件夹（本地路径、云端目标、排除规则、启用/暂停）。
    pub async fn update_folder(
        &self,
        folder_id: Uuid,
        update: BackupFolderUpdate,
    ) -> Result<BackupFolder, BackupError> {
        let user_id = self.user_id()?;
        let mut record = self
            .inner
            .state
            .get_backup_folder(user_id, folder_id)
            .await?
            .ok_or(BackupError::FolderNotFound)?;
        if record.source_kind != BackupFolderSourceKind::User {
            return Err(BackupError::FolderNotFound);
        }
        let mut path_changed = false;
        if let Some(path) = &update.local_path {
            let absolute = absolute_path(path)?;
            // 目录 metadata 只做一次快速 stat，保持在 async 函数内；真正遍历
            // 目录的 scan_tree 仍在 spawn_blocking 中执行。
            let io_path =
                path_for_io(&absolute).map_err(|_| BackupError::InvalidConfig("备份路径无效"))?;
            let metadata = std::fs::metadata(&io_path)
                .map_err(|_| BackupError::InvalidConfig("备份路径不存在或无法读取"))?;
            if !metadata.is_dir() {
                return Err(BackupError::InvalidConfig("备份路径不是文件夹"));
            }
            let new_name = local_name(&absolute)?;
            if record.local_path != absolute {
                let existing = self.inner.state.list_user_backup_folders(user_id).await?;
                ensure_no_path_conflict(&existing, &absolute, Some(folder_id))?;
                record.local_path = absolute;
                record.local_name = new_name;
                record.last_backup_at = None;
                // 默认云端目标是按本地文件夹名逐层创建的，换路径后需要重新解析。
                if record.cloud_target_id.is_none() {
                    record.cloud_folder_id = None;
                    record.cloud_path = None;
                }
                path_changed = true;
            }
        }
        if let Some(globs) = &update.exclude_globs {
            record.exclude_globs = globs.clone();
        }
        if let Some(subfolders) = &update.exclude_subfolders {
            record.exclude_subfolders = subfolders.clone();
        }
        Exclusions::compile(&record.exclude_globs, &record.exclude_subfolders)?;
        if let Some(target) = update.cloud_target_id {
            record.cloud_target_id = target;
            // 目标变了：下次扫描重新解析/创建。
            record.cloud_folder_id = None;
            record.cloud_path = None;
        }
        if let Some(enabled) = update.enabled {
            record.enabled = enabled;
        }
        record.updated_at = unix_seconds(self.inner.clock.now());
        self.inner
            .state
            .replace_backup_folder(record.clone())
            .await?;
        if path_changed {
            self.inner
                .state
                .clear_backup_index(user_id, folder_id)
                .await?;
            if let Ok(mut watchers) = self.inner.watchers.lock() {
                watchers.remove(&folder_id);
            }
        }
        self.upsert_runtime_folder(&record);
        if !record.enabled {
            self.set_folder_status(
                folder_id,
                BackupFolderStatus::Paused,
                Some(BackupPauseReason::User),
            );
        }
        self.sync_watchers().await;
        self.wake();
        Ok(record.into())
    }

    /// 移除普通备份文件夹：只删除本机配置与索引，云端内容保留。
    ///
    /// 微信来源由 [`BackupEngine::set_wechat_categories`] 取消勾选时移除；
    /// 这个 API 只接受普通备份文件夹。
    pub async fn remove_folder(&self, folder_id: Uuid) -> Result<(), BackupError> {
        let user_id = self.user_id()?;
        let record = self
            .inner
            .state
            .get_backup_folder(user_id, folder_id)
            .await?
            .ok_or(BackupError::FolderNotFound)?;
        if record.source_kind != BackupFolderSourceKind::User {
            return Err(BackupError::FolderNotFound);
        }
        self.inner
            .state
            .delete_backup_folder(user_id, folder_id)
            .await?;
        if let Ok(mut watchers) = self.inner.watchers.lock() {
            watchers.remove(&folder_id);
        }
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.statuses.remove(&folder_id);
            runtime.paused_folders.remove(&folder_id);
            runtime.summary = summarize(&runtime);
        }
        self.emit_summary();
        Ok(())
    }

    /// 添加前预检：按草稿中的排除规则统计文件数与首次上传字节数，并读取剩余配额。
    ///
    /// 普通文件夹内部的微信数据根（design D2）会被自动排除，并返回
    /// [`BackupPreflight::excluded_wechat_bytes`] 供界面提示。
    pub async fn preflight(
        &self,
        draft: &BackupFolderDraft,
    ) -> Result<BackupPreflight, BackupError> {
        let local_path = absolute_path(&draft.local_path)?;
        let name = local_name(&local_path)?;
        // 预检是用户主动操作，每次刷新一次数据根缓存（不随文件事件触发）。
        self.refresh_wechat_data_roots().await;
        let roots = self.wechat_data_roots();
        let wechat = exclusions_for(&local_path, &roots);
        if wechat.scanned_root_inside_wechat {
            let root = local_path.clone();
            let (excluded_files, excluded_bytes) =
                tokio::task::spawn_blocking(move || measure_trees(&[root]))
                    .await
                    .map_err(|_| BackupError::InvalidConfig("扫描任务失败"))?;
            let remaining_quota_bytes = self.remaining_quota().await;
            return Ok(BackupPreflight {
                file_count: 0,
                total_bytes: 0,
                upload_bytes: 0,
                excluded_count: excluded_files,
                unreadable_count: 0,
                remaining_quota_bytes,
                excluded_wechat_file_count: excluded_files,
                excluded_wechat_bytes: excluded_bytes,
            });
        }
        let mut subfolders = draft.exclude_subfolders.clone();
        subfolders.extend(wechat.relative_subfolders);
        let exclusions = Exclusions::compile(&draft.exclude_globs, &subfolders)?;
        let measured_roots = wechat.measured_roots;
        let include_hidden = self.cached_settings().include_hidden;
        let tree = tokio::task::spawn_blocking(move || {
            let options = WalkOptions {
                include_hidden,
                exclusions: &exclusions,
            };
            let tree = scan_tree(&local_path, &name, &options)?;
            let (excluded_wechat_file_count, excluded_wechat_bytes) =
                measure_trees(&measured_roots);
            Ok::<_, BackupError>((tree, excluded_wechat_file_count, excluded_wechat_bytes))
        })
        .await
        .map_err(|_| BackupError::InvalidConfig("扫描任务失败"))??;
        let (tree, excluded_wechat_file_count, excluded_wechat_bytes) = tree;
        let (file_count, total_bytes) = tree.counts();
        let remaining_quota_bytes = self.remaining_quota().await;
        let mut sizes = Vec::new();
        tree.collect_sizes(&mut sizes);
        let upload_bytes = projected_total_bytes(&sizes);
        Ok(BackupPreflight {
            file_count,
            total_bytes,
            upload_bytes,
            excluded_count: tree.excluded,
            unreadable_count: tree.unreadable,
            remaining_quota_bytes,
            excluded_wechat_file_count,
            excluded_wechat_bytes,
        })
    }

    async fn remaining_quota(&self) -> Option<u64> {
        match self.inner.client.get_usage().await {
            Ok(usage) => Some(
                usage
                    .quota_bytes
                    .saturating_sub(usage.used_bytes)
                    .saturating_sub(usage.reserved_bytes)
                    .max(0) as u64,
            ),
            Err(_) => None,
        }
    }

    /// 刷新缓存的微信数据根目录（引擎启动、每次全量核对与预检时调用）。
    ///
    /// 发现失败不会影响普通备份：保留上一次缓存（启动时为空的缓存），并记录
    /// 一条不含路径的警告。
    pub async fn refresh_wechat_data_roots(&self) {
        let locator = self.inner.wechat_locator.clone();
        let manual = self.inner.config.wechat_manual_directories.clone();
        let roots = match locator.discover_data_roots(&manual).await {
            Ok(roots) => roots,
            Err(_) => {
                tracing::warn!("微信数据根发现失败，保留上一次缓存");
                self.inner.wechat_roots_ready.store(true, Ordering::SeqCst);
                return;
            }
        };
        let mut deduped = Vec::with_capacity(roots.len());
        let mut seen = HashSet::new();
        for root in roots {
            if seen.insert(comparable_path(&root)) {
                deduped.push(root);
            }
        }
        if let Ok(mut guard) = self.inner.wechat_roots.lock() {
            *guard = deduped;
        }
        self.inner.wechat_roots_ready.store(true, Ordering::SeqCst);
    }

    fn wechat_data_roots(&self) -> Vec<PathBuf> {
        self.inner
            .wechat_roots
            .lock()
            .map(|roots| roots.clone())
            .unwrap_or_default()
    }

    /// 首次使用缓存前做一次发现；之后由全量核对 / 预检刷新。
    async fn ensure_wechat_roots(&self) {
        if !self.inner.wechat_roots_ready.load(Ordering::SeqCst) {
            self.refresh_wechat_data_roots().await;
        }
    }

    /// 设置一个微信账号勾选的类别（design D2）。
    ///
    /// 按勾选增 / 删微信来源备份项；每个类别的每个来源路径一项，同一本地路径
    /// 只注册一次。“聊天记录数据库”不注册为普通备份项（第 3 组快照），但仍
    /// 记录勾选状态供快照使用。取消勾选只停止备份：云端内容永不删除。
    pub async fn set_wechat_categories(
        &self,
        account: &WechatAccount,
        categories: &[WechatCategory],
    ) -> Result<(), BackupError> {
        let user_id = self.user_id()?;
        if !validate_account_name(&account.account_name) {
            return Err(BackupError::InvalidConfig("微信账号目录名无效"));
        }
        let data_root = absolute_path(&account.data_root)?;
        let data_root_text = data_root
            .to_str()
            .ok_or(BackupError::InvalidConfig("微信数据目录无效"))?
            .to_owned();
        let selections: Vec<(String, bool)> = WechatCategory::ALL
            .iter()
            .map(|category| (category.as_str().to_owned(), categories.contains(category)))
            .collect();
        let now = unix_seconds(self.inner.clock.now());
        self.inner
            .state
            .replace_wechat_selections(
                user_id,
                data_root_text,
                account.account_name.clone(),
                account.version.as_str().to_owned(),
                selections,
                now,
            )
            .await?;
        self.reconcile_wechat_sources(user_id).await
    }

    /// 按全部勾选增删微信来源备份项；同一路径只注册一次。
    async fn reconcile_wechat_sources(&self, user_id: Uuid) -> Result<(), BackupError> {
        let selections = self.inner.state.list_wechat_selections(user_id).await?;
        let desired = tokio::task::spawn_blocking(move || desired_sources(&selections))
            .await
            .map_err(|_| BackupError::InvalidConfig("微信来源计算失败"))?;
        let existing = self.inner.state.list_wechat_backup_folders(user_id).await?;
        let mut by_path: HashMap<String, BackupFolderRecord> = HashMap::new();
        for record in existing {
            by_path.insert(comparable_path(&record.local_path), record);
        }

        let now = unix_seconds(self.inner.clock.now());
        let mut added = Vec::new();
        for source in desired {
            let key = comparable_path(&source.local_path);
            if by_path.remove(&key).is_some() {
                continue;
            }
            if !source.exists {
                continue;
            }
            let data_root = source
                .data_root
                .to_str()
                .ok_or(BackupError::InvalidConfig("微信数据目录无效"))?
                .to_owned();
            let record = BackupFolderRecord {
                id: Uuid::new_v4(),
                user_id,
                local_path: source.local_path,
                local_name: source.local_name,
                cloud_target_id: None,
                cloud_folder_id: None,
                cloud_path: None,
                exclude_globs: Vec::new(),
                exclude_subfolders: source.exclude_subfolders,
                enabled: true,
                last_backup_at: None,
                created_at: now,
                updated_at: now,
                source_kind: BackupFolderSourceKind::Wechat,
                wechat: Some(WechatSourceMeta {
                    data_root,
                    account_name: source.account_name,
                    version: source.version.as_str().to_owned(),
                    category: source.category.as_str().to_owned(),
                    relative_path: source.relative_path,
                }),
            };
            self.inner
                .state
                .insert_backup_folder(record.clone())
                .await?;
            added.push(record);
        }

        let mut removed = Vec::new();
        for (_, record) in by_path {
            self.inner
                .state
                .delete_backup_folder(user_id, record.id)
                .await?;
            removed.push(record.id);
        }

        for record in added {
            self.upsert_runtime_folder(&record);
        }
        if !removed.is_empty() {
            if let Ok(mut runtime) = self.inner.runtime.lock() {
                for folder_id in &removed {
                    runtime.statuses.remove(folder_id);
                    runtime.paused_folders.remove(folder_id);
                }
                runtime.summary = summarize(&runtime);
            }
            if let Ok(mut watchers) = self.inner.watchers.lock() {
                for folder_id in &removed {
                    watchers.remove(folder_id);
                }
            }
            self.emit_summary();
        }
        self.sync_watchers().await;
        self.wake();
        Ok(())
    }

    /// 列出已注册的微信来源备份项及其状态（“微信备份”页面使用）。
    pub async fn list_wechat_sources(&self) -> Result<Vec<WechatBackupSource>, BackupError> {
        let user_id = self.user_id()?;
        let records = self.inner.state.list_wechat_backup_folders(user_id).await?;
        let statuses = self.runtime_statuses();
        let mut sources = Vec::new();
        for record in records {
            let Some(meta) = &record.wechat else {
                continue;
            };
            let Some(version) = WechatVersion::parse(&meta.version) else {
                continue;
            };
            let Some(category) = WechatCategory::parse(&meta.category) else {
                continue;
            };
            let state = statuses.get(&record.id);
            let status = state
                .map(|state| state.status)
                .unwrap_or(if record.enabled {
                    BackupFolderStatus::IdleOk
                } else {
                    BackupFolderStatus::Paused
                });
            let pause_reason = state
                .and_then(|state| state.pause_reason)
                .or(if record.enabled {
                    None
                } else {
                    Some(BackupPauseReason::User)
                });
            sources.push(WechatBackupSource {
                folder_id: record.id,
                data_root: PathBuf::from(&meta.data_root),
                account_name: meta.account_name.clone(),
                version,
                category,
                relative_path: meta.relative_path.clone(),
                cloud_path: record.cloud_path,
                enabled: record.enabled,
                status,
                pause_reason,
                last_backup_at: state
                    .and_then(|state| state.last_backup_at)
                    .or(record.last_backup_at),
            });
        }
        sources.sort_by(|left, right| {
            left.account_name
                .cmp(&right.account_name)
                .then_with(|| category_order(left.category).cmp(&category_order(right.category)))
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        });
        Ok(sources)
    }

    /// 每个微信账号每个类别的状态汇总（第 4 组界面使用）。
    ///
    /// 未勾选的类别也会出现（`selected = false`）；数据库快照类别已勾选但尚未
    /// 注册为来源，`status` 为 `None`（第 3 组提供快照状态）。
    pub async fn list_wechat_accounts(
        &self,
    ) -> Result<Vec<WechatAccountBackupStatus>, BackupError> {
        let user_id = self.user_id()?;
        let selections = self.inner.state.list_wechat_selections(user_id).await?;
        let sources = self.list_wechat_sources().await?;

        struct AccountEntry {
            version: WechatVersion,
            category_selected: HashMap<WechatCategory, bool>,
        }
        let mut accounts: BTreeMap<(String, String), AccountEntry> = BTreeMap::new();
        for selection in selections {
            let Some(version) = WechatVersion::parse(&selection.version) else {
                continue;
            };
            let Some(category) = WechatCategory::parse(&selection.category) else {
                continue;
            };
            let key = (selection.data_root.clone(), selection.account_name.clone());
            let entry = accounts.entry(key).or_insert_with(|| AccountEntry {
                version,
                category_selected: HashMap::new(),
            });
            entry.category_selected.insert(category, selection.selected);
        }

        let mut result = Vec::with_capacity(accounts.len());
        for ((data_root, account_name), entry) in accounts {
            let data_root_comparable = comparable_path(Path::new(&data_root));
            let mut categories = Vec::with_capacity(WechatCategory::ALL.len());
            let mut last_backup_at: Option<i64> = None;
            for category in WechatCategory::ALL {
                let selected = entry
                    .category_selected
                    .get(&category)
                    .copied()
                    .unwrap_or(false);
                let matching: Vec<&WechatBackupSource> = sources
                    .iter()
                    .filter(|source| {
                        source.category == category
                            && source.account_name == account_name
                            && comparable_path(&source.data_root) == data_root_comparable
                    })
                    .collect();
                let status = combine_statuses(&matching);
                let pause_reason = if matches!(status, Some(BackupFolderStatus::NeedsUnlock)) {
                    Some(BackupPauseReason::NeedsUnlock)
                } else if matches!(status, Some(BackupFolderStatus::Paused)) {
                    matching
                        .iter()
                        .find_map(|source| source.pause_reason)
                        .or(Some(BackupPauseReason::User))
                } else {
                    None
                };
                let category_last = matching
                    .iter()
                    .filter_map(|source| source.last_backup_at)
                    .max();
                last_backup_at = match (last_backup_at, category_last) {
                    (Some(current), Some(candidate)) => Some(current.max(candidate)),
                    (Some(current), None) => Some(current),
                    (None, candidate) => candidate,
                };
                categories.push(WechatCategoryBackupStatus {
                    category,
                    selected,
                    status,
                    pause_reason,
                    last_backup_at: category_last,
                });
            }
            result.push(WechatAccountBackupStatus {
                data_root: PathBuf::from(data_root),
                account_name,
                version: entry.version,
                categories,
                last_backup_at,
            });
        }
        Ok(result)
    }

    /// 微信备份首次上传预检：勾选类别的文件数、明文/密文字节数、剩余配额、
    /// 是否超额，以及取消“图片与视频”后需要上传的字节数（design D6）。
    ///
    /// 数据库类别也计入（第 3 组快照会上传它）。
    pub async fn preflight_wechat(
        &self,
        account: &WechatAccount,
        categories: &[WechatCategory],
    ) -> Result<WechatBackupPreflight, BackupError> {
        if !validate_account_name(&account.account_name) {
            return Err(BackupError::InvalidConfig("微信账号目录名无效"));
        }
        self.user_id()?;
        let data_root = absolute_path(&account.data_root)?;
        let account_dir = data_root.join(&account.account_name);
        let layout = layout_for(account.version);
        let selected: HashSet<WechatCategory> = categories.iter().copied().collect();
        let mut jobs: Vec<(WechatCategory, PathBuf)> = Vec::new();
        for category in WechatCategory::ALL {
            if !selected.contains(&category) {
                continue;
            }
            let Some(category_layout) = layout.category(category) else {
                continue;
            };
            for relative in category_layout.account_paths {
                jobs.push((category, account_dir.join(relative)));
            }
            for relative in category_layout.data_root_paths {
                jobs.push((
                    category,
                    data_root.join(relative.replace("{account}", &account.account_name)),
                ));
            }
        }
        let cache_subfolders: Vec<String> = layout
            .cache_directories
            .iter()
            .map(|directory| (*directory).to_owned())
            .collect();
        let include_hidden = self.cached_settings().include_hidden;
        let stats = tokio::task::spawn_blocking(move || {
            let exclusions = Exclusions::compile(&[], &cache_subfolders)?;
            let mut file_count = 0_u64;
            let mut total_bytes = 0_u64;
            let mut upload_bytes = 0_u64;
            let mut upload_without_images = 0_u64;
            let mut database_bytes = 0_u64;
            let mut excluded_count = 0_u64;
            let mut unreadable_count = 0_u64;
            for (category, path) in jobs {
                if !path_for_io(&path).is_ok_and(|path| path.is_dir()) {
                    continue;
                }
                let options = WalkOptions {
                    include_hidden,
                    exclusions: &exclusions,
                };
                let tree = scan_tree(&path, "wechat", &options)?;
                let (files, bytes) = tree.counts();
                file_count = file_count.saturating_add(files);
                total_bytes = total_bytes.saturating_add(bytes);
                excluded_count = excluded_count.saturating_add(tree.excluded);
                unreadable_count = unreadable_count.saturating_add(tree.unreadable);
                if category == WechatCategory::Databases {
                    database_bytes = database_bytes.saturating_add(bytes);
                }
                let mut sizes = Vec::new();
                tree.collect_sizes(&mut sizes);
                let projected = projected_total_bytes(&sizes);
                upload_bytes = upload_bytes.saturating_add(projected);
                if category != WechatCategory::ImagesAndVideos {
                    upload_without_images = upload_without_images.saturating_add(projected);
                }
            }
            Ok::<_, BackupError>((
                file_count,
                total_bytes,
                upload_bytes,
                upload_without_images,
                database_bytes,
                excluded_count,
                unreadable_count,
            ))
        })
        .await
        .map_err(|_| BackupError::InvalidConfig("扫描任务失败"))??;
        let (
            file_count,
            total_bytes,
            upload_bytes,
            upload_bytes_without_images_and_videos,
            database_bytes,
            excluded_count,
            unreadable_count,
        ) = stats;
        let remaining_quota_bytes = self.remaining_quota().await;
        let over_quota = remaining_quota_bytes.is_some_and(|remaining| upload_bytes > remaining);
        Ok(WechatBackupPreflight {
            file_count,
            total_bytes,
            upload_bytes,
            upload_bytes_without_images_and_videos,
            database_bytes,
            excluded_count,
            unreadable_count,
            remaining_quota_bytes,
            over_quota,
        })
    }

    fn runtime_statuses(&self) -> HashMap<Uuid, BackupFolderState> {
        self.inner
            .runtime
            .lock()
            .map(|runtime| runtime.statuses.clone())
            .unwrap_or_default()
    }

    /// 暂停全部备份（保留已上传内容；恢复后继续）。
    pub async fn pause_all(&self) -> Result<(), BackupError> {
        let user_id = self.user_id()?;
        let mut settings = self.load_settings(user_id).await?;
        settings.paused = true;
        self.save_settings(user_id, settings).await?;
        self.inner.paused_all.store(true, Ordering::SeqCst);
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.settings = settings;
            for state in runtime.statuses.values_mut() {
                state.status = BackupFolderStatus::Paused;
                state.pause_reason = Some(BackupPauseReason::User);
            }
            runtime.summary = summarize(&runtime);
        }
        self.emit_all_folder_states();
        self.emit_summary();
        Ok(())
    }

    /// 恢复全部备份。
    pub async fn resume_all(&self) -> Result<(), BackupError> {
        let user_id = self.user_id()?;
        let mut settings = self.load_settings(user_id).await?;
        settings.paused = false;
        self.save_settings(user_id, settings).await?;
        self.inner.paused_all.store(false, Ordering::SeqCst);
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.settings = settings;
            let paused_folders = runtime.paused_folders.clone();
            for (folder_id, state) in runtime.statuses.iter_mut() {
                if state.pause_reason == Some(BackupPauseReason::User)
                    && !paused_folders.contains(folder_id)
                {
                    state.status = BackupFolderStatus::IdleOk;
                    state.pause_reason = None;
                }
            }
            runtime.summary = summarize(&runtime);
        }
        self.emit_all_folder_states();
        self.emit_summary();
        self.wake();
        Ok(())
    }

    /// 暂停单个文件夹（持久化为 `enabled = false`）。
    pub async fn pause_folder(&self, folder_id: Uuid) -> Result<(), BackupError> {
        let user_id = self.user_id()?;
        let updated_at = unix_seconds(self.inner.clock.now());
        self.inner
            .state
            .set_backup_folder_enabled(user_id, folder_id, false, updated_at)
            .await?;
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.paused_folders.insert(folder_id);
        }
        self.set_folder_status(
            folder_id,
            BackupFolderStatus::Paused,
            Some(BackupPauseReason::User),
        );
        Ok(())
    }

    /// 恢复单个文件夹。
    pub async fn resume_folder(&self, folder_id: Uuid) -> Result<(), BackupError> {
        let user_id = self.user_id()?;
        let updated_at = unix_seconds(self.inner.clock.now());
        self.inner
            .state
            .set_backup_folder_enabled(user_id, folder_id, true, updated_at)
            .await?;
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.paused_folders.remove(&folder_id);
        }
        self.set_folder_status(folder_id, BackupFolderStatus::IdleOk, None);
        self.wake();
        Ok(())
    }

    /// 最近活动（只来自本机状态库）。
    pub async fn recent_activity(&self, limit: u32) -> Result<Vec<BackupActivity>, BackupError> {
        let user_id = self.user_id()?;
        let records = self
            .inner
            .state
            .list_backup_activity(user_id, limit)
            .await?;
        Ok(records
            .into_iter()
            .map(|record| BackupActivity {
                id: record.id,
                folder_id: record.folder_id,
                relative_path: record.relative_path,
                kind: BackupActivityKind::parse(&record.kind).unwrap_or(BackupActivityKind::Failed),
                occurred_at: record.occurred_at,
                message: record.message,
            })
            .collect())
    }

    /// 唤醒调度器（解锁后、设置变化或界面手动触发时调用）。
    pub fn wake(&self) {
        if let Ok(commands) = self.inner.commands.lock() {
            let _ = commands.send(Command::Wake);
        }
    }

    fn user_id(&self) -> Result<Uuid, BackupError> {
        let account = self.inner.client.ensure_writable()?;
        Ok(account.user_id)
    }

    async fn load_settings(&self, user_id: Uuid) -> Result<BackupSettings, BackupError> {
        let include_hidden = self
            .inner
            .state
            .backup_setting(user_id, SETTING_INCLUDE_HIDDEN.to_owned())
            .await?
            .is_some_and(|value| value == "1");
        let paused = self
            .inner
            .state
            .backup_setting(user_id, SETTING_PAUSED.to_owned())
            .await?
            .is_some_and(|value| value == "1");
        let upload_limit = self
            .inner
            .state
            .backup_setting(user_id, SETTING_UPLOAD_LIMIT.to_owned())
            .await?
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0);
        Ok(BackupSettings {
            include_hidden,
            upload_limit_bytes_per_second: upload_limit,
            paused,
        })
    }

    async fn save_settings(
        &self,
        user_id: Uuid,
        settings: BackupSettings,
    ) -> Result<(), BackupError> {
        self.inner
            .state
            .set_backup_setting(
                user_id,
                SETTING_INCLUDE_HIDDEN.to_owned(),
                if settings.include_hidden { "1" } else { "0" }.to_owned(),
            )
            .await?;
        self.inner
            .state
            .set_backup_setting(
                user_id,
                SETTING_PAUSED.to_owned(),
                if settings.paused { "1" } else { "0" }.to_owned(),
            )
            .await?;
        self.inner
            .state
            .set_backup_setting(
                user_id,
                SETTING_UPLOAD_LIMIT.to_owned(),
                settings
                    .upload_limit_bytes_per_second
                    .unwrap_or_default()
                    .to_string(),
            )
            .await?;
        Ok(())
    }

    async fn refresh_statuses_for_pause(&self) {
        let settings = self.cached_settings();
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            let paused_folders = runtime.paused_folders.clone();
            for (folder_id, state) in runtime.statuses.iter_mut() {
                if settings.paused {
                    state.status = BackupFolderStatus::Paused;
                    state.pause_reason = Some(BackupPauseReason::User);
                } else if state.pause_reason == Some(BackupPauseReason::User)
                    && !paused_folders.contains(folder_id)
                {
                    state.status = BackupFolderStatus::IdleOk;
                    state.pause_reason = None;
                }
            }
            runtime.summary = summarize(&runtime);
        }
        self.emit_all_folder_states();
    }

    fn upsert_runtime_folder(&self, record: &BackupFolderRecord) {
        let paused = self.inner.paused_all.load(Ordering::SeqCst) || !record.enabled;
        let state = BackupFolderState {
            folder_id: record.id,
            status: if paused {
                BackupFolderStatus::Paused
            } else {
                BackupFolderStatus::IdleOk
            },
            pause_reason: paused.then_some(BackupPauseReason::User),
            last_backup_at: record.last_backup_at,
        };
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            if !record.enabled {
                runtime.paused_folders.insert(record.id);
            } else {
                runtime.paused_folders.remove(&record.id);
            }
            runtime.statuses.insert(record.id, state.clone());
            runtime.summary = summarize(&runtime);
        }
        self.emit_folder_state(state);
        self.emit_summary();
    }

    fn set_folder_status(
        &self,
        folder_id: Uuid,
        status: BackupFolderStatus,
        pause_reason: Option<BackupPauseReason>,
    ) {
        let state = {
            let Ok(mut runtime) = self.inner.runtime.lock() else {
                return;
            };
            let last_backup_at = runtime
                .statuses
                .get(&folder_id)
                .and_then(|state| state.last_backup_at);
            let state = BackupFolderState {
                folder_id,
                status,
                pause_reason,
                last_backup_at,
            };
            runtime.statuses.insert(folder_id, state.clone());
            runtime.summary = summarize(&runtime);
            state
        };
        self.emit_folder_state(state);
        self.emit_summary();
    }

    fn set_last_backup_at(&self, folder_id: Uuid, at: i64) {
        if let Ok(mut runtime) = self.inner.runtime.lock()
            && let Some(state) = runtime.statuses.get_mut(&folder_id)
        {
            state.last_backup_at = Some(at);
        }
        if let Ok(runtime) = self.inner.runtime.lock()
            && let Some(state) = runtime.statuses.get(&folder_id).cloned()
        {
            self.emit_folder_state(state);
        }
    }

    fn emit_folder_state(&self, state: BackupFolderState) {
        let _ = self
            .inner
            .events
            .send(super::types::BackupEvent::Folder(state));
    }

    fn emit_all_folder_states(&self) {
        let states: Vec<BackupFolderState> = match self.inner.runtime.lock() {
            Ok(runtime) => runtime.statuses.values().cloned().collect(),
            Err(_) => return,
        };
        for state in states {
            self.emit_folder_state(state);
        }
    }

    fn emit_summary(&self) {
        let summary = self.summary();
        let _ = self
            .inner
            .events
            .send(super::types::BackupEvent::Summary(summary));
    }

    fn is_paused(&self, folder_id: Uuid) -> bool {
        if self.inner.paused_all.load(Ordering::SeqCst) {
            return true;
        }
        self.inner
            .runtime
            .lock()
            .map(|runtime| runtime.settings.paused || runtime.paused_folders.contains(&folder_id))
            .unwrap_or(true)
    }

    async fn record_activity(
        &self,
        user_id: Uuid,
        folder_id: Uuid,
        relative_path: String,
        kind: BackupActivityKind,
        message: Option<String>,
    ) -> Result<(), BackupError> {
        let occurred_at = unix_seconds(self.inner.clock.now());
        let record = BackupActivityRecord {
            id: 0,
            user_id,
            folder_id,
            relative_path: relative_path.clone(),
            kind: kind.as_str().to_owned(),
            occurred_at,
            message: message.clone(),
        };
        let id = self.inner.state.insert_backup_activity(record).await?;
        let activity = BackupActivity {
            id,
            folder_id,
            relative_path,
            kind,
            occurred_at,
            message,
        };
        let _ = self
            .inner
            .events
            .send(super::types::BackupEvent::Activity(activity));
        Ok(())
    }
}

impl BackupEngine {
    /// 启动调度器：首次唤醒做启动全量核对，之后监听文件变化并每 6 小时全量核对。
    pub async fn start(&self) -> Result<(), BackupError> {
        {
            let guard = self
                .inner
                .scheduler
                .lock()
                .map_err(|_| BackupError::InvalidConfig("引擎状态锁损坏"))?;
            if guard.is_some() {
                return Err(BackupError::AlreadyRunning);
            }
        }
        // 每次启动创建新的命令通道：stop() 后旧调度器已被 abort，新通道保证
        // 旧任务不会收到/处理新事件，也让引擎可以 start → stop → start。
        let (commands, command_receiver) = mpsc::unbounded_channel();
        *self
            .inner
            .commands
            .lock()
            .map_err(|_| BackupError::InvalidConfig("引擎状态锁损坏"))? = commands;
        let (watch_sender, watch_receiver) = mpsc::unbounded_channel();
        *self
            .inner
            .watch_sender
            .lock()
            .map_err(|_| BackupError::InvalidConfig("引擎状态锁损坏"))? = Some(watch_sender);

        self.refresh_runtime().await?;
        // 启动时发现一次微信数据根；普通备份文件夹据此自动排除。
        self.refresh_wechat_data_roots().await;
        // 旧 watcher 可能绑定到上一次的 watch 通道；统一丢弃后用新 sender 重建。
        if let Ok(mut watchers) = self.inner.watchers.lock() {
            watchers.clear();
        }
        self.sync_watchers().await;
        let inner = self.inner.clone();
        let handle = tokio::spawn(async move {
            run_scheduler(inner, command_receiver, watch_receiver).await;
        });
        *self
            .inner
            .scheduler
            .lock()
            .map_err(|_| BackupError::InvalidConfig("引擎状态锁损坏"))? = Some(handle);
        self.wake();
        Ok(())
    }

    /// 停止调度器与文件监听；已上传内容与本地索引保持不变。
    pub async fn stop(&self) {
        if let Ok(commands) = self.inner.commands.lock() {
            let _ = commands.send(Command::Shutdown);
        }
        if let Ok(mut guard) = self.inner.scheduler.lock()
            && let Some(handle) = guard.take()
        {
            handle.abort();
        }
        if let Ok(mut watchers) = self.inner.watchers.lock() {
            watchers.clear();
        }
        if let Ok(mut sender) = self.inner.watch_sender.lock() {
            *sender = None;
        }
    }

    /// 立即备份一个文件夹并返回统计。
    pub async fn scan_folder_now(&self, folder_id: Uuid) -> Result<BackupRunReport, BackupError> {
        let user_id = self.user_id()?;
        let folder = self
            .inner
            .state
            .get_backup_folder(user_id, folder_id)
            .await?
            .ok_or(BackupError::FolderNotFound)?;
        let settings = self.load_settings(user_id).await?;
        self.run_folder(&folder, &settings).await
    }

    /// 立即备份全部文件夹并返回每个文件夹的统计。
    pub async fn scan_all_now(&self) -> Result<Vec<BackupRunReport>, BackupError> {
        let user_id = self.user_id()?;
        let settings = self.load_settings(user_id).await?;
        let folders = self.inner.state.list_backup_folders(user_id).await?;
        let mut reports = Vec::new();
        for folder in folders {
            if settings.paused || !folder.enabled {
                self.set_folder_status(
                    folder.id,
                    BackupFolderStatus::Paused,
                    Some(BackupPauseReason::User),
                );
                continue;
            }
            match self.run_folder(&folder, &settings).await {
                Ok(report) => reports.push(report),
                Err(error) => {
                    self.record_folder_error(user_id, &folder, &error).await?;
                }
            }
        }
        self.mark_checked();
        Ok(reports)
    }

    async fn refresh_runtime(&self) -> Result<(), BackupError> {
        let user_id = self.user_id()?;
        let settings = self.load_settings(user_id).await?;
        let folders = self.inner.state.list_backup_folders(user_id).await?;
        self.inner
            .paused_all
            .store(settings.paused, Ordering::SeqCst);
        {
            let mut runtime = self
                .inner
                .runtime
                .lock()
                .map_err(|_| BackupError::InvalidConfig("引擎状态锁损坏"))?;
            runtime.settings = settings;
            runtime.paused_folders = folders
                .iter()
                .filter(|folder| !folder.enabled)
                .map(|folder| folder.id)
                .collect();
            runtime.statuses = folders
                .iter()
                .map(|folder| {
                    let paused = settings.paused || !folder.enabled;
                    (
                        folder.id,
                        BackupFolderState {
                            folder_id: folder.id,
                            status: if paused {
                                BackupFolderStatus::Paused
                            } else {
                                BackupFolderStatus::IdleOk
                            },
                            pause_reason: paused.then_some(BackupPauseReason::User),
                            last_backup_at: folder.last_backup_at,
                        },
                    )
                })
                .collect();
            runtime.summary = summarize(&runtime);
        }
        self.emit_all_folder_states();
        self.emit_summary();
        Ok(())
    }

    async fn sync_watchers(&self) {
        if !self.inner.config.watch_files {
            return;
        }
        let Ok(user_id) = self.user_id() else {
            return;
        };
        let Ok(folders) = self.inner.state.list_backup_folders(user_id).await else {
            return;
        };
        let settings = self.cached_settings();
        let wanted: HashSet<Uuid> = folders
            .iter()
            .filter(|folder| folder.enabled && !settings.paused)
            .map(|folder| folder.id)
            .collect();
        let Ok(mut watchers) = self.inner.watchers.lock() else {
            return;
        };
        watchers.retain(|folder_id, _| wanted.contains(folder_id));
        let Ok(sender_guard) = self.inner.watch_sender.lock() else {
            return;
        };
        let Some(sender) = sender_guard.as_ref().cloned() else {
            return;
        };
        for folder in folders {
            if !wanted.contains(&folder.id) || watchers.contains_key(&folder.id) {
                continue;
            }
            match watch_folder(&folder.local_path, folder.id, sender.clone()) {
                Ok(watcher) => {
                    watchers.insert(folder.id, watcher);
                }
                Err(_) => {
                    // 不记录路径；降级为定时全量核对（design D6）。
                    tracing::warn!(folder_id = %folder.id, "文件监听不可用，降级为全量核对");
                }
            }
        }
    }

    fn mark_checked(&self) {
        let now = unix_seconds(self.inner.clock.now());
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.last_checked_at = Some(now);
            // 不要在这里清 retry_at：本轮扫描可能刚因断网/文件被占用安排了
            // 下一次重试；由调度器在真正触发重试时清除。
            runtime.summary = summarize(&runtime);
        }
        self.emit_summary();
    }

    fn mark_needs_unlock(&self) {
        let mut changed = false;
        {
            let Ok(mut runtime) = self.inner.runtime.lock() else {
                return;
            };
            for state in runtime.statuses.values_mut() {
                if state.status != BackupFolderStatus::NeedsUnlock {
                    state.status = BackupFolderStatus::NeedsUnlock;
                    state.pause_reason = Some(BackupPauseReason::NeedsUnlock);
                    changed = true;
                }
            }
            if changed {
                runtime.summary = summarize(&runtime);
            }
        }
        if changed {
            self.emit_all_folder_states();
            self.emit_summary();
        }
    }

    async fn record_folder_error(
        &self,
        user_id: Uuid,
        folder: &BackupFolderRecord,
        error: &BackupError,
    ) -> Result<(), BackupError> {
        let message = error.to_string();
        self.record_activity(
            user_id,
            folder.id,
            folder.local_name.clone(),
            BackupActivityKind::Failed,
            Some(message),
        )
        .await?;
        self.set_folder_status(
            folder.id,
            BackupFolderStatus::Error {
                unreadable_count: 0,
            },
            None,
        );
        Ok(())
    }

    fn schedule_retry(&self) {
        let at = self.inner.clock.now() + self.inner.config.retry_interval;
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.retry_at = Some(at);
        }
    }

    async fn resolve_target(
        &self,
        account: &crate::Account,
        folder: &BackupFolderRecord,
    ) -> Result<(Uuid, Option<String>), BackupError> {
        if let Some(target) = folder.cloud_target_id {
            return Ok((target, None));
        }
        let mut parent = account.root_node_id;
        let names = match &folder.wechat {
            Some(meta) => {
                let category = WechatCategory::parse(&meta.category)
                    .ok_or(BackupError::InvalidConfig("微信类别无效"))?;
                let last = meta
                    .relative_path
                    .rsplit('/')
                    .find(|part| !part.is_empty())
                    .ok_or(BackupError::InvalidConfig("微信来源路径无效"))?;
                vec![
                    "微信备份".to_owned(),
                    self.inner.config.device_name.clone(),
                    meta.account_name.clone(),
                    category.label().to_owned(),
                    crate::io_path::normalize_cloud_name(last),
                ]
            }
            None => vec![
                "电脑备份".to_owned(),
                self.inner.config.device_name.clone(),
                folder.local_name.clone(),
            ],
        };
        let mut path = String::from("全部文件");
        for name in names {
            parent = self.ensure_cloud_folder(parent, &name).await?;
            path.push('/');
            path.push_str(&name);
        }
        Ok((parent, Some(path)))
    }

    /// 逐层查找或创建云端文件夹；`name_conflict` 竞态时重新列出复用。
    async fn ensure_cloud_folder(&self, parent_id: Uuid, name: &str) -> Result<Uuid, ClientError> {
        let key = name_key(name);
        let nodes = self.inner.client.list_folder_decrypted(parent_id).await?;
        if let Some(node) = nodes.iter().find(|node| name_key(node.name()) == key) {
            return match node.kind() {
                NodeKind::Folder => Ok(node.view.id),
                NodeKind::File => Err(transfer_error(
                    TransferErrorCode::TargetUnavailable,
                    "云端同一位置已有同名文件",
                )),
            };
        }
        match self.inner.client.create_folder(parent_id, name).await {
            Ok(folder) => Ok(folder.id),
            Err(error) if error.api_code() == Some(&ApiErrorCode::NameConflict) => {
                let nodes = self.inner.client.list_folder_decrypted(parent_id).await?;
                nodes
                    .into_iter()
                    .find(|node| node.kind() == NodeKind::Folder && name_key(node.name()) == key)
                    .map(|node| node.view.id)
                    .ok_or(ClientError::Transfer(crate::error::TransferError::new(
                        TransferErrorCode::TargetUnavailable,
                        "云端同名文件夹在并发创建后不可用",
                    )))
            }
            Err(error) => Err(error),
        }
    }

    async fn run_folder(
        &self,
        folder: &BackupFolderRecord,
        settings: &BackupSettings,
    ) -> Result<BackupRunReport, BackupError> {
        // 同一文件夹（以及整个引擎）的扫描/上传串行化；第二个调用会等第一个
        // 完成后再用最新索引判断为“未变化”，不会重复上传或产生额外版本。
        let _run_guard = self.inner.run_lock.lock().await;
        let folder_id = folder.id;
        let mut report = BackupRunReport {
            folder_id,
            status: BackupFolderStatus::Scanning,
            scanned_files: 0,
            hashed_files: 0,
            uploaded_files: 0,
            updated_files: 0,
            unchanged_files: 0,
            unreadable_files: 0,
            retry_later: 0,
            failed_files: 0,
            skipped_excluded: 0,
        };

        if !self.inner.client.is_unlocked() {
            self.set_folder_status(
                folder_id,
                BackupFolderStatus::NeedsUnlock,
                Some(BackupPauseReason::NeedsUnlock),
            );
            report.status = BackupFolderStatus::NeedsUnlock;
            return Ok(report);
        }
        if settings.paused || !folder.enabled || self.is_paused(folder_id) {
            self.set_folder_status(
                folder_id,
                BackupFolderStatus::Paused,
                Some(BackupPauseReason::User),
            );
            report.status = BackupFolderStatus::Paused;
            return Ok(report);
        }

        let user_id = folder.user_id;
        // 普通备份文件夹自动排除微信数据根（design D2，2026-11-01 审查者决定）。
        // 微信来源本身就在数据根内部，不做这个排除。
        let mut exclude_subfolders = folder.exclude_subfolders.clone();
        if folder.source_kind == BackupFolderSourceKind::User {
            self.ensure_wechat_roots().await;
            let roots = self.wechat_data_roots();
            let wechat = exclusions_for(&folder.local_path, &roots);
            if wechat.scanned_root_inside_wechat {
                // 整个文件夹就是微信数据：全部由“微信备份”处理。
                self.set_folder_status(folder_id, BackupFolderStatus::IdleOk, None);
                report.status = BackupFolderStatus::IdleOk;
                return Ok(report);
            }
            exclude_subfolders.extend(wechat.relative_subfolders);
        }
        self.set_folder_status(folder_id, BackupFolderStatus::Scanning, None);
        let exclusions = Exclusions::compile(&folder.exclude_globs, &exclude_subfolders)?;
        let scan = {
            let root = folder.local_path.clone();
            let name = folder.local_name.clone();
            let include_hidden = settings.include_hidden;
            tokio::task::spawn_blocking(move || {
                let options = WalkOptions {
                    include_hidden,
                    exclusions: &exclusions,
                };
                scan_tree(&root, &name, &options)
            })
            .await
            .map_err(|_| BackupError::InvalidConfig("扫描任务失败"))?
        };
        let tree = match scan {
            Ok(tree) => tree,
            Err(error) => {
                self.record_folder_error(user_id, folder, &error).await?;
                report.status = BackupFolderStatus::Error {
                    unreadable_count: 0,
                };
                return Ok(report);
            }
        };
        let (scanned_files, _) = tree.counts();
        report.scanned_files = scanned_files;
        report.skipped_excluded = tree.excluded;
        report.unreadable_files = tree.unreadable;

        let account = self.inner.client.ensure_writable()?;
        let (cloud_folder_id, cloud_path) = match self.resolve_target(&account, folder).await {
            Ok(target) => target,
            Err(BackupError::Client(error)) => {
                self.handle_run_client_error(user_id, folder, &mut report, &error)
                    .await?;
                return Ok(report);
            }
            Err(error) => {
                self.record_folder_error(user_id, folder, &error).await?;
                report.status = BackupFolderStatus::Error {
                    unreadable_count: report.unreadable_files,
                };
                return Ok(report);
            }
        };
        let now = unix_seconds(self.inner.clock.now());
        self.inner
            .state
            .update_backup_folder_cloud(
                user_id,
                folder_id,
                Some(cloud_folder_id),
                cloud_path,
                None,
                now,
            )
            .await?;

        let index: HashMap<String, BackupIndexRecord> = self
            .inner
            .state
            .list_backup_index(user_id, folder_id)
            .await?
            .into_iter()
            .map(|record| (record.relative_path.clone(), record))
            .collect();
        let limiter = TokenBucket::new(settings.upload_limit_bytes_per_second.unwrap_or(0));
        let mut state = RunState {
            report,
            seen: HashSet::new(),
            processed: 0,
            stop: false,
            network_wait: false,
            completed: false,
        };
        match self
            .process_tree(
                user_id,
                folder,
                &tree,
                cloud_folder_id,
                &index,
                &limiter,
                &mut state,
            )
            .await
        {
            Ok(()) => {}
            Err(BackupError::Client(error)) => {
                self.handle_run_client_error(user_id, folder, &mut state.report, &error)
                    .await?;
                state.stop = true;
            }
            Err(error) => {
                self.record_folder_error(user_id, folder, &error).await?;
                state.report.status = BackupFolderStatus::Error {
                    unreadable_count: state.report.unreadable_files,
                };
                return Ok(state.report);
            }
        }

        // 只在完整扫描时清理本地已删除文件的索引；暂停/断网时保留未处理项。
        if state.completed {
            let stale: Vec<String> = index
                .keys()
                .filter(|key| !state.seen.contains(*key))
                .cloned()
                .collect();
            for relative in stale {
                self.inner
                    .state
                    .delete_backup_index(user_id, folder_id, relative)
                    .await?;
            }
        }

        let unlocked = self.inner.client.is_unlocked();
        let paused = self.is_paused(folder_id);
        let final_status = if !unlocked {
            // process_tree 可能在上传中途因锁定/退出登录提前返回；此时必须
            // 报 needs_unlock，而不是用户暂停。
            BackupFolderStatus::NeedsUnlock
        } else if state.network_wait {
            self.schedule_retry();
            BackupFolderStatus::Paused
        } else if paused {
            BackupFolderStatus::Paused
        } else if state.report.unreadable_files > 0 || state.report.failed_files > 0 {
            BackupFolderStatus::Error {
                unreadable_count: state.report.unreadable_files,
            }
        } else if state.stop {
            BackupFolderStatus::Error {
                unreadable_count: 0,
            }
        } else {
            BackupFolderStatus::IdleOk
        };
        // “稍后重试”的文件（被占用 / 上传中被修改）不会自己产生新的文件事件，
        // 因此本轮结束后安排一次重试；与断网重试共用 retry_at 机制。
        if unlocked && !paused && !state.network_wait && state.report.retry_later > 0 {
            self.schedule_retry();
        }
        let pause_reason = match final_status {
            BackupFolderStatus::Paused if state.network_wait => Some(BackupPauseReason::Network),
            BackupFolderStatus::Paused => Some(BackupPauseReason::User),
            BackupFolderStatus::NeedsUnlock => Some(BackupPauseReason::NeedsUnlock),
            _ => None,
        };
        self.set_folder_status(folder_id, final_status, pause_reason);
        if matches!(final_status, BackupFolderStatus::IdleOk) {
            let completed_at = unix_seconds(self.inner.clock.now());
            self.set_last_backup_at(folder_id, completed_at);
            self.inner
                .state
                .update_backup_folder_cloud(
                    user_id,
                    folder_id,
                    Some(cloud_folder_id),
                    None,
                    Some(completed_at),
                    completed_at,
                )
                .await?;
        }
        state.report.status = final_status;
        Ok(state.report)
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_tree(
        &self,
        user_id: Uuid,
        folder: &BackupFolderRecord,
        root: &LocalTree,
        cloud_root: Uuid,
        index: &HashMap<String, BackupIndexRecord>,
        limiter: &Option<Arc<TokenBucket>>,
        state: &mut RunState,
    ) -> Result<(), BackupError> {
        let mut queue: VecDeque<(&LocalTree, Uuid)> = VecDeque::new();
        queue.push_back((root, cloud_root));
        while let Some((local, cloud_folder_id)) = queue.pop_front() {
            if state.stop || self.is_paused(folder.id) || !self.inner.client.is_unlocked() {
                return Ok(());
            }
            let nodes = self
                .inner
                .client
                .list_folder_decrypted(cloud_folder_id)
                .await?;
            let mut by_key: HashMap<String, DecryptedNode> = HashMap::new();
            for node in nodes {
                by_key.insert(name_key(node.name()), node);
            }

            for dir in &local.dirs {
                if state.stop || self.is_paused(folder.id) {
                    return Ok(());
                }
                let key = name_key(&dir.name);
                let cloud_id = match by_key.get(&key) {
                    Some(node) if node.kind() == NodeKind::Folder => node.view.id,
                    Some(_) => {
                        let (files, _) = dir.counts();
                        state.report.failed_files = state.report.failed_files.saturating_add(files);
                        self.record_activity(
                            user_id,
                            folder.id,
                            dir.relative.clone(),
                            BackupActivityKind::Failed,
                            Some("云端同一位置已有同名文件，已跳过这个文件夹".to_owned()),
                        )
                        .await?;
                        continue;
                    }
                    None => match self.ensure_cloud_folder(cloud_folder_id, &dir.name).await {
                        Ok(id) => id,
                        Err(error) => {
                            self.handle_run_client_error(
                                user_id,
                                folder,
                                &mut state.report,
                                &error,
                            )
                            .await?;
                            state.stop = true;
                            return Ok(());
                        }
                    },
                };
                queue.push_back((dir, cloud_id));
            }

            for file in &local.files {
                if state.stop || self.is_paused(folder.id) || !self.inner.client.is_unlocked() {
                    return Ok(());
                }
                state.seen.insert(file.relative.clone());
                let key = name_key(&file.name);
                let existing = by_key.get(&key);
                if let Some(node) = existing
                    && node.kind() == NodeKind::Folder
                {
                    state.report.failed_files = state.report.failed_files.saturating_add(1);
                    self.record_activity(
                        user_id,
                        folder.id,
                        file.relative.clone(),
                        BackupActivityKind::Failed,
                        Some("云端同一位置已有同名文件夹，未上传这个文件".to_owned()),
                    )
                    .await?;
                    continue;
                }
                self.process_file(
                    user_id,
                    folder,
                    cloud_folder_id,
                    file,
                    existing,
                    index,
                    limiter,
                    state,
                )
                .await?;
                state.processed = state.processed.saturating_add(1);
                if state.report.scanned_files > 0 {
                    self.set_folder_status(
                        folder.id,
                        BackupFolderStatus::Uploading {
                            done: state.processed.min(state.report.scanned_files),
                            total: state.report.scanned_files,
                        },
                        None,
                    );
                }
            }
        }
        state.completed = true;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_file(
        &self,
        user_id: Uuid,
        folder: &BackupFolderRecord,
        cloud_folder_id: Uuid,
        file: &LocalFile,
        existing: Option<&DecryptedNode>,
        index: &HashMap<String, BackupIndexRecord>,
        limiter: &Option<Arc<TokenBucket>>,
        state: &mut RunState,
    ) -> Result<(), BackupError> {
        let key = name_key(&file.name);
        // 1) 索引快速跳过：大小与修改时间均未变，且云端节点未变化 → 不读文件、不请求。
        if let Some(entry) = index.get(&file.relative)
            && entry.size == file.size
            && entry.mtime_ns == file.mtime_ns
            && existing.is_some_and(|node| node.view.id == entry.cloud_node_id)
        {
            state.report.unchanged_files = state.report.unchanged_files.saturating_add(1);
            return Ok(());
        }

        // 2) 计算 SHA-256；读取前后指纹不一致说明文件正在被修改，稍后重试。
        let expected = FileFingerprint {
            size: file.size,
            mtime_ns: file.mtime_ns,
        };
        let path = file.path.clone();
        let hash = tokio::task::spawn_blocking(move || hash_file_stable(&path, expected))
            .await
            .map_err(|_| BackupError::InvalidConfig("哈希任务失败"))?;
        let hash = match hash {
            Ok(Some(hash)) => hash,
            Ok(None) => {
                state.report.retry_later = state.report.retry_later.saturating_add(1);
                self.record_activity(
                    user_id,
                    folder.id,
                    file.relative.clone(),
                    BackupActivityKind::RetryLater,
                    Some("文件正在被修改，稍后重试".to_owned()),
                )
                .await?;
                return Ok(());
            }
            Err(ClientError::Io(_)) => {
                state.report.unreadable_files = state.report.unreadable_files.saturating_add(1);
                state.report.retry_later = state.report.retry_later.saturating_add(1);
                self.record_activity(
                    user_id,
                    folder.id,
                    file.relative.clone(),
                    BackupActivityKind::RetryLater,
                    Some("文件被其他程序占用，稍后重试".to_owned()),
                )
                .await?;
                return Ok(());
            }
            Err(error) => return Err(BackupError::Client(error)),
        };
        state.report.hashed_files = state.report.hashed_files.saturating_add(1);

        // 3) 与云端当前版本比较（常量时间）。
        if let Some(node) = existing {
            if let Some(remote) = &node.content_sha256 {
                if Client::hashes_equal(&hash, remote) {
                    self.update_index(user_id, folder, file, node.view.id, &hash)
                        .await?;
                    state.report.unchanged_files = state.report.unchanged_files.saturating_add(1);
                    self.record_activity(
                        user_id,
                        folder.id,
                        file.relative.clone(),
                        BackupActivityKind::Unchanged,
                        None,
                    )
                    .await?;
                    return Ok(());
                }
            } else if node.view.size == Some(file.size)
                && node.view.mtime == Some(file.mtime_ns / 1_000_000_000)
            {
                // 旧版本没有 content_sha256：退回大小 + 修改时间比较（folder-transfer spec）。
                self.update_index(user_id, folder, file, node.view.id, &hash)
                    .await?;
                state.report.unchanged_files = state.report.unchanged_files.saturating_add(1);
                self.record_activity(
                    user_id,
                    folder.id,
                    file.relative.clone(),
                    BackupActivityKind::Unchanged,
                    None,
                )
                .await?;
                return Ok(());
            }
        }

        // 4) 上传新文件或新版本（后台任务带限速器）。
        let task = match existing {
            Some(node) => self.inner.client.upload_new_version_with_limiter(
                node.view.id,
                file.path.clone(),
                limiter.clone(),
            ),
            None => self.inner.client.upload_file_with_limiter(
                cloud_folder_id,
                file.path.clone(),
                limiter.clone(),
            ),
        };
        let task = match task {
            Ok(task) => task,
            Err(error) => {
                self.handle_file_client_error(user_id, folder, file, state, error)
                    .await?;
                return Ok(());
            }
        };
        match task.run().await {
            Ok(outcome) => {
                let kind = if existing.is_some() {
                    state.report.updated_files = state.report.updated_files.saturating_add(1);
                    BackupActivityKind::NewVersion
                } else {
                    state.report.uploaded_files = state.report.uploaded_files.saturating_add(1);
                    BackupActivityKind::Added
                };
                self.update_index(user_id, folder, file, outcome.node_id, &hash)
                    .await?;
                self.record_activity(user_id, folder.id, file.relative.clone(), kind, None)
                    .await?;
            }
            Err(error) => {
                // 新文件上传时若另一个客户端刚创建了同名文件，服务端返回
                // name_conflict；重新列出父目录，复用同内容节点或追加新版本。
                if existing.is_none()
                    && error.api_code() == Some(&ApiErrorCode::NameConflict)
                    && let Ok(nodes) = self
                        .inner
                        .client
                        .list_folder_decrypted(cloud_folder_id)
                        .await
                    && let Some(node) = nodes.into_iter().find(|node| name_key(node.name()) == key)
                {
                    if node.kind() == NodeKind::Folder {
                        state.report.failed_files = state.report.failed_files.saturating_add(1);
                        self.record_activity(
                            user_id,
                            folder.id,
                            file.relative.clone(),
                            BackupActivityKind::Failed,
                            Some("云端同一位置已有同名文件夹，未上传这个文件".to_owned()),
                        )
                        .await?;
                        return Ok(());
                    }
                    if node
                        .content_sha256
                        .as_deref()
                        .is_some_and(|remote| Client::hashes_equal(&hash, remote))
                    {
                        self.update_index(user_id, folder, file, node.view.id, &hash)
                            .await?;
                        state.report.unchanged_files =
                            state.report.unchanged_files.saturating_add(1);
                        self.record_activity(
                            user_id,
                            folder.id,
                            file.relative.clone(),
                            BackupActivityKind::Unchanged,
                            None,
                        )
                        .await?;
                        return Ok(());
                    }
                    match self.inner.client.upload_new_version_with_limiter(
                        node.view.id,
                        file.path.clone(),
                        limiter.clone(),
                    ) {
                        Ok(task) => match task.run().await {
                            Ok(outcome) => {
                                state.report.updated_files =
                                    state.report.updated_files.saturating_add(1);
                                self.update_index(user_id, folder, file, outcome.node_id, &hash)
                                    .await?;
                                self.record_activity(
                                    user_id,
                                    folder.id,
                                    file.relative.clone(),
                                    BackupActivityKind::NewVersion,
                                    None,
                                )
                                .await?;
                                return Ok(());
                            }
                            Err(retry_error) => {
                                self.handle_file_client_error(
                                    user_id,
                                    folder,
                                    file,
                                    state,
                                    retry_error,
                                )
                                .await?;
                                return Ok(());
                            }
                        },
                        Err(retry_error) => {
                            self.handle_file_client_error(
                                user_id,
                                folder,
                                file,
                                state,
                                retry_error,
                            )
                            .await?;
                            return Ok(());
                        }
                    }
                }
                self.handle_file_client_error(user_id, folder, file, state, error)
                    .await?;
            }
        }
        Ok(())
    }

    async fn update_index(
        &self,
        user_id: Uuid,
        folder: &BackupFolderRecord,
        file: &LocalFile,
        cloud_node_id: Uuid,
        content_sha256: &str,
    ) -> Result<(), BackupError> {
        self.inner
            .state
            .upsert_backup_index(BackupIndexRecord {
                user_id,
                folder_id: folder.id,
                relative_path: file.relative.clone(),
                size: file.size,
                mtime_ns: file.mtime_ns,
                cloud_node_id,
                content_sha256: content_sha256.to_owned(),
                updated_at: unix_seconds(self.inner.clock.now()),
            })
            .await?;
        Ok(())
    }

    async fn handle_file_client_error(
        &self,
        user_id: Uuid,
        folder: &BackupFolderRecord,
        file: &LocalFile,
        state: &mut RunState,
        error: ClientError,
    ) -> Result<(), BackupError> {
        if matches!(&error, ClientError::FileChanged) {
            state.report.retry_later = state.report.retry_later.saturating_add(1);
            self.record_activity(
                user_id,
                folder.id,
                file.relative.clone(),
                BackupActivityKind::RetryLater,
                Some("文件在上传过程中被修改，稍后重试".to_owned()),
            )
            .await?;
            return Ok(());
        }
        if matches!(&error, ClientError::Io(_)) {
            state.report.unreadable_files = state.report.unreadable_files.saturating_add(1);
            state.report.retry_later = state.report.retry_later.saturating_add(1);
            self.record_activity(
                user_id,
                folder.id,
                file.relative.clone(),
                BackupActivityKind::RetryLater,
                Some("文件被其他程序占用，稍后重试".to_owned()),
            )
            .await?;
            return Ok(());
        }
        if is_network_error(&error) {
            state.network_wait = true;
            state.stop = true;
        } else if matches!(error.transfer_code(), TransferErrorCode::QuotaExceeded) {
            state.stop = true;
        }
        state.report.failed_files = state.report.failed_files.saturating_add(1);
        self.record_activity(
            user_id,
            folder.id,
            file.relative.clone(),
            BackupActivityKind::Failed,
            Some(error.to_string()),
        )
        .await?;
        Ok(())
    }

    async fn handle_run_client_error(
        &self,
        user_id: Uuid,
        folder: &BackupFolderRecord,
        report: &mut BackupRunReport,
        error: &ClientError,
    ) -> Result<(), BackupError> {
        let (status, reason) = if matches!(error, ClientError::Locked) {
            (
                BackupFolderStatus::NeedsUnlock,
                Some(BackupPauseReason::NeedsUnlock),
            )
        } else if is_network_error(error) {
            self.schedule_retry();
            (BackupFolderStatus::Paused, Some(BackupPauseReason::Network))
        } else {
            (
                BackupFolderStatus::Error {
                    unreadable_count: report.unreadable_files,
                },
                None,
            )
        };
        report.failed_files = report.failed_files.saturating_add(1);
        self.record_activity(
            user_id,
            folder.id,
            folder.local_name.clone(),
            BackupActivityKind::Failed,
            Some(error.to_string()),
        )
        .await?;
        self.set_folder_status(folder.id, status, reason);
        report.status = status;
        Ok(())
    }
}

struct RunState {
    report: BackupRunReport,
    seen: HashSet<String>,
    processed: u64,
    stop: bool,
    network_wait: bool,
    completed: bool,
}

fn is_network_error(error: &ClientError) -> bool {
    matches!(
        error.transfer_code(),
        TransferErrorCode::Network | TransferErrorCode::Server | TransferErrorCode::RateLimited
    )
}

async fn run_scheduler(
    inner: Arc<Inner>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    mut watch: mpsc::UnboundedReceiver<WatchSignal>,
) {
    let engine = BackupEngine {
        inner: inner.clone(),
    };
    let mut next_full = inner.clock.now() + inner.config.full_scan_interval;
    let mut was_locked = !inner.client.is_unlocked();
    // 重试间隔可能比 5 秒 tick 更短；取较小值，保证重试按时触发（测试也可
    // 用很短的 retry_interval 驱动调度器，而不必等 5 秒）。
    let tick = SCHEDULER_TICK
        .min(inner.config.retry_interval)
        .max(Duration::from_millis(10));
    loop {
        // 未解锁：暂停并报告 needs_unlock；解锁后自动继续。
        if !inner.client.is_unlocked() {
            engine.mark_needs_unlock();
            was_locked = true;
        } else if was_locked {
            was_locked = false;
            if let Err(error) = engine.scan_all_now().await {
                tracing::warn!(error = %error, "解锁后继续备份失败");
            }
        }

        let now = inner.clock.now();
        if now >= next_full {
            // 每次全量核对刷新一次微信数据根（不是每个文件都发现）。
            engine.refresh_wechat_data_roots().await;
            if let Err(error) = engine.scan_all_now().await {
                tracing::warn!(error = %error, "定时全量核对失败");
            }
            next_full = now + inner.config.full_scan_interval;
        }
        let retry_due = inner
            .runtime
            .lock()
            .ok()
            .and_then(|runtime| runtime.retry_at)
            .is_some_and(|at| now >= at);
        if retry_due {
            if let Ok(mut runtime) = inner.runtime.lock() {
                runtime.retry_at = None;
            }
            if let Err(error) = engine.scan_all_now().await {
                tracing::warn!(error = %error, "断网恢复后的重试失败");
            }
        }

        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(Command::Wake) => {
                        engine.sync_watchers().await;
                        if inner.client.is_unlocked()
                            && let Err(error) = engine.scan_all_now().await
                        {
                            tracing::warn!(error = %error, "手动/启动核对失败");
                        }
                    }
                    Some(Command::Shutdown) | None => break,
                }
            }
            signal = watch.recv() => {
                let Some(signal) = signal else {
                    break;
                };
                // 5 秒防抖：等待期间合并的事件一起处理。
                inner.clock.sleep(inner.config.debounce).await;
                let mut folder_ids = HashSet::new();
                let mut full_scan = signal.needs_full_scan;
                folder_ids.insert(signal.folder_id);
                while let Ok(more) = watch.try_recv() {
                    full_scan |= more.needs_full_scan;
                    folder_ids.insert(more.folder_id);
                }
                if !inner.client.is_unlocked() {
                    engine.mark_needs_unlock();
                    continue;
                }
                if full_scan {
                    if let Err(error) = engine.scan_all_now().await {
                        tracing::warn!(error = %error, "监听溢出后的全量核对失败");
                    }
                } else {
                    for folder_id in folder_ids {
                        if let Err(error) = engine.scan_folder_now(folder_id).await {
                            tracing::warn!(folder_id = %folder_id, error = %error, "文件变化触发的备份失败");
                        }
                    }
                }
            }
            // 调度 tick 用真实时间：注入的假时钟可能立刻返回，不能让它把运行时
            // 忙等住。业务等待（防抖/重试）仍走注入时钟，测试可以控制。
            () = tokio::time::sleep(tick) => {}
        }
    }
}

/// 汇总当前状态。
fn summarize(runtime: &Runtime) -> BackupSummary {
    let folder_count = runtime.statuses.len() as u64;
    let mut unreadable = 0_u64;
    let mut uploading: Option<(u64, u64)> = None;
    let mut paused = runtime.settings.paused;
    let mut needs_unlock = false;
    let mut error = false;
    for state in runtime.statuses.values() {
        match state.status {
            BackupFolderStatus::NeedsUnlock => needs_unlock = true,
            BackupFolderStatus::Paused => paused = true,
            BackupFolderStatus::Error { unreadable_count } => {
                error = true;
                unreadable = unreadable.saturating_add(unreadable_count);
            }
            BackupFolderStatus::Uploading { done, total } => {
                let (current_done, current_total) = uploading.unwrap_or((0, 0));
                uploading = Some((
                    current_done.saturating_add(done),
                    current_total.saturating_add(total),
                ));
            }
            BackupFolderStatus::IdleOk | BackupFolderStatus::Scanning => {}
        }
    }
    let status = if needs_unlock {
        BackupSummaryStatus::NeedsUnlock
    } else if paused {
        BackupSummaryStatus::Paused
    } else if let Some((done, total)) = uploading {
        BackupSummaryStatus::Uploading { done, total }
    } else if error {
        BackupSummaryStatus::Error {
            unreadable_count: unreadable,
        }
    } else {
        BackupSummaryStatus::AllBackedUp
    };
    BackupSummary {
        status,
        folder_count,
        unreadable_count: unreadable,
        last_checked_at: runtime.last_checked_at,
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf, BackupError> {
    if path.as_os_str().is_empty() {
        return Err(BackupError::InvalidConfig("备份路径不能为空"));
    }
    std::path::absolute(path).map_err(|_| BackupError::InvalidConfig("备份路径无效"))
}

fn category_order(category: WechatCategory) -> usize {
    WechatCategory::ALL
        .iter()
        .position(|candidate| *candidate == category)
        .unwrap_or(WechatCategory::ALL.len())
}

/// 一个类别有多个来源时（图片与视频的两个路径）汇总成界面状态。
fn combine_statuses(sources: &[&WechatBackupSource]) -> Option<BackupFolderStatus> {
    if sources.is_empty() {
        return None;
    }
    if sources
        .iter()
        .any(|source| source.status == BackupFolderStatus::NeedsUnlock)
    {
        return Some(BackupFolderStatus::NeedsUnlock);
    }
    let mut uploading: Option<(u64, u64)> = None;
    for source in sources {
        if let BackupFolderStatus::Uploading { done, total } = source.status {
            let (current_done, current_total) = uploading.unwrap_or((0, 0));
            uploading = Some((
                current_done.saturating_add(done),
                current_total.saturating_add(total),
            ));
        }
    }
    if let Some((done, total)) = uploading {
        return Some(BackupFolderStatus::Uploading { done, total });
    }
    if sources
        .iter()
        .any(|source| source.status == BackupFolderStatus::Paused)
    {
        return Some(BackupFolderStatus::Paused);
    }
    if sources
        .iter()
        .any(|source| matches!(source.status, BackupFolderStatus::Error { .. }))
    {
        let unreadable_count = sources
            .iter()
            .filter_map(|source| match source.status {
                BackupFolderStatus::Error { unreadable_count } => Some(unreadable_count),
                _ => None,
            })
            .fold(0_u64, u64::saturating_add);
        return Some(BackupFolderStatus::Error { unreadable_count });
    }
    if sources
        .iter()
        .any(|source| source.status == BackupFolderStatus::Scanning)
    {
        return Some(BackupFolderStatus::Scanning);
    }
    Some(BackupFolderStatus::IdleOk)
}

fn path_conflict(existing: &Path, candidate: &Path) -> Option<&'static str> {
    let existing = comparable_path(existing);
    let candidate = comparable_path(candidate);
    if existing == candidate {
        Some("这个文件夹已经添加过了")
    } else if path_within(&candidate, &existing) || path_within(&existing, &candidate) {
        Some("备份文件夹不能互相包含")
    } else {
        None
    }
}

fn ensure_no_path_conflict(
    existing: &[BackupFolderRecord],
    candidate: &Path,
    exclude: Option<Uuid>,
) -> Result<(), BackupError> {
    for folder in existing {
        if exclude == Some(folder.id) {
            continue;
        }
        if let Some(message) = path_conflict(&folder.local_path, candidate) {
            return Err(BackupError::InvalidConfig(message));
        }
    }
    Ok(())
}

fn local_name(path: &Path) -> Result<String, BackupError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or(BackupError::InvalidConfig("备份路径没有名称"))?;
    let name = crate::io_path::normalize_cloud_name(name);
    if validate_name(&name).is_some() {
        return Err(BackupError::InvalidConfig("备份文件夹名称不可用"));
    }
    Ok(name)
}

fn unix_seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}

/// 单个文件首次上传所需的密文字节数（每个文件至少 1 块，空文件也有 1 块开销）。
pub(crate) fn projected_file_bytes(size: u64) -> u64 {
    let count = crypto_core::chunk_count_for_size(size).unwrap_or(1);
    size.saturating_add(u64::from(count) * crypto_core::CHUNK_CIPHERTEXT_OVERHEAD as u64)
}

/// 多个文件的首次上传密文字节总数；开销按每个文件自己的块数计算。
pub(crate) fn projected_total_bytes(sizes: &[u64]) -> u64 {
    sizes.iter().fold(0_u64, |total, size| {
        total.saturating_add(projected_file_bytes(*size))
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::Path;

    use super::*;

    #[test]
    fn projected_upload_bytes_counts_chunk_overhead_per_file() {
        assert_eq!(projected_file_bytes(0), crypto_core::FRAME_OVERHEAD as u64);
        assert_eq!(
            projected_total_bytes(&[10, 10, 10]),
            30 + 3 * crypto_core::CHUNK_CIPHERTEXT_OVERHEAD as u64
        );
        // 三个 10 字节文件各自至少 1 块，不能只按总字节数算一次块开销。
        assert_eq!(
            projected_total_bytes(&[10, 10, 10]),
            3 * projected_file_bytes(10)
        );
    }

    #[test]
    fn path_conflict_detects_duplicates_and_nesting() {
        assert_eq!(
            path_conflict(Path::new("C:/backup/work"), Path::new("C:/backup/work")),
            Some("这个文件夹已经添加过了")
        );
        assert_eq!(
            path_conflict(Path::new("C:/backup/work"), Path::new("C:/backup/work/sub")),
            Some("备份文件夹不能互相包含")
        );
        assert_eq!(
            path_conflict(Path::new("C:/backup/work/sub"), Path::new("C:/backup/work")),
            Some("备份文件夹不能互相包含")
        );
        assert_eq!(
            path_conflict(Path::new("C:/backup/work"), Path::new("C:/backup/other")),
            None
        );
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn path_comparison_is_case_insensitive_on_windows_and_macos() {
        assert_eq!(
            path_conflict(Path::new("C:/Backup/Work"), Path::new("c:/backup/work")),
            Some("这个文件夹已经添加过了")
        );
    }
}
