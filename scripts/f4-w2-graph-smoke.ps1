# F4 wave.2 交互逻辑栈级冒烟:.rxgraph schema + 节点注册表 + graph_validate 校验器 +
# Script 组件 + graph MCP 三工具(graph_validate/graph_create/graph_get)+ Script 挂载引用防护。
# 链路:HttpReq → gateway(8102,JWT)→ forge-agentd(8103)→ code-forge-mcp / engine-scene-mcp → engine-host。
# 断言:door_opener ok → 四坏图逐类拒绝(GRAPH_* code)→ 同事件双入口拒绝 → graph_create 落盘 →
# graph_get 读回 → component_set 挂 Script 成功 → 不存在 graphRef 拒 SCRIPT_REF_NOT_FOUND → component_get 复核。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f4-w2-graph-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f4-w2-graph-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
# 期望成功的 MCP 调用:HTTP 非 200 或 isError → throw;返回 content[0].text 解析后 JSON。
function McpCall($tool, $arguments) {
  $outer = McpCallRaw $tool $arguments
  if ($outer.isError -eq $true) { throw "$tool 工具级 isError: $($outer.content[0].text)" }
  return $outer.content[0].text | ConvertFrom-Json
}
# 原始调用:返回外层(isError 由调用方断言)。
function McpCallRaw($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 12 -Compress
  try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json; charset=utf-8" -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 60
  } catch {
    $resp = $_.Exception.Response
    $errBody = ""
    if ($resp) { $sr = New-Object System.IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8); $errBody = $sr.ReadToEnd(); $sr.Close() }
    throw "$tool 失败 status=$([int]$resp.StatusCode) body=$errBody 请求体=$body"
  }
  if ($r.StatusCode -ne 200) { throw "$tool http=$($r.StatusCode)" }
  return $r.Content | ConvertFrom-Json
}

