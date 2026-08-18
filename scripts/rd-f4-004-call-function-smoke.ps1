# RD-F4-004 call_function 栈级冒烟(G-RD4-2 运行时门):
#   fixture .rx add → 图 on_start → call_function → var.set result → play_enter →
#   events_drain 断言 logic.call result=5.5;同时 graph_validate 三新码拒坏图。
# 前置:cargo build --workspace;gateway-go\forge-gateway.exe;packages\client dist 已构建。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\rd-f4-004-call-function-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\rd-f4-004-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function McpCall($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 12 -Compress
  try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json; charset=utf-8" -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 30
  } catch {
    $resp = $_.Exception.Response; $errBody = ""
    if ($resp) { $sr = New-Object System.IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8); $errBody = $sr.ReadToEnd(); $sr.Close() }
    throw "$tool 失败 status=$([int]$resp.StatusCode) body=$errBody"
  }
  if ($r.StatusCode -ne 200) { throw "$tool http=$($r.StatusCode)" }
  $outer = $r.Content | ConvertFrom-Json
  if ($outer.isError -eq $true) { throw "$tool 工具级 isError: $($outer.content[0].text)" }
  return $outer.content[0].text | ConvertFrom-Json
}

function DrainEvents {
  # host_events_drain 形态归一(服务端恒为裸数组;PS 5.1 序列化偶发 {value=[...]} 包装):
  # 逐层剥包装直到元素为事件对象(带 event 键),防御性上限 4 层。
  $raw = McpCall "mcp__engine-scene__host_events_drain" @{}
  $arr = @($raw)
  $guard = 0
  while ($arr.Count -ge 1 -and $guard -lt 4) {
    $guard++
    $first = $arr[0]
    if ($null -ne $first.PSObject.Properties['event']) { break }
    if ($null -ne $first.PSObject.Properties['value']) { $arr = @($first.value); continue }
    break
  }
  return $arr
}
New-Item -ItemType Directory -Force evidence | Out-Null
foreach ($b in @("target\debug\forge-agentd.exe", "gateway-go\forge-gateway.exe", "target\debug\engine-scene-mcp.exe", "target\debug\code-forge-mcp.exe")) {
  if (-not (Test-Path $b)) { throw "缺二进制 $b —— 先 cargo build --workspace" }
}
if (-not (Test-Path "packages\client\dist\index.html")) { throw "client dist 未构建" }

