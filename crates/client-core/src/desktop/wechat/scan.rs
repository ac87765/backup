//! 只读扫描与位置解析（design D1/D2）。
//!
//! 所有文件系统操作只读取目录项与元数据（`read_dir` / `symlink_metadata`），
//! 不打开任何文件内容；4.x `.ini` 的唯一例外见 [`read_first_line`]。扫描过程
//! 不写微信目录，也不跟随符号链接。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::io_path::path_for_io;

use super::layout::{
    WECHAT_3_LAYOUT, WECHAT_4_LAYOUT, WINDOWS_LAYOUTS, WechatCategoryLayout, WechatLayout,
};
use super::system::WechatSystemLocations;
use super::{WechatAccount, WechatCategory, WechatCategoryInfo, WechatError, WechatSource};

/// 位置发现与目录遍历的阻塞实现；只由 [`super::WechatLocator::discover`] 在
/// `spawn_blocking` 线程中调用。
pub(crate) fn discover_blocking(
    system: &dyn WechatSystemLocations,
    manual: &[PathBuf],
) -> Result<Vec<WechatAccount>, WechatError> {
    let mut state = DiscoveryState::default();
    for path in manual {
        discover_manual(path, &mut state);
    }
    discover_system(system, &mut state);

    let mut accounts = state.accounts;
    mark_current_login_candidate(&mut accounts);
    Ok(accounts)
}

/// 只收集数据根目录（不统计账号内文件），供普通备份文件夹排除微信数据。
pub(crate) fn discover_roots_blocking(
    system: &dyn WechatSystemLocations,
    manual: &[PathBuf],
) -> Result<Vec<PathBuf>, WechatError> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();
    for path in manual {
        collect_manual_roots(path, &mut roots, &mut seen);
    }

    let documents = system.documents_dir().ok().flatten();
    if let Ok(Some(value)) = system.wechat_3_file_save_path()
        && let Some(root) = registry_root(&value, documents.as_deref())
    {
        push_root(root, &mut roots, &mut seen);
    }
    if let Ok(Some(appdata)) = system.appdata_dir()
        && let Some(root) = config_root(&appdata)
    {
        push_root(root, &mut roots, &mut seen);
    }
    if let Some(documents) = documents {
        for layout in WINDOWS_LAYOUTS {
            let root = documents.join(layout.data_root_name);
            if is_directory(&root) {
                push_root(root, &mut roots, &mut seen);
            }
        }
    }
    Ok(roots)
}

fn collect_manual_roots(path: &Path, roots: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    if !is_directory(path) {
        return;
    }
    if detect_account_layout(path).is_some() {
        if let Some(data_root) = path.parent() {
            push_root(data_root.to_path_buf(), roots, seen);
        }
        return;
    }
    let mut candidates = Vec::new();
    for layout in WINDOWS_LAYOUTS {
        if path_name_eq(path, layout.data_root_name) {
            candidates.push(path.to_path_buf());
        }
        let child = path.join(layout.data_root_name);
        if is_directory(&child) {
            candidates.push(child);
        }
        if contains_account_directory(path, layout) {
            candidates.push(path.to_path_buf());
        }
    }
    for candidate in candidates {
        push_root(candidate, roots, seen);
    }
}

fn push_root(root: PathBuf, roots: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    if is_directory(&root) && seen.insert(path_key(&root)) {
        roots.push(root);
    }
}

#[derive(Default)]
struct DiscoveryState {
    seen_roots: HashSet<PathBuf>,
    seen_accounts: HashSet<PathBuf>,
    accounts: Vec<WechatAccount>,
}

fn discover_manual(path: &Path, state: &mut DiscoveryState) {
    if !is_directory(path) {
        return;
    }

    if let Some(layout) = detect_account_layout(path) {
        if let Some(data_root) = path.parent() {
            add_account(path, data_root, layout, WechatSource::Manual, state);
        }
        return;
    }

    let mut roots: Vec<(&'static WechatLayout, PathBuf)> = Vec::new();
    for layout in WINDOWS_LAYOUTS {
        if path_name_eq(path, layout.data_root_name) {
            roots.push((layout, path.to_path_buf()));
        }
        let child = path.join(layout.data_root_name);
        if is_directory(&child) {
            roots.push((layout, child));
        }
        // 容忍用户选择了一个名字不标准、但直接包含账号目录的根目录。
        if contains_account_directory(path, layout) {
            roots.push((layout, path.to_path_buf()));
        }
    }
    for (layout, root) in roots {
        scan_root(&root, layout, WechatSource::Manual, state);
    }
}

fn discover_system(system: &dyn WechatSystemLocations, state: &mut DiscoveryState) {
    let documents = system.documents_dir().ok().flatten();

    if let Ok(Some(value)) = system.wechat_3_file_save_path()
        && let Some(root) = registry_root(&value, documents.as_deref())
    {
        scan_root(&root, &WECHAT_3_LAYOUT, WechatSource::Registry, state);
    }

    if let Ok(Some(appdata)) = system.appdata_dir()
        && let Some(root) = config_root(&appdata)
    {
        scan_root(&root, &WECHAT_4_LAYOUT, WechatSource::ConfigFile, state);
    }

    if let Some(documents) = documents {
        for layout in WINDOWS_LAYOUTS {
            let root = documents.join(layout.data_root_name);
            if is_directory(&root) {
                scan_root(&root, layout, WechatSource::DefaultLocation, state);
            }
        }
    }
}

fn scan_root(
    root: &Path,
    layout: &'static WechatLayout,
    source: WechatSource,
    state: &mut DiscoveryState,
) {
    if !is_directory(root) || !state.seen_roots.insert(path_key(root)) {
        return;
    }
    let Ok(io_root) = path_for_io(root) else {
        return;
    };
    let Ok(entries) = fs::read_dir(io_root) else {
        return;
    };

    let mut children = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        if layout.excludes_directory_name(name_text) {
            continue;
        }
        children.push(name);
    }
    children.sort();

    for name in children {
        let account_dir = root.join(&name);
        if is_account_directory(&account_dir, layout) {
            add_account(&account_dir, root, layout, source, state);
        }
    }
}

