//! 路径比较工具（备份引擎与微信数据根排除共用）。
//!
//! 比较键统一 `/`、NFC；Windows/macOS 文件系统通常大小写不敏感，因此这两个
//! 平台还做小写化。Linux 保留大小写（区分大小写的文件系统）。这样普通文件夹
//! 冲突检查与“排除微信数据根”使用同一套规范化规则。

use std::path::Path;

use uuid::Uuid;

use crate::Client;
use crate::error::{ApiErrorCode, ClientError};
use crate::model::{NodeKind, TransferErrorCode};
use crate::transfer::transfer_error;

use super::filter::name_key;

pub(crate) fn comparable_path(path: &Path) -> String {
    use unicode_normalization::UnicodeNormalization;
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.as_str().nfc().collect::<String>();
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let text = text.to_lowercase();
    let trimmed = text.trim_end_matches('/').to_owned();
    if trimmed.is_empty() { text } else { trimmed }
}

/// `child` 是否等于 `parent` 或位于其下（两者都必须是比较键）。
pub(crate) fn path_within(child: &str, parent: &str) -> bool {
    child == parent
        || child
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// 逐层查找或创建云端文件夹；`name_conflict` 竞态时重新列出复用。
///
/// 普通备份、微信来源与数据库快照共用这一实现，保证同名文件夹的合并规则一致。
pub(crate) async fn ensure_cloud_folder(
    client: &Client,
    parent_id: Uuid,
    name: &str,
) -> Result<Uuid, ClientError> {
    let key = name_key(name);
    let nodes = client.list_folder_decrypted(parent_id).await?;
    if let Some(node) = nodes.iter().find(|node| name_key(node.name()) == key) {
        return match node.kind() {
            NodeKind::Folder => Ok(node.view.id),
            NodeKind::File => Err(transfer_error(
                TransferErrorCode::TargetUnavailable,
                "云端同一位置已有同名文件",
            )),
        };
    }
    match client.create_folder(parent_id, name).await {
        Ok(folder) => Ok(folder.id),
        Err(error) if error.api_code() == Some(&ApiErrorCode::NameConflict) => {
            let nodes = client.list_folder_decrypted(parent_id).await?;
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
