# Stage 5 side-by-side: copy of f4-side-by-side.ps1 + -Fixtures stage4|stage5, per-scene env / warmFrames (scripts\f5-make-fixtures.py).
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f5-side-by-side.ps1 -Fixtures stage5 [-Only gi,volumes]
# Generated from scripts\f4-side-by-side.ps1 by tools\stage5\make-f5-sbs.ps1 (workspace); the f4 header below still applies.
# Stage 4 多场景画面对照(f3-side-by-side.ps1 的多场景版):crates\godot-host\tests\fixtures\stage4\scenes\*.json 的场景
# 分别用 rurix(engine-host)与 godot 四种配置出帧。每个 (配置, 场景) 起一个新宿主,连续取两帧(哈希相同 = 稳定),
# 与 rurix 比最大差 / 全帧平均差 / 超差比例 / 探针方框均值;存每帧 PNG、差值图(×Gain)和每场景 3×2 并排拼图。
# 缺省灯光场景(场景表 target 非空)要求四种配置的全帧平均差 ≤ target;含 Light 的场景 rurix 不读 Light(01 §5.3),只作对照。
# 不用 WMI(本机 WMI 挂住时 Get-CimInstance / Get-NetTCPConnection 无限等待):子进程用 Toolhelp32 快照查。
# 直连宿主 JSON-RPC(4 字节小端长度 + UTF-8 JSON);读到 0 字节即抛错;所有等待有上限;只停自己起的进程(按 PID)。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f4-side-by-side.ps1 [-Only maze,lights] [-Device 'Intel(R) Graphics']
# 输出: evidence\godot-backend\stage4\side-by-side-<时间>\ 下 <场景>-<配置>.png、<场景>-diff-<配置>.png、<场景>-side-by-side.png、
#       summary.json、summary.md、run.log。exit 0 = 全部出帧、godot 两帧哈希相同、有目标的场景达标;1 = 有不合格;2 = 环境错误。
param(
    [string]$Fixtures = 'stage5',
    [string[]]$Only = @(),
    [string]$Device = '',
    [string]$Runtime = '',
    [int]$Gain = 4
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing, System.Web.Extensions
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Runtime) { $Runtime = Join-Path $repo 'target\godot-runtime' }
$engineHost = Join-Path $repo 'target\debug\engine-host.exe'
$godotExe = Join-Path $Runtime 'forge-godot_console.exe'
$fixRoot = Join-Path $repo "crates\godot-host\tests\fixtures\$Fixtures"
foreach ($f in @($engineHost, $godotExe, (Join-Path $fixRoot 'scenes'))) { if (-not (Test-Path -LiteralPath $f)) { Write-Host "缺少 $f"; exit 2 } }
$Runtime = (Resolve-Path -LiteralPath $Runtime).Path
$ts = Get-Date -Format 'yyyyMMdd-HHmmss'
$ev = Join-Path $repo "evidence\godot-backend\stage5\side-by-side-$Fixtures-$ts"
New-Item -ItemType Directory -Force $ev | Out-Null
$scr = if ($env:KIROCREW_SCRATCH) { $env:KIROCREW_SCRATCH } else { $env:TEMP }
$work = Join-Path $scr "f5-sbs-$ts"
New-Item -ItemType Directory -Force $work | Out-Null
$utf8 = New-Object Text.UTF8Encoding $false
$logFile = Join-Path $ev 'run.log'
function Log([string]$m) { $line = '[{0}] {1}' -f (Get-Date -Format 'HH:mm:ss'), $m; Write-Host $line; [IO.File]::AppendAllText($logFile, "$line`r`n", $utf8) }
$json = New-Object System.Web.Script.Serialization.JavaScriptSerializer; $json.MaxJsonLength = [int]::MaxValue; $json.RecursionLimit = 256
$sha = [Security.Cryptography.SHA256]::Create()
function Get-Hex([byte[]]$b) { ([BitConverter]::ToString($sha.ComputeHash($b))).Replace('-', '') }
# JavaScriptSerializer 的对象是 Dictionary[string,object];缺键返回 $null;数组原样返回(不展开)。
function Get-Key($d, [string]$k) {
    if ($d -is [Collections.Generic.Dictionary[string, object]] -and $d.ContainsKey($k)) { $v = $d[$k]; if ($v -is [Array]) { return , $v }; return $v }
    return $null
}

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class F4Px {
    public static byte[] RgbaToBgra(byte[] s) { var d = new byte[s.Length]; for (int i = 0; i + 3 < s.Length; i += 4) { d[i] = s[i + 2]; d[i + 1] = s[i + 1]; d[i + 2] = s[i]; d[i + 3] = 255; } return d; }
    // [0] 最大通道差 [1] 全帧 RGB 平均绝对差(与 g4util::stats 相同)[2] 最大通道差 > 2 的像素比例 [3] > 8 的比例 [4] > 2 的像素数 [5] > 8 的像素数
    public static double[] Stats(byte[] a, byte[] b) {
        int max = 0; double sum = 0; long gt2 = 0, gt8 = 0, n = 0;
        for (int i = 0; i + 3 < a.Length && i + 3 < b.Length; i += 4) {
            int m = 0, s = 0;
            for (int c = 0; c < 3; c++) { int d = Math.Abs(a[i + c] - b[i + c]); s += d; if (d > m) m = d; }
            if (m > max) max = m;
            sum += s / 3.0; if (m > 2) gt2++; if (m > 8) gt8++; n++;
        }
        if (n == 0) return new double[] { 0, 0, 0, 0, 0, 0 };
        return new double[] { max, sum / n, (double)gt2 / n, (double)gt8 / n, gt2, gt8 };
    }
    // 前景统计(补充,不进判据):前景 = rurix 帧里与 (0,0) 清屏色不同的像素;[0] 前景像素数 [1] 最大通道差 [2] 平均差 [3] > 8 的像素数
    public static double[] FgStats(byte[] a, byte[] b) {
        int max = 0; double sum = 0; long gt8 = 0, n = 0;
        for (int i = 0; i + 3 < a.Length && i + 3 < b.Length; i += 4) {
            if (a[i] == a[0] && a[i + 1] == a[1] && a[i + 2] == a[2]) continue;
            int m = 0, s = 0;
            for (int c = 0; c < 3; c++) { int d = Math.Abs(a[i + c] - b[i + c]); s += d; if (d > m) m = d; }
            if (m > max) max = m;
            sum += s / 3.0; if (m > 8) gt8++; n++;
        }
        return new double[] { n, max, n == 0 ? 0 : sum / n, gt8 };
    }
    public static byte[] Diff(byte[] a, byte[] b, int gain) {
        var d = new byte[a.Length];
        for (int i = 0; i + 3 < a.Length && i + 3 < b.Length; i += 4) {
            for (int c = 0; c < 3; c++) { int v = Math.Abs(a[i + c] - b[i + c]) * gain; d[i + c] = (byte)(v > 255 ? 255 : v); }
            d[i + 3] = 255;
        }
        return d;
    }
    // (x, y) 为中心、边长 2r+1 的方框平均 RGB(与 g4util::mean_box 相同)
    public static double[] BoxMean(byte[] px, int w, int h, int x, int y, int r) {
        var s = new double[3]; int n = 0;
        for (int yy = Math.Max(0, y - r); yy <= Math.Min(h - 1, y + r); yy++)
            for (int xx = Math.Max(0, x - r); xx <= Math.Min(w - 1, x + r); xx++) { int i = (yy * w + xx) * 4; for (int c = 0; c < 3; c++) s[c] += px[i + c]; n++; }
        for (int c = 0; c < 3; c++) s[c] = Math.Round(s[c] / Math.Max(1, n), 2);
        return s;
    }
}
public static class F4Tree {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct PE32 {
        public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID;
        public uint th32ModuleID; public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase;
        public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile;
    }
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32FirstW(IntPtr h, ref PE32 e);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] static extern bool Process32NextW(IntPtr h, ref PE32 e);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    public static List<int> Children(int ppid) {
        var list = new List<int>();
        IntPtr h = CreateToolhelp32Snapshot(2, 0);
        if (h == IntPtr.Zero || h == new IntPtr(-1)) return list;
        var e = new PE32(); e.dwSize = (uint)Marshal.SizeOf(typeof(PE32));
        if (Process32FirstW(h, ref e)) { do { if (e.th32ParentProcessID == (uint)ppid) list.Add((int)e.th32ProcessID); e.dwSize = (uint)Marshal.SizeOf(typeof(PE32)); } while (Process32NextW(h, ref e)); }
        CloseHandle(h);
        return list;
    }
}
'@


