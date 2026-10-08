# Stage 4 场景矩阵:demo 9 + pvz 57 个基线场景(列表与 rurix-frame-baseline.ps1 相同)× godot 四种配置。
# 每个配置、每个项目起一次 Godot,逐个 scene.load → viewport.frame 两次,记 draws / nonZeroPixels / 两帧 sha256 / 是否相等 / 报错原文;
# 每个配置记一次 render.capabilities 的 legs 与 coverage;另起 rurix(engine-host)同尺寸出一遍作参照(Vulkan 按基线 deviceName 钉卡)。
# 宿主中途断开(EOF / 超时)→ 该场景如实记报错,重启宿主接着跑(每个配置×项目最多重启 -MaxRestarts 次)。
# 直连宿主 JSON-RPC(4 字节小端长度 + UTF-8 JSON,一条长连接);读到 0 字节即抛错;所有等待有上限;只停自己起的进程(按 PID)。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f4-scene-matrix.ps1 [-Runtime <dir>] [-W 320 -H 180] [-NoRurix]
# 输出: evidence\godot-backend\stage4\scene-matrix-<时间>\matrix.json + summary.md + run.log
# exit 0 = 66 场景 × 4 配置 = 264 帧全部出帧且两帧哈希相同;1 = 有未出帧 / 两帧不同;2 = 环境错误(缺文件等)。
param(
    [string]$Runtime = '',
    [string]$EngineHost = '',
    [int]$W = 320,
    [int]$H = 180,
    [string[]]$Configs = @('forward_plus/d3d12', 'forward_plus/vulkan', 'mobile/d3d12', 'gl_compatibility/opengl3'),
    [string[]]$Projects = @('projects\demo', 'projects\pvz'),
    [string]$SceneFilter = '*',
    [string]$Device = '',
    [int]$MaxRestarts = 3,
    [switch]$NoRurix,
    # Stage 6: a previously visible reference scene must not silently become an empty frame.
    [switch]$RequireContent,
    [string]$EvidenceStage = 'stage4'
)
$ErrorActionPreference = 'Stop'
if ($RequireContent -and $NoRurix) { throw '-RequireContent needs the Rurix reference pass.' }
if ($EvidenceStage -notmatch '^stage[0-9]+$') { throw 'EvidenceStage must be stageN.' }
Add-Type -AssemblyName System.Web.Extensions
# Independently inspect returned pixels: a uniformly colored frame is not evidence
# of sprite content, even if its background differs from the engine's clear color.
Add-Type -TypeDefinition @'
namespace ForgeMatrix {
    public static class Pixels {
        public static int Varying(byte[] rgba) {
            if (rgba.Length < 4) return 0;
            int count = 0;
            for (int i = 4; i < rgba.Length; i += 4) {
                if (System.Math.Abs(rgba[i] - rgba[0]) > 3 ||
                    System.Math.Abs(rgba[i+1] - rgba[1]) > 3 ||
                    System.Math.Abs(rgba[i+2] - rgba[2]) > 3) count++;
            }
            return count;
        }
    }
}
'@
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Runtime) { $Runtime = Join-Path $repo 'target\godot-runtime' }
if (-not $EngineHost) { $EngineHost = Join-Path $repo 'target\debug\engine-host.exe' }
$godotExe = Join-Path $Runtime 'forge-godot_console.exe'
foreach ($f in @($godotExe, $EngineHost)) { if (-not (Test-Path -LiteralPath $f)) { Write-Host "缺少 $f"; exit 2 } }
$Runtime = (Resolve-Path -LiteralPath $Runtime).Path
$ts = Get-Date -Format 'yyyyMMdd-HHmmss'
$ev = Join-Path $repo "evidence\godot-backend\$EvidenceStage\scene-matrix-$ts"
New-Item -ItemType Directory -Force $ev | Out-Null
$utf8 = New-Object Text.UTF8Encoding $false
$logFile = Join-Path $ev 'run.log'
function Log([string]$m) { $line = '[{0}] {1}' -f (Get-Date -Format 'HH:mm:ss'), $m; Write-Host $line; [IO.File]::AppendAllText($logFile, "$line`r`n", $utf8) }
$json = New-Object System.Web.Script.Serialization.JavaScriptSerializer; $json.MaxJsonLength = [int]::MaxValue; $json.RecursionLimit = 256
$sha = [Security.Cryptography.SHA256]::Create()
function Get-Hex([byte[]]$b) { ([BitConverter]::ToString($sha.ComputeHash($b))).Replace('-', '') }
function To-Json($o) { ConvertTo-Json -InputObject $o -Compress -Depth 12 }
# JavaScriptSerializer 的对象是 Dictionary[string,object];缺键返回 $null 而不是抛错。
function Get-Key($d, [string]$k) {
    if ($d -is [Collections.Generic.Dictionary[string, object]] -and $d.ContainsKey($k)) { $v = $d[$k]; if ($v -is [Array]) { return , $v }; return $v }
    return $null
}

