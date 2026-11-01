//! `client-core` 类型 → 桥接 DTO 的转换。
//!
//! 本模块不在 `src/api` 下，flutter_rust_bridge 不会把它当作可暴露 API 解析；
//! 所有 DTO 仍然定义在 `src/api/types.rs`。

use std::path::Path;

use client_core::desktop::{
    BackupActivity, BackupActivityKind, BackupEvent, BackupFolder, BackupFolderDraft,
    BackupFolderState, BackupFolderStatus, BackupFolderUpdate, BackupPauseReason, BackupPreflight,
    BackupRunReport, BackupSettings, BackupSummary, BackupSummaryStatus,
};
use client_core::{
    Account, AccountStatus, ConflictPolicy, DownloadOutcome, FolderCounts, FolderDownloadOutcome,
    FolderJobStatus, FolderProgress, FolderUploadOutcome, KdfParamsView, NodeKind, NodeSort,
    NodeSortField, NodeView, PendingUpload, PendingUploadStatus, PermanentDeleteOutcome,
    RestoreTarget, SkippedItem, TransferDirection, TransferFailure, TransferProgress,
    TransferStatus, TrashItem, UploadOutcome, UsageView, VersionView,
};

use crate::api::error::BridgeError;
use crate::api::types::{
    AccountDto, AccountStatusDto, BackupActivityDto, BackupActivityKindDto, BackupEventDto,
    BackupEventKindDto, BackupFolderDraftDto, BackupFolderDto, BackupFolderStateDto,
    BackupFolderStatusDto, BackupFolderStatusKindDto, BackupFolderUpdateDto, BackupPauseReasonDto,
    BackupPreflightDto, BackupRunReportDto, BackupSettingsDto, BackupSummaryDto,
    BackupSummaryStatusKindDto, ConflictPolicyDto, DownloadOutcomeDto, FolderCountsDto,
    FolderDownloadOutcomeDto, FolderJobStatusDto, FolderProgressDto, FolderUploadOutcomeDto,
    KdfParamsDto, NodeDto, NodeKindDto, NodeSortDto, NodeSortFieldDto, PendingUploadDto,
    PendingUploadStatusDto, PermanentDeleteOutcomeDto, RestoreTargetDto, SkippedItemDto,
    TransferDirectionDto, TransferFailureDto, TransferProgressDto, TransferStatusDto, TrashItemDto,
    UploadOutcomeDto, UsageDto, VersionDto,
};

/// 本地路径转展示字符串；路径来自用户选择，可出现在界面返回值里，但不写日志。
pub(crate) fn path_display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `u64` → Dart `int`（i64）。计数/字节数不会接近 i64 上限。
pub(crate) fn i64_from_u64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn i64_from_u32(value: u32) -> i64 {
    i64::from(value)
}

pub(crate) fn u64_from_i64(value: i64, field: &str) -> Result<u64, BridgeError> {
    u64::try_from(value).map_err(|_| BridgeError::invalid_number(field))
}

pub(crate) fn u32_from_i64(value: i64, field: &str) -> Result<u32, BridgeError> {
    u32::try_from(value).map_err(|_| BridgeError::invalid_number(field))
}

pub(crate) fn parse_uuid(value: &str, kind: &str) -> Result<uuid::Uuid, BridgeError> {
    uuid::Uuid::parse_str(value.trim()).map_err(|_| BridgeError::invalid_id(kind))
}

pub(crate) fn parse_optional_uuid(
    value: Option<String>,
    kind: &str,
) -> Result<Option<uuid::Uuid>, BridgeError> {
    value.map(|value| parse_uuid(&value, kind)).transpose()
}

impl From<AccountStatus> for AccountStatusDto {
    fn from(status: AccountStatus) -> Self {
        match status {
            AccountStatus::Pending => Self::Pending,
            AccountStatus::Active => Self::Active,
            AccountStatus::Suspended => Self::Suspended,
            AccountStatus::Rejected => Self::Rejected,
            AccountStatus::Unknown => Self::Unknown,
        }
    }
}

