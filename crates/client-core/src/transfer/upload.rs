//! 上传管线：4 MiB 加密分块、批量预签名/确认、提交与断点续传。

use std::path::Path;
use std::time::Duration;

use crypto_core::{
    CHUNK_PLAINTEXT_SIZE, ContentHasher, ContentId, NodeId, NodeKey, NodeMetadata, VersionId,
    VersionMetadata, chunk_count_for_size, compute_name_token, derive_content_key, derive_name_key,
    encrypt_chunk, encrypt_node_metadata, encrypt_version_metadata, is_valid_content_sha256,
    wrap_node_key,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    CHUNK_CONCURRENCY, MAX_BATCH_CHUNKS, ProgressSink, TransferControl, UploadRequest, emit,
    failure, file_name, transfer_error,
};
use crate::Client;
use crate::error::ClientError;
use crate::hex::encode_hex;
use crate::io_path::{
    FileFingerprint, derive_mime, ensure_valid_name, normalize_cloud_name, open_for_shared_read,
    read_chunk,
};
use crate::model::{
    PendingUploadStatus, TransferDirection, TransferErrorCode, TransferStatus, UploadOutcome,
};
use crate::state::{ConfirmedChunk, UploadMode, UploadRecord};

const MAX_CHUNK_PUT_ATTEMPTS: u32 = 5;
const MAX_RATE_LIMIT_RETRIES: u32 = 8;
const MAX_RESIGN: u32 = 5;

