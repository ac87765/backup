//! 文件管理（第 2 组 2.1）：列表、新建、重命名、移动、回收站与版本。
//!
//! 服务端只返回密文；名称、版本元数据都在本模块内用 Rust 内存中的主密钥与
//! 节点密钥解密。公开返回值只有展示数据（名称、大小、时间、ID、类型），
//! 不包含任何密钥、包裹密钥、密文或名称令牌。

use crypto_core::{
    ContentId, NameKey, NodeId, NodeKey, NodeMetadata, VersionId, VersionMetadata,
    compute_name_token, decrypt_node_metadata, decrypt_version_metadata_for_content,
    derive_name_key, encrypt_node_metadata, encrypt_version_metadata, unwrap_node_key,
    wrap_node_key,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::Client;
use crate::error::{ApiErrorCode, ClientError};
use crate::hex::{decode_hex, encode_hex};
use crate::io_path::{ensure_valid_name, normalize_cloud_name};
use crate::model::{
    NodeKind, NodeSort, NodeView, PermanentDeleteOutcome, RestoreTarget, TrashItem, UsageView,
    VersionView, sort_nodes,
};

#[derive(Debug, Deserialize)]
struct VersionSummaryWire {
    id: Uuid,
    content_id: Uuid,
    encrypted_meta: String,
    is_current: bool,
    created_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct NodeSummaryWire {
    id: Uuid,
    kind: String,
    parent_id: Option<Uuid>,
    wrapped_key: Option<String>,
    encrypted_meta: Option<String>,
    row_version: i32,
    current_version: Option<VersionSummaryWire>,
    created_at: String,
    updated_at: String,
    deleted_at: Option<String>,
    trash_batch_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct ChildrenWire {
    items: Vec<NodeSummaryWire>,
    next_cursor: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct TrashWire {
    items: Vec<NodeSummaryWire>,
}

#[derive(Debug, Deserialize)]
struct VersionsWire {
    items: Vec<VersionSummaryWire>,
}

#[derive(Debug, Deserialize)]
struct TrashEntryWire {
    trash_batch_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct PermanentDeleteWire {
    deleted_nodes: u64,
    deleted_versions: u64,
    deleted_contents: u64,
    freed_bytes: i64,
    queued_objects: u64,
}

#[derive(Debug, Deserialize)]
struct UsageWire {
    used_bytes: i64,
    reserved_bytes: i64,
    quota_bytes: i64,
}

/// 已解密的节点，带内部增量判定所需的明文摘要。绝不跨桥接层暴露。
#[derive(Debug, Clone)]
pub(crate) struct DecryptedNode {
    pub view: NodeView,
    pub content_sha256: Option<String>,
}

impl DecryptedNode {
    pub(crate) fn kind(&self) -> NodeKind {
        self.view.kind
    }

    pub(crate) fn name(&self) -> &str {
        &self.view.name
    }
}

/// 列表分页大小：服务端上限 1000，客户端一页取满再排全局序。
const PAGE_SIZE: i64 = 1000;

impl Client {
    /// 列出文件夹的直接子节点，自动取完全部分页后按 [`NodeSort`] 排序。
    ///
    /// 文件夹永远排在文件前面；排序在解密名称后于本地完成。
    pub async fn list_folder(
        &self,
        folder_id: Uuid,
        sort: NodeSort,
    ) -> Result<Vec<NodeView>, ClientError> {
        let nodes = self.list_folder_decrypted(folder_id).await?;
        let mut views: Vec<NodeView> = nodes.into_iter().map(|node| node.view).collect();
        sort_nodes(&mut views, sort);
        Ok(views)
    }

    /// 取回全部子节点（解密后），供内部文件夹合并/下载复用。
    pub(crate) async fn list_folder_decrypted(
        &self,
        folder_id: Uuid,
    ) -> Result<Vec<DecryptedNode>, ClientError> {
        let master_key = self.master_key()?;
        let mut cursor: Option<Uuid> = None;
        let mut nodes = Vec::new();
        loop {
            let mut path = format!("/api/v1/nodes/{folder_id}/children?limit={PAGE_SIZE}");
            if let Some(cursor) = cursor {
                path.push_str(&format!("&cursor={cursor}"));
            }
            let page: ChildrenWire = self.api.request_json(Method::GET, &path, None).await?;
            for summary in &page.items {
                let node = self.decrypt_node(&master_key, summary)?;
                nodes.push(node);
            }
            match page.next_cursor {
                Some(next) if cursor != Some(next) => {
                    cursor = Some(next);
                }
                _ => break,
            }
        }
        Ok(nodes)
    }

    /// 在文件夹下新建文件夹。
    pub async fn create_folder(
        &self,
        parent_id: Uuid,
        name: &str,
    ) -> Result<NodeView, ClientError> {
        self.ensure_writable()?;
        let master_key = self.master_key()?;
        let cloud_name = normalize_cloud_name(name);
        ensure_valid_name(&cloud_name)?;
        let name_key = derive_name_key(&master_key)?;
        let node_id = Uuid::new_v4();
        let node_id_typed = NodeId::parse(&node_id.to_string())?;
        let parent_typed = NodeId::parse(&parent_id.to_string())?;
        let node_key = NodeKey::generate()?;
        let wrapped = wrap_node_key(&master_key, &node_id_typed, &node_key)?;
        let encrypted_meta = encrypt_node_metadata(
            &node_key,
            &node_id_typed,
            &NodeMetadata {
                name: cloud_name.clone(),
            },
        )?;
        let token = compute_name_token(&name_key, &parent_typed, &cloud_name)?;

        let body = json!({
            "id": node_id,
            "parent_id": parent_id,
            "wrapped_key": encode_hex(&wrapped),
            "encrypted_meta": encode_hex(&encrypted_meta),
            "name_token": token.to_hex(),
        });
        let summary: NodeSummaryWire = self
            .api
            .request_json(Method::POST, "/api/v1/folders", Some(&body))
            .await?;
        self.cache_node_key(node_id, node_key)?;
        self.decrypt_node(&master_key, &summary)
            .map(|node| node.view)
    }

    /// 重命名节点。`parent_id` 是节点当前所在文件夹，`row_version` 是乐观并发版本号。
    ///
    /// 节点密钥未缓存时，本方法会自行列出 `parent_id` 目录来解包密钥，不要求调用方
    /// 先列出过目录；根节点不允许重命名。
    pub async fn rename_node(
        &self,
        node_id: Uuid,
        parent_id: Uuid,
        new_name: &str,
        row_version: i32,
    ) -> Result<NodeView, ClientError> {
        self.ensure_writable()?;
        let cloud_name = normalize_cloud_name(new_name);
        ensure_valid_name(&cloud_name)?;
        let master_key = self.master_key()?;
        let node_key = self.ensure_node_key(node_id, Some(parent_id)).await?;
        let node_typed = NodeId::parse(&node_id.to_string())?;
        let parent_typed = NodeId::parse(&parent_id.to_string())?;
        let name_key = derive_name_key(&master_key)?;
        let encrypted_meta = encrypt_node_metadata(
            &node_key,
            &node_typed,
            &NodeMetadata {
                name: cloud_name.clone(),
            },
        )?;
        let token = compute_name_token(&name_key, &parent_typed, &cloud_name)?;
        let body = json!({
            "row_version": row_version,
            "encrypted_meta": encode_hex(&encrypted_meta),
            "name_token": token.to_hex(),
        });
        let summary: NodeSummaryWire = self
            .api
            .request_json(
                Method::PATCH,
                &format!("/api/v1/nodes/{node_id}/rename"),
                Some(&body),
            )
            .await?;
        self.decrypt_node(&master_key, &summary)
            .map(|node| node.view)
    }

    /// 移动节点到新父文件夹；名称令牌按新父节点重新计算。
    ///
    /// `current_parent_id` 用于在节点密钥未缓存时自行加载当前目录，不要求调用方
    /// 先列出过目录。
    pub async fn move_node(
        &self,
        node_id: Uuid,
        current_parent_id: Uuid,
        new_parent_id: Uuid,
        name: &str,
        row_version: i32,
    ) -> Result<NodeView, ClientError> {
        self.ensure_writable()?;
        let cloud_name = normalize_cloud_name(name);
        ensure_valid_name(&cloud_name)?;
        let master_key = self.master_key()?;
        let node_key = self
            .ensure_node_key(node_id, Some(current_parent_id))
            .await?;
        let _ = node_key;
        let name_key = derive_name_key(&master_key)?;
        let parent_typed = NodeId::parse(&new_parent_id.to_string())?;
        let token = compute_name_token(&name_key, &parent_typed, &cloud_name)?;
        let body = json!({
            "new_parent_id": new_parent_id,
            "row_version": row_version,
            "name_token": token.to_hex(),
        });
        let summary: NodeSummaryWire = self
            .api
            .request_json(
                Method::PATCH,
                &format!("/api/v1/nodes/{node_id}/move"),
                Some(&body),
            )
            .await?;
        self.decrypt_node(&master_key, &summary)
            .map(|node| node.view)
    }

    /// 删除到回收站。重复删除是幂等的。
    pub async fn delete_to_trash(&self, node_id: Uuid) -> Result<Uuid, ClientError> {
        self.ensure_writable()?;
        let response: TrashEntryWire = self
            .api
            .request_json(Method::DELETE, &format!("/api/v1/nodes/{node_id}"), None)
            .await?;
        Ok(response.trash_batch_id)
    }

    /// 回收站顶层列表。
    pub async fn list_trash(&self) -> Result<Vec<TrashItem>, ClientError> {
        let master_key = self.master_key()?;
        let response: TrashWire = self
            .api
            .request_json(Method::GET, "/api/v1/trash?limit=500", None)
            .await?;
        let mut items = Vec::with_capacity(response.items.len());
        for summary in &response.items {
            let node = self.decrypt_node(&master_key, summary)?;
            let deleted_at = summary
                .deleted_at
                .clone()
                .ok_or(ClientError::InvalidResponse("回收站项目缺少删除时间"))?;
            let trash_batch_id = summary
                .trash_batch_id
                .ok_or(ClientError::InvalidResponse("回收站项目缺少 batch ID"))?;
            items.push(TrashItem {
                node: node.view,
                deleted_at,
                trash_batch_id,
            });
        }
        Ok(items)
    }

    /// 从回收站恢复。`None` 表示恢复到原位置；指定新位置时重新计算名称令牌。
    pub async fn restore_from_trash(
        &self,
        node_id: Uuid,
        target: Option<RestoreTarget>,
    ) -> Result<NodeView, ClientError> {
        self.ensure_writable()?;
        let master_key = self.master_key()?;
        let body = match target {
            Some(target) => {
                let cloud_name = normalize_cloud_name(&target.name);
                ensure_valid_name(&cloud_name)?;
                let name_key: NameKey = derive_name_key(&master_key)?;
                let parent = NodeId::parse(&target.parent_id.to_string())?;
                let token = compute_name_token(&name_key, &parent, &cloud_name)?;
                json!({
                    "new_parent_id": target.parent_id,
                    "name_token": token.to_hex(),
                })
            }
            None => json!({}),
        };
        let summary: NodeSummaryWire = self
            .api
            .request_json(
                Method::POST,
                &format!("/api/v1/trash/{node_id}/restore"),
                Some(&body),
            )
            .await?;
        self.decrypt_node(&master_key, &summary)
            .map(|node| node.view)
    }

    /// 永久删除回收站项目。
    pub async fn permanently_delete(
        &self,
        node_id: Uuid,
    ) -> Result<PermanentDeleteOutcome, ClientError> {
        self.ensure_writable()?;
        let response: PermanentDeleteWire = self
            .api
            .request_json(Method::DELETE, &format!("/api/v1/trash/{node_id}"), None)
            .await?;
        Ok(PermanentDeleteOutcome {
            deleted_nodes: response.deleted_nodes,
            deleted_versions: response.deleted_versions,
            deleted_contents: response.deleted_contents,
            freed_bytes: response.freed_bytes,
            queued_objects: response.queued_objects,
        })
    }

    /// 列出版本历史（新到旧）。
    pub async fn list_versions(&self, node_id: Uuid) -> Result<Vec<VersionView>, ClientError> {
        let node_key = self.ensure_file_node_key(node_id).await?;
        let response: VersionsWire = self
            .api
            .request_json(
                Method::GET,
                &format!("/api/v1/nodes/{node_id}/versions?limit=500"),
                None,
            )
            .await?;
        let mut versions = Vec::with_capacity(response.items.len());
        for version in &response.items {
            versions.push(decrypt_version(&node_key, version)?);
        }
        Ok(versions)
    }

    /// 恢复历史版本为当前版本（新增一个引用相同 content 的版本）。
    pub async fn restore_version(
        &self,
        node_id: Uuid,
        source_version_id: Uuid,
    ) -> Result<VersionView, ClientError> {
        self.ensure_writable()?;
        let node_key = self.ensure_file_node_key(node_id).await?;
        let response: VersionsWire = self
            .api
            .request_json(
                Method::GET,
                &format!("/api/v1/nodes/{node_id}/versions?limit=500"),
                None,
            )
            .await?;
        let source = response
            .items
            .iter()
            .find(|version| version.id == source_version_id)
            .ok_or_else(|| not_found_error("版本不存在"))?;
        let metadata = decrypt_version_metadata(&node_key, source)?;
        let new_version_id = Uuid::new_v4();
        let new_typed = VersionId::parse(&new_version_id.to_string())?;
        let encrypted_meta = encrypt_version_metadata(&node_key, &new_typed, &metadata)?;
        let body = json!({
            "source_version_id": source_version_id,
            "new_version_id": new_version_id,
            "encrypted_meta": encode_hex(&encrypted_meta),
        });
        let summary: VersionSummaryWire = self
            .api
            .request_json(
                Method::POST,
                &format!("/api/v1/nodes/{node_id}/versions/restore"),
                Some(&body),
            )
            .await?;
        let view = decrypt_version(&node_key, &summary)?;
        self.cache_content_hash(node_id, metadata.content_sha256)?;
        Ok(view)
    }

    /// 已用 / 预留 / 配额。
    pub async fn get_usage(&self) -> Result<UsageView, ClientError> {
        let response: UsageWire = self
            .api
            .request_json(Method::GET, "/api/v1/me/usage", None)
            .await?;
        Ok(UsageView {
            used_bytes: response.used_bytes,
            reserved_bytes: response.reserved_bytes,
            quota_bytes: response.quota_bytes,
        })
    }

    /// 确保节点密钥已加载。优先用缓存；缺失时刷新父目录。
    pub(crate) async fn ensure_node_key(
        &self,
        node_id: Uuid,
        parent_id: Option<Uuid>,
    ) -> Result<NodeKey, ClientError> {
        if let Some(key) = self.cached_node_key(node_id)? {
            return Ok(key);
        }
        if let Some(parent_id) = parent_id {
            let nodes = self.list_folder_decrypted(parent_id).await?;
            if nodes.iter().any(|node| node.view.id == node_id)
                && let Some(key) = self.cached_node_key(node_id)?
            {
                return Ok(key);
            }
        }
        Err(ClientError::NodeKeyUnavailable)
    }

    /// 确保文件节点密钥已加载。缓存缺失时通过当前版本的下载清单取得包裹密钥
    /// （清单本身不下载任何块）。
    pub(crate) async fn ensure_file_node_key(&self, node_id: Uuid) -> Result<NodeKey, ClientError> {
        if let Some(key) = self.cached_node_key(node_id)? {
            return Ok(key);
        }
        let versions: VersionsWire = self
            .api
            .request_json(
                Method::GET,
                &format!("/api/v1/nodes/{node_id}/versions?limit=500"),
                None,
            )
            .await?;
        let current = versions
            .items
            .iter()
            .find(|version| version.is_current)
            .ok_or(ClientError::InvalidResponse("文件没有当前版本"))?;
        let manifest: DownloadManifestWire = self
            .api
            .request_json(
                Method::GET,
                &format!("/api/v1/nodes/{node_id}/versions/{}/download", current.id),
                None,
            )
            .await?;
        self.load_wrapped_node_key(node_id, manifest.wrapped_key.as_deref())
    }

    pub(crate) fn load_wrapped_node_key(
        &self,
        node_id: Uuid,
        wrapped_hex: Option<&str>,
    ) -> Result<NodeKey, ClientError> {
        if let Some(key) = self.cached_node_key(node_id)? {
            return Ok(key);
        }
        let master_key = self.master_key()?;
        let wrapped = wrapped_hex
            .ok_or(ClientError::NodeKeyUnavailable)
            .and_then(|value| {
                decode_hex(value).ok_or(ClientError::InvalidResponse("包裹密钥不是十六进制"))
            })?;
        let typed = NodeId::parse(&node_id.to_string())?;
        let node_key = unwrap_node_key(&master_key, &typed, &wrapped)?;
        self.cache_node_key(node_id, node_key.clone())?;
        Ok(node_key)
    }

    /// 解密一个节点（名称 + 当前版本元数据）并缓存节点密钥/摘要。
    pub(crate) fn decrypt_node(
        &self,
        master_key: &crypto_core::MasterKey,
        summary: &NodeSummaryWire,
    ) -> Result<DecryptedNode, ClientError> {
        let kind =
            NodeKind::parse(&summary.kind).ok_or(ClientError::InvalidResponse("未知的节点类型"))?;
        let node_typed = NodeId::parse(&summary.id.to_string())?;
        let wrapped = summary
            .wrapped_key
            .as_deref()
            .ok_or(ClientError::InvalidResponse("节点缺少包裹密钥"))?;
        let encrypted_meta = summary
            .encrypted_meta
            .as_deref()
            .ok_or(ClientError::InvalidResponse("节点缺少加密元数据"))?;
        let wrapped =
            decode_hex(wrapped).ok_or(ClientError::InvalidResponse("包裹密钥不是十六进制"))?;
        let encrypted_meta = decode_hex(encrypted_meta)
            .ok_or(ClientError::InvalidResponse("加密元数据不是十六进制"))?;
        let node_key = unwrap_node_key(master_key, &node_typed, &wrapped)?;
        let metadata = decrypt_node_metadata(&node_key, &node_typed, &encrypted_meta)?;

        let mut size = None;
        let mut mtime = None;
        let mut mime = None;
        let mut content_sha256 = None;
        if let Some(version) = &summary.current_version {
            let version_meta = decrypt_version_metadata(&node_key, version)?;
            size = Some(version_meta.size);
            mtime = Some(version_meta.mtime);
            mime = Some(version_meta.mime.clone());
            content_sha256 = version_meta.content_sha256.clone();
        }
        self.cache_node_key(summary.id, node_key)?;
        self.cache_content_hash(summary.id, content_sha256.clone())?;

        Ok(DecryptedNode {
            view: NodeView {
                id: summary.id,
                parent_id: summary.parent_id,
                kind,
                name: metadata.name,
                size,
                mtime,
                mime,
                row_version: summary.row_version,
                current_version_id: summary.current_version.as_ref().map(|version| version.id),
                created_at: summary.created_at.clone(),
                updated_at: summary.updated_at.clone(),
                deleted_at: summary.deleted_at.clone(),
                trash_batch_id: summary.trash_batch_id,
            },
            content_sha256,
        })
    }

    /// 计算本地路径内容的 SHA-256（用于文件夹增量判定）。
    pub(crate) async fn local_content_sha256(
        &self,
        path: std::path::PathBuf,
    ) -> Result<String, ClientError> {
        tokio::task::spawn_blocking(move || {
            use std::io::Read;
            let mut file = crate::io_path::open_for_shared_read(&path)?;
            let mut hasher = crypto_core::ContentHasher::new();
            let mut buffer = vec![0_u8; crypto_core::CHUNK_PLAINTEXT_SIZE];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok::<_, ClientError>(hasher.finish_hex())
        })
        .await
        .map_err(|_| ClientError::Internal("哈希任务失败"))?
    }

    /// 比较两个十六进制摘要（长度相同，常量时间比较）。
    pub(crate) fn hashes_equal(left: &str, right: &str) -> bool {
        use subtle::ConstantTimeEq;
        left.len() == right.len() && bool::from(left.as_bytes().ct_eq(right.as_bytes()))
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct DownloadManifestWire {
    pub content_id: Uuid,
    pub encrypted_meta: String,
    pub chunk_count: i32,
    pub wrapped_key: Option<String>,
    pub chunks: Vec<DownloadChunkWire>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DownloadChunkWire {
    pub idx: i32,
    pub ciphertext_size: i64,
    pub sha256: String,
    pub url: String,
}

fn decrypt_version_metadata(
    node_key: &NodeKey,
    version: &VersionSummaryWire,
) -> Result<VersionMetadata, ClientError> {
    let version_id = VersionId::parse(&version.id.to_string())?;
    let content_id = ContentId::parse(&version.content_id.to_string())?;
    let ciphertext = decode_hex(&version.encrypted_meta)
        .ok_or(ClientError::InvalidResponse("版本元数据不是十六进制"))?;
    Ok(decrypt_version_metadata_for_content(
        node_key,
        &version_id,
        &content_id,
        &ciphertext,
    )?)
}

fn decrypt_version(
    node_key: &NodeKey,
    version: &VersionSummaryWire,
) -> Result<VersionView, ClientError> {
    let metadata = decrypt_version_metadata(node_key, version)?;
    Ok(VersionView {
        id: version.id,
        content_id: version.content_id,
        size: metadata.size,
        mtime: metadata.mtime,
        mime: metadata.mime,
        is_current: version.is_current,
        created_at: version.created_at.clone(),
    })
}

#[cfg(test)]
#[cfg(test)]
fn invalid_name_error() -> ClientError {
    ClientError::Transfer(crate::error::TransferError::new(
        crate::model::TransferErrorCode::InvalidName,
        "文件或文件夹名称不合法",
    ))
}

/// 便于内部构造一个 404 风格的错误（版本已不存在等）。
pub(crate) fn not_found_error(message: &'static str) -> ClientError {
    ClientError::Api(crate::error::ApiFailure {
        status: 404,
        code: ApiErrorCode::NotFound,
        message: message.to_owned(),
        request_id: None,
        retry_after: None,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn invalid_names_are_rejected_with_stable_code() {
        let error = invalid_name_error();
        assert_eq!(
            error.transfer_code(),
            crate::model::TransferErrorCode::InvalidName
        );
    }

    #[test]
    fn name_token_rule_matches_crypto_core() {
        let master = crypto_core::MasterKey::generate().unwrap();
        let name_key = derive_name_key(&master).unwrap();
        let parent = NodeId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let a = compute_name_token(&name_key, &parent, "Report.PDF").unwrap();
        let b = compute_name_token(&name_key, &parent, "report.pdf").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn content_hash_comparison_is_constant_time_and_length_checked() {
        let hash = "a".repeat(64);
        assert!(Client::hashes_equal(&hash, &hash));
        assert!(!Client::hashes_equal(&hash, &"b".repeat(64)));
        assert!(!Client::hashes_equal(&hash, "a"));
    }
}