impl From<&Account> for AccountDto {
    fn from(account: &Account) -> Self {
        Self {
            user_id: account.user_id.to_string(),
            email: account.email.clone(),
            email_verified: account.email_verified,
            root_node_id: account.root_node_id.to_string(),
            status: account.status.into(),
            status_reason: account.status_reason.clone(),
            is_admin: account.is_admin,
            writes_allowed: account.writes_allowed(),
            unlocked: false,
        }
    }
}

pub(crate) fn account_dto(account: &Account, unlocked: bool) -> AccountDto {
    let mut dto = AccountDto::from(account);
    dto.unlocked = unlocked;
    dto
}

impl From<KdfParamsView> for KdfParamsDto {
    fn from(params: KdfParamsView) -> Self {
        Self {
            algorithm: params.algorithm.to_owned(),
            memory_kib: i64_from_u32(params.memory_kib),
            iterations: i64_from_u32(params.iterations),
            parallelism: i64_from_u32(params.parallelism),
        }
    }
}

impl From<NodeKind> for NodeKindDto {
    fn from(kind: NodeKind) -> Self {
        match kind {
            NodeKind::File => Self::File,
            NodeKind::Folder => Self::Folder,
        }
    }
}

impl From<NodeSortFieldDto> for NodeSortField {
    fn from(field: NodeSortFieldDto) -> Self {
        match field {
            NodeSortFieldDto::Name => Self::Name,
            NodeSortFieldDto::ModifiedTime => Self::ModifiedTime,
            NodeSortFieldDto::Size => Self::Size,
        }
    }
}

impl From<NodeSortDto> for NodeSort {
    fn from(sort: NodeSortDto) -> Self {
        Self {
            field: sort.field.into(),
            descending: sort.descending,
        }
    }
}

impl From<&NodeView> for NodeDto {
    fn from(node: &NodeView) -> Self {
        Self {
            id: node.id.to_string(),
            parent_id: node.parent_id.map(|id| id.to_string()),
            kind: node.kind.into(),
            name: node.name.clone(),
            size: node.size.map(i64_from_u64),
            mtime: node.mtime,
            mime: node.mime.clone(),
            row_version: node.row_version,
            current_version_id: node.current_version_id.map(|id| id.to_string()),
            created_at: node.created_at.clone(),
            updated_at: node.updated_at.clone(),
            deleted_at: node.deleted_at.clone(),
            trash_batch_id: node.trash_batch_id.map(|id| id.to_string()),
        }
    }
}

impl From<&TrashItem> for TrashItemDto {
    fn from(item: &TrashItem) -> Self {
        Self {
            node: (&item.node).into(),
            deleted_at: item.deleted_at.clone(),
            trash_batch_id: item.trash_batch_id.to_string(),
        }
    }
}

impl From<&VersionView> for VersionDto {
    fn from(version: &VersionView) -> Self {
        Self {
            id: version.id.to_string(),
            content_id: version.content_id.to_string(),
            size: i64_from_u64(version.size),
            mtime: version.mtime,
            mime: version.mime.clone(),
            is_current: version.is_current,
            created_at: version.created_at.clone(),
        }
    }
}

pub(crate) fn restore_target(
    target: Option<RestoreTargetDto>,
) -> Result<Option<RestoreTarget>, BridgeError> {
    target
        .map(|target| {
            Ok(RestoreTarget {
                parent_id: parse_uuid(&target.parent_id, "文件夹")?,
                name: target.name,
            })
        })
        .transpose()
}

impl From<&PermanentDeleteOutcome> for PermanentDeleteOutcomeDto {
    fn from(outcome: &PermanentDeleteOutcome) -> Self {
        Self {
            deleted_nodes: i64_from_u64(outcome.deleted_nodes),
            deleted_versions: i64_from_u64(outcome.deleted_versions),
            deleted_contents: i64_from_u64(outcome.deleted_contents),
            freed_bytes: outcome.freed_bytes,
            queued_objects: i64_from_u64(outcome.queued_objects),
        }
    }
}

impl From<&UsageView> for UsageDto {
    fn from(usage: &UsageView) -> Self {
        Self {
            used_bytes: usage.used_bytes,
            reserved_bytes: usage.reserved_bytes,
            quota_bytes: usage.quota_bytes,
        }
    }
}

