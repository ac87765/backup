//! 安装已校验的更新：Windows 静默安装，macOS 替换应用并重启。
//!
//! 安装函数只接受 [`VerifiedUpdate`]（大小与 SHA-256 已校验），从类型上避免
//! 运行未校验的文件。调用方必须先在合适的时机停止备份引擎。

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::UpdateError;
use super::download::VerifiedUpdate;

/// 安装动作的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// 安装程序 / 替换脚本已在后台启动。
    ///
    /// Windows：调用方应立即退出应用，由安装包在完成后重新启动；
    /// macOS：辅助脚本会等待当前进程退出后替换应用并重新打开。
    Started,
    /// 无法写入当前应用目录：调用方应打开 .dmg 让用户拖到“应用程序”。
    ManualInstallRequired,
}

/// Windows 安装命令（程序 + 参数），与真正的进程启动分开，便于测试。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsInstallCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// 构造 Windows 静默更新安装命令。
///
/// `/UPDATE=1` 让 `encrypted_backup.iss` 的 `InitializeSetup` 等待旧应用退出，
/// 并在安装完成后由 `[Run]` 重新启动应用。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[must_use]
pub fn windows_install_command(installer: &Path) -> WindowsInstallCommand {
    WindowsInstallCommand {
        program: installer.to_path_buf(),
        args: [
            "/SILENT",
            "/SUPPRESSMSGBOXES",
            "/NORESTART",
            "/CLOSEAPPLICATIONS",
            "/UPDATE=1",
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
    }
}

/// 安装已校验的更新。
pub fn install_update(verified: &VerifiedUpdate) -> Result<InstallOutcome, UpdateError> {
    #[cfg(target_os = "windows")]
    {
        install_windows(verified)
    }
    #[cfg(target_os = "macos")]
    {
        install_macos(verified)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = verified;
        Err(UpdateError::Unsupported)
    }
}

/// 打开已下载的 .dmg（macOS 无法自动替换时的退回方案）。
pub fn open_update_dmg(verified: &VerifiedUpdate) -> Result<(), UpdateError> {
    #[cfg(target_os = "macos")]
    {
        use std::process::{Command, Stdio};
        Command::new("open")
            .arg(&verified.path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| UpdateError::InstallFailed("无法打开安装包"))?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = verified;
        Err(UpdateError::Unsupported)
    }
}

/// 从可执行文件路径向上找到 `.app` 包。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[must_use]
pub fn app_bundle_from_exe(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|ancestor| {
            ancestor
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
        })
        .map(Path::to_path_buf)
}

/// 当前应用同目录下的暂存路径：`.<应用名>.app<suffix>`。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[must_use]
pub fn macos_staging_path(app: &Path, suffix: &str) -> Option<PathBuf> {
    let name = app.file_name()?;
    let mut staging = OsString::from(".");
    staging.push(name);
    staging.push(suffix);
    Some(app.with_file_name(staging))
}

/// 生成 macOS 替换应用的辅助脚本（只生成文本，不执行）。
///
/// 脚本在旧进程退出后：`mv` 当前应用为 `.old` → `mv` 新应用为正式名 →
/// 删除 `.old` → `open` 新应用；任何一步失败都会回滚并打开原应用。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[must_use]
pub fn macos_helper_script(pid: u32, target: &Path, new: &Path, old: &Path) -> String {
    let target = shell_quote(target);
    let new = shell_quote(new);
    let old = shell_quote(old);
    format!(
        r#"#!/bin/sh
# 由“加密备份”更新器生成：等待旧进程退出后替换应用并重新打开。
pid={pid}
target={target}
new={new}
old={old}
i=0
while kill -0 "$pid" 2>/dev/null; do
  sleep 0.2
  i=$((i + 1))
  if [ "$i" -ge 150 ]; then
    # 30 秒仍未退出：保持现有应用不动。
    open "$target"
    exit 1
  fi
done
if mv "$target" "$old"; then
  if mv "$new" "$target"; then
    rm -rf "$old"
    open "$target"
    exit 0
  fi
  mv "$old" "$target"
fi
open "$target"
exit 1
"#
    )
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn shell_quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(target_os = "windows")]
fn install_windows(verified: &VerifiedUpdate) -> Result<InstallOutcome, UpdateError> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    // DETACHED_PROCESS：安装包不继承控制台/句柄；安装包自己以 /SILENT 运行。
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    let command = windows_install_command(&verified.path);
    Command::new(&command.program)
        .args(&command.args)
        .creation_flags(DETACHED_PROCESS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| UpdateError::InstallFailed("无法启动安装程序"))?;
    Ok(InstallOutcome::Started)
}

