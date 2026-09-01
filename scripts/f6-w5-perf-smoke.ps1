# F6 wave.5 性能门栈级冒烟(G-F6-5,D-F6-E):迷宫场景 1080p 实测 ≥60fps。
# 方法:maze.rxscene play 态(--game 直启)→ viewport.setCamera 就位 → viewport.frame format=none
#   连续 N=300 帧墙钟计时 ×3 轮,fps=N/耗时,取均值/最小;format=none 渲染+统计照常、跳过像素
#   编码回传(1080p rgba8 pixelsB64 ≈11MB/帧,传输开销会污染渲染 fps 测量,本脚本附 1 帧 rgba8
#   校验 nonZeroPixels>0 证明测量期间渲染真实发生)。
# 留档:evidence/f6-w5-perf-{ts}.json(均值/最小/轮次明细/机器配置)+ .log;未达 60fps 如实 FAIL。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f6-w5-perf-smoke.ps1 ; exit 0 = PASS
param([string]$EngineBin = "target\release\engine-host.exe")
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f6-w5-perf-smoke-$ts.log"
$jsonFile = "evidence\f6-w5-perf-$ts.json"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function Read-Full($stream, [byte[]]$buf, $n) {
  $off = 0
  while ($off -lt $n) {
    $r = $stream.Read($buf, $off, $n - $off)
    if ($r -le 0) { throw "TCP 流提前关闭(读 $off/$n)" }
    $off += $r
  }
}
$script:rpcId = 0
function Rpc-Call($stream, $method, $params) {
  $script:rpcId += 1
  $msg = @{ jsonrpc = "2.0"; id = $script:rpcId; method = $method; params = $params } | ConvertTo-Json -Compress -Depth 10
  $payload = [Text.Encoding]::UTF8.GetBytes($msg)
  $len = [BitConverter]::GetBytes([int]$payload.Length)
  $stream.Write($len, 0, 4); $stream.Write($payload, 0, $payload.Length); $stream.Flush()
  $lenBuf = New-Object byte[] 4
  Read-Full $stream $lenBuf 4
  $n = [BitConverter]::ToInt32($lenBuf, 0)
  $buf = New-Object byte[] $n
  Read-Full $stream $buf $n
  return [Text.Encoding]::UTF8.GetString($buf) | ConvertFrom-Json
}