# 场景表。target = 全帧平均差上限(8 bit,RGB 三通道平均,四种 godot 配置都要达标);$null = 只报数字不设目标。
# 缺省灯光场景 = 场景里没有启用的 Light(godot 走缺省方向光 + 环境光,01 §5.3)。graycard_mesh 放宽到 2.0:
# sprite_mesh 腿的缺省灯按三个正轴面拟合,任意朝向的面最大差 −16(02 §9.5 Stage 4)。
# 含 Light 的场景:rurix 不读 Light 组件、始终用写死灯光,godot 按 Light 真画,差值是设计上的,只作对照。
$SceneTable = @(
    @{ name = 'maze'; kind = '缺省灯光 · demo 迷宫(36 个内置 cube,sprite_mesh 腿)'; target = 1.0 },
    @{ name = 'graycard_model'; kind = '缺省灯光 · 模型腿灰卡(NdotL 1.0 → 0.2)'; target = 1.0 },
    @{ name = 'graycard_mesh'; kind = '缺省灯光 · sprite_mesh 腿灰卡(cube 任意朝向)'; target = 2.0 },
    @{ name = 'materials'; kind = '模型 / 材质(贴图、法线 Y、AO、emissive、mask、blend、unlit、metal、doubleSided)'; target = $null },
    @{ name = 'anim'; kind = '骨骼动画(GPU 蒙皮,walk t = 0.5 / 1.5)'; target = $null },
    @{ name = 'lights'; kind = '灯光(方向 + 点 + 聚光带阴影、Parent;rurix 不读 Light)'; target = $null },
    @{ name = 'pz_mvp'; kind = 'demo 关卡 pz_mvp_phase1(52 MeshRenderer + 2 Light;rurix 不读 Light)'; target = $null }
)
if ($Fixtures -eq 'stage5') {
    $SceneTable = @(
        @{ name = 'env_panel'; kind = 'Environment: sky / AgX / glow / SSAO (rurix ignores Environment)'; target = $null },
        @{ name = 'fog'; kind = 'Environment exponential fog, 5 depths (rurix ignores Environment)'; target = $null },
        @{ name = 'gi'; kind = 'SDFGI after 240 warm frames (rurix ignores Environment)'; target = $null },
        @{ name = 'volumes'; kind = 'ReflectionProbe / Decal / FogVolume (rurix ignores them)'; target = $null },
        @{ name = 'particles_05'; kind = 'ParticleEmitter kinds 1-4, age 0.5 s, FORGE_GPU_PARTICLES=1'; target = $null },
        @{ name = 'particles_09'; kind = 'ParticleEmitter kinds 1-4, age 0.9 s, FORGE_GPU_PARTICLES=1'; target = $null }
    )
}
$Configs = @(
    @{ tag = 'rurix'; kind = 'rurix' },
    @{ tag = 'forward_plus-d3d12'; kind = 'godot'; method = 'forward_plus'; driver = 'd3d12' },
    @{ tag = 'forward_plus-vulkan'; kind = 'godot'; method = 'forward_plus'; driver = 'vulkan' },
    @{ tag = 'mobile-d3d12'; kind = 'godot'; method = 'mobile'; driver = 'd3d12' },
    @{ tag = 'gl_compatibility-opengl3'; kind = 'godot'; method = 'gl_compatibility'; driver = 'opengl3' }
)
# EditorCamera::default(与基线 / 场景矩阵相同):load 类场景没给相机时在 load 前复位。
$DefaultCam = @{ target = @(0.0, 0.5, 0.0); yaw = 35.0; pitch = 28.0; dist = 9.0; fovY = 50.0; ortho = $false; orthoSize = 5.0 }

