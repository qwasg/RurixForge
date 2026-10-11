<#
.SYNOPSIS
  一键准备 RurixForge 源码开发环境:检查并安装构建工具,安装依赖,完成首次构建。

.DESCRIPTION
  只想使用 RurixForge 的话,直接下载安装包即可(GitHub Releases 里的 RurixForge-Setup-<版本>.exe)。
  本脚本面向要从源码运行、修改代码的开发者,依次完成:
    1. 检查 Git、Node.js 22+、pnpm、Rust(rustup)、Visual Studio C++ 构建工具;
       缺的经 winget 安装(逐项询问,-Yes 跳过询问)。VS 构建工具约需 5 GB 磁盘,安装时会弹出 UAC 确认。
    2. pnpm install、cargo fetch、补齐上游物理库的构建文件
    3. 构建 Rust 服务(debug)与网页端
    4. 下载并校验 Godot 官方模板,生成 Godot 运行时(-SkipGodot 跳过)
  可以重复运行:已就绪的步骤会跳过或增量完成。

.PARAMETER Yes
  缺少的构建工具直接安装,不逐项询问。

.PARAMETER SkipPrerequisites
  不检查、不安装构建工具(环境已就绪,或不允许安装软件时)。

.PARAMETER SkipGodot
  不准备 Godot 运行时(新建 2D 项目默认使用 Godot,跳过后只能用 Rurix 后端)。

