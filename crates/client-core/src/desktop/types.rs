//! 桌面备份引擎的公开展示类型。
//!
//! 这些类型会经 flutter_rust_bridge 交给界面。它们只包含展示数据（名称、相对
//! 路径、计数、时间、状态），不包含密钥、令牌、包裹密钥或加密元数据。本机
//! 绝对路径只在 [`BackupFolder`] 配置里出现（用户自己选择的路径，界面需要显示）。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use super::wechat::{
    WechatAccount, WechatCategory, WechatLocator, WechatProcessProbe, WechatVersion,
};
use crate::state::BackupFolderRecord;

/// 文件监听防抖窗口（design D6：5 秒）。
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_secs(5);
/// 全量核对周期（design D6：每 6 小时）。
pub const DEFAULT_FULL_SCAN_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// 断网 / 服务器不可用后的重试间隔。
pub const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_secs(60);
/// 数据库快照连续失败时的最长退避（一小时）。
pub const SNAPSHOT_RETRY_MAX: Duration = Duration::from_secs(3600);
/// 微信退出检测与数据库快照检查周期（design D3：每 10 秒）。
pub const DEFAULT_WECHAT_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(10);
/// 数据库快照默认保留份数（design D3：最近 3 份）。
pub const DEFAULT_WECHAT_SNAPSHOT_RETENTION: u8 = 3;
/// 保留份数的允许范围（界面设置 1–10）。
pub const WECHAT_SNAPSHOT_RETENTION_MIN: u8 = 1;
/// 保留份数的允许范围上限。
pub const WECHAT_SNAPSHOT_RETENTION_MAX: u8 = 10;

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
    /// 聊天记录数据库：微信正在运行，等它退出后再做快照（design D3）。
    WaitingForWechatExit,
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
            Self::WaitingForWechatExit => "waiting_for_wechat_exit",
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

/// 微信来源标识（事件流用：账号 + 类别）。
///
/// 只含展示数据；账号目录名本来就会显示在“微信备份”页面上，不写日志。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatEventSource {
    pub account_name: String,
    pub category: WechatCategory,
}

/// 单个备份文件夹的状态快照（事件流也用它）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFolderState {
    pub folder_id: Uuid,
    pub status: BackupFolderStatus,
    pub pause_reason: Option<BackupPauseReason>,
    /// 最近一次完整核对完成的时间（Unix 秒）。
    pub last_backup_at: Option<i64>,
    /// 普通备份文件夹为 `None`；微信来源带账号与类别，供事件流区分。
    pub wechat: Option<WechatEventSource>,
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
    /// 微信来源的账号与类别；普通备份文件夹为 `None`。
    pub wechat: Option<WechatEventSource>,
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
    /// 被自动排除的微信数据文件数（由“微信备份”单独处理，design D2）。
    pub excluded_wechat_file_count: u64,
    /// 被自动排除的微信数据明文字节数。
    pub excluded_wechat_bytes: u64,
}

/// 微信备份的首次上传预检（design D6：开启前的大小与配额提示）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WechatBackupPreflight {
    /// 勾选类别的文件数（含聊天记录数据库）。
    pub file_count: u64,
    /// 勾选类别的明文总字节数（含聊天记录数据库）。
    pub total_bytes: u64,
    /// 首次需要上传的密文字节数（逐文件计算块开销，与普通预检一致）。
    pub upload_bytes: u64,
    /// 取消“图片与视频”后首次需要上传的密文字节数（界面 quota 第三格）。
    pub upload_bytes_without_images_and_videos: u64,
    /// 聊天记录数据库类别的明文字节数（第 3 组快照会上传它）。
    pub database_bytes: u64,
    pub excluded_count: u64,
    pub unreadable_count: u64,
    /// 剩余云端空间；未登录或查询失败时为 `None`。
    pub remaining_quota_bytes: Option<u64>,
    /// 勾选内容是否超过剩余配额。
    pub over_quota: bool,
}