$procs = @()
$script:startTime = Get-Date
try {
  # ── 0. fixture .rx + .rxgraph ──
  $demo = "$root\projects\demo"
  $scriptsDir = "$demo\Content\Scripts"
  New-Item -ItemType Directory -Force $scriptsDir | Out-Null
  $rx = "#[export(c)]`npub fn add(a: f32, b: f32) -> f32 { a + b }`n#[export(c)]`npub fn mixed(a: f32, b: i32) -> f32 { a }`n"
  [IO.File]::WriteAllText("$scriptsDir\callprobe.rx", $rx, (New-Object System.Text.UTF8Encoding($false)))

  $graphsDir = "$demo\Content\Graphs"
  New-Item -ItemType Directory -Force $graphsDir | Out-Null
  $goodGraph = @{
    version = 1; id = "g_call"; name = "CallProbe"
    nodes = @(
      @{ id = "n1"; type = "event.on_start"; pos = @(0,0) }
      @{ id = "n2"; type = "call.call_function"; pos = @(1,0);
         inputs = @{ module = @{ const = "Content/Scripts/callprobe.rx" }; fn = @{ const = "add" }; args = @{ const = @(2.0, 3.5) } } }
      @{ id = "n3"; type = "var.set"; pos = @(2,0);
         inputs = @{ name = @{ const = "result" }; value = @{ node = "n2"; pin = "result" } } }
    )
    edges = @(
      @{ from = @("n1","exec"); to = @("n2","exec") }
      @{ from = @("n2","exec"); to = @("n3","exec") }
    )
  } | ConvertTo-Json -Depth 12 -Compress
  [IO.File]::WriteAllText("$graphsDir\call_probe.rxgraph", $goodGraph, (New-Object System.Text.UTF8Encoding($false)))

  $badGraph = @{
    version = 1; id = "g_bad"; name = "BadProbe"
    nodes = @(
      @{ id = "n1"; type = "event.on_start"; pos = @(0,0) }
      @{ id = "n2"; type = "call.call_function"; pos = @(1,0);
         inputs = @{ module = @{ const = "Content/Scripts/callprobe.rx" }; fn = @{ const = "mixed" }; args = @{ const = @(1.0, 2) } } }
    )
    edges = @( @{ from = @("n1","exec"); to = @("n2","exec") } )
  } | ConvertTo-Json -Depth 12 -Compress
  [IO.File]::WriteAllText("$graphsDir\call_bad.rxgraph", $badGraph, (New-Object System.Text.UTF8Encoding($false)))

  $ghostGraph = @{
    version = 1; id = "g_ghost"; name = "GhostProbe"
    nodes = @(
      @{ id = "n1"; type = "event.on_start"; pos = @(0,0) }
      @{ id = "n2"; type = "call.call_function"; pos = @(1,0);
         inputs = @{ module = @{ const = "Content/Scripts/nope.rx" }; fn = @{ const = "add" }; args = @{ const = @(1,2) } } }
    )
    edges = @( @{ from = @("n1","exec"); to = @("n2","exec") } )
  } | ConvertTo-Json -Depth 12 -Compress
  [IO.File]::WriteAllText("$graphsDir\call_ghost.rxgraph", $ghostGraph, (New-Object System.Text.UTF8Encoding($false)))

  # ── 1. 起 agentd + gateway ──
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'rd4',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 2. 校验臂:好图通过,坏图逐类拒 ──
  Log "== graph_validate 三新码门 =="
  $gvGood = McpCall "mcp__code-forge__graph_validate" @{ path = "Content/Graphs/call_probe.rxgraph" }
  if ($gvGood.ok -ne $true) { throw "好图须通过: $($gvGood.errors | ConvertTo-Json -Compress)" }
  Log "好图 PASS"

  $gvBad = McpCall "mcp__code-forge__graph_validate" @{ path = "Content/Graphs/call_bad.rxgraph" }
  if ($gvBad.ok -eq $true) { throw "坏图须不通过(混合参数类型)" }
  $hasSig = $gvBad.errors | Where-Object { $_.code -eq 'GRAPH_CALL_SIG_MISMATCH' }
  if (-not $hasSig) { throw "坏图须返 GRAPH_CALL_SIG_MISMATCH: $($gvBad.errors | ConvertTo-Json -Compress)" }
  Log "坏图 GRAPH_CALL_SIG_MISMATCH PASS"

  $gvGhost = McpCall "mcp__code-forge__graph_validate" @{ path = "Content/Graphs/call_ghost.rxgraph" }
  if ($gvGhost.ok -eq $true) { throw "ghost 图须不通过(module 不存在)" }
  $hasMod = $gvGhost.errors | Where-Object { $_.code -eq 'GRAPH_CALL_MODULE_NOT_FOUND' }
  if (-not $hasMod) { throw "ghost 图须返 GRAPH_CALL_MODULE_NOT_FOUND: $($gvGhost.errors | ConvertTo-Json -Compress)" }
  Log "ghost 图 GRAPH_CALL_MODULE_NOT_FOUND PASS"

  # ── 3. 场景 + 挂载 + play_enter + 断言 ──
  Log "== 场景链:scene_new → entity_create(Script) → play_enter → events_drain =="
  McpCall "mcp__engine-scene__scene_new" @{ name = "rd4" } | Out-Null
  $probe = McpCall "mcp__engine-scene__entity_create" @{
    name = "probe"
    components = @(
      @{ type = "Script"; props = @{ module = ""; graphRef = "Content/Graphs/call_probe.rxgraph"; props = @{} } }
    )
    translation = @(0.0, 0.0, 0.0)
  }
  $probeId = $probe.id
  McpCall "mcp__engine-scene__play_enter" @{} | Out-Null
  McpCall "mcp__engine-scene__play_pause" @{} | Out-Null
  $evs = @(DrainEvents)
  Log "drain 原文: $($evs | ConvertTo-Json -Compress -Depth 8)"
  $callEv = $evs | Where-Object { $_.event -eq 'logic.call' -and $_.entityId -eq $probeId }
  if (-not $callEv) { throw "事件环须含 logic.call(entityId=$probeId): $($evs | ConvertTo-Json -Compress -Depth 6)" }
  $callResult = @($callEv)[0].result; if ([Math]::Abs([double]$callResult - 5.5) -gt 0.01) { throw "call_function add(2,3.5) 须 ≈5.5,实际 $callResult" }
  Log "runtime logic.call PASS(result=$callResult)"

  # ── 4. 缓存零重建:play_exit → 记 dll 写入时间 → 重进 → 不变 ──
  Log "== 缓存命中零重建 =="
  McpCall "mcp__engine-scene__play_exit" @{} | Out-Null
  $dll = Get-ChildItem "$demo\.forge\cache\rxdll\callprobe-*.dll" -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $dll) { throw "缺缓存产物 $demo\.forge\cache\rxdll\callprobe-*.dll" }
  $wt = $dll.LastWriteTimeUtc
  McpCall "mcp__engine-scene__play_enter" @{} | Out-Null
  McpCall "mcp__engine-scene__play_pause" @{} | Out-Null
  $evs2 = @(DrainEvents)
  Log "drain2 原文: $($evs2 | ConvertTo-Json -Compress -Depth 8)"
  $callEv2 = @(@($evs2) | Where-Object { $_.event -eq 'logic.call' } | Select-Object -First 1)[0]
  if (-not $callEv2 -or [Math]::Abs([double]$callEv2.result - 5.5) -gt 0.01) { throw "重进 logic.call 须再现 5.5: $($evs2 | ConvertTo-Json -Compress -Depth 6)" }
  $dll2 = Get-ChildItem "$demo\.forge\cache\rxdll\callprobe-*.dll" | Select-Object -First 1
  if (@(Get-ChildItem "$demo\.forge\cache\rxdll\callprobe-*.dll").Count -ne 1) { throw "缓存键漂移:产物不止一份" }
  if ($dll2.LastWriteTimeUtc -ne $wt) { throw "复跑重建(缓存未命中):$wt → $($dll2.LastWriteTimeUtc)" }
  Log "缓存命中 PASS($($dll2.Name) 零重建,重进 result=$($callEv2.result))"

  # ── 5. 运行时错误腿:args 经 NodePin 供非数组 → logic.call_error 不静默 ──
  Log "== logic.call_error 腿(NodePin args 非数组) =="
  McpCall "mcp__engine-scene__play_exit" @{} | Out-Null
  $errGraphObj = @{
    version = 1; id = "g_callerr"; name = "CallErrProbe"
    nodes = @(
      @{ id = "n1"; type = "event.on_start"; pos = @(0,0) },
      @{ id = "nx"; type = "var.set"; pos = @(1,0); inputs = @{ name = @{ const = "x" }; value = @{ const = $true } } },
      @{ id = "ng"; type = "var.get"; pos = @(1,1); inputs = @{ name = @{ const = "x" } } },
      @{ id = "n2"; type = "call.call_function"; pos = @(2,0);
         inputs = @{ module = @{ const = "Content/Scripts/callprobe.rx" }; fn = @{ const = "add" }; args = @{ node = "ng"; pin = "out" } } }
    )
    edges = @(
      @{ from = @("n1","exec"); to = @("nx","exec") }
      @{ from = @("nx","exec"); to = @("n2","exec") }
    )
  }
  $gvErr = McpCall "mcp__code-forge__graph_validate" @{ graph = $errGraphObj }
  if ($gvErr.ok -ne $true) { throw "错误腿图校验须放行(NodePin args 运行时检查): $($gvErr.errors | ConvertTo-Json -Compress)" }
  $gcErr = McpCall "mcp__code-forge__graph_create" @{ name = "call_err"; graph = $errGraphObj }
  if ($gcErr.ok -ne $true) { throw "graph_create call_err: $($gcErr | ConvertTo-Json -Compress)" }
  $errEnt = McpCall "mcp__engine-scene__entity_create" @{
    name = "probe_err"
    components = @( @{ type = "Script"; props = @{ module = ""; graphRef = "Content/Graphs/call_err.rxgraph"; props = @{} } } )
    translation = @(5.0, 0.0, 0.0)
  }
  McpCall "mcp__engine-scene__play_enter" @{} | Out-Null
  McpCall "mcp__engine-scene__play_pause" @{} | Out-Null
  $evs3 = @(DrainEvents)
  Log "drain3 原文: $($evs3 | ConvertTo-Json -Compress -Depth 8)"
  $errEv = @(@($evs3) | Where-Object { $_.event -eq 'logic.call_error' } | Select-Object -First 1)[0]
  if (-not $errEv) { throw "事件环须含 logic.call_error(args 非数组): $($evs3 | ConvertTo-Json -Compress -Depth 6)" }
  if ($errEv.reason -notmatch 'args') { throw "call_error reason 须含 args 语义: $($errEv.reason)" }
  Log "logic.call_error PASS(reason=$($errEv.reason);不静默不伪造)"
  McpCall "mcp__engine-scene__play_exit" @{} | Out-Null
  Log "RD-F4-004 call_function 运行时门冒烟 PASS(G-RD4-2)"
} finally {
  foreach ($p in $procs) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
  # 孤儿清杀(F4 环境坑:继承 stdout 句柄致后续 cargo 管道假死)——按启动时间过滤。
  $cut = $script:startTime
  Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -match 'engine-scene-mcp|engine-host|code-forge-mcp' -and $_.StartTime -gt $cut } | Stop-Process -Force -ErrorAction SilentlyContinue
  # 负腿图清理(留 callprobe.rx + call_probe.rxgraph 为提交 fixture)。
  Remove-Item "$root\projects\demo\Content\Graphs\call_bad.rxgraph", "$root\projects\demo\Content\Graphs\call_ghost.rxgraph", "$root\projects\demo\Content\Graphs\call_err.rxgraph" -Force -ErrorAction SilentlyContinue
}
