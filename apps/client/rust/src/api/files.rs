//! 文件管理 API：列表、新建、重命名、移动、回收站、版本、用量。

use client_core::NodeSort;

use crate::api::error::BridgeError;
use crate::api::types::{
    NodeDto, NodeSortDto, PermanentDeleteOutcomeDto, RestoreTargetDto, TrashItemDto, UsageDto,
    VersionDto,
};
use crate::convert::{parse_uuid, restore_target};
use crate::state;

/// 列出文件夹的直接子节点（文件夹在前，按 `sort` 排序）。
pub async fn list_folder(
    folder_id: String,
    sort: NodeSortDto,
) -> Result<Vec<NodeDto>, BridgeError> {
    let app = state::app()?;
    let folder_id = parse_uuid(&folder_id, "文件夹")?;
    let nodes = app
        .client
        .list_folder(folder_id, NodeSort::from(sort))
        .await?;
    Ok(nodes.iter().map(NodeDto::from).collect())
}

/// 新建文件夹。
pub async fn create_folder(parent_id: String, name: String) -> Result<NodeDto, BridgeError> {
    let app = state::app()?;
    let parent_id = parse_uuid(&parent_id, "文件夹")?;
    let node = app.client.create_folder(parent_id, &name).await?;
    Ok((&node).into())
}

/// 重命名节点；`row_version` 是列表返回的乐观并发版本号。
pub async fn rename_node(
    node_id: String,
    parent_id: String,
    new_name: String,
    row_version: i32,
) -> Result<NodeDto, BridgeError> {
    let app = state::app()?;
    let node_id = parse_uuid(&node_id, "节点")?;
    let parent_id = parse_uuid(&parent_id, "文件夹")?;
    let node = app
        .client
        .rename_node(node_id, parent_id, &new_name, row_version)
        .await?;
    Ok((&node).into())
}

/// 移动节点到新父文件夹。
pub async fn move_node(
    node_id: String,
    current_parent_id: String,
    new_parent_id: String,
    name: String,
    row_version: i32,
) -> Result<NodeDto, BridgeError> {
    let app = state::app()?;
    let node_id = parse_uuid(&node_id, "节点")?;
    let current_parent_id = parse_uuid(&current_parent_id, "当前文件夹")?;
    let new_parent_id = parse_uuid(&new_parent_id, "目标文件夹")?;
    let node = app
        .client
        .move_node(
            node_id,
            current_parent_id,
            new_parent_id,
            &name,
            row_version,
        )
        .await?;
    Ok((&node).into())
}

/// 删除到回收站；返回回收站批次 ID。
pub async fn delete_to_trash(node_id: String) -> Result<String, BridgeError> {
    let app = state::app()?;
    let node_id = parse_uuid(&node_id, "节点")?;
    Ok(app.client.delete_to_trash(node_id).await?.to_string())
}

/// 回收站顶层列表。
pub async fn list_trash() -> Result<Vec<TrashItemDto>, BridgeError> {
    let app = state::app()?;
    let items = app.client.list_trash().await?;
    Ok(items.iter().map(TrashItemDto::from).collect())
}

/// 从回收站恢复；`target` 为 `None` 时恢复原位置。
pub async fn restore_from_trash(
    node_id: String,
    target: Option<RestoreTargetDto>,
) -> Result<NodeDto, BridgeError> {
    let app = state::app()?;
    let node_id = parse_uuid(&node_id, "节点")?;
    let target = restore_target(target)?;
    let node = app.client.restore_from_trash(node_id, target).await?;
    Ok((&node).into())
}

/// 永久删除回收站项目。
pub async fn permanently_delete(node_id: String) -> Result<PermanentDeleteOutcomeDto, BridgeError> {
    let app = state::app()?;
    let node_id = parse_uuid(&node_id, "节点")?;
    let outcome = app.client.permanently_delete(node_id).await?;
    Ok((&outcome).into())
}

/// 版本历史（新到旧）。
pub async fn list_versions(node_id: String) -> Result<Vec<VersionDto>, BridgeError> {
    let app = state::app()?;
    let node_id = parse_uuid(&node_id, "节点")?;
    let versions = app.client.list_versions(node_id).await?;
    Ok(versions.iter().map(VersionDto::from).collect())
}

/// 恢复历史版本为当前版本。
pub async fn restore_version(
    node_id: String,
    source_version_id: String,
) -> Result<VersionDto, BridgeError> {
    let app = state::app()?;
    let node_id = parse_uuid(&node_id, "节点")?;
    let source_version_id = parse_uuid(&source_version_id, "版本")?;
    let version = app
        .client
        .restore_version(node_id, source_version_id)
        .await?;
    Ok((&version).into())
}

/// 已用 / 预留 / 配额。
pub async fn get_usage() -> Result<UsageDto, BridgeError> {
    let app = state::app()?;
    let usage = app.client.get_usage().await?;
    Ok((&usage).into())
}
