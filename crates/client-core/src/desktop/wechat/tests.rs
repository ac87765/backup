#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

//! 夹具测试：不读取、不提交任何真实微信数据。所有目录名与文件都是虚构值。

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tempfile::TempDir;

use super::{
    MACOS_LAYOUTS, WECHAT_3_LAYOUT, WECHAT_4_LAYOUT, WINDOWS_LAYOUTS, WechatAccountNotes,
    WechatCategory, WechatError, WechatLocator, WechatSource, WechatSystemLocations, WechatVersion,
    layout_for,
};
use crate::state::StateStore;

#[derive(Default)]
struct FakeLocations {
    documents: Option<PathBuf>,
    appdata: Option<PathBuf>,
    registry: Option<String>,
}

impl WechatSystemLocations for FakeLocations {
    fn documents_dir(&self) -> Result<Option<PathBuf>, WechatError> {
        Ok(self.documents.clone())
    }

    fn appdata_dir(&self) -> Result<Option<PathBuf>, WechatError> {
        Ok(self.appdata.clone())
    }

    fn wechat_3_file_save_path(&self) -> Result<Option<String>, WechatError> {
        Ok(self.registry.clone())
    }
}

fn locator(locations: FakeLocations) -> WechatLocator {
    WechatLocator::with_system_locations(Arc::new(locations))
}

fn empty_locator() -> WechatLocator {
    locator(FakeLocations::default())
}

fn mkdir(path: &Path) -> PathBuf {
    fs::create_dir_all(path).unwrap();
    path.to_path_buf()
}

fn write_file(path: &Path, len: usize, mtime_secs: u64) -> PathBuf {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, vec![b'x'; len]).unwrap();
    set_mtime(path, mtime_secs);
    path.to_path_buf()
}

fn set_mtime(path: &Path, mtime_secs: u64) {
    let file = OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(mtime_secs))
        .unwrap();
}

fn create_account4(root: &Path, name: &str) -> PathBuf {
    let directory = root.join(name);
    mkdir(&directory.join("msg"));
    mkdir(&directory.join("db_storage"));
    directory
}

fn create_account3(root: &Path, name: &str) -> PathBuf {
    let directory = root.join(name);
    mkdir(&directory.join("FileStorage"));
    mkdir(&directory.join("Msg"));
    directory
}

fn category_size(account: &super::WechatAccount, category: WechatCategory) -> (u64, u64) {
    let info = account.category(category).unwrap();
    (info.size_bytes, info.file_count)
}

#[test]
fn layout_descriptions_cover_windows_versions_and_reserve_macos() {
    assert_eq!(WECHAT_3_LAYOUT.data_root_name, "WeChat Files");
    assert_eq!(WECHAT_4_LAYOUT.data_root_name, "xwechat_files");
    assert_eq!(WECHAT_3_LAYOUT.account_markers, ["FileStorage", "Msg"]);
    assert_eq!(WECHAT_4_LAYOUT.account_markers, ["msg", "db_storage"]);
    assert_eq!(WECHAT_3_LAYOUT.excluded_directories.len(), 5);
    assert!(WECHAT_4_LAYOUT.excludes_directory_name("all users"));
    assert!(WECHAT_4_LAYOUT.excludes_directory_name("ALL_USERS"));
    assert!(WECHAT_4_LAYOUT.excludes_directory_name("bAcKuP"));
    assert!(WECHAT_4_LAYOUT.excludes_directory_name("Applet"));
    assert!(WECHAT_4_LAYOUT.excludes_directory_name("wmpf"));
    assert!(!WECHAT_4_LAYOUT.excludes_directory_name("wxid_test0001"));

    assert_eq!(WINDOWS_LAYOUTS.len(), 2);
    assert!(MACOS_LAYOUTS.is_empty());
    assert_eq!(
        layout_for(WechatVersion::Wechat3).data_root_name,
        "WeChat Files"
    );
    assert_eq!(
        layout_for(WechatVersion::Wechat4).data_root_name,
        "xwechat_files"
    );

    assert_eq!(
        WECHAT_3_LAYOUT
            .category(WechatCategory::ChatFiles)
            .unwrap()
            .account_paths,
        &["FileStorage/File"][..]
    );
    assert_eq!(
        WECHAT_3_LAYOUT
            .category(WechatCategory::ImagesAndVideos)
            .unwrap()
            .account_paths,
        &["FileStorage/Image", "FileStorage/Video"][..]
    );
    assert_eq!(
        WECHAT_4_LAYOUT
            .category(WechatCategory::Databases)
            .unwrap()
            .account_paths,
        &["db_storage"][..]
    );
    assert_eq!(
        WECHAT_4_LAYOUT
            .category(WechatCategory::PhoneMigration)
            .unwrap()
            .data_root_paths,
        &["Backup/{account}"][..]
    );
    assert_eq!(WECHAT_3_LAYOUT.cache_directories, ["Cache", "temp"]);
    assert_eq!(
        WECHAT_4_LAYOUT.cache_directories,
        ["cache", "temp", "apm_record", "resource", "business/xweb"]
    );
}

