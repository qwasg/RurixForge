# F6 wave.2 回归矩阵门栈级冒烟(G-F6-2):迷宫原型 + test-matrix swarm 真并发 + skill 工具面对齐。
# 断言:①绿全矩阵(playtest/run)6/6 全过——迷宫 .rx call_function 链(墙碰撞/门钥匙/终点判定)
#   ②swarm test-matrix 2 分片并发:聚合 6 项全绿;分片 pid 互异 + 绝对时间窗重叠 + 运行中 engine-host 进程计数 ≥2
#   ③红子矩阵经 swarm 如实标红(errors=2,不充绿)
#   ④playtest-regression SKILL.md 声明工具在 MCP 面全部存在
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f6-w2-maze-matrix-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f6-w2-maze-matrix-smoke-$ts.log"
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

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$script:startTime = Get-Date
$script:maxEngineHost = 0
$script:maxSceneMcp = 0
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪"

  # ── 1. 绿全矩阵:迷宫 .rx call_function 链全通 ──
  Log "== 绿全矩阵 tests/maze/matrix.json(playtest/run)=="
  $g = Post-Json "http://127.0.0.1:8103/api/forge/playtest/run" @{ matrixRef = "tests/maze/matrix.json" } 300 | ConvertFrom-Json
  foreach ($c in $g.cases) { Log ("  {0} {1} [{2}] {3}" -f $(if ($c.pass) { 'OK ' } else { 'RED' }), $c.name, $c.kind, $c.detail) }
  if ($g.ok -ne $true) { throw "绿矩阵须 ok=true: $($g.cases | Where-Object { -not $_.pass } | ConvertTo-Json -Compress -Depth 6)" }
  if ($g.passed -ne 6 -or $g.failed -ne 0) { throw "绿矩阵须 6/0: passed=$($g.passed) failed=$($g.failed)" }
  Log "绿全矩阵 PASS(6/6 durationMs=$($g.durationMs))"

  # ── 2. swarm test-matrix 2 分片真并发(后台 POST + 进程采样)──
  Log "== swarm test-matrix 绿 2 分片 =="
  $caseNames = @("实体计数=36", "玩家抵达终点", "钥匙拾取下沉", "门开启下沉", "终点庆祝升起", "玩家挂winner标签")
  $job = Start-Job -ScriptBlock {
    param($names)
    $body = @{ shardType = "test-matrix"; items = $names; shardCount = 2; operation = @{ kind = "test_run"; matrixRef = "tests/maze/matrix.json"; maxConcurrent = 2 } } | ConvertTo-Json -Depth 12 -Compress
    $req = [System.Net.HttpWebRequest]::Create("http://127.0.0.1:8103/api/forge/swarm/execute")
    $req.Method = 'POST'; $req.ContentType = 'application/json; charset=utf-8'; $req.Timeout = 600000
    $bytes = [Text.Encoding]::UTF8.GetBytes($body); $req.ContentLength = $bytes.Length
    $st = $req.GetRequestStream(); $st.Write($bytes, 0, $bytes.Length); $st.Close()
    $resp = $req.GetResponse()
    $sr = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
    $sr.ReadToEnd()
  } -ArgumentList (,$caseNames)
  while ($job.State -eq 'Running') {
    $eh = @(Get-Process -Name 'engine-host' -ErrorAction SilentlyContinue).Count
    $sm = @(Get-Process -Name 'engine-scene-mcp' -ErrorAction SilentlyContinue).Count
    if ($eh -gt $script:maxEngineHost) { $script:maxEngineHost = $eh }
    if ($sm -gt $script:maxSceneMcp) { $script:maxSceneMcp = $sm }
    Start-Sleep -Milliseconds 200
  }
  $sw = Receive-Job $job | ConvertFrom-Json
  Remove-Job $job -Force
  Log "  运行中进程计数峰值:engine-host=$($script:maxEngineHost) engine-scene-mcp=$($script:maxSceneMcp)"
  if ($script:maxEngineHost -lt 2) { throw "并发证据不足:engine-host 进程峰值须 ≥2,实 $($script:maxEngineHost)" }
  if ([int]$sw.aggregate.failed -ne 0) { throw "swarm 绿矩阵须零错误: $($sw.shards | ConvertTo-Json -Compress -Depth 8)" }
  $okSum = 0; foreach ($s in $sw.shards) { $okSum += [int]$s.okCount }
  if ($okSum -ne 6) { throw "swarm 绿矩阵聚合须 6 项 ok,实 $okSum" }
  $pids = @($sw.shards | ForEach-Object { $_.evidence.pid } | Where-Object { $null -ne $_ })
  if ($pids.Count -lt 2 -or $pids[0] -eq $pids[1]) { throw "分片 pid 须互异: $($pids -join ',')" }
  $w0 = $sw.shards[0].evidence.windowMs; $w1 = $sw.shards[1].evidence.windowMs
  $overlap = ([double]$w0[0] -lt [double]$w1[1]) -and ([double]$w1[0] -lt [double]$w0[1])
  Log "  分片 pid=$($pids -join '/') 时间窗 [$($w0[0])..$($w0[1])] × [$($w1[0])..$($w1[1])] 重叠=$overlap"
  if (-not $overlap) { throw "分片时间窗须重叠(真并发证据)" }
  Log "swarm 绿 2 分片 PASS(聚合 6/0,pid 互异,时间窗重叠,engine-host 峰值=$($script:maxEngineHost))"

  # ── 3. swarm 红子矩阵:如实标红不充绿 ──
  Log "== swarm test-matrix 红 1 分片 =="
  $redNames = @("必红-玩家错位", "必红-门已开(未拿钥匙)")
  $sred = Post-Json "http://127.0.0.1:8103/api/forge/swarm/execute" @{ shardType = "test-matrix"; items = $redNames; shardCount = 1; operation = @{ kind = "test_run"; matrixRef = "tests/maze/matrix_red.json" } } 300 | ConvertFrom-Json
  if ([int]$sred.aggregate.failed -ne 2) { throw "红子矩阵须 errors=2 如实: $($sred | ConvertTo-Json -Compress -Depth 8)" }
  if ($sred.shards[0].status -ne 'failed') { throw "红分片须 status=failed" }
  foreach ($e in $sred.shards[0].errors) { Log ("  RED {0}: {1}" -f $e.item, $e.error) }
  Log "swarm 红子矩阵 PASS(errors=2 如实标红)"

  # ── 4. playtest-regression SKILL 声明工具在 MCP 面全部存在 ──
  Log "== SKILL 工具面对齐 =="
  $tools = (Invoke-WebRequest -Uri "http://127.0.0.1:8103/api/forge/mcp/tools" -UseBasicParsing -TimeoutSec 10).Content | ConvertFrom-Json
  $need = @('scene_load', 'viewport_set_camera', 'play_enter', 'play_pause', 'logic_inject_input', 'play_step', 'play_exit', 'entity_list', 'component_get', 'transform_get', 'scene_summary', 'viewport_frame')
  foreach ($n in $need) {
    if (-not ($tools.tools -contains "mcp__engine-scene__$n")) { throw "SKILL 声明工具缺失: mcp__engine-scene__$n" }
  }
  Log "SKILL 声明 12 引擎工具全部存在(mcp/tools 共 $($tools.tools.Count) 个)"

  Log "F6 wave.2 回归矩阵门冒烟 PASS(G-F6-2)"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-scene-mcp', 'code-forge-mcp', 'asset-pipeline-mcp', 'gen-image-mcp', 'gen-model-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
}