New-Item -ItemType Directory -Force evidence | Out-Null
$script:startTime = Get-Date
$engineProc = $null
$stdoutF = Join-Path $env:TEMP "forge-f6w5-engine-out-$ts.log"
$stderrF = Join-Path $env:TEMP "forge-f6w5-engine-err-$ts.log"
$report = [ordered]@{
  milestone = 'F6'; wave = 'wave.5'; gate = 'G-F6-5'; method = 'D-F6-E'
  scene = 'Content/Scenes/maze.rxscene'; resolution = '1920x1080'; framesPerRound = 300; rounds = 3
  timestamp = (Get-Date -Format 'o')
}
try {
  Log "== engine-host --game maze(play 态直启;$EngineBin)=="
  $env:FORGE_PROJECT_ROOT = Join-Path $root 'projects\demo'
  $engineProc = Start-Process -FilePath $EngineBin -ArgumentList @('--port', '17891', '--game', 'Content/Scenes/maze.rxscene') -PassThru -WindowStyle Hidden -RedirectStandardOutput $stdoutF -RedirectStandardError $stderrF
  $booted = $false
  foreach ($i in 1..80) {
    Start-Sleep -Milliseconds 250
    $txt = if (Test-Path $stdoutF) { Get-Content $stdoutF -Raw -ErrorAction SilentlyContinue } else { '' }
    if ($txt -match 'FORGE_HOST_LISTENING port=17891' -and $txt -match 'FORGE_HOST_GAME_BOOTED') { $booted = $true; break }
    if ($engineProc.HasExited) { $err = if (Test-Path $stderrF) { Get-Content $stderrF -Raw -ErrorAction SilentlyContinue } else { '' }; throw "engine-host 提前退出: $err $txt" }
  }
  if (-not $booted) { throw "engine-host 就绪超时" }
  Log "就绪(PID=$($engineProc.Id))"

  $client = New-Object System.Net.Sockets.TcpClient
  $client.Connect('127.0.0.1', 17891)
  $stream = $client.GetStream()
  try {
    # 相机就位:maze 网格中心俯视(同 w4 冒烟视角,36 实体全在视锥内)。
    Rpc-Call $stream 'viewport.setCamera' @{ target = @(10.0, 0.0, 5.0); yawDeg = 90.0; pitchDeg = 62.0; dist = 24.0; fovYDeg = 55.0 } | Out-Null

    # 诚实性校验:先取 1 帧 rgba8,证明渲染真实发生(nonZeroPixels>0)。
    $probe = Rpc-Call $stream 'viewport.frame' @{ width = 1920; height = 1080 }
    $nzProbe = [int64]$probe.result.nonZeroPixels
    $device = "$($probe.result.deviceName)"
    Log ("  rgba8 校验帧:nonZeroPixels={0} draws={1} device={2}" -f $nzProbe, $probe.result.draws, $device)
    if ($nzProbe -le 0) { throw "校验帧 nonZeroPixels 须 >0,实 $nzProbe" }
    Remove-Variable probe -ErrorAction SilentlyContinue
    [GC]::Collect()

    # D-F6-E:连续 N=300 帧墙钟 ×3 轮(段计时插桩:每 50 帧一段,慢段如实可见)。
    $roundFps = @()
    for ($r = 1; $r -le 3; $r++) {
      $sw = [System.Diagnostics.Stopwatch]::StartNew()
      $seg = [System.Diagnostics.Stopwatch]::StartNew()
      $last = $null
      for ($i = 0; $i -lt 300; $i++) {
        $last = Rpc-Call $stream 'viewport.frame' @{ width = 1920; height = 1080; format = 'none' }
        if ($null -eq $last.result -or [int]$last.result.width -ne 1920) { throw "帧响应异常: $($last | ConvertTo-Json -Compress -Depth 5)" }
        if (($i + 1) % 50 -eq 0) { Log ("    轮 {0} 帧 {1}-{2}: {3} ms" -f $r, ($i - 49), ($i + 1), $seg.ElapsedMilliseconds); $seg.Restart() }
      }
      $sw.Stop()
      $fps = [Math]::Round(300.0 / $sw.Elapsed.TotalSeconds, 2)
      $roundFps += $fps
      Log ("  轮 {0}: 300 帧 {1} ms → fps={2} (frames={3} draws={4})" -f $r, $sw.ElapsedMilliseconds, $fps, $last.result.frames, $last.result.draws)
      # none 档不回读:渲染真实性由前置 rgba8 校验帧 + draws>0 证明(nonZeroPixels 恒 0 属预期)。
      if ([int]$last.result.draws -le 0) { throw "轮 $r draws=0,场景未实际绘制,测量无效" }
    }
    $mean = [Math]::Round((($roundFps | Measure-Object -Average).Average), 2)
    $min = ($roundFps | Measure-Object -Minimum).Minimum
    Log ("  结果(渲染+提交产能口径,format=none):均值 fps={0} 最小 fps={1} 轮次=[{2}]" -f $mean, $min, ($roundFps -join ', '))

    # 对照口径(不判门,如实留档):rgba8 档端到端 100 帧——含 submit→wait 同步 readback
    # + 8MB 拷贝 + 2M 像素统计 + pixelsB64 编码/传输/解析,是编辑器远程视口回退腿成本,非游戏渲染产能。
    $sw2 = [System.Diagnostics.Stopwatch]::StartNew()
    for ($i = 0; $i -lt 100; $i++) { $null = Rpc-Call $stream 'viewport.frame' @{ width = 1920; height = 1080 } }
    $sw2.Stop()
    $e2e = [Math]::Round(100.0 / $sw2.Elapsed.TotalSeconds, 2)
    Log ("  对照(帧通道端到端口径,rgba8 100 帧):fps={0}({1} ms)——RD-F6-002 留档" -f $e2e, $sw2.ElapsedMilliseconds)

    # 机器配置留档。
    $cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name
    $ramGB = [Math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory / 1GB, 1)
    $os = (Get-CimInstance Win32_OperatingSystem).Caption
    $report.roundsFps = $roundFps; $report.meanFps = $mean; $report.minFps = $min
    $report.e2eRgba8Fps = $e2e
    $report.caliber = 'none 档 = 渲染+提交产能(游戏运行时语义,判门);rgba8 档 = 编辑器帧通道端到端(对照,readback 瓶颈 RD-F6-002)'
    $report.machine = @{ gpu = $device; cpu = $cpu; ramGB = $ramGB; os = "$os" }
    $report.binary = $EngineBin
    $report.binaryProfile = if ($EngineBin -like '*release*') { 'release' } else { 'debug' }
    $report.probeNonZeroPixels = $nzProbe
    $report.thresholdFps = 60
    $report.pass = ($mean -ge 60)
    ($report | ConvertTo-Json -Depth 6) | Set-Content $jsonFile -Encoding UTF8
    Log "  留档 $jsonFile"

    if ($mean -lt 60) { throw "性能门未达:均值 fps=$mean < 60(如实 FAIL,瓶颈分析见 json)" }
    Log "性能门 PASS(均值 $mean ≥ 60,最小 $min)"
  } finally {
    $stream.Close(); $client.Close()
  }

  Log "F6 wave.5 性能门冒烟 PASS(G-F6-5)"
  exit 0
} catch {
  Log "FAIL: $_"
  if (-not (Test-Path $jsonFile)) { ($report | ConvertTo-Json -Depth 6) | Set-Content $jsonFile -Encoding UTF8 }
  exit 1
} finally {
  Remove-Item Env:\FORGE_PROJECT_ROOT -ErrorAction SilentlyContinue
  if ($null -ne $engineProc) { try { if (-not $engineProc.HasExited) { $engineProc.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-host', 'engine-scene-mcp') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
}