#[tokio::test]
async fn discovers_two_wechat4_accounts_and_excludes_non_account_directories() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("xwechat_files");
    let first = create_account4(&root, "wxid_test0001");
    let second = create_account4(&root, "wxid_test0002");

    write_file(&first.join("msg/file/2026-01/report.pdf"), 10, 4_100);
    write_file(&first.join("msg/attach/hash/2026-01/photo.dat"), 20, 4_200);
    write_file(&first.join("msg/video/2026-01/clip.mp4"), 30, 4_300);
    write_file(&first.join("db_storage/contact/contact.db"), 40, 4_400);
    // 缓存/临时目录不属于任何类别，不得计入大小。
    write_file(&first.join("cache/2026-01/blob.dat"), 1_000, 4_500);
    write_file(&first.join("temp/tmp.dat"), 1_000, 4_600);
    write_file(&first.join("business/xweb/cache.dat"), 1_000, 4_700);
    // Backup 即使带有特征目录也必须作为非账号目录排除。
    create_account4(&root, "Backup");
    write_file(&root.join("Backup/wxid_test0001/backup.bin"), 50, 4_800);

    write_file(&second.join("msg/file/2026-01/old.docx"), 5, 1_100);
    write_file(&second.join("db_storage/message/message.db"), 7, 1_200);

    create_account4(&root, "all_users");
    write_file(&root.join("all_users/msg/file/avatar.dat"), 1, 1_000);
    create_account4(&root, "Applet");
    create_account4(&root, "WMPF");
    // 只有一个特征目录，不是账号目录。
    mkdir(&root.join("wxid_partial/msg"));

    let accounts = empty_locator()
        .discover(std::slice::from_ref(&root))
        .await
        .unwrap();
    let names: Vec<&str> = accounts
        .iter()
        .map(|account| account.account_name.as_str())
        .collect();
    assert_eq!(names, ["wxid_test0001", "wxid_test0002"]);
    for excluded in ["all_users", "Applet", "WMPF", "Backup", "wxid_partial"] {
        assert!(!names.contains(&excluded), "{excluded} 不应出现");
    }

    let first_account = &accounts[0];
    assert_eq!(first_account.version, WechatVersion::Wechat4);
    assert_eq!(first_account.source, WechatSource::Manual);
    assert_eq!(first_account.data_root, root);
    assert_eq!(first_account.account_dir, root.join("wxid_test0001"));
    assert_eq!(first_account.total_size_bytes, 150);
    assert_eq!(first_account.total_file_count, 5);
    assert_eq!(first_account.last_modified_ns, Some(4_800 * 1_000_000_000));
    assert!(first_account.is_current_login_candidate);
    assert!(first_account.has_phone_migration_backup);
    assert_eq!(
        category_size(first_account, WechatCategory::ChatFiles),
        (10, 1)
    );
    assert_eq!(
        category_size(first_account, WechatCategory::ImagesAndVideos),
        (50, 2)
    );
    assert_eq!(
        category_size(first_account, WechatCategory::Databases),
        (40, 1)
    );
    assert_eq!(
        category_size(first_account, WechatCategory::PhoneMigration),
        (50, 1)
    );
    assert_eq!(
        first_account
            .category(WechatCategory::ChatFiles)
            .unwrap()
            .paths,
        vec![root.join("wxid_test0001/msg/file")]
    );
    assert_eq!(
        first_account
            .category(WechatCategory::PhoneMigration)
            .unwrap()
            .paths,
        vec![root.join("Backup/wxid_test0001")]
    );

    let second_account = &accounts[1];
    assert_eq!(second_account.total_size_bytes, 12);
    assert_eq!(second_account.total_file_count, 2);
    assert!(!second_account.is_current_login_candidate);
    assert!(!second_account.has_phone_migration_backup);
}

