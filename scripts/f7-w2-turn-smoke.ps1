# F7 wave.2 turn 执行事件化冒烟(G-F7-2):ask:execute 五模式 / runs 控制 / todos REST / 事件序列。
# 流程全经 host http://127.0.0.1:3080(forgeProxy 透传;ask:execute 已入长生命周期豁免)。
# 腿:①build(mock)SSE 序列+run 终态+自动命名 ②ask 零 tool.invoked ③multitask 异步委派基线(D-036 模板退役)
#   ④todos REST+todo.* 事件+snapshot 填真 ⑤cancel 非 running ok:false ⑥revert before 首个 composer.user.message 截断
#   ⑦deepseek live(有 key 实测/无 key 如实 SKIP) ⑧PASS/FAIL 汇总。
# mock 确定性:agentd#1 以 FORGE_GEN_DATA_DIR=临时目录 + 清 FORGE_LLM_API_KEY 启动(keystore/env 均不命中 → mock);
# leg⑦ 重启 agentd#2(真实 keystore/env 面)→ design-snapshot availability 探针判定 key,有无如实。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f7-w2-turn-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$logFile = "evidence\f7-w2-turn-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
$script:pass = 0; $script:fail = 0; $script:failures = @(); $script:skips = @()
function Check($cond, $name) {
  if ($cond) { $script:pass++; Log "  PASS $name" }
  else { $script:fail++; $script:failures += $name; Log "  FAIL $name" }
}
function Skip($name, $reason) { $script:skips += "$name($reason)"; Log "  SKIP=not-triggered $name($reason)" }