function Read-Exact($s, [byte[]]$b) { $o = 0; while ($o -lt $b.Length) { $k = $s.Read($b, $o, $b.Length - $o); if ($k -le 0) { throw '宿主关闭了连接(EOF)' }; $o += $k } }
$script:rpcId = 0
# 传输层失败(EOF / 超时 / 写失败)把 $hc.broken 置真,调用方据此重启宿主;方法级错误原样抛出 "<method>: <message>"。
function Rpc($hc, [string]$method, $params) {
    $script:rpcId++
    $body = [Text.Encoding]::UTF8.GetBytes($json.Serialize(@{ jsonrpc = '2.0'; id = $script:rpcId; method = $method; params = $params }))
    try {
        $hc.s.Write([BitConverter]::GetBytes([uint32]$body.Length), 0, 4); $hc.s.Write($body, 0, $body.Length)
        $hd = New-Object byte[] 4; Read-Exact $hc.s $hd
        $buf = New-Object byte[] ([BitConverter]::ToUInt32($hd, 0)); Read-Exact $hc.s $buf
    } catch { $hc.broken = $true; throw "${method}: 传输失败:$($_.Exception.Message)" }
    $r = $json.DeserializeObject([Text.Encoding]::UTF8.GetString($buf))
    if ($r.ContainsKey('error') -and $null -ne $r['error']) { throw "${method}: $($r['error']['message'])" }
    return $r['result']
}
function Resolve-VkIcd([string]$dev) {
    $cls = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
    foreach ($k in (Get-ChildItem $cls -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -match '^\d{4}$' })) {
        $p = Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue
        if ($p -and $p.DriverDesc -eq $dev) { foreach ($f in @($p.VulkanDriverName)) { if ($f -and (Test-Path -LiteralPath $f)) { return $f } } }
    }
    return ''
}
# rurix 钉卡:缺省取基线 evidence\godot-backend\rurix-frame-baseline.json 首条 deviceName(与 -Compare 同一做法)。
if (-not $Device) {
    $bl = Join-Path $repo 'evidence\godot-backend\rurix-frame-baseline.json'
    if (Test-Path -LiteralPath $bl) { $mm = [regex]::Match([IO.File]::ReadAllText($bl), '"deviceName":"([^"]*)"'); if ($mm.Success) { $Device = $mm.Groups[1].Value } }
}
$script:vkIcd = if ($Device) { Resolve-VkIcd $Device } else { '' }


