//! 本地文件系统辅助：Windows 长路径、共享读、临时文件原子落盘与名称处理。
//!
//! - 读取用户文件时在 Windows 上用共享读（允许其他程序同时读写/删除），
//!   避免“文件被占用”导致备份失败；其他平台使用普通只读打开。
//! - 超过 260 字符的 Windows 路径统一加 `\\?\`（UNC 加 `\\?\UNC\`）前缀。
//! - 下载先写同目录临时文件，完整性校验通过后再重命名；任何失败都由
//!   [`TempFileGuard`] 删除临时文件，不留下部分明文。
//! - 云端名称落盘前做 Windows 非法字符、保留设备名与长度处理。

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use uuid::Uuid;

/// 本地状态库/长路径转换错误（只在内部使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StatePathError {
    #[error("路径不是有效 UTF-8")]
    InvalidUtf8,
}

/// Windows 路径段上限（UTF-16 code unit）。
#[cfg(target_os = "windows")]
const WINDOWS_SEGMENT_LIMIT: usize = 255;
/// 超过该长度就启用 `\\?\` 前缀（留出余量）。
#[cfg(target_os = "windows")]
const WINDOWS_LONG_PATH_THRESHOLD: usize = 240;

/// 转换为适合文件系统调用的路径。
///
/// - Windows：相对路径先转绝对路径；超过阈值或含超长组件时加 `\\?\`（UNC
///   使用 `\\?\UNC\`）。前缀一旦存在不重复添加。
/// - 其他平台：原样返回。
pub(crate) fn path_for_io(path: &Path) -> Result<PathBuf, StatePathError> {
    #[cfg(not(target_os = "windows"))]
    {
        Ok(path.to_path_buf())
    }

    #[cfg(target_os = "windows")]
    {
        let text = path.to_string_lossy();
        if text.starts_with(r"\\?\") {
            return Ok(path.to_path_buf());
        }
        let needs_prefix = text.len() >= WINDOWS_LONG_PATH_THRESHOLD
            || path.components().any(|component| {
                component
                    .as_os_str()
                    .to_string_lossy()
                    .encode_utf16()
                    .count()
                    > WINDOWS_SEGMENT_LIMIT
            });
        if !needs_prefix {
            return Ok(path.to_path_buf());
        }
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|_| StatePathError::InvalidUtf8)?
                .join(path)
        };
        let absolute = absolute.to_string_lossy();
        if let Some(rest) = absolute.strip_prefix(r"\\") {
            Ok(PathBuf::from(format!(r"\\?\UNC\{rest}")))
        } else {
            Ok(PathBuf::from(format!(r"\\?\{absolute}")))
        }
    }
}

/// 共享读打开（Windows 允许其他进程同时读写/删除）。
pub(crate) fn open_for_shared_read(path: &Path) -> io::Result<File> {
    let path = path_for_io(path)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "路径编码无效"))?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    options.open(path)
}

/// 文件指纹：大小 + 修改时间（纳秒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileFingerprint {
    pub size: u64,
    pub mtime_ns: i64,
}

impl FileFingerprint {
    /// 从共享读句柄读取，避免 stat 与后续读取之间文件被替换。
    pub(crate) fn from_file(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        Ok(Self {
            size: metadata.len(),
            mtime_ns: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
                .unwrap_or_default(),
        })
    }

    pub(crate) fn of_path(path: &Path) -> io::Result<Self> {
        let file = open_for_shared_read(path)?;
        Self::from_file(&file)
    }

    /// 两次读取之间文件是否被修改（大小或修改时间任一变化都算）。
    pub(crate) fn changed_from(self, other: Self) -> bool {
        self.size != other.size || self.mtime_ns != other.mtime_ns
    }
}

/// 读取一个明文分块。调用方必须先校验文件指纹；返回的缓冲区长度必须等于
/// 该块应有的明文长度。
pub(crate) fn read_chunk(
    path: &Path,
    index: u32,
    chunk_size: usize,
    total_size: u64,
) -> io::Result<Vec<u8>> {
    let start = u64::from(index)
        .checked_mul(chunk_size as u64)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "块偏移溢出"))?;
    if start > total_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "块序号超出文件大小",
        ));
    }
    let remaining = total_size - start;
    let expected = remaining.min(chunk_size as u64) as usize;
    let mut file = open_for_shared_read(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut buffer = vec![0_u8; expected];
    file.read_exact(&mut buffer)?;
    Ok(buffer)
}

