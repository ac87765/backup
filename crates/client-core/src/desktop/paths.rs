//! 路径比较工具（备份引擎与微信数据根排除共用）。
//!
//! 比较键统一 `/`、NFC；Windows/macOS 文件系统通常大小写不敏感，因此这两个
//! 平台还做小写化。Linux 保留大小写（区分大小写的文件系统）。这样普通文件夹
//! 冲突检查与“排除微信数据根”使用同一套规范化规则。

use std::path::Path;

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
