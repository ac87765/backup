//! 客户端展示数据与传输事件。
//!
//! 本模块的类型会被桥接层（flutter_rust_bridge）原样暴露给界面，因此**禁止**
//! 出现密钥、包裹密钥、加密元数据或名称令牌。节点密钥只存在于 [`crate::Client`]
//! 的私有缓存中。

use std::cmp::Ordering;
use std::fmt;
use std::path::PathBuf;

use uuid::Uuid;

/// 节点类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Folder,
}

impl NodeKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Folder => "folder",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "file" => Some(Self::File),
            "folder" => Some(Self::Folder),
            _ => None,
        }
    }
}

/// 文件/文件夹的展示数据（名称已解密，大小/时间为版本元数据中的明文值）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub kind: NodeKind,
    /// 解密后的名称；只驻留内存，不写入日志。
    pub name: String,
    /// 文件明文大小；文件夹为 `None`。
    pub size: Option<u64>,
    /// 原始修改时间（Unix 秒）；文件夹为 `None`。
    pub mtime: Option<i64>,
    /// MIME 类型；文件夹为 `None`。
    pub mime: Option<String>,
    /// 乐观并发版本号（重命名/移动需要原样传回）。
    pub row_version: i32,
    pub current_version_id: Option<Uuid>,
    pub created_at: String,
    pub updated_at: String,
    /// 仅在回收站列表中非空。
    pub deleted_at: Option<String>,
    pub trash_batch_id: Option<Uuid>,
}

/// 排序字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeSortField {
    Name,
    ModifiedTime,
    Size,
}

/// 排序方式；文件夹永远排在文件前面（与 Web 文件列表一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeSort {
    pub field: NodeSortField,
    pub descending: bool,
}

impl NodeSort {
    pub const NAME_ASC: Self = Self {
        field: NodeSortField::Name,
        descending: false,
    };
}

impl Default for NodeSort {
    fn default() -> Self {
        Self::NAME_ASC
    }
}

/// 版本展示数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionView {
    pub id: Uuid,
    pub content_id: Uuid,
    /// 解密后的明文大小。
    pub size: u64,
    pub mtime: i64,
    pub mime: String,
    pub is_current: bool,
    pub created_at: String,
}

/// 回收站列表项：节点展示数据 + 删除信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    pub node: NodeView,
    pub deleted_at: String,
    pub trash_batch_id: Uuid,
}

/// 永久删除统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PermanentDeleteOutcome {
    pub deleted_nodes: u64,
    pub deleted_versions: u64,
    pub deleted_contents: u64,
    pub freed_bytes: i64,
    pub queued_objects: u64,
}

/// 已用 / 预留 / 配额。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageView {
    pub used_bytes: i64,
    pub reserved_bytes: i64,
    pub quota_bytes: i64,
}

/// 恢复回收站项目时的目标位置。`None` 表示恢复到原位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreTarget {
    pub parent_id: Uuid,
    /// 原名称（用于按新父节点重新计算名称令牌）。
    pub name: String,
}

/// 下载同名文件处理策略（design/desktop/restore.html）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// 默认：下载的文件改名为“名称 (1).ext”，保留本地原文件。
    #[default]
    KeepBoth,
    /// 用云端版本覆盖本地文件。
    Replace,
    /// 保留本地文件，不下载该文件。
    Skip,
}

/// 传输方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Upload,
    Download,
}

/// 传输状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Uploading,
    Downloading,
    Paused,
    Retrying,
    Completed,
    Failed,
    Cancelled,
}

impl TransferStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Uploading => "uploading",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Retrying => "retrying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// 稳定的传输错误码（供界面显示与重试判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferErrorCode {
    Network,
    RateLimited,
    Server,
    UploadSessionClosed,
    ChunkNotFound,
    ChunkSizeMismatch,
    UploadIncomplete,
    NameConflict,
    TargetUnavailable,
    QuotaExceeded,
    InvalidName,
    LocalIo,
    FileChanged,
    Integrity,
    AccountNotWritable,
    Locked,
    Cancelled,
    Other,
}

