//! 初始化：创建 Rust 核心与备份引擎。

use client_core::Client;
use client_core::desktop::{BackupEngine, BackupEngineConfig};

use crate::api::error::BridgeError;
use crate::state;

/// 初始化客户端。
///
/// - `app_data_dir`：本地状态库目录（design D5，由界面传入平台应用数据目录）；
/// - `server_url`：服务端地址；
/// - `device_name`：默认云端路径 `电脑备份/<设备名>/<本地文件夹名>` 使用的设备名。
///
/// 可重复调用：新调用会替换旧状态。初始化本身不登录、不发网络请求。
pub async fn initialize(
    app_data_dir: String,
    server_url: String,
    device_name: String,
) -> Result<(), BridgeError> {
    let client = Client::builder(server_url)
        .app_data_dir(app_data_dir)
        .build()?;
    let engine = BackupEngine::new(client.clone(), BackupEngineConfig::new(device_name))?;
    let _state = state::install(client, engine)?;
    Ok(())
}
