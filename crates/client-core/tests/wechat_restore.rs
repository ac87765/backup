//! Docker 集成测试：备份 → 快照 → 在另一个 tempdir 的“新电脑”中恢复 → 逐字节一致。
//!
//! 由审查者在 WSL 运行：
//! `cargo test -p client-core --features desktop --test wechat_restore`
//!
//! 覆盖 spec“新电脑恢复”：文件类别合并恢复后目录结构与内容一致；数据库快照恢复后
//! 与原快照逐字节相同。测试只使用临时夹具，不读取宿主机真实微信目录、不枚举进程。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use client_core::desktop::wechat::{
    WechatCategory, WechatLocator, WechatProcessProbe, WechatSystemLocations, WechatVersion,
};
use client_core::desktop::{
    BackupEngine, BackupEngineConfig, WechatRestoreRequest, WechatSnapshotSelection,
};
use client_core::{Client, ClientBuilder, MemorySecretStore};
use common::{TestServer, TestUser, create_user, environment, start_server};

const PASSWORD: &str = "correct horse battery staple";
const ACCOUNT: &str = "wxid_docker_restore01";

struct DocumentsLocations {
    documents: PathBuf,
}

impl WechatSystemLocations for DocumentsLocations {
    fn documents_dir(&self) -> Result<Option<PathBuf>, client_core::desktop::wechat::WechatError> {
        Ok(Some(self.documents.clone()))
    }

    fn appdata_dir(&self) -> Result<Option<PathBuf>, client_core::desktop::wechat::WechatError> {
        Ok(None)
    }

    fn wechat_3_file_save_path(
        &self,
    ) -> Result<Option<String>, client_core::desktop::wechat::WechatError> {
        Ok(None)
    }
}

struct NeverRunningProbe;

impl WechatProcessProbe for NeverRunningProbe {
    fn is_running(&self, _version: WechatVersion) -> bool {
        false
    }
}

fn backup_client(server: &TestServer, app_data: &Path) -> Client {
    ClientBuilder::new(server.base_url.clone())
        .secret_store(Arc::new(MemorySecretStore::new()))
        .app_data_dir(app_data)
        .build()
        .unwrap()
}

fn create_fixture(base: &Path) -> PathBuf {
    let documents = base.join("文档");
    let account = documents.join("xwechat_files").join(ACCOUNT);
    for directory in [
        "msg/file/2026-10",
        "msg/attach/0123456789abcdef0123456789abcdef/2026-10",
        "msg/video/2026-10",
        "db_storage/message",
        "db_storage/contact",
        "db_storage/MMKV",
    ] {
        std::fs::create_dir_all(account.join(directory)).unwrap();
    }
    std::fs::write(account.join("msg/file/2026-10/报告.pdf"), b"pdf-report").unwrap();
    std::fs::write(
        account.join("msg/attach/0123456789abcdef0123456789abcdef/2026-10/pic.dat"),
        b"image-bytes",
    )
    .unwrap();
    std::fs::write(account.join("msg/video/2026-10/clip.mp4"), b"video-bytes").unwrap();
    std::fs::write(
        account.join("db_storage/message/message_0.db"),
        b"database-bytes",
    )
    .unwrap();
    std::fs::write(
        account.join("db_storage/message/message_0.db-wal"),
        b"wal-bytes",
    )
    .unwrap();
    std::fs::write(
        account.join("db_storage/contact/contact.kvdb"),
        b"kvdb-bytes",
    )
    .unwrap();
    std::fs::write(account.join("db_storage/MMKV/mmkv.bin"), b"mmkv-bytes").unwrap();
    documents
}

async fn build_engine(client: &Client, documents: &Path, device_name: &str) -> BackupEngine {
    BackupEngine::new(
        client.clone(),
        BackupEngineConfig {
            device_name: device_name.to_owned(),
            debounce: Duration::from_millis(50),
            full_scan_interval: Duration::from_secs(3600),
            retry_interval: Duration::from_secs(60),
            watch_files: false,
            wechat_locator: Some(WechatLocator::with_system_locations(Arc::new(
                DocumentsLocations {
                    documents: documents.to_path_buf(),
                },
            ))),
            wechat_manual_directories: Vec::new(),
            wechat_process_probe: Some(Arc::new(NeverRunningProbe)),
            wechat_snapshot_interval: Duration::from_secs(10),
        },
    )
    .unwrap()
}