/// 删除临时文件的 RAII 守卫。
pub(crate) struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// 把临时文件重命名到目标位置，并解除守卫。
    pub(crate) fn persist(mut self, destination: &Path) -> io::Result<()> {
        let from = path_for_io(&self.path)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "临时文件路径编码无效"))?;
        let to = path_for_io(destination)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "目标路径编码无效"))?;
        std::fs::rename(from, to)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// 目标文件占位守卫：下载失败时删除占位文件，避免在目标目录留下空文件。
pub(crate) struct DestinationGuard {
    path: PathBuf,
    armed: bool,
}

impl DestinationGuard {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    /// `Replace` 策略：目标可能已存在，下载失败时绝不能删除本地旧文件，
    /// 因此这个守卫只提供路径信息、不做清理。
    pub(crate) fn existing(path: PathBuf) -> Self {
        Self { path, armed: false }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn commit(&mut self) {
        self.armed = false;
    }
}

impl Drop for DestinationGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// 在目标文件同目录创建唯一临时文件（`.<name>.<uuid>.part`）。
pub(crate) fn create_temp_in_dir(directory: &Path, file_name: &str) -> io::Result<TempFileGuard> {
    let directory = path_for_io(directory)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "目录路径编码无效"))?;
    std::fs::create_dir_all(&directory)?;
    let prefix = if file_name.is_empty() {
        "download".to_owned()
    } else {
        file_name.to_owned()
    };
    for _ in 0..16 {
        let candidate = directory.join(format!(".{prefix}.{}.part", Uuid::new_v4().simple()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                drop(file);
                return Ok(TempFileGuard::new(candidate));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "无法创建唯一临时文件",
    ))
}

/// 按冲突策略选择一个可写目标路径。
///
/// - `KeepBoth`：保留两份，自动追加 ` (1)`、` (2)`…；
/// - `Replace`：返回原路径（重命名会覆盖）；
/// - `Skip`：目标已存在时返回 `None`。
///
/// `KeepBoth` 与 `Skip` 会用 `create_new` 预占目标，避免并发下载时互相覆盖；
/// 返回的 [`DestinationGuard`] 在失败时删除占位文件。
pub(crate) fn choose_destination(
    directory: &Path,
    file_name: &str,
    policy: crate::model::ConflictPolicy,
) -> io::Result<Option<DestinationGuard>> {
    use crate::model::ConflictPolicy;

    let directory = path_for_io(directory)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "目录路径编码无效"))?;
    let base = sanitize_local_name(file_name);

    match policy {
        ConflictPolicy::Replace => Ok(Some(DestinationGuard::existing(directory.join(base)))),
        ConflictPolicy::Skip => {
            let path = directory.join(&base);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    drop(file);
                    Ok(Some(DestinationGuard::new(path)))
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
                Err(error) => Err(error),
            }
        }
        ConflictPolicy::KeepBoth => {
            for index in 0..10_000_u32 {
                let candidate = if index == 0 {
                    base.clone()
                } else {
                    numbered_name(&base, index)
                };
                let path = directory.join(candidate);
                match OpenOptions::new().write(true).create_new(true).open(&path) {
                    Ok(file) => {
                        drop(file);
                        return Ok(Some(DestinationGuard::new(path)));
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "同名文件过多，无法生成唯一名称",
            ))
        }
    }
}

fn numbered_name(base: &str, index: u32) -> String {
    match base.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => {
            format!("{stem} ({index}).{extension}")
        }
        _ => format!("{base} ({index})"),
    }
}