#[tokio::test]
async fn discovers_wechat3_accounts_and_excludes_all_users_and_applet() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("WeChat Files");
    let account = create_account3(&root, "wxid_test0001");

    write_file(&account.join("FileStorage/File/doc.bin"), 11, 2_100);
    write_file(&account.join("FileStorage/Image/photo.dat"), 12, 2_200);
    write_file(&account.join("FileStorage/Video/clip.mp4"), 13, 2_300);
    write_file(&account.join("Msg/message.db"), 14, 2_400);
    // Backup 即使带有特征目录也必须作为非账号目录排除。
    create_account3(&root, "Backup");
    write_file(&root.join("Backup/migration.bin"), 15, 2_500);
    write_file(&account.join("Cache/cache.dat"), 1_000, 2_600);
    write_file(&account.join("temp/tmp.dat"), 1_000, 2_700);

    create_account3(&root, "All Users");
    create_account3(&root, "Applet");
    create_account3(&root, "WMPF");

    let accounts = empty_locator()
        .discover(std::slice::from_ref(&root))
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    let found = &accounts[0];
    assert_eq!(found.account_name, "wxid_test0001");
    assert_eq!(found.version, WechatVersion::Wechat3);
    assert_eq!(found.source, WechatSource::Manual);
    assert_eq!(found.data_root, root);
    assert_eq!(found.total_size_bytes, 65);
    assert_eq!(found.total_file_count, 5);
    assert!(found.is_current_login_candidate);
    assert!(found.has_phone_migration_backup);
    assert_eq!(category_size(found, WechatCategory::ChatFiles), (11, 1));
    assert_eq!(
        category_size(found, WechatCategory::ImagesAndVideos),
        (25, 2)
    );
    assert_eq!(category_size(found, WechatCategory::Databases), (14, 1));
    assert_eq!(
        category_size(found, WechatCategory::PhoneMigration),
        (15, 1)
    );
    assert_eq!(
        found
            .category(WechatCategory::PhoneMigration)
            .unwrap()
            .paths,
        vec![root.join("Backup")]
    );
}

#[tokio::test]
async fn manual_directories_accept_root_parent_and_single_account() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().join("Data");
    let root = base.join("xwechat_files");
    let account = create_account4(&root, "wxid_test0001");
    write_file(&account.join("msg/file/a.bin"), 3, 1_000);

    let locator = empty_locator();
    for input in [root.clone(), base.clone(), account.clone()] {
        let accounts = locator.discover(&[input]).await.unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].source, WechatSource::Manual);
        assert_eq!(accounts[0].data_root, root);
    }

    let deduplicated = locator
        .discover(&[root.clone(), base, account])
        .await
        .unwrap();
    assert_eq!(deduplicated.len(), 1);
}

