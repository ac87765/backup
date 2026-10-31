//! Docker 集成测试：真实 PostgreSQL + MinIO + 服务端上的文件夹备份。
//!
//! 由审查者在 WSL 运行：
//! `cargo test -p client-core --features desktop --test backup`
//!
//! 覆盖真实服务端的目录逐层创建、增量新版本、旧版本恢复与“本地删除不删除云端”。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use client_core::desktop::{
    BackupEngine, BackupEngineConfig, BackupFolderDraft, BackupFolderStatus,
};
use client_core::{Client, ClientBuilder, ConflictPolicy, MemorySecretStore, NodeKind, NodeView};
use common::{TestServer, TestUser, create_user, environment, start_server};

const PASSWORD: &str = "correct horse battery staple";

fn backup_client(server: &TestServer, app_data: &Path) -> Client {
    ClientBuilder::new(server.base_url.clone())
        .secret_store(Arc::new(MemorySecretStore::new()))
        .app_data_dir(app_data)
        .build()
        .unwrap()
}

async fn login(client: &Client, user: &TestUser) {
    client.login(&user.email, PASSWORD).await.unwrap();
}

async fn children(client: &Client, folder_id: uuid::Uuid) -> Vec<NodeView> {
    client
        .list_folder(folder_id, Default::default())
        .await
        .unwrap()
}

async fn default_target(client: &Client, local_name: &str) -> uuid::Uuid {
    let root = client.account().unwrap().root_node_id;
    let mut parent = root;
    for name in ["电脑备份", "TEST-PC", local_name] {
        parent = children(client, parent)
            .await
            .into_iter()
            .find(|node| node.name == name && node.kind == NodeKind::Folder)
            .unwrap_or_else(|| panic!("缺少云端文件夹 {name}"))
            .id;
    }
    parent
}

fn set_mtime_forward(path: &Path) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() + Duration::from_secs(120))
        .unwrap();
}

#[tokio::test]
async fn folder_backup_incremental_versions_and_local_delete_on_real_server() {
    let env = environment().await;
    let server = start_server(env).await;
    let user = create_user(&server.database, "active", false, PASSWORD).await;
    let app_data = tempfile::tempdir().unwrap();
    let client = backup_client(&server, app_data.path());
    login(&client, &user).await;

    let files = tempfile::tempdir().unwrap();
    let work = files.path().join("资料");
    std::fs::create_dir_all(work.join("子目录")).unwrap();
    std::fs::write(work.join("a.txt"), b"version-one").unwrap();
    std::fs::write(work.join("子目录/b.txt"), b"bee").unwrap();
    std::fs::write(work.join("cafe\u{301}.txt"), b"nfd").unwrap();

    let engine = BackupEngine::new(
        client.clone(),
        BackupEngineConfig {
            device_name: "TEST-PC".to_owned(),
            debounce: Duration::from_millis(50),
            full_scan_interval: Duration::from_secs(3600),
            retry_interval: Duration::from_secs(60),
            watch_files: false,
        },
    )
    .unwrap();
    let folder = engine
        .add_folder(BackupFolderDraft::new(&work))
        .await
        .unwrap();
    let first = engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(first.status, BackupFolderStatus::IdleOk);
    assert_eq!(first.uploaded_files, 3);

    let target = default_target(&client, "资料").await;
    let cloud = children(&client, target).await;
    assert_eq!(cloud.len(), 3);
    let sub = cloud.iter().find(|node| node.name == "子目录").unwrap().id;
    assert_eq!(children(&client, sub).await.len(), 1);
    let a = cloud.iter().find(|node| node.name == "a.txt").unwrap();

    // 修改 → 新版本，旧版本可恢复。
    std::fs::write(work.join("a.txt"), b"version-two-longer").unwrap();
    set_mtime_forward(&work.join("a.txt"));
    let second = engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(second.updated_files, 1);
    let versions = client.list_versions(a.id).await.unwrap();
    assert_eq!(versions.len(), 2);
    let old = versions.iter().find(|version| !version.is_current).unwrap();
    let output = tempfile::tempdir().unwrap();
    let downloaded = client
        .download_file(
            a.id,
            old.id,
            "a.txt",
            output.path(),
            ConflictPolicy::Replace,
        )
        .unwrap()
        .run()
        .await
        .unwrap();
    assert_eq!(std::fs::read(downloaded.path).unwrap(), b"version-one");

    // 本地删除不删除云端。
    std::fs::remove_file(work.join("子目录/b.txt")).unwrap();
    let third = engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(third.uploaded_files, 0);
    assert_eq!(children(&client, target).await.len(), 3);
    assert_eq!(children(&client, sub).await.len(), 1);

    // 同名 NFC 文件不产生重复：再扫描一次，内容相同只更新索引。
    let fourth = engine.scan_folder_now(folder.id).await.unwrap();
    assert_eq!(fourth.unchanged_files, 2);
    assert_eq!(fourth.uploaded_files, 0);
}