/// 云端名称落盘前的 Windows 安全化规则：
///
/// 1. `< > : " / \ | ? *` 与所有 C0 控制字符替换为 `_`；
/// 2. 去掉结尾的空格与点（Windows 会静默截断）；
/// 3. `.`/`..`/空名称改为 `_`；
/// 4. `CON`、`PRN`、`AUX`、`NUL`、`COM1`–`COM9`、`LPT1`–`LPT9` 等保留设备名
///    在主干后加 `_`（即使带扩展名，如 `CON.txt` → `CON_.txt`）；
/// 5. 超过 240 个 UTF-16 单元的组件截断主干并保留扩展名，避免触发 MAX_PATH。
#[must_use]
pub fn sanitize_local_name(name: &str) -> String {
    let mut output = String::with_capacity(name.len());
    for character in name.chars() {
        if character == '\0' || character.is_control() {
            output.push('_');
            continue;
        }
        if matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        ) {
            output.push('_');
        } else {
            output.push(character);
        }
    }

    while output.ends_with(' ') || output.ends_with('.') {
        output.pop();
    }
    if output.is_empty() || output == "." || output == ".." {
        output.push('_');
    }

    output = protect_reserved_device_name(&output);
    truncate_component(&output, 240)
}

fn protect_reserved_device_name(name: &str) -> String {
    let upper = name.to_uppercase();
    let stem = upper.split('.').next().unwrap_or_default().trim_end();
    let reserved = matches!(stem, "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit()
            && stem.as_bytes()[3] != b'0');
    if !reserved {
        return name.to_owned();
    }

    match name.find('.') {
        Some(index) => {
            let mut protected = String::new();
            protected.push_str(&name[..index]);
            protected.push('_');
            protected.push_str(&name[index..]);
            protected
        }
        None => format!("{name}_"),
    }
}

fn truncate_component(name: &str, max_utf16: usize) -> String {
    if name.encode_utf16().count() <= max_utf16 {
        return name.to_owned();
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !extension.is_empty() && extension.len() <= 16 => {
            (stem, Some(extension))
        }
        _ => (name, None),
    };
    let suffix_len = extension
        .as_ref()
        .map(|extension| extension.encode_utf16().count() + 1)
        .unwrap_or_default();
    let budget = max_utf16.saturating_sub(suffix_len).max(1);
    let mut stem_out = String::new();
    let mut used = 0;
    for character in stem.chars() {
        let width = character.len_utf16();
        if used + width > budget {
            break;
        }
        stem_out.push(character);
        used += width;
    }
    if stem_out.is_empty() {
        stem_out.push('_');
    }
    match extension {
        Some(extension) => format!("{stem_out}.{extension}"),
        None => stem_out,
    }
}

/// Web 与客户端共同的文件名规则（服务端只能看到名称令牌，因此必须客户端校验）。
///
/// 返回错误文案；合法时返回 `None`。
#[must_use]
pub fn validate_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("名称不能为空");
    }
    if name.trim().is_empty() {
        return Some("名称不能只包含空白字符");
    }
    if name.contains('/') {
        return Some("名称不能包含 \"/\"");
    }
    if name.contains('\0') {
        return Some("名称不能包含 NUL 字符");
    }
    if name == "." || name == ".." {
        return Some("名称不能是 \".\" 或 \"..\"");
    }
    if name.len() > 255 {
        return Some("名称不能超过 255 个 UTF-8 字节");
    }
    None
}

/// 云端名称统一为 NFC（macOS 本地文件系统常给 NFD）。
#[must_use]
pub(crate) fn normalize_cloud_name(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfc().collect()
}

/// 校验名称；不合法时返回稳定的 `invalid_name` 传输错误。
pub(crate) fn ensure_valid_name(name: &str) -> Result<(), crate::error::ClientError> {
    match validate_name(name) {
        None => Ok(()),
        Some(message) => Err(crate::error::ClientError::Transfer(
            crate::error::TransferError::new(crate::model::TransferErrorCode::InvalidName, message),
        )),
    }
}

/// 文件夹上传时忽略的系统/临时文件（与 folder-transfer spec 一致）。
#[must_use]
pub(crate) fn is_ignored_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    if matches!(lower.as_str(), ".ds_store" | "thumbs.db" | "desktop.ini") {
        return true;
    }
    if lower.starts_with("._") || lower.starts_with("~$") {
        return true;
    }
    lower.starts_with(".~lock.") && lower.ends_with('#')
}

