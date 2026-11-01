#Requires -Version 7.0
<#
.SYNOPSIS
    发布带签名的桌面客户端：取回标签 CI 产物、生成并签名 latest.json、上传到服务器。

.DESCRIPTION
    在维护者的 Windows 电脑上运行（PowerShell 7）。流程见
    openspec/changes/add-client-releases/design.md 的 D2/D3：
      1. 校验标签与本地 git 标签；
      2. 用 gh 取回该标签成功的 Windows / macOS 安装包；
      3. 从服务器取回上一份 latest.json（没有则视为首个版本）；
      4. cargo run -p release-tool：生成清单 → 私钥签名 → 公钥验签并复核安装包；
      5. scp 上传安装包与临时清单，远端校验 SHA-256，最后原子替换 sig/json；
      6. 按 semver 只保留最近 3 个版本目录。

    私钥只以路径形式传给 release-tool，本脚本不读取、不打印、不复制私钥内容。

.PARAMETER Tag
    已推送的标签，格式 vX.Y.Z。

.PARAMETER Notes
    更新说明文本文件，UTF-8，每行一条。

.PARAMETER MinSupported
    最低支持版本；省略时等于上一份清单的 min_supported，没有上一份时等于本版本。

.PARAMETER Previous
    可选的本地上一份 latest.json（DryRun 或离线演练用）；省略时从服务器取回。

.PARAMETER Artifacts
    可选的本地目录，直接使用其中的 .exe 与 .dmg（DryRun 必需，也便于离线演练）。

.PARAMETER PublicKey
    可选的自检公钥路径；省略时使用 release-tool 内置的发布公钥。轮换或 DryRun 用测试密钥时需要。

.PARAMETER DryRun
    执行到本地自检为止，打印将要执行的远端命令，不连接服务器、不调用 gh。
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,

    [Parameter(Mandatory = $true)]
    [string]$Notes,

    [string]$MinSupported,

    [string]$Server = 'root@120.79.245.99',

    [string]$SshKey = '~/.ssh/id_dsh',

    [string]$Key = (Join-Path $env:USERPROFILE '.backup-release\release-signing.key'),

    [string]$Previous,

    [string]$Artifacts,

    [string]$PublicKey,

    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Stop-Release {
    param([Parameter(Mandatory = $true)][string]$Message)
    throw $Message
}

function Write-Step {
    param([Parameter(Mandatory = $true)][string]$Message)
    Write-Host "`n== $Message"
}

function Expand-HomePath {
    param([Parameter(Mandatory = $true)][string]$Path)
    if ($Path -eq '~') { return $HOME }
    if ($Path.StartsWith('~/') -or $Path.StartsWith('~\')) {
        return (Join-Path $HOME $Path.Substring(2))
    }
    return $Path
}

function Get-FullPath {
    param([Parameter(Mandatory = $true)][string]$Path)
    return [System.IO.Path]::GetFullPath((Expand-HomePath -Path $Path))
}

function Get-SingleArtifact {
    param(
        [Parameter(Mandatory = $true)][string]$Directory,
        [Parameter(Mandatory = $true)][string]$Extension,
        [Parameter(Mandatory = $true)][string]$Label
    )
    $files = @(
        Get-ChildItem -LiteralPath $Directory -File -Recurse |
            Where-Object { $_.Extension -ieq $Extension }
    )
    if ($files.Count -ne 1) {
        Stop-Release "$Label 应恰好包含 1 个 $Extension 文件，实际 $($files.Count) 个：$Directory"
    }
    return $files[0]
}

function Invoke-Remote {
    param([Parameter(Mandatory = $true)][string]$Command)
    & ssh @sshOptions $Server $Command
    if ($LASTEXITCODE -ne 0) {
        Stop-Release "远端命令失败（退出码 $LASTEXITCODE）：$Command"
    }
}

function Send-File {
    param(
        [Parameter(Mandatory = $true)][string]$LocalPath,
        [Parameter(Mandatory = $true)][string]$RemotePath
    )
    & scp @sshOptions $LocalPath ("{0}:{1}" -f $Server, $RemotePath)
    if ($LASTEXITCODE -ne 0) {
        Stop-Release "scp 上传失败（退出码 $LASTEXITCODE）：$LocalPath → $RemotePath"
    }
}

function Show-Remote {
    param([Parameter(Mandatory = $true)][string]$Command)
    Write-Host ("  ssh {0} `"{1}`"" -f $Server, ($Command -replace '"', '\"'))
}

function Show-Scp {
    param(
        [Parameter(Mandatory = $true)][string]$LocalPath,
        [Parameter(Mandatory = $true)][string]$RemotePath
    )
    Write-Host ("  scp `"{0}`" `"{1}:{2}`"" -f $LocalPath, $Server, $RemotePath)
}

# ---- 预检：仓库、标签、私钥位置、SSH 密钥、更新说明 ----

$scriptDir = $PSScriptRoot
$repoRootRaw = & git -C (Join-Path $scriptDir '..') rev-parse --show-toplevel
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace(($repoRootRaw | Out-String))) {
    Stop-Release '无法定位 git 仓库根目录；请在仓库内的 deploy/ 目录运行本脚本。'
}
$repoRoot = Get-FullPath -Path (($repoRootRaw | Select-Object -First 1).Trim())

