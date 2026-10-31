//! 文件夹上传合并与下载到本地（folder-transfer spec）。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use uuid::Uuid;

use super::{FolderSink, TransferControl, failure, transfer_error};
use crate::Client;
use crate::error::ClientError;
use crate::files::DecryptedNode;
use crate::io_path::{path_for_io, sanitize_local_name};
use crate::model::{
    ConflictPolicy, FolderCounts, FolderDownloadOutcome, FolderJobStatus, FolderProgress,
    FolderUploadOutcome, NodeKind, SkippedItem, TransferErrorCode,
};
use crate::transfer::download;
use crate::transfer::scan::{FolderLocalTree, scan_local_tree};

/// 单个文件夹任务的最大文件数（folder-transfer spec）。
pub const MAX_FILES_PER_TASK: u64 = 50_000;

const FOLDER_CONCURRENCY: usize = 3;

/// 文件夹上传过程中的计数与跳过/失败项。
struct FolderRuntime {
    counts: FolderCounts,
    skipped: Vec<SkippedItem>,
    failed: Vec<SkippedItem>,
    unchanged_bytes: u64,
}

impl FolderRuntime {
    fn new(mut skipped: Vec<SkippedItem>, counts: FolderCounts) -> Self {
        skipped.shrink_to_fit();
        Self {
            counts,
            skipped,
            failed: Vec::new(),
            unchanged_bytes: 0,
        }
    }

