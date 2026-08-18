# F6 wave.4 打包门栈级冒烟(G-F6-4):project-pack + engine-host --game。
# 断言:①pack 报告闭包 = maze 场景+4 图+maze.rx 全量 .meta(无缺);闭包外资产(Main/call_probe/纹理等)不入包
#   ②产物磁盘布局:Content 树 + .forge/cache/rxdll/maze-*.dll + bin/engine-host.exe + pack-run.ps1
#   ③拷贝至干净目录(脱离 workspace)经 pack-run.ps1 启动 --game:LISTENING + GAME_BOOTED + scene.summary 36 实体 play_running
#   ④viewport_frame nonZeroPixels>0(出帧);⑤entity.create 被 -32601 拒(编辑面裁剪)
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f6-w4-pack-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f6-w4-pack-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function Post-Json($url, $bodyObj, $timeoutSec = 120) {
  $body = $bodyObj | ConvertTo-Json -Depth 12 -Compress
  $req = [System.Net.HttpWebRequest]::Create($url)
  $req.Method = 'POST'
  $req.ContentType = 'application/json; charset=utf-8'
  $req.Timeout = $timeoutSec * 1000
  $bytes = [Text.Encoding]::UTF8.GetBytes($body)
  $req.ContentLength = $bytes.Length
  $st = $req.GetRequestStream(); $st.Write($bytes, 0, $bytes.Length); $st.Close()
  try { $resp = $req.GetResponse() }
  catch [System.Net.WebException] {
    $resp = $_.Exception.Response
    if ($null -eq $resp) { throw }
    $sr = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
    $errBody = $sr.ReadToEnd(); $sr.Close()
    throw "HTTP $([int]$resp.StatusCode): $errBody"
  }
  $sr = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
  $text = $sr.ReadToEnd(); $sr.Close()
  return $text
}
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
  $len = [BitConverter]::GetBytes([int]$payload.Length) # x64 小端
  $stream.Write($len, 0, 4); $stream.Write($payload, 0, $payload.Length); $stream.Flush()
  $lenBuf = New-Object byte[] 4
  Read-Full $stream $lenBuf 4
  $n = [BitConverter]::ToInt32($lenBuf, 0)
  $buf = New-Object byte[] $n
  Read-Full $stream $buf $n
  return [Text.Encoding]::UTF8.GetString($buf) | ConvertFrom-Json
}

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$script:startTime = Get-Date
$packOut = Join-Path $env:TEMP "forge-f6w4-pack-$ts"
$cleanDir = Join-Path $env:TEMP "forge-f6w4-clean-$ts"
$engineProc = $null
try {
  # ── 0. 构建新鲜二进制(engine-host 本波改动须落盘)──
  Log "== cargo build engine-host + forge-agentd =="
  $ErrorActionPreference = 'Continue'
  cargo build -p engine-host -p forge-agentd 2>&1 | Out-Null
  $buildExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($buildExit -ne 0) { throw "cargo build 失败(exit=$buildExit)" }
  Log "构建 PASS"

  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪"

  # ── 1. project/pack:maze 场景 → 独立目录 ──
  Log "== POST /api/forge/project/pack =="
  $rep = Post-Json "http://127.0.0.1:8103/api/forge/project/pack" @{ sceneRef = "projects/demo/Content/Scenes/maze.rxscene"; outDir = $packOut } 120 | ConvertFrom-Json
  $closure = @($rep.closure)
  Log ("  闭包 {0} 项:{1}" -f $closure.Count, ($closure -join ', '))
  $need = @(
    'Content/Scenes/maze.rxscene', 'Content/Scenes/maze.rxscene.meta',
    'Content/Graphs/maze_player.rxgraph', 'Content/Graphs/maze_player.rxgraph.meta',
    'Content/Graphs/maze_key.rxgraph', 'Content/Graphs/maze_key.rxgraph.meta',
    'Content/Graphs/maze_door.rxgraph', 'Content/Graphs/maze_door.rxgraph.meta',
    'Content/Graphs/maze_goal.rxgraph', 'Content/Graphs/maze_goal.rxgraph.meta',
    'Content/Scripts/maze.rx')
  foreach ($n in $need) { if (-not ($closure -contains $n)) { throw "闭包缺件: $n" } }
  Log "  闭包 11 项核心资产无缺"
  $banned = @(
    'Content/Scenes/Main.rxscene', 'Content/Graphs/call_probe.rxgraph',
    'Content/Graphs/door_opener.rxgraph', 'Content/Graphs/f4w3_probe.rxgraph',
    'Content/Scripts/callprobe.rx', 'Content/Textures/wood_albedo.png',
    'Content/Materials/w4_mat.rxmat', 'Content/Meshes/f5w2_chair.gltf')
  foreach ($b in $banned) { if ($closure -contains $b) { throw "闭包外资产混入: $b" } }
  Log "  闭包外 8 抽样资产未入包(体积如实)"
  $filePaths = @($rep.files | ForEach-Object { $_.path })
  if (-not ($filePaths -contains 'bin/engine-host.exe')) { throw "产物缺 bin/engine-host.exe" }
  if (-not ($filePaths -contains 'pack-run.ps1')) { throw "产物缺 pack-run.ps1" }
  $rxdll = @($filePaths | Where-Object { $_ -like '.forge/cache/rxdll/maze-*.dll' })
  if ($rxdll.Count -lt 1) { throw "产物缺 rxdll maze dll(干净机 call_function 无缓存必 logic.call_error)" }
  Log ("  产物含引擎二进制 + 启动脚本 + rxdll:{0}" -f ($rxdll -join ','))
  if (@($rep.warnings | Where-Object { "$_" -like '*rxdll 缓存缺失*maze.rx*' }).Count -gt 0) { throw "maze.rx 缓存缺失警告不应出现: $($rep.warnings -join ';')" }
  Log ("  totalBytes={0} debugEngine={1} warnings={2}" -f $rep.totalBytes, $rep.debugEngine, (@($rep.warnings).Count))
  if ([int64]$rep.totalBytes -lt 1000000) { throw "体积过小不可信: $($rep.totalBytes)" }

  # ── 2. 磁盘布局核验 ──
  foreach ($n in $need) { if (-not (Test-Path (Join-Path $packOut $n))) { throw "磁盘缺件: $n" } }
  foreach ($b in @('Content/Scenes/Main.rxscene', 'Content/Textures/wood_albedo.png')) { if (Test-Path (Join-Path $packOut $b)) { throw "磁盘混入闭包外资产: $b" } }
  if (-not (Test-Path (Join-Path $packOut 'bin\engine-host.exe'))) { throw "磁盘缺 bin/engine-host.exe" }
  $scriptText = Get-Content (Join-Path $packOut 'pack-run.ps1') -Raw
  if ($scriptText -notmatch 'FORGE_PROJECT_ROOT' -or $scriptText -notmatch '--game "Content/Scenes/maze.rxscene"') { throw "pack-run.ps1 内容不符: $scriptText" }
  Log "磁盘布局核验 PASS(Content 树 + 缓存 + bin + 脚本)"

  # ── 3. 拷贝至干净目录(脱离 workspace)经 pack-run.ps1 启动 --game ──
  Log "== 干净目录启动(pack-run.ps1 → engine-host --game)=="
  Copy-Item -Recurse -Force $packOut $cleanDir
  $stdoutF = Join-Path $env:TEMP "forge-f6w4-engine-out-$ts.log"
  $stderrF = Join-Path $env:TEMP "forge-f6w4-engine-err-$ts.log"
  $engineProc = Start-Process -FilePath "powershell" -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $cleanDir 'pack-run.ps1')) -PassThru -WindowStyle Hidden -RedirectStandardOutput $stdoutF -RedirectStandardError $stderrF
  $booted = $false
  foreach ($i in 1..80) {
    Start-Sleep -Milliseconds 250
    $txt = if (Test-Path $stdoutF) { Get-Content $stdoutF -Raw -ErrorAction SilentlyContinue } else { '' }
    if ($txt -match 'FORGE_HOST_LISTENING port=17890' -and $txt -match 'FORGE_HOST_GAME_BOOTED scene=Content/Scenes/maze.rxscene') { $booted = $true; break }
    if ($engineProc.HasExited) { $err = if (Test-Path $stderrF) { Get-Content $stderrF -Raw -ErrorAction SilentlyContinue } else { '' }; throw "engine-host 提前退出(code=$($engineProc.ExitCode)): $err $txt" }
  }
  if (-not $booted) { $txt = if (Test-Path $stdoutF) { Get-Content $stdoutF -Raw -ErrorAction SilentlyContinue } else { '' }; $err = if (Test-Path $stderrF) { Get-Content $stderrF -Raw -ErrorAction SilentlyContinue } else { '' }; throw "engine-host --game 就绪超时: stdout=[$txt] stderr=[$err]" }
  Log "干净目录 LISTENING + GAME_BOOTED(PID=$($engineProc.Id))"

  # ── 4. TCP JSON-RPC:只读面出帧 + 编辑面拒 ──
  $client = New-Object System.Net.Sockets.TcpClient
  $client.Connect('127.0.0.1', 17890)
  $stream = $client.GetStream()
  try {
    $ping = Rpc-Call $stream 'host.ping' @{}
    if ($ping.result.pong -ne $true) { throw "host.ping 异常: $($ping | ConvertTo-Json -Compress)" }
    Log "  host.ping pong(backend=$($ping.result.backend) pid=$($ping.result.pid))"

    $sum = Rpc-Call $stream 'scene.summary' @{}
    if ([int]$sum.result.entityCount -ne 36) { throw "entityCount 须 36,实 $($sum.result.entityCount)" }
    if ($sum.result.playState -ne 'play_running') { throw "playState 须 play_running,实 $($sum.result.playState)" }
    Log "  scene.summary 36 实体 + play_running(--game 自动 scene_load+play_enter)"

    $cam = Rpc-Call $stream 'viewport.setCamera' @{ target = @(10.0, 0.0, 5.0); yawDeg = 90.0; pitchDeg = 62.0; dist = 24.0; fovYDeg = 55.0 }
    $frame = Rpc-Call $stream 'viewport.frame' @{ width = 960; height = 540 }
    $nz = [int]$frame.result.nonZeroPixels
    Log ("  viewport.frame {0}x{1} nonZeroPixels={2} draws={3} device={4}" -f $frame.result.width, $frame.result.height, $nz, $frame.result.draws, $frame.result.deviceName)
    if ($nz -le 0) { throw "出帧证据不足:nonZeroPixels 须 >0,实 $nz" }
    Log "  出帧 PASS(nonZeroPixels=$nz)"

    $edit = Rpc-Call $stream 'entity.create' @{ name = 'hack'; transform = @{ translation = @(0,0,0); rotation = @(0,0,0,1); scale = @(1,1,1) } }
    if ([int]$edit.error.code -ne -32601 -or "$($edit.error.message)" -notlike '*game*') { throw "编辑面须 -32601 拒: $($edit | ConvertTo-Json -Compress)" }
    Log "  entity.create 被拒(-32601 $($edit.error.message))——编辑面裁剪 PASS"
  } finally {
    $stream.Close(); $client.Close()
  }

  Log "F6 wave.4 打包门冒烟 PASS(G-F6-4)"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  if ($null -ne $engineProc) { try { if (-not $engineProc.HasExited) { $engineProc.Kill() } } catch {} }
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-scene-mcp', 'code-forge-mcp', 'asset-pipeline-mcp', 'gen-image-mcp', 'gen-model-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
}
