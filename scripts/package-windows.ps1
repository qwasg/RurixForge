<#
.SYNOPSIS
  构建 RurixForge Windows 安装包:target\installer\out\RurixForge-Setup-<版本>.exe。

.DESCRIPTION
  在源码检出里一条命令出包(构建环境可先用 scripts\setup.ps1 一键装好):
    1. 依赖:pnpm install --frozen-lockfile;tools\installer 用 npm ci 装 electron-builder
    2. 网页端:@forge/protocol → host → client → desktop(含 UltraPlan 浏览器校验运行时)
    3. Rust:release 构建全部服务;Godot 运行时用 release 版 godot_host.dll + 官方模板(缺模板时自动下载并校验)
    4. 按 apps\desktop\src\backend.cjs 约定的布局收集到 target\installer\stage\forge(安装后位于 resources\forge)
    5. electron-builder 生成 NSIS 安装包,并写 SHA256SUMS.txt

  资源目录(agents\、skills\、registry\、tools\blender\、projects\demo\)只收 git 跟踪的文件,
  本机数据、密钥与生成记录不会进安装包。

.PARAMETER SkipInstall
  不重新安装依赖(node_modules 已是最新时省时间)。

.PARAMETER SkipBuild
  不重新构建,直接用现有产物收集出包(只改了安装器配置时用)。

.PARAMETER SkipGodot
  不带 Godot 运行时(安装包小约 130 MB,但新建 2D 项目无法运行),只用于调试打包流程。

