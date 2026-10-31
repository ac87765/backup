//! 微信来源注册的纯计算（design D2）。
//!
//! 输入是状态库中的“账号 × 类别”勾选，输出应该是哪些来源备份项。每个类别的
//! 每个来源路径一项（例如“图片与视频”有 `msg/attach` 与 `msg/video`）。聊天
//! 记录数据库不注册为普通备份项（第 3 组快照）。
//!
//! 同一路径只保留一项：3.x 的“手机迁移备份”位于数据根目录的 `Backup/`，
//! 同一数据根下的多个账号可能指向同一路径。勾选按（数据根、账号、类别）排序，
//! 先出现的账号作为该来源的展示归属，云端路径因此稳定。

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::io_path::{normalize_cloud_name, path_for_io};
use crate::state::WechatSelectionRecord;

use super::super::paths::comparable_path;
use super::layout::{WechatCategory, WechatVersion, layout_for};

/// 一个应存在的微信来源备份项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesiredWechatSource {
    pub data_root: PathBuf,
    pub account_name: String,
    pub version: WechatVersion,
    pub category: WechatCategory,
    /// 相对数据根目录的路径（`/` 分隔）。
    pub relative_path: String,
    pub local_path: PathBuf,
    pub local_name: String,
    pub exclude_subfolders: Vec<String>,
    /// 注册时来源路径是否存在；不存在的可选来源（如空的手机迁移备份）跳过。
    pub exists: bool,
}

/// 根据全部勾选计算应注册的来源（按路径去重、顺序确定）。
pub(crate) fn desired_sources(selections: &[WechatSelectionRecord]) -> Vec<DesiredWechatSource> {
    let mut desired: BTreeMap<String, DesiredWechatSource> = BTreeMap::new();
    for selection in selections {
        if !selection.selected {
            continue;
        }
        let Some(version) = WechatVersion::parse(&selection.version) else {
            continue;
        };
        let Some(category) = WechatCategory::parse(&selection.category) else {
            continue;
        };
        if !category.is_file_source() {
            continue;
        }
        let layout = layout_for(version);
        let Some(category_layout) = layout.category(category) else {
            continue;
        };
        let data_root = PathBuf::from(&selection.data_root);
        let mut relatives: Vec<String> = Vec::new();
        for relative in category_layout.account_paths {
            relatives.push(format!("{}/{}", selection.account_name, relative));
        }
        for relative in category_layout.data_root_paths {
            relatives.push(relative.replace("{account}", &selection.account_name));
        }
        for relative in relatives {
            let Some(last) = relative.rsplit('/').find(|part| !part.is_empty()) else {
                continue;
            };
            let local_name = normalize_cloud_name(last);
            if local_name.is_empty() {
                continue;
            }
            let local_path = data_root.join(&relative);
            let key = comparable_path(&local_path);
            if key.is_empty() || desired.contains_key(&key) {
                continue;
            }
            desired.insert(
                key,
                DesiredWechatSource {
                    data_root: data_root.clone(),
                    account_name: selection.account_name.clone(),
                    version,
                    category,
                    relative_path: relative,
                    local_path: local_path.clone(),
                    local_name,
                    exclude_subfolders: layout
                        .cache_directories
                        .iter()
                        .map(|directory| (*directory).to_owned())
                        .collect(),
                    exists: path_for_io(&local_path).is_ok_and(|path| path.is_dir()),
                },
            );
        }
    }
    desired.into_values().collect()
}

/// 账号目录名是否可安全用作云端目录名（与本地备注名校验一致）。
pub(crate) fn validate_account_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use uuid::Uuid;

    use super::*;

    fn selection(
        data_root: &std::path::Path,
        account: &str,
        category: WechatCategory,
        selected: bool,
    ) -> WechatSelectionRecord {
        WechatSelectionRecord {
            user_id: Uuid::new_v4(),
            data_root: data_root.to_string_lossy().to_string(),
            account_name: account.to_owned(),
            version: "wechat3".to_owned(),
            category: category.as_str().to_owned(),
            selected,
            updated_at: 0,
        }
    }

    #[test]
    fn shared_phone_migration_path_is_registered_once_and_databases_is_not_a_source() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("Backup")).unwrap();
        std::fs::create_dir_all(root.join("wxid_a/FileStorage/File")).unwrap();
        std::fs::create_dir_all(root.join("wxid_b/FileStorage/File")).unwrap();

        let selections = vec![
            selection(root, "wxid_a", WechatCategory::ChatFiles, true),
            selection(root, "wxid_a", WechatCategory::Databases, true),
            selection(root, "wxid_a", WechatCategory::PhoneMigration, true),
            selection(root, "wxid_b", WechatCategory::PhoneMigration, true),
        ];
        let sources = desired_sources(&selections);

        // 3.x 的迁移备份位于数据根的 Backup/：两个账号指向同一路径，只注册一次。
        let backups: Vec<&DesiredWechatSource> = sources
            .iter()
            .filter(|source| source.category == WechatCategory::PhoneMigration)
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].relative_path, "Backup");
        assert_eq!(backups[0].account_name, "wxid_a", "归属顺序必须确定");

        // 数据库类别走第 3 组快照，不注册为普通备份项。
        assert!(
            sources
                .iter()
                .all(|source| source.category != WechatCategory::Databases)
        );
        assert!(
            sources
                .iter()
                .any(|source| source.category == WechatCategory::ChatFiles)
        );
    }
}
