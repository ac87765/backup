//! 数据驱动的微信目录布局描述（design D1/D2）。
//!
//! 微信的目录结构不是公开接口。本文件把“根目录名、账号目录特征、非账号目录、
//! 类别 → 相对路径、缓存排除目录”集中成普通 Rust 常量；微信改版时只改这里，
//! 并补对应夹具。macOS 条目在第 5 组实现前保持为空（`MACOS_LAYOUTS`），
//! 但结构已经为 macOS 3.x / 4.x 预留。
//!
//! 相对路径一律用 `/` 分隔；4.x 的手机迁移备份位于数据根目录下，用
//! `{account}` 代表账号目录名。

/// 微信数据布局版本。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WechatVersion {
    Wechat3,
    Wechat4,
}

impl WechatVersion {
    /// 界面展示用的版本标签。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Wechat3 => "3.x",
            Self::Wechat4 => "4.x",
        }
    }
}

/// 备份类别（design D2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WechatCategory {
    ChatFiles,
    ImagesAndVideos,
    Databases,
    PhoneMigration,
}

impl WechatCategory {
    /// 界面展示用的类别名称。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ChatFiles => "聊天文件",
            Self::ImagesAndVideos => "图片与视频",
            Self::Databases => "聊天记录数据库",
            Self::PhoneMigration => "手机迁移备份",
        }
    }
}

/// 一个类别在某个版本布局中的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WechatCategoryLayout {
    pub category: WechatCategory,
    /// 相对于账号目录的路径。
    pub account_paths: &'static [&'static str],
    /// 相对于数据根目录的路径；可包含 `{account}` 占位符。
    pub data_root_paths: &'static [&'static str],
}

/// 一个微信版本的数据布局描述。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WechatLayout {
    pub version: WechatVersion,
    /// 数据根目录名（位于“文档”或被微信记录的自定义位置下）。
    pub data_root_name: &'static str,
    /// 账号目录必须同时存在的子目录。
    pub account_markers: &'static [&'static str],
    /// 非账号目录（大小写不敏感）。
    pub excluded_directories: &'static [&'static str],
    /// 缓存/临时目录（大小写不敏感；备份整个账号目录时排除）。
    pub cache_directories: &'static [&'static str],
    /// 类别 → 相对路径。
    pub categories: &'static [WechatCategoryLayout],
}

impl WechatLayout {
    /// 目录名是否属于需要排除的非账号目录（大小写不敏感）。
    #[must_use]
    pub fn excludes_directory_name(&self, name: &str) -> bool {
        self.excluded_directories
            .iter()
            .any(|excluded| name.eq_ignore_ascii_case(excluded))
    }

    /// 查找某个类别的布局；数据驱动，不在代码里硬编码版本判断。
    #[must_use]
    pub fn category(&self, category: WechatCategory) -> Option<&'static WechatCategoryLayout> {
        self.categories
            .iter()
            .find(|layout| layout.category == category)
    }
}

/// Windows 微信 3.x 布局（design Context / D2）。
pub const WECHAT_3_LAYOUT: WechatLayout = WechatLayout {
    version: WechatVersion::Wechat3,
    data_root_name: "WeChat Files",
    account_markers: &["FileStorage", "Msg"],
    excluded_directories: &["All Users", "all_users", "Backup", "Applet", "WMPF"],
    cache_directories: &["Cache", "temp"],
    categories: &[
        WechatCategoryLayout {
            category: WechatCategory::ChatFiles,
            account_paths: &["FileStorage/File"],
            data_root_paths: &[],
        },
        WechatCategoryLayout {
            category: WechatCategory::ImagesAndVideos,
            account_paths: &["FileStorage/Image", "FileStorage/Video"],
            data_root_paths: &[],
        },
        WechatCategoryLayout {
            category: WechatCategory::Databases,
            account_paths: &["Msg"],
            data_root_paths: &[],
        },
        WechatCategoryLayout {
            category: WechatCategory::PhoneMigration,
            account_paths: &[],
            // design D2：3.x 的迁移备份在数据根目录的 Backup/ 下。
            data_root_paths: &["Backup"],
        },
    ],
};

/// Windows 微信 4.x 布局（design Context / D2）。
pub const WECHAT_4_LAYOUT: WechatLayout = WechatLayout {
    version: WechatVersion::Wechat4,
    data_root_name: "xwechat_files",
    account_markers: &["msg", "db_storage"],
    excluded_directories: &["All Users", "all_users", "Backup", "Applet", "WMPF"],
    cache_directories: &["cache", "temp", "apm_record", "resource", "business/xweb"],
    categories: &[
        WechatCategoryLayout {
            category: WechatCategory::ChatFiles,
            account_paths: &["msg/file"],
            data_root_paths: &[],
        },
        WechatCategoryLayout {
            category: WechatCategory::ImagesAndVideos,
            account_paths: &["msg/attach", "msg/video"],
            data_root_paths: &[],
        },
        WechatCategoryLayout {
            category: WechatCategory::Databases,
            account_paths: &["db_storage"],
            data_root_paths: &[],
        },
        WechatCategoryLayout {
            category: WechatCategory::PhoneMigration,
            account_paths: &[],
            // design D2：4.x 的迁移备份按账号放在数据根目录的 Backup/<账号目录名>/。
            data_root_paths: &["Backup/{account}"],
        },
    ],
};

/// Windows 当前启用的布局（3.x 在前，4.x 在后）。
pub const WINDOWS_LAYOUTS: [&WechatLayout; 2] = [&WECHAT_3_LAYOUT, &WECHAT_4_LAYOUT];

/// macOS 布局（第 5 组实现）。
///
/// 先保持为空：macOS 3.x 在 `Library/Application Support/com.tencent.xinWeChat/`
/// 的容器内，4.x 在 `Documents/xwechat_files/`；两者都需要容器路径与权限引导，
/// 不属于第 1 组范围。结构已经预留，第 5 组只需在此补充条目。
pub const MACOS_LAYOUTS: [&WechatLayout; 0] = [];

/// 按版本取布局。
#[must_use]
pub fn layout_for(version: WechatVersion) -> &'static WechatLayout {
    match version {
        WechatVersion::Wechat3 => &WECHAT_3_LAYOUT,
        WechatVersion::Wechat4 => &WECHAT_4_LAYOUT,
    }
}
