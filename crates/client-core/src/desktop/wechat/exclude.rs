//! 普通备份文件夹自动排除微信数据根（design D2，2026-11-01 审查者决定）。
//!
//! 微信 4.x 的数据就在“文档\xwechat_files”，首次引导又默认勾选“文档”。因此
//! 普通备份文件夹扫描时**始终**跳过 D1 发现到的微信数据根子树（无论“微信
//! 备份”是否开启），避免在微信运行时上传不一致的数据库副本、重复上传数 GB
//! 图片。微信来源本身不受影响（它们就在数据根内部）。
//!
//! 数据根本身的大小只在预检时统计（界面提示“已排除微信数据”及其大小）；
//! 统计只读取目录项与元数据，不打开文件内容。

use std::path::{Path, PathBuf};

use crate::io_path::path_for_io;

use super::super::paths::{comparable_path, path_within};

/// 一次扫描中微信数据根的排除结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WechatRootExclusions {
    /// 位于被扫描文件夹内部的微信数据根，相对于该文件夹的路径键。
    /// 直接作为“排除子文件夹”传给扫描器，跳过整个子树。
    pub relative_subfolders: Vec<String>,
    /// 需要统计大小的数据根（预检提示用）。
    pub measured_roots: Vec<PathBuf>,
    /// 被扫描文件夹本身位于某个微信数据根内部（整个文件夹都应排除）。
    pub scanned_root_inside_wechat: bool,
}

/// 计算某个普通备份文件夹应排除的微信数据根。
pub(crate) fn exclusions_for(folder: &Path, data_roots: &[PathBuf]) -> WechatRootExclusions {
    let folder_text = comparable_path(folder);
    let mut result = WechatRootExclusions::default();
    if folder_text.is_empty() {
        return result;
    }
    for root in data_roots {
        let root_text = comparable_path(root);
        if root_text.is_empty() {
            continue;
        }
        if root_text == folder_text {
            result.scanned_root_inside_wechat = true;
            continue;
        }
        if path_within(&folder_text, &root_text) {
            // 被扫描文件夹在微信数据根内部：全部内容都由“微信备份”处理。
            result.scanned_root_inside_wechat = true;
            continue;
        }
        if !path_within(&root_text, &folder_text) {
            continue;
        }
        let Some(rest) = root_text.strip_prefix(folder_text.as_str()) else {
            continue;
        };
        let relative = rest.trim_start_matches('/').to_owned();
        if relative.is_empty() {
            result.scanned_root_inside_wechat = true;
            continue;
        }
        result.relative_subfolders.push(relative);
        result.measured_roots.push(root.clone());
    }
    result.relative_subfolders.sort();
    result.relative_subfolders.dedup();
    result.measured_roots.sort();
    result.measured_roots.dedup();
    result
}

/// 统计若干目录树中普通文件的数量与总大小（不跟随符号链接，不读取内容）。
pub(crate) fn measure_trees(roots: &[PathBuf]) -> (u64, u64) {
    let mut files = 0_u64;
    let mut bytes = 0_u64;
    for root in roots {
        measure_dir(root, &mut files, &mut bytes);
    }
    (files, bytes)
}

fn measure_dir(directory: &Path, files: &mut u64, bytes: &mut u64) {
    let Ok(io_directory) = path_for_io(directory) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(io_directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = directory.join(entry.file_name());
        if file_type.is_dir() {
            measure_dir(&path, files, bytes);
        } else if file_type.is_file()
            && let Ok(metadata) = entry.metadata()
        {
            *files = files.saturating_add(1);
            *bytes = bytes.saturating_add(metadata.len());
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn nested_data_root_is_replaced_by_a_relative_exclusion() {
        let folder = Path::new("/data/Documents");
        let root = PathBuf::from("/data/Documents/xwechat_files");
        let exclusions = exclusions_for(folder, &[root]);
        assert!(!exclusions.scanned_root_inside_wechat);
        assert_eq!(exclusions.relative_subfolders, vec!["xwechat_files"]);
        assert_eq!(
            exclusions.measured_roots,
            vec![PathBuf::from("/data/Documents/xwechat_files")]
        );
    }

    #[test]
    fn folder_inside_a_data_root_is_excluded_entirely() {
        let folder = Path::new("/data/Documents/xwechat_files/wxid_test0001/msg/file");
        let root = PathBuf::from("/data/Documents/xwechat_files");
        let exclusions = exclusions_for(folder, &[root]);
        assert!(exclusions.scanned_root_inside_wechat);
        assert!(exclusions.relative_subfolders.is_empty());
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    #[test]
    fn comparison_uses_the_engine_normalization() {
        // Windows/macOS 大小写不敏感：路径大小写不同也必须识别为同一数据根。
        let folder = Path::new("/Data/Documents");
        let root = PathBuf::from("/data/documents/XWECHAT_FILES");
        let exclusions = exclusions_for(folder, &[root]);
        assert_eq!(exclusions.relative_subfolders, vec!["xwechat_files"]);
    }
}
