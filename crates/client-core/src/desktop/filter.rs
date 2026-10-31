//! 备份扫描与排除规则（design D6）。
//!
//! - 默认排除系统/临时文件：`.DS_Store`、`Thumbs.db`、`desktop.ini`、`._*`、
//!   `~$*`、`.~lock.*#`、`*.tmp`、`*.part`、`*.crdownload`；
//! - 用户 glob 规则由 `globset`（ripgrep 的 glob 引擎）编译，大小写不敏感，
//!   既匹配相对路径也匹配文件名；
//! - 排除子文件夹按规范化后的相对目录前缀匹配；
//! - 隐藏文件/目录默认不备份（以 `.` 开头，Windows 另外识别 HIDDEN 属性）；
//! - 不跟随符号链接；`.lnk` 是普通文件（内容是快捷方式描述），备份它不会
//!   解析或跟随其目标，因此按普通文件备份，避免用户丢失这个文件本身。
//! - 名称统一 NFC 后再比较/上传（与名称令牌规则一致）。

#[cfg(test)]
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::error::ClientError;
use crate::io_path::{FileFingerprint, is_ignored_name, open_for_shared_read, path_for_io};
use crate::model::normalize_folder_name;

use super::error::BackupError;

/// 临时文件扩展名（大小写不敏感）。
const TEMP_EXTENSIONS: [&str; 3] = ["tmp", "part", "crdownload"];

/// 编译后的排除规则。
#[derive(Debug, Clone)]
pub(crate) struct Exclusions {
    globs: GlobSet,
    subfolders: Vec<String>,
}

impl Exclusions {
    /// 编译用户规则；非法 glob 返回配置错误（不 panic）。
    pub(crate) fn compile(globs: &[String], subfolders: &[String]) -> Result<Self, BackupError> {
        let mut builder = GlobSetBuilder::new();
        for pattern in globs {
            let trimmed = pattern.trim();
            if trimmed.is_empty() {
                continue;
            }
            let glob = GlobBuilder::new(trimmed)
                .case_insensitive(true)
                .literal_separator(false)
                .build()
                .map_err(|_| BackupError::InvalidConfig("排除规则不是合法的 glob"))?;
            builder.add(glob);
        }
        let globs = builder
            .build()
            .map_err(|_| BackupError::InvalidConfig("排除规则无法编译"))?;
        let subfolders = subfolders
            .iter()
            .map(|folder| normalize_relative_key(folder))
            .filter(|folder| !folder.is_empty())
            .collect();
        Ok(Self { globs, subfolders })
    }

    /// 路径是否命中用户 glob（相对路径与文件名都尝试，行为与常见备份工具一致）。
    pub(crate) fn matches(&self, relative: &str, name: &str) -> bool {
        self.globs.is_match(relative) || self.globs.is_match(name)
    }

    /// 相对目录是否在排除子文件夹内（含其所有后代）。
    pub(crate) fn excludes_directory(&self, relative_key: &str) -> bool {
        self.subfolders.iter().any(|folder| {
            relative_key == folder
                || relative_key
                    .strip_prefix(folder.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    }
}

/// 规范化相对路径键：NFC + 小写 + `/` 分隔、去掉首尾 `/`。
pub(crate) fn normalize_relative_key(value: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    value
        .replace('\\', "/")
        .trim_matches('/')
        .nfc()
        .collect::<String>()
        .to_lowercase()
}

fn join_relative(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}

/// 扫描选项。
pub(crate) struct WalkOptions<'a> {
    pub include_hidden: bool,
    pub exclusions: &'a Exclusions,
}

/// 一个本地文件；`relative` 已是 NFC 且用 `/` 分隔。
#[derive(Debug, Clone)]
pub(crate) struct LocalFile {
    pub name: String,
    pub path: PathBuf,
    pub relative: String,
    pub size: u64,
    pub mtime_ns: i64,
}

/// 本地目录树（结构保留，便于逐层与云端合并）。
#[derive(Debug, Clone)]
pub(crate) struct LocalTree {
    pub name: String,
    /// 相对备份根目录的路径（根为 `""`，NFC、`/` 分隔）。
    pub relative: String,
    pub dirs: Vec<LocalTree>,
    pub files: Vec<LocalFile>,
    pub unreadable: u64,
    pub excluded: u64,
}

impl LocalTree {
    /// 文件数与明文总字节数。
    pub(crate) fn counts(&self) -> (u64, u64) {
        let mut files = 0_u64;
        let mut bytes = 0_u64;
        for file in &self.files {
            files = files.saturating_add(1);
            bytes = bytes.saturating_add(file.size);
        }
        for dir in &self.dirs {
            let (child_files, child_bytes) = dir.counts();
            files = files.saturating_add(child_files);
            bytes = bytes.saturating_add(child_bytes);
        }
        (files, bytes)
    }

    /// 全部文件大小（用于预检按文件逐块计算密文开销）。
    pub(crate) fn collect_sizes(&self, output: &mut Vec<u64>) {
        for file in &self.files {
            output.push(file.size);
        }
        for dir in &self.dirs {
            dir.collect_sizes(output);
        }
    }

    /// 全部相对路径（用于清理本地已删除文件的索引；测试与诊断使用）。
    #[cfg(test)]
    pub(crate) fn collect_relatives(&self, output: &mut HashSet<String>) {
        for file in &self.files {
            output.insert(file.relative.clone());
        }
        for dir in &self.dirs {
            dir.collect_relatives(output);
        }
    }
}

/// 扫描一个备份根目录。
pub(crate) fn scan_tree(
    root: &Path,
    name: &str,
    options: &WalkOptions<'_>,
) -> Result<LocalTree, BackupError> {
    let io_root = path_for_io(root)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "路径编码无效"))
        .map_err(ClientError::from)?;
    let metadata = std::fs::metadata(&io_root).map_err(ClientError::from)?;
    if !metadata.is_dir() {
        return Err(BackupError::InvalidConfig("备份路径不是文件夹"));
    }
    let mut tree = LocalTree {
        name: name.to_owned(),
        relative: String::new(),
        dirs: Vec::new(),
        files: Vec::new(),
        unreadable: 0,
        excluded: 0,
    };
    scan_dir(root, "", &mut tree, options)?;
    Ok(tree)
}