/// 根据文件名猜测 MIME；未知时用 `application/octet-stream`。
///
/// 不引入额外 MIME 数据库依赖：这里只覆盖常见类型，和 Web 的
/// `File.type || application/octet-stream` 语义一致（未知类型可为空串，
/// 但客户端统一用 octet-stream，便于服务端/下载端展示）。
#[must_use]
pub(crate) fn derive_mime(name: &str) -> String {
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mime = match extension.as_str() {
        "txt" | "log" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "json" => "application/json",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    };
    mime.to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::model::ConflictPolicy;

    #[test]
    fn sanitizes_windows_invalid_characters_and_reserved_names() {
        assert_eq!(
            sanitize_local_name("a<b>c:d\"e/f\\g|h?i*j"),
            "a_b_c_d_e_f_g_h_i_j"
        );
        assert_eq!(sanitize_local_name("报告.pdf"), "报告.pdf");
        assert_eq!(sanitize_local_name("CON"), "CON_");
        assert_eq!(sanitize_local_name("CON.txt"), "CON_.txt");
        assert_eq!(sanitize_local_name("lpt9.tar.gz"), "lpt9_.tar.gz");
        assert_eq!(sanitize_local_name("name. "), "name");
        assert_eq!(sanitize_local_name(".."), "_");
        assert_eq!(sanitize_local_name(""), "_");
        assert_eq!(sanitize_local_name("bad\u{0}name"), "bad_name");
    }

    #[test]
    fn truncates_long_components_preserving_extension() {
        let name = format!("{}.pdf", "长".repeat(300));
        let sanitized = sanitize_local_name(&name);
        assert!(sanitized.encode_utf16().count() <= 240);
        assert!(sanitized.ends_with(".pdf"));
    }

    #[test]
    fn validates_names_like_web() {
        assert!(validate_name("").is_some());
        assert!(validate_name("   ").is_some());
        assert!(validate_name("a/b").is_some());
        assert!(validate_name(".").is_some());
        assert!(validate_name("..").is_some());
        assert!(validate_name(&"a".repeat(256)).is_some());
        assert!(validate_name("合同.pdf").is_none());
    }

    #[test]
    fn ignore_rules_match_folder_transfer_spec() {
        for ignored in [
            ".DS_Store",
            "Thumbs.db",
            "desktop.ini",
            "._resource",
            "~$report.docx",
            ".~lock.report.docx#",
        ] {
            assert!(is_ignored_name(ignored), "{ignored}");
        }
        assert!(!is_ignored_name("report.docx"));
    }

    #[test]
    fn keep_both_uses_numbered_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = choose_destination(dir.path(), "photo.jpg", ConflictPolicy::KeepBoth)
            .unwrap()
            .unwrap();
        first.commit();

        let second = choose_destination(dir.path(), "photo.jpg", ConflictPolicy::KeepBoth)
            .unwrap()
            .unwrap();
        assert!(second.path().ends_with("photo (1).jpg"));
        let mut second = second;
        second.commit();
    }

    #[test]
    fn skip_reports_existing_and_replace_uses_original() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"local").unwrap();
        assert!(
            choose_destination(dir.path(), "a.txt", ConflictPolicy::Skip)
                .unwrap()
                .is_none()
        );
        let replace = choose_destination(dir.path(), "a.txt", ConflictPolicy::Replace)
            .unwrap()
            .unwrap();
        assert!(replace.path().ends_with("a.txt"));
    }

    #[test]
    fn destination_guard_removes_uncommitted_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let guard = choose_destination(dir.path(), "a.txt", ConflictPolicy::Skip)
            .unwrap()
            .unwrap();
        let path = guard.path().to_path_buf();
        assert!(path.exists());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn temp_guard_removes_file_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let guard = create_temp_in_dir(dir.path(), "a.txt").unwrap();
        let path = guard.path().to_path_buf();
        assert!(path.exists());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn reads_chunks_with_shared_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.bin");
        std::fs::write(&path, b"abcdefgh").unwrap();
        assert_eq!(read_chunk(&path, 0, 4, 8).unwrap(), b"abcd");
        assert_eq!(read_chunk(&path, 1, 4, 8).unwrap(), b"efgh");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn long_paths_get_verbatim_prefix() {
        let path = PathBuf::from(format!("C:\\{}\\file.txt", "a".repeat(300)));
        let converted = path_for_io(&path).unwrap();
        assert!(converted.to_string_lossy().starts_with(r"\\?\C:\"));
    }
}
