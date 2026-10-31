//! 桌面备份引擎的公开展示类型。
//!
//! 这些类型会经 flutter_rust_bridge 交给界面。它们只包含展示数据（名称、相对
//! 路径、计数、时间、状态），不包含密钥、令牌、包裹密钥或加密元数据。本机
//! 绝对路径只在 [`BackupFolder`] 配置里出现（用户自己选择的路径，界面需要显示）。

use std::path::PathBuf;
use std::time::Duration;

use uuid::Uuid;

use crate::state::BackupFolderRecord;

/// 文件监听防抖窗口（design D6：5 秒）。
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_secs(5);
/// 全量核对周期（design D6：每 6 小时）。
pub const DEFAULT_FULL_SCAN_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// 断网 / 服务器不可用后的重试间隔。
pub const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_secs(60);

/// 某个备份文件夹的运行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupFolderStatus {
    /// 最近一次核对完成，没有待上传项。
    IdleOk,
    /// 正在扫描本地目录。
    Scanning,
    /// 正在上传；`done`/`total` 是文件数。
    Uploading { done: u64, total: u64 },
    /// 已暂停（用户暂停、文件夹停用、断网等待或需要解锁）。
    Paused,
    /// 有文件无法读取；其余文件不受影响。
    Error { unreadable_count: u64 },
    /// 内存中没有主密钥，解锁后自动继续。
    NeedsUnlock,
}

impl BackupFolderStatus {
    /// 稳定字符串，供桥接层与界面使用。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IdleOk => "idle_ok",
            Self::Scanning => "scanning",
            Self::Uploading { .. } => "uploading",
            Self::Paused => "paused",
            Self::Error { .. } => "error",
            Self::NeedsUnlock => "needs_unlock",
        }
    }
}

/// 暂停原因；界面据此显示“已暂停”还是“需要解锁”。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupPauseReason {
    User,
    NeedsUnlock,
    Network,
}

/// 单个备份文件夹的状态快照（事件流也用它）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFolderState {
    pub folder_id: Uuid,
    pub status: BackupFolderStatus,
    pub pause_reason: Option<BackupPauseReason>,
    /// 最近一次完整核对完成的时间（Unix 秒）。
    pub last_backup_at: Option<i64>,
}

/// 全局汇总状态（托盘弹窗用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupSummaryStatus {
    AllBackedUp,
    Uploading { done: u64, total: u64 },
    Paused,
    NeedsUnlock,
    Error { unreadable_count: u64 },
}

impl BackupSummaryStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllBackedUp => "all_backed_up",
            Self::Uploading { .. } => "uploading",
            Self::Paused => "paused",
            Self::NeedsUnlock => "needs_unlock",
            Self::Error { .. } => "error",
        }
    }
}

/// 全局汇总（托盘用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupSummary {
    pub status: BackupSummaryStatus,
    pub folder_count: u64,
    pub unreadable_count: u64,
    /// 最近一次核对时间（Unix 秒）。
    pub last_checked_at: Option<i64>,
}

impl Default for BackupSummary {
    fn default() -> Self {
        Self {
            status: BackupSummaryStatus::AllBackedUp,
            folder_count: 0,
            unreadable_count: 0,
            last_checked_at: None,
        }
    }
}

/// 最近活动类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupActivityKind {
    /// 新增文件。
    Added,
    /// 上传了新版本。
    NewVersion,
    /// 大小/时间变化但内容与云端相同，只更新本地索引。
    Unchanged,
    /// 文件在上传中被修改或被占用，稍后重试。
    RetryLater,
    /// 上传失败。
    Failed,
}

impl BackupActivityKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::NewVersion => "new_version",
            Self::Unchanged => "unchanged",
            Self::RetryLater => "retry_later",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "added" => Some(Self::Added),
            "new_version" => Some(Self::NewVersion),
            "unchanged" => Some(Self::Unchanged),
            "retry_later" => Some(Self::RetryLater),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// 一条最近活动（只保存在本机，最多 200 条）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupActivity {
    pub id: i64,
    pub folder_id: Uuid,
    /// 相对备份根目录的路径（界面展示用，不含本机绝对路径）。
    pub relative_path: String,
    pub kind: BackupActivityKind,
    /// Unix 秒。
    pub occurred_at: i64,
    /// 面向用户的失败原因；不含本机绝对路径。
    pub message: Option<String>,
}

/// 状态与活动事件（`tokio::sync::broadcast`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackupEvent {
    Folder(BackupFolderState),
    Activity(BackupActivity),
    Summary(BackupSummary),
}

/// 备份文件夹配置（界面展示数据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFolder {
    pub id: Uuid,
    pub local_path: PathBuf,
    pub local_name: String,
    /// 用户指定的云端目标；`None` 表示默认路径。
    pub cloud_target_id: Option<Uuid>,
    /// 已解析的云端叶子文件夹；首次备份前为 `None`。
    pub cloud_folder_id: Option<Uuid>,
    /// 默认目标的展示路径；自定义目标为 `None`。
    pub cloud_path: Option<String>,
    pub exclude_globs: Vec<String>,
    pub exclude_subfolders: Vec<String>,
    pub enabled: bool,
    pub last_backup_at: Option<i64>,
}

