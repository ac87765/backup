//! 初始化：创建 Rust 核心、备份引擎与更新检查器。

use std::path::Path;
use std::sync::Arc;

use client_core::Client;
use client_core::StateStore;
use client_core::desktop::update::{UpdateChecker, UpdateDownloader};
use client_core::desktop::{BackupEngine, BackupEngineConfig};

use crate::api::error::BridgeError;
use crate::state;

/// 初始化客户端。
///
/// - `app_data_dir`：本地状态库目录（design D5，由界面传入平台应用数据目录）；
/// - `server_url`：服务端地址（登录与更新检查共用）；
/// - `device_name`：默认云端路径 `电脑备份/<设备名>/<本地文件夹名>` 使用的设备名；
/// - `app_version`：Dart 的 `APP_VERSION`（`git describe` 注入），由界面传入，
///   桥接层不自行推断。
///
/// 可重复调用：新调用会替换旧状态。初始化本身不登录、不发网络请求。
pub async fn initialize(
    app_data_dir: String,
    server_url: String,
    device_name: String,
    app_version: String,
) -> Result<(), BridgeError> {
    // 状态库在 Client 与更新检查器之间共享：防回滚水位与横幅记录属于本机，
    // 在登录前也必须可读写。
    let state_store = Arc::new(StateStore::open(Path::new(&app_data_dir))?);
    let client = Client::builder(server_url.clone())
        .state_store(state_store.clone())
        .build()?;
    let engine = BackupEngine::new(client.clone(), BackupEngineConfig::new(device_name))?;
    let checker = UpdateChecker::new(&server_url, app_version.clone(), state_store)?;
    let downloader = UpdateDownloader::new(&server_url, &app_data_dir, &app_version)?;
    let _state = state::install(client, engine, checker, downloader)?;
    Ok(())
}
