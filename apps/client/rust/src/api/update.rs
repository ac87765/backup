//! 客户端更新 API（add-client-releases 第 2 组）。
//!
//! 薄层：调用 `client-core::desktop::update`，把结果转成 DTO。当前版本由 Dart
//! 在 `initialize` 时传入；检查请求不携带任何用户凭据。

use client_core::desktop::update::{
    InstallOutcome, UpdateCancellation, UpdateCheck, UpdatePlatform,
};

use crate::api::error::BridgeError;
use crate::api::types::{
    UpdateCheckDto, UpdateCheckStateDto, UpdateInstallKindDto, UpdateInstallOutcomeDto,
    UpdateProgressDto,
};
use crate::convert::update_info;
use crate::frb_generated::StreamSink;
use crate::state::{self, ActiveUpdateDownload, AvailableUpdate};

/// 检查更新；`now` 是 Dart 侧 Unix 秒（传 0 使用 Rust 侧时间）。
///
/// 网络/验签/格式/防回滚问题都以 `state = unavailable` 返回，不阻止界面继续
/// 使用；`reason` 给出稳定原因（含 `rollback_rejected`）。
pub async fn update_check(now: i64) -> Result<UpdateCheckDto, BridgeError> {
    let app = state::app()?;
    let outcome = app.updates.checker.check().await;
    let checked_at = if now > 0 { now } else { outcome.checked_at };

    let (state_value, info, reason, available) = match outcome.check {
        UpdateCheck::UpToDate => (UpdateCheckStateDto::UpToDate, None, None, None),
        UpdateCheck::Available { manifest, forced } => {
            let dto = update_info(&manifest, forced);
            (
                UpdateCheckStateDto::Available,
                Some(dto),
                None,
                Some(AvailableUpdate { manifest }),
            )
        }
        UpdateCheck::DevelopmentBuild => (UpdateCheckStateDto::DevelopmentBuild, None, None, None),
        UpdateCheck::Unavailable { reason } => (
            UpdateCheckStateDto::Unavailable,
            None,
            Some(reason.as_str().to_owned()),
            None,
        ),
    };

    {
        let mut guard = app.updates.available()?;
        *guard = available;
    }

    Ok(UpdateCheckDto {
        state: state_value,
        info,
        checked_at,
        reason,
    })
}

/// 读取“用户已关闭横幅”的版本；用于每个版本只提示一次。
pub async fn update_banner_dismissed_version() -> Result<Option<String>, BridgeError> {
    let app = state::app()?;
    Ok(app.updates.checker.banner_dismissed_version().await?)
}

/// 记录用户已关闭某个版本的横幅。
pub async fn update_dismiss_banner(version: String) -> Result<(), BridgeError> {
    let app = state::app()?;
    Ok(app.updates.checker.dismiss_banner(&version).await?)
}

/// 下载并校验最近一次检查发现的新版本；返回下载 ID。
///
/// 进度通过 `update_progress_stream` 推送；同一时间只允许一个下载。
/// 下载完成后安装包路径保存在 Rust 侧，供 `update_install` 使用。
pub async fn update_download() -> Result<String, BridgeError> {
    let app = state::app()?;
    let update = {
        let guard = app.updates.available()?;
        guard
            .clone()
            .ok_or_else(|| BridgeError::new("no_update", "没有可下载的更新，请先检查更新"))?
    };
    let platform = UpdatePlatform::current()
        .ok_or_else(|| BridgeError::new("unsupported", "当前平台不支持自动安装更新"))?;

    let download_id = uuid::Uuid::new_v4().to_string();
    let cancel = UpdateCancellation::new();
    {
        let mut active = app.updates.active()?;
        if active.is_some() {
            return Err(BridgeError::new("download_in_progress", "已有更新正在下载"));
        }
        *active = Some(ActiveUpdateDownload {
            cancel: cancel.clone(),
        });
    }
    // 新下载开始后旧的“已校验文件”不再对应当前检查结果。
    {
        let mut verified = app.updates.verified()?;
        *verified = None;
    }

    state::spawn_update_download(app, download_id.clone(), update, platform, cancel);
    Ok(download_id)
}

/// 取消正在进行的更新下载；删除 `*.partial` 并推送 cancelled 终态事件。
pub async fn update_cancel_download() -> Result<(), BridgeError> {
    let app = state::app()?;
    let active = app.updates.active()?;
    let download = active
        .as_ref()
        .ok_or_else(|| BridgeError::new("no_download", "当前没有正在下载的更新"))?;
    download.cancel.cancel();
    Ok(())
}

/// 安装已下载并校验的更新。
///
/// 先停止备份引擎，再启动安装：
/// - Windows：静默安装包已在后台启动，Dart 收到 `started` 后应立即退出应用；
/// - macOS：替换脚本已启动，会等待当前进程退出后替换并重新打开；
/// - 无法写入当前应用目录时打开 `.dmg` 并返回 `manual_install_required`。
pub async fn update_install() -> Result<UpdateInstallOutcomeDto, BridgeError> {
    let app = state::app()?;
    let verified = {
        let guard = app.updates.verified()?;
        guard
            .clone()
            .ok_or_else(|| BridgeError::new("no_verified_update", "请先下载并校验更新"))?
    };

    app.engine.stop().await;

    match client_core::desktop::update::install_update(&verified) {
        Ok(InstallOutcome::Started) => Ok(UpdateInstallOutcomeDto {
            kind: UpdateInstallKindDto::Started,
        }),
        Ok(InstallOutcome::ManualInstallRequired) => {
            let opened = client_core::desktop::update::open_update_dmg(&verified).is_ok();
            Err(BridgeError::new(
                "manual_install_required",
                if opened {
                    "无法替换当前应用，安装包已打开，请把“加密备份”拖到“应用程序”"
                } else {
                    "无法替换当前应用，请打开下载页面手动安装"
                },
            ))
        }
        Err(error) => Err(error.into()),
    }
}

/// 打开已下载的 .dmg（macOS 手动安装退回方案）。
pub async fn update_open_dmg() -> Result<(), BridgeError> {
    let app = state::app()?;
    let verified = {
        let guard = app.updates.verified()?;
        guard
            .clone()
            .ok_or_else(|| BridgeError::new("no_verified_update", "请先下载并校验更新"))?
    };
    client_core::desktop::update::open_update_dmg(&verified).map_err(BridgeError::from)
}

/// 更新下载进度事件流。
pub fn update_progress_stream(sink: StreamSink<UpdateProgressDto>) {
    let Ok(app) = state::app() else {
        return;
    };
    let receiver = app.updates.subscribe_progress();
    state::forward_update_stream(sink, receiver);
}