$keyFull = Get-FullPath -Path $Key
$repoPrefix = $repoRoot.TrimEnd([char[]]@('\', '/')) +
    [System.IO.Path]::DirectorySeparatorChar
if ($keyFull.Equals($repoRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
    $keyFull.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    Stop-Release "发布私钥不能位于仓库目录内：$keyFull"
}
if (-not (Test-Path -LiteralPath $keyFull -PathType Leaf)) {
    Stop-Release "找不到发布私钥：$keyFull（真实密钥不得进入仓库）"
}

$sshKeyFull = Get-FullPath -Path $SshKey
if (-not $DryRun -and -not (Test-Path -LiteralPath $sshKeyFull -PathType Leaf)) {
    Stop-Release "找不到 SSH 私钥：$sshKeyFull"
}
$sshOptions = @('-i', $sshKeyFull, '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=20')

if ($Tag -notmatch '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
    Stop-Release "标签格式必须为 vX.Y.Z（不含预发布/构建元数据）：$Tag"
}
$version = $Tag.Substring(1)
$localTags = @(& git -C $repoRoot tag --list $Tag)
if ($LASTEXITCODE -ne 0) {
    Stop-Release 'git tag 失败；请在完整仓库中运行。'
}
if (-not ($localTags -ccontains $Tag)) {
    if ($DryRun) {
        # DryRun 不访问 CI；标签只在正式取产物时需要。仍要求格式正确，便于演练。
        Write-Warning "DryRun：本地没有标签 $Tag；正式发布前必须先创建并推送该标签。"
    }
    else {
        Stop-Release "本地不存在标签 $Tag；请先创建并推送标签。"
    }
}

$notesFull = Get-FullPath -Path $Notes
if (-not (Test-Path -LiteralPath $notesFull -PathType Leaf)) {
    Stop-Release "找不到更新说明文件：$notesFull"
}

$publicKeyFull = $null
if (-not [string]::IsNullOrWhiteSpace($PublicKey)) {
    $publicKeyFull = Get-FullPath -Path $PublicKey
    if (-not (Test-Path -LiteralPath $publicKeyFull -PathType Leaf)) {
        Stop-Release "找不到自检公钥：$publicKeyFull"
    }
}

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('backup-release-' + [Guid]::NewGuid().ToString('N'))
$latestPath = Join-Path $tempRoot 'latest.json'
$sigPath = Join-Path $tempRoot 'latest.json.sig'
$artifactDir = Join-Path $tempRoot 'artifacts'

try {
    New-Item -ItemType Directory -Path $tempRoot | Out-Null
    New-Item -ItemType Directory -Path $artifactDir | Out-Null

    $windowsName = "EncryptedBackup-$version-windows-x64.exe"
    $macosName = "EncryptedBackup-$version-macos.dmg"
    $windowsPath = Join-Path $artifactDir $windowsName
    $macosPath = Join-Path $artifactDir $macosName

    # ---- 取回安装包 ----
    if (-not [string]::IsNullOrWhiteSpace($Artifacts)) {
        Write-Step "使用本地安装包目录：$Artifacts"
        $artifactSource = Get-FullPath -Path $Artifacts
        if (-not (Test-Path -LiteralPath $artifactSource -PathType Container)) {
            Stop-Release "本地安装包目录不存在：$artifactSource"
        }
        $windowsSource = Get-SingleArtifact -Directory $artifactSource -Extension '.exe' -Label '本地安装包目录'
        $macosSource = Get-SingleArtifact -Directory $artifactSource -Extension '.dmg' -Label '本地安装包目录'
    }
    elseif ($DryRun) {
        Stop-Release 'DryRun 不访问 CI，需要 -Artifacts <目录> 提供恰好一个 .exe 与一个 .dmg。'
    }
    else {
        if ($null -eq (Get-Command gh -ErrorAction SilentlyContinue)) {
            Stop-Release '找不到 gh；请安装 GitHub CLI 并先运行 gh auth login。'
        }
        Write-Step "取回标签 $Tag 的 CI 产物"
        Push-Location -LiteralPath $repoRoot
        try {
            $runsJson = & gh run list --workflow client.yml --branch $Tag --limit 50 `
                --json databaseId,headBranch,status,conclusion,createdAt
            if ($LASTEXITCODE -ne 0) {
                Stop-Release "gh run list 失败（退出码 $LASTEXITCODE）；请确认已 gh auth login。"
            }
            $runsText = ($runsJson | Out-String).Trim()
            if ([string]::IsNullOrWhiteSpace($runsText)) {
                Stop-Release "标签 $Tag 没有找到 client.yml 运行。"
            }
            $runs = @($runsText | ConvertFrom-Json)
            $run = $runs |
                Where-Object { $_.headBranch -eq $Tag -and $_.conclusion -eq 'success' } |
                Sort-Object -Property createdAt -Descending |
                Select-Object -First 1
            if ($null -eq $run) {
                Stop-Release "标签 $Tag 没有成功的 client.yml 运行；请等待 Windows 与 macOS 构建成功后再发布。"
            }
            Write-Host "使用运行 $($run.databaseId)（$($run.createdAt)）"

            $windowsDownload = Join-Path $tempRoot 'gh-windows'
            $macosDownload = Join-Path $tempRoot 'gh-macos'
            & gh run download $run.databaseId -n encrypted-backup-windows -D $windowsDownload
            if ($LASTEXITCODE -ne 0) {
                Stop-Release "下载 Windows 产物失败（退出码 $LASTEXITCODE）。"
            }
            & gh run download $run.databaseId -n encrypted-backup-macos -D $macosDownload
            if ($LASTEXITCODE -ne 0) {
                Stop-Release "下载 macOS 产物失败（退出码 $LASTEXITCODE）。"
            }
        }
        finally {
            Pop-Location
        }
        $windowsSource = Get-SingleArtifact -Directory $windowsDownload -Extension '.exe' -Label 'Windows 产物'
        $macosSource = Get-SingleArtifact -Directory $macosDownload -Extension '.dmg' -Label 'macOS 产物'
    }

    Copy-Item -LiteralPath $windowsSource.FullName -Destination $windowsPath
    Copy-Item -LiteralPath $macosSource.FullName -Destination $macosPath
    Write-Host "Windows：$windowsName"
    Write-Host "macOS：  $macosName"

    # ---- 上一份清单与最低支持版本 ----
    $previousPath = $null
    if (-not [string]::IsNullOrWhiteSpace($Previous)) {
        $previousSource = Get-FullPath -Path $Previous
        if (-not (Test-Path -LiteralPath $previousSource -PathType Leaf)) {
            Stop-Release "找不到本地上一份清单：$previousSource"
        }
        $previousPath = Join-Path $tempRoot 'previous.json'
        Copy-Item -LiteralPath $previousSource -Destination $previousPath
        Write-Host "上一份清单：$previousSource（本地）"
    }
    elseif (-not $DryRun) {
        Write-Step '取回服务器上的上一份清单'
        $remoteState = (& ssh @sshOptions $Server `
                "if [ -f '/opt/backup/downloads/latest.json' ]; then echo present; else echo absent; fi") -join "`n"
        if ($LASTEXITCODE -ne 0) {
            Stop-Release "连接服务器失败（退出码 $LASTEXITCODE）；请检查网络与 SSH 密钥。"
        }
        switch ($remoteState.Trim()) {
            'present' {
                $previousPath = Join-Path $tempRoot 'previous.json'
                & scp @sshOptions ("{0}:/opt/backup/downloads/latest.json" -f $Server) $previousPath
                if ($LASTEXITCODE -ne 0) {
                    Stop-Release "取回服务器上的 latest.json 失败（退出码 $LASTEXITCODE）。"
                }
                Write-Host '上一份清单：服务器 /opt/backup/downloads/latest.json'
            }
            'absent' {
                $previousPath = $null
                Write-Host '服务器上还没有 latest.json，按首个版本处理。'
            }
            default {
                Stop-Release "无法判断服务器上的 latest.json：$remoteState"
            }
        }
    }

    if (-not [string]::IsNullOrWhiteSpace($MinSupported)) {
        $minSupportedValue = $MinSupported.Trim()
    }
    elseif ($null -ne $previousPath) {
        $previousObject = Get-Content -LiteralPath $previousPath -Raw | ConvertFrom-Json
        $minSupportedValue = [string]$previousObject.min_supported
        if ([string]::IsNullOrWhiteSpace($minSupportedValue)) {
            Stop-Release '上一份清单缺少 min_supported；请用 -MinSupported 显式指定。'
        }
    }
    else {
        $minSupportedValue = $version
    }
    if ($minSupportedValue -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
        Stop-Release "最低支持版本必须为 X.Y.Z：$minSupportedValue"
    }

    $releasedAt = [DateTimeOffset]::UtcNow.ToString(
        "yyyy-MM-dd'T'HH:mm:ss'Z'",
        [System.Globalization.CultureInfo]::InvariantCulture
    )

    # ---- 生成清单、签名、本地自检 ----
    if ($null -eq (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Stop-Release '找不到 cargo；请在 Rust 工具链已安装的环境中运行。'
    }

    Push-Location -LiteralPath $repoRoot
    try {
        Write-Step '生成并签名清单'
        $manifestArguments = @(
            'run', '-q', '-p', 'release-tool', '--release', '--', 'manifest',
            '--version', $version,
            '--released-at', $releasedAt,
            '--min-supported', $minSupportedValue,
            '--notes', $notesFull,
            '--windows', $windowsPath,
            '--macos', $macosPath,
            '--out', $latestPath
        )
        if ($null -ne $previousPath) {
            $manifestArguments += @('--previous', $previousPath)
        }
        & cargo @manifestArguments
        if ($LASTEXITCODE -ne 0) {
            Stop-Release "release-tool manifest 失败（退出码 $LASTEXITCODE）。"
        }

        $signArguments = @(
            'run', '-q', '-p', 'release-tool', '--release', '--', 'sign',
            '--key', $keyFull,
            '--in', $latestPath,
            '--out', $sigPath
        )
        & cargo @signArguments
        if ($LASTEXITCODE -ne 0) {
            Stop-Release "release-tool sign 失败（退出码 $LASTEXITCODE）。"
        }

        Write-Step '本地自检：验签并复核安装包大小与 SHA-256'
        $verifyArguments = @('run', '-q', '-p', 'release-tool', '--release', '--', 'verify')
        if ($null -ne $publicKeyFull) {
            $verifyArguments += @('--pub', $publicKeyFull)
        }
        $verifyArguments += @(
            '--in', $latestPath,
            '--sig', $sigPath,
            '--files', $artifactDir
        )
        & cargo @verifyArguments
        if ($LASTEXITCODE -ne 0) {
            Stop-Release "release-tool verify 失败（退出码 $LASTEXITCODE）。"
        }
    }
    finally {
        Pop-Location
    }

    $localManifest = Get-Content -LiteralPath $latestPath -Raw | ConvertFrom-Json
    $windowsSha = [string]$localManifest.platforms.'windows-x64'.sha256
    $macosSha = [string]$localManifest.platforms.'macos-universal'.sha256

    $remoteVersionDir = "/opt/backup/downloads/$version"
    $remoteWindows = "$remoteVersionDir/$windowsName"
    $remoteMacos = "$remoteVersionDir/$macosName"
    $remoteLatestTmp = '/opt/backup/downloads/latest.json.tmp'
    $remoteSigTmp = '/opt/backup/downloads/latest.json.sig.tmp'
    $remoteLatest = '/opt/backup/downloads/latest.json'
    $remoteSig = '/opt/backup/downloads/latest.json.sig'

    $publishCommand = "mv -f '$remoteSigTmp' '$remoteSig' && " +
        "mv -f '$remoteLatestTmp' '$remoteLatest' && " +
        "chmod 644 '$remoteLatest' '$remoteSig'"

    if ($DryRun) {
        Write-Step 'DryRun：以下远端命令不会执行'
        Show-Remote "install -d -o root -g root -m 755 '$remoteVersionDir'"
        Show-Scp $windowsPath $remoteWindows
        Show-Scp $macosPath $remoteMacos
        Show-Remote "cd '$remoteVersionDir' && sha256sum -- '$windowsName' '$macosName'"
        Show-Scp $latestPath $remoteLatestTmp
        Show-Scp $sigPath $remoteSigTmp
        # 客户端先读 latest.json 再读 latest.json.sig。先替换 sig 再替换 json 的极短
        # 窗口里，可能读到“旧 json + 新 sig”：验签失败。客户端把任何验签/格式失败
        # 当作“暂时无法检查更新”，不提示、不下载，下一轮重试；不会安装未签名内容。
        # 反过来先 json 后 sig 也有同类窗口；这里按 design D3 固定为 sig 先、json 后。
        Show-Remote $publishCommand
        Show-Remote "find '/opt/backup/downloads' -mindepth 1 -maxdepth 1 -type d -printf '%f\n'"
        Write-Host '  # 上一条命令的输出按 semver 排序，对不在最新 3 个中的每个版本目录执行：'
        Show-Remote "rm -rf -- '/opt/backup/downloads/<旧版本>'"
        Write-Host 'DryRun 完成：本地自检已通过，未连接服务器、未调用 gh。'
        return
    }

    # ---- 上传安装包并校验远端 SHA-256 ----
    Write-Step "创建服务器目录 $remoteVersionDir"
    Invoke-Remote "install -d -o root -g root -m 755 '$remoteVersionDir'"
    Send-File $windowsPath $remoteWindows
    Send-File $macosPath $remoteMacos

    Write-Step '校验远端安装包 SHA-256'
    $hashOutput = & ssh @sshOptions $Server `
        "cd '$remoteVersionDir' && sha256sum -- '$windowsName' '$macosName'"
    if ($LASTEXITCODE -ne 0) {
        Stop-Release "远端 sha256sum 失败（退出码 $LASTEXITCODE）。"
    }
    $remoteHashes = @{}
    foreach ($line in @($hashOutput)) {
        $text = ([string]$line).Trim()
        if ($text -match '^([0-9a-f]{64})\s+\*?(.+)$') {
            $remoteHashes[$Matches[2].Trim()] = $Matches[1]
        }
    }
    $expectedHashes = @{}
    $expectedHashes[$windowsName] = $windowsSha
    $expectedHashes[$macosName] = $macosSha
    foreach ($name in $expectedHashes.Keys) {
        if (-not $remoteHashes.ContainsKey($name)) {
            Stop-Release "远端 sha256sum 输出缺少 $name。"
        }
        if ($remoteHashes[$name] -ne $expectedHashes[$name]) {
            Stop-Release "远端 $name 的 SHA-256 与本地清单不一致；未更新 latest.json。"
        }
    }
    Write-Host '远端安装包哈希一致。'

    # ---- 上传临时文件并原子替换：先 sig 再 json ----
    Write-Step '上传最新清单并替换'
    Send-File $latestPath $remoteLatestTmp
    Send-File $sigPath $remoteSigTmp
    Invoke-Remote $publishCommand

    # ---- 按 semver 保留最近 3 个版本 ----
    Write-Step '清理旧版本目录（按 semver，不按修改时间）'
    $listing = & ssh @sshOptions $Server `
        "find '/opt/backup/downloads' -mindepth 1 -maxdepth 1 -type d -printf '%f\n'"
    if ($LASTEXITCODE -ne 0) {
        Stop-Release "列出版本目录失败（退出码 $LASTEXITCODE）。"
    }
    $semverPattern = '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'
    $versions = @(
        $listing |
            ForEach-Object { ([string]$_).Trim() } |
            Where-Object { $_ -match $semverPattern } |
            Sort-Object -Property @{ Expression = { [version]$_ } } -Descending
    )
    $keep = @($versions | Select-Object -First 3)
    # 安全兜底：刚发布的版本必须保留，否则 latest.json 可能指向已删除的目录。
    # 正常发布时 $version 就是最新版本，不会进入此分支。
    if (-not ($keep -ccontains $version)) {
        $keep = @($keep | Select-Object -First 2)
        $keep += $version
    }
    $remove = @($versions | Where-Object { -not ($keep -ccontains $_) })
    Write-Host "保留版本：$($keep -join ', ')"
    foreach ($oldVersion in $remove) {
        # 双重检查：只删除严格的 X.Y.Z 目录名，绝不触碰 latest.json* 或其他文件。
        if ($oldVersion -notmatch $semverPattern) { continue }
        Invoke-Remote "rm -rf -- '/opt/backup/downloads/$oldVersion'"
        Write-Host "已删除旧版本：$oldVersion"
    }

    Write-Step "发布完成：$version"
    Write-Host "安装包：$remoteVersionDir/"
    Write-Host "清单：  $remoteLatest"
}
finally {
    if (Test-Path -LiteralPath $tempRoot) {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