impl TransferErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network_error",
            Self::RateLimited => "rate_limited",
            Self::Server => "server_error",
            Self::UploadSessionClosed => "upload_session_closed",
            Self::ChunkNotFound => "chunk_not_found",
            Self::ChunkSizeMismatch => "chunk_size_mismatch",
            Self::UploadIncomplete => "upload_incomplete",
            Self::NameConflict => "name_conflict",
            Self::TargetUnavailable => "target_unavailable",
            Self::QuotaExceeded => "quota_exceeded",
            Self::InvalidName => "invalid_name",
            Self::LocalIo => "local_io_error",
            Self::FileChanged => "file_changed",
            Self::Integrity => "integrity_error",
            Self::AccountNotWritable => "account_not_writable",
            Self::Locked => "locked",
            Self::Cancelled => "cancelled",
            Self::Other => "transfer_error",
        }
    }
}

/// 传输事件中的错误信息；不含路径、密钥、令牌或预签名 URL。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferFailure {
    pub code: TransferErrorCode,
    pub message: String,
}

/// 进度事件（通过 `tokio::sync::broadcast` 推送）。
///
/// 字段只有传输 ID、计数/字节、状态与稳定错误码；不包含文件名、路径、
/// 密钥或预签名 URL，因此可以安全地跨桥接层传给界面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferProgress {
    pub transfer_id: Uuid,
    pub direction: TransferDirection,
    pub status: TransferStatus,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub completed_chunks: u32,
    pub total_chunks: u32,
    pub error: Option<TransferFailure>,
}

/// 单文件上传结果（只含展示数据与 ID）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadOutcome {
    pub node_id: Uuid,
    pub version_id: Uuid,
    pub content_id: Uuid,
    pub size: u64,
}

/// 单文件下载结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    pub node_id: Uuid,
    pub version_id: Uuid,
    /// 实际写出的本地路径（`skipped` 时为本地已有文件路径）。
    pub path: PathBuf,
    /// 按“跳过”策略没有下载（本地已有同名文件）。
    pub skipped: bool,
}

/// 断点续传记录（只含本机元数据，不含密钥或文件内容）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingUpload {
    pub session_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub node_id: Uuid,
    pub content_id: Uuid,
    pub version_id: Uuid,
    pub local_path: PathBuf,
    pub file_size: u64,
    pub file_mtime_ns: i64,
    pub chunk_count: u32,
    pub declared_size: u64,
    pub status: PendingUploadStatus,
}

/// 未完成上传的本地状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingUploadStatus {
    Uploading,
    Paused,
    Failed,
}

impl PendingUploadStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uploading => "uploading",
            Self::Paused => "paused",
            Self::Failed => "failed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "uploading" => Some(Self::Uploading),
            "paused" => Some(Self::Paused),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// 文件夹上传/下载任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderJobStatus {
    Scanning,
    Planning,
    Running,
    Done,
    Cancelled,
    Failed,
}

impl FolderJobStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scanning => "scanning",
            Self::Planning => "planning",
            Self::Running => "running",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// 文件夹任务摘要。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderCounts {
    pub added: u64,
    pub updated: u64,
    pub unchanged: u64,
    pub failed: u64,
}

/// 被跳过的本地项（路径为界面展示数据，不写日志）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedItem {
    pub path: String,
    pub reason: String,
}

/// 文件夹上传结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderUploadOutcome {
    pub job_id: Uuid,
    pub status: FolderJobStatus,
    pub total_files: u64,
    pub completed_files: u64,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub counts: FolderCounts,
    pub skipped: Vec<SkippedItem>,
    pub failed: Vec<SkippedItem>,
}

/// 文件夹进度事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderProgress {
    pub job_id: Uuid,
    pub status: FolderJobStatus,
    pub completed_files: u64,
    pub total_files: u64,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub counts: FolderCounts,
    /// 当前正在处理的相对路径（展示数据，不含本机绝对路径）。
    pub current: Option<String>,
    pub error: Option<TransferFailure>,
}