    /// 本地是文件夹、云端同一位置是文件：整个子树作为失败项，不覆盖云端。
    fn mark_tree_failed(&mut self, tree: &FolderLocalTree, reason: &str) {
        for file in &tree.all_files() {
            self.counts.failed += 1;
            self.failed.push(SkippedItem {
                path: file.relative.clone(),
                reason: reason.to_owned(),
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanMode {
    New,
    Version,
}

#[derive(Debug, Clone)]
struct PlanItem {
    path: PathBuf,
    relative: String,
    parent_id: Uuid,
    node_id: Option<Uuid>,
    mode: PlanMode,
    size: u64,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_upload(
    client: &Client,
    job_id: Uuid,
    parent_id: Uuid,
    local_dir: &Path,
    started: &AtomicBool,
    control: &TransferControl,
    sink: &FolderSink,
) -> Result<FolderUploadOutcome, ClientError> {
    let result =
        run_upload_inner(client, job_id, parent_id, local_dir, started, control, sink).await;
    if let Err(error) = &result {
        let cancelled = matches!(error, ClientError::Cancelled) || control.is_cancelled();
        emit(
            sink,
            job_id,
            if cancelled {
                FolderJobStatus::Cancelled
            } else {
                FolderJobStatus::Failed
            },
            &FolderCounts::default(),
            0,
            0,
            0,
            0,
            None,
            Some(failure(error)),
        );
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_upload_inner(
    client: &Client,
    job_id: Uuid,
    parent_id: Uuid,
    local_dir: &Path,
    started: &AtomicBool,
    control: &TransferControl,
    sink: &FolderSink,
) -> Result<FolderUploadOutcome, ClientError> {
    client.ensure_writable()?;
    client.master_key()?;
    emit(
        sink,
        job_id,
        FolderJobStatus::Scanning,
        &FolderCounts::default(),
        0,
        0,
        0,
        0,
        None,
        None,
    );
    let root = {
        let local_dir = local_dir.to_path_buf();
        tokio::task::spawn_blocking(move || scan_local_tree(&local_dir, MAX_FILES_PER_TASK))
            .await
            .map_err(|_| ClientError::Internal("扫描文件夹任务失败"))??
    };
    if root.truncated {
        return Err(transfer_error(
            TransferErrorCode::Other,
            format!("这个文件夹超过 {MAX_FILES_PER_TASK} 个文件，请拆分后重试"),
        ));
    }

    let total_files = root.total_files;
    let total_bytes = root.total_bytes;
    let counts = FolderCounts::default();
    emit(
        sink,
        job_id,
        FolderJobStatus::Planning,
        &counts,
        0,
        total_files,
        total_bytes,
        0,
        None,
        None,
    );

    if !started.load(Ordering::SeqCst) {
        preflight_quota(client, &root).await?;
        started.store(true, Ordering::SeqCst);
    }
    control.checkpoint().await?;

    let mut runtime = FolderRuntime::new(root.skipped.clone(), counts);
    let mut root_plan = Vec::new();
    let root_folder = merge_root(
        client,
        &root,
        parent_id,
        &mut root_plan,
        &mut runtime,
        control,
    )
    .await?;
    let _ = root_folder;

    if control.is_cancelled() {
        return Ok(cancelled_upload_outcome(
            job_id,
            &root,
            &runtime,
            total_files,
            total_bytes,
        ));
    }

    let mut completed_files = runtime.counts.unchanged + runtime.failed.len() as u64;
    let mut completed_bytes = runtime.unchanged_bytes;
    let mut queue: VecDeque<PlanItem> = root_plan.into_iter().collect();
    let mut join_set = tokio::task::JoinSet::new();

    for _ in 0..FOLDER_CONCURRENCY {
        let Some(item) = queue.pop_front() else {
            break;
        };
        spawn_upload(&mut join_set, client, control, item);
    }

    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((item, result)) => {
                if control.is_cancelled() || matches!(result, Err(ClientError::Cancelled)) {
                    continue;
                }
                completed_files = completed_files.saturating_add(1);
                completed_bytes = completed_bytes.saturating_add(item.size);
                match &result {
                    Ok(_) => match item.mode {
                        PlanMode::New => runtime.counts.added += 1,
                        PlanMode::Version => runtime.counts.updated += 1,
                    },
                    Err(error) => {
                        runtime.counts.failed += 1;
                        runtime.failed.push(SkippedItem {
                            path: item.relative.clone(),
                            reason: error.to_string(),
                        });
                    }
                }
                emit(
                    sink,
                    job_id,
                    FolderJobStatus::Running,
                    &runtime.counts,
                    completed_files,
                    total_files,
                    completed_bytes,
                    total_bytes,
                    None,
                    None,
                );
            }
            Err(_) => {
                runtime.counts.failed += 1;
            }
        }
        if control.is_cancelled() {
            continue;
        }
        if let Some(item) = queue.pop_front() {
            spawn_upload(&mut join_set, client, control, item);
        }
    }

    // 取消后仍在队列里的文件不派发。
    let cancelled = control.is_cancelled();
    let status = if cancelled {
        FolderJobStatus::Cancelled
    } else if runtime.counts.failed > 0 {
        FolderJobStatus::Failed
    } else {
        FolderJobStatus::Done
    };
    let outcome = FolderUploadOutcome {
        job_id,
        status,
        total_files,
        completed_files,
        total_bytes,
        completed_bytes,
        counts: runtime.counts,
        skipped: runtime.skipped,
        failed: runtime.failed,
    };
    emit(
        sink,
        job_id,
        status,
        &outcome.counts,
        completed_files,
        total_files,
        completed_bytes,
        total_bytes,
        None,
        None,
    );
    Ok(outcome)
}

fn cancelled_upload_outcome(
    job_id: Uuid,
    root: &FolderLocalTree,
    runtime: &FolderRuntime,
    total_files: u64,
    total_bytes: u64,
) -> FolderUploadOutcome {
    let _ = root;
    FolderUploadOutcome {
        job_id,
        status: FolderJobStatus::Cancelled,
        total_files,
        completed_files: runtime.counts.unchanged + runtime.failed.len() as u64,
        total_bytes,
        completed_bytes: runtime.unchanged_bytes,
        counts: runtime.counts,
        skipped: runtime.skipped.clone(),
        failed: runtime.failed.clone(),
    }
}

fn spawn_upload(
    join_set: &mut tokio::task::JoinSet<(PlanItem, Result<(), ClientError>)>,
    client: &Client,
    control: &TransferControl,
    item: PlanItem,
) {
    let client = client.clone();
    let control = control.clone();
    join_set.spawn(async move {
        if control.is_cancelled() {
            return (item, Err(ClientError::Cancelled));
        }
        let task = match item.mode {
            PlanMode::New => client.upload_file(item.parent_id, item.path.clone()),
            PlanMode::Version => match item.node_id {
                Some(node_id) => client.upload_new_version(node_id, item.path.clone()),
                None => Err(transfer_error(TransferErrorCode::Other, "缺少目标文件 ID")),
            },
        };
        let result = match task {
            Ok(task) => task.run().await.map(|_| ()),
            Err(error) => Err(error),
        };
        (item, result)
    });
}

async fn preflight_quota(client: &Client, root: &FolderLocalTree) -> Result<(), ClientError> {
    let usage = client.get_usage().await?;
    let remaining = usage
        .quota_bytes
        .saturating_sub(usage.used_bytes)
        .saturating_sub(usage.reserved_bytes)
        .max(0) as u64;
    let needed = root
        .all_files()
        .iter()
        .map(|file| projected_ciphertext_size(file.size))
        .sum::<u64>();
    if needed > remaining {
        return Err(transfer_error(
            TransferErrorCode::QuotaExceeded,
            format!(
                "空间不足：需要 {}，剩余 {}",
                format_bytes(needed),
                format_bytes(remaining)
            ),
        ));
    }
    Ok(())
}

fn projected_ciphertext_size(size: u64) -> u64 {
    let count = crypto_core::chunk_count_for_size(size).unwrap_or(1);
    size.saturating_add(u64::from(count) * crypto_core::CHUNK_CIPHERTEXT_OVERHEAD as u64)
}

async fn merge_root(
    client: &Client,
    root: &FolderLocalTree,
    parent_id: Uuid,
    plan: &mut Vec<PlanItem>,
    runtime: &mut FolderRuntime,
    control: &TransferControl,
) -> Result<Uuid, ClientError> {
    let existing = client.list_folder_decrypted(parent_id).await?;
    let by_name: HashMap<String, DecryptedNode> = existing
        .into_iter()
        .map(|node| (crate::model::normalize_folder_name(node.name()), node))
        .collect();
    let key = crate::model::normalize_folder_name(&root.name);
    let folder_id = match by_name.get(&key) {
        Some(node) if node.kind() == NodeKind::Folder => node.view.id,
        Some(node) => {
            let reason = format!("云端同一位置已有同名文件“{}”", node.name());
            runtime.mark_tree_failed(root, &reason);
            return Ok(parent_id);
        }
        None => create_folder_with_race(client, parent_id, &root.name).await?,
    };
    merge_folder(client, root, folder_id, "", plan, runtime, control).await?;
    Ok(folder_id)
}

async fn merge_folder(
    client: &Client,
    local: &FolderLocalTree,
    cloud_id: Uuid,
    prefix: &str,
    plan: &mut Vec<PlanItem>,
    runtime: &mut FolderRuntime,
    control: &TransferControl,
) -> Result<(), ClientError> {
    control.checkpoint().await?;
    let cloud_items = client.list_folder_decrypted(cloud_id).await?;
    let by_name: HashMap<String, DecryptedNode> = cloud_items
        .into_iter()
        .map(|node| (crate::model::normalize_folder_name(node.name()), node))
        .collect();

    for folder in &local.folders {
        let relative = if prefix.is_empty() {
            folder.name.clone()
        } else {
            format!("{prefix}/{}", folder.name)
        };
        let key = crate::model::normalize_folder_name(&folder.name);
        let folder_id = match by_name.get(&key) {
            Some(node) if node.kind() == NodeKind::Folder => node.view.id,
            Some(node) => {
                let reason = format!("云端同一位置已有同名文件“{}”", node.name());
                runtime.mark_tree_failed(folder, &reason);
                continue;
            }
            None => create_folder_with_race(client, cloud_id, &folder.name).await?,
        };
        Box::pin(merge_folder(
            client, folder, folder_id, &relative, plan, runtime, control,
        ))
        .await?;
    }

    for file in &local.files {
        control.checkpoint().await?;
        let key = crate::model::normalize_folder_name(&file.name);
        let existing = by_name.get(&key);
        match existing {
            None => plan.push(PlanItem {
                path: file.path.clone(),
                relative: file.relative.clone(),
                parent_id: cloud_id,
                node_id: None,
                mode: PlanMode::New,
                size: file.size,
            }),
            Some(node) if node.kind() == NodeKind::Folder => {
                runtime.counts.failed += 1;
                runtime.failed.push(SkippedItem {
                    path: file.relative.clone(),
                    reason: format!("云端同一位置已有同名文件夹“{}”", node.name()),
                });
            }
            Some(node) => {
                node.view
                    .current_version_id
                    .ok_or(ClientError::InvalidResponse("文件缺少当前版本"))?;
                let changed = if node.view.size.unwrap_or_default() != file.size {
                    true
                } else if let Some(remote_hash) = &node.content_sha256 {
                    match client.local_content_sha256(file.path.clone()).await {
                        Ok(local_hash) => !Client::hashes_equal(&local_hash, remote_hash),
                        Err(error) => {
                            runtime.counts.failed += 1;
                            runtime.failed.push(SkippedItem {
                                path: file.relative.clone(),
                                reason: error.to_string(),
                            });
                            continue;
                        }
                    }
                } else {
                    node.view.mtime.unwrap_or_default() != (file.mtime_ns / 1_000_000_000)
                };
                if changed {
                    plan.push(PlanItem {
                        path: file.path.clone(),
                        relative: file.relative.clone(),
                        parent_id: cloud_id,
                        node_id: Some(node.view.id),
                        mode: PlanMode::Version,
                        size: file.size,
                    });
                } else {
                    runtime.counts.unchanged += 1;
                    runtime.unchanged_bytes = runtime.unchanged_bytes.saturating_add(file.size);
                }
            }
        }
    }
    Ok(())
}

async fn create_folder_with_race(
    client: &Client,
    parent_id: Uuid,
    name: &str,
) -> Result<Uuid, ClientError> {
    match client.create_folder(parent_id, name).await {
        Ok(folder) => Ok(folder.id),
        Err(error) if error.api_code() == Some(&crate::error::ApiErrorCode::NameConflict) => {
            let items = client.list_folder_decrypted(parent_id).await?;
            let key = crate::model::normalize_folder_name(name);
            items
                .into_iter()
                .find(|node| {
                    node.kind() == NodeKind::Folder
                        && crate::model::normalize_folder_name(node.name()) == key
                })
                .map(|node| node.view.id)
                .ok_or(error)
        }
        Err(error) => Err(error),
    }
}

#[allow(clippy::too_many_arguments)]
fn emit(
    sink: &FolderSink,
    job_id: Uuid,
    status: FolderJobStatus,
    counts: &FolderCounts,
    completed_files: u64,
    total_files: u64,
    completed_bytes: u64,
    total_bytes: u64,
    current: Option<String>,
    error: Option<crate::model::TransferFailure>,
) {
    sink(FolderProgress {
        job_id,
        status,
        completed_files,
        total_files,
        completed_bytes,
        total_bytes,
        counts: *counts,
        current,
        error,
    });
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// 下载：先收集完整清单（用于进度与空目录），再逐文件下载。
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_download(
    client: &Client,
    job_id: Uuid,
    folder_id: Uuid,
    folder_name: &str,
    destination_root: &Path,
    policy: ConflictPolicy,
    control: &TransferControl,
    sink: &FolderSink,
) -> Result<FolderDownloadOutcome, ClientError> {
    let result = run_download_inner(
        client,
        job_id,
        folder_id,
        folder_name,
        destination_root,
        policy,
        control,
        sink,
    )
    .await;
    if let Err(error) = &result {
        let cancelled = matches!(error, ClientError::Cancelled) || control.is_cancelled();
        emit(
            sink,
            job_id,
            if cancelled {
                FolderJobStatus::Cancelled
            } else {
                FolderJobStatus::Failed
            },
            &FolderCounts::default(),
            0,
            0,
            0,
            0,
            None,
            Some(failure(error)),
        );
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_download_inner(
    client: &Client,
    job_id: Uuid,
    folder_id: Uuid,
    folder_name: &str,
    destination_root: &Path,
    policy: ConflictPolicy,
    control: &TransferControl,
    sink: &FolderSink,
) -> Result<FolderDownloadOutcome, ClientError> {
    client.master_key()?;
    let root_name = sanitize_local_name(folder_name);
    let root_dir = path_for_io(destination_root)
        .map_err(|_| ClientError::Internal("目标目录路径无效"))?
        .join(&root_name);
    std::fs::create_dir_all(&root_dir)?;

    emit(
        sink,
        job_id,
        FolderJobStatus::Scanning,
        &FolderCounts::default(),
        0,
        0,
        0,
        0,
        None,
        None,
    );
    let mut tree = FolderDownloadPlan::default();
    collect_download_plan(
        client,
        folder_id,
        root_dir.clone(),
        String::new(),
        &mut tree,
        control,
    )
    .await?;
    let total_files = tree.files.len() as u64;
    let total_bytes = tree.files.iter().map(|file| file.size).sum::<u64>();
    for directory in &tree.directories {
        std::fs::create_dir_all(
            path_for_io(directory).map_err(|_| ClientError::Internal("目录路径无效"))?,
        )?;
    }
    emit(
        sink,
        job_id,
        FolderJobStatus::Running,
        &FolderCounts::default(),
        0,
        total_files,
        0,
        total_bytes,
        None,
        None,
    );

    let mut completed_files = 0_u64;
    let mut completed_bytes = 0_u64;
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    let mut status = FolderJobStatus::Done;
    for file in &tree.files {
        if control.is_cancelled() {
            status = FolderJobStatus::Cancelled;
            break;
        }
        if control.is_paused() {
            emit(
                sink,
                job_id,
                FolderJobStatus::Running,
                &FolderCounts::default(),
                completed_files,
                total_files,
                completed_bytes,
                total_bytes,
                Some(file.relative.clone()),
                None,
            );
        }
        control.checkpoint().await?;
        let result = download::run_silent(
            client,
            file.node_id,
            file.version_id,
            &file.name,
            &file.parent_dir,
            policy,
            control,
        )
        .await;
        match result {
            Ok(outcome) => {
                completed_files = completed_files.saturating_add(1);
                completed_bytes = completed_bytes.saturating_add(file.size);
                if outcome.skipped {
                    skipped.push(SkippedItem {
                        path: file.relative.clone(),
                        reason: "本地已有同名文件，按策略跳过".to_owned(),
                    });
                }
            }
            Err(ClientError::Cancelled) => {
                status = FolderJobStatus::Cancelled;
                break;
            }
            Err(error) => {
                completed_files = completed_files.saturating_add(1);
                failed.push(SkippedItem {
                    path: file.relative.clone(),
                    reason: error.to_string(),
                });
            }
        }
        emit(
            sink,
            job_id,
            FolderJobStatus::Running,
            &FolderCounts {
                failed: failed.len() as u64,
                ..FolderCounts::default()
            },
            completed_files,
            total_files,
            completed_bytes,
            total_bytes,
            Some(file.relative.clone()),
            None,
        );
    }

    if status != FolderJobStatus::Cancelled && !failed.is_empty() {
        status = FolderJobStatus::Failed;
    }
    let outcome = FolderDownloadOutcome {
        job_id,
        status,
        root: root_dir,
        completed_files,
        total_files,
        completed_bytes,
        total_bytes,
        skipped,
        failed,
    };
    emit(
        sink,
        job_id,
        status,
        &FolderCounts {
            failed: outcome.failed.len() as u64,
            ..FolderCounts::default()
        },
        completed_files,
        total_files,
        completed_bytes,
        total_bytes,
        None,
        None,
    );
    Ok(outcome)
}

#[derive(Default)]
struct FolderDownloadPlan {
    directories: Vec<PathBuf>,
    files: Vec<DownloadPlanFile>,
}

struct DownloadPlanFile {
    relative: String,
    name: String,
    parent_dir: PathBuf,
    node_id: Uuid,
    version_id: Uuid,
    size: u64,
}

fn collect_download_plan<'a>(
    client: &'a Client,
    folder_id: Uuid,
    local_dir: PathBuf,
    prefix: String,
    tree: &'a mut FolderDownloadPlan,
    control: &'a TransferControl,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ClientError>> + Send + 'a>> {
    Box::pin(async move {
        control.checkpoint().await?;
        let items = client.list_folder_decrypted(folder_id).await?;
        for item in items {
            let sanitized = sanitize_local_name(item.name());
            let relative = if prefix.is_empty() {
                sanitized.clone()
            } else {
                format!("{prefix}/{sanitized}")
            };
            match item.kind() {
                NodeKind::Folder => {
                    let child = local_dir.join(&sanitized);
                    tree.directories.push(child.clone());
                    collect_download_plan(
                        client,
                        item.view.id,
                        child,
                        relative,
                        &mut *tree,
                        control,
                    )
                    .await?;
                }
                NodeKind::File => {
                    let version_id = item
                        .view
                        .current_version_id
                        .ok_or(ClientError::InvalidResponse("文件缺少当前版本"))?;
                    tree.files.push(DownloadPlanFile {
                        relative,
                        name: sanitized,
                        parent_dir: local_dir.clone(),
                        node_id: item.view.id,
                        version_id,
                        size: item.view.size.unwrap_or_default(),
                    });
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn projected_size_matches_crypto_core_layout() {
        assert_eq!(
            projected_ciphertext_size(0),
            crypto_core::FRAME_OVERHEAD as u64
        );
        assert_eq!(
            projected_ciphertext_size(crypto_core::CHUNK_PLAINTEXT_SIZE as u64),
            (crypto_core::CHUNK_PLAINTEXT_SIZE + crypto_core::FRAME_OVERHEAD) as u64
        );
    }

    #[test]
    fn format_bytes_is_stable() {
        assert_eq!(format_bytes(0), "0.0 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
    }
}
