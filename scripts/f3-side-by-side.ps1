# Stage 3 帧对比:同一场景(3 立方体 + 相机,与桌面冒烟同一布场)分别用 rurix(engine-host)和 godot 四种配置出帧,
# 存每帧 PNG + sha256,拼一张并排对比图;godot 每种配置连续取两帧,哈希必须相同(帧稳定性)。
# 直连宿主 JSON-RPC(4 字节小端长度 + JSON,一条长连接);对端关连接立即报错,不会死等;所有等待有上限。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts/f3-side-by-side.ps1 [-W 640 -H 360] ; exit 0 = 全部出帧且 godot 两帧哈希一致
param([int]$W = 640, [int]$H = 360, [string]$Device = 'Intel(R) Graphics')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing, System.Web.Extensions
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$ts = Get-Date -Format 'yyyyMMdd-HHmmss'
$ev = Join-Path $root "evidence\godot-backend\side-by-side-$ts"; New-Item -ItemType Directory -Force $ev | Out-Null
$logFile = Join-Path $ev 'run.log'
function Log($m) { $line = '[{0}] {1}' -f (Get-Date -Format 'HH:mm:ss'), $m; $line; [IO.File]::AppendAllText($logFile, "$line`r`n", [Text.Encoding]::UTF8) }
$json = New-Object System.Web.Script.Serialization.JavaScriptSerializer; $json.MaxJsonLength = [int]::MaxValue; $json.RecursionLimit = 256
$sha = [Security.Cryptography.SHA256]::Create()
function Read-Exact($s, [byte[]]$b) { $o = 0; while ($o -lt $b.Length) { $k = $s.Read($b, $o, $b.Length - $o); if ($k -le 0) { throw '宿主关闭了连接(EOF)' }; $o += $k } }
$script:rpcId = 0
function Rpc($hc, [string]$method, $params) {
    $script:rpcId++
    $body = [Text.Encoding]::UTF8.GetBytes($json.Serialize(@{ jsonrpc = '2.0'; id = $script:rpcId; method = $method; params = $params }))
    $hc.s.Write([BitConverter]::GetBytes([uint32]$body.Length), 0, 4); $hc.s.Write($body, 0, $body.Length)
    $hd = New-Object byte[] 4; Read-Exact $hc.s $hd
    $buf = New-Object byte[] ([BitConverter]::ToUInt32($hd, 0)); Read-Exact $hc.s $buf
    $r = $json.DeserializeObject([Text.Encoding]::UTF8.GetString($buf))
    if ($r.ContainsKey('error') -and $r['error']) { throw "${method}: $($r['error']['message'])" }
    return $r['result']
}
function Resolve-VkIcd([string]$device) {
    $cls = 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}'
    foreach ($k in (Get-ChildItem $cls -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -match '^\d{4}$' })) {
        $p = Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue
        if ($p -and $p.DriverDesc -eq $device) { foreach ($f in @($p.VulkanDriverName)) { if ($f -and (Test-Path -LiteralPath $f)) { return $f } } }
    }
    return ''
}
# 起宿主并连上:rurix = engine-host.exe --port 0(Vulkan 钉到基线用的卡);godot = 运行时目录的 forge-godot_console.exe(与测试 / 监督器同参)。
function Start-Host([string]$backend, [string]$method = '', [string]$driver = '') {
    if ($backend -eq 'rurix') {
        $psi = New-Object Diagnostics.ProcessStartInfo (Join-Path $root 'target\debug\engine-host.exe'), '--port 0'
        $psi.WorkingDirectory = $root
        $icd = Resolve-VkIcd $Device; if ($icd) { $psi.EnvironmentVariables['VK_DRIVER_FILES'] = $icd }
        if ($psi.EnvironmentVariables.ContainsKey('FORGE_HOST_PORT')) { $psi.EnvironmentVariables.Remove('FORGE_HOST_PORT') }
    } else {
        $rt = Join-Path $root 'target\godot-runtime'
        $psi = New-Object Diagnostics.ProcessStartInfo (Join-Path $rt 'forge-godot_console.exe'), "--rendering-method $method --rendering-driver $driver --audio-driver Dummy"
        $psi.WorkingDirectory = $rt
        $psi.EnvironmentVariables['FORGE_HOST_PORT'] = '0'; $psi.EnvironmentVariables['FORGE_RENDER_BACKEND'] = 'godot'
        $psi.EnvironmentVariables['FORGE_RENDER_METHOD'] = $method; $psi.EnvironmentVariables['FORGE_RENDER_DRIVER'] = $driver
    }
    $psi.EnvironmentVariables['FORGE_PROJECT_ROOT'] = Join-Path $root 'projects\demo'
    if ($psi.EnvironmentVariables.ContainsKey('FORGE_GPU_PARTICLES')) { $psi.EnvironmentVariables.Remove('FORGE_GPU_PARTICLES') }
    $psi.UseShellExecute = $false; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
    $p = [Diagnostics.Process]::Start($psi)
    $null = $p.StandardError.ReadToEndAsync()
    $lines = New-Object Collections.Generic.List[string]; $port = 0; $deadline = (Get-Date).AddSeconds(60)
    while ((Get-Date) -lt $deadline) {
        $t = $p.StandardOutput.ReadLineAsync()
        if (-not $t.Wait([Math]::Max(1, [int]($deadline - (Get-Date)).TotalMilliseconds))) { break }
        $l = $t.Result; if ($null -eq $l) { break }; $lines.Add($l)
        if ($l -match '^FORGE_HOST_LISTENING port=(\d+)') { $port = [int]$Matches[1]; break }
    }
    if ($port -eq 0) { try { $p.Kill() } catch {}; throw "$backend $method/$driver 60s 内没有就绪行:$($lines -join ' | ')" }
    $null = $p.StandardOutput.ReadToEndAsync() # 就绪后继续排空 stdout(02 §6.2),否则管道写满宿主会卡住
    $c = New-Object Net.Sockets.TcpClient('127.0.0.1', $port); $c.ReceiveTimeout = 60000; $c.SendTimeout = 60000
    return @{ proc = $p; port = $port; client = $c; s = $c.GetStream() }
}
function Stop-Host($hc) { if (-not $hc) { return }; try { $hc.client.Close() } catch {}; if (-not $hc.proc.HasExited) { try { $hc.proc.Kill() } catch {} }; [void]$hc.proc.WaitForExit(10000) }

