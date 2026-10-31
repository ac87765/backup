//! 备份引擎错误。错误信息不包含本地路径、文件名、令牌或密钥。

use crate::error::ClientError;
use crate::state::StateError;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error(transparent)]
    Client(#[from] ClientError),

    #[error(transparent)]
    State(#[from] StateError),

    /// 配置不合法（路径不是文件夹、glob 非法、设备名不可用等）。
    #[error("备份文件夹配置无效：{0}")]
    InvalidConfig(&'static str),

    /// 尚未登录或没有可写账号。
    #[error("需要登录并解锁后才能备份")]
    NotSignedIn,

    /// 文件夹 ID 不属于当前用户或已删除。
    #[error("找不到备份文件夹")]
    FolderNotFound,

    /// 监听失败；引擎会降级为定时全量核对。
    #[error("文件监听不可用，已降级为定时全量核对")]
    WatchUnavailable,

    /// 调度器已经在运行。
    #[error("备份引擎已在运行")]
    AlreadyRunning,
}