fn scan_dir(
    directory: &Path,
    prefix: &str,
    tree: &mut LocalTree,
    options: &WalkOptions<'_>,
) -> Result<(), BackupError> {
    let io_path = match path_for_io(directory) {
        Ok(path) => path,
        Err(_) => {
            tree.unreadable = tree.unreadable.saturating_add(1);
            return Ok(());
        }
    };
    let read_dir = match std::fs::read_dir(&io_path) {
        Ok(read_dir) => read_dir,
        Err(_) => {
            tree.unreadable = tree.unreadable.saturating_add(1);
            return Ok(());
        }
    };
    let mut entries = Vec::new();
    for entry in read_dir {
        match entry {
            Ok(entry) => entries.push(entry),
            Err(_) => tree.unreadable = tree.unreadable.saturating_add(1),
        }
    }
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let raw_name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => {
                tree.unreadable = tree.unreadable.saturating_add(1);
                continue;
            }
        };
        let name = crate::io_path::normalize_cloud_name(&raw_name);
        let relative = join_relative(prefix, &name);
        if is_ignored_name(&name) || is_temp_name(&name) {
            tree.excluded = tree.excluded.saturating_add(1);
            continue;
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => {
                tree.unreadable = tree.unreadable.saturating_add(1);
                continue;
            }
        };
        let metadata = entry.metadata();
        if !options.include_hidden && is_hidden(&name, metadata.as_ref().ok()) {
            tree.excluded = tree.excluded.saturating_add(1);
            continue;
        }
        if file_type.is_symlink() {
            // 不跟随符号链接（也包含 macOS 的 alias 解析结果）.
            tree.excluded = tree.excluded.saturating_add(1);
            continue;
        }
        if options.exclusions.matches(&relative, &name) {
            tree.excluded = tree.excluded.saturating_add(1);
            continue;
        }
        let path = directory.join(&raw_name);
        if file_type.is_dir() {
            if options
                .exclusions
                .excludes_directory(&normalize_relative_key(&relative))
            {
                tree.excluded = tree.excluded.saturating_add(1);
                continue;
            }
            let mut child = LocalTree {
                name: name.clone(),
                relative: relative.clone(),
                dirs: Vec::new(),
                files: Vec::new(),
                unreadable: 0,
                excluded: 0,
            };
            scan_dir(&path, &relative, &mut child, options)?;
            tree.unreadable = tree.unreadable.saturating_add(child.unreadable);
            tree.excluded = tree.excluded.saturating_add(child.excluded);
            tree.dirs.push(child);
        } else if file_type.is_file() {
            let metadata = match metadata {
                Ok(metadata) => metadata,
                Err(_) => {
                    tree.unreadable = tree.unreadable.saturating_add(1);
                    continue;
                }
            };
            tree.files.push(LocalFile {
                name,
                path,
                relative,
                size: metadata.len(),
                mtime_ns: mtime_ns(&metadata),
            });
        } else {
            // 设备文件、socket、FIFO 等不是普通文件，跳过。
            tree.excluded = tree.excluded.saturating_add(1);
        }
    }
    Ok(())
}

fn is_temp_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| TEMP_EXTENSIONS.contains(&extension.as_str()))
}