async fn login(client: &Client, user: &TestUser) {
    client.login(&user.email, PASSWORD).await.unwrap();
}

fn tree_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(directory: &Path, base: &Path, output: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if entry.metadata().unwrap().is_dir() {
                walk(&path, base, output);
            } else {
                let relative = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                output.insert(relative, std::fs::read(path).unwrap());
            }
        }
    }
    let mut output = BTreeMap::new();
    walk(root, root, &mut output);
    output
}

#[tokio::test]
async fn wechat_restore_on_new_computer_is_byte_identical_on_real_server() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let app_data = tempfile::tempdir().unwrap();
    let client = backup_client(&server, app_data.path());
    login(&client, &user).await;

    let fixture = tempfile::tempdir().unwrap();
    let documents = create_fixture(fixture.path());
    let old_engine = build_engine(&client, &documents, "OLD-PC").await;
    let locator = WechatLocator::with_system_locations(Arc::new(DocumentsLocations {
        documents: documents.clone(),
    }));
    let account = locator
        .discover(&[])
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.account_name == ACCOUNT)
        .expect("夹具中应有微信账号");
    let categories = [
        WechatCategory::ChatFiles,
        WechatCategory::ImagesAndVideos,
        WechatCategory::Databases,
    ];
    old_engine
        .set_wechat_categories(&account, &categories)
        .await
        .unwrap();
    old_engine.scan_all_now().await.unwrap();
    let snapshot = old_engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(
        snapshot.outcome,
        client_core::desktop::WechatSnapshotOutcome::Completed
    );
    let snapshots = old_engine.wechat_database_snapshots(ACCOUNT).await.unwrap();
    assert_eq!(snapshots.len(), 1);
    let selection = WechatSnapshotSelection {
        device_name: snapshots[0].device_name.clone(),
        timestamp: snapshots[0].timestamp.clone(),
    };

    // 新电脑：只登录过一次微信，账号目录里只有基本结构。
    let target_base = tempfile::tempdir().unwrap();
    let target_root = target_base.path().join("新电脑").join("xwechat_files");
    let target_account = target_root.join(ACCOUNT);
    for directory in ["msg/file", "msg/attach", "msg/video"] {
        std::fs::create_dir_all(target_account.join(directory)).unwrap();
    }

    let new_engine = build_engine(&client, target_base.path(), "NEW-PC").await;
    let request = WechatRestoreRequest {
        data_root: target_root,
        account_name: ACCOUNT.to_owned(),
        version: WechatVersion::Wechat4,
        categories: vec![WechatCategory::ChatFiles, WechatCategory::ImagesAndVideos],
        restore_database: true,
        snapshot: Some(selection),
    };
    let task = new_engine.restore_wechat(request).await.unwrap();
    let report = task.run().await.unwrap();
    assert!(report.database_restored);

    // 文件类别：相对目录结构原样落到新电脑，内容逐字节一致。
    for relative in ["msg/file", "msg/attach", "msg/video"] {
        assert_eq!(
            tree_bytes(&account.account_dir.join(relative)),
            tree_bytes(&target_account.join(relative)),
            "{relative} 恢复后必须逐字节一致"
        );
    }
    // 数据库：新电脑原本没有 db_storage 内容，恢复结果必须与快照逐字节一致。
    assert_eq!(
        tree_bytes(&account.account_dir.join("db_storage")),
        tree_bytes(&target_account.join("db_storage")),
        "数据库恢复后必须逐字节一致"
    );
    // 没有任何旧数据库目录可保留（新电脑首次恢复）。
    assert!(
        std::fs::read_dir(&target_account)
            .unwrap()
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with("db_storage.before-restore-"))
    );
}
