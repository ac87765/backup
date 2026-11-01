//! 备份引擎错误。错误信息不包含本地路径、文件名、令牌或密钥。

use crate::error::ClientError;
use crate::state::StateError;

use super::wechat::WechatError;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error(transparent)]
    Client(#[from] ClientError),

    #[error(transparent)]
    State(#[from] StateError),

    /// 微信来源发现 / 备注名错误（消息不含本机路径）。
    #[error(transparent)]
    Wechat(#[from] WechatError),

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

    /// 恢复需要微信完全退出；开始前或恢复过程中检测到微信在运行。
    #[error("微信正在运行，请先完全退出微信后再恢复")]
    WechatRunning,

    /// 所选快照没有 `快照完成` 标记（或标记在恢复前消失）。
    #[error("所选数据库快照不完整，不能用于恢复")]
    SnapshotIncomplete,

    /// 快照清单与云端文件不一致（大小 / SHA-256 不符，或清单无法解析）。
    #[error("快照内容校验失败，已还原原数据库")]
    SnapshotCorrupted,

    /// 指定设备与时间戳的快照不存在。
    #[error("找不到所选的数据库快照")]
    SnapshotNotFound,
}
