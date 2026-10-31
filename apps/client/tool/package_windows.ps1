# 加密备份 Windows 安装包打包脚本（Inno Setup 6）。
#
# 一条命令完成：
#   1. flutter build windows --release（注入 APP_VERSION / GIT_COMMIT）
#   2. ISCC 编译 windows/installer/encrypted_backup.iss
#
# 用法（在仓库任意目录）：
#   pwsh apps/client/tool/package_windows.ps1
#   pwsh apps/client/tool/package_windows.ps1 -Version 1.2.3 -Commit 0123abc
#   pwsh apps/client/tool/package_windows.ps1 -SkipBuild   # CI 已构建时只打包
#
# 产物：apps/client/build/installer/加密备份-<版本>-windows-x64.exe
#
# 本脚本只读取仓库、写 apps/client/build/ 与 ISCC 的临时输出；不安装应用、不写
# 注册表、不修改任何系统设置。

[CmdletBinding()]
param(
    # 覆盖 APP_VERSION（默认 git describe --tags --always --dirty）。
    [string]$Version,
    # 覆盖 GIT_COMMIT（默认 git rev-parse --short HEAD）。
    [string]$Commit,
    # Inno Setup 编译器路径；为空时依次查 PATH、%LOCALAPPDATA%、%ProgramFiles(x86)%。
    [string]$IsccPath,
    # 跳过 flutter build，直接使用已有的 Release 目录（CI 用）。
    [switch]$SkipBuild,
    # 可选的 SERVER_URL 编译期常量；为空时使用 app_config.dart 的默认生产地址。
    [string]$ServerUrl
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$scriptDir = $PSScriptRoot
$clientRoot = (Resolve-Path (Join-Path $scriptDir '..')).Path
$repoRoot = (Resolve-Path (Join-Path $clientRoot '..\..')).Path
$issPath = Join-Path $clientRoot 'windows\installer\encrypted_backup.iss'
$releaseDir = Join-Path $clientRoot 'build\windows\x64\runner\Release'
$installerDir = Join-Path $clientRoot 'build\installer'

function Invoke-Git {
    param([Parameter(Mandatory = $true)][string[]]$GitArguments)

    $value = & git -C $repoRoot @GitArguments 2>$null
    if ($LASTEXITCODE -ne 0) {
        throw "git $($GitArguments -join ' ') 失败（退出码 $LASTEXITCODE）。请在完整仓库中运行。"
    }
    $text = ($value | Out-String).Trim()
    if ([string]::IsNullOrWhiteSpace($text)) {
        throw "git $($GitArguments -join ' ') 没有输出。"
    }
    return $text
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = Invoke-Git -GitArguments @('describe', '--tags', '--always', '--dirty')
}
if ([string]::IsNullOrWhiteSpace($Commit)) {
    $Commit = Invoke-Git -GitArguments @('rev-parse', '--short', 'HEAD')
}

# Windows 文件名不能包含 : / \ 等字符；git 标签允许这些字符，做一次替换。
$fileVersion = ($Version -replace '[\\/:*?"<>|\x00-\x1f]', '-') -replace '\s+', '-'
$fileVersion = $fileVersion.Trim().TrimEnd('.')
if ([string]::IsNullOrWhiteSpace($fileVersion)) {
    throw "版本号 '$Version' 不能用于文件名。"
}

if (-not $SkipBuild) {
    $flutter = Get-Command flutter -ErrorAction SilentlyContinue
    if ($null -eq $flutter) {
        throw '找不到 flutter 命令。请先安装 Flutter 3.47.7 并加入 PATH，或用 -SkipBuild 使用已有 Release 目录。'
    }
    $dartDefines = @(
        "--dart-define=APP_VERSION=$Version",
        "--dart-define=GIT_COMMIT=$Commit"
    )
    if (-not [string]::IsNullOrWhiteSpace($ServerUrl)) {
        $dartDefines += "--dart-define=SERVER_URL=$ServerUrl"
    }

    Write-Host "构建 Windows Release：APP_VERSION=$Version GIT_COMMIT=$Commit"
    Push-Location $clientRoot
    try {
        & flutter build windows --release @dartDefines
        if ($LASTEXITCODE -ne 0) {
            throw "flutter build windows --release 失败（退出码 $LASTEXITCODE）。"
        }
    }
    finally {
        Pop-Location
    }
}

if (-not (Test-Path -LiteralPath $releaseDir -PathType Container)) {
    throw "找不到 Release 目录：$releaseDir。请先运行 flutter build windows --release（或去掉 -SkipBuild）。"
}
if (-not (Test-Path -LiteralPath $issPath -PathType Leaf)) {
    throw "找不到安装脚本：$issPath。"
}

function Resolve-Iscc {
    param([string]$ExplicitPath)

    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        if (-not (Test-Path -LiteralPath $ExplicitPath -PathType Leaf)) {
            throw "指定的 ISCC 不存在：$ExplicitPath"
        }
        return (Resolve-Path -LiteralPath $ExplicitPath).Path
    }

    $command = Get-Command 'ISCC.exe' -ErrorAction SilentlyContinue
    if ($null -ne $command) {
        return $command.Source
    }

    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    throw '找不到 Inno Setup 6 的 ISCC.exe。请安装 Inno Setup 6，或用 -IsccPath 指定编译器路径。'
}

$iscc = Resolve-Iscc -ExplicitPath $IsccPath
New-Item -ItemType Directory -Force -Path $installerDir | Out-Null

Write-Host "编译安装包：$issPath"
& $iscc "/DAppVersion=$Version" "/DAppFileVersion=$fileVersion" $issPath
if ($LASTEXITCODE -ne 0) {
    throw "Inno Setup 编译失败（退出码 $LASTEXITCODE）。"
}

$output = Join-Path $installerDir "加密备份-$fileVersion-windows-x64.exe"
if (-not (Test-Path -LiteralPath $output -PathType Leaf)) {
    throw "ISCC 返回成功，但找不到预期产物：$output"
}

Write-Host "安装包：$output"