# 递归按 key 排序归一(消除 Rust serde BTreeMap 落盘序与 PS 文件序的属性序差异,值级深比较)。
function Normalize($v) {
  if ($v -is [pscustomobject]) {
    $o = [ordered]@{}
    foreach ($p in ($v.PSObject.Properties | Sort-Object Name)) { $o[$p.Name] = Normalize $p.Value }
    return $o
  }
  if ($v -is [System.Collections.IEnumerable] -and -not ($v -is [string])) {
    return @($v | ForEach-Object { Normalize $_ })
  }
  return $v
}

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$fx = "$root\tests\fixtures\f4"
$script:startTime = Get-Date
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f4w2',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 1. graph_validate door_opener(10 §4.1 逐字结构)→ ok ──
  Log "== graph_validate door_opener.rxgraph(好图)=="
  $door = [IO.File]::ReadAllText("$fx\door_opener.rxgraph", [Text.Encoding]::UTF8) | ConvertFrom-Json
  $v1 = McpCall "mcp__code-forge__graph_validate" @{ graph = $door }
  if ($v1.ok -ne $true) { throw "door_opener 须 ok: $($v1 | ConvertTo-Json -Compress -Depth 8)" }
  Log "graph_validate door_opener PASS(ok=true errors=[])"

  # ── 2. 四坏图逐类拒绝(断言 error code)──
  Log "== graph_validate 四坏图(逐类 GRAPH_* 拒绝)=="
  $badCases = @(
    @{ file = "bad_dangling.rxgraph"; code = "GRAPH_DANGLING_INPUT" },
    @{ file = "bad_type_mismatch.rxgraph"; code = "GRAPH_TYPE_MISMATCH" },
    @{ file = "bad_exec_cycle.rxgraph"; code = "GRAPH_EXEC_CYCLE" },
    @{ file = "bad_data_cycle.rxgraph"; code = "GRAPH_DATA_CYCLE" }
  )
  foreach ($c in $badCases) {
    $bad = [IO.File]::ReadAllText("$fx\$($c.file)", [Text.Encoding]::UTF8) | ConvertFrom-Json
    $v = McpCall "mcp__code-forge__graph_validate" @{ graph = $bad }
    if ($v.ok -ne $false) { throw "$($c.file) 须 ok=false" }
    $codes = @($v.errors | ForEach-Object { $_.code })
    if ($codes -notcontains $c.code) { throw "$($c.file) 须含 $($c.code),实际: $($codes -join ',')" }
    Log "graph_validate $($c.file) PASS($($c.code);nodeId=$(@($v.errors)[0].nodeId))"
  }

  # ── 3. 同事件双入口拒绝(GRAPH_DUP_EVENT)──
  Log "== graph_validate 同事件双入口 =="
  $dupGraph = @{
    version = 1; id = "g_dup"; name = "DupEvent"; exposedProps = @()
    nodes = @(
      @{ id = "e1"; type = "event.on_timer"; pos = @(0, 0) },
      @{ id = "e2"; type = "event.on_timer"; pos = @(100, 100) }
    )
    edges = @()
  }
  $v3 = McpCall "mcp__code-forge__graph_validate" @{ graph = $dupGraph }
  if ($v3.ok -ne $false) { throw "双入口须 ok=false" }
  if (@($v3.errors | ForEach-Object { $_.code }) -notcontains "GRAPH_DUP_EVENT") { throw "须含 GRAPH_DUP_EVENT: $($v3 | ConvertTo-Json -Compress -Depth 8)" }
  Log "graph_validate 双入口 PASS(GRAPH_DUP_EVENT)"

  # ── 4. graph_create 落盘 ──
  Log "== graph_create door_opener(落 projects/demo/Content/Graphs)=="
  $c1 = McpCall "mcp__code-forge__graph_create" @{ name = "door_opener"; graph = $door }
  if ($c1.ok -ne $true) { throw "graph_create 须 ok: $($c1 | ConvertTo-Json -Compress)" }
  if ($c1.path -ne "Content/Graphs/door_opener.rxgraph") { throw "path 不实: $($c1.path)" }
  $diskFile = "$root\projects\demo\Content\Graphs\door_opener.rxgraph"
  if (-not (Test-Path $diskFile)) { throw "落盘文件不存在: $diskFile" }
  Log "graph_create PASS(path=$($c1.path))"
  # graph_validate path 模式(相对项目根)复核落盘图。
  $vp = McpCall "mcp__code-forge__graph_validate" @{ path = "Content/Graphs/door_opener.rxgraph" }
  if ($vp.ok -ne $true) { throw "path 模式校验落盘图须 ok: $($vp | ConvertTo-Json -Compress -Depth 8)" }
  Log "graph_validate path 模式 PASS(ok=true)"

  # ── 5. graph_get 读回(与落盘文件逐字节同义:双侧同一 PS 序列化器归一后串等)──
  Log "== graph_get 读回 =="
  $g1 = McpCall "mcp__code-forge__graph_get" @{ path = "Content/Graphs/door_opener.rxgraph" }
  $diskJson = ([IO.File]::ReadAllText($diskFile, [Text.Encoding]::UTF8) | ConvertFrom-Json) | ForEach-Object { Normalize $_ } | ConvertTo-Json -Depth 12 -Compress
  $gotJson = $g1.graph | ForEach-Object { Normalize $_ } | ConvertTo-Json -Depth 12 -Compress
  if ($gotJson -cne $diskJson) { throw "graph_get 读回与落盘文件不等值`n disk=$diskJson`n got =$gotJson" }
  # 语义复核:关键结构与 10 §4.1 蓝本一致。
  if ($g1.graph.name -ne "DoorOpener" -or @($g1.graph.nodes).Count -ne 4 -or @($g1.graph.edges).Count -ne 2) { throw "读回结构不符: $gotJson" }
  if ($g1.graph.exposedProps[0].name -ne "openSpeed" -or $g1.graph.exposedProps[0].kind -ne "F32") { throw "exposedProps 不符: $gotJson" }
  Log "graph_get PASS(读回与落盘等值,nodes=4 edges=2)"

  # ── 6. component_set 挂 Script(graphRef 存在 → 成功)──
  Log "== component_set 挂 Script(graphRef=Content/Graphs/door_opener.rxgraph)=="
  $ent = McpCall "mcp__engine-scene__entity_create" @{ name = "f4w2_door" }
  $eid = $ent.id
  McpCall "mcp__engine-scene__component_add" @{ id = $eid; type = "Script"; props = @{ module = ""; graphRef = ""; props = @{} } } | Out-Null
  McpCall "mcp__engine-scene__component_set" @{ id = $eid; type = "Script"; props = @{ module = ""; graphRef = "Content/Graphs/door_opener.rxgraph"; props = @{ openSpeed = 120.0 } } } | Out-Null
  Log "component_set 挂 Script PASS(id=$eid)"

  # ── 7. 不存在 graphRef 拒绝(SCRIPT_REF_NOT_FOUND)──
  Log "== component_set 不存在 graphRef 拒绝 =="
  $raw = McpCallRaw "mcp__engine-scene__component_set" @{ id = $eid; type = "Script"; props = @{ module = ""; graphRef = "Content/Graphs/ghost.rxgraph"; props = @{} } }
  if ($raw.isError -ne $true) { throw "不存在 graphRef 须 isError: $($raw | ConvertTo-Json -Compress -Depth 8)" }
  if ($raw.content[0].text -notmatch "SCRIPT_REF_NOT_FOUND") { throw "须含 SCRIPT_REF_NOT_FOUND: $($raw.content[0].text)" }
  Log "component_set ghost graphRef PASS(SCRIPT_REF_NOT_FOUND)"

  # ── 8. component_get 复核挂载(未被 ghost 拒绝污染)──
  Log "== component_get 复核 =="
  $cg = McpCall "mcp__engine-scene__component_get" @{ id = $eid; type = "Script" }
  if ($cg.props.graphRef -ne "Content/Graphs/door_opener.rxgraph") { throw "graphRef 复核失败: $($cg | ConvertTo-Json -Compress -Depth 8)" }
  if ($cg.props.props.openSpeed -ne 120.0) { throw "props 复核失败: $($cg | ConvertTo-Json -Compress -Depth 8)" }
  Log "component_get PASS(graphRef=$($cg.props.graphRef) openSpeed=$($cg.props.props.openSpeed))"

  Log "F4 wave.2 graph 栈级冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  # 连带清杀本冒烟派生的 MCP/host 子孙进程(孤儿继承 stdout 句柄会挂住调用方管道,实测坑)。
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-scene-mcp', 'code-forge-mcp', 'asset-pipeline-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
}