# 起宿主并连上。rurix = engine-host.exe --port 0;godot = 运行时目录的 forge-godot_console.exe(与 f3-side-by-side.ps1 同参)。
# stdout 逐行找 "FORGE_HOST_LISTENING port="(godot 40 s、rurix 60 s 上限),就绪后继续排空 stdout / stderr。
function Start-HostProc([string]$kind, [string]$projRoot, [string]$method = '', [string]$driver = '') {
    if ($kind -eq 'rurix') {
        $psi = New-Object Diagnostics.ProcessStartInfo $EngineHost, '--port 0'
        $psi.WorkingDirectory = $repo
        if ($script:vkIcd) { $psi.EnvironmentVariables['VK_DRIVER_FILES'] = $script:vkIcd }
        foreach ($k in 'FORGE_HOST_PORT', 'FORGE_RENDER_BACKEND', 'FORGE_RENDER_METHOD', 'FORGE_RENDER_DRIVER') { if ($psi.EnvironmentVariables.ContainsKey($k)) { $psi.EnvironmentVariables.Remove($k) } }
        $limitS = 60
    } else {
        $psi = New-Object Diagnostics.ProcessStartInfo $godotExe, "--rendering-method $method --rendering-driver $driver --audio-driver Dummy"
        $psi.WorkingDirectory = $Runtime
        $psi.EnvironmentVariables['FORGE_HOST_PORT'] = '0'; $psi.EnvironmentVariables['FORGE_RENDER_BACKEND'] = 'godot'
        $psi.EnvironmentVariables['FORGE_RENDER_METHOD'] = $method; $psi.EnvironmentVariables['FORGE_RENDER_DRIVER'] = $driver
        $limitS = 40
    }
    $psi.EnvironmentVariables['FORGE_PROJECT_ROOT'] = $projRoot
    if ($psi.EnvironmentVariables.ContainsKey('FORGE_GPU_PARTICLES')) { $psi.EnvironmentVariables.Remove('FORGE_GPU_PARTICLES') }
    $psi.UseShellExecute = $false; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
    $t0 = Get-Date
    $p = [Diagnostics.Process]::Start($psi)
    $errTask = $p.StandardError.ReadToEndAsync()
    $lines = New-Object Collections.Generic.List[string]; $port = 0; $deadline = $t0.AddSeconds($limitS)
    while ((Get-Date) -lt $deadline) {
        $t = $p.StandardOutput.ReadLineAsync()
        if (-not $t.Wait([Math]::Max(1, [int]($deadline - (Get-Date)).TotalMilliseconds))) { break }
        $l = $t.Result; if ($null -eq $l) { break }; $lines.Add($l)
        if ($l.StartsWith('FORGE_HOST_LISTENING port=')) { $port = [int]($l.Substring(26).Trim() -replace '\D.*$', ''); break }
    }
    if ($port -eq 0) {
        try { $p.Kill() } catch {}; [void]$p.WaitForExit(10000)
        $tail = if ($errTask.Wait(2000)) { $errTask.Result } else { '' }; if ($tail.Length -gt 600) { $tail = $tail.Substring($tail.Length - 600) }
        throw "$kind $method/$driver ${limitS}s 内没有就绪行:$(($lines | Select-Object -Last 8) -join ' | ') stderr: $tail"
    }
    $null = $p.StandardOutput.ReadToEndAsync()
    # console 版会再起主 exe(Job 管住整棵树);记下子进程 PID,停的时候确认它们也退了。
    $kids = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)" -ErrorAction SilentlyContinue | ForEach-Object { [int]$_.ProcessId })
    $c = New-Object Net.Sockets.TcpClient('127.0.0.1', $port); $c.ReceiveTimeout = 60000; $c.SendTimeout = 60000
    return @{ proc = $p; port = $port; client = $c; s = $c.GetStream(); kids = $kids; err = $errTask; readySec = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 1); broken = $false }
}
function Stop-HostProc($hc) {
    if (-not $hc) { return }
    try { $hc.client.Close() } catch {}
    if (-not $hc.proc.HasExited) { try { $hc.proc.Kill() } catch {} }
    [void]$hc.proc.WaitForExit(10000)
    foreach ($id in $hc.kids) {
        $kp = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($kp -and ([string]$kp.Path).StartsWith($Runtime, [StringComparison]::OrdinalIgnoreCase) -and -not $kp.WaitForExit(10000)) {
            Log "  子进程 $id 10 s 未随 Job 退出,按 PID 停止"; try { $kp.Kill() } catch {}; [void]$kp.WaitForExit(5000)
        }
    }
}
function Get-StderrTail($hc, [int]$n = 400) {
    if (-not $hc -or -not $hc.err.Wait(3000)) { return '' }
    $t = [string]$hc.err.Result; if ($t.Length -gt $n) { $t = $t.Substring($t.Length - $n) }; return ($t -replace '\s+', ' ').Trim()
}
# 与 rurix-frame-baseline.ps1 Get-BaselineScenes 相同:forge.toml 的 entry-scene 在前,再按全路径排序的 Content/**/*.rxscene。
function Get-BaselineScenes([string]$root) {
    $entry = 'Content/Scenes/Main.rxscene'
    $ft = Join-Path $root 'forge.toml'
    if (Test-Path -LiteralPath $ft) { foreach ($l in (Get-Content -LiteralPath $ft -Encoding UTF8)) { if ($l -match '^\s*entry-scene\s*=\s*"([^"]+)"') { $entry = $Matches[1]; break } } }
    $list = New-Object Collections.Generic.List[string]; $list.Add(($entry -replace '\\', '/'))
    $content = Join-Path $root 'Content'
    if (Test-Path -LiteralPath $content) {
        Get-ChildItem -LiteralPath $content -Recurse -File -Filter '*.rxscene' | Sort-Object FullName | ForEach-Object {
            $rel = $_.FullName.Substring($root.Length + 1) -replace '\\', '/'
            if (-not $list.Contains($rel)) { $list.Add($rel) }
        }
    }
    return @($list | Where-Object { $_ -like $SceneFilter })
}
# EditorCamera::default(与基线同):每次 load 前复位,"加载后相机"不受上一个场景影响。
$DefaultCam = @{ target = @(0.0, 0.5, 0.0); yaw = 35.0; pitch = 28.0; dist = 9.0; fovY = 50.0; ortho = $false; orthoSize = 5.0 }


