//! 桥接层 DTO（Dart 侧唯一可见的数据模型）。
//!
//! 硬性边界（design D2 / tasks 4.2）：
//! - 只包含名称、大小、时间、进度、状态、错误码、用户选择的本机路径等展示数据；
//! - 不包含密钥、包裹密钥、加密元数据、名称令牌、会话令牌、预签名 URL 或字节数组；
//! - 不直接暴露 `client-core` 的内部类型。转换集中在 `src/convert.rs`。
//!
//! `tests/boundary.rs` 会自动解析本目录下所有 `pub struct` / `pub enum`，新增
//! DTO 时无需手工登记检查清单。

/// 账号状态（与 user-accounts spec 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatusDto {
    Pending,
    Active,
    Suspended,
    Rejected,
    Unknown,
}

/// 登录前服务端下发的 KDF 参数（展示数据；不含盐或密钥材料）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KdfParamsDto {
    pub algorithm: String,
    pub memory_kib: i64,
    pub iterations: i64,
    pub parallelism: i64,
}

/// 当前用户展示数据（`status` / `is_admin` 供第 5 组界面使用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountDto {
    pub user_id: String,
    pub email: String,
    pub email_verified: bool,
    pub root_node_id: String,
    pub status: AccountStatusDto,
    pub status_reason: Option<String>,
    pub is_admin: bool,
    pub writes_allowed: bool,
    /// 内存中是否已有主密钥（“解锁”状态）。
    pub unlocked: bool,
}

/// 节点类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKindDto {
    File,
    Folder,
}

/// 排序字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeSortFieldDto {
    Name,
    ModifiedTime,
    Size,
}

/// 排序方式；文件夹永远排在文件前面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeSortDto {
    pub field: NodeSortFieldDto,
    pub descending: bool,
}

/// 文件/文件夹展示数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeDto {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: NodeKindDto,
    pub name: String,
    /// 文件明文大小；文件夹为 `None`。
    pub size: Option<i64>,
    /// 原始修改时间（Unix 秒）；文件夹为 `None`。
    pub mtime: Option<i64>,
    pub mime: Option<String>,
    /// 乐观并发版本号（重命名/移动需要原样传回）。
    pub row_version: i32,
    pub current_version_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub deleted_at: Option<String>,
    pub trash_batch_id: Option<String>,
}

/// 回收站列表项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItemDto {
    pub node: NodeDto,
    pub deleted_at: String,
    pub trash_batch_id: String,
}

/// 版本展示数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionDto {
    pub id: String,
    pub content_id: String,
    pub size: i64,
    pub mtime: i64,
    pub mime: String,
    pub is_current: bool,
    pub created_at: String,
}

/// 恢复回收站项目的目标位置；`None` 表示原位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreTargetDto {
    pub parent_id: String,
    pub name: String,
}

/// 永久删除统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermanentDeleteOutcomeDto {
    pub deleted_nodes: i64,
    pub deleted_versions: i64,
    pub deleted_contents: i64,
    pub freed_bytes: i64,
    pub queued_objects: i64,
}

/// 已用 / 预留 / 配额。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageDto {
    pub used_bytes: i64,
    pub reserved_bytes: i64,
    pub quota_bytes: i64,
}

/// 下载同名文件处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicyDto {
    KeepBoth,
    Replace,
    Skip,
}

/// 传输方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirectionDto {
    Upload,
    Download,
}

/// 传输状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatusDto {
    Pending,
    Uploading,
    Downloading,
    Paused,
    Retrying,
    Completed,
    Failed,
    Cancelled,
}

/// 传输错误（稳定错误码 + 中文提示；不含路径或密钥）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferFailureDto {
    pub code: String,
    pub message: String,
}

/// 单文件进度事件（不含文件名、路径或预签名 URL）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferProgressDto {
    pub transfer_id: String,
    pub direction: TransferDirectionDto,
    pub status: TransferStatusDto,
    pub completed_bytes: i64,
    pub total_bytes: i64,
    pub completed_chunks: i64,
    pub total_chunks: i64,
    pub error: Option<TransferFailureDto>,
}

/// 文件夹任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderJobStatusDto {
    Scanning,
    Planning,
    Running,
    Done,
    Cancelled,
    Failed,
}

/// 文件夹任务计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderCountsDto {
    pub added: i64,
    pub updated: i64,
    pub unchanged: i64,
    pub failed: i64,
}

/// 被跳过的项（路径是用户自己选择的本地/云端展示数据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedItemDto {
    pub path: String,
    pub reason: String,
}

/// 文件夹进度事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderProgressDto {
    pub job_id: String,
    pub status: FolderJobStatusDto,
    pub completed_files: i64,
    pub total_files: i64,
    pub completed_bytes: i64,
    pub total_bytes: i64,
    pub counts: FolderCountsDto,
    /// 当前正在处理的相对路径（不含本机绝对路径）。
    pub current: Option<String>,
    pub error: Option<TransferFailureDto>,
}

/// 单文件上传结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadOutcomeDto {
    pub node_id: String,
    pub version_id: String,
    pub content_id: String,
    pub size: i64,
}

/// 单文件下载结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcomeDto {
    pub node_id: String,
    pub version_id: String,
    /// 实际写出的本地路径（用户自己选择的目录）。
    pub path: String,
    pub skipped: bool,
}

/// 文件夹上传结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderUploadOutcomeDto {
    pub job_id: String,
    pub status: FolderJobStatusDto,
    pub total_files: i64,
    pub completed_files: i64,
    pub total_bytes: i64,
    pub completed_bytes: i64,
    pub counts: FolderCountsDto,
    pub skipped: Vec<SkippedItemDto>,
    pub failed: Vec<SkippedItemDto>,
}