fn add_account(
    account_dir: &Path,
    data_root: &Path,
    layout: &'static WechatLayout,
    source: WechatSource,
    state: &mut DiscoveryState,
) {
    if !is_account_directory(account_dir, layout) {
        return;
    }
    if !state.seen_accounts.insert(path_key(account_dir)) {
        return;
    }
    if let Some(account) = scan_account(account_dir, data_root, layout, source) {
        state.accounts.push(account);
    }
}

fn scan_account(
    account_dir: &Path,
    data_root: &Path,
    layout: &'static WechatLayout,
    source: WechatSource,
) -> Option<WechatAccount> {
    let account_name = account_dir.file_name()?.to_str()?.to_owned();
    let mut categories = Vec::with_capacity(layout.categories.len());
    let mut total_size_bytes = 0_u64;
    let mut total_file_count = 0_u64;
    let mut last_modified_ns = None;

    for category_layout in layout.categories {
        let paths = category_paths(account_dir, data_root, &account_name, category_layout);
        let mut stats = ScanStats::default();
        let mut exists = false;
        for path in &paths {
            let Ok(io_path) = path_for_io(path) else {
                continue;
            };
            let Ok(metadata) = fs::symlink_metadata(io_path) else {
                continue;
            };
            exists = true;
            scan_entry(path, &metadata, &mut stats);
        }

        total_size_bytes = total_size_bytes.saturating_add(stats.size_bytes);
        total_file_count = total_file_count.saturating_add(stats.file_count);
        last_modified_ns = max_option(last_modified_ns, stats.last_modified_ns);
        categories.push(WechatCategoryInfo {
            category: category_layout.category,
            paths,
            size_bytes: stats.size_bytes,
            file_count: stats.file_count,
            last_modified_ns: stats.last_modified_ns,
            exists,
        });
    }

    let has_phone_migration_backup = categories
        .iter()
        .find(|info| info.category == WechatCategory::PhoneMigration)
        .is_some_and(|info| info.file_count > 0);

    Some(WechatAccount {
        account_name,
        version: layout.version,
        data_root: data_root.to_path_buf(),
        account_dir: account_dir.to_path_buf(),
        source,
        categories,
        total_size_bytes,
        total_file_count,
        last_modified_ns,
        is_current_login_candidate: false,
        has_phone_migration_backup,
    })
}

fn category_paths(
    account_dir: &Path,
    data_root: &Path,
    account_name: &str,
    layout: &WechatCategoryLayout,
) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(layout.account_paths.len() + layout.data_root_paths.len());
    for relative in layout.account_paths {
        paths.push(account_dir.join(relative));
    }
    for relative in layout.data_root_paths {
        paths.push(data_root.join(relative.replace("{account}", account_name)));
    }
    paths
}

fn is_account_directory(directory: &Path, layout: &WechatLayout) -> bool {
    layout
        .account_markers
        .iter()
        .all(|marker| has_subdirectory(directory, marker))
}

fn contains_account_directory(directory: &Path, layout: &WechatLayout) -> bool {
    let Ok(io_dir) = path_for_io(directory) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(io_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        if is_account_directory(&directory.join(entry.file_name()), layout) {
            return true;
        }
    }
    false
}

/// 大小写不敏感地检查子目录（账号特征目录）。
fn has_subdirectory(directory: &Path, name: &str) -> bool {
    let Ok(io_dir) = path_for_io(directory) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(io_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            continue;
        }
        if entry
            .file_name()
            .to_str()
            .is_some_and(|child| child.eq_ignore_ascii_case(name))
        {
            return true;
        }
    }
    false
}

fn detect_account_layout(directory: &Path) -> Option<&'static WechatLayout> {
    WINDOWS_LAYOUTS
        .iter()
        .copied()
        .find(|layout| is_account_directory(directory, layout))
}