impl From<ConflictPolicyDto> for ConflictPolicy {
    fn from(policy: ConflictPolicyDto) -> Self {
        match policy {
            ConflictPolicyDto::KeepBoth => Self::KeepBoth,
            ConflictPolicyDto::Replace => Self::Replace,
            ConflictPolicyDto::Skip => Self::Skip,
        }
    }
}

impl From<TransferDirection> for TransferDirectionDto {
    fn from(direction: TransferDirection) -> Self {
        match direction {
            TransferDirection::Upload => Self::Upload,
            TransferDirection::Download => Self::Download,
        }
    }
}

impl From<TransferStatus> for TransferStatusDto {
    fn from(status: TransferStatus) -> Self {
        match status {
            TransferStatus::Pending => Self::Pending,
            TransferStatus::Uploading => Self::Uploading,
            TransferStatus::Downloading => Self::Downloading,
            TransferStatus::Paused => Self::Paused,
            TransferStatus::Retrying => Self::Retrying,
            TransferStatus::Completed => Self::Completed,
            TransferStatus::Failed => Self::Failed,
            TransferStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<&TransferFailure> for TransferFailureDto {
    fn from(failure: &TransferFailure) -> Self {
        Self {
            code: failure.code.as_str().to_owned(),
            message: failure.message.clone(),
        }
    }
}

impl From<&TransferProgress> for TransferProgressDto {
    fn from(progress: &TransferProgress) -> Self {
        Self {
            transfer_id: progress.transfer_id.to_string(),
            direction: progress.direction.into(),
            status: progress.status.into(),
            completed_bytes: i64_from_u64(progress.completed_bytes),
            total_bytes: i64_from_u64(progress.total_bytes),
            completed_chunks: i64::from(progress.completed_chunks),
            total_chunks: i64::from(progress.total_chunks),
            error: progress.error.as_ref().map(TransferFailureDto::from),
        }
    }
}

impl From<FolderJobStatus> for FolderJobStatusDto {
    fn from(status: FolderJobStatus) -> Self {
        match status {
            FolderJobStatus::Scanning => Self::Scanning,
            FolderJobStatus::Planning => Self::Planning,
            FolderJobStatus::Running => Self::Running,
            FolderJobStatus::Done => Self::Done,
            FolderJobStatus::Cancelled => Self::Cancelled,
            FolderJobStatus::Failed => Self::Failed,
        }
    }
}

impl From<&FolderCounts> for FolderCountsDto {
    fn from(counts: &FolderCounts) -> Self {
        Self {
            added: i64_from_u64(counts.added),
            updated: i64_from_u64(counts.updated),
            unchanged: i64_from_u64(counts.unchanged),
            failed: i64_from_u64(counts.failed),
        }
    }
}

impl From<&SkippedItem> for SkippedItemDto {
    fn from(item: &SkippedItem) -> Self {
        Self {
            path: item.path.clone(),
            reason: item.reason.clone(),
        }
    }
}

impl From<&FolderProgress> for FolderProgressDto {
    fn from(progress: &FolderProgress) -> Self {
        Self {
            job_id: progress.job_id.to_string(),
            status: progress.status.into(),
            completed_files: i64_from_u64(progress.completed_files),
            total_files: i64_from_u64(progress.total_files),
            completed_bytes: i64_from_u64(progress.completed_bytes),
            total_bytes: i64_from_u64(progress.total_bytes),
            counts: (&progress.counts).into(),
            current: progress.current.clone(),
            error: progress.error.as_ref().map(TransferFailureDto::from),
        }
    }
}

impl From<&UploadOutcome> for UploadOutcomeDto {
    fn from(outcome: &UploadOutcome) -> Self {
        Self {
            node_id: outcome.node_id.to_string(),
            version_id: outcome.version_id.to_string(),
            content_id: outcome.content_id.to_string(),
            size: i64_from_u64(outcome.size),
        }
    }
}

impl From<&DownloadOutcome> for DownloadOutcomeDto {
    fn from(outcome: &DownloadOutcome) -> Self {
        Self {
            node_id: outcome.node_id.to_string(),
            version_id: outcome.version_id.to_string(),
            path: path_display(&outcome.path),
            skipped: outcome.skipped,
        }
    }
}

impl From<&FolderUploadOutcome> for FolderUploadOutcomeDto {
    fn from(outcome: &FolderUploadOutcome) -> Self {
        Self {
            job_id: outcome.job_id.to_string(),
            status: outcome.status.into(),
            total_files: i64_from_u64(outcome.total_files),
            completed_files: i64_from_u64(outcome.completed_files),
            total_bytes: i64_from_u64(outcome.total_bytes),
            completed_bytes: i64_from_u64(outcome.completed_bytes),
            counts: (&outcome.counts).into(),
            skipped: outcome.skipped.iter().map(SkippedItemDto::from).collect(),
            failed: outcome.failed.iter().map(SkippedItemDto::from).collect(),
        }
    }
}

impl From<&FolderDownloadOutcome> for FolderDownloadOutcomeDto {
    fn from(outcome: &FolderDownloadOutcome) -> Self {
        Self {
            job_id: outcome.job_id.to_string(),
            status: outcome.status.into(),
            root: path_display(&outcome.root),
            completed_files: i64_from_u64(outcome.completed_files),
            total_files: i64_from_u64(outcome.total_files),
            completed_bytes: i64_from_u64(outcome.completed_bytes),
            total_bytes: i64_from_u64(outcome.total_bytes),
            skipped: outcome.skipped.iter().map(SkippedItemDto::from).collect(),
            failed: outcome.failed.iter().map(SkippedItemDto::from).collect(),
        }
    }
}

impl From<PendingUploadStatus> for PendingUploadStatusDto {
    fn from(status: PendingUploadStatus) -> Self {
        match status {
            PendingUploadStatus::Uploading => Self::Uploading,
            PendingUploadStatus::Paused => Self::Paused,
            PendingUploadStatus::Failed => Self::Failed,
        }
    }
}

impl From<&PendingUpload> for PendingUploadDto {
    fn from(pending: &PendingUpload) -> Self {
        Self {
            session_id: pending.session_id.to_string(),
            parent_id: pending.parent_id.map(|id| id.to_string()),
            node_id: pending.node_id.to_string(),
            content_id: pending.content_id.to_string(),
            version_id: pending.version_id.to_string(),
            local_path: path_display(&pending.local_path),
            file_size: i64_from_u64(pending.file_size),
            file_mtime_ns: pending.file_mtime_ns,
            chunk_count: i64_from_u32(pending.chunk_count),
            declared_size: i64_from_u64(pending.declared_size),
            status: pending.status.into(),
        }
    }
}

impl From<BackupFolderStatus> for BackupFolderStatusDto {
    fn from(status: BackupFolderStatus) -> Self {
        match status {
            BackupFolderStatus::IdleOk => Self::idle_ok(),
            BackupFolderStatus::Scanning => Self::scanning(),
            BackupFolderStatus::Uploading { done, total } => Self::uploading(done, total),
            BackupFolderStatus::Paused => Self::paused(),
            BackupFolderStatus::Error { unreadable_count } => Self::error(unreadable_count),
            BackupFolderStatus::NeedsUnlock => Self::needs_unlock(),
            // 第 4 组再为“等微信退出后备份”增加桥接枚举与界面文案；
            // 这里先映射为暂停，保证非桌面端桥接层可编译。
            BackupFolderStatus::WaitingForWechatExit => Self::paused(),
        }
    }
}

impl BackupFolderStatusDto {
    fn idle_ok() -> Self {
        Self {
            kind: BackupFolderStatusKindDto::IdleOk,
            done: 0,
            total: 0,
            unreadable_count: 0,
        }
    }

    fn scanning() -> Self {
        Self {
            kind: BackupFolderStatusKindDto::Scanning,
            ..Self::idle_ok()
        }
    }

    fn uploading(done: u64, total: u64) -> Self {
        Self {
            kind: BackupFolderStatusKindDto::Uploading,
            done: i64_from_u64(done),
            total: i64_from_u64(total),
            unreadable_count: 0,
        }
    }

    fn paused() -> Self {
        Self {
            kind: BackupFolderStatusKindDto::Paused,
            ..Self::idle_ok()
        }
    }

    fn error(unreadable_count: u64) -> Self {
        Self {
            kind: BackupFolderStatusKindDto::Error,
            done: 0,
            total: 0,
            unreadable_count: i64_from_u64(unreadable_count),
        }
    }

    fn needs_unlock() -> Self {
        Self {
            kind: BackupFolderStatusKindDto::NeedsUnlock,
            ..Self::idle_ok()
        }
    }
}

impl From<BackupPauseReason> for BackupPauseReasonDto {
    fn from(reason: BackupPauseReason) -> Self {
        match reason {
            BackupPauseReason::User => Self::User,
            BackupPauseReason::NeedsUnlock => Self::NeedsUnlock,
            BackupPauseReason::Network => Self::Network,
        }
    }
}

impl From<&BackupFolderState> for BackupFolderStateDto {
    fn from(state: &BackupFolderState) -> Self {
        Self {
            folder_id: state.folder_id.to_string(),
            status: state.status.into(),
            pause_reason: state.pause_reason.map(BackupPauseReasonDto::from),
            last_backup_at: state.last_backup_at,
        }
    }
}

impl From<BackupSummaryStatus> for BackupSummaryStatusKindDto {
    fn from(status: BackupSummaryStatus) -> Self {
        match status {
            BackupSummaryStatus::AllBackedUp => Self::AllBackedUp,
            BackupSummaryStatus::Uploading { .. } => Self::Uploading,
            BackupSummaryStatus::Paused => Self::Paused,
            BackupSummaryStatus::NeedsUnlock => Self::NeedsUnlock,
            BackupSummaryStatus::Error { .. } => Self::Error,
        }
    }
}

impl From<&BackupSummary> for BackupSummaryDto {
    fn from(summary: &BackupSummary) -> Self {
        let (done, total, unreadable_count) = match summary.status {
            BackupSummaryStatus::Uploading { done, total } => (done, total, 0),
            BackupSummaryStatus::Error { unreadable_count } => (0, 0, unreadable_count),
            _ => (0, 0, 0),
        };
        Self {
            kind: summary.status.into(),
            done: i64_from_u64(done),
            total: i64_from_u64(total),
            unreadable_count: i64_from_u64(unreadable_count),
            folder_count: i64_from_u64(summary.folder_count),
            last_checked_at: summary.last_checked_at,
        }
    }
}

impl From<BackupActivityKind> for BackupActivityKindDto {
    fn from(kind: BackupActivityKind) -> Self {
        match kind {
            BackupActivityKind::Added => Self::Added,
            BackupActivityKind::NewVersion => Self::NewVersion,
            BackupActivityKind::Unchanged => Self::Unchanged,
            BackupActivityKind::RetryLater => Self::RetryLater,
            BackupActivityKind::Failed => Self::Failed,
        }
    }
}

impl From<&BackupActivity> for BackupActivityDto {
    fn from(activity: &BackupActivity) -> Self {
        Self {
            id: activity.id,
            folder_id: activity.folder_id.to_string(),
            relative_path: activity.relative_path.clone(),
            kind: activity.kind.into(),
            occurred_at: activity.occurred_at,
            message: activity.message.clone(),
        }
    }
}

impl From<&BackupEvent> for BackupEventDto {
    fn from(event: &BackupEvent) -> Self {
        match event {
            BackupEvent::Folder(state) => Self {
                kind: BackupEventKindDto::FolderStatus,
                folder: Some(state.into()),
                activity: None,
                summary: None,
            },
            BackupEvent::Activity(activity) => Self {
                kind: BackupEventKindDto::Activity,
                folder: None,
                activity: Some(activity.into()),
                summary: None,
            },
            BackupEvent::Summary(summary) => Self {
                kind: BackupEventKindDto::Summary,
                folder: None,
                activity: None,
                summary: Some(summary.into()),
            },
        }
    }
}

impl From<&BackupFolder> for BackupFolderDto {
    fn from(folder: &BackupFolder) -> Self {
        Self {
            id: folder.id.to_string(),
            local_path: path_display(&folder.local_path),
            local_name: folder.local_name.clone(),
            cloud_target_id: folder.cloud_target_id.map(|id| id.to_string()),
            cloud_folder_id: folder.cloud_folder_id.map(|id| id.to_string()),
            cloud_path: folder.cloud_path.clone(),
            exclude_globs: folder.exclude_globs.clone(),
            exclude_subfolders: folder.exclude_subfolders.clone(),
            enabled: folder.enabled,
            last_backup_at: folder.last_backup_at,
        }
    }
}

pub(crate) fn backup_folder_draft(
    draft: BackupFolderDraftDto,
) -> Result<BackupFolderDraft, BridgeError> {
    Ok(BackupFolderDraft {
        local_path: draft.local_path.into(),
        cloud_target_id: parse_optional_uuid(draft.cloud_target_id, "云端目标文件夹")?,
        exclude_globs: draft.exclude_globs,
        exclude_subfolders: draft.exclude_subfolders,
    })
}

pub(crate) fn backup_folder_update(
    update: BackupFolderUpdateDto,
) -> Result<BackupFolderUpdate, BridgeError> {
    let cloud_target_id = if update.reset_cloud_target {
        Some(None)
    } else {
        parse_optional_uuid(update.cloud_target_id, "云端目标文件夹")?.map(Some)
    };
    Ok(BackupFolderUpdate {
        local_path: update.local_path.map(Into::into),
        cloud_target_id,
        exclude_globs: update.exclude_globs,
        exclude_subfolders: update.exclude_subfolders,
        enabled: update.enabled,
    })
}

pub(crate) fn backup_preflight(preflight: &BackupPreflight) -> BackupPreflightDto {
    BackupPreflightDto {
        file_count: i64_from_u64(preflight.file_count),
        total_bytes: i64_from_u64(preflight.total_bytes),
        upload_bytes: i64_from_u64(preflight.upload_bytes),
        excluded_count: i64_from_u64(preflight.excluded_count),
        unreadable_count: i64_from_u64(preflight.unreadable_count),
        remaining_quota_bytes: preflight.remaining_quota_bytes.map(i64_from_u64),
    }
}

pub(crate) fn backup_run_report(report: &BackupRunReport) -> BackupRunReportDto {
    BackupRunReportDto {
        folder_id: report.folder_id.to_string(),
        status: report.status.into(),
        scanned_files: i64_from_u64(report.scanned_files),
        hashed_files: i64_from_u64(report.hashed_files),
        uploaded_files: i64_from_u64(report.uploaded_files),
        updated_files: i64_from_u64(report.updated_files),
        unchanged_files: i64_from_u64(report.unchanged_files),
        unreadable_files: i64_from_u64(report.unreadable_files),
        retry_later: i64_from_u64(report.retry_later),
        failed_files: i64_from_u64(report.failed_files),
        skipped_excluded: i64_from_u64(report.skipped_excluded),
    }
}

impl From<&BackupSettings> for BackupSettingsDto {
    fn from(settings: &BackupSettings) -> Self {
        Self {
            include_hidden: settings.include_hidden,
            upload_limit_bytes_per_second: settings.upload_limit_bytes_per_second.map(i64_from_u64),
            paused: settings.paused,
        }
    }
}

pub(crate) fn backup_settings(settings: BackupSettingsDto) -> Result<BackupSettings, BridgeError> {
    Ok(BackupSettings {
        include_hidden: settings.include_hidden,
        upload_limit_bytes_per_second: settings
            .upload_limit_bytes_per_second
            .map(|value| u64_from_i64(value, "上传限速"))
            .transpose()?,
        paused: settings.paused,
        // 保留份数的桥接字段与界面在第 4 组加入；在那之前保持默认值，
        // 核心引擎的默认值仍是 3，设置变化时由核心归一化到 1–10。
        wechat_snapshot_retention: client_core::desktop::DEFAULT_WECHAT_SNAPSHOT_RETENTION,
    })
}
