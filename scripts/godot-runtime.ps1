<#
.SYNOPSIS
  生成 Godot 宿主的运行时目录(01 §4 结论 D1、02 §6.2),默认 target\godot-runtime(git 之外)。

.DESCRIPTION
  目录 = res://,官方模板启动时把 CWD 设成 exe 所在目录并从那里加载工程(不用 .pck、不传 --path):
    forge-godot.exe / forge-godot_console.exe   官方模板(console 版按 "_console.exe" → ".exe" 找主 exe,
                                                  并用 KILL_ON_JOB_CLOSE 的 Job 管住子进程:杀 console 版即杀整棵树)
    project.godot                                01 §4.1 的键(driver.windows 显式写 d3d12,Godot 缺省是 vulkan)
    forge_host.gdextension / .godot\extension_list.cfg / .godot\global_script_class_cache.cfg(空)
    forge_runtime.tscn                           main_scene 必须非空,且 SceneTree 分支会真的加载它(00 §2)
    bin\godot_host.dll                           crates\godot-host 的 cdylib
    runtime-manifest.json                        本次生成用的模板 / dll 的 sha256
  渲染方式与驱动写进 project.godot 当缺省;监督器按 forge.toml [render] 用 --rendering-method / --rendering-driver 覆盖。

.PARAMETER Build
  先 cargo build -p godot-host --locked(Profile=release 时加 --release)。
#>
param(
    [string]$Out = (Join-Path (Split-Path -Parent $PSScriptRoot) 'target\godot-runtime'),
    [ValidateSet('debug', 'release')][string]$Profile = 'debug',
    [ValidateSet('debug', 'release')][string]$Template = 'release',
    [ValidateSet('forward_plus', 'mobile', 'gl_compatibility')][string]$Method = 'forward_plus',
    [ValidateSet('d3d12', 'vulkan', 'opengl3')][string]$Driver = 'd3d12',
    [ValidateRange(1, 240)][int]$MaxFps = 60,
    [switch]$Build
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$tpl = Join-Path $root 'vendor\godot\4.7.2-stable\templates'
$utf8 = New-Object System.Text.UTF8Encoding($false) # Godot 的 ConfigFile 不要 BOM

if ($Build) {
    if (-not $env:CMAKE) { $env:CMAKE = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe' }
    $cargoArgs = @('build', '-p', 'godot-host', '--locked')
    if ($Profile -eq 'release') { $cargoArgs += '--release' }
    Push-Location $root
    try { & cargo @cargoArgs; if ($LASTEXITCODE -ne 0) { throw "cargo build -p godot-host 失败(exit $LASTEXITCODE)" } } finally { Pop-Location }
}

$exe = Join-Path $tpl "windows_${Template}_x86_64.exe"
$con = Join-Path $tpl "windows_${Template}_x86_64_console.exe"
$shaderBuildTarget = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }
$dll = Join-Path $shaderBuildTarget "$Profile\godot_host.dll"
foreach ($f in $exe, $con, $dll) { if (-not (Test-Path $f)) { throw "缺少 $f(模板先跑 scripts\godot-fetch.ps1 -Templates;dll 先 cargo build -p godot-host)" } }

New-Item -ItemType Directory -Force $Out, (Join-Path $Out 'bin'), (Join-Path $Out '.godot') | Out-Null
function Copy-IfChanged($src, $dst) {
    if ((Test-Path $dst) -and ((Get-FileHash $src -Algorithm SHA256).Hash -eq (Get-FileHash $dst -Algorithm SHA256).Hash)) { return }
    Copy-Item -LiteralPath $src -Destination $dst -Force
}
Copy-IfChanged $exe (Join-Path $Out 'forge-godot.exe')
Copy-IfChanged $con (Join-Path $Out 'forge-godot_console.exe')
Copy-IfChanged $dll (Join-Path $Out 'bin\godot_host.dll')


# 01 §4.1 的键;物理 / 导航 / 音频一律 Dummy(确定性红线:物理与逻辑只走 rurix-physics + forge-logic)。
# Dummy 服务器的注册见 GD/servers/register_server_types.cpp:285、:313、:326、:356。
$compat = 'opengl3' # 01 §4.1:gl_compatibility 用原生 OpenGL 3.3(另有 opengl3_angle)
$project = @"
; 由 scripts/godot-runtime.ps1 生成,勿手改(01 §4.1)。
config_version=5

[application]

config/name="ForgeHost"
run/main_scene="res://forge_runtime.tscn"
run/main_loop_type="ForgeHost"
run/max_fps=$MaxFps
run/low_processor_mode=false

[audio]

driver/driver="Dummy"

[display]

window/size/viewport_width=64
window/size/viewport_height=64
window/size/mode=0
window/size/borderless=true
window/size/no_focus=true
window/size/initial_position_type=0
window/size/initial_position=Vector2i(-32000, -32000)
window/vsync/vsync_mode=0

[navigation]

2d/navigation_engine="Dummy"
3d/navigation_engine="Dummy"

[physics]

2d/physics_engine="Dummy"
3d/physics_engine="Dummy"

[rendering]

renderer/rendering_method="$Method"
rendering_device/driver.windows="$Driver"
gl_compatibility/driver.windows="$compat"
driver/threads/thread_model=1
rendering_device/vsync/frame_queue_size=2
"@
$gdext = @"
[configuration]

entry_symbol = "gdext_rust_init"
compatibility_minimum = 4.7
reloadable = false

[libraries]

windows.debug.x86_64 = "res://bin/godot_host.dll"
windows.release.x86_64 = "res://bin/godot_host.dll"
"@
$tscn = @"
[gd_scene format=3]

[node name="ForgeRuntime" type="Node"]
"@
[IO.File]::WriteAllText((Join-Path $Out 'project.godot'), $project.Replace("`r`n", "`n") + "`n", $utf8)
[IO.File]::WriteAllText((Join-Path $Out 'forge_host.gdextension'), $gdext.Replace("`r`n", "`n") + "`n", $utf8)
[IO.File]::WriteAllText((Join-Path $Out 'forge_runtime.tscn'), $tscn.Replace("`r`n", "`n") + "`n", $utf8)
[IO.File]::WriteAllText((Join-Path $Out '.godot\extension_list.cfg'), "res://forge_host.gdextension`n", $utf8)
[IO.File]::WriteAllText((Join-Path $Out '.godot\global_script_class_cache.cfg'), '', $utf8)

$hashFiles = @(
    'forge-godot.exe',
    'forge-godot_console.exe',
    'project.godot',
    'forge_host.gdextension',
    'forge_runtime.tscn',
    '.godot/extension_list.cfg',
    '.godot/global_script_class_cache.cfg',
    'bin/godot_host.dll'
)
$hashes = [ordered]@{}
foreach ($relative in $hashFiles) {
    $file = Join-Path $Out $relative
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "runtime 必需文件缺失: $file" }
    $hashes[$relative] = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
}
$manifest = [ordered]@{
    schema      = 'forge.godot_runtime.v1'
    generatedAt = (Get-Date).ToString('o')
    godot       = (Get-Content (Join-Path $tpl 'version.txt') -Raw).Trim()
    template    = $Template
    profile     = $Profile
    defaults    = [ordered]@{ method = $Method; driver = $Driver; maxFps = $MaxFps }
    sha256      = $hashes
}
[IO.File]::WriteAllText((Join-Path $Out 'runtime-manifest.json'), ($manifest | ConvertTo-Json -Depth 5), $utf8)
"godot runtime ready: $Out (template=$Template profile=$Profile method=$Method driver=$Driver)"
