//! Docker 集成测试：真实 PostgreSQL + MinIO + 服务端上的微信数据库快照。
//!
//! 由审查者在 WSL 运行：
//! `cargo test -p client-core --features desktop --test wechat_snapshot`
//!
//! 覆盖 spec“数据库原样上传”：快照完成后下载每个文件，与本地原文件逐字节比较；
//! `快照完成` 清单 JSON 中的 SHA-256 也必须与原文件一致。测试只使用临时夹具，
//! 不读取宿主机真实微信目录、不枚举宿主机进程。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use client_core::desktop::wechat::{
    WechatCategory, WechatLocator, WechatProcessProbe, WechatSystemLocations, WechatVersion,
};
use client_core::desktop::{BackupEngine, BackupEngineConfig, WechatSnapshotOutcome};
use client_core::{Client, ClientBuilder, ConflictPolicy, MemorySecretStore, NodeKind, NodeView};
use common::{TestServer, TestUser, create_user, environment, start_server};
use crypto_core::ContentHasher;
use serde::Deserialize;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const ACCOUNT: &str = "wxid_docker0001";
const MARKER: &str = "快照完成";

/// 只提供测试临时“文档”目录的系统位置。
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

/// 假进程探测：集成测试不枚举宿主机真实进程。
struct NeverRunningProbe;

impl WechatProcessProbe for NeverRunningProbe {
    fn is_running(&self, _version: WechatVersion) -> bool {
        false
    }
}

#[derive(Debug, Deserialize)]
struct MarkerManifest {
    snapshot_time: i64,
    files: Vec<MarkerFile>,
}

#[derive(Debug, Deserialize)]
struct MarkerFile {
    relative_path: String,
    size: u64,
    content_sha256: String,
}

fn backup_client(server: &TestServer, app_data: &Path) -> Client {
    ClientBuilder::new(server.base_url.clone())
        .secret_store(Arc::new(MemorySecretStore::new()))
        .app_data_dir(app_data)
        .build()
        .unwrap()
}

async fn children(client: &Client, folder_id: Uuid) -> Vec<NodeView> {
    client
        .list_folder(folder_id, Default::default())
        .await
        .unwrap()
}

async fn folder_by_path(client: &Client, names: &[&str]) -> Uuid {
    let mut parent = client.account().unwrap().root_node_id;
    for name in names {
        parent = children(client, parent)
            .await
            .into_iter()
            .find(|node| node.kind == NodeKind::Folder && node.name == *name)
            .unwrap_or_else(|| panic!("缺少云端文件夹 {name}"))
            .id;
    }
    parent
}