function Resolve-VkIcd([string]$dev) {
    $cls = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
    foreach ($k in (Get-ChildItem $cls -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -match '^\d{4}$' })) {
        $p = Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue
        if ($p -and $p.DriverDesc -eq $dev) { foreach ($f in @($p.VulkanDriverName)) { if ($f -and (Test-Path -LiteralPath $f)) { return $f } } }
    }
    return ''
}
# rurix 钉卡:缺省取基线 evidence\godot-backend\rurix-frame-baseline.json 首条 deviceName(与基线 -Compare、场景矩阵同一做法)。
if (-not $Device) {
    $bl = Join-Path $repo 'evidence\godot-backend\rurix-frame-baseline.json'
    if (Test-Path -LiteralPath $bl) { $mm = [regex]::Match([IO.File]::ReadAllText($bl), '"deviceName":"([^"]*)"'); if ($mm.Success) { $Device = $mm.Groups[1].Value } }
    if (-not $Device) { $Device = 'Intel(R) Graphics' }
}
$vkIcd = Resolve-VkIcd $Device

function Read-Exact($s, [byte[]]$b) { $o = 0; while ($o -lt $b.Length) { $k = $s.Read($b, $o, $b.Length - $o); if ($k -le 0) { throw '宿主关闭了连接(EOF)' }; $o += $k } }
$script:rpcId = 0
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