/// 一个已注册的微信来源备份项及其状态（供“微信备份”页面使用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatBackupSource {
    pub folder_id: Uuid,
    /// 数据根目录（界面展示用；与普通备份文件夹的本地路径同一隐私边界）。
    pub data_root: PathBuf,
    pub account_name: String,
    pub version: WechatVersion,
    pub category: WechatCategory,
    /// 相对数据根目录的来源路径（`/` 分隔）。
    pub relative_path: String,
    /// 已解析的云端展示路径；首次备份前为 `None`。
    pub cloud_path: Option<String>,
    pub enabled: bool,
    pub status: BackupFolderStatus,
    pub pause_reason: Option<BackupPauseReason>,
    pub last_backup_at: Option<i64>,
}

/// 一个账号内某个类别的汇总状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatCategoryBackupStatus {
    pub category: WechatCategory,
    /// 用户是否勾选了这个类别。
    pub selected: bool,
    /// 已注册来源的汇总状态；未勾选或尚未注册（数据库快照由第 3 组提供）
    /// 时为 `None`。
    pub status: Option<BackupFolderStatus>,
    pub pause_reason: Option<BackupPauseReason>,
    pub last_backup_at: Option<i64>,
}

/// 一个微信账号的备份状态（每个类别各一项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatAccountBackupStatus {
    pub data_root: PathBuf,
    pub account_name: String,
    pub version: WechatVersion,
    pub categories: Vec<WechatCategoryBackupStatus>,
    /// 各类别最近备份时间中的最大值。
    pub last_backup_at: Option<i64>,
}

/// 一次数据库快照尝试的结果（design D3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WechatSnapshotOutcome {
    /// 上传了全部文件并写入了 `快照完成` 标记。
    Completed,
    /// 内容与上一份完整快照相同，没有新建快照、没有上传。
    Unchanged,
    /// 微信正在运行，等待退出后再试。
    WaitingForWechatExit,
    /// 上传过程中文件变化，本次快照已放弃并移入回收站。
    Aborted,
    /// 全局备份已暂停。
    Paused,
    /// 尚未解锁，无法上传。
    NeedsUnlock,
}

impl WechatSnapshotOutcome {
    /// 稳定字符串，供桥接层与界面使用。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Unchanged => "unchanged",
            Self::WaitingForWechatExit => "waiting_for_wechat_exit",
            Self::Aborted => "aborted",
            Self::Paused => "paused",
            Self::NeedsUnlock => "needs_unlock",
        }
    }
}

/// 一次数据库快照尝试的统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WechatSnapshotReport {
    pub outcome: WechatSnapshotOutcome,
    /// 本次（或最近一次完整快照）的时间戳；没有完整快照时为 `None`。
    pub snapshot_at: Option<i64>,
    /// 清单中的数据库文件数（不含 `快照完成` 标记文件）。
    pub total_files: u64,
    /// 本次上传的文件数。
    pub uploaded_files: u64,
    /// 本次上传的明文字节数。
    pub uploaded_bytes: u64,
}

/// 发现到的微信账号 + 本地备注名（桥接层展示用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatDiscoveredAccount {
    pub account: WechatAccount,
    /// 仅保存在本地状态库的备注名。
    pub note: Option<String>,
}

/// 云端一份可恢复的数据库快照。
///
/// 只返回带 `快照完成` 标记的快照。账号可能在多台设备上备份过，因此同时带
/// 设备名，恢复时用它 + 时间戳目录名精确定位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatDatabaseSnapshot {
    pub device_name: String,
    /// 时间戳目录名（`2026-11-01T153000Z`）。
    pub timestamp: String,
    /// 快照时间（Unix 秒）。
    pub snapshot_at: i64,
    pub total_bytes: u64,
    pub file_count: u64,
    /// 始终为 `true`（接口只返回完整快照，字段留给界面展示）。
    pub complete: bool,
}

/// 恢复时选择的快照（设备名 + 时间戳目录名）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatSnapshotSelection {
    pub device_name: String,
    pub timestamp: String,
}

