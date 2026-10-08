# Stage 3 桌面腿:真实 Electron(FORGE_SMOKE_VISIBLE=1)+ viewport-presenter 子窗口,走 apps/desktop/src/main.cjs 的 7 段 `bind buf`。
# 判据同 f1-w2(G-F1-9):presented >= 3、D3D12 framePath=zero_copy / Vulkan或OpenGL framePath=readback_upload、锚点像素 ≠ 底色、OS 截屏锚点 = readback 中心 ±3、electron 退出码 0;
# 另做点选(中心应命中 cube-a)和 PIE 切换(edit → play_running → edit)。
# 与 f1-w2 的差别:
#   -Backend rurix|godot;agentd 直连 127.0.0.1:8103(不起 gateway);agentd / gen 数据目录放 scratch;
#   rurix:VK_DRIVER_FILES 钉到 presenter 缺省 adapter 那块卡(import 档要求同 LUID);
#   godot:scratch 里的 projects\demo 副本 + forge.toml [render] backend = "godot",由监督器按 [render] 拉起 Godot 宿主;
#   Electron 带 --disable-features=CalculateNativeWinOcclusion 启动:冒烟窗口被用户前台窗口整窗盖住时 Chromium 仍出帧;
#   "OS 级"比对改用 PrintWindow(PW_RENDERFULLCONTENT)取 presenter 子窗口的 DWM 合成结果,不要求窗口露在屏幕上。
# 所有等待都有上限;结束按 PID 停掉自己起的进程树;agentd 新建的 data\skills-config.json 事后删掉。
# 前提:cargo build(engine-host / engine-scene-mcp / viewport-presenter / godot-host)、scripts\godot-runtime.ps1、pnpm -r build。
# 用法: powershell -NoProfile -File scripts/f3-desktop-presenter-smoke.ps1 -Backend rurix|godot ; exit 0 = PASS,1 = FAIL,2 = 环境问题
param(
    [ValidateSet('rurix', 'godot')][string]$Backend = 'rurix',
    [ValidateSet('forward_plus', 'mobile', 'gl_compatibility')][string]$Method = 'forward_plus',
    [ValidateSet('d3d12', 'vulkan', 'opengl3')][string]$Driver = 'd3d12',
    [string]$Device = 'Intel(R) Graphics',
    [ValidateRange(1,65535)][int]$AgentdPort = 8103,
    [ValidateRange(1,65535)][int]$DesktopPort = 3080,
    [string]$AgentdExe = '',
    # 真屏幕比对:冒烟窗口反复 SetWindowPos(HWND_TOPMOST)(不激活),再按物理像素 CopyFromScreen presenter 区域。
    # 会在用户屏幕最上层停留约 20 s,缺省关闭。
    [switch]$OnScreen
)
$ErrorActionPreference = 'Stop'
if ($Backend -eq 'godot' -and ((($Method -eq 'gl_compatibility') -ne ($Driver -eq 'opengl3')))) { throw 'gl_compatibility requires opengl3; forward_plus/mobile require d3d12 or vulkan.' }
$agentdOrigin = "http://127.0.0.1:$AgentdPort"
if ($AgentdPort -eq $DesktopPort) { throw 'AgentdPort and DesktopPort must differ.' }
$expectedFramePath = if ($Backend -eq 'godot' -and $Driver -ne 'd3d12') { 'readback_upload' } else { 'zero_copy' }
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -Namespace F3Smoke -Name U32 -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
[DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
[DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out int v, int size);
[DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
public delegate bool EnumProc(IntPtr h, IntPtr l);
[DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumProc cb, IntPtr l);
public static IntPtr FindChildOfPid(IntPtr parent, uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumChildWindows(parent, (h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
}
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
[DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int idx);
[StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
[DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
public static IntPtr HitTest(int x, int y) { POINT p; p.X = x; p.Y = y; return WindowFromPoint(p); }
// 直接子窗口按 z 序(上 → 下)列出,诊断 presenter 子窗口与 Chromium 自己的窗口谁在上面;style 的 0x08000000 = WS_DISABLED
public static string DescribeChildren(IntPtr parent) {
    var sb = new System.Text.StringBuilder(); int i = 0;
    for (IntPtr c = GetWindow(parent, 5); c != IntPtr.Zero && i < 20; c = GetWindow(c, 2), i++) {
        uint pid; GetWindowThreadProcessId(c, out pid); RECT r; GetWindowRect(c, out r); var cn = new System.Text.StringBuilder(128); GetClassName(c, cn, 128);
        sb.AppendFormat("#{0} pid={1} class={2} vis={3} style=0x{8:X8} ex=0x{9:X8} rect={4},{5}-{6},{7}; ", i, pid, cn, IsWindowVisible(c), r.L, r.T, r.R, r.B, GetWindowLong(c, -16), GetWindowLong(c, -20));
    }
    return sb.ToString();
}
'@
# presenter 是 PER_MONITOR_AWARE_V2,STAT rect 是物理像素;本进程不设 DPI 感知时 CopyFromScreen 走缩放后的逻辑坐标(125% 下对不上)。
[void][F3Smoke.U32]::SetProcessDPIAware()
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format 'yyyyMMdd-HHmmss'
$ev = Join-Path $root "evidence\godot-backend\desktop-presenter-$Backend-$ts"
New-Item -ItemType Directory -Force $ev | Out-Null
$logFile = Join-Path $ev 'smoke.log'
function Log($m) { $line = '[{0}] {1}' -f (Get-Date -Format 'HH:mm:ss'), $m; $line; [IO.File]::AppendAllText($logFile, "$line`r`n", [Text.Encoding]::UTF8) }
$scr = if ($env:KIROCREW_SCRATCH) { $env:KIROCREW_SCRATCH } else { $env:TEMP }
$work = Join-Path $scr "f3-desktop-$Backend-$ts"
New-Item -ItemType Directory -Force $work | Out-Null
$script:jwt = ''
function McpCall($tool, $arguments) {
    $body = [Text.Encoding]::UTF8.GetBytes((@{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 8 -Compress))
    $r = Invoke-WebRequest -Uri "$agentdOrigin/api/forge/mcp/call" -Method POST -Body $body -ContentType 'application/json; charset=utf-8' -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 135
    if ($r.StatusCode -ne 200) { throw "$tool http=$($r.StatusCode)" }
    $outer = [Text.Encoding]::UTF8.GetString($r.RawContentStream.ToArray()) | ConvertFrom-Json
    if ($outer.isError) { throw "${tool}: $($outer.content[0].text)" }
    return $outer.content[0].text | ConvertFrom-Json
}
function Resolve-VkIcd([string]$device) {
    $cls = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
    foreach ($k in (Get-ChildItem $cls -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -match '^\d{4}$' })) {
        $p = Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue
        if ($p -and $p.DriverDesc -eq $device) { foreach ($f in @($p.VulkanDriverName)) { if ($f -and (Test-Path -LiteralPath $f)) { return $f } } }
    }
    return ''
}
# 进程树快照(按 ParentProcessId 从根 PID 往下收),用于留痕和按 PID 清理。
function Get-Tree([int[]]$roots) {
    $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name, CommandLine)
    $seen = @{}; $q = New-Object Collections.Generic.Queue[int]; foreach ($r in $roots) { $q.Enqueue($r) }
    $out = @()
    while ($q.Count -gt 0) {
        $p = $q.Dequeue()
        foreach ($c in ($all | Where-Object { $_.ParentProcessId -eq $p -and -not $seen.ContainsKey([int]$_.ProcessId) })) { $seen[[int]$c.ProcessId] = 1; $out += $c; $q.Enqueue([int]$c.ProcessId) }
    }
    return $out
}

$electronEvidence = Join-Path $ev 'electron'
New-Item -ItemType Directory -Force $electronEvidence | Out-Null
$rectJson = Join-Path $electronEvidence 'viewport-presenter-rect.json'
$doneFlag = Join-Path $electronEvidence 'os-capture-done.flag'
$eSmokeLog = Join-Path $electronEvidence 'smoke.log'
$skillsCfg = Join-Path $root 'data\skills-config.json'
$skillsCfgExisted = Test-Path $skillsCfg
$agentd = $null; $electron = $null; $tree = @(); $exit = 1
$eLogStart = if (Test-Path $eSmokeLog) { (Get-Item $eSmokeLog).Length } else { 0 }
try {
    $busy = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -in $AgentdPort, $DesktopPort })
    if ($busy.Count -gt 0) { $exit = 2; throw "端口被占用:$(($busy | ForEach-Object { "$($_.LocalPort)←pid $($_.OwningProcess)" }) -join ', ')(先关掉已开的 agentd / desktop)" }
    Remove-Item $rectJson, $doneFlag -Force -ErrorAction SilentlyContinue
    foreach ($k in 'FORGE_HOST_PORT', 'FORGE_RENDER_BACKEND', 'FORGE_RENDER_METHOD', 'FORGE_RENDER_DRIVER', 'FORGE_PROJECT_ROOT', 'FORGE_AGENTD_WORKSPACE_ROOT', 'VK_DRIVER_FILES') { Remove-Item "Env:$k" -ErrorAction SilentlyContinue }
    $env:FORGE_AGENTD_DATA_DIR = Join-Path $work 'agentd'; $env:FORGE_GEN_DATA_DIR = Join-Path $work 'gen'
    if ($Backend -eq 'rurix') {
        $icd = Resolve-VkIcd $Device; if (-not $icd) { $exit = 2; throw "找不到 $Device 的 Vulkan ICD" }
        $env:VK_DRIVER_FILES = $icd; Log "VK_DRIVER_FILES=$icd"
    } else {
        $proj = Join-Path $work 'project'
        & robocopy (Join-Path $root 'projects\demo') $proj /E /NFL /NDL /NJH /NJS /NP /XD pack-journey pack-reverify target | Out-Null
        $toml = Join-Path $proj 'forge.toml'
        if ((Get-Content $toml -Raw) -match '(?m)^\[render\]') { $exit = 2; throw "demo 的 forge.toml 已有 [render] 段,脚本需要调整" }
        [IO.File]::AppendAllText($toml, "`r`n[render]`r`nbackend = `"godot`"`r`nmethod = `"$Method`"`r`ndriver = `"$Driver`"`r`n", (New-Object Text.UTF8Encoding $false))
        $env:FORGE_PROJECT_ROOT = $proj; Log "项目副本 $proj(forge.toml: godot $Method/$Driver)"
        # agentd 给 engine-scene-mcp 注入的 FORGE_PROJECT_ROOT = 默认工作区的项目根(scope::project_root_of),会盖掉上面这个;
        # 默认工作区根认 env FORGE_AGENTD_WORKSPACE_ROOT,指到副本(有 forge.toml)才会按副本的 [render] 起宿主。
        $env:FORGE_AGENTD_WORKSPACE_ROOT = $proj
    }
    $env:FORGE_AGENTD_ADDR = "127.0.0.1:$AgentdPort"
    $env:FORGE_AGENTD_ORIGIN = $agentdOrigin
    if (-not $AgentdExe) { $AgentdExe = Join-Path $root 'target\debug\forge-agentd.exe' }
    $agentd = Start-Process -FilePath $AgentdExe -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $ev 'agentd-stdout.log') -RedirectStandardError (Join-Path $ev 'agentd-stderr.log')
    $ok = $false; foreach ($i in 1..80) { try { $r = Invoke-WebRequest -Uri "$agentdOrigin/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw 'agentd /health 20s 未就绪' }
    Log "agentd 就绪 pid=$($agentd.Id) backend=$Backend"
    $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f3desk',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

    Log '== 布场:3 立方体 + 相机(中心锚点应命中 cube-a,非底色) =='
    # 测试原生共享呈现须显式 3D；demo 可为 2D，后者按设计用网页帧叠加网格。
    McpCall 'mcp__engine-scene__scene_new' @{ name = "f3-desktop-$Backend"; mode = '3d' } | Out-Null
    $ops = @(
        @{ op = 'create'; name = 'cube-a'; translation = @(0.0, 0.5, 0.0); components = @(@{ type = 'MeshRenderer'; enabled = $true; props = @{ mesh = 'cube'; material = 'a' } }) },
        @{ op = 'create'; name = 'cube-b'; translation = @(1.6, 0.5, 0.4); rotation = @(0.0, 0.3826834, 0.0, 0.9238795); components = @(@{ type = 'MeshRenderer'; enabled = $true; props = @{ mesh = 'cube'; material = 'b' } }) },
        @{ op = 'create'; name = 'cube-c'; translation = @(-1.6, 0.9, -0.4); scale = @(1.0, 1.8, 1.0); components = @(@{ type = 'MeshRenderer'; enabled = $true; props = @{ mesh = 'cube'; material = 'c' } }) }
    )
    $batch = McpCall 'mcp__engine-scene__entity_batch_apply' @{ ops = $ops }
    if ($batch.applied -ne 3) { throw "batch applied=$($batch.applied) ≠ 3" }
    McpCall 'mcp__engine-scene__viewport_set_camera' @{ target = @(0.0, 0.6, 0.0); yaw = 30.0; pitch = 24.0; dist = 6.5; ortho = $false } | Out-Null
    $tree = @(Get-Tree @($agentd.Id))
    foreach ($p in $tree) { Log ("  agentd 子进程 pid={0} {1} {2}" -f $p.ProcessId, $p.Name, ($p.CommandLine -replace '^"[^"]*"\s*', '')) }
    $hostNames = @($tree | Where-Object { $_.Name -match '^(engine-host|forge-godot.*)\.exe$' } | ForEach-Object { $_.Name })
    $wantHost = if ($Backend -eq 'godot') { '^forge-godot' } else { '^engine-host' }
    if (@($hostNames | Where-Object { $_ -match $wantHost }).Count -eq 0 -or @($hostNames | Where-Object { $_ -notmatch $wantHost }).Count -gt 0) { throw "监督器起的宿主不对:backend=$Backend,实际 $($hostNames -join ',')" }
    Log "  监督器按配置起了 $($hostNames -join ' + ')(backend=$Backend)"
    $backendInfo = McpCall 'mcp__engine-scene__render_backend_info' @{}
    $capabilities = McpCall 'mcp__engine-scene__render_capabilities' @{}
    [IO.File]::WriteAllText((Join-Path $ev 'backend-info.json'), ($backendInfo | ConvertTo-Json -Depth 16), (New-Object Text.UTF8Encoding $false))
    [IO.File]::WriteAllText((Join-Path $ev 'capabilities.json'), ($capabilities | ConvertTo-Json -Depth 16), (New-Object Text.UTF8Encoding $false))
    if ($backendInfo.renderBackend -ne $Backend -or
        ($Backend -eq 'godot' -and ($backendInfo.method -ne $Method -or $backendInfo.driver -ne $Driver))) {
        throw "实际渲染配置不符: $($backendInfo | ConvertTo-Json -Compress -Depth 8)"
    }

    Log '== 启动可见 Electron(FORGE_SMOKE_VISIBLE=1,presenter 子窗口嵌入) =='
    Push-Location (Join-Path $root 'apps\desktop')
    try { $electronBin = node -e "console.log(require('electron'))" } finally { Pop-Location }
    if (-not (Test-Path $electronBin)) { $exit = 2; throw "electron 二进制未找到: $electronBin" }
    # 用户前台窗口(如最大化的编辑器)整窗盖住冒烟窗口时,Chromium 的原生遮挡计算会停帧:capturePage 报 UnknownVizError、
    # 视口 bounds 不再上报、presenter 不嵌入。后台进程又拿不到前台/置顶权限(实测 SetWindowPos(HWND_TOPMOST) 不生效),
    # 所以关掉遮挡计算,改用 PrintWindow(PW_RENDERFULLCONTENT) 取 DWM 合成结果,不要求窗口真的露在屏幕上、也不打扰用户。
    $psi = New-Object Diagnostics.ProcessStartInfo $electronBin, '--disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows .'
    $psi.WorkingDirectory = Join-Path $root 'apps\desktop'; $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['FORGE_HOST_PORT'] = [string]$DesktopPort
    $psi.EnvironmentVariables['FORGE_SMOKE_USER_DATA'] = Join-Path $work 'electron-user-data'
    $psi.EnvironmentVariables['FORGE_SMOKE_EVIDENCE_DIR'] = $electronEvidence
    $psi.EnvironmentVariables['FORGE_SMOKE'] = '1'; $psi.EnvironmentVariables['FORGE_SMOKE_VISIBLE'] = '1'; $psi.EnvironmentVariables['FORGE_SMOKE_SCENARIO'] = 'editor'
    $psi.EnvironmentVariables['ELECTRON_ENABLE_LOGGING'] = '1' # Chromium 内部日志进 stderr(GPU 进程崩溃 / Viz 错误留痕)
    $electron = [Diagnostics.Process]::Start($psi)
    Log "electron pid=$($electron.Id)"
    $hwnd = [IntPtr]::Zero; $deadline = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $deadline -and -not $electron.HasExited) { $electron.Refresh(); if ($electron.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $electron.MainWindowHandle; break }; Start-Sleep -Milliseconds 100 }
    if ($hwnd -ne [IntPtr]::Zero) {
        if ([F3Smoke.U32]::IsIconic($hwnd)) { [void][F3Smoke.U32]::ShowWindow($hwnd, 4) } # 最小化时 DWM 不合成,还原但不激活
        $wr = New-Object F3Smoke.U32+RECT; [void][F3Smoke.U32]::GetWindowRect($hwnd, [ref]$wr); $cloak = 0; [void][F3Smoke.U32]::DwmGetWindowAttribute($hwnd, 14, [ref]$cloak, 4)
        Log "electron 窗口 hwnd=$hwnd rect=$($wr.L),$($wr.T)-$($wr.R),$($wr.B) visible=$([F3Smoke.U32]::IsWindowVisible($hwnd)) cloaked=$cloak 屏幕=$([Windows.Forms.SystemInformation]::VirtualScreen)"
    } else { Log '警告:30s 内没拿到 electron 主窗口句柄' }

    Log '== 等 presenter 证据 JSON(rect + readback 锚点像素) =='
    $deadline = (Get-Date).AddSeconds(90); $nudge = 0
    while (-not (Test-Path $rectJson)) {
        if ((Get-Date) -gt $deadline) { throw 'presenter 证据 JSON 90s 未产出(见 electron-smoke.log)' }
        if ($electron.HasExited) { throw "electron 早退 code=$($electron.ExitCode)(见 electron-smoke.log)" }
        if ($hwnd -ne [IntPtr]::Zero -and [F3Smoke.U32]::IsIconic($hwnd)) { [void][F3Smoke.U32]::ShowWindow($hwnd, 4); Log '  冒烟窗口被最小化(最小化时 Chromium 不出帧),已不激活还原' }
        if ($OnScreen -and $hwnd -ne [IntPtr]::Zero) { [void][F3Smoke.U32]::SetWindowPos($hwnd, [IntPtr](-1), 0, 0, 0, 0, 0x0013) } # Electron show 之后会把 z 序改回来,反复置顶
        # 流按需出帧(场景 / 相机不变就没有新帧,视口角标 0fps):每 2s 把 yaw 在 30.0 / 30.2 间拨一下,presenter 才攒得到 presented>=3
        $nudge++; if ($nudge % 4 -eq 0) { try { McpCall 'mcp__engine-scene__viewport_set_camera' @{ target = @(0.0, 0.6, 0.0); yaw = (30.0 + 0.2 * (($nudge / 4) % 2)); pitch = 24.0; dist = 6.5; ortho = $false } | Out-Null } catch {} }
        Start-Sleep -Milliseconds 500
    }
    McpCall 'mcp__engine-scene__viewport_set_camera' @{ target = @(0.0, 0.6, 0.0); yaw = 30.0; pitch = 24.0; dist = 6.5; ortho = $false } | Out-Null
    Start-Sleep -Milliseconds 300
    $e = Get-Content $rectJson -Raw -Encoding UTF8 | ConvertFrom-Json
    Copy-Item $rectJson (Join-Path $ev 'viewport-presenter-rect.json') -Force
    Log "evidence rect=$($e.rect.x),$($e.rect.y) $($e.rect.w)x$($e.rect.h) share=$($e.texW)x$($e.texH) presented=$($e.presented) centerRgba=$($e.centerRgba -join ',') framePath=$($e.framePath) device=$($e.deviceName)"
    if ($e.presented -lt 3) { throw "presented=$($e.presented) < 3" }
    if ($e.framePath -ne $expectedFramePath) { throw "framePath=$($e.framePath) expected=$expectedFramePath ($Backend $Method/$Driver)" }
    $cr = $e.centerRgba
    if ([Math]::Abs($cr[0] - 23) -le 2 -and [Math]::Abs($cr[1] - 24) -le 2 -and [Math]::Abs($cr[2] - 29) -le 2) { throw '锚点像素 = 底色(±2),场景没画出来' }

    $crit = [ordered]@{ "数据通路:7 段 bind buf 被 presenter 接受、framePath=$expectedFramePath、presented>=3、中心非底色" = $true }
    Log '== presenter 画面比对(DWM PrintWindow PW_RENDERFULLCONTENT;-OnScreen 时截真屏幕) =='
    $presP = @(Get-Tree @($electron.Id) | Where-Object { $_.Name -eq 'viewport-presenter.exe' }) | Select-Object -Last 1
    if (-not $presP) { throw '找不到 viewport-presenter 进程' }
    $ph = [F3Smoke.U32]::FindChildOfPid($hwnd, [uint32]$presP.ProcessId)
    if ($ph -eq [IntPtr]::Zero) { throw "Electron 窗口下找不到 presenter(pid $($presP.ProcessId))的子窗口" }
    $prc = New-Object F3Smoke.U32+RECT; [void][F3Smoke.U32]::GetWindowRect($ph, [ref]$prc)
    $pw = $prc.R - $prc.L; $phh = $prc.B - $prc.T
    Log "  presenter pid=$($presP.ProcessId) 子窗口 $($prc.L),$($prc.T) ${pw}x${phh}(STAT $($e.rect.x),$($e.rect.y) $($e.rect.w)x$($e.rect.h),共享 $($e.texW)x$($e.texH))"
    Log "  Electron 直接子窗口(z 序上→下):$([F3Smoke.U32]::DescribeChildren($hwnd))"
    function Near($px) { [Math]::Abs($px.R - $cr[0]) -le 3 -and [Math]::Abs($px.G - $cr[1]) -le 3 -and [Math]::Abs($px.B - $cr[2]) -le 3 }
    function Fmt($px) { "R$($px.R) G$($px.G) B$($px.B)" }
    # 取 presenter 子窗口那块区域:缺省从顶层窗口的 PrintWindow(DWM 合成,含原生子窗口)裁出来;-OnScreen 时置顶后截真屏幕。
    function Grab-Presenter([string]$name) {
        if ([F3Smoke.U32]::IsIconic($hwnd)) { [void][F3Smoke.U32]::ShowWindow($hwnd, 4); Start-Sleep -Milliseconds 1500 }
        $bmp = New-Object Drawing.Bitmap $pw, $phh; $g = [Drawing.Graphics]::FromImage($bmp)
        if ($OnScreen) { [void][F3Smoke.U32]::SetWindowPos($hwnd, [IntPtr](-1), 0, 0, 0, 0, 0x0013); Start-Sleep -Milliseconds 700; $g.CopyFromScreen($prc.L, $prc.T, 0, 0, $bmp.Size) }
        else {
            $wr = New-Object F3Smoke.U32+RECT; [void][F3Smoke.U32]::GetWindowRect($hwnd, [ref]$wr)
            $wb = New-Object Drawing.Bitmap ($wr.R - $wr.L), ($wr.B - $wr.T); $gw = [Drawing.Graphics]::FromImage($wb); $hdc = $gw.GetHdc()
            [void][F3Smoke.U32]::PrintWindow($hwnd, $hdc, 2); $gw.ReleaseHdc($hdc); $gw.Dispose()
            $g.DrawImage($wb, (New-Object Drawing.Rectangle 0, 0, $pw, $phh), (New-Object Drawing.Rectangle ($prc.L - $wr.L), ($prc.T - $wr.T), $pw, $phh), [Drawing.GraphicsUnit]::Pixel); $wb.Dispose()
        }
        $g.Dispose(); $bmp.Save((Join-Path $ev "$name.png"), [Drawing.Imaging.ImageFormat]::Png); return $bmp
    }
    $how = if ($OnScreen) { '真屏幕' } else { 'PrintWindow' }
    # ① 原样:用户实际看到的 presenter 区域。中心锚点恰在 web 叠加的 2D 轴线上,web 层盖在 presenter 之上时读到的是轴线色。
    $asIs = $false
    foreach ($attempt in 1..3) { Start-Sleep -Milliseconds 500; $b = Grab-Presenter 'presenter-as-is'; $px = $b.GetPixel([int]($pw / 2), [int]($phh / 2)); $b.Dispose(); Log "  ① 原样($how)中心 $(Fmt $px) vs readback R$($cr[0]) G$($cr[1]) B$($cr[2])"; if (Near $px) { $asIs = $true; break } }
    $crit['用户看到的是 presenter 画面(原样 z 序,中心 ±3)'] = $asIs
    # 输入:presenter 盖在上面以后,视口里的鼠标不能落到它身上(否则网页里的点选 / 环绕 / 滚轮全被吞掉)。
    # 禁用的子窗口不收鼠标、由父窗接手;缺省模式窗口在别的窗口后面,WindowFromPoint 没有意义,只查 WS_DISABLED;
    # -OnScreen 时窗口在最上层,再做一次真实命中测试(WindowFromPoint 跳过禁用窗口,返回 presenter 就说明它会吃掉输入)。
    $pStyle = [F3Smoke.U32]::GetWindowLong($ph, -16); $disabled = ($pStyle -band 0x08000000) -ne 0; $hitOk = $true; $hitNote = '未做(窗口不在最上层)'
    if ($OnScreen) {
        $hitH = [F3Smoke.U32]::HitTest($prc.L + [int]($pw / 2), $prc.T + [int]($phh / 2)); $hitPid = [uint32]0; [void][F3Smoke.U32]::GetWindowThreadProcessId($hitH, [ref]$hitPid)
        $hitCls = New-Object Text.StringBuilder 128; [void][F3Smoke.U32]::GetClassName($hitH, $hitCls, 128)
        $hitOk = ($hitH -ne $ph -and $hitPid -ne [uint32]$presP.ProcessId); $hitNote = "视口中心命中 pid=$hitPid class=$hitCls"
    }
    Log ("  输入:presenter style=0x{0:X8}(WS_DISABLED={1});命中测试:{2}" -f $pStyle, $disabled, $hitNote)
    $crit['视口鼠标输入不落在 presenter 上(WS_DISABLED' + $(if ($OnScreen) { ' + 真屏幕命中测试' } else { '' }) + ')'] = ($disabled -and $hitOk)
    # ② 把 presenter 子窗口提到兄弟窗口最上面再取:presenter 自己显示的是不是引擎帧,拉伸还是 1:1。
    [void][F3Smoke.U32]::SetWindowPos($ph, [IntPtr]::Zero, 0, 0, 0, 0, 0x0013); Start-Sleep -Milliseconds 900
    $b = Grab-Presenter 'presenter-raised'
    $p11 = $b.GetPixel([Math]::Min($pw - 1, [int]($e.texW / 2)), [Math]::Min($phh - 1, [int]($e.texH / 2))); $pst = $b.GetPixel([int]($pw / 2), [int]($phh / 2)); $corner = $b.GetPixel($pw - 3, $phh - 3); $b.Dispose()
    $scaling = if (Near $pst) { '拉伸铺满' } elseif (Near $p11) { "1:1 不拉伸(只占左上 $($e.texW)x$($e.texH),右下角 $(Fmt $corner))" } else { '两处都不是引擎帧' }
    Log "  ② 提到最上后($how):1:1 中心 $(Fmt $p11) / 拉伸中心 $(Fmt $pst) → $scaling;z 序:$([F3Smoke.U32]::DescribeChildren($hwnd))"
    $crit['presenter 自身显示 = 引擎帧(子窗口提到最上后,±3)'] = ((Near $p11) -or (Near $pst))
    $crit['presenter 拉伸铺满视口面板'] = [bool](Near $pst)
    if ($OnScreen) { [void][F3Smoke.U32]::SetWindowPos($hwnd, [IntPtr](-2), 0, 0, 0, 0, 0x0013) }

    Log '== 点选 + PIE =='
    $pick = McpCall 'mcp__engine-scene__viewport_pick' @{ x = 480; y = 270; width = 960; height = 540 }
    Log "  pick(480,270 @960x540) = $($pick | ConvertTo-Json -Compress -Depth 6)"
    $crit['点选中心命中 cube-a'] = [bool]($pick.hit -and $pick.name -eq 'cube-a')
    $s0 = (McpCall 'mcp__engine-scene__play_state' @{}).state
    McpCall 'mcp__engine-scene__play_enter' @{} | Out-Null; Start-Sleep -Milliseconds 600
    $s1 = (McpCall 'mcp__engine-scene__play_state' @{}).state
    McpCall 'mcp__engine-scene__play_exit' @{} | Out-Null
    $s2 = (McpCall 'mcp__engine-scene__play_state' @{}).state
    Log "  PIE: $s0 → $s1 → $s2"
    $crit['PIE edit → play_running → edit'] = ($s0 -eq 'edit' -and $s1 -eq 'play_running' -and $s2 -eq 'edit')

    New-Item -ItemType File -Force $doneFlag | Out-Null
    Log '== 等 electron 正常退出 =='
    $crit['electron 退出码 0'] = [bool]($electron.WaitForExit(60000) -and $electron.ExitCode -eq 0)
    Log '== 结论 =='
    foreach ($k in $crit.Keys) { Log ("  {0}  {1}" -f $(if ($crit[$k]) { 'PASS' } else { 'FAIL' }), $k) }
    $exit = if (@($crit.Values | Where-Object { -not $_ }).Count -eq 0) { 0 } else { 1 }
    Log "$(if ($exit -eq 0) { 'PASS' } else { 'FAIL' }) backend=$Backend"
} catch {
    Log "FAIL: $_"
    if ($exit -eq 0) { $exit = 1 }
    New-Item -ItemType File -Force $doneFlag -ErrorAction SilentlyContinue | Out-Null
} finally {
    # 清理每一步各自兜底:taskkill 的 stderr 在 EAP=Stop 下 `2>&1` 会变成终止错误,曾让后面的 agentd 没被停掉。
    $ErrorActionPreference = 'Continue'
    $roots = @(); if ($electron) { $roots += $electron.Id }; if ($agentd) { $roots += $agentd.Id }
    try { $tree = @(Get-Tree $roots) + @($tree) } catch {}
    foreach ($p in $tree | Where-Object { $_.Name -match 'godot|engine-host' } | Sort-Object ProcessId -Unique) { Log ("  宿主 pid={0} {1} {2}" -f $p.ProcessId, $p.Name, ($p.CommandLine -replace '^"[^"]*"\s*', '')) }
    foreach ($id in @($roots) + @($tree | ForEach-Object { [int]$_.ProcessId })) { try { Stop-Process -Id $id -Force -ErrorAction Stop } catch {} }
    Start-Sleep -Milliseconds 800
    $left = @(@($roots) + @($tree | ForEach-Object { [int]$_.ProcessId }) | Sort-Object -Unique | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
    if ($left.Count -gt 0) { Log "  警告:还有进程没停掉:$($left -join ',')" } else { Log "  已停掉自己起的 $(@($roots).Count + @($tree).Count) 个进程" }
    try { if (-not $skillsCfgExisted -and (Test-Path $skillsCfg)) { Remove-Item $skillsCfg -Force; Log '删掉 agentd 新建的 data\skills-config.json' } } catch { Log "  删 skills-config.json 失败:$_" }
    try {
        if ((Test-Path $eSmokeLog) -and (Get-Item $eSmokeLog).Length -gt $eLogStart) {
            $fs = [IO.File]::Open($eSmokeLog, 'Open', 'Read', 'ReadWrite'); [void]$fs.Seek($eLogStart, 'Begin')
            $buf = New-Object byte[] ($fs.Length - $eLogStart); [void]$fs.Read($buf, 0, $buf.Length); $fs.Close()
            [IO.File]::WriteAllBytes((Join-Path $ev 'electron-smoke.log'), $buf)
        }
        $png = Get-ChildItem $electronEvidence -Filter 'desktop-smoke-editor-*.png' -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -gt (Get-Item $logFile).CreationTime } | Sort-Object LastWriteTime | Select-Object -Last 1
        if ($png) { Move-Item $png.FullName (Join-Path $ev 'electron-capturePage.png') -Force }
    } catch { Log "  收证据失败:$_" }
}
exit $exit