.PARAMETER Out
  输出根目录,缺省 target\installer。

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1
#>
param(
    [switch]$SkipInstall,
    [switch]$SkipBuild,
    [switch]$SkipGodot,
    [string]$Out
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
. (Join-Path $PSScriptRoot 'lib\build-common.ps1')
$root = $script:ForgeRoot
if (-not $Out) { $Out = Join-Path $root 'target\installer' }
$Out = [IO.Path]::GetFullPath($Out)
$stage = Join-Path $Out 'stage'
$forge = Join-Path $stage 'forge'
$appDir = Join-Path $stage 'app'
$dist = Join-Path $Out 'out'
$version = (Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
$godotTag = (Get-Content (Join-Path $root 'GODOT_PIN.json') -Raw | ConvertFrom-Json).godot.tag
$started = Get-Date
$utf8 = New-Object Text.UTF8Encoding($false)

# 安装版 bin\ 里的可执行文件:agentd 与它按同目录查找的伙伴进程。
$binaries = @(
    'forge-agentd', 'engine-host', 'viewport-presenter',
    'engine-scene-mcp', 'asset-pipeline-mcp', 'code-forge-mcp', 'context-mcp',
    'gen-image-mcp', 'gen-model-mcp', 'store-mcp', 'blender-bridge-mcp'
)
# Rust 产物动态链接的 VC++ 运行库,随包放在 exe 旁边,用户机器不必另装 VC++ 运行库。
$vcRuntime = @('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')

Write-Step "RurixForge $version Windows 安装包"
Write-Note "源码:$root"
Write-Note "输出:$Out"

Write-Step '检查构建环境'
if (-not [Environment]::Is64BitOperatingSystem) { throw '只支持 64 位 Windows。' }
foreach ($tool in 'git', 'node', 'npm', 'cargo') {
    if (-not (Test-Command $tool)) { throw "缺少 $tool;先运行 scripts\setup.ps1 安装构建环境。" }
}
if ((Get-NodeMajor) -lt 22) { throw "需要 Node.js 22 或更新版本(当前 $(& node --version))。" }
$vcDir = Get-VcRuntimeDir
if (-not $vcDir) { throw '找不到 Visual Studio C++ 构建工具(含 VC++ 运行库);先运行 scripts\setup.ps1。' }
$cmake = Find-CMake
if ($cmake) { $env:CMAKE = $cmake }
Write-Note "Node $(& node --version) · $(& cargo --version)"
Write-Note "VC++ 运行库:$vcDir"

$builderCli = Join-Path $root 'tools\installer\node_modules\electron-builder\cli.js'
if (-not $SkipInstall) {
    Write-Step '安装依赖'
    Invoke-Pnpm @('install', '--frozen-lockfile')
}
if (-not $SkipInstall -or -not (Test-Path -LiteralPath $builderCli)) {
    Invoke-Native -File 'npm' -Arguments @('ci', '--no-audit', '--no-fund') -WorkingDirectory (Join-Path $root 'tools\installer')
}

if (-not $SkipBuild) {
    Write-Step '构建网页端:protocol / host / client / desktop'
    foreach ($pkg in '@forge/protocol', '@forge/host', '@forge/client', '@forge/desktop') {
        Invoke-Pnpm @('--filter', $pkg, 'build')
    }

    Write-Step 'Rust release 构建'
    Invoke-Native -File 'cargo' -Arguments @('fetch', '--locked') -WorkingDirectory $root
    & (Join-Path $PSScriptRoot 'bootstrap-rurix-physics.ps1')
    Invoke-Native -File 'cargo' -Arguments @('build', '--release', '--locked', '--workspace', '--exclude', 'godot-host') -WorkingDirectory $root
}

Write-Step '收集安装内容'
if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
$bin = Join-Path $forge 'bin'
New-Item -ItemType Directory -Force -Path $bin, (Join-Path $appDir 'src') | Out-Null

foreach ($name in $binaries) {
    $src = Join-Path $root "target\release\$name.exe"
    if (-not (Test-Path -LiteralPath $src)) { throw "缺少 $src;去掉 -SkipBuild 重新构建。" }
    Copy-Item -LiteralPath $src -Destination $bin
}
foreach ($dll in $vcRuntime) { Copy-Item -LiteralPath (Join-Path $vcDir $dll) -Destination $bin }
Write-Note "bin\:$($binaries.Count) 个程序 + VC++ 运行库"

if ($SkipGodot) {
    Write-Warn '按 -SkipGodot 跳过 Godot 运行时:安装后新建的 2D 项目无法运行。'
} else {
    $templates = Join-Path $root "vendor\godot\$godotTag\templates"
    if (-not (Test-Path -LiteralPath (Join-Path $templates 'windows_release_x86_64.exe'))) {
        Write-Note "下载 Godot $godotTag 导出模板(约 1 GB,按官方 SHA-512 校验)"
        & (Join-Path $PSScriptRoot 'godot-fetch.ps1') -Templates
    }
    $runtime = Join-Path $forge 'godot-runtime'
    $runtimeArgs = @{ Out = $runtime; Profile = 'release' }
    if (-not $SkipBuild) { $runtimeArgs.Build = $true }
    & (Join-Path $PSScriptRoot 'godot-runtime.ps1') @runtimeArgs
    # godot_host.dll 由 forge-godot.exe 加载,运行库放在两处都能被找到。
    foreach ($dir in $runtime, (Join-Path $runtime 'bin')) {
        foreach ($dll in $vcRuntime) { Copy-Item -LiteralPath (Join-Path $vcDir $dll) -Destination $dir }
    }
}

$ultraplan = Join-Path $root 'apps\desktop\dist\ultraplan-runtime'
if (-not (Test-Path -LiteralPath (Join-Path $ultraplan 'runtime-manifest.json'))) { throw "缺少 $ultraplan;去掉 -SkipBuild 重新构建。" }
Copy-Tree $ultraplan (Join-Path $forge 'ultraplan-runtime')

$hostDir = Join-Path $forge 'packages\host'
Copy-Tree (Join-Path $root 'packages\host\dist') (Join-Path $hostDir 'dist')
Copy-Item -LiteralPath (Join-Path $root 'packages\host\package.json') -Destination $hostDir
$protocolDir = Join-Path $hostDir 'node_modules\@forge\protocol'
Copy-Tree (Join-Path $root 'packages\protocol\dist') (Join-Path $protocolDir 'dist')
Copy-Item -LiteralPath (Join-Path $root 'packages\protocol\package.json') -Destination $protocolDir
Copy-Tree (Join-Path $root 'packages\client\dist') (Join-Path $forge 'packages\client\dist')

$count = Copy-TrackedFiles -Paths @('agents', 'skills', 'registry', 'tools/blender', 'projects/demo/forge.toml', 'projects/demo/Content') -Destination $forge
Write-Note "随包资源:$count 个受跟踪文件"
# 首次启动复制成用户的默认工作区(仓根的「默认工作区」项目)。
$null = Copy-TrackedFiles -Paths @('forge.toml', 'Content') -Destination (Join-Path $forge 'templates\default-workspace')
Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination $forge
Copy-Item -LiteralPath (Join-Path $root 'tools\installer\THIRD_PARTY_NOTICES.txt') -Destination $forge

if (Test-Command go) {
    $bridge = Join-Path $forge 'data\antigravity-runtime'
    New-Item -ItemType Directory -Force -Path $bridge | Out-Null
    Invoke-Native -File 'go' -Arguments @('build', '-trimpath', '-ldflags=-s -w', '-o', (Join-Path $bridge 'forge-antigravity-bridge.exe'), '.') `
        -WorkingDirectory (Join-Path $root 'tools\antigravity-bridge')
    Copy-Item -LiteralPath (Join-Path $root 'tools\antigravity-bridge\LICENSE.CLIProxyAPI') -Destination $bridge
    Write-Note 'Antigravity 桥接:已包含'
} else {
    Write-Warn '未安装 Go,安装包不含 Antigravity 桥接(其余功能不受影响)。'
}

Get-ChildItem -LiteralPath (Join-Path $root 'apps\desktop\src') -Filter '*.cjs' | Copy-Item -Destination (Join-Path $appDir 'src')
$appPackage = [ordered]@{
    name        = 'rurixforge'
    productName = 'RurixForge'
    version     = $version
    description = 'RurixForge:和 AI 一起做游戏的桌面工作台'
    author      = 'RurixForge contributors'
    license     = 'Apache-2.0'
    homepage    = 'https://github.com/qwasg/RurixForge'
    main        = 'src/main.cjs'
}
[IO.File]::WriteAllText((Join-Path $appDir 'package.json'), ($appPackage | ConvertTo-Json), $utf8)
$stageBytes = (Get-ChildItem -LiteralPath $stage -Recurse -File | Measure-Object Length -Sum).Sum
Write-Note ("待打包内容:{0:N0} MB" -f ($stageBytes / 1MB))

Write-Step '生成安装程序(electron-builder + NSIS)'
$electronPkg = Resolve-LinkTarget (Join-Path $root 'apps\desktop\node_modules\electron')
$electronVersion = (Get-Content (Join-Path $electronPkg 'package.json') -Raw | ConvertFrom-Json).version
$env:FORGE_PKG_FORGE = $forge
$env:FORGE_PKG_OUT = $dist
$env:FORGE_PKG_ELECTRON_VERSION = $electronVersion
$electronDist = Join-Path $electronPkg 'dist'
if (Test-Path -LiteralPath (Join-Path $electronDist 'electron.exe')) {
    $env:FORGE_PKG_ELECTRON_DIST = $electronDist
} else {
    Remove-Item Env:FORGE_PKG_ELECTRON_DIST -ErrorAction SilentlyContinue
}
# 没有代码签名证书:关掉自动查找,安装包不签名(首次运行 Windows 可能提示「未知发布者」)。
$env:CSC_IDENTITY_AUTO_DISCOVERY = 'false'
if (Test-Path -LiteralPath $dist) { Remove-Item -LiteralPath $dist -Recurse -Force }
Invoke-Native -File 'node' -Arguments @($builderCli, '--win', 'nsis', '--x64',
    '--config', (Join-Path $root 'tools\installer\electron-builder.config.cjs'),
    '--projectDir', $appDir) -WorkingDirectory $root

$installer = Join-Path $dist "RurixForge-Setup-$version.exe"
if (-not (Test-Path -LiteralPath $installer)) { throw "没有生成 $installer" }
$hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path $dist 'SHA256SUMS.txt'), "$hash  $(Split-Path -Leaf $installer)`n", $utf8)

Write-Step '完成'
Write-Host "  安装程序:$installer"
Write-Host ("  大小:{0:N0} MB" -f ((Get-Item -LiteralPath $installer).Length / 1MB))
Write-Host "  SHA-256:$hash"
Write-Host "  免安装试运行:$(Join-Path $dist 'win-unpacked\RurixForge.exe')"
Write-Host ("  用时:{0:N0} 分钟" -f ((Get-Date) - $started).TotalMinutes)