impl From<BackupFolderRecord> for BackupFolder {
    fn from(record: BackupFolderRecord) -> Self {
        Self {
            id: record.id,
            local_path: record.local_path,
            local_name: record.local_name,
            cloud_target_id: record.cloud_target_id,
            cloud_folder_id: record.cloud_folder_id,
            cloud_path: record.cloud_path,
            exclude_globs: record.exclude_globs,
            exclude_subfolders: record.exclude_subfolders,
            enabled: record.enabled,
            last_backup_at: record.last_backup_at,
        }
    }
}

/// 添加备份文件夹的请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFolderDraft {
    pub local_path: PathBuf,
    /// `None` = 默认 `全部文件/电脑备份/<设备名>/<本地文件夹名>`。
    pub cloud_target_id: Option<Uuid>,
    pub exclude_globs: Vec<String>,
    pub exclude_subfolders: Vec<String>,
}

impl BackupFolderDraft {
    #[must_use]
    pub fn new(local_path: impl Into<PathBuf>) -> Self {
        Self {
            local_path: local_path.into(),
            cloud_target_id: None,
            exclude_globs: Vec::new(),
            exclude_subfolders: Vec::new(),
        }
    }
}

/// 修改备份文件夹；`None` 表示不修改该字段。
///
/// `cloud_target_id` 是双层 `Option`：`None` = 不修改，`Some(None)` = 恢复默认
/// 云端路径，`Some(Some(id))` = 指定云端目标文件夹。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackupFolderUpdate {
    /// 更换本地路径：会清空该文件夹的本地索引并在下次扫描时重新建立（云端不删除）。
    pub local_path: Option<PathBuf>,
    pub cloud_target_id: Option<Option<Uuid>>,
    pub exclude_globs: Option<Vec<String>>,
    pub exclude_subfolders: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

/// 添加前的预检结果：文件数、首次需上传字节数（含每块加密开销）与剩余配额。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupPreflight {
    pub file_count: u64,
    /// 明文总字节数。
    pub total_bytes: u64,
    /// 首次需要上传的密文字节数（含每块 41 字节开销）。
    pub upload_bytes: u64,
    pub excluded_count: u64,
    pub unreadable_count: u64,
    /// 剩余云端空间；未登录或查询失败时为 `None`。
    pub remaining_quota_bytes: Option<u64>,
}

/// 一次备份运行的结果统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupRunReport {
    pub folder_id: Uuid,
    pub status: BackupFolderStatus,
    pub scanned_files: u64,
    /// 本次真正读取并计算 SHA-256 的文件数；未变化文件不计入。
    pub hashed_files: u64,
    pub uploaded_files: u64,
    pub updated_files: u64,
    pub unchanged_files: u64,
    pub unreadable_files: u64,
    pub retry_later: u64,
    pub failed_files: u64,
    pub skipped_excluded: u64,
}

/// 全局备份设置（持久化在本地状态库）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackupSettings {
    /// 备份隐藏文件（以 `.` 开头或带 Windows HIDDEN 属性）；默认关闭。
    pub include_hidden: bool,
    /// 后台备份上传限速（字节/秒）；`None` 表示不限制。手动上传不受影响。
    pub upload_limit_bytes_per_second: Option<u64>,
    /// 全局暂停。
    pub paused: bool,
}

/// 备份引擎配置。
#[derive(Debug, Clone)]
pub struct BackupEngineConfig {
    /// 设备名，用于默认云端路径 `全部文件/电脑备份/<设备名>/<本地文件夹名>`。
    pub device_name: String,
    /// 文件监听防抖窗口；默认 5 秒。
    pub debounce: Duration,
    /// 全量核对周期；默认 6 小时。
    pub full_scan_interval: Duration,
    /// 断网 / 服务器不可用后的重试间隔；默认 60 秒。
    pub retry_interval: Duration,
    /// 是否启用文件监听。测试可关闭，只用显式“立即备份”驱动。
    pub watch_files: bool,
}

impl BackupEngineConfig {
    #[must_use]
    pub fn new(device_name: impl Into<String>) -> Self {
        Self {
            device_name: device_name.into(),
            debounce: DEFAULT_DEBOUNCE,
            full_scan_interval: DEFAULT_FULL_SCAN_INTERVAL,
            retry_interval: DEFAULT_RETRY_INTERVAL,
            watch_files: true,
        }
    }

    /// 本机默认设备名（Windows `COMPUTERNAME`，类 Unix `HOSTNAME`）。
    #[must_use]
    pub fn default_device_name() -> String {
        let raw = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "computer".to_owned());
        let cleaned: String = raw
            .chars()
            .map(|character| {
                if character.is_control()
                    || matches!(
                        character,
                        '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                    )
                {
                    '_'
                } else {
                    character
                }
            })
            .take(64)
            .collect();
        let cleaned = cleaned.trim().to_owned();
        if cleaned.is_empty() {
            "computer".to_owned()
        } else {
            cleaned
        }
    }
}

impl Default for BackupEngineConfig {
    fn default() -> Self {
        Self::new(Self::default_device_name())
    }
}
