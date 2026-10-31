//! 本地文件夹扫描（文件夹上传的输入）。
//!
//! 主线程/调用方不接触任何密钥；扫描只读取本机目录结构与文件元数据。
//! 忽略系统文件、不跟随符号链接，名称不合法或无法读取的项进入跳过列表。

use std::path::{Path, PathBuf};

use crate::ClientError;
use crate::io_path::{
    ensure_valid_name, is_ignored_name, normalize_cloud_name, path_for_io, validate_name,
};
use crate::model::SkippedItem;
use crate::model::TransferErrorCode;
use crate::transfer::transfer_error;

#[derive(Debug, Clone)]
pub(crate) struct FolderLocalFile {
    pub name: String,
    pub path: PathBuf,
    pub relative: String,
    pub size: u64,
    pub mtime_ns: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct FolderLocalTree {
    pub name: String,
    pub folders: Vec<FolderLocalTree>,
    pub files: Vec<FolderLocalFile>,
    pub skipped: Vec<SkippedItem>,
    pub total_files: u64,
    pub total_bytes: u64,
    pub truncated: bool,
}

impl FolderLocalTree {
    /// 深度优先收集全部文件（用于云端同名文件冲突时标记整个子树失败）。
    pub(crate) fn all_files(&self) -> Vec<&FolderLocalFile> {
        let mut files = Vec::new();
        collect_files(self, &mut files);
        files
    }
}

fn collect_files<'a>(tree: &'a FolderLocalTree, output: &mut Vec<&'a FolderLocalFile>) {
    output.extend(tree.files.iter());
    for folder in &tree.folders {
        collect_files(folder, output);
    }
}

/// 扫描一个本地文件夹；超过 `max_files` 时设置 `truncated`，由调用方拒绝开始。
pub(crate) fn scan_local_tree(
    local_dir: &Path,
    max_files: u64,
) -> Result<FolderLocalTree, ClientError> {
    let io_path =
        path_for_io(local_dir).map_err(|_| ClientError::Internal("本地路径不是有效 UTF-8"))?;
    let metadata = std::fs::metadata(&io_path)?;
    if !metadata.is_dir() {
        return Err(transfer_error(
            TransferErrorCode::LocalIo,
            "所选路径不是文件夹",
        ));
    }
    let name = local_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(super::invalid_name)?;
    let name = normalize_cloud_name(name);
    ensure_valid_name(&name)?;

    let mut tree = FolderLocalTree {
        name,
        folders: Vec::new(),
        files: Vec::new(),
        skipped: Vec::new(),
        total_files: 0,
        total_bytes: 0,
        truncated: false,
    };
    let mut count = 0_u64;
    scan_dir(local_dir, "", &mut tree, &mut count, max_files)?;
    Ok(tree)
}