fn path_name_eq(path: &Path, name: &str) -> bool {
    path.file_name()
        .is_some_and(|file_name| file_name.to_string_lossy().eq_ignore_ascii_case(name))
}

/// 目录存在性检查走 `path_for_io`，与扫描一样支持 Windows 长路径。
fn is_directory(path: &Path) -> bool {
    path_for_io(path).is_ok_and(|io_path| io_path.is_dir())
}

/// 3.x 注册表值解析：`MyDocument:` 表示“文档”，否则按绝对路径处理。
fn registry_root(value: &str, documents: Option<&Path>) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let base =
        if value.eq_ignore_ascii_case("MyDocument:") || value.eq_ignore_ascii_case("MyDocument") {
            documents?.to_path_buf()
        } else {
            PathBuf::from(value)
        };
    if !base.is_absolute() {
        return None;
    }
    root_under(&base, WECHAT_3_LAYOUT.data_root_name)
}

/// 4.x 配置值解析：内容应是 `xwechat_files` 的父目录。
fn config_root(appdata: &Path) -> Option<PathBuf> {
    let config_dir = appdata.join("Tencent").join("xwechat").join("config");
    let entries = fs::read_dir(path_for_io(&config_dir).ok()?).ok()?;

    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let is_ini = Path::new(&name)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ini"));
        if is_ini && entry.file_type().is_ok_and(|file_type| file_type.is_file()) {
            files.push((name, entry.path()));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));

    for (_, path) in files {
        let Some(value) = read_first_line(&path) else {
            continue;
        };
        if !Path::new(&value).is_absolute() {
            continue;
        }
        if let Some(root) = root_under(&PathBuf::from(value), WECHAT_4_LAYOUT.data_root_name) {
            return Some(root);
        }
    }
    None
}

fn root_under(base: &Path, root_name: &str) -> Option<PathBuf> {
    if !is_directory(base) {
        return None;
    }
    if path_name_eq(base, root_name) {
        return Some(base.to_path_buf());
    }
    let root = base.join(root_name);
    is_directory(&root).then_some(root)
}

/// 只读取 `.ini` 的第一行；去掉 UTF-8 BOM 与首尾空白。
/// 最多读取 4096 字节，避免异常超大文件。
fn read_first_line(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader, Read};

    let file = crate::io_path::open_for_shared_read(path).ok()?;
    let mut line = Vec::new();
    BufReader::new(file)
        .take(4096)
        .read_until(b'\n', &mut line)
        .ok()?;
    if line.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(&line);
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() || text.contains('\0') {
        return None;
    }
    Some(text.to_owned())
}

#[derive(Default)]
struct ScanStats {
    size_bytes: u64,
    file_count: u64,
    last_modified_ns: Option<i64>,
}

fn scan_entry(path: &Path, metadata: &fs::Metadata, stats: &mut ScanStats) {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return;
    }
    if file_type.is_file() {
        add_file(metadata, stats);
        return;
    }
    if !file_type.is_dir() {
        return;
    }

    let Ok(io_path) = path_for_io(path) else {
        return;
    };
    let Ok(entries) = fs::read_dir(io_path) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry
            .file_type()
            .is_ok_and(|file_type| !file_type.is_symlink())
        {
            continue;
        }
        let child_path = path.join(entry.file_name());
        let Ok(io_child) = path_for_io(&child_path) else {
            continue;
        };
        let Ok(child_metadata) = fs::symlink_metadata(io_child) else {
            continue;
        };
        scan_entry(&child_path, &child_metadata, stats);
    }
}

fn add_file(metadata: &fs::Metadata, stats: &mut ScanStats) {
    stats.file_count = stats.file_count.saturating_add(1);
    stats.size_bytes = stats.size_bytes.saturating_add(metadata.len());
    stats.last_modified_ns = max_option(stats.last_modified_ns, mtime_ns(metadata));
}

fn mtime_ns(metadata: &fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
}

fn max_option(current: Option<i64>, candidate: Option<i64>) -> Option<i64> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.max(candidate)),
        (Some(current), None) => Some(current),
        (None, candidate) => candidate,
    }
}

fn mark_current_login_candidate(accounts: &mut [WechatAccount]) {
    accounts.sort_by(|left, right| {
        right
            .last_modified_ns
            .cmp(&left.last_modified_ns)
            .then_with(|| left.account_name.cmp(&right.account_name))
    });

    let mut marked = false;
    for account in accounts.iter_mut() {
        if !marked && account.last_modified_ns.is_some() {
            account.is_current_login_candidate = true;
            marked = true;
        } else {
            account.is_current_login_candidate = false;
        }
    }
}

/// 去重键：先规范化，再在 Windows 上按大小写不敏感比较。
fn path_key(path: &Path) -> PathBuf {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    let canonical = PathBuf::from(canonical.to_string_lossy().to_lowercase());
    canonical
}
