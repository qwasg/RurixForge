# F7 wave.1 事件基座冒烟(G-F7-1):agentd EventBus/会话持久化/SSE/design-snapshot + host 代理透传。
# 腿:①直连 agentd 会话 CRUD/fork/revert(事件落盘断言) ②SSE 直连 replay/live/fromSeq 续传
#   ③gap 腿(FORGE_AGENTD_EVENT_BUFFER=4 小窗口,fromSeq=0 首帧须合成 stream.gap)
#   ④host 代理腿:经 3080 CRUD + SSE ≥20s 不断(旧 15s 超时已豁免,收 keep-alive 注释帧)
#   ⑤design-snapshot 经 3080 字段穷举。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f7-w1-events-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$logFile = "evidence\f7-w1-events-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
$script:pass = 0; $script:fail = 0; $script:failures = @()
function Check($cond, $name) {
  if ($cond) { $script:pass++; Log "  PASS $name" }
  else { $script:fail++; $script:failures += $name; Log "  FAIL $name" }
}

function Invoke-Json($method, $url, $bodyObj = $null) {
  $req = [System.Net.HttpWebRequest]::Create($url)
  $req.Method = $method
  $req.Timeout = 30000
  if ($null -ne $bodyObj) {
    $bytes = [Text.Encoding]::UTF8.GetBytes(($bodyObj | ConvertTo-Json -Depth 12 -Compress))
    $req.ContentType = 'application/json; charset=utf-8'
    $req.ContentLength = $bytes.Length
    $st = $req.GetRequestStream(); $st.Write($bytes, 0, $bytes.Length); $st.Close()
  } elseif ($method -eq 'POST' -or $method -eq 'PATCH') { $req.ContentLength = 0 }
  try { $resp = $req.GetResponse() }
  catch [System.Net.WebException] {
    $resp = $_.Exception.Response
    if ($null -eq $resp) { throw }
  }
  $sr = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
  $text = $sr.ReadToEnd(); $sr.Close(); $resp.Close()
  $obj = $null; try { $obj = $text | ConvertFrom-Json } catch {}
  return @{ status = [int]$resp.StatusCode; json = $obj; text = $text }
}

# 读 SSE 流 $seconds 秒($kick 在 ~400ms 处触发一次,用于 live 腿注入动作)。
# 返回 @{ Text; Ended }:Ended=true 表示连接在窗口内被对端关闭/重置。
# 注意:StreamReader 同时只允许一个挂起读——同一 $task 轮询等待,完成才发下一读;
# 窗口结束时仍有挂起读 = 连接存活(对端无数据但未断)。
function Read-Sse($url, $seconds, $kick = $null) {
  $req = [System.Net.HttpWebRequest]::Create($url)
  $req.Method = 'GET'
  $req.Accept = 'text/event-stream'
  $req.Timeout = ($seconds + 15) * 1000
  $req.ReadWriteTimeout = ($seconds + 15) * 1000
  $resp = $req.GetResponse()
  $reader = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
  $sb = New-Object Text.StringBuilder
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $ended = $false; $kicked = $false
  $task = $reader.ReadLineAsync()
  while ($sw.Elapsed.TotalSeconds -lt $seconds) {
    if (-not $kicked -and $null -ne $kick -and $sw.ElapsedMilliseconds -gt 400) { & $kick; $kicked = $true }
    $w = $false
    try { $w = $task.Wait(300) } catch { $ended = $true; break } # 连接重置 = 已断
    if ($w) {
      try { $line = $task.Result } catch { $ended = $true; break }
      if ($null -eq $line) { $ended = $true; break } # 对端关闭
      [void]$sb.Append($line + "`n")
      $task = $reader.ReadLineAsync()
    }
  }
  # 窗口结束:挂起读已完成且为 null → 对端已关;挂起读未完成 → 连接存活。
  if (-not $ended -and $null -ne $task -and $task.IsCompleted) {
    try { if ($null -eq $task.Result) { $ended = $true } else { [void]$sb.Append($task.Result + "`n") } } catch { $ended = $true }
  }
  try { $reader.Close(); $resp.Close() } catch {}
  return @{ Text = $sb.ToString(); Ended = $ended }
}

function Test-PortFree($port) {
  try {
    $l = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, $port)
    $l.Start(); $l.Stop(); return $true
  } catch { return $false }
}