.PARAMETER NoBuild
  只装环境和依赖,不构建。

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\setup.ps1
#>
param(
    [switch]$Yes,
    [switch]$SkipPrerequisites,
    [switch]$SkipGodot,
    [switch]$NoBuild
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
. (Join-Path $PSScriptRoot 'lib\build-common.ps1')
$root = $script:ForgeRoot
$started = Get-Date

function Confirm-Install([string]$What) {
    if ($Yes) { return $true }
    $answer = Read-Host "    安装 $what ?[Y/n]"
    return ($answer -eq '' -or $answer -match '^(y|yes|是)$')
}

function Install-WithWinget([string]$Id, [string]$Override) {
    if (-not (Test-Command winget)) {
        throw "没有 winget(Windows 10 1809+ 的「应用安装程序」提供)。请手动安装 $Id 后重新运行本脚本。"
    }
    $wingetArgs = @('install', '--id', $Id, '--exact', '--source', 'winget', '--accept-package-agreements', '--accept-source-agreements')
    if ($Override) { $wingetArgs += @('--override', $Override) }
    & winget @wingetArgs
    # winget 对「已安装」也会返回非零码;以安装后的实际检测结果为准。
    Update-SessionPath
}

# 已装 Visual Studio / Build Tools 但缺 C++ 工作负载时,用 VS 安装器补装,而不是再装一份。
function Install-VcTools {
    $vswhere = Get-VsWhere
    $existing = $null
    if ($vswhere) { $existing = & $vswhere -latest -products '*' -property installationPath | Select-Object -First 1 }
    $workload = '--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
    if ($existing) {
        $installer = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\setup.exe'
        Write-Note "为已有的 Visual Studio 补装 C++ 构建工具:$existing"
        $proc = Start-Process -FilePath $installer -ArgumentList "modify --installPath `"$existing`" $workload --passive --norestart" -Verb RunAs -Wait -PassThru
        if ($proc.ExitCode -notin 0, 3010) { throw "Visual Studio 安装器退出码 $($proc.ExitCode)" }
    } else {
        Install-WithWinget 'Microsoft.VisualStudio.2022.BuildTools' "--wait --passive --norestart $workload"
    }
}

Write-Step 'RurixForge 开发环境一键准备'
Write-Note "源码:$root"
if (-not [Environment]::Is64BitOperatingSystem) { throw '只支持 64 位 Windows。' }

if (-not $SkipPrerequisites) {
    Write-Step '检查构建工具'
    Update-SessionPath
    $missing = @()
    if (-not (Test-Command git)) { $missing += 'git' }
    if ((Get-NodeMajor) -lt 22) { $missing += 'node' }
    if (-not (Test-Command rustup) -and -not (Test-Command cargo)) { $missing += 'rust' }
    if (-not (Find-VisualStudio)) { $missing += 'vc' }
    if ($missing.Count -eq 0) { Write-Note '构建工具齐全。' }

    if ($missing -contains 'git' -and (Confirm-Install 'Git')) { Install-WithWinget 'Git.Git' }
    if ($missing -contains 'node' -and (Confirm-Install 'Node.js LTS')) { Install-WithWinget 'OpenJS.NodeJS.LTS' }
    if ($missing -contains 'rust' -and (Confirm-Install 'Rust(rustup)')) { Install-WithWinget 'Rustlang.Rustup' }
    if ($missing -contains 'vc' -and (Confirm-Install 'Visual Studio 2022 C++ 构建工具(约 5 GB)')) { Install-VcTools }
    Update-SessionPath

    $problems = @()
    if (-not (Test-Command git)) { $problems += 'Git' }
    if ((Get-NodeMajor) -lt 22) { $problems += 'Node.js 22+' }
    if (-not (Test-Command cargo)) { $problems += 'Rust(cargo)' }
    if (-not (Find-VisualStudio)) { $problems += 'Visual Studio C++ 构建工具' }
    if ($problems.Count -gt 0) {
        throw "仍缺少:$($problems -join '、')。安装后请新开一个 PowerShell 窗口再运行本脚本。"
    }
    # pnpm 装成全局命令,之后 pnpm dev:desktop 等命令可直接使用(npm 全局目录在用户目录下,无需管理员)。
    if (-not (Test-Command pnpm)) {
        Write-Note "安装 pnpm@$script:PnpmVersion"
        Invoke-Native -File 'npm' -Arguments @('install', '--global', "pnpm@$script:PnpmVersion", '--no-audit', '--no-fund')
        Update-SessionPath
    }
    Write-Note "Git $((& git --version) -replace '^git version ', '') · Node $(& node --version) · $(& cargo --version)"
    if (-not (Test-Command go)) { Write-Note '未安装 Go:只有云服务、Go 网关和 Antigravity 桥接需要它,可以以后再装。' }
}

$cmake = Find-CMake
if ($cmake) { $env:CMAKE = $cmake }

Write-Step '安装依赖'
Invoke-Pnpm @('install', '--frozen-lockfile')
Invoke-Native -File 'cargo' -Arguments @('fetch', '--locked') -WorkingDirectory $root
& (Join-Path $PSScriptRoot 'bootstrap-rurix-physics.ps1')

if (-not $NoBuild) {
    Write-Step '构建 Rust 服务(首次需要较长时间)'
    Invoke-Native -File 'cargo' -Arguments @('build', '--workspace', '--exclude', 'godot-host', '--locked') -WorkingDirectory $root

    Write-Step '构建网页端'
    foreach ($pkg in '@forge/protocol', '@forge/host', '@forge/client', '@forge/desktop') {
        Invoke-Pnpm @('--filter', $pkg, 'build')
    }

    if ($SkipGodot) {
        Write-Warn '已跳过 Godot 运行时;需要时运行 scripts\godot-fetch.ps1 -Templates 与 scripts\godot-runtime.ps1 -Build。'
    } else {
        Write-Step '准备 Godot 运行时(首次下载约 1 GB 并校验)'
        & (Join-Path $PSScriptRoot 'godot-fetch.ps1') -Templates
        & (Join-Path $PSScriptRoot 'godot-runtime.ps1') -Build
    }
}

Write-Step ("完成,用时 {0:N0} 分钟" -f ((Get-Date) - $started).TotalMinutes)
Write-Host '  启动桌面应用:pnpm dev:desktop   (会自动启动 AI 后端 forge-agentd)'
Write-Host '  只开网页界面:pnpm dev:host 与 pnpm dev:client(需另行运行 target\debug\forge-agentd.exe)'
Write-Host '  构建安装包:  powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1'
