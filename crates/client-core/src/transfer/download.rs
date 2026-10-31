//! 下载管线：逐块校验、解密、临时文件原子落盘。

use std::path::Path;
use std::time::Duration;

use crypto_core::{
    ContentHasher, ContentId, VersionId, chunk_count_for_size, decrypt_chunk,
    decrypt_version_metadata_for_content, derive_content_key, verify_plaintext_size,
};
use reqwest::Method;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::{ProgressSink, TransferControl, emit, failure, transfer_error};
use crate::Client;
use crate::error::ClientError;
use crate::files::DownloadManifestWire;
use crate::hex::decode_hex;
use crate::io_path::{choose_destination, create_temp_in_dir, path_for_io, sanitize_local_name};
use crate::model::{
    ConflictPolicy, DownloadOutcome, TransferDirection, TransferErrorCode, TransferStatus,
};

const MAX_CHUNK_GET_ATTEMPTS: u32 = 5;
const MAX_RATE_LIMIT_RETRIES: u32 = 8;
/// 单个文件最多重新申请下载清单的次数（预签名 URL 过期时）。
const MAX_MANIFEST_REFRESHES: u32 = 3;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run(
    client: &Client,
    node_id: Uuid,
    version_id: Uuid,
    file_name: &str,
    destination_dir: &Path,
    policy: ConflictPolicy,
    download_id: Uuid,
    control: &TransferControl,
    sink: Option<&ProgressSink>,
) -> Result<DownloadOutcome, ClientError> {
    let result = run_inner(
        client,
        node_id,
        version_id,
        file_name,
        destination_dir,
        policy,
        download_id,
        control,
        sink,
    )
    .await;
    if let (Err(error), Some(sink)) = (&result, sink) {
        let cancelled = matches!(error, ClientError::Cancelled) || control.is_cancelled();
        emit(
            sink,
            download_id,
            TransferDirection::Download,
            if cancelled {
                TransferStatus::Cancelled
            } else {
                TransferStatus::Failed
            },
            0,
            0,
            0,
            0,
            Some(failure(error)),
        );
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_inner(
    client: &Client,
    node_id: Uuid,
    version_id: Uuid,
    file_name: &str,
    destination_dir: &Path,
    policy: ConflictPolicy,
    download_id: Uuid,
    control: &TransferControl,
    sink: Option<&ProgressSink>,
) -> Result<DownloadOutcome, ClientError> {
    client.master_key()?;
    control.checkpoint().await?;
    let mut manifest = fetch_manifest(client, node_id, version_id).await?;
    let content_id = ContentId::parse(&manifest.content_id.to_string())?;
    let version_typed = VersionId::parse(&version_id.to_string())?;
    let node_key = client.load_wrapped_node_key(node_id, manifest.wrapped_key.as_deref())?;
    let encrypted_meta = decode_hex(&manifest.encrypted_meta)
        .ok_or(ClientError::InvalidResponse("版本元数据不是十六进制"))?;
    let metadata = decrypt_version_metadata_for_content(
        &node_key,
        &version_typed,
        &content_id,
        &encrypted_meta,
    )?;

    let expected_chunks = chunk_count_for_size(metadata.size)?;
    if manifest.chunk_count < 0
        || manifest.chunk_count as u32 != expected_chunks
        || manifest.chunks.len() != expected_chunks as usize
    {
        return Err(ClientError::Integrity("版本元数据与块清单不一致"));
    }

    let chunk_count = expected_chunks;
    let total_bytes = metadata.size;
    emit_optional(
        sink,
        download_id,
        TransferStatus::Downloading,
        0,
        total_bytes,
        0,
        chunk_count,
    );

    let destination = choose_destination(destination_dir, file_name, policy)?;
    let Some(mut destination) = destination else {
        let existing = path_for_io(destination_dir)
            .map_err(|_| ClientError::Internal("目标目录路径无效"))?
            .join(sanitize_local_name(file_name));
        emit_optional(
            sink,
            download_id,
            TransferStatus::Completed,
            0,
            total_bytes,
            chunk_count,
            chunk_count,
        );
        return Ok(DownloadOutcome {
            node_id,
            version_id,
            path: existing,
            skipped: true,
        });
    };

    let destination_dir =
        path_for_io(destination_dir).map_err(|_| ClientError::Internal("目标目录路径无效"))?;
    std::fs::create_dir_all(&destination_dir)?;
    let temp = create_temp_in_dir(&destination_dir, &sanitize_local_name(file_name))?;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path_for_io(temp.path()).map_err(|_| ClientError::Internal("临时路径无效"))?)
        .await?;

    let content_key = derive_content_key(&node_key, &content_id)?;
    let mut completed_bytes = 0_u64;
    manifest.chunks.sort_by_key(|chunk| chunk.idx);
    let original_chunks = manifest.chunks.clone();
    let mut refreshes = 0_u32;
    for (position, expected) in original_chunks.iter().enumerate() {
        if expected.idx < 0 || expected.idx as usize != position {
            return Err(ClientError::Integrity("下载清单块序号不连续"));
        }
        if expected.ciphertext_size < 0 {
            return Err(ClientError::Integrity("块密文大小无效"));
        }
        if control.is_paused() {
            emit_optional(
                sink,
                download_id,
                TransferStatus::Paused,
                completed_bytes,
                total_bytes,
                position as u32,
                chunk_count,
            );
        }
        control.checkpoint().await?;
        let ciphertext = loop {
            let url = manifest
                .chunks
                .get(position)
                .map(|chunk| chunk.url.clone())
                .ok_or(ClientError::Integrity("刷新后的下载清单缺少块"))?;
            match fetch_chunk(client, &url, control).await {
                Ok(ciphertext) => break ciphertext,
                Err(ChunkFetchError::Expired) => {
                    refreshes += 1;
                    if refreshes > MAX_MANIFEST_REFRESHES {
                        return Err(transfer_error(
                            TransferErrorCode::Other,
                            "下载地址反复过期，重新申请清单次数已达上限",
                        ));
                    }
                    let mut refreshed = fetch_manifest(client, node_id, version_id).await?;
                    verify_refreshed_manifest(&manifest, &mut refreshed)?;
                    manifest = refreshed;
                }
                Err(ChunkFetchError::Failed(error)) => return Err(error),
            }
        };
        if ciphertext.len() as i64 != expected.ciphertext_size {
            return Err(ClientError::Integrity("块密文大小与清单不一致"));
        }
        let mut hasher = ContentHasher::new();
        hasher.update(&ciphertext);
        if !Client::hashes_equal(&hasher.finish_hex(), &expected.sha256) {
            return Err(ClientError::Integrity("块密文 SHA-256 不匹配"));
        }
        let is_last = position + 1 == chunk_count as usize;
        let plaintext = decrypt_chunk(
            &content_key,
            &content_id,
            position as u32,
            chunk_count,
            is_last,
            &ciphertext,
        )
        .map_err(|_| ClientError::Integrity("块解密认证失败"))?;
        let expected_len = if is_last {
            metadata
                .size
                .saturating_sub(position as u64 * crypto_core::CHUNK_PLAINTEXT_SIZE as u64)
        } else {
            crypto_core::CHUNK_PLAINTEXT_SIZE as u64
        };
        if plaintext.len() as u64 != expected_len {
            return Err(ClientError::Integrity("块明文大小不符合分块布局"));
        }
        file.write_all(&plaintext).await?;
        completed_bytes = completed_bytes.saturating_add(plaintext.len() as u64);
        emit_optional(
            sink,
            download_id,
            TransferStatus::Downloading,
            completed_bytes,
            total_bytes,
            position as u32 + 1,
            chunk_count,
        );
    }

    verify_plaintext_size(&metadata, completed_bytes)?;
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    temp.persist(destination.path())?;
    destination.commit();
    emit_optional(
        sink,
        download_id,
        TransferStatus::Completed,
        completed_bytes,
        total_bytes,
        chunk_count,
        chunk_count,
    );
    Ok(DownloadOutcome {
        node_id,
        version_id,
        path: destination.path().to_path_buf(),
        skipped: false,
    })
}

fn emit_optional(
    sink: Option<&ProgressSink>,
    download_id: Uuid,
    status: TransferStatus,
    completed_bytes: u64,
    total_bytes: u64,
    completed_chunks: u32,
    total_chunks: u32,
) {
    if let Some(sink) = sink {
        emit(
            sink,
            download_id,
            TransferDirection::Download,
            status,
            completed_bytes,
            total_bytes,
            completed_chunks,
            total_chunks,
            None,
        );
    }
}

enum ChunkFetchError {
    /// 预签名 URL 已过期/被拒绝，需要重新申请下载清单。
    Expired,
    Failed(ClientError),
}

impl From<ClientError> for ChunkFetchError {
    fn from(error: ClientError) -> Self {
        Self::Failed(error)
    }
}

async fn fetch_manifest(
    client: &Client,
    node_id: Uuid,
    version_id: Uuid,
) -> Result<DownloadManifestWire, ClientError> {
    client
        .api
        .request_json::<DownloadManifestWire>(
            Method::GET,
            &format!("/api/v1/nodes/{node_id}/versions/{version_id}/download"),
            None,
        )
        .await
}

/// 重新申请清单后必须与原清单完全一致，防止服务端在重试时替换内容/块映射。
fn verify_refreshed_manifest(
    original: &DownloadManifestWire,
    refreshed: &mut DownloadManifestWire,
) -> Result<(), ClientError> {
    refreshed.chunks.sort_by_key(|chunk| chunk.idx);
    let same_head = refreshed.content_id == original.content_id
        && refreshed.chunk_count == original.chunk_count
        && refreshed.encrypted_meta == original.encrypted_meta
        && refreshed.chunks.len() == original.chunks.len();
    if !same_head {
        return Err(ClientError::Integrity("刷新后的下载清单与原清单不一致"));
    }
    for (left, right) in original.chunks.iter().zip(refreshed.chunks.iter()) {
        if left.idx != right.idx
            || left.ciphertext_size != right.ciphertext_size
            || !Client::hashes_equal(&left.sha256, &right.sha256)
        {
            return Err(ClientError::Integrity("刷新后的下载清单块信息不一致"));
        }
    }
    Ok(())
}

async fn fetch_chunk(
    client: &Client,
    url: &str,
    control: &TransferControl,
) -> Result<Vec<u8>, ChunkFetchError> {
    let mut attempt = 0_u32;
    let mut rate_retries = 0_u32;
    loop {
        control.checkpoint().await?;
        let response = client.storage_http.get(url).send().await;
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                attempt += 1;
                if attempt >= MAX_CHUNK_GET_ATTEMPTS {
                    return Err(ChunkFetchError::Failed(transfer_error(
                        TransferErrorCode::Network,
                        "下载块时网络中断",
                    )));
                }
                sleep_backoff(client, attempt, control).await?;
                continue;
            }
        };
        let status = response.status();
        if status.is_success() {
            return response
                .bytes()
                .await
                .map(|bytes| bytes.to_vec())
                .map_err(|_| {
                    ChunkFetchError::Failed(transfer_error(
                        TransferErrorCode::Network,
                        "读取块响应失败",
                    ))
                });
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            // OSS/MinIO 对过期预签名 URL 返回 403（AccessDenied / Request has expired）。
            return Err(ChunkFetchError::Expired);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            rate_retries += 1;
            if rate_retries > MAX_RATE_LIMIT_RETRIES {
                return Err(ChunkFetchError::Failed(transfer_error(
                    TransferErrorCode::RateLimited,
                    "对象存储持续限流",
                )));
            }
            let retry_after = retry_after(response.headers());
            let delay = crate::api::rate_limit_delay(retry_after, client.clock.jitter());
            client.clock.sleep(delay).await;
            continue;
        }
        if status.is_server_error() {
            attempt += 1;
            if attempt >= MAX_CHUNK_GET_ATTEMPTS {
                return Err(ChunkFetchError::Failed(transfer_error(
                    TransferErrorCode::Server,
                    "对象存储暂时不可用",
                )));
            }
            sleep_backoff(client, attempt, control).await?;
            continue;
        }
        return Err(ChunkFetchError::Failed(transfer_error(
            TransferErrorCode::Other,
            "对象存储拒绝了块下载",
        )));
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

/// 文件夹下载复用的“无进度”入口。
pub(super) async fn run_silent(
    client: &Client,
    node_id: Uuid,
    version_id: Uuid,
    file_name: &str,
    destination_dir: &Path,
    policy: ConflictPolicy,
    control: &TransferControl,
) -> Result<DownloadOutcome, ClientError> {
    run(
        client,
        node_id,
        version_id,
        file_name,
        destination_dir,
        policy,
        Uuid::new_v4(),
        control,
        None,
    )
    .await
}