/// 隐藏：以 `.` 开头；Windows 上另外识别 HIDDEN 文件属性。
fn is_hidden(name: &str, metadata: Option<&std::fs::Metadata>) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if let Some(metadata) = metadata
            && metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
        {
            return true;
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = metadata;
    }
    false
}

pub(crate) fn mtime_ns(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or_default()
}

/// 稳定哈希：读取前后大小/修改时间必须一致，否则返回 `None`（稍后重试）。
pub(crate) fn hash_file_stable(
    path: &Path,
    expected: FileFingerprint,
) -> Result<Option<String>, ClientError> {
    use std::io::Read;

    let before = FileFingerprint::of_path(path)?;
    if before.changed_from(expected) {
        return Ok(None);
    }
    let mut file = open_for_shared_read(path)?;
    let mut hasher = crypto_core::ContentHasher::new();
    let mut buffer = vec![0_u8; crypto_core::CHUNK_PLAINTEXT_SIZE];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    drop(file);
    let after = FileFingerprint::of_path(path)?;
    if after.changed_from(expected) {
        return Ok(None);
    }
    Ok(Some(hasher.finish_hex()))
}

/// 名称比较键：NFC + 小写（与名称令牌规则一致）。
pub(crate) fn name_key(name: &str) -> String {
    normalize_folder_name(name)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn options(exclusions: &Exclusions) -> WalkOptions<'_> {
        WalkOptions {
            include_hidden: false,
            exclusions,
        }
    }

    #[test]
    fn default_rules_exclude_system_and_temp_files() {
        let exclusions = Exclusions::compile(&[], &[]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for name in [
            ".DS_Store",
            "Thumbs.db",
            "desktop.ini",
            "._resource",
            "~$report.docx",
            ".~lock.report.docx#",
            "a.tmp",
            "a.PART",
            "a.crdownload",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::write(dir.path().join("keep.txt"), b"keep").unwrap();
        let tree = scan_tree(dir.path(), "test", &options(&exclusions)).unwrap();
        assert_eq!(tree.files.len(), 1);
        assert_eq!(tree.files[0].name, "keep.txt");
    }

    #[test]
    fn user_globs_and_subfolder_exclusions_apply() {
        let exclusions =
            Exclusions::compile(&["*.psd".to_owned()], &["skip/me".to_owned()]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("skip/me")).unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("a.psd"), b"x").unwrap();
        std::fs::write(dir.path().join("nested/b.psd"), b"x").unwrap();
        std::fs::write(dir.path().join("skip/me/c.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("keep.txt"), b"x").unwrap();
        let tree = scan_tree(dir.path(), "test", &options(&exclusions)).unwrap();
        let mut files = HashSet::new();
        tree.collect_relatives(&mut files);
        assert!(files.contains("keep.txt"));
        assert!(!files.contains("a.psd"));
        assert!(!files.contains("nested/b.psd"));
        assert!(!files.contains("skip/me/c.txt"));
    }

    #[test]
    fn hidden_files_are_excluded_by_default_and_can_be_enabled() {
        let exclusions = Exclusions::compile(&[], &[]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), b"x").unwrap();
        std::fs::write(dir.path().join("visible.txt"), b"x").unwrap();
        let hidden = scan_tree(dir.path(), "test", &options(&exclusions)).unwrap();
        assert_eq!(hidden.files.len(), 1);
        let mut options = options(&exclusions);
        options.include_hidden = true;
        let all = scan_tree(dir.path(), "test", &options).unwrap();
        assert_eq!(all.files.len(), 2);
    }

    #[test]
    fn nfd_names_are_normalized_to_nfc() {
        let exclusions = Exclusions::compile(&[], &[]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cafe\u{301}.txt"), b"x").unwrap();
        let tree = scan_tree(dir.path(), "test", &options(&exclusions)).unwrap();
        assert_eq!(tree.files[0].name, "caf\u{e9}.txt");
        assert_eq!(tree.files[0].relative, "caf\u{e9}.txt");
    }

    #[test]
    fn stable_hash_detects_changed_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"abc").unwrap();
        let fingerprint = FileFingerprint::of_path(&path).unwrap();
        assert!(hash_file_stable(&path, fingerprint).unwrap().is_some());
        std::fs::write(&path, b"abcd").unwrap();
        assert!(
            hash_file_stable(&path, fingerprint).unwrap().is_none(),
            "文件在哈希前变化必须放弃本次"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let exclusions = Exclusions::compile(&[], &[]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("target.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt"))
            .unwrap();
        let tree = scan_tree(dir.path(), "test", &options(&exclusions)).unwrap();
        let mut files = HashSet::new();
        tree.collect_relatives(&mut files);
        assert!(files.contains("target.txt"));
        assert!(!files.contains("link.txt"));
    }
}