function Invoke-Json($method, $url, $bodyObj = $null, $timeoutSec = 30) {
  $req = [System.Net.HttpWebRequest]::Create($url)
  $req.Method = $method
  $req.Timeout = $timeoutSec * 1000
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

# 读 SSE 流 $seconds 秒($kick 在 ~400ms 处触发一次)。复用 f7-w1 技法:
# 同一 StreamReader 同时只允许一个挂起读,逐轮 Wait(300) 轮询;连接被对端关闭 → Ended=true。
function Read-Sse($url, $seconds, $kick = $null) {
  $req = [System.Net.HttpWebRequest]::Create($url)
  $req.Method = 'GET'
  $req.Accept = 'text/event-stream'
  $req.Timeout = ($seconds + 30) * 1000
  $req.ReadWriteTimeout = ($seconds + 30) * 1000
  $resp = $req.GetResponse()
  $reader = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
  $sb = New-Object Text.StringBuilder
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $ended = $false; $kicked = $false
  $task = $reader.ReadLineAsync()
  while ($sw.Elapsed.TotalSeconds -lt $seconds) {
    if (-not $kicked -and $null -ne $kick -and $sw.ElapsedMilliseconds -gt 400) { & $kick; $kicked = $true }
    $w = $false
    try { $w = $task.Wait(300) } catch { $ended = $true; break }
    if ($w) {
      try { $line = $task.Result } catch { $ended = $true; break }
      if ($null -eq $line) { $ended = $true; break }
      [void]$sb.Append($line + "`n")
      $task = $reader.ReadLineAsync()
    }
  }
  if (-not $ended -and $null -ne $task -and $task.IsCompleted) {
    try { if ($null -eq $task.Result) { $ended = $true } else { [void]$sb.Append($task.Result + "`n") } } catch { $ended = $true }
  }
  try { $reader.Close(); $resp.Close() } catch {}
  return @{ Text = $sb.ToString(); Ended = $ended }
}

# SSE 文本中事件序列有序性断言辅助:返回 $true 当且仅当各 'event: X' 均出现且下标递增。
function Test-SeqOrder($text, [string[]]$types) {
  $idx = -1
  foreach ($t in $types) {
    $i = $text.IndexOf("event: $t")
    if ($i -lt 0 -or $i -le $idx) { return $false }
    $idx = $i
  }
  return $true
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
# 原 FORGE_LLM_API_KEY 留存(leg⑦ 判定;R-5:值永不进日志)。
$savedLlmKey = $env:FORGE_LLM_API_KEY
try {
  if (-not (Test-PortFree 8103)) { throw "8103 被占用(疑有运行中的 agentd),请先关闭再跑冒烟" }
  if (-not (Test-PortFree 3080)) { throw "3080 被占用(疑有运行中的 host),请先关闭再跑冒烟" }

  Log "== 构建 forge-agentd / engine-scene-mcp / engine-host =="
  $ErrorActionPreference = 'Continue'
  cargo build -p forge-agentd -p engine-scene-mcp -p engine-host 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  cargo: $_" }
  $cargoExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($cargoExit -ne 0) { throw "cargo build 失败(exit=$cargoExit)" }
  $engineBinsReady = (Test-Path "target\debug\engine-scene-mcp.exe") -and (Test-Path "target\debug\engine-host.exe")
  Log "engine 二进制: $(if ($engineBinsReady) {'就绪'} else {'缺失(leg③⑦ 将如实 SKIP)'})"

  Log "== 启动 agentd#1(mock 确定性:隔离 gen 数据目录 + 清 FORGE_LLM_API_KEY) =="
  Remove-Item env:FORGE_LLM_API_KEY -ErrorAction SilentlyContinue
  $env:FORGE_GEN_DATA_DIR = Join-Path $env:TEMP ("f7w2-gen-" + [guid]::NewGuid().ToString('N'))
  $script:procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  Remove-Item env:FORGE_GEN_DATA_DIR -ErrorAction SilentlyContinue
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd#1 就绪(8103,mock provider 面)"

  Log "== 构建+启动 host(3080) =="
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

  # ── 腿 1:build(mock)全事件序列 + run 终态 + 自动命名(48 字截断)──
  Log "== 腿1:ask:execute build(mock)经 3080 + SSE 序列 =="
  $r = Invoke-Json POST "$H/api/forge/sessions" @{ title = 'w2-腿1' }
  Check ($r.status -eq 200 -and $r.json.session.id -like 'sess_*') "建会话S1 200"
  $sid1 = $r.json.session.id
  $input1 = '请描述当前场景的整体结构布局与光照风格基调,并给出三条具体的可直接落地执行的优化建议与其对应理由说明'
  Check ($input1.Length -gt 48) "输入超 48 字(实际 $($input1.Length),验证截断命名)"
  $askResp = $null
  $sse1 = Read-Sse "$H/api/forge/sessions/$sid1/events/stream?fromSeq=0" 4 {
    $script:__r1 = Invoke-Json POST "$H/api/forge/sessions/$sid1/ask:execute" @{ userInput = $input1; mode = 'build' }
  }
  $r1 = $script:__r1
  Check ($r1.status -eq 200) "ask:execute build HTTP 200(实际 $($r1.status))"
  Check ($r1.json.run.status -eq 'completed') "run.status=completed(实际 $($r1.json.run.status))"
  $runId1 = $r1.json.run.id
  Check ($runId1 -like 'run_*') "run id run_* 形态"
  Check ($r1.json.message.text -match 'mock:已收到') "mock provider 终稿文本"
  Check (Test-SeqOrder $sse1.Text @('composer.user.message','agent.started','agent.message','agent.completed')) "SSE 事件序 composer.user.message→agent.started→agent.message→agent.completed"
  Check ($sse1.Text -notmatch 'agent\.tool\.invoked') "mock build 零 agent.tool.invoked"
  Check ($sse1.Text -match '"composerMode":"build"' -and $sse1.Text -match "`"runId`":`"$runId1`"") "composer.user.message payload{composerMode,runId}"
  Check ($sse1.Text -match '"model":"mock"') "agent.started model=mock"
  $s1 = Invoke-Json GET "$H/api/forge/sessions/$sid1"
  $expectTitle = $input1.Substring(0, 48)
  Check ($s1.json.session.title -eq $expectTitle) "首消息自动命名=输入前 48 字"
  Check ($s1.json.session.titleManuallySet -eq $false) "titleManuallySet 保持 false"
  Check ($null -eq $s1.json.session.activeRunId) "activeRunId 终态清理"
  $gr = Invoke-Json GET "$H/api/forge/runs/$runId1"
  Check ($gr.status -eq 200 -and $gr.json.run.status -eq 'completed' -and $gr.json.run.trigger -eq 'composer_chat') "GET /runs/{id} 终态+trigger"

  # ── 腿 2:ask 模式纯对话零工具 ──
  Log "== 腿2:ask 模式 =="
  $r = Invoke-Json POST "$H/api/forge/sessions" @{ title = 'w2-腿2' }
  $sid2 = $r.json.session.id
  $script:__r2 = $null
  $sse2 = Read-Sse "$H/api/forge/sessions/$sid2/events/stream?fromSeq=0" 4 {
    $script:__r2 = Invoke-Json POST "$H/api/forge/sessions/$sid2/ask:execute" @{ userInput = '引擎里 Transform 组件有哪些字段?'; mode = 'ask' }
  }
  $r2 = $script:__r2
  Check ($r2.status -eq 200 -and $r2.json.run.status -eq 'completed') "ask run completed"
  Check ($sse2.Text -notmatch 'agent\.tool\.invoked') "ask 全程零 agent.tool.invoked"
  Check (Test-SeqOrder $sse2.Text @('composer.user.message','agent.started','agent.message','agent.completed')) "ask SSE 序列齐"

  # ── 腿 3:multitask 异步委派(D-036;原「碰撞体模板 + swarm 分片」链已退役)──
  # 本腿在 mock provider 面能验的是「模式基线」:mock 步进不产 tool_calls,HTTP 面驱动不出
  # dispatch,故派发链本身(受理即返回 / 后台卡片 / 回执落盘 / 回执唤醒与中途收件,D-038)由
  # cargo test agent::tests::{multitask_*,receipt_*} 覆盖,这里如实 SKIP 不充绿。
  Log "== 腿3:multitask 异步委派基线(模板退役)=="
  $r = Invoke-Json POST "$H/api/forge/sessions" @{ title = 'w2-腿3' }
  $sid3 = $r.json.session.id
  $script:__r3 = $null
  $sse3 = Read-Sse "$H/api/forge/sessions/$sid3/events/stream?fromSeq=0" 6 {
    $script:__r3 = Invoke-Json POST "$H/api/forge/sessions/$sid3/ask:execute" @{ userInput = '给所有关卡块加碰撞体'; mode = 'multitask' } 30
  }
  $r3 = $script:__r3
  Check ($r3.status -eq 200 -and $r3.json.run.status -eq 'completed') "multitask run completed(实际 $($r3.json.run.status);err=$($r3.json.error))"
  Check (Test-SeqOrder $sse3.Text @('composer.user.message','agent.started','agent.message','agent.completed')) "multitask SSE 序列齐(普通工具循环轮)"
  Check ($sse3.Text -notmatch 'swarm\.execute') "swarm.execute 模板已退役(零合成工具事件)"
  Check ($null -eq (Invoke-Json GET "$H/api/forge/sessions/$sid3").json.session.activeRunId) "activeRunId 终态清理(后台派发不占用输入)"
  # 旧「模板未命中 → run failed」口径随模板退役:同一句闲聊现在照常收束。
  $r = Invoke-Json POST "$H/api/forge/sessions" @{ title = 'w2-腿3-闲聊' }
  $sid3b = $r.json.session.id
  $script:__r3b = $null
  $sse3b = Read-Sse "$H/api/forge/sessions/$sid3b/events/stream?fromSeq=0" 4 {
    $script:__r3b = Invoke-Json POST "$H/api/forge/sessions/$sid3b/ask:execute" @{ userInput = '随便聊聊天气'; mode = 'multitask' }
  }
  $r3b = $script:__r3b
  Check ($r3b.status -eq 200 -and $r3b.json.run.status -eq 'completed') "闲聊不再 failed(模板未命中口径已退役;实际 $($r3b.json.run.status))"
  Check ($sse3b.Text -notmatch '模板未命中') "零「模板未命中」文案"
  Skip 'multitask-dispatch-chain' 'mock 步进不产 tool_calls,派发/唤醒链由 cargo test multitask_*/receipt_* 覆盖'

  # ── 腿 4:todos REST + todo.* 事件 + snapshot 填真 ──
  Log "== 腿4:todos =="
  $r = Invoke-Json POST "$H/api/forge/sessions" @{ title = 'w2-腿4' }
  $sid4 = $r.json.session.id
  $script:__todoId = $null
  $sse4 = Read-Sse "$H/api/forge/sessions/$sid4/events/stream?fromSeq=0" 4 {
    $ct = Invoke-Json POST "$H/api/forge/todos" @{ sessionId = $sid4; title = '修碰撞体'; kind = 'edit'; description = '批量 RigidBody' }
    $script:__todoId = $ct.json.todo.id
    Start-Sleep -Milliseconds 200
    $null = Invoke-Json PATCH "$H/api/forge/todos/$($script:__todoId)" @{ status = 'completed'; summary = '已落' }
  }
  $todoId = $script:__todoId
  Check ($null -ne $todoId -and $todoId -like 'todo_*') "POST /todos 建 todo_*"
  Check ($sse4.Text -match 'event: todo\.created' -and $sse4.Text -match '"title":"修碰撞体"' -and $sse4.Text -match '"kind":"edit"' -and $sse4.Text -match '"status":"queued"') "todo.created 事件载荷"
  Check ($sse4.Text -match 'event: todo\.updated' -and $sse4.Text -match '"status":"completed"') "todo.updated 事件载荷"
  $tl = Invoke-Json GET "$H/api/forge/sessions/$sid4/todos"
  Check ($tl.status -eq 200 -and @($tl.json.todos).Count -eq 1 -and $tl.json.todos[0].status -eq 'completed' -and $tl.json.todos[0].summary -eq '已落') "GET todos 列表回填"
  $snap4 = Invoke-Json GET "$H/api/forge/design-snapshot?sessionId=$sid4"
  Check (@($snap4.json.todos).Count -eq 1) "snapshot todos 填真(实际 $(@($snap4.json.todos).Count))"
  # 校验面:空 title 400 / 非法 status 400 / 不存在 404。
  $bad = Invoke-Json POST "$H/api/forge/todos" @{ sessionId = $sid4; title = ' ' }
  Check ($bad.status -eq 400 -and $bad.json.error.code -eq 'TODO_INVALID') "POST 空 title 400 TODO_INVALID"
  $bad2 = Invoke-Json PATCH "$H/api/forge/todos/$todoId" @{ status = 'bogus' }
  Check ($bad2.status -eq 400 -and $bad2.json.error.code -eq 'TODO_INVALID') "PATCH 非法 status 400"
  $bad3 = Invoke-Json PATCH "$H/api/forge/todos/todo_none" @{ status = 'running' }
  Check ($bad3.status -eq 404 -and $bad3.json.error.code -eq 'TODO_NOT_FOUND') "PATCH 不存在 404 TODO_NOT_FOUND"

  # ── 腿 5:cancel 非 running 如实 ok:false ──
  Log "== 腿5:runs cancel =="
  $c1 = Invoke-Json POST "$H/api/forge/runs/$runId1/cancel"
  Check ($c1.status -eq 200 -and $c1.json.ok -eq $false -and $c1.json.status -eq 'completed') "cancel 非 running → ok:false(completed)"
  $c2 = Invoke-Json POST "$H/api/forge/runs/run_none/cancel"
  Check ($c2.status -eq 404 -and $c2.json.error.code -eq 'RUN_NOT_FOUND') "cancel 不存在 404 RUN_NOT_FOUND"

  # ── 腿 6:revert before 首个 composer.user.message → snapshot 事件截断实测 ──
  Log "== 腿6:revert 截断 =="
  $r = Invoke-Json POST "$H/api/forge/sessions" @{ title = 'w2-腿6' }
  $sid6 = $r.json.session.id
  $null = Invoke-Json POST "$H/api/forge/sessions/$sid6/ask:execute" @{ userInput = '第一轮对话内容'; mode = 'build' }
  $null = Invoke-Json POST "$H/api/forge/sessions/$sid6/ask:execute" @{ userInput = '第二轮对话内容'; mode = 'build' }
  $snap6a = Invoke-Json GET "$H/api/forge/design-snapshot?sessionId=$sid6"
  $types6a = @($snap6a.json.events | ForEach-Object { $_.type })
  Check (($types6a | Where-Object { $_ -eq 'composer.user.message' }).Count -eq 2) "两轮后 2 条 composer.user.message(实际 $($types6a.Count) 总事件)"
  $firstUserMsg = @($snap6a.json.events | Where-Object { $_.type -eq 'composer.user.message' })[0]
  $rv = Invoke-Json POST "$H/api/forge/sessions/$sid6/revert" @{ messageId = $firstUserMsg.id; mode = 'before' }
  Check ($rv.status -eq 200) "revert before 首个 composer.user.message 200"
  $snap6b = Invoke-Json GET "$H/api/forge/design-snapshot?sessionId=$sid6"
  $types6b = @($snap6b.json.events | ForEach-Object { $_.type }) -join ','
  Check ($types6b -eq 'session.created,session.reverted') "截断后事件=session.created,session.reverted(实际 $types6b)"
  Check ($snap6b.json.latestSeq -eq 2) "latestSeq=2(实际 $($snap6b.json.latestSeq))"

  # ── 腿 7:deepseek live(有 key 实测/无 key 如实 SKIP)──
  Log "== 腿7:deepseek live =="
  # 重启 agentd#2:真实 keystore/env 面(恢复 FORGE_LLM_API_KEY;不隔离 gen 数据目录)。
  foreach ($p in $script:procs) { try { if ($p.Name -eq 'forge-agentd' -and -not $p.HasExited) { $p.Kill() } } catch {} }
  Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'forge-agentd' -and $_.StartTime -ge $script:startTime } | ForEach-Object { try { $_.Kill() } catch {} }
  Start-Sleep -Milliseconds 800
  if ($null -ne $savedLlmKey -and $savedLlmKey -ne '') { $env:FORGE_LLM_API_KEY = $savedLlmKey }
  $script:procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd#2 就绪超时" }
  Remove-Item env:FORGE_LLM_API_KEY -ErrorAction SilentlyContinue
  $probe = Invoke-Json GET "$H/api/forge/design-snapshot"
  $availability = $probe.json.models.models[0].availability
  Log "  deepseek availability 探针: $availability"
  if ($availability -ne 'available') {
    Skip 'deepseek' 'no key'
  } elseif (-not $engineBinsReady) {
    Skip 'deepseek' 'engine bin missing(build 模式须拉 MCP 工具面)'
  } else {
    $r = Invoke-Json POST "$H/api/forge/sessions" @{ title = 'w2-腿7' }
    $sid7 = $r.json.session.id
    $script:__r7 = $null
    $sse7 = Read-Sse "$H/api/forge/sessions/$sid7/events/stream?fromSeq=0" 60 {
      $script:__r7 = Invoke-Json POST "$H/api/forge/sessions/$sid7/ask:execute" @{ userInput = '回复 ok 即可,不要调用任何工具'; mode = 'build' } 120
    }
    $r7 = $script:__r7
    Check ($r7.status -eq 200 -and $r7.json.run.status -eq 'completed') "deepseek live run completed(实际 $($r7.json.run.status);err=$($r7.json.error))"
    Check ($sse7.Text -match '"provider":"deepseek"') "agent.message provider=deepseek"
    Check ($sse7.Text -match 'event: agent\.completed') "agent.completed 终帧"
    if ($sse7.Text -match 'event: agent\.usage') { Log "  agent.usage 事件实测到达(deepseek 带 usage)" } else { Log "  注:本轮无 agent.usage(响应未带 usage 如实)" }
  }

  Log "================================"
  Log ("SUMMARY: pass={0} fail={1} skip={2}" -f $script:pass, $script:fail, $script:skips.Count)
  if ($script:skips.Count -gt 0) { Log ("SKIPS: " + ($script:skips -join ' | ')) }
  if ($script:fail -gt 0) {
    Log ("FAILED ITEMS: " + ($script:failures -join ' | '))
    Log "F7 wave.2 turn 冒烟 FAIL"
    exit 1
  }
  Log "F7 wave.2 turn 冒烟 PASS(G-F7-2)"
  exit 0
} catch {
  Log "FATAL: $_"
  Log ("SUMMARY: pass={0} fail={1} fatal=1" -f $script:pass, $script:fail)
  Log "F7 wave.2 turn 冒烟 FAIL"
  exit 1
} finally {
  foreach ($p in $script:procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
  # 连带清杀:按启动时间过滤(engine-scene-mcp/engine-host 孤儿会继承 stdout 句柄假死管道)。
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('forge-agentd','engine-scene-mcp','engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
  Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -match 'packages[\\/]host[\\/]dist[\\/]index\.js' } |
    ForEach-Object { try { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } catch {} }
}