Add-Type -TypeDefinition @'
public static class F3Px {
    public static byte[] RgbaToBgra(byte[] s) { var d = new byte[s.Length]; for (int i = 0; i + 3 < s.Length; i += 4) { d[i] = s[i + 2]; d[i + 1] = s[i + 1]; d[i + 2] = s[i]; d[i + 3] = 255; } return d; }
    public static int MaxDiff(byte[] a, byte[] b) { int m = 0; for (int i = 0; i < a.Length && i < b.Length; i++) { if ((i & 3) == 3) continue; int d = a[i] > b[i] ? a[i] - b[i] : b[i] - a[i]; if (d > m) m = d; } return m; }
    public static double FracOver(byte[] a, byte[] b, int tol) { long n = 0, k = 0; for (int i = 0; i + 3 < a.Length && i + 3 < b.Length; i += 4) { n++; for (int c = 0; c < 3; c++) { int d = a[i + c] > b[i + c] ? a[i + c] - b[i + c] : b[i + c] - a[i + c]; if (d > tol) { k++; break; } } } return n == 0 ? 0 : (double)k / n; }
}
'@
function To-Bitmap([byte[]]$rgba, [int]$w, [int]$h) {
    $bmp = New-Object Drawing.Bitmap $w, $h, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $bd = $bmp.LockBits((New-Object Drawing.Rectangle 0, 0, $w, $h), [Drawing.Imaging.ImageLockMode]::WriteOnly, $bmp.PixelFormat)
    $bgra = [F3Px]::RgbaToBgra($rgba)
    for ($y = 0; $y -lt $h; $y++) { [Runtime.InteropServices.Marshal]::Copy($bgra, $y * $w * 4, [IntPtr]($bd.Scan0.ToInt64() + $y * $bd.Stride), $w * 4) }
    $bmp.UnlockBits($bd); return $bmp
}
function Build-Scene($hc) {
    Rpc $hc 'scene.new' @{ name = 'f3-side-by-side' } | Out-Null
    Rpc $hc 'entity.create' @{ name = 'cube-a'; translation = @(0.0, 0.5, 0.0); components = @(@{ type = 'MeshRenderer'; enabled = $true; props = @{ mesh = 'cube'; material = 'a' } }) } | Out-Null
    Rpc $hc 'entity.create' @{ name = 'cube-b'; translation = @(1.6, 0.5, 0.4); rotation = @(0.0, 0.3826834, 0.0, 0.9238795); components = @(@{ type = 'MeshRenderer'; enabled = $true; props = @{ mesh = 'cube'; material = 'b' } }) } | Out-Null
    Rpc $hc 'entity.create' @{ name = 'cube-c'; translation = @(-1.6, 0.9, -0.4); scale = @(1.0, 1.8, 1.0); components = @(@{ type = 'MeshRenderer'; enabled = $true; props = @{ mesh = 'cube'; material = 'c' } }) } | Out-Null
    Rpc $hc 'viewport.setCamera' @{ target = @(0.0, 0.6, 0.0); yaw = 30.0; pitch = 24.0; dist = 6.5 } | Out-Null
}
function Get-Frame($hc) {
    $f = Rpc $hc 'viewport.frame' @{ width = $W; height = $H }
    $px = [Convert]::FromBase64String($f['pixelsB64'])
    return @{ px = $px; hash = ([BitConverter]::ToString($sha.ComputeHash($px))).Replace('-', ''); w = [int]$f['width']; h = [int]$f['height']; draws = $f['draws']; nz = $f['nonZeroPixels']; device = $f['deviceName']; path = $f['framePath'] }
}