# 起宿主并连上:rurix = engine-host.exe --port 0(Vulkan 钉卡);godot = 运行时目录的 forge-godot_console.exe(与测试 / 矩阵同参)。
function Start-HostProc($cfg, [string]$projRoot, $extraEnv) {
    if ($cfg.kind -eq 'rurix') {
        $psi = New-Object Diagnostics.ProcessStartInfo $engineHost, '--port 0'
        $psi.WorkingDirectory = $repo
        if ($vkIcd) { $psi.EnvironmentVariables['VK_DRIVER_FILES'] = $vkIcd }
        foreach ($k in 'FORGE_HOST_PORT', 'FORGE_RENDER_BACKEND', 'FORGE_RENDER_METHOD', 'FORGE_RENDER_DRIVER') { if ($psi.EnvironmentVariables.ContainsKey($k)) { $psi.EnvironmentVariables.Remove($k) } }
        $limitS = 60
    } else {
        $psi = New-Object Diagnostics.ProcessStartInfo $godotExe, "--rendering-method $($cfg.method) --rendering-driver $($cfg.driver) --audio-driver Dummy"
        $psi.WorkingDirectory = $Runtime
        $psi.EnvironmentVariables['FORGE_HOST_PORT'] = '0'; $psi.EnvironmentVariables['FORGE_RENDER_BACKEND'] = 'godot'
        $psi.EnvironmentVariables['FORGE_RENDER_METHOD'] = $cfg.method; $psi.EnvironmentVariables['FORGE_RENDER_DRIVER'] = $cfg.driver
        if ($psi.EnvironmentVariables.ContainsKey('VK_DRIVER_FILES')) { $psi.EnvironmentVariables.Remove('VK_DRIVER_FILES') }
        $limitS = 40
    }
    $psi.EnvironmentVariables['FORGE_PROJECT_ROOT'] = $projRoot
    if ($psi.EnvironmentVariables.ContainsKey('FORGE_GPU_PARTICLES')) { $psi.EnvironmentVariables.Remove('FORGE_GPU_PARTICLES') }
    if ($extraEnv -is [Collections.Generic.Dictionary[string, object]]) { foreach ($k in @($extraEnv.Keys)) { $psi.EnvironmentVariables[$k] = [string]$extraEnv[$k] } }
    $psi.UseShellExecute = $false; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
    $t0 = Get-Date
    $p = [Diagnostics.Process]::Start($psi)
    $errTask = $p.StandardError.ReadToEndAsync()
    $lines = New-Object Collections.Generic.List[string]; $port = 0; $deadline = $t0.AddSeconds($limitS)
    while ((Get-Date) -lt $deadline) {
        $t = $p.StandardOutput.ReadLineAsync()
        if (-not $t.Wait([Math]::Max(1, [int]($deadline - (Get-Date)).TotalMilliseconds))) { break }
        $l = $t.Result; if ($null -eq $l) { break }; $lines.Add($l)
        if ($l -match '^FORGE_HOST_LISTENING port=(\d+)') { $port = [int]$Matches[1]; break }
    }
    $kids = @([F4Tree]::Children($p.Id))
    if ($port -eq 0) {
        try { $p.Kill() } catch {}; [void]$p.WaitForExit(10000)
        foreach ($id in $kids) { $kp = Get-Process -Id $id -ErrorAction SilentlyContinue; if ($kp) { try { $kp.Kill() } catch {} } }
        throw "$($cfg.tag) ${limitS}s 内没有就绪行:$(($lines | Select-Object -Last 6) -join ' | ')"
    }
    $null = $p.StandardOutput.ReadToEndAsync() # 就绪后继续排空 stdout(02 §6.2),否则管道写满宿主会卡住
    $c = New-Object Net.Sockets.TcpClient('127.0.0.1', $port); $c.ReceiveTimeout = 60000; $c.SendTimeout = 60000
    return @{ proc = $p; port = $port; client = $c; s = $c.GetStream(); kids = $kids; err = $errTask; readySec = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 1); broken = $false }
}
# console 版用 KILL_ON_JOB_CLOSE 的 Job 管住主 exe:停 console 版即停整棵树;子进程 10 s 内没退再按 PID 停。
function Stop-HostProc($hc) {
    if (-not $hc) { return }
    try { $hc.client.Close() } catch {}
    if (-not $hc.proc.HasExited) { try { $hc.proc.Kill() } catch {} }
    [void]$hc.proc.WaitForExit(10000)
    foreach ($id in $hc.kids) {
        $kp = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($kp -and -not $kp.WaitForExit(10000)) { Log "  子进程 $id 10 s 未随 Job 退出,按 PID 停止"; try { $kp.Kill() } catch {}; [void]$kp.WaitForExit(5000) }
    }
}