#[tokio::test]
async fn no_accounts_returns_empty_list() {
    let temp = TempDir::new().unwrap();
    let empty = temp.path().join("empty");
    mkdir(&empty);
    assert!(empty_locator().discover(&[empty]).await.unwrap().is_empty());

    let documents = temp.path().join("Documents");
    mkdir(&documents.join("WeChat Files"));
    mkdir(&documents.join("xwechat_files"));
    let locations = FakeLocations {
        documents: Some(documents),
        ..Default::default()
    };
    assert!(locator(locations).discover(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn registry_file_save_path_finds_wechat3_data_root() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().join("WeChatData");
    let root = base.join("WeChat Files");
    let account = create_account3(&root, "wxid_test0001");
    write_file(&account.join("Msg/message.db"), 10, 1_000);

    let locations = FakeLocations {
        registry: Some(base.to_string_lossy().into_owned()),
        ..Default::default()
    };
    let accounts = locator(locations).discover(&[]).await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].source, WechatSource::Registry);
    assert_eq!(accounts[0].data_root, root);
}

#[tokio::test]
async fn my_document_registry_value_resolves_to_documents_folder() {
    let temp = TempDir::new().unwrap();
    let documents = temp.path().join("Documents");
    let root = documents.join("WeChat Files");
    let account = create_account3(&root, "wxid_test0001");
    write_file(&account.join("Msg/message.db"), 10, 1_000);

    let locations = FakeLocations {
        documents: Some(documents),
        registry: Some("MyDocument:".to_owned()),
        ..Default::default()
    };
    let accounts = locator(locations).discover(&[]).await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].source, WechatSource::Registry);
    assert_eq!(accounts[0].data_root, root);
}

#[tokio::test]
async fn wechat4_ini_config_finds_data_root_and_ignores_invalid_files() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().join("Doc");
    let root = base.join("xwechat_files");
    let account = create_account4(&root, "wxid_test0001");
    write_file(&account.join("msg/file/a.bin"), 10, 1_000);

    let appdata = temp.path().join("AppData");
    let config = appdata.join("Tencent/xwechat/config");
    mkdir(&config);
    let mut with_bom = Vec::from([0xEF_u8, 0xBB, 0xBF]);
    with_bom.extend_from_slice(base.to_string_lossy().as_bytes());
    with_bom.extend_from_slice(b"\r\n");
    fs::write(config.join("aaaaaaaa.ini"), with_bom).unwrap();
    fs::write(
        config.join("bbbbbbbb.ini"),
        format!("{}\n", base.join("missing").display()),
    )
    .unwrap();
    fs::write(config.join("cccccccc.ini"), b"enabled=1\n").unwrap();
    fs::write(config.join("dddddddd.ini"), b"relative/path\n").unwrap();
    fs::write(config.join("ignore.txt"), base.to_string_lossy().as_bytes()).unwrap();

    let locations = FakeLocations {
        appdata: Some(appdata),
        ..Default::default()
    };
    let accounts = locator(locations).discover(&[]).await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].source, WechatSource::ConfigFile);
    assert_eq!(accounts[0].data_root, root);
}

#[tokio::test]
async fn default_location_uses_documents_folder() {
    let temp = TempDir::new().unwrap();
    let documents = temp.path().join("Documents");
    let root = documents.join("xwechat_files");
    let account = create_account4(&root, "wxid_test0001");
    write_file(&account.join("msg/file/a.bin"), 10, 1_000);

    let locations = FakeLocations {
        documents: Some(documents),
        ..Default::default()
    };
    let accounts = locator(locations).discover(&[]).await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].source, WechatSource::DefaultLocation);
    assert_eq!(accounts[0].data_root, root);
}