#[derive(Debug, Deserialize)]
struct UploadSessionWire {
    id: Uuid,
    content_id: Uuid,
    kind: String,
    status: String,
    node_id: Uuid,
    version_id: Uuid,
    chunk_count: i32,
    declared_size: i64,
    #[serde(default)]
    wrapped_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConfirmedChunksWire {
    chunk_count: i32,
    items: Vec<ConfirmedChunkWire>,
}

#[derive(Debug, Deserialize)]
struct ConfirmedChunkWire {
    idx: i32,
    declared_size: i64,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct PresignBatchWire {
    items: Vec<PresignItemWire>,
}

#[derive(Debug, Deserialize)]
struct PresignItemWire {
    idx: i32,
    ok: bool,
    url: Option<String>,
    expected_size: Option<i64>,
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConfirmBatchWire {
    items: Vec<ConfirmItemWire>,
}

#[derive(Debug, Deserialize)]
struct ConfirmItemWire {
    idx: i32,
    ok: bool,
    declared_size: Option<i64>,
    sha256: Option<String>,
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommitWire {
    status: String,
    node_id: Uuid,
    version_id: Uuid,
    content_id: Uuid,
}

#[derive(Debug, Clone)]
struct ConfirmInput {
    idx: u32,
    declared_size: i64,
    sha256: String,
}

#[derive(Default)]
struct RunContext {
    session_id: Option<Uuid>,
    total_bytes: u64,
    total_chunks: u32,
    completed_bytes: u64,
    completed_chunks: u32,
}

pub(super) async fn run(
    client: &Client,
    request: &UploadRequest,
    transfer_id: Uuid,
    control: &TransferControl,
    sink: &ProgressSink,
) -> Result<UploadOutcome, ClientError> {
    let mut context = RunContext::default();
    let result = run_inner(client, request, transfer_id, control, sink, &mut context).await;

    match &result {
        Ok(outcome) => {
            emit(
                sink,
                transfer_id,
                TransferDirection::Upload,
                TransferStatus::Completed,
                context.total_bytes,
                context.total_bytes,
                context.total_chunks,
                context.total_chunks,
                None,
            );
            Ok(outcome.clone())
        }
        Err(error) => {
            let cancelled = matches!(error, ClientError::Cancelled) || control.is_cancelled();
            if let Some(session_id) = context.session_id {
                if cancelled || matches!(error, ClientError::FileChanged) {
                    let _ = client.abort_upload_session(session_id).await;
                    let _ = client.state_store.delete_upload(session_id).await;
                } else if matches!(
                    error.api_code(),
                    Some(crate::error::ApiErrorCode::UploadSessionClosed)
                ) {
                    let _ = client.state_store.delete_upload(session_id).await;
                } else {
                    let _ = client
                        .state_store
                        .set_status(session_id, PendingUploadStatus::Failed)
                        .await;
                }
            }
            emit(
                sink,
                transfer_id,
                TransferDirection::Upload,
                if cancelled {
                    TransferStatus::Cancelled
                } else {
                    TransferStatus::Failed
                },
                context.completed_bytes,
                context.total_bytes,
                context.completed_chunks,
                context.total_chunks,
                Some(failure(error)),
            );
            Err(result.err().unwrap_or(ClientError::Cancelled))
        }
    }
}

async fn run_inner(
    client: &Client,
    request: &UploadRequest,
    transfer_id: Uuid,
    control: &TransferControl,
    sink: &ProgressSink,
    context: &mut RunContext,
) -> Result<UploadOutcome, ClientError> {
    client.ensure_writable()?;
    let master_key = client.master_key()?;
    control.checkpoint().await?;

    let (mode, parent_id, node_id, path) = match request {
        UploadRequest::NewFile { parent_id, path } => {
            (UploadMode::NewFile, Some(*parent_id), None, path.clone())
        }
        UploadRequest::NewVersion { node_id, path } => {
            (UploadMode::NewVersion, None, Some(*node_id), path.clone())
        }
        UploadRequest::Resume { session_id, path } => {
            // 记录里保存了 mode/父节点；这里先读出，再走统一的续传逻辑。
            let record = client
                .state_store
                .get_upload(*session_id)
                .await?
                .ok_or_else(|| {
                    transfer_error(TransferErrorCode::Other, "找不到待续传的上传会话")
                })?;
            if record.user_id != client.ensure_writable()?.user_id
                || record.local_path.as_path() != path.as_path()
            {
                return Err(transfer_error(
                    TransferErrorCode::Other,
                    "续传记录与当前账号或文件路径不一致",
                ));
            }
            (
                record.mode,
                record.parent_id,
                Some(record.node_id),
                path.clone(),
            )
        }
    };

    let file_name = normalize_cloud_name(file_name(&path)?);
    ensure_valid_name(&file_name)?;
    let before = FileFingerprint::of_path(&path)?;

    let existing = match request {
        UploadRequest::Resume { session_id, .. } => {
            client.state_store.get_upload(*session_id).await?
        }
        _ => {
            client
                .state_store
                .find_upload(
                    client.ensure_writable()?.user_id,
                    mode,
                    parent_id,
                    node_id,
                    &path,
                    before.size,
                    before.mtime_ns,
                )
                .await?
        }
    };

    if let Some(record) = existing {
        if record.file_size != before.size || record.file_mtime_ns != before.mtime_ns {
            return Err(transfer_error(
                TransferErrorCode::Other,
                "文件指纹与续传记录不一致，请重新选择文件",
            ));
        }
        let session = get_session(client, record.session_id).await?;
        if session.status == "committed" {
            let _ = client.state_store.delete_upload(record.session_id).await;
            context.total_bytes = record.file_size;
            context.total_chunks = record.chunk_count;
            context.completed_bytes = record.file_size;
            context.completed_chunks = record.chunk_count;
            return Ok(UploadOutcome {
                node_id: record.node_id,
                version_id: record.version_id,
                content_id: record.content_id,
                size: record.file_size,
            });
        }
        if session.status == "open" {
            verify_session(
                &session,
                record.content_id,
                record.node_id,
                record.version_id,
                record.chunk_count,
                record.declared_size,
                mode,
            )?;
            let node_key = client
                .load_wrapped_node_key(record.node_id, session.wrapped_key.as_deref())
                .or_else(|_| {
                    client
                        .cached_node_key(record.node_id)?
                        .ok_or(ClientError::NodeKeyUnavailable)
                })?;
            context.session_id = Some(record.session_id);
            context.total_bytes = record.file_size;
            context.total_chunks = record.chunk_count;
            if control.is_paused() {
                emit(
                    sink,
                    transfer_id,
                    TransferDirection::Upload,
                    TransferStatus::Paused,
                    0,
                    context.total_bytes,
                    0,
                    context.total_chunks,
                    None,
                );
            }
            control.checkpoint().await?;
            return run_session(
                client,
                transfer_id,
                &record,
                &path,
                before,
                &node_key,
                control,
                sink,
                context,
            )
            .await;
        }
        // 会话已中止/过期：从头开始。
        let _ = client.state_store.delete_upload(record.session_id).await;
    }

    start_new(
        client,
        transfer_id,
        mode,
        parent_id,
        node_id,
        &path,
        &file_name,
        &master_key,
        before,
        control,
        sink,
        context,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn start_new(
    client: &Client,
    transfer_id: Uuid,
    mode: UploadMode,
    parent_id: Option<Uuid>,
    node_id: Option<Uuid>,
    path: &Path,
    name: &str,
    master_key: &crypto_core::MasterKey,
    before: FileFingerprint,
    control: &TransferControl,
    sink: &ProgressSink,
    context: &mut RunContext,
) -> Result<UploadOutcome, ClientError> {
    // 流式哈希；完成后再次校验指纹，文件在上传开始前被修改就放弃。
    let content_sha256 = hash_file(path, before, control).await?;

    let chunk_count = chunk_count_for_size(before.size)?;
    let declared_size = before
        .size
        .checked_add(u64::from(chunk_count) * crypto_core::CHUNK_CIPHERTEXT_OVERHEAD as u64)
        .ok_or_else(|| transfer_error(TransferErrorCode::Other, "文件大小溢出"))?;
    let content_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let node_id = node_id.unwrap_or_else(Uuid::new_v4);

    let node_typed = NodeId::parse(&node_id.to_string())?;
    let version_typed = VersionId::parse(&version_id.to_string())?;
    let content_typed = ContentId::parse(&content_id.to_string())?;

    let (node_key, wrapped_key, encrypted_meta, name_token) = match mode {
        UploadMode::NewFile => {
            let parent_id = parent_id.ok_or_else(|| {
                transfer_error(TransferErrorCode::TargetUnavailable, "缺少父文件夹")
            })?;
            let node_key = NodeKey::generate()?;
            let wrapped = wrap_node_key(master_key, &node_typed, &node_key)?;
            let meta = encrypt_node_metadata(
                &node_key,
                &node_typed,
                &NodeMetadata {
                    name: name.to_owned(),
                },
            )?;
            let name_key = derive_name_key(master_key)?;
            let parent_typed = NodeId::parse(&parent_id.to_string())?;
            let token = compute_name_token(&name_key, &parent_typed, name)?;
            (
                node_key,
                Some(encode_hex(&wrapped)),
                Some(encode_hex(&meta)),
                Some(token.to_hex()),
            )
        }
        UploadMode::NewVersion => {
            let node_key = client
                .ensure_file_node_key(node_id)
                .await
                .map_err(|_| ClientError::NodeKeyUnavailable)?;
            (node_key, None, None, None)
        }
    };

    let metadata = VersionMetadata {
        content_id: content_typed,
        size: before.size,
        mtime: before.mtime_ns / 1_000_000_000,
        mime: derive_mime(&path_for_display(path)),
        content_sha256: Some(content_sha256),
    };
    let version_meta = encrypt_version_metadata(&node_key, &version_typed, &metadata)?;

    let mut body = json!({
        "content_id": content_id,
        "kind": mode.as_str(),
        "node_id": node_id,
        "version_id": version_id,
        "version_meta": encode_hex(&version_meta),
        "chunk_count": chunk_count,
        "declared_size": declared_size,
    });
    if mode == UploadMode::NewFile {
        let parent_id = parent_id
            .ok_or_else(|| transfer_error(TransferErrorCode::TargetUnavailable, "缺少父文件夹"))?;
        let object = body
            .as_object_mut()
            .ok_or_else(|| ClientError::Internal("上传请求体构造失败"))?;
        object.insert("parent_id".to_owned(), json!(parent_id));
        object.insert(
            "wrapped_key".to_owned(),
            json!(wrapped_key.unwrap_or_default()),
        );
        object.insert(
            "encrypted_meta".to_owned(),
            json!(encrypted_meta.unwrap_or_default()),
        );
        object.insert(
            "name_token".to_owned(),
            json!(name_token.unwrap_or_default()),
        );
    }

    let session: UploadSessionWire = client
        .api
        .request_json(Method::POST, "/api/v1/upload-sessions", Some(&body))
        .await?;
    verify_session(
        &session,
        content_id,
        node_id,
        version_id,
        chunk_count,
        declared_size,
        mode,
    )?;
    client.load_wrapped_node_key(node_id, session.wrapped_key.as_deref())?;
    client.cache_node_key(node_id, node_key.clone())?;

    let record = UploadRecord {
        session_id: session.id,
        user_id: client.ensure_writable()?.user_id,
        local_path: path.to_path_buf(),
        mode,
        parent_id,
        node_id,
        content_id,
        version_id,
        chunk_count,
        declared_size,
        file_size: before.size,
        file_mtime_ns: before.mtime_ns,
        status: PendingUploadStatus::Uploading,
    };
    client.state_store.upsert_upload(record.clone()).await?;
    context.session_id = Some(session.id);
    context.total_bytes = before.size;
    context.total_chunks = chunk_count;

    run_session(
        client,
        transfer_id,
        &record,
        path,
        before,
        &node_key,
        control,
        sink,
        context,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_session(
    client: &Client,
    transfer_id: Uuid,
    record: &UploadRecord,
    path: &Path,
    fingerprint: FileFingerprint,
    node_key: &NodeKey,
    control: &TransferControl,
    sink: &ProgressSink,
    context: &mut RunContext,
) -> Result<UploadOutcome, ClientError> {
    let confirmed: ConfirmedChunksWire = client
        .api
        .request_json(
            Method::GET,
            &format!("/api/v1/upload-sessions/{}/chunks", record.session_id),
            None,
        )
        .await?;
    if confirmed.chunk_count < 0 || confirmed.chunk_count as u32 != record.chunk_count {
        return Err(ClientError::InvalidResponse(
            "已确认块列表的块数与会话不一致",
        ));
    }
    let mut server_confirmed = Vec::with_capacity(confirmed.items.len());
    for chunk in &confirmed.items {
        if chunk.idx < 0 {
            return Err(ClientError::InvalidResponse("已确认块序号为负"));
        }
        server_confirmed.push(ConfirmedChunk {
            idx: chunk.idx as u32,
            declared_size: chunk.declared_size,
            sha256: chunk.sha256.clone(),
        });
    }
    client
        .state_store
        .replace_confirmed(record.session_id, server_confirmed.clone())
        .await?;

    let mut done = vec![false; record.chunk_count as usize];
    let mut completed = 0_u32;
    let mut completed_bytes = 0_u64;
    for chunk in &server_confirmed {
        if let Some(slot) = done.get_mut(chunk.idx as usize)
            && !*slot
        {
            *slot = true;
            completed += 1;
            completed_bytes = completed_bytes.saturating_add(plaintext_len(chunk.declared_size));
        }
    }
    context.completed_chunks = completed;
    context.completed_bytes = completed_bytes;
    emit(
        sink,
        transfer_id,
        TransferDirection::Upload,
        TransferStatus::Uploading,
        completed_bytes,
        context.total_bytes,
        completed,
        context.total_chunks,
        None,
    );

    let missing: Vec<u32> = done
        .iter()
        .enumerate()
        .filter_map(|(idx, done)| (!done).then_some(idx as u32))
        .collect();

    for batch in missing.chunks(MAX_BATCH_CHUNKS) {
        if control.is_paused() {
            emit(
                sink,
                transfer_id,
                TransferDirection::Upload,
                TransferStatus::Paused,
                context.completed_bytes,
                context.total_bytes,
                context.completed_chunks,
                context.total_chunks,
                None,
            );
        }
        control.checkpoint().await?;
        if control.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        let presigned = presign_batch(client, record.session_id, batch).await?;
        let mut presigned_map = std::collections::HashMap::new();
        for item in &presigned.items {
            if item.idx >= 0 {
                presigned_map.insert(item.idx as u32, item);
            }
        }

        let mut join_set = tokio::task::JoinSet::new();
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(CHUNK_CONCURRENCY));
        let mut presign_failure: Option<ClientError> = None;
        for idx in batch.iter().copied() {
            let item = presigned_map.get(&idx);
            let Some(item) = item.filter(|item| item.ok && item.url.is_some()) else {
                let (code, message) = item.map_or(
                    ("presign_failed".to_owned(), "预签名失败".to_owned()),
                    |item| {
                        (
                            item.code
                                .clone()
                                .unwrap_or_else(|| "presign_failed".to_owned()),
                            item.message
                                .clone()
                                .unwrap_or_else(|| "预签名失败".to_owned()),
                        )
                    },
                );
                presign_failure = Some(api_code_error(&code, &message));
                continue;
            };
            let Some(expected_size) = item.expected_size else {
                presign_failure = Some(ClientError::InvalidResponse("预签名缺少块大小"));
                continue;
            };
            let url = item.url.clone().unwrap_or_default();
            let client = client.clone();
            let control = control.clone();
            let path = path.to_path_buf();
            let node_key = node_key.clone();
            let content_id = record.content_id;
            let count = record.chunk_count;
            let session_id = record.session_id;
            let semaphore = semaphore.clone();
            join_set.spawn(async move {
                let _permit = match semaphore.acquire_owned().await {
                    Ok(permit) => permit,
                    Err(_) => return (idx, Err(ClientError::Internal("块并发控制失败"))),
                };
                let result = upload_one_chunk(
                    &client,
                    &control,
                    session_id,
                    &path,
                    fingerprint,
                    &node_key,
                    content_id,
                    count,
                    idx,
                    &url,
                    expected_size as u64,
                )
                .await;
                (idx, result)
            });
        }

        let mut confirms = Vec::new();
        let mut chunk_error = None;
        while let Some(joined) = join_set.join_next().await {
            match joined {
                Ok((_idx, Ok(confirm))) => confirms.push(confirm),
                Ok((_idx, Err(error))) => {
                    if chunk_error.is_none() {
                        chunk_error = Some(error);
                    }
                }
                Err(_) => {
                    if chunk_error.is_none() {
                        chunk_error = Some(ClientError::Internal("上传块任务失败"));
                    }
                }
            }
        }
        if let Some(error) = chunk_error {
            return Err(error);
        }
        if let Some(error) = presign_failure {
            return Err(error);
        }
        if confirms.is_empty() {
            continue;
        }
        let confirmed = confirm_batch(client, record.session_id, &confirms).await?;
        for item in &confirmed.items {
            if item.ok {
                let declared = item.declared_size.unwrap_or_default();
                let sha256 = item.sha256.clone().unwrap_or_default();
                if item.idx >= 0 {
                    client
                        .state_store
                        .record_confirmed(
                            record.session_id,
                            ConfirmedChunk {
                                idx: item.idx as u32,
                                declared_size: declared,
                                sha256,
                            },
                        )
                        .await?;
                }
                context.completed_chunks = context.completed_chunks.saturating_add(1);
                context.completed_bytes = context
                    .completed_bytes
                    .saturating_add(plaintext_len(declared));
            } else {
                return Err(api_code_error(
                    item.code.as_deref().unwrap_or("confirm_failed"),
                    item.message.as_deref().unwrap_or("块确认失败"),
                ));
            }
        }
        emit(
            sink,
            transfer_id,
            TransferDirection::Upload,
            TransferStatus::Uploading,
            context.completed_bytes,
            context.total_bytes,
            context.completed_chunks,
            context.total_chunks,
            None,
        );
    }

    if control.is_paused() {
        emit(
            sink,
            transfer_id,
            TransferDirection::Upload,
            TransferStatus::Paused,
            context.completed_bytes,
            context.total_bytes,
            context.completed_chunks,
            context.total_chunks,
            None,
        );
    }
    control.checkpoint().await?;
    let after = FileFingerprint::of_path(path)?;
    if after.changed_from(fingerprint) {
        return Err(ClientError::FileChanged);
    }
    let commit: CommitWire = client
        .api
        .request_json(
            Method::POST,
            &format!("/api/v1/upload-sessions/{}/commit", record.session_id),
            None,
        )
        .await?;
    if commit.status != "committed" {
        return Err(ClientError::InvalidResponse("提交后会话状态异常"));
    }
    let _ = client.state_store.delete_upload(record.session_id).await;
    context.completed_chunks = context.total_chunks;
    context.completed_bytes = context.total_bytes;
    Ok(UploadOutcome {
        node_id: commit.node_id,
        version_id: commit.version_id,
        content_id: commit.content_id,
        size: record.file_size,
    })
}

#[allow(clippy::too_many_arguments)]
async fn upload_one_chunk(
    client: &Client,
    control: &TransferControl,
    session_id: Uuid,
    path: &Path,
    fingerprint: FileFingerprint,
    node_key: &NodeKey,
    content_id: Uuid,
    count: u32,
    idx: u32,
    url: &str,
    expected_size: u64,
) -> Result<ConfirmInput, ClientError> {
    control.checkpoint().await?;
    let path = path.to_path_buf();
    let content_typed = ContentId::parse(&content_id.to_string())?;
    let content_key = derive_content_key(node_key, &content_typed)?;
    let plaintext = tokio::task::spawn_blocking(move || {
        let before = FileFingerprint::of_path(&path)?;
        if before.changed_from(fingerprint) {
            return Err(ClientError::FileChanged);
        }
        let plaintext = read_chunk(&path, idx, CHUNK_PLAINTEXT_SIZE, fingerprint.size)?;
        let after = FileFingerprint::of_path(&path)?;
        if after.changed_from(fingerprint) {
            return Err(ClientError::FileChanged);
        }
        Ok::<_, ClientError>(plaintext)
    })
    .await
    .map_err(|_| ClientError::Internal("读取文件块任务失败"))??;

    let plaintext = zeroize::Zeroizing::new(plaintext);
    let ciphertext = encrypt_chunk(
        &content_key,
        &content_typed,
        idx,
        count,
        idx + 1 == count,
        &plaintext,
    )?;
    if ciphertext.len() as u64 != expected_size {
        return Err(transfer_error(
            TransferErrorCode::ChunkSizeMismatch,
            "加密后的块大小与预签名声明不一致",
        ));
    }
    let mut hasher = ContentHasher::new();
    hasher.update(&ciphertext);
    let sha256 = hasher.finish_hex();

    control.checkpoint().await?;
    let mut url = url.to_owned();
    let mut attempt = 0_u32;
    let mut rate_retries = 0_u32;
    let mut resigns = 0_u32;
    loop {
        if control.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        let response = client
            .storage_http
            .put(&url)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(ciphertext.clone())
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                attempt += 1;
                if attempt >= MAX_CHUNK_PUT_ATTEMPTS {
                    return Err(transfer_error(
                        TransferErrorCode::Network,
                        "上传块时网络中断",
                    ));
                }
                sleep_backoff(client, attempt, control).await?;
                continue;
            }
        };
        let status = response.status();
        if status.is_success() {
            return Ok(ConfirmInput {
                idx,
                declared_size: expected_size as i64,
                sha256,
            });
        }
        if status == reqwest::StatusCode::FORBIDDEN && resigns < MAX_RESIGN {
            resigns += 1;
            let fresh = presign_batch(client, session_id, &[idx]).await?;
            let item = fresh.items.iter().find(|item| item.idx == idx as i32);
            let Some(item) = item.filter(|item| item.ok && item.url.is_some()) else {
                return Err(transfer_error(
                    TransferErrorCode::Other,
                    "预签名 URL 过期后重新申请失败",
                ));
            };
            url = item.url.clone().unwrap_or_default();
            continue;
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            rate_retries += 1;
            if rate_retries > MAX_RATE_LIMIT_RETRIES {
                return Err(transfer_error(
                    TransferErrorCode::RateLimited,
                    "上传块时服务器持续限流",
                ));
            }
            let retry_after = retry_after(response.headers());
            let delay = crate::api::rate_limit_delay(retry_after, client.clock.jitter());
            client.clock.sleep(delay).await;
            continue;
        }
        if status.is_server_error() {
            attempt += 1;
            if attempt >= MAX_CHUNK_PUT_ATTEMPTS {
                return Err(transfer_error(
                    TransferErrorCode::Server,
                    "对象存储暂时不可用",
                ));
            }
            sleep_backoff(client, attempt, control).await?;
            continue;
        }
        return Err(transfer_error(
            TransferErrorCode::Other,
            "对象存储拒绝了块上传",
        ));
    }
}

async fn sleep_backoff(
    client: &Client,
    attempt: u32,
    control: &TransferControl,
) -> Result<(), ClientError> {
    let base_ms = 500_u64.saturating_mul(1_u64 << attempt.min(6));
    let jitter_ms = (client.clock.jitter().clamp(0.0, 1.0) * 250.0) as u64;
    client
        .clock
        .sleep(Duration::from_millis(base_ms.saturating_add(jitter_ms)))
        .await;
    control.checkpoint().await
}

async fn hash_file(
    path: &Path,
    fingerprint: FileFingerprint,
    control: &TransferControl,
) -> Result<String, ClientError> {
    let path = path.to_path_buf();
    let hash = tokio::task::spawn_blocking(move || {
        let before = FileFingerprint::of_path(&path)?;
        if before.changed_from(fingerprint) {
            return Err(ClientError::FileChanged);
        }
        use std::io::Read;
        let mut file = open_for_shared_read(&path)?;
        let mut hasher = ContentHasher::new();
        let mut buffer = vec![0_u8; CHUNK_PLAINTEXT_SIZE];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let after = FileFingerprint::of_path(&path)?;
        if after.changed_from(fingerprint) {
            return Err(ClientError::FileChanged);
        }
        Ok::<_, ClientError>(hasher.finish_hex())
    })
    .await
    .map_err(|_| ClientError::Internal("哈希任务失败"))??;
    control.checkpoint().await?;
    if !is_valid_content_sha256(&hash) {
        return Err(ClientError::Internal("内容摘要格式无效"));
    }
    Ok(hash)
}

async fn get_session(client: &Client, session_id: Uuid) -> Result<UploadSessionWire, ClientError> {
    client
        .api
        .request_json(
            Method::GET,
            &format!("/api/v1/upload-sessions/{session_id}"),
            None,
        )
        .await
}

async fn presign_batch(
    client: &Client,
    session_id: Uuid,
    indices: &[u32],
) -> Result<PresignBatchWire, ClientError> {
    let indices: Vec<i64> = indices.iter().map(|idx| i64::from(*idx)).collect();
    let body = json!({ "indices": indices });
    client
        .api
        .request_json(
            Method::POST,
            &format!("/api/v1/upload-sessions/{session_id}/chunks/presign"),
            Some(&body),
        )
        .await
}

async fn confirm_batch(
    client: &Client,
    session_id: Uuid,
    chunks: &[ConfirmInput],
) -> Result<ConfirmBatchWire, ClientError> {
    let chunks: Vec<Value> = chunks
        .iter()
        .map(|chunk| {
            json!({
                "idx": chunk.idx,
                "declared_size": chunk.declared_size,
                "sha256": chunk.sha256,
            })
        })
        .collect();
    let body = json!({ "chunks": chunks });
    client
        .api
        .request_json(
            Method::POST,
            &format!("/api/v1/upload-sessions/{session_id}/chunks/confirm"),
            Some(&body),
        )
        .await
}

fn verify_session(
    session: &UploadSessionWire,
    content_id: Uuid,
    node_id: Uuid,
    version_id: Uuid,
    chunk_count: u32,
    declared_size: u64,
    mode: UploadMode,
) -> Result<(), ClientError> {
    if session.content_id != content_id
        || session.node_id != node_id
        || session.version_id != version_id
        || session.chunk_count < 0
        || session.chunk_count as u32 != chunk_count
        || session.declared_size < 0
        || session.declared_size as u64 != declared_size
        || session.kind != mode.as_str()
    {
        return Err(ClientError::InvalidResponse("上传会话与请求不一致"));
    }
    Ok(())
}

fn plaintext_len(ciphertext_size: i64) -> u64 {
    u64::try_from(ciphertext_size)
        .unwrap_or_default()
        .saturating_sub(crypto_core::CHUNK_CIPHERTEXT_OVERHEAD as u64)
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let seconds = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

fn api_code_error(code: &str, message: &str) -> ClientError {
    use crate::error::ApiErrorCode;
    let known = match code {
        "name_conflict" => Some(ApiErrorCode::NameConflict),
        "target_unavailable" => Some(ApiErrorCode::TargetUnavailable),
        "quota_exceeded" => Some(ApiErrorCode::QuotaExceeded),
        "upload_session_closed" => Some(ApiErrorCode::UploadSessionClosed),
        "chunk_not_found" => Some(ApiErrorCode::ChunkNotFound),
        "chunk_size_mismatch" => Some(ApiErrorCode::ChunkSizeMismatch),
        "upload_incomplete" => Some(ApiErrorCode::UploadIncomplete),
        _ => None,
    };
    match known {
        Some(code) => ClientError::Api(crate::error::ApiFailure {
            status: 400,
            code,
            message: message.to_owned(),
            request_id: None,
            retry_after: None,
        }),
        None => transfer_error(TransferErrorCode::Other, message.to_owned()),
    }
}

fn path_for_display(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn plaintext_size_subtracts_frame_overhead_per_chunk() {
        assert_eq!(plaintext_len(41), 0);
        assert_eq!(
            plaintext_len((CHUNK_PLAINTEXT_SIZE + 41) as i64),
            CHUNK_PLAINTEXT_SIZE as u64
        );
    }

    #[test]
    fn error_mapping_is_stable() {
        let error = api_code_error("quota_exceeded", "空间不足");
        assert_eq!(error.transfer_code(), TransferErrorCode::QuotaExceeded);
    }

    #[test]
    fn chunk_size_for_empty_file_is_one() {
        assert_eq!(chunk_count_for_size(0).unwrap(), 1);
    }
}