New-Item -ItemType Directory -Force evidence | Out-Null
$script:procs = @()
$script:startTime = Get-Date
try {
  if (-not (Test-PortFree 8103)) { throw "8103 被占用(疑有运行中的 agentd),请先关闭再跑冒烟" }
  if (-not (Test-PortFree 3080)) { throw "3080 被占用(疑有运行中的 host),请先关闭再跑冒烟" }

  Log "== 构建 forge-agentd =="
  $ErrorActionPreference = 'Continue'
  cargo build -p forge-agentd 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  cargo: $_" }
  $cargoExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($cargoExit -ne 0) { throw "cargo build -p forge-agentd 失败(exit=$cargoExit)" }

  Log "== 启动 agentd(FORGE_AGENTD_EVENT_BUFFER=4,gap 腿用小窗口)=="
  $env:FORGE_AGENTD_EVENT_BUFFER = '4'
  $script:procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  Remove-Item env:FORGE_AGENTD_EVENT_BUFFER -ErrorAction SilentlyContinue
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪(8103)"

  $AG = "http://127.0.0.1:8103"

  # ── 腿 1:直连 CRUD + fork + revert(事件落盘)──
  Log "== 腿1:直连会话 CRUD/fork/revert =="
  $r = Invoke-Json POST "$AG/api/forge/sessions" @{ title = '冒烟A' }
  Check ($r.status -eq 200 -and $r.json.session.id -like 'sess_*') "创建会话A 200 + sess_* id"
  $sidA = $r.json.session.id
  Check ($r.json.session.status -eq 'idle' -and $r.json.session.agentKind -eq 'coding' -and $r.json.session.webSearchEnabled -eq $true) "创建缺省字段面(idle/coding/web=true)"
  $snap = Invoke-Json GET "$AG/api/forge/design-snapshot?sessionId=$sidA"
  Check ($snap.json.events.Count -eq 1 -and $snap.json.events[0].type -eq 'session.created' -and $snap.json.events[0].seq -eq 1) "session.created 持久事件落盘(seq=1)"
  $r = Invoke-Json PATCH "$AG/api/forge/sessions/$sidA" @{ title = '冒烟A-改' }
  Check ($r.json.session.titleManuallySet -eq $true) "PATCH title → titleManuallySet=true"
  $r = Invoke-Json PATCH "$AG/api/forge/sessions/$sidA" @{ pinned = $true }
  Check ($r.json.session.pinned -eq $true) "PATCH pinned=true"
  $lst = Invoke-Json GET "$AG/api/forge/sessions"
  Check (@($lst.json.sessions | Where-Object { $_.id -eq $sidA }).Count -eq 1) "GET 列表含会话A({sessions} 形态)"
  # fork:分支标题 + 事件克隆 + session.forked
  $r = Invoke-Json POST "$AG/api/forge/sessions/$sidA/fork"
  Check ($r.status -eq 200 -and $r.json.session.title -eq '分支 · 冒烟A-改') "fork 标题「分支 · 冒烟A-改」"
  $sidF = $r.json.session.id
  $fsnap = Invoke-Json GET "$AG/api/forge/design-snapshot?sessionId=$sidF"
  $ftypes = @($fsnap.json.events | ForEach-Object { $_.type }) -join ','
  $fseqs = @($fsnap.json.events | ForEach-Object { $_.seq }) -join ','
  Check ($ftypes -eq 'session.created,session.updated,session.updated,session.forked') "fork 事件克隆+forked(实际 $ftypes)"
  Check ($fseqs -eq '1,2,3,4') "fork seq 单调保持(实际 $fseqs)"
  # revert mode=before:截到 seq2 之前 → 余 created + reverted
  $snapA = Invoke-Json GET "$AG/api/forge/design-snapshot?sessionId=$sidA"
  $msgId = $snapA.json.events[1].id
  $r = Invoke-Json POST "$AG/api/forge/sessions/$sidA/revert" @{ messageId = $msgId; mode = 'before' }
  Check ($r.status -eq 200) "revert mode=before 200"
  $snapA2 = Invoke-Json GET "$AG/api/forge/design-snapshot?sessionId=$sidA"
  $atypes = @($snapA2.json.events | ForEach-Object { $_.type }) -join ','
  Check ($atypes -eq 'session.created,session.reverted') "revert before 截断语义(实际 $atypes)"
  Check ($snapA2.json.latestSeq -eq 2) "revert 后 latestSeq=2(实际 $($snapA2.json.latestSeq))"

  # ── 腿 2:SSE 直连 replay / live / fromSeq 续传 ──
  Log "== 腿2:SSE 直连 replay/live/续传 =="
  $s1 = Read-Sse "$AG/api/forge/sessions/$sidA/events/stream?fromSeq=0" 1.5
  $frames1 = @($s1.Text -split "`n" | Where-Object { $_ -ne '' })
  $ids = @($frames1 | Where-Object { $_ -match '^id: (\d+)$' } | ForEach-Object { [int]($_ -replace '^id: ', '') })
  Check (-not $s1.Ended) "SSE 连接窗口内不被对端关闭"
  Check (($ids -join ',') -eq '1,2') "replay seq 递增(实际 $($ids -join ','))"
  Check ($s1.Text -match 'event: session.created' -and $s1.Text -match 'event: session.reverted') "replay 事件类型齐"
  Check ($s1.Text -match 'data: \{') "data 帧为 wire JSON"
  $s2 = Read-Sse "$AG/api/forge/sessions/$sidA/events/stream?fromSeq=2" 1.2
  Check (($s2.Text -notmatch 'event: ')) "fromSeq=latest 续传零回放帧"
  $s3 = Read-Sse "$AG/api/forge/sessions/$sidA/events/stream?fromSeq=2" 1.8 { Invoke-Json PATCH "$AG/api/forge/sessions/$sidA" @{ pinned = $false } | Out-Null }
  Check ($s3.Text -match 'event: session.updated' -and $s3.Text -match 'id: 3') "live 推送 session.updated(seq 3 续接)"

  # ── 腿 3:gap(buffer=4,7 事件,fromSeq=0 首帧 stream.gap)──
  Log "== 腿3:gap 超窗 =="
  $r = Invoke-Json POST "$AG/api/forge/sessions" @{ title = '冒烟B-gap' }
  $sidB = $r.json.session.id
  foreach ($i in 1..6) { Invoke-Json PATCH "$AG/api/forge/sessions/$sidB" @{ pinned = ($i % 2 -eq 1) } | Out-Null }
  $sg = Read-Sse "$AG/api/forge/sessions/$sidB/events/stream?fromSeq=0" 1.8
  $firstFrame = ($sg.Text -split "`n`n")[0]
  Check ($firstFrame -match 'event: stream.gap') "超窗首帧 stream.gap(实际首帧: $($firstFrame -replace "`n",' / '))"
  Check ($firstFrame -match 'replay-window-exceeded' -and $firstFrame -match '"gap":true') "gap payload{gap:true,reason}"
  Check ($sg.Text -match 'id: 4' -and $sg.Text -match 'id: 7' -and $sg.Text -notmatch 'id: 3') "回放段=窗口 seq4..7(无 seq3)"

  # ── 腿 4:host 代理(CRUD 经 3080 + SSE ≥20s 不断)──
  Log "== 腿4:host 代理(构建+启动 host) =="
  $ErrorActionPreference = 'Continue'
  pnpm --filter @forge/host build 2>&1 | Select-Object -Last 2 | ForEach-Object { Log "  pnpm: $_" }
  $pnpmExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($pnpmExit -ne 0) { throw "pnpm --filter @forge/host build 失败(exit=$pnpmExit)" }
  $script:procs += Start-Process -FilePath "node" -ArgumentList "packages\host\dist\index.js" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:3080/api/forge/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "host 就绪超时" }
  Log "host 就绪(3080)"

  $H = "http://127.0.0.1:3080"
  $r = Invoke-Json GET "$H/api/forge/sessions"
  Check ($r.status -eq 200 -and @($r.json.sessions | Where-Object { $_.id -eq $sidA }).Count -eq 1) "经 3080 GET sessions 见会话A(代理生效,{sessions} 形态)"
  $r = Invoke-Json POST "$H/api/forge/sessions" @{ title = '冒烟C-经3080' }
  Check ($r.status -eq 200 -and $r.json.session.id -like 'sess_*') "经 3080 POST 建会话C"
  $sidC = $r.json.session.id
  $r = Invoke-Json GET "$H/api/forge/sessions/$sidC"
  Check ($r.status -eq 200 -and $r.json.session.title -eq '冒烟C-经3080') "经 3080 GET 会话C"
  $r = Invoke-Json PATCH "$H/api/forge/sessions/$sidC" @{ title = 'C改' }
  Check ($r.json.session.titleManuallySet -eq $true) "经 3080 PATCH 会话C"
  $r = Invoke-Json DELETE "$H/api/forge/sessions/$sidC"
  Check ($r.status -eq 200 -and $r.json.ok -eq $true) "经 3080 DELETE 会话C"
  $r = Invoke-Json GET "$H/api/forge/sessions/$sidC"
  Check ($r.status -eq 404 -and $r.json.error.code -eq 'SESSION_NOT_FOUND') "删后 GET 404 SESSION_NOT_FOUND 透传"

  Log "  SSE 经 3080 保持 22s(旧超时 15s,须收 keep-alive 且不断连)..."
  $ks = Read-Sse "$H/api/forge/sessions/$sidA/events/stream?fromSeq=0" 22
  $kaLines = @($ks.Text -split "`n" | Where-Object { $_ -match '^:' })
  Check ($ks.Text -match 'event: session.created') "经 3080 SSE replay 帧到达"
  Check ($kaLines.Count -ge 1) "22s 内收到 keep-alive 注释帧(实际 $($kaLines.Count) 行: $($kaLines -join ';'))"
  Check (-not $ks.Ended) "SSE 经 3080 保持 ≥22s 连接不断(15s 旧超时已豁免)"

  # ── 腿 5:design-snapshot 经 3080 字段穷举 ──
  Log "== 腿5:design-snapshot 经 3080 字段穷举 =="
  $ds = Invoke-Json GET "$H/api/forge/design-snapshot?sessionId=$sidA"
  $keys = @($ds.json.PSObject.Properties.Name | Sort-Object) -join ','
  Check ($keys -eq 'activeSession,chatFolders,events,latestSeq,models,run,sessions,todos') "字段穷举(实际 $keys)"
  Check ($ds.json.activeSession.id -eq $sidA) "activeSession=会话A"
  Check (@($ds.json.events).Count -eq 3 -and $ds.json.latestSeq -eq 3) "events 全量回放 3 条 + latestSeq=3"
  Check (@($ds.json.todos).Count -eq 0 -and $null -eq $ds.json.run) "todos=[] run=null 占位"
  $m = $ds.json.models
  # F8 wave.2:models 面增至三档(deepseek-chat/mock/openai-compat;第三档为通用 openai-compat 渠道,id/provider 固定)。
  Check (@($m.models).Count -eq 3 -and $m.models[0].id -eq 'deepseek-chat' -and $m.models[1].id -eq 'mock' -and $m.models[2].id -eq 'openai-compat' -and $m.models[2].provider -eq 'openai-compat' -and $m.defaultModelId -eq 'deepseek-chat') "models 三档(deepseek-chat/mock/openai-compat)"
  Check (@('available','needs-key') -contains $m.models[0].availability -and $m.models[1].availability -eq 'available' -and @('available','needs-key') -contains $m.models[2].availability) "availability 两态合法(deepseek=$($m.models[0].availability) openai-compat=$($m.models[2].availability))"
  Check ($ds.text -notmatch 'sk-') "响应面无密钥串(R-5)"
  $ds0 = Invoke-Json GET "$H/api/forge/design-snapshot"
  Check ($null -eq $ds0.json.activeSession -and @($ds0.json.events).Count -eq 0 -and $ds0.json.latestSeq -eq 0) "空 sessionId → null/[]/0 三态"

  Log "================================"
  Log ("SUMMARY: pass={0} fail={1}" -f $script:pass, $script:fail)
  if ($script:fail -gt 0) {
    Log ("FAILED ITEMS: " + ($script:failures -join ' | '))
    Log "F7 wave.1 事件基座冒烟 FAIL"
    exit 1
  }
  Log "F7 wave.1 事件基座冒烟 PASS(G-F7-1)"
  exit 0
} catch {
  Log "FATAL: $_"
  Log ("SUMMARY: pass={0} fail={1} fatal=1" -f $script:pass, $script:fail)
  Log "F7 wave.1 事件基座冒烟 FAIL"
  exit 1
} finally {
  foreach ($p in $script:procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
  # 连带清杀:agentd 按启动时间过滤;node 按命令行精确匹配 host 入口(防误杀)。
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -eq 'forge-agentd' -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
  Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -match 'packages[\\/]host[\\/]dist[\\/]index\.js' } |
    ForEach-Object { try { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } catch {} }
}
