//! 文件系统监听（design D6，`notify` crate）。
//!
//! 每个备份文件夹一个递归 watcher。事件只携带文件夹 ID 与“是否需要全量核对”，
//! 不带本地路径（日志与事件都不输出路径）。监听失败或事件溢出（`need_rescan`）
//! 时由调度器降级为全量核对。
//!
//! 只读访问事件（`EventKind::Access`）被忽略：引擎自己哈希/上传时会读取文件，
//! 某些平台后端会因此产生 Access 事件，若不忽略会形成“扫描 → 读取 → 事件 →
//! 再扫描”的循环。带 `need_rescan` 标志的事件仍然升级为全量核对。

use std::path::Path;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::io_path::path_for_io;

use super::error::BackupError;

/// 文件监听信号；路径不进入信号，避免跨线程/日志泄漏本机路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WatchSignal {
    pub folder_id: Uuid,
    /// `true` 表示事件可能丢失，需要全量核对。
    pub needs_full_scan: bool,
}

/// 保持 watcher 存活；drop 后停止监听。
pub(crate) struct FolderWatcher {
    _watcher: RecommendedWatcher,
}

/// 把 `notify` 结果映射成需要发送的信号；纯函数，便于单元测试。
fn signal_for_result(
    folder_id: Uuid,
    result: notify::Result<notify::Event>,
) -> Option<WatchSignal> {
    match result {
        Ok(event) => {
            if event.need_rescan() {
                // 事件可能丢失：即使同时是 Access 事件也必须全量核对。
                Some(WatchSignal {
                    folder_id,
                    needs_full_scan: true,
                })
            } else if event.kind.is_access() {
                None
            } else {
                Some(WatchSignal {
                    folder_id,
                    needs_full_scan: false,
                })
            }
        }
        // 监听后端错误：无法区分漏掉了哪些事件，直接升级为全量核对。
        Err(_) => Some(WatchSignal {
            folder_id,
            needs_full_scan: true,
        }),
    }
}

/// 递归监听一个目录。监听失败返回 [`BackupError::WatchUnavailable`]，调用方
/// 应降级为定时全量核对而不是让备份失败。
pub(crate) fn watch_folder(
    root: &Path,
    folder_id: Uuid,
    sender: UnboundedSender<WatchSignal>,
) -> Result<FolderWatcher, BackupError> {
    let root = path_for_io(root).map_err(|_| BackupError::WatchUnavailable)?;
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if let Some(signal) = signal_for_result(folder_id, result) {
            // 调度器退出后 send 失败是正常情况，不能 panic。
            let _ = sender.send(signal);
        }
    })
    .map_err(|_| BackupError::WatchUnavailable)?;
    watcher
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|_| BackupError::WatchUnavailable)?;
    Ok(FolderWatcher { _watcher: watcher })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use notify::Event;
    use notify::event::{AccessKind, EventKind, Flag, ModifyKind};

    use super::*;

    #[test]
    fn access_events_are_ignored() {
        let folder_id = Uuid::new_v4();
        let event = Event::new(EventKind::Access(AccessKind::Read));
        assert_eq!(signal_for_result(folder_id, Ok(event)), None);
    }

    #[test]
    fn rescan_access_event_still_requests_full_scan() {
        let folder_id = Uuid::new_v4();
        let event = Event::new(EventKind::Access(AccessKind::Read)).set_flag(Flag::Rescan);
        assert_eq!(
            signal_for_result(folder_id, Ok(event)),
            Some(WatchSignal {
                folder_id,
                needs_full_scan: true
            })
        );
    }

    #[test]
    fn modify_event_requests_incremental_scan() {
        let folder_id = Uuid::new_v4();
        let event = Event::new(EventKind::Modify(ModifyKind::Any));
        assert_eq!(
            signal_for_result(folder_id, Ok(event)),
            Some(WatchSignal {
                folder_id,
                needs_full_scan: false
            })
        );
    }

    #[test]
    fn watcher_error_requests_full_scan() {
        let folder_id = Uuid::new_v4();
        assert_eq!(
            signal_for_result(folder_id, Err(notify::Error::generic("test"))),
            Some(WatchSignal {
                folder_id,
                needs_full_scan: true
            })
        );
    }
}