# 不能用 New-Object:cmdlet 输出一律包成 PSObject,JavaScriptSerializer 会反射 PSObject 的成员而报循环引用;::new() 给的是原始对象。
function New-Dict { return [System.Collections.Generic.Dictionary[string, object]]::new() }
# props 里 "@名字" = 同配方先建的同名实体 id(夹具约定,见 scenes\lights.json 的 doc)。
function Resolve-Refs($props, $ids) {
    if (-not ($props -is [Collections.Generic.Dictionary[string, object]])) { return $props }
    $o = New-Dict
    foreach ($k in @($props.Keys)) {
        $v = $props[$k]
        if ($v -is [string] -and $v.StartsWith('@')) { $n = $v.Substring(1); if (-not $ids.ContainsKey($n)) { throw "引用 $v 找不到先建的同名实体" }; $o[$k] = $ids[$n] }
        else { $o[$k] = $v }
    }
    return $o
}
# 按夹具布场(两边同一组 RPC,与 g4util::build 相同):
#   load 类:没给相机 → load 前复位 EditorCamera 缺省;给了相机 → load 后设置。
#   entities 类:scene.new → 逐个 entity.create → setCamera;idMod8 = k 的实体先不带组件建,id % 8 != k 就再建(空实体不画),
#   满足后逐个 component.add(MeshRenderer.material 为空串时按 id 调色板取色,两边必须拿到同一个 id % 8)。
function Build-FixtureScene($hc, $sc, [string]$projRoot) {
    $cam = Get-Key $sc 'camera'
    $load = Get-Key $sc 'load'
    if ($load) {
        if (-not $cam) { $null = Rpc $hc 'viewport.setCamera' $DefaultCam }
        # Join-Path 的输出是 PSObject 包装,JavaScriptSerializer 会反射它的属性而报循环引用,必须转成 [string]。
        $null = Rpc $hc 'scene.load' @{ path = [string](Join-Path $projRoot (([string]$load) -replace '/', '\')) }
        if ($cam) { $null = Rpc $hc 'viewport.setCamera' $cam }
        return
    }
    $null = Rpc $hc 'scene.new' @{ name = "f4-sbs-$($sc['name'])" }
    $ids = @{}
    # Get-Key 用 ", $v" 原样返回数组;直接赋值得到数组本身(外面再套 @() 会变成"只有一项、这一项是整个数组")。
    $ents = Get-Key $sc 'entities'
    if (-not $ents) { throw "夹具 $($sc['name']) 既没有 load 也没有 entities" }
    foreach ($e in $ents) {
        $comps = @(); $cv = Get-Key $e 'components'; if ($cv) { $comps = @($cv) }
        $fixed = New-Dict
        foreach ($k in @($e.Keys)) { if ($k -ne 'components' -and $k -ne 'idMod8') { $fixed[$k] = $e[$k] } }
        $want = Get-Key $e 'idMod8'
        if ($null -ne $want) {
            $want = [int]$want; $id = -1
            for ($t = 0; $t -lt 16; $t++) { $r = Rpc $hc 'entity.create' $fixed; $id = [int]$r['id']; if ($id % 8 -eq $want) { break } }
            if ($id % 8 -ne $want) { throw "idMod8 = ${want}:16 次内没建出 id % 8 == $want 的实体" }
            foreach ($c in $comps) { $null = Rpc $hc 'component.add' @{ id = $id; type = $c['type']; enabled = $c['enabled']; props = (Resolve-Refs $c['props'] $ids) } }
        } else {
            $cl = New-Object Collections.ArrayList
            foreach ($c in $comps) { $cc = New-Dict; foreach ($k in @($c.Keys)) { $cc[$k] = $c[$k] }; $cc['props'] = Resolve-Refs $c['props'] $ids; [void]$cl.Add($cc) }
            $fixed['components'] = $cl.ToArray()
            $r = Rpc $hc 'entity.create' $fixed; $id = [int]$r['id']
        }
        $ids[[string]$e['name']] = $id
    }
    if ($cam) { $null = Rpc $hc 'viewport.setCamera' $cam }
}
# rurix 沿用基线的有界重试(尺寸去抖窗口内的读回长度错,最多 3 次、间隔 1.7 s);godot 不重试。
function Get-Frame($hc, [int]$w, [int]$h, [bool]$retry) {
    $f = $null
    for ($k = 0; ; $k++) {
        try { $f = Rpc $hc 'viewport.frame' @{ width = $w; height = $h }; break }
        catch { if (-not $retry -or $hc.broken -or $k -ge 3) { throw }; Start-Sleep -Milliseconds 1700 }
    }
    $px = [Convert]::FromBase64String([string]$f['pixelsB64'])
    if ([int]$f['width'] -ne $w -or [int]$f['height'] -ne $h -or $px.Length -ne $w * $h * 4) { throw "帧尺寸 $($f['width'])x$($f['height'])($($px.Length) 字节)≠ ${w}x${h}" }
    return @{ px = $px; hash = (Get-Hex $px); draws = (Get-Key $f 'draws'); nz = (Get-Key $f 'nonZeroPixels'); tris = (Get-Key $f 'triangles'); device = (Get-Key $f 'deviceName') }
}
function Save-Png([byte[]]$rgba, [int]$w, [int]$h, [string]$path) {
    $bmp = New-Object Drawing.Bitmap $w, $h, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $bd = $bmp.LockBits((New-Object Drawing.Rectangle 0, 0, $w, $h), [Drawing.Imaging.ImageLockMode]::WriteOnly, $bmp.PixelFormat)
    $bgra = [F4Px]::RgbaToBgra($rgba)
    for ($y = 0; $y -lt $h; $y++) { [Runtime.InteropServices.Marshal]::Copy($bgra, $y * $w * 4, [IntPtr]($bd.Scan0.ToInt64() + $y * $bd.Stride), $w * 4) }
    $bmp.UnlockBits($bd); $bmp.Save($path, [Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
# 3 列 × 2 行:rurix、F+ D3D12、F+ Vulkan / Mobile、Compatibility、F+ D3D12 与 rurix 的差 ×Gain;每格上方标签条。
function Save-Montage($cells, [int]$w, [int]$h, [string]$path) {
    $cols = 3; $lab = 26; $pad = 8
    $canvas = New-Object Drawing.Bitmap ($cols * ($w + $pad) + $pad), (2 * ($h + $lab + $pad) + $pad)
    $g = [Drawing.Graphics]::FromImage($canvas); $g.Clear([Drawing.Color]::FromArgb(32, 33, 38)); $font = New-Object Drawing.Font 'Microsoft YaHei', 11
    for ($i = 0; $i -lt $cells.Count; $i++) {
        $x = $pad + ($i % $cols) * ($w + $pad); $y = $pad + [Math]::Floor($i / $cols) * ($h + $lab + $pad)
        $g.DrawString($cells[$i].label, $font, [Drawing.Brushes]::Gainsboro, [single]$x, [single]($y + 3))
        if ($cells[$i].png -and (Test-Path -LiteralPath $cells[$i].png)) { $im = [Drawing.Image]::FromFile($cells[$i].png); $g.DrawImage($im, [int]$x, [int]($y + $lab), $w, $h); $im.Dispose() }
    }
    $font.Dispose(); $g.Dispose(); $canvas.Save($path, [Drawing.Imaging.ImageFormat]::Png); $canvas.Dispose()
}


# ---- main ----
$started = Get-Date
$tbl = @($SceneTable | Where-Object { $Only.Count -eq 0 -or $Only -contains $_.name })
if ($tbl.Count -eq 0) { Log "-Only $($Only -join ',') 没有匹配的场景"; exit 2 }
$rtDll = (Get-FileHash -LiteralPath (Join-Path $Runtime 'bin\godot_host.dll') -Algorithm SHA256).Hash
$ehSha = (Get-FileHash -LiteralPath $engineHost -Algorithm SHA256).Hash
Log ("多场景对照:{0} 个场景 × {1} 路;运行时 {2}(dll {3});engine-host {4};rurix 钉卡 {5}(VK_DRIVER_FILES={6})" -f $tbl.Count, $Configs.Count,
    $Runtime, $rtDll.Substring(0, 12), $ehSha.Substring(0, 12), $Device, $(if ($vkIcd) { $vkIcd } else { '(未钉)' }))
$scenes = New-Object Collections.Generic.List[object]; $allOk = $true
foreach ($st in $tbl) {
    $sc = $json.DeserializeObject([IO.File]::ReadAllText((Join-Path $fixRoot "scenes\$($st.name).json"), [Text.Encoding]::UTF8))
    $w = [int]$sc['width']; $h = [int]$sc['height']; $proj = [string]$sc['project']
    $pv = Get-Key $sc 'probes'; $probes = if ($pv) { @($pv) } else { @() }
    $lv = Get-Key $sc 'probeLabels'; $plabels = if ($lv) { @($lv) } else { @() }
    $entry = [ordered]@{ scene = $st.name; kind = $st.kind; target = $st.target; project = $proj; size = @($w, $h); probeLabels = $plabels; configs = [ordered]@{}; pass = $true; problems = @() }
    Log "== $($st.name):$($st.kind)$(if ($null -ne $st.target) { ";目标 平均差 ≤ $($st.target)" })"
    $ref = $null; $diffCell = $null; $cells = New-Object Collections.Generic.List[object]
    foreach ($cfg in $Configs) {
        $hc = $null; $r = [ordered]@{}; $png = $null; $label = $cfg.tag
        try {
            if ($proj -eq 'demo') { $root = Join-Path $repo 'projects\demo' }
            else { $root = Join-Path $work ('{0}-{1}-{2}' -f $proj, $st.name, $cfg.tag); Copy-Item -LiteralPath (Join-Path $fixRoot $proj) -Destination $root -Recurse -Force }
            $hc = Start-HostProc $cfg $root (Get-Key $sc 'env')
            $r.readySec = $hc.readySec
            if ($cfg.kind -eq 'rurix') { Start-Sleep -Milliseconds 1700 }
            Build-FixtureScene $hc $sc $root
            $retry = ($cfg.kind -eq 'rurix')
            $wf = Get-Key $sc 'warmFrames'; if ($wf) { for ($k = 0; $k -lt [int]$wf; $k++) { $null = Get-Frame $hc $w $h $retry } }
            $f1 = Get-Frame $hc $w $h $retry; $f2 = Get-Frame $hc $w $h $retry
            $png = Join-Path $ev "$($st.name)-$($cfg.tag).png"; Save-Png $f1.px $w $h $png
            $r.draws = $f1.draws; $r.triangles = $f1.tris; $r.nonZero = $f1.nz; $r.device = $f1.device
            $r.sha256 = $f1.hash; $r.stable = ($f1.hash -eq $f2.hash)
            if ($probes.Count) { $r.probes = @($probes | ForEach-Object { , ([F4Px]::BoxMean($f1.px, $w, $h, [int]$_[0], [int]$_[1], 2)) }) }
            if ($cfg.kind -eq 'rurix') { $ref = $f1.px; $label = "rurix  draws $($f1.draws)" }
            elseif ($ref) {
                $s = [F4Px]::Stats($ref, $f1.px)
                $r.maxDiff = [int]$s[0]; $r.meanDiff = [Math]::Round($s[1], 3); $r.fracOver2 = [Math]::Round($s[2], 6); $r.fracOver8 = [Math]::Round($s[3], 6)
                $r.over2Px = [long]$s[4]; $r.over8Px = [long]$s[5]
                $fg = [F4Px]::FgStats($ref, $f1.px)
                $r.fgPx = [long]$fg[0]; $r.fgMaxDiff = [int]$fg[1]; $r.fgMeanDiff = [Math]::Round($fg[2], 3); $r.fgOver8Px = [long]$fg[3]
                $dpng = Join-Path $ev "$($st.name)-diff-$($cfg.tag).png"; Save-Png ([F4Px]::Diff($ref, $f1.px, $Gain)) $w $h $dpng
                if ($cfg.tag -eq 'forward_plus-d3d12') { $diffCell = $dpng }
                if ($null -ne $st.target) { $r.targetMet = ($r.meanDiff -le $st.target) }
                $label = '{0}  mean {1:N2}  max {2}' -f $cfg.tag, $r.meanDiff, $r.maxDiff
            } else { $label = "$($cfg.tag)(没有 rurix 参照)" }
        } catch { $r.error = "$_"; $label = "$($cfg.tag):失败" }
        finally { Stop-HostProc $hc }
        $bad = @()
        if ($r.error) { $bad += "出帧失败:$($r.error)" }
        else {
            if ([long]$r.nonZero -le 0) { $bad += '空帧(nonZeroPixels = 0)' }
            if ($r.stable -ne $true) { $bad += '两帧哈希不同' }
            if ($r.Contains('targetMet') -and -not $r.targetMet) { $bad += "平均差 $($r.meanDiff) > 目标 $($st.target)" }
            if ($cfg.kind -eq 'godot' -and -not $ref) { $bad += '没有 rurix 参照,无法比较' }
        }
        if ($bad.Count) { $entry.pass = $false; $allOk = $false; foreach ($b in $bad) { $entry.problems += "$($cfg.tag):$b" } }
        $entry.configs[$cfg.tag] = $r
        $cells.Add(@{ label = $label; png = $png })
        $msg = if ($r.error) { "✗ $($r.error)" } else {
            'draws={0} tris={1} nz={2} stable={3}{4}' -f $r.draws, $r.triangles, $r.nonZero, $r.stable,
            $(if ($r.Contains('meanDiff')) { ' mean={0} max={1} >2={2}px >8={3}px fg={4}px fgMean={5}{6}' -f $r.meanDiff, $r.maxDiff, $r.over2Px, $r.over8Px, $r.fgPx, $r.fgMeanDiff, $(if ($r.Contains('targetMet')) { " 达标=$($r.targetMet)" }) }) }
        Log ('  {0,-26} ready {1,4}s {2}' -f $cfg.tag, $r.readySec, $msg)
    }
    $cells.Add(@{ label = "F+ D3D12 与 rurix 的差 ×$Gain"; png = $diffCell })
    Save-Montage $cells $w $h (Join-Path $ev "$($st.name)-side-by-side.png")
    $scenes.Add($entry)
}


# ---- 汇总 ----
$code = if ($allOk) { 0 } else { 1 }
$result = if ($allOk) { 'PASS' } else { 'FAIL' }
$secs = [int]((Get-Date) - $started).TotalSeconds
$gitHead = ''; try { $gitHead = [string](git -C $repo rev-parse --short HEAD) } catch {}
$out = [ordered]@{ schema = 'forge.f4.side_by_side.v1'; createdAt = $started.ToString('o'); seconds = $secs; gitHead = $gitHead
    runtime = [ordered]@{ dir = $Runtime; dllSha256 = $rtDll }; engineHost = [ordered]@{ exe = $engineHost; sha256 = $ehSha; device = $Device; vkDriverFiles = $vkIcd }
    gain = $Gain; result = $result; exitCode = $code; scenes = $scenes }
[IO.File]::WriteAllText((Join-Path $ev 'summary.json'), (ConvertTo-Json -InputObject $out -Depth 10), $utf8)

function Esc([string]$s) { return ($s -replace '\|', '\|' -replace '\r?\n', ' ') }
$gcfg = @($Configs | Where-Object { $_.kind -eq 'godot' } | ForEach-Object { $_.tag })
$md = New-Object Collections.Generic.List[string]
$md.Add("# Stage 5 side-by-side ($Fixtures) / Stage 4 多场景画面对照 $ts"); $md.Add('')
$md.Add(('- 结论:**{0}**(exit {1});{2} 个场景 × 5 路,用时 {3} s;git {4}。' -f $result, $code, $tbl.Count, $secs, $gitHead))
$md.Add(('- godot 运行时 `{0}`(godot_host.dll sha256 `{1}`);rurix `{2}`(sha256 `{3}`),Vulkan 钉到 {4}。' -f $Runtime, $rtDll, $engineHost, $ehSha, $Device))
$md.Add('- 差值 = 与 rurix 同尺寸帧逐像素比:最大 = 最大通道差;平均 = 全帧 RGB 平均绝对差(与 g4util::stats 相同);>2 / >8 = 最大通道差超过 2 / 8 的像素个数(全帧 = 宽 × 高)。')
$md.Add('- 前景 = rurix 帧里与左上角清屏色不同的像素(补充统计,不进判据)。Mobile 的 3D 缓冲是 RGB10A2,暗清屏色量化后每个背景像素差 2-3,全帧平均差因此整体抬高;前景平均差不受清屏色影响。')
$md.Add('- 判据:五路都出帧且非空帧、两帧 sha256 相同;有目标的场景(缺省灯光)四种 godot 配置的平均差 ≤ 目标。含 Light 的场景 rurix 不读 Light,只作对照。')
$md.Add(("- 每个场景的拼图 ``<场景>-side-by-side.png``:rurix、F+ D3D12、F+ Vulkan / Mobile、Compatibility、F+ D3D12 与 rurix 的差 ×{0}。" -f $Gain)); $md.Add('')
$md.Add('## 缺省灯光场景目标'); $md.Add('')
$md.Add('| 场景 | 目标(平均差 ≤) | ' + ($gcfg -join ' | ') + ' | 达标 |')
$md.Add('|---|---|' + (($gcfg | ForEach-Object { '---' }) -join '|') + '|---|')
foreach ($e in $scenes) {
    if ($null -eq $e.target) { continue }
    $vals = @($gcfg | ForEach-Object { $c = $e.configs[$_]; if ($c -and $c.Contains('meanDiff')) { '{0:N3}' -f $c.meanDiff } else { '✗' } })
    $met = @($gcfg | Where-Object { $c = $e.configs[$_]; -not ($c -and $c.targetMet -eq $true) }).Count -eq 0
    $md.Add(('| {0} | {1} | {2} | {3} |' -f $e.scene, $e.target, ($vals -join ' | '), $(if ($met) { '是' } else { '**否**' })))
}
$md.Add('')
foreach ($e in $scenes) {
    $md.Add("## $($e.scene)"); $md.Add('')
    $md.Add("$(Esc $e.kind)。项目 ``$($e.project)``,$($e.size[0])×$($e.size[1])$(if ($null -ne $e.target) { ",目标 平均差 ≤ $($e.target)" })。"); $md.Add('')
    $md.Add('| 配置 | draws | 三角形 | nonZero | 两帧相同 | 最大差 | 平均差 | >2 像素 | >8 像素 | 前景像素 | 前景平均差 | 前景 >8 | 达标 |')
    $md.Add('|---|---|---|---|---|---|---|---|---|---|---|---|---|')
    foreach ($t in @($Configs | ForEach-Object { $_.tag })) {
        $c = $e.configs[$t]
        if ($c.error) { $md.Add(('| {0} | ✗ {1} | | | | | | | | | | | |' -f $t, (Esc $c.error))); continue }
        $md.Add(('| {0} | {1} | {2} | {3} | {4} | {5} | {6} | {7} | {8} | {9} | {10} | {11} | {12} |' -f $t, $c.draws, $c.triangles, $c.nonZero, $(if ($c.stable) { '是' } else { '**否**' }),
            $c.maxDiff, $c.meanDiff, $c.over2Px, $c.over8Px, $c.fgPx, $c.fgMeanDiff, $c.fgOver8Px, $(if ($c.Contains('targetMet')) { if ($c.targetMet) { '是' } else { '**否**' } } else { '—' })))
    }
    if ($e.probeLabels.Count) {
        $md.Add(''); $md.Add('探针(5×5 方框平均 RGB;括号里是与 rurix 的最大通道差):'); $md.Add('')
        $md.Add('| 探针 | rurix | ' + ($gcfg -join ' | ') + ' |'); $md.Add('|---|---|' + (($gcfg | ForEach-Object { '---' }) -join '|') + '|')
        $rp = $e.configs['rurix'].probes
        for ($i = 0; $i -lt $e.probeLabels.Count; $i++) {
            $rv = if ($rp) { $rp[$i] } else { $null }
            $cellsTxt = @($gcfg | ForEach-Object {
                $gp = $e.configs[$_].probes
                if (-not $gp) { '✗' } else {
                    $v = $gp[$i]; $dd = if ($rv) { ' ({0:N0})' -f (@(0, 1, 2 | ForEach-Object { [Math]::Abs($v[$_] - $rv[$_]) }) | Measure-Object -Maximum).Maximum } else { '' }
                    ('{0:N0},{1:N0},{2:N0}' -f $v[0], $v[1], $v[2]) + $dd
                } })
            $rtxt = if ($rv) { '{0:N0},{1:N0},{2:N0}' -f $rv[0], $rv[1], $rv[2] } else { '✗' }
            $md.Add(('| {0} | {1} | {2} |' -f $e.probeLabels[$i], $rtxt, ($cellsTxt -join ' | ')))
        }
    }
    if ($e.problems.Count) { $md.Add(''); $md.Add('不合格:'); foreach ($p in $e.problems) { $md.Add("- $(Esc $p)") } }
    $md.Add('')
}
[IO.File]::WriteAllLines((Join-Path $ev 'summary.md'), $md, $utf8)
foreach ($e in $scenes) { Log ('{0,-16} {1}{2}' -f $e.scene, $(if ($e.pass) { 'PASS' } else { 'FAIL' }), $(if ($e.problems.Count) { ':' + ($e.problems -join ';') })) }
Log ("{0}:输出 {1}(summary.json / summary.md / 每场景拼图)exit {2}" -f $result, $ev, $code)
exit $code