/// 文件夹下载结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderDownloadOutcomeDto {
    pub job_id: String,
    pub status: FolderJobStatusDto,
    pub root: String,
    pub completed_files: i64,
    pub total_files: i64,
    pub completed_bytes: i64,
    pub total_bytes: i64,
    pub skipped: Vec<SkippedItemDto>,
    pub failed: Vec<SkippedItemDto>,
}

/// 一次传输任务的终态结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferResultDto {
    pub transfer_id: String,
    pub status: TransferStatusDto,
    pub error: Option<TransferFailureDto>,
    pub upload: Option<UploadOutcomeDto>,
    pub download: Option<DownloadOutcomeDto>,
    pub folder_upload: Option<FolderUploadOutcomeDto>,
    pub folder_download: Option<FolderDownloadOutcomeDto>,
}

/// 未完成上传的本地状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingUploadStatusDto {
    Uploading,
    Paused,
    Failed,
}

/// 断点续传记录（只含本机元数据，不含密钥或文件内容）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingUploadDto {
    pub session_id: String,
    pub parent_id: Option<String>,
    pub node_id: String,
    pub content_id: String,
    pub version_id: String,
    pub local_path: String,
    pub file_size: i64,
    pub file_mtime_ns: i64,
    pub chunk_count: i64,
    pub declared_size: i64,
    pub status: PendingUploadStatusDto,
}

/// 备份文件夹运行状态类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupFolderStatusKindDto {
    IdleOk,
    Scanning,
    Uploading,
    Paused,
    Error,
    NeedsUnlock,
}

/// 备份文件夹状态；`done` / `total` 仅在 `Uploading` 有意义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupFolderStatusDto {
    pub kind: BackupFolderStatusKindDto,
    pub done: i64,
    pub total: i64,
    pub unreadable_count: i64,
}

/// 暂停原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupPauseReasonDto {
    User,
    NeedsUnlock,
    Network,
}

/// 单个备份文件夹的状态快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFolderStateDto {
    pub folder_id: String,
    pub status: BackupFolderStatusDto,
    pub pause_reason: Option<BackupPauseReasonDto>,
    pub last_backup_at: Option<i64>,
}

/// 全局汇总状态类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupSummaryStatusKindDto {
    AllBackedUp,
    Uploading,
    Paused,
    NeedsUnlock,
    Error,
}

/// 全局汇总（托盘/状态栏用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupSummaryDto {
    pub kind: BackupSummaryStatusKindDto,
    pub done: i64,
    pub total: i64,
    pub unreadable_count: i64,
    pub folder_count: i64,
    pub last_checked_at: Option<i64>,
}

/// 最近活动类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupActivityKindDto {
    Added,
    NewVersion,
    Unchanged,
    RetryLater,
    Failed,
}

/// 一条最近活动。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupActivityDto {
    pub id: i64,
    pub folder_id: String,
    /// 相对备份根目录的路径（不含本机绝对路径）。
    pub relative_path: String,
    pub kind: BackupActivityKindDto,
    pub occurred_at: i64,
    pub message: Option<String>,
}

/// 备份事件类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupEventKindDto {
    FolderStatus,
    Activity,
    Summary,
}

/// 备份事件；三个可选字段中只有一个非空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEventDto {
    pub kind: BackupEventKindDto,
    pub folder: Option<BackupFolderStateDto>,
    pub activity: Option<BackupActivityDto>,
    pub summary: Option<BackupSummaryDto>,
}

/// 备份文件夹配置（本机路径为用户自己选择的展示数据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFolderDto {
    pub id: String,
    pub local_path: String,
    pub local_name: String,
    pub cloud_target_id: Option<String>,
    pub cloud_folder_id: Option<String>,
    pub cloud_path: Option<String>,
    pub exclude_globs: Vec<String>,
    pub exclude_subfolders: Vec<String>,
    pub enabled: bool,
    pub last_backup_at: Option<i64>,
}

/// 添加备份文件夹的请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFolderDraftDto {
    pub local_path: String,
    pub cloud_target_id: Option<String>,
    pub exclude_globs: Vec<String>,
    pub exclude_subfolders: Vec<String>,
}

/// 修改备份文件夹；`None` 表示不修改该字段。
///
/// `cloud_target_id` 为 `Some(id)` 时指定目标；要恢复默认目标，把
/// `reset_cloud_target` 设为 `true`（Dart 的 `String?` 无法区分“未提供”与
/// “显式恢复默认”，因此拆成两个字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFolderUpdateDto {
    pub local_path: Option<String>,
    pub cloud_target_id: Option<String>,
    pub reset_cloud_target: bool,
    pub exclude_globs: Option<Vec<String>>,
    pub exclude_subfolders: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

/// 添加前的预检结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupPreflightDto {
    pub file_count: i64,
    pub total_bytes: i64,
    pub upload_bytes: i64,
    pub excluded_count: i64,
    pub unreadable_count: i64,
    pub remaining_quota_bytes: Option<i64>,
}

/// 一次备份运行统计。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupRunReportDto {
    pub folder_id: String,
    pub status: BackupFolderStatusDto,
    pub scanned_files: i64,
    pub hashed_files: i64,
    pub uploaded_files: i64,
    pub updated_files: i64,
    pub unchanged_files: i64,
    pub unreadable_files: i64,
    pub retry_later: i64,
    pub failed_files: i64,
    pub skipped_excluded: i64,
}

/// 全局备份设置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupSettingsDto {
    pub include_hidden: bool,
    pub upload_limit_bytes_per_second: Option<i64>,
    pub paused: bool,
}