/// 递归收集云端文件（相对时间戳目录的路径）。
async fn cloud_files(
    client: &Client,
    folder_id: Uuid,
    prefix: &str,
    output: &mut Vec<(String, NodeView)>,
) {
    for node in children(client, folder_id).await {
        let relative = if prefix.is_empty() {
            node.name.clone()
        } else {
            format!("{prefix}/{}", node.name)
        };
        match node.kind {
            NodeKind::Folder => Box::pin(cloud_files(client, node.id, &relative, output)).await,
            NodeKind::File => output.push((relative, node)),
        }
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = ContentHasher::new();
    hasher.update(bytes);
    hasher.finish_hex()
}

fn create_fixture(base: &Path) -> (PathBuf, PathBuf) {
    let documents = base.join("文档");
    let account = documents.join("xwechat_files").join(ACCOUNT);
    for directory in [
        "msg/file/2026-10",
        "db_storage/message",
        "db_storage/contact",
        "db_storage/MMKV",
    ] {
        std::fs::create_dir_all(account.join(directory)).unwrap();
    }
    let database = account.join("db_storage");
    // 一个跨分块的大文件（4 MiB + 17 字节），验证分块上传后仍逐字节一致。
    let large: Vec<u8> = (0..(4 * 1024 * 1024 + 17_u32))
        .map(|index| (index % 251) as u8)
        .collect();
    std::fs::write(database.join("message/message_0.db"), &large).unwrap();
    std::fs::write(database.join("message/message_0.db-wal"), b"wal-bytes").unwrap();
    std::fs::write(database.join("message/message_0.db-shm"), b"shm-bytes").unwrap();
    std::fs::write(database.join("contact/contact.kvdb"), b"kvdb-bytes").unwrap();
    std::fs::write(database.join("contact/contact.kvdb-shm"), b"kvdb-shm").unwrap();
    std::fs::write(database.join("contact/contact.material"), b"material").unwrap();
    std::fs::write(database.join("MMKV/mmkv.bin"), b"mmkv-bytes").unwrap();
    (documents, account)
}

fn local_file_path(account: &Path, relative: &str) -> PathBuf {
    let mut path = account.join("db_storage");
    for segment in relative.split('/') {
        path = path.join(segment);
    }
    path
}

async fn login(client: &Client, user: &TestUser) {
    client.login(&user.email, PASSWORD).await.unwrap();
}

#[tokio::test]
async fn wechat_database_snapshot_is_byte_identical_on_real_server() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let app_data = tempfile::tempdir().unwrap();
    let client = backup_client(&server, app_data.path());
    login(&client, &user).await;

    let fixture = tempfile::tempdir().unwrap();
    let (documents, account_dir) = create_fixture(fixture.path());
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

    let engine = BackupEngine::new(
        client.clone(),
        BackupEngineConfig {
            device_name: "TEST-PC".to_owned(),
            debounce: Duration::from_millis(50),
            full_scan_interval: Duration::from_secs(3600),
            retry_interval: Duration::from_secs(60),
            watch_files: false,
            wechat_locator: Some(WechatLocator::with_system_locations(Arc::new(
                DocumentsLocations {
                    documents: documents.clone(),
                },
            ))),
            wechat_manual_directories: Vec::new(),
            wechat_process_probe: Some(Arc::new(NeverRunningProbe)),
            wechat_snapshot_interval: Duration::from_secs(10),
        },
    )
    .unwrap();
    engine
        .set_wechat_categories(&account, &[WechatCategory::Databases])
        .await
        .unwrap();

    let before = tree_snapshot(&account_dir);
    let report = engine
        .snapshot_wechat_databases_now(&account)
        .await
        .unwrap();
    assert_eq!(report.outcome, WechatSnapshotOutcome::Completed);

    let parent = folder_by_path(&client, &["微信备份", "TEST-PC", ACCOUNT, "聊天记录数据库"]).await;
    let directories = children(&client, parent)
        .await
        .into_iter()
        .filter(|node| node.kind == NodeKind::Folder)
        .collect::<Vec<_>>();
    assert_eq!(directories.len(), 1, "应只有一份快照");
    let snapshot_dir = directories[0].id;

    let mut files = Vec::new();
    cloud_files(&client, snapshot_dir, "", &mut files).await;
    let marker = files
        .iter()
        .find(|(relative, _)| relative == MARKER)
        .map(|(_, node)| node.clone())
        .expect("快照必须有完成标记");
    let data_files: Vec<(String, NodeView)> = files
        .into_iter()
        .filter(|(relative, _)| relative != MARKER)
        .collect();
    assert_eq!(data_files.len(), 7, "db_storage 内全部文件都要上传");

    // 逐字节比较下载结果，并核对清单摘要。
    let output = tempfile::tempdir().unwrap();
    let mut downloaded_hashes = Vec::new();
    for (relative, node) in &data_files {
        let version_id = node.current_version_id.expect("文件应有当前版本");
        let download = client
            .download_file(
                node.id,
                version_id,
                &node.name,
                output.path(),
                ConflictPolicy::Replace,
            )
            .unwrap()
            .run()
            .await
            .unwrap();
        let downloaded = std::fs::read(&download.path).unwrap();
        let original = std::fs::read(local_file_path(&account_dir, relative)).unwrap();
        assert_eq!(downloaded, original, "{relative} 必须逐字节一致");
        downloaded_hashes.push((relative.clone(), hash_bytes(&original)));
    }

    // 下载并解析 `快照完成`，其 SHA-256 必须与原文件一致。
    let version_id = marker.current_version_id.expect("标记应有当前版本");
    let marker_download = client
        .download_file(
            marker.id,
            version_id,
            MARKER,
            output.path(),
            ConflictPolicy::Replace,
        )
        .unwrap()
        .run()
        .await
        .unwrap();
    let manifest: MarkerManifest =
        serde_json::from_slice(&std::fs::read(&marker_download.path).unwrap()).unwrap();
    downloaded_hashes.sort();
    let mut manifest_hashes: Vec<(String, String)> = manifest
        .files
        .iter()
        .map(|file| (file.relative_path.clone(), file.content_sha256.clone()))
        .collect();
    manifest_hashes.sort();
    assert_eq!(manifest_hashes, downloaded_hashes);
    for file in &manifest.files {
        let original = std::fs::read(local_file_path(&account_dir, &file.relative_path)).unwrap();
        assert_eq!(file.size, original.len() as u64);
        assert_eq!(file.content_sha256, hash_bytes(&original));
    }
    assert!(manifest.snapshot_time > 0);

    // 只读：快照前后微信目录（路径、大小、修改时间）完全一致。
    assert_eq!(before, tree_snapshot(&account_dir));
}

fn tree_snapshot(root: &Path) -> Vec<(PathBuf, u64, i64)> {
    fn walk(directory: &Path, base: &Path, output: &mut Vec<(PathBuf, u64, i64)>) {
        let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap())
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let metadata = entry.metadata().unwrap();
            let path = entry.path();
            if metadata.is_dir() {
                walk(&path, base, output);
            } else {
                let modified = metadata
                    .modified()
                    .unwrap()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as i64;
                output.push((
                    path.strip_prefix(base).unwrap().to_path_buf(),
                    metadata.len(),
                    modified,
                ));
            }
        }
    }
    let mut output = Vec::new();
    walk(root, root, &mut output);
    output.sort();
    output
}