#[tokio::test]
async fn same_root_from_config_and_default_is_listed_once() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().join("Data");
    let root = base.join("xwechat_files");
    let account = create_account4(&root, "wxid_test0001");
    write_file(&account.join("msg/file/a.bin"), 10, 1_000);

    let appdata = temp.path().join("AppData");
    let config = appdata.join("Tencent/xwechat/config");
    mkdir(&config);
    fs::write(
        config.join("aaaaaaaa.ini"),
        base.to_string_lossy().as_bytes(),
    )
    .unwrap();

    let locations = FakeLocations {
        documents: Some(base),
        appdata: Some(appdata),
        ..Default::default()
    };
    let accounts = locator(locations).discover(&[]).await.unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].source, WechatSource::ConfigFile);
    assert_eq!(accounts[0].data_root, root);
}

#[derive(Debug, PartialEq, Eq)]
struct SnapshotEntry {
    path: String,
    len: u64,
    modified: SystemTime,
}

#[tokio::test]
async fn discovery_does_not_modify_wechat_directories() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("xwechat_files");
    let account = create_account4(&root, "wxid_test0001");
    write_file(&account.join("msg/file/a.pdf"), 10, 1_000);
    write_file(&account.join("db_storage/contact/contact.db"), 20, 2_000);
    write_file(&root.join("Backup/wxid_test0001/backup.bin"), 30, 3_000);
    create_account4(&root, "all_users");

    let before = snapshot(&root);
    let accounts = empty_locator()
        .discover(std::slice::from_ref(&root))
        .await
        .unwrap();
    assert_eq!(accounts.len(), 1);
    let after = snapshot(&root);
    assert_eq!(before, after, "发现过程不得修改微信目录");
}

fn snapshot(root: &Path) -> Vec<SnapshotEntry> {
    let mut entries = Vec::new();
    collect_snapshot(root, root, &mut entries);
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

fn collect_snapshot(root: &Path, path: &Path, entries: &mut Vec<SnapshotEntry>) {
    let metadata = fs::symlink_metadata(path).unwrap();
    let relative = path.strip_prefix(root).unwrap_or(path);
    entries.push(SnapshotEntry {
        path: relative.to_string_lossy().into_owned(),
        len: metadata.len(),
        modified: metadata.modified().unwrap(),
    });
    if metadata.is_dir() {
        let mut children: Vec<PathBuf> = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        children.sort();
        for child in children {
            collect_snapshot(root, &child, entries);
        }
    }
}

#[tokio::test]
async fn notes_can_be_set_read_and_cleared() {
    let notes = WechatAccountNotes::new(StateStore::open_in_memory().unwrap());
    let root = Path::new("D:/WeChatData/xwechat_files");
    assert!(notes.get(root, "wxid_test0001").await.unwrap().is_none());

    notes
        .set(root, "wxid_test0001", "  主账号  ")
        .await
        .unwrap();
    assert_eq!(
        notes.get(root, "wxid_test0001").await.unwrap(),
        Some("主账号".to_owned())
    );

    notes.clear(root, "wxid_test0001").await.unwrap();
    assert!(notes.get(root, "wxid_test0001").await.unwrap().is_none());
}

#[tokio::test]
async fn notes_reject_invalid_values() {
    let notes = WechatAccountNotes::new(StateStore::open_in_memory().unwrap());
    let root = Path::new("D:/WeChatData/xwechat_files");
    let account = "wxid_test0001";

    for note in ["", "   ", "a\nb", "a\tb"] {
        let result = notes.set(root, account, note).await;
        assert!(
            matches!(result, Err(WechatError::InvalidNote(_))),
            "{note:?} 应被拒绝"
        );
    }
    let too_long = "a".repeat(super::WECHAT_NOTE_MAX_CHARS + 1);
    assert!(matches!(
        notes.set(root, account, &too_long).await,
        Err(WechatError::InvalidNote(_))
    ));
    assert!(
        notes
            .set(root, account, &"a".repeat(super::WECHAT_NOTE_MAX_CHARS))
            .await
            .is_ok()
    );
    assert!(matches!(
        notes.set(root, "", "备注").await,
        Err(WechatError::InvalidAccount)
    ));
}