#[cfg(target_os = "macos")]
fn install_macos(verified: &VerifiedUpdate) -> Result<InstallOutcome, UpdateError> {
    use std::process::{Command, Stdio};

    let current_exe =
        std::env::current_exe().map_err(|_| UpdateError::InstallFailed("无法定位当前应用"))?;
    let current_app = app_bundle_from_exe(&current_exe)
        .ok_or(UpdateError::InstallFailed("无法定位当前应用包"))?;
    let parent = current_app
        .parent()
        .ok_or(UpdateError::InstallFailed("应用目录不可用"))?;

    let mount_path =
        std::env::temp_dir().join(format!("encrypted-backup-update-{}", std::process::id()));
    std::fs::create_dir_all(&mount_path)?;
    let mut mount = MountGuard::new(mount_path);
    mount.attach(&verified.path)?;

    let new_app = find_single_app(&mount.path)?;
    let new_bundle = bundle_identifier(&new_app)?;
    let current_bundle = bundle_identifier(&current_app)?;
    if new_bundle != current_bundle {
        return Err(UpdateError::InstallFailed("安装包中的应用与当前应用不一致"));
    }

    if !directory_is_writable(parent) {
        return Ok(InstallOutcome::ManualInstallRequired);
    }

    let target_new = macos_staging_path(&current_app, ".new")
        .ok_or(UpdateError::InstallFailed("应用路径无效"))?;
    let target_old = macos_staging_path(&current_app, ".old")
        .ok_or(UpdateError::InstallFailed("应用路径无效"))?;
    remove_path(&target_new)?;

    let copied = Command::new("ditto")
        .arg(&new_app)
        .arg(&target_new)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !copied {
        remove_path(&target_new)?;
        return Ok(InstallOutcome::ManualInstallRequired);
    }

    mount.detach()?;

    let script = macos_helper_script(std::process::id(), &current_app, &target_new, &target_old);
    let spawned = Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if spawned.is_err() {
        remove_path(&target_new)?;
        return Err(UpdateError::InstallFailed("无法启动更新脚本"));
    }
    Ok(InstallOutcome::Started)
}

#[cfg(target_os = "macos")]
struct MountGuard {
    path: PathBuf,
    attached: bool,
}

#[cfg(target_os = "macos")]
impl MountGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            attached: false,
        }
    }

    fn attach(&mut self, dmg: &Path) -> Result<(), UpdateError> {
        use std::process::{Command, Stdio};
        let status = Command::new("hdiutil")
            .arg("attach")
            .arg("-nobrowse")
            .arg("-readonly")
            .arg("-mountpoint")
            .arg(&self.path)
            .arg(dmg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| UpdateError::InstallFailed("无法调用 hdiutil"))?;
        if !status.success() {
            return Err(UpdateError::InstallFailed("无法挂载安装包"));
        }
        self.attached = true;
        Ok(())
    }

    fn detach(&mut self) -> Result<(), UpdateError> {
        use std::process::{Command, Stdio};
        if !self.attached {
            return Ok(());
        }
        let status = Command::new("hdiutil")
            .arg("detach")
            .arg(&self.path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| UpdateError::InstallFailed("无法调用 hdiutil"))?;
        self.attached = false;
        if !status.success() {
            return Err(UpdateError::InstallFailed("无法卸载安装包"));
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
impl Drop for MountGuard {
    fn drop(&mut self) {
        use std::process::{Command, Stdio};
        if self.attached {
            let _ = Command::new("hdiutil")
                .arg("detach")
                .arg(&self.path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[cfg(target_os = "macos")]
fn find_single_app(mount: &Path) -> Result<PathBuf, UpdateError> {
    let mut apps = Vec::new();
    for entry in std::fs::read_dir(mount)? {
        let entry = entry?;
        let path = entry.path();
        let is_app = path.is_dir()
            && path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| extension.eq_ignore_ascii_case("app"));
        if is_app {
            apps.push(path);
        }
    }
    if apps.len() != 1 {
        return Err(UpdateError::InstallFailed("安装包中应恰好包含一个应用"));
    }
    apps.pop()
        .ok_or(UpdateError::InstallFailed("安装包中应恰好包含一个应用"))
}

#[cfg(target_os = "macos")]
fn bundle_identifier(app: &Path) -> Result<String, UpdateError> {
    use std::process::Command;
    let plist = app.join("Contents").join("Info.plist");
    let output = Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg("Print :CFBundleIdentifier")
        .arg(&plist)
        .output()
        .map_err(|_| UpdateError::InstallFailed("无法读取应用标识"))?;
    if !output.status.success() {
        return Err(UpdateError::InstallFailed("无法读取应用标识"));
    }
    let identifier = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if identifier.is_empty() {
        return Err(UpdateError::InstallFailed("无法读取应用标识"));
    }
    Ok(identifier)
}

#[cfg(target_os = "macos")]
fn directory_is_writable(directory: &Path) -> bool {
    let probe = directory.join(format!(
        ".encrypted-backup-write-test-{}",
        std::process::id()
    ));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
fn remove_path(path: &Path) -> Result<(), UpdateError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path).map_err(Into::into),
        Ok(_) => std::fs::remove_file(path).map_err(Into::into),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
