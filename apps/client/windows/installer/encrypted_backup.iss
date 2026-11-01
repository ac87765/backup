; 加密备份 Windows 安装脚本（Inno Setup 6）
;
; 由 apps/client/tool/package_windows.ps1 调用，也可手动编译：
;   ISCC.exe /DAppVersion=1.2.3 /DAppFileVersion=1.2.3 encrypted_backup.iss
;
; 设计约束：
; - 按用户安装（PrivilegesRequired=lowest），默认装到当前用户的 Programs 目录，
;   不需要管理员权限；
; - 卸载只删除程序文件与开机启动的 HKCU Run 值，不删除应用数据与系统凭据；
; - AppMutex 与 windows/runner/main.cpp 的单实例 Mutex 完全一致，安装 / 卸载时
;   若应用仍在运行，Inno Setup 会让用户先退出。
;
; 本文件使用 UTF-8 with BOM 保存（Inno Setup 需要 BOM 才能可靠识别中文）。

#define MyAppName "加密备份"
#define MyAppExeName "encrypted_backup_client.exe"
#define MyBuildDir "..\..\build\windows\x64\runner\Release"

; 版本号由打包脚本通过 /DAppVersion=... 注入；直接运行 ISCC 时给一个可读的占位值。
#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
; 文件名用的版本号（去掉 Windows 文件名非法字符），由打包脚本注入。
#ifndef AppFileVersion
  #define AppFileVersion AppVersion
#endif

[Setup]
; AppId 永久固定：升级时 Inno Setup 依靠它识别同一个应用。发布后永不更改，
; 否则会被当成另一个应用，产生重复的“应用和功能”条目与两套安装目录。
AppId={{995C1123-307A-4784-8268-7E2ED62CE3F6}
AppName={#MyAppName}
AppVersion={#AppVersion}
AppVerName={#MyAppName} {#AppVersion}
AppPublisher=Encrypted Backup
; PrivilegesRequired=lowest 时 {autopf} 解析为当前用户的 Programs 目录
; （%LOCALAPPDATA%\Programs），不需要管理员权限。
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
; 产物是 x64，拒绝在不支持的架构上安装。
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; 与 apps/client/windows/runner/main.cpp 中 kSingleInstanceMutex 的值一致。
AppMutex=Local\EncryptedBackupClient.SingleInstance
CloseApplications=yes
RestartApplications=no
OutputDir=..\..\build\installer
OutputBaseFilename={#MyAppName}-{#AppFileVersion}-windows-x64
SetupIconFile=..\runner\resources\app_icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
; 使用同目录的简体中文翻译（来自 jrsoftware/issrc is-6_7_3 标签的 Files/Languages/Unofficial，非官方维护）。
Name: "chinesesimp"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式(&D)"; GroupDescription: "附加任务："; Flags: unchecked

[Files]
; Release 目录的全部内容（exe、DLL、data\ 等）。
Source: "{#MyBuildDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
; 安装完成页的可选“立即启动”（静默安装不启动）。
Filename: "{app}\{#MyAppExeName}"; Description: "立即启动{#MyAppName}"; Flags: nowait postinstall skipifsilent
; 更新模式（/UPDATE=1）：安装完成后重新启动客户端。静默安装也要执行，
; 因为更新流程会先退出应用；见 [Code] 中 IsUpdateMode / InitializeSetup 的说明。
Filename: "{app}\{#MyAppExeName}"; Flags: nowait skipifnotsilent; Check: IsUpdateMode

[Code]
const
  StartupRunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  // 与 lib/desktop/startup/startup_service.dart 中 launchAtStartup.setup(appName)
  // 及 launch_at_startup 的 Windows 实现一致：HKCU Run 的值名就是“加密备份”。
  StartupValueName = '加密备份';
  // 与 [Setup] 的 AppMutex 及 windows/runner/main.cpp 的单实例 Mutex 一致。
  AppMutexName = 'Local\EncryptedBackupClient.SingleInstance';
  // 客户端自动更新时传给安装包的标志。
  UpdateModeFlag = '/UPDATE=1';

function IsUpdateMode: Boolean;
var
  Index: Integer;
begin
  Result := False;
  for Index := 1 to ParamCount do
  begin
    if CompareText(ParamStr(Index), UpdateModeFlag) = 0 then
    begin
      Result := True;
      Exit;
    end;
  end;
end;

// 静默模式（/SILENT）下，Inno 遇到“应用正在运行”的提示默认按“取消”处理，
// 所以更新安装不能依赖 CloseApplications：先循环等待旧客户端释放 AppMutex
// （每 250 ms 检查一次，最多 30 秒），超时则安静地取消安装并写安装日志。
function InitializeSetup(): Boolean;
var
  Waited: Integer;
begin
  Result := True;
  if not IsUpdateMode then
    Exit;

  Waited := 0;
  while CheckForMutexes(AppMutexName) do
  begin
    if Waited >= 120 then
    begin
      Log('更新安装：等待应用退出超过 30 秒，已取消静默安装。');
      Result := False;
      Exit;
    end;
    Sleep(250);
    Waited := Waited + 1;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    // 只删除本应用写入的开机启动值，不删除整个 Run 键，也不影响其他程序。
    // launch_at_startup 还会写 StartupApproved\Run 的同名值；那是 Windows 的
    // “启动应用”开关状态，保留它不会让程序继续开机启动。
    RegDeleteValue(HKEY_CURRENT_USER, StartupRunKey, StartupValueName);
  end
  else if CurUninstallStep = usPostUninstall then
  begin
    // 卸载不删除用户数据与系统凭据，必须明确告知手动清除方法。
    MsgBox(
      '卸载完成。' + #13#10 + #13#10 +
      '为保护你的备份配置和登录状态，卸载程序不会删除：' + #13#10 +
      '- 应用数据目录：%APPDATA%\Encrypted Backup\加密备份' + #13#10 +
      '  （client-state.sqlite3 本地状态库与 desktop-settings.json 设置）' + #13#10 +
      '- Windows 凭据管理器中的系统凭据：' + #13#10 +
      '  EncryptedBackup/session-token 与 EncryptedBackup/master-key' + #13#10 + #13#10 +
      '如需彻底清除：先在应用内“退出登录”（会删除系统凭据），再删除上述数据目录。' + #13#10 +
      '如果应用已经卸载，可在“控制面板 → 凭据管理器 → Windows 凭据 → 普通凭据”中' + #13#10 +
      '手动删除这两个 EncryptedBackup/ 开头的条目。' + #13#10 + #13#10 +
      '开机启动项已删除。',
      mbInformation, MB_OK);
  end;
end;