# 一个场景:相机复位 → scene.load → viewport.frame × $frames。rurix 沿用基线的有界重试(尺寸去抖窗口内的读回长度错,
# viewport.rs:1884-1903;最多 3 次、间隔 1.7 s,次数记进 retries);godot 不重试,第一次的报错原样记录。
function Measure-Scene($hc, [string]$projRoot, [string]$scene, [int]$frames, [bool]$retry) {
    $r = [ordered]@{ error = $null }
    $t0 = Get-Date
    try {
        $null = Rpc $hc 'viewport.setCamera' $DefaultCam
        # Join-Path 的输出是 PSObject 包装,JavaScriptSerializer 会去反射它的属性(循环引用报错),必须转成 [string]。
        $info = Rpc $hc 'scene.load' @{ path = [string](Join-Path $projRoot ($scene -replace '/', '\')) }
        $md = Get-Key $info 'mode'; if ($null -ne $md) { $r.mode = $md }
        $hashes = @(); $first = $null; $retries = 0
        for ($i = 0; $i -lt $frames; $i++) {
            $f = $null
            for ($k = 0; ; $k++) {
                try { $f = Rpc $hc 'viewport.frame' @{ width = $W; height = $H }; break }
                catch { if (-not $retry -or $hc.broken -or $k -ge 3) { throw }; $retries++; Start-Sleep -Milliseconds 1700 }
            }
            $px = [Convert]::FromBase64String([string]$f['pixelsB64'])
            if ([int]$f['width'] -ne $W -or [int]$f['height'] -ne $H -or $px.Length -ne $W * $H * 4) {
                throw "帧尺寸 $($f['width'])x$($f['height'])($($px.Length) 字节)≠ ${W}x${H}"
            }
            $hashes += (Get-Hex $px)
            if ($i -eq 0) { $first = $f; $r.varyingPixels = [ForgeMatrix.Pixels]::Varying($px) }
        }
        foreach ($k in 'draws', 'triangles', 'nonZeroPixels', 'truncated', 'meshFallbacks', 'deviceName') { $r[$k] = Get-Key $first $k }
        $r.sha256 = $hashes
        if ($frames -ge 2) { $r.stable = ($hashes[0] -eq $hashes[1]) }
        if ($retries) { $r.retries = $retries }
    } catch { $r.error = "$_" }
    $r.ms = [int]((Get-Date) - $t0).TotalMilliseconds
    return $r
}

# 一遍:$kind = rurix | godot;每个项目起一个宿主,逐场景测;宿主断开就停掉、记 stderr 尾巴、重启接着测。
function Invoke-Pass([string]$kind, [string]$cfg, [int]$frames, $ci) {
    $method, $driver = if ($kind -eq 'godot') { $cfg.Split('/') } else { '', '' }
    foreach ($proj in $Projects) {
        $root = Join-Path $repo $proj; $hc = $null; $restarts = 0
        try {
            foreach ($sc in $sceneLists[$proj]) {
                $e = $entries["$proj|$sc"]; $r = $null
                if (-not $hc) {
                    if ($restarts -gt $MaxRestarts) { $r = [ordered]@{ error = "宿主已重启 $MaxRestarts 次,本项目剩余场景不再尝试" } }
                    else {
                        try {
                            $hc = Start-HostProc $kind $root $method $driver; $ci.starts++
                            Log ("  {0} {1}: 宿主就绪 {2}s pid {3} 端口 {4}" -f $cfg, $proj, $hc.readySec, $hc.proc.Id, $hc.port)
                            if ($kind -eq 'rurix') { Start-Sleep -Milliseconds 1700 }
                            if (-not $ci.Contains('capabilities')) {
                                try {
                                    $ci.backendInfo = Rpc $hc 'render.backendInfo' @{}
                                    $caps = Rpc $hc 'render.capabilities' @{}
                                    $ci.legs = Get-Key $caps 'legs'; $ci.coverage = Get-Key $caps 'coverage'; $ci.capabilities = $caps
                                } catch { $ci.capabilitiesError = "$_"; $ci.capabilities = $null }
                            }
                        } catch { $restarts++; $hc = $null; $r = [ordered]@{ error = "宿主启动失败:$_" }; $ci.startErrors += "$_" }
                    }
                }
                if (-not $r) {
                    $r = Measure-Scene $hc $root $sc $frames ($kind -eq 'rurix')
                    if ($hc.broken -or $hc.proc.HasExited) {
                        Stop-HostProc $hc; $r.hostLost = $true; $r.stderrTail = Get-StderrTail $hc
                        $hc = $null; $restarts++; $ci.restarts++
                    }
                }
                if ($kind -eq 'rurix') { $e.rurix = $r } else { $e.godot[$cfg] = $r }
                $st = if ($r.error) { "✗ $($r.error)" } else { "draws={0} nz={1} stable={2}" -f $r.draws, $r.nonZeroPixels, $(if ($null -ne $r.stable) { $r.stable } else { '-' }) }
                Log ("  {0,-24} {1,-13} {2,-44} {3,6}ms {4}" -f $cfg, $proj, $sc, $r.ms, $st)
            }
        } finally { Stop-HostProc $hc }
    }
}

function Get-Stats([string]$cfg) {
    $s = [ordered]@{ scenes = 0; framed = 0; stable = 0; errors = 0; empty = 0 }
    foreach ($e in $entries.Values) {
        $r = if ($cfg -eq 'rurix') { $e.rurix } else { $e.godot[$cfg] }
        $s.scenes++
        if (-not $r -or $r.error) { $s.errors++; continue }
        $s.framed++
        if ($r.stable -eq $true) { $s.stable++ }
        if ([long]$r.nonZeroPixels -le 0) { $s.empty++ }
    }
    return $s
}

# ---- main ----
$started = Get-Date
$sceneLists = [ordered]@{}; $entries = [ordered]@{}
foreach ($proj in $Projects) {
    $sceneLists[$proj] = @(Get-BaselineScenes (Join-Path $repo $proj))
    foreach ($sc in $sceneLists[$proj]) { $entries["$proj|$sc"] = [ordered]@{ project = ($proj -replace '\\', '/'); scene = $sc; rurix = $null; godot = [ordered]@{} } }
}
$nScenes = $entries.Count
$rtInfo = [ordered]@{ dir = $Runtime; dllSha256 = (Get-FileHash -LiteralPath (Join-Path $Runtime 'bin\godot_host.dll') -Algorithm SHA256).Hash }
$mf = Join-Path $Runtime 'runtime-manifest.json'
if (Test-Path -LiteralPath $mf) { $rtInfo.generatedAt = Get-Key ($json.DeserializeObject([IO.File]::ReadAllText($mf))) 'generatedAt' }
$gitHead = ''; try { $gitHead = [string](git -C $repo rev-parse HEAD) } catch {}
$rurixInfo = [ordered]@{ config = 'rurix'; exe = $EngineHost; exeSha256 = (Get-FileHash -LiteralPath $EngineHost -Algorithm SHA256).Hash
    device = $Device; vkDriverFiles = $script:vkIcd; starts = 0; restarts = 0; startErrors = @() }
$projTxt = ($Projects | ForEach-Object { '{0} {1}' -f ($_ -replace '\\', '/'), @($sceneLists[$_]).Count }) -join ', '
Log ("场景矩阵:{0} 个场景({1})× {2};{3}x{4};运行时 {5}(dll {6});engine-host {7};VK_DRIVER_FILES={8}" -f $nScenes, $projTxt,
    ($Configs -join ' / '), $W, $H, $Runtime, $rtInfo.dllSha256.Substring(0, 12), $EngineHost, $(if ($script:vkIcd) { $script:vkIcd } else { '(未钉)' }))
if (-not $NoRurix) { Log '== rurix(参照,每场景 1 帧)'; Invoke-Pass 'rurix' 'rurix' 1 $rurixInfo }
$cfgInfo = New-Object Collections.Generic.List[object]
foreach ($cfg in $Configs) {
    Log "== godot $cfg"
    $mth, $drv = $cfg.Split('/')
    $ci = [ordered]@{ config = $cfg; method = $mth; driver = $drv; starts = 0; restarts = 0; startErrors = @() }
    Invoke-Pass 'godot' $cfg 2 $ci
    $cfgInfo.Add($ci)
}
$allOk = $nScenes -gt 0
if ($RequireContent) {
    foreach ($e in $entries.Values) {
        if (-not $e.rurix -or $e.rurix.error) { $allOk = $false; continue }
        foreach ($cfg in $Configs) {
            $r = $e.godot[$cfg]
            if ($r -and -not $r.error -and [long]$e.rurix.nonZeroPixels -gt 0 -and
                ([long]$r.nonZeroPixels -le 0 -or
                 ([long]$e.rurix.varyingPixels -gt 0 -and [long]$r.varyingPixels -le 0) -or
                 ([long]$e.rurix.draws -gt 0 -and [long]$r.draws -le 0))) {
                $r.error = 'CONTENT_MISSING: Rurix reference contains visible geometry but Godot returned no visible draws or only uniform background.'
                $allOk = $false
            }
        }
    }
}
foreach ($ci in $cfgInfo) { $ci.stats = Get-Stats $ci.config; if ($ci.stats.framed -ne $nScenes -or $ci.stats.stable -ne $nScenes) { $allOk = $false } }
if (-not $NoRurix) { $rurixInfo.stats = Get-Stats 'rurix' }
$code = if ($allOk) { 0 } else { 1 }
$result = if ($allOk) { 'PASS' } else { 'FAIL' }
$secs = [int]((Get-Date) - $started).TotalSeconds
$run = [ordered]@{ createdAt = $started.ToString('o'); seconds = $secs; size = @($W, $H); scenes = $nScenes; configs = $Configs
    frames = $nScenes * $Configs.Count; requireContent = [bool]$RequireContent; runtime = $rtInfo; gitHead = $gitHead; result = $result; exitCode = $code }

$lines = New-Object Collections.Generic.List[string]
$lines.Add('{'); $lines.Add('"schema": "forge.f4.scene_matrix.v1",'); $lines.Add('"run": ' + (To-Json $run) + ',')
$lines.Add('"rurix": ' + (To-Json $rurixInfo) + ','); $lines.Add('"configs": [')
for ($i = 0; $i -lt $cfgInfo.Count; $i++) { $lines.Add('  ' + (To-Json $cfgInfo[$i]) + $(if ($i -lt $cfgInfo.Count - 1) { ',' } else { '' })) }
$lines.Add('],'); $lines.Add('"entries": [')
$all = New-Object Collections.Generic.List[object]; foreach ($x in $entries.Values) { $all.Add($x) }
for ($i = 0; $i -lt $all.Count; $i++) { $lines.Add('  ' + (To-Json $all[$i]) + $(if ($i -lt $all.Count - 1) { ',' } else { '' })) }
$lines.Add(']'); $lines.Add('}')
[IO.File]::WriteAllLines((Join-Path $ev 'matrix.json'), $lines, $utf8)


function Cell($r) {
    if (-not $r) { return '—' }
    if ($r.error) { return '✗' }
    $t = '{0} / {1}' -f $r.draws, $r.nonZeroPixels
    if ($r.stable -eq $false) { $t += ' ≠' }
    return $t
}
function Esc([string]$s) { return ($s -replace '\|', '\|' -replace '\r?\n', ' ') }
$md = New-Object Collections.Generic.List[string]
$md.Add("# $EvidenceStage 场景矩阵 $ts"); $md.Add('')
$md.Add(('- 场景 {0} 个({1};列表同 rurix-frame-baseline.ps1),尺寸 {2}×{3};相机 = 复位到 EditorCamera 缺省后 scene.load 的结果。' -f $nScenes, $projTxt, $W, $H))
$md.Add(('- godot 运行时 `{0}`(godot_host.dll sha256 `{1}`,生成于 {2})。' -f $Runtime, $rtInfo.dllSha256, $rtInfo.generatedAt))
$md.Add(('- rurix 参照 `{0}`(sha256 `{1}`),VK_DRIVER_FILES = `{2}`(钉到 {3})。' -f $EngineHost, $rurixInfo.exeSha256, $script:vkIcd, $Device))
$md.Add('- 判据:出帧 = scene.load 与两次 viewport.frame 都成功且返回 W×H RGBA8;稳定 = 两帧 sha256 相同;空帧 = nonZeroPixels 为 0。')
$md.Add("- RequireContent=$RequireContent：启用时参考帧必须成功，参考有内容的场景不能退化为空帧、零绘制或均匀背景；varyingPixels 直接统计返回 RGBA 与首像素 RGB 相差超过 3 的像素数，具体像素语义由 g6 专项测试验证。")
$md.Add(('- 结论:**{0}**(exit {1};{2} 帧);用时 {3} s;git {4}。' -f $result, $code, ($nScenes * $Configs.Count), $secs, $gitHead)); $md.Add('')
$md.Add('| 配置 | 场景 | 出帧 | 稳定 | 报错 | 空帧 | 启动 / 重启 | legs | coverage |')
$md.Add('|---|---|---|---|---|---|---|---|---|')
foreach ($ci in $cfgInfo) {
    $s = $ci.stats; $legs = if ($ci.legs) { ($ci.legs -join ', ') } elseif ($ci.capabilitiesError) { "✗ $($ci.capabilitiesError)" } else { '' }
    $cov = if ($null -ne $ci.coverage) { To-Json $ci.coverage } else { '' }
    $md.Add(('| {0} | {1} | {2} | {3} | {4} | {5} | {6} / {7} | {8} | {9} |' -f $ci.config, $s.scenes, $s.framed, $s.stable, $s.errors, $s.empty, $ci.starts, $ci.restarts, (Esc $legs), (Esc $cov)))
}
if (-not $NoRurix) {
    $s = $rurixInfo.stats; $legs = if ($rurixInfo.legs) { ($rurixInfo.legs -join ', ') } else { '' }
    $md.Add(('| rurix(参照,1 帧) | {0} | {1} | — | {2} | {3} | {4} / {5} | {6} | |' -f $s.scenes, $s.framed, $s.errors, $s.empty, $rurixInfo.starts, $rurixInfo.restarts, (Esc $legs)))
}
$md.Add(''); $md.Add('## 报错与两帧不同(原文)'); $md.Add('')
$nBad = 0
foreach ($e in $entries.Values) {
    $pairs = New-Object Collections.Generic.List[object]
    if ($e.rurix) { $pairs.Add(@('rurix', $e.rurix)) }
    foreach ($cfg in $Configs) { $pairs.Add(@($cfg, $e.godot[$cfg])) }
    foreach ($pr in $pairs) {
        $r = $pr[1]
        if (-not $r) { continue }
        if ($r.error) { $nBad++; $md.Add(('- `{0}` {1} `{2}`:{3}{4}' -f $pr[0], $e.project, $e.scene, (Esc $r.error), $(if ($r.stderrTail) { ' — stderr 尾:' + (Esc $r.stderrTail) } else { '' }))) }
        elseif ($r.stable -eq $false) { $nBad++; $md.Add(('- `{0}` {1} `{2}`:两帧不同 {3} ≠ {4}' -f $pr[0], $e.project, $e.scene, $r.sha256[0].Substring(0, 12), $r.sha256[1].Substring(0, 12))) }
    }
}
if ($nBad -eq 0) { $md.Add('(无)') }
$md.Add(''); $md.Add('## 逐场景 draws / nonZeroPixels(✗ = 报错,≠ = 两帧不同,— = 未跑)'); $md.Add('')
$md.Add('| 项目 | 场景 | rurix | ' + ($Configs -join ' | ') + ' |')
$md.Add('|---|---|---|' + (($Configs | ForEach-Object { '---' }) -join '|') + '|')
foreach ($e in $entries.Values) {
    $cells = @(Cell $e.rurix) + @($Configs | ForEach-Object { Cell $e.godot[$_] })
    $md.Add(('| {0} | {1} | {2} |' -f $e.project, $e.scene, ($cells -join ' | ')))
}
[IO.File]::WriteAllLines((Join-Path $ev 'summary.md'), $md, $utf8)
foreach ($ci in $cfgInfo) { $s = $ci.stats; Log ('{0,-24} 出帧 {1}/{2} 稳定 {3} 报错 {4} 空帧 {5}' -f $ci.config, $s.framed, $s.scenes, $s.stable, $s.errors, $s.empty) }
Log ("{0}:输出 {1}(matrix.json / summary.md / run.log)exit {2}" -f $result, $ev, $code)
exit $code