$configs = @(@('rurix', '', ''), @('godot', 'forward_plus', 'd3d12'), @('godot', 'forward_plus', 'vulkan'), @('godot', 'mobile', 'd3d12'), @('godot', 'gl_compatibility', 'opengl3'))
$res = @(); $ok = $true; $ref = $null
foreach ($c in $configs) {
    $tag = if ($c[0] -eq 'rurix') { 'rurix' } else { "godot-$($c[1])-$($c[2])" }
    $hh = $null
    try {
        $t0 = Get-Date; $hh = Start-Host $c[0] $c[1] $c[2]; $readyS = ((Get-Date) - $t0).TotalSeconds
        $bi = Rpc $hh 'render.backendInfo' @{}
        Build-Scene $hh
        $f1 = Get-Frame $hh; $f2 = Get-Frame $hh
        if ($f1.w -ne $W -or $f1.h -ne $H) { throw "帧尺寸 $($f1.w)x$($f1.h) ≠ ${W}x${H}" }
        $bmp = To-Bitmap $f1.px $W $H; $bmp.Save((Join-Path $ev "$tag.png"), [Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
        $stable = $f1.hash -eq $f2.hash
        $r = [ordered]@{ tag = $tag; readySec = [Math]::Round($readyS, 1); draws = $f1.draws; nonZero = $f1.nz; device = $f1.device; framePath = $f1.path; sha256 = $f1.hash; stable = $stable; backendInfo = $bi }
        if ($c[0] -eq 'rurix') { $ref = $f1.px } elseif ($ref) { $r.maxDiffVsRurix = [F3Px]::MaxDiff($ref, $f1.px); $r.fracOver2VsRurix = [Math]::Round([F3Px]::FracOver($ref, $f1.px, 2), 4) }
        Log ("{0,-32} ready {1,4}s draws={2} nonZero={3} stable={4} sha={5} maxDiffVsRurix={6} >2:{7} dev={8}" -f $tag, $r.readySec, $r.draws, $r.nonZero, $stable, $f1.hash.Substring(0, 12), $r.maxDiffVsRurix, $r.fracOver2VsRurix, $f1.device)
        if (-not $stable -or [int]$f1.nz -le 0 -or [int]$f1.draws -lt 3) { $ok = $false; Log "  ✗ $tag 不合格(两帧哈希不同 / 空帧 / draws<3)" }
        $res += $r
    } catch { $ok = $false; Log "  ✗ $tag 失败:$_"; $res += [ordered]@{ tag = $tag; error = "$_" } }
    finally { Stop-Host $hh }
}
# 并排拼图:3 列 × 2 行,每格上方标签条
$cols = 3; $lab = 26; $pad = 8
$canvas = New-Object Drawing.Bitmap ($cols * ($W + $pad) + $pad), (2 * ($H + $lab + $pad) + $pad)
$g = [Drawing.Graphics]::FromImage($canvas); $g.Clear([Drawing.Color]::FromArgb(32, 33, 38)); $font = New-Object Drawing.Font 'Microsoft YaHei', 11
for ($i = 0; $i -lt $res.Count; $i++) {
    $x = $pad + ($i % $cols) * ($W + $pad); $y = $pad + [Math]::Floor($i / $cols) * ($H + $lab + $pad)
    $r = $res[$i]; $txt = if ($r.error) { "$($r.tag):失败" } else { "$($r.tag)  draws $($r.draws)" + $(if ($null -ne $r.maxDiffVsRurix) { "  vs rurix max Δ $($r.maxDiffVsRurix)" } else { '' }) }
    $g.DrawString($txt, $font, [Drawing.Brushes]::Gainsboro, [single]$x, [single]($y + 3))
    $pf = Join-Path $ev "$($r.tag).png"; if (Test-Path $pf) { $im = [Drawing.Image]::FromFile($pf); $g.DrawImage($im, [int]$x, [int]($y + $lab), $W, $H); $im.Dispose() }
}
$g.Dispose(); $canvas.Save((Join-Path $ev 'side-by-side.png'), [Drawing.Imaging.ImageFormat]::Png); $canvas.Dispose()
[IO.File]::WriteAllText((Join-Path $ev 'summary.json'), ($res | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding $false))
Log $(if ($ok) { "PASS:5 路全部出帧,godot 四种配置两帧哈希一致;拼图 $ev\side-by-side.png" } else { 'FAIL(见上)' })
exit $(if ($ok) { 0 } else { 1 })