fn scan_dir(
    directory: &Path,
    prefix: &str,
    tree: &mut FolderLocalTree,
    count: &mut u64,
    max_files: u64,
) -> Result<(), ClientError> {
    if tree.truncated {
        return Ok(());
    }
    let io_path =
        path_for_io(directory).map_err(|_| ClientError::Internal("本地路径不是有效 UTF-8"))?;
    let read_dir = match std::fs::read_dir(io_path) {
        Ok(read_dir) => read_dir,
        Err(_) => {
            tree.skipped.push(SkippedItem {
                path: prefix.to_owned(),
                reason: "无法读取该文件夹".to_owned(),
            });
            return Ok(());
        }
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(_) => {
                tree.skipped.push(SkippedItem {
                    path: prefix.to_owned(),
                    reason: "无法读取目录项".to_owned(),
                });
            }
        }
    }
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if tree.truncated {
            return Ok(());
        }
        let name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => {
                tree.skipped.push(SkippedItem {
                    path: join_relative(prefix, &entry.file_name().to_string_lossy()),
                    reason: "名称不是有效 UTF-8".to_owned(),
                });
                continue;
            }
        };
        if is_ignored_name(&name) {
            continue;
        }
        if validate_name(&name).is_some() {
            tree.skipped.push(SkippedItem {
                path: join_relative(prefix, &name),
                reason: "名称不合法".to_owned(),
            });
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => {
                tree.skipped.push(SkippedItem {
                    path: join_relative(prefix, &name),
                    reason: "无法读取文件类型".to_owned(),
                });
                continue;
            }
        };
        if file_type.is_symlink() {
            tree.skipped.push(SkippedItem {
                path: join_relative(prefix, &name),
                reason: "不跟随符号链接".to_owned(),
            });
            continue;
        }
        let cloud_name = normalize_cloud_name(&name);
        ensure_valid_name(&cloud_name)?;
        let path = directory.join(&name);
        let relative = join_relative(prefix, &cloud_name);
        if file_type.is_dir() {
            let mut child = FolderLocalTree {
                name: cloud_name,
                folders: Vec::new(),
                files: Vec::new(),
                skipped: Vec::new(),
                total_files: 0,
                total_bytes: 0,
                truncated: false,
            };
            scan_dir(&path, &relative, &mut child, count, max_files)?;
            tree.skipped.extend(child.skipped.iter().cloned());
            tree.total_files = tree.total_files.saturating_add(child.total_files);
            tree.total_bytes = tree.total_bytes.saturating_add(child.total_bytes);
            tree.truncated |= child.truncated;
            tree.folders.push(child);
        } else if file_type.is_file() {
            if *count >= max_files {
                tree.truncated = true;
                return Ok(());
            }
            let metadata = match std::fs::metadata(
                path_for_io(&path).map_err(|_| ClientError::Internal("本地路径不是有效 UTF-8"))?,
            ) {
                Ok(metadata) => metadata,
                Err(_) => {
                    tree.skipped.push(SkippedItem {
                        path: relative,
                        reason: "无法读取文件元数据".to_owned(),
                    });
                    continue;
                }
            };
            *count = count.saturating_add(1);
            tree.total_files = tree.total_files.saturating_add(1);
            tree.total_bytes = tree.total_bytes.saturating_add(metadata.len());
            tree.files.push(FolderLocalFile {
                name: cloud_name,
                path,
                relative,
                size: metadata.len(),
                mtime_ns: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
                    .unwrap_or_default(),
            });
        }
    }
    Ok(())
}

fn join_relative(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn scan_ignores_system_files_and_recurses() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("photos");
        std::fs::create_dir_all(root.join("2025")).unwrap();
        std::fs::write(root.join("2025/a.jpg"), b"a").unwrap();
        std::fs::write(root.join("2025/b.jpg"), b"bb").unwrap();
        std::fs::write(root.join(".DS_Store"), b"x").unwrap();
        std::fs::write(root.join("Thumbs.db"), b"x").unwrap();

        let tree = scan_local_tree(&root, 100).unwrap();
        assert_eq!(tree.total_files, 2);
        assert_eq!(tree.total_bytes, 3);
        assert_eq!(tree.folders.len(), 1);
        assert_eq!(tree.folders[0].name, "2025");
        assert_eq!(tree.folders[0].files.len(), 2);
    }

    #[test]
    fn scan_truncates_above_limit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("photos");
        std::fs::create_dir_all(&root).unwrap();
        for index in 0..4 {
            std::fs::write(root.join(format!("{index}.bin")), b"x").unwrap();
        }
        let tree = scan_local_tree(&root, 2).unwrap();
        assert!(tree.truncated);
        assert_eq!(tree.total_files, 2);
    }

    #[test]
    fn nfd_names_are_normalized_to_nfc_for_the_cloud() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("photos");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("cafe\u{301}.txt"), b"x").unwrap();
        let tree = scan_local_tree(&root, 100).unwrap();
        assert_eq!(tree.files[0].name, "caf\u{e9}.txt");
    }

    #[test]
    fn all_files_collects_nested_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("photos");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/c.txt"), b"c").unwrap();
        let tree = scan_local_tree(&root, 100).unwrap();
        let files = tree.all_files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative, "a/b/c.txt");
    }
}