/// 云端可恢复的文件类别与大小。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatRestoreCategoryInfo {
    pub category: WechatCategory,
    pub file_count: u64,
    pub total_bytes: u64,
    /// 云端是否存在该类别的文件。
    pub available: bool,
}

/// 一次恢复请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WechatRestoreRequest {
    /// 目标微信数据根目录。默认由调用方传入发现到的原账号数据根；新电脑上
    /// 传入用户选择的数据根（账号目录 = `data_root/<account_name>`）。
    pub data_root: PathBuf,
    pub account_name: String,
    pub version: WechatVersion,
    /// 要恢复的文件类别（“聊天记录数据库”不在这里，由 `restore_database` 控制）。
    pub categories: Vec<WechatCategory>,
    pub restore_database: bool,
    /// `restore_database = true` 时必须是带完成标记的快照。
    pub snapshot: Option<WechatSnapshotSelection>,
}

/// 一次恢复的结果统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WechatRestoreReport {
    pub transfer_id: Uuid,
    pub restored_files: u64,
    pub restored_bytes: u64,
    /// 本机已有、按“跳过”策略未覆盖的文件数。
    pub skipped_files: u64,
    /// 是否替换了数据库目录（成功写入快照）。
    pub database_restored: bool,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupSettings {
    /// 备份隐藏文件（以 `.` 开头或带 Windows HIDDEN 属性）；默认关闭。
    pub include_hidden: bool,
    /// 后台备份上传限速（字节/秒）；`None` 表示不限制。手动上传不受影响。
    pub upload_limit_bytes_per_second: Option<u64>,
    /// 全局暂停。
    pub paused: bool,
    /// 每个微信账号保留的完整数据库快照份数（1–10，默认 3）。
    pub wechat_snapshot_retention: u8,
}

impl BackupSettings {
    /// 把保留份数收敛到允许范围；读设置与实际清理都使用这个值。
    #[must_use]
    pub fn normalized_wechat_snapshot_retention(self) -> u8 {
        self.wechat_snapshot_retention
            .clamp(WECHAT_SNAPSHOT_RETENTION_MIN, WECHAT_SNAPSHOT_RETENTION_MAX)
    }
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            include_hidden: false,
            upload_limit_bytes_per_second: None,
            paused: false,
            wechat_snapshot_retention: DEFAULT_WECHAT_SNAPSHOT_RETENTION,
        }
    }
}

/// 备份引擎配置。
#[derive(Clone)]
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
    /// 微信系统位置来源（注册表 / 4.x 配置 / “文档”）；`None` = 当前平台实现。
    /// 测试注入假实现，避免读取真实微信目录。
    pub wechat_locator: Option<WechatLocator>,
    /// 用户手动添加的微信目录（design D1），参与普通备份文件夹的数据根排除。
    pub wechat_manual_directories: Vec<PathBuf>,
    /// 微信进程探测；`None` = 当前平台实现。测试注入假实现，绝不枚举真实进程。
    pub wechat_process_probe: Option<Arc<dyn WechatProcessProbe>>,
    /// 微信退出检测与数据库快照检查周期；默认 10 秒。
    pub wechat_snapshot_interval: Duration,
}

impl std::fmt::Debug for BackupEngineConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BackupEngineConfig")
            .field("device_name", &self.device_name)
            .field("debounce", &self.debounce)
            .field("full_scan_interval", &self.full_scan_interval)
            .field("retry_interval", &self.retry_interval)
            .field("watch_files", &self.watch_files)
            .field("wechat_locator", &self.wechat_locator)
            .field("wechat_manual_directories", &self.wechat_manual_directories)
            .field("wechat_snapshot_interval", &self.wechat_snapshot_interval)
            .finish_non_exhaustive()
    }
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
            wechat_locator: None,
            wechat_manual_directories: Vec::new(),
            wechat_process_probe: None,
            wechat_snapshot_interval: DEFAULT_WECHAT_SNAPSHOT_INTERVAL,
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