/// 文件夹下载结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderDownloadOutcome {
    pub job_id: Uuid,
    pub status: FolderJobStatus,
    pub root: PathBuf,
    pub completed_files: u64,
    pub total_files: u64,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub skipped: Vec<SkippedItem>,
    pub failed: Vec<SkippedItem>,
}

impl fmt::Display for TransferFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// 文件夹名称规范化：`NFC(lower(NFC(name)))`，与名称令牌规则一致。
pub(crate) fn normalize_folder_name(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfc()
        .collect::<String>()
        .to_lowercase()
        .nfc()
        .collect()
}

/// 按 [`NodeSort`] 排序；文件夹永远在前。
pub(crate) fn sort_nodes(nodes: &mut [NodeView], sort: NodeSort) {
    nodes.sort_by(|left, right| {
        let kind_order = match (left.kind, right.kind) {
            (NodeKind::Folder, NodeKind::Folder) | (NodeKind::File, NodeKind::File) => {
                Ordering::Equal
            }
            (NodeKind::Folder, NodeKind::File) => Ordering::Less,
            (NodeKind::File, NodeKind::Folder) => Ordering::Greater,
        };
        if kind_order != Ordering::Equal {
            return kind_order;
        }

        let primary = match sort.field {
            NodeSortField::Name => {
                let left_lower = left.name.to_lowercase();
                let right_lower = right.name.to_lowercase();
                left_lower
                    .cmp(&right_lower)
                    .then_with(|| left.name.cmp(&right.name))
            }
            NodeSortField::ModifiedTime => left
                .mtime
                .unwrap_or_default()
                .cmp(&right.mtime.unwrap_or_default()),
            NodeSortField::Size => left
                .size
                .unwrap_or_default()
                .cmp(&right.size.unwrap_or_default()),
        };
        if sort.descending {
            primary.reverse()
        } else {
            primary
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn node(name: &str, kind: NodeKind, size: u64, mtime: i64) -> NodeView {
        NodeView {
            id: Uuid::new_v4(),
            parent_id: None,
            kind,
            name: name.to_owned(),
            size: Some(size),
            mtime: Some(mtime),
            mime: None,
            row_version: 1,
            current_version_id: None,
            created_at: String::new(),
            updated_at: String::new(),
            deleted_at: None,
            trash_batch_id: None,
        }
    }

    #[test]
    fn folders_always_sort_before_files() {
        let mut nodes = vec![
            node("a.txt", NodeKind::File, 10, 1),
            node("z-folder", NodeKind::Folder, 0, 0),
            node("b.txt", NodeKind::File, 20, 2),
        ];
        sort_nodes(&mut nodes, NodeSort::NAME_ASC);
        assert_eq!(nodes[0].kind, NodeKind::Folder);
        assert_eq!(nodes[1].name, "a.txt");
    }

    #[test]
    fn sort_by_name_is_case_insensitive() {
        let mut nodes = vec![
            node("beta.txt", NodeKind::File, 1, 1),
            node("Alpha.txt", NodeKind::File, 1, 1),
        ];
        sort_nodes(&mut nodes, NodeSort::NAME_ASC);
        assert_eq!(nodes[0].name, "Alpha.txt");
    }

    #[test]
    fn sort_by_size_and_mtime_can_be_descending() {
        let mut nodes = vec![
            node("small", NodeKind::File, 1, 50),
            node("large", NodeKind::File, 100, 10),
        ];
        sort_nodes(
            &mut nodes,
            NodeSort {
                field: NodeSortField::Size,
                descending: true,
            },
        );
        assert_eq!(nodes[0].name, "large");

        sort_nodes(
            &mut nodes,
            NodeSort {
                field: NodeSortField::ModifiedTime,
                descending: true,
            },
        );
        assert_eq!(nodes[0].name, "small");
    }

    #[test]
    fn folder_name_normalization_matches_name_token_rule() {
        assert_eq!(
            normalize_folder_name("Cafe\u{301}"),
            normalize_folder_name("CAF\u{c9}")
        );
    }
}
