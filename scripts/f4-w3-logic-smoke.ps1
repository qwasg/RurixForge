# F4 wave.3 栈级冒烟(G-F4-3 触发开门全链):图解释执行运行时 + 事件规范序 + 物理接线 + 热重载。
# 链路:HttpReq → gateway(8102,JWT)→ forge-agentd(8103)→ engine-scene-mcp → engine-host
#   (forge-logic 解释器 + rurix-physics Jolt)。
# 事件名(实现真实名,drain 序即规范序):logic.start(load 即 on_start,10 §3.1)/
#   logic.input / logic.contact(phase=begin|persist|end)/ logic.trigger(phase=enter|exit)/
#   logic.timer / logic.update / logic.message / logic.log(debug.log 节点)/
#   logic.unsupported(未实现节点如实上报)/ logic.inject_input(注入回执)。
# 断言:①enter 发 logic.start ②player 进 Trigger AABB → logic.trigger enter + 门 yaw≈90°(±1°)
#   ③改常量 openSpeed=45 重进 → yaw≈45° ④规范序 logic.input < logic.contact(begin) < logic.update
#   ⑤play 态 component_set Script → logic.start 重发(热重载)⑥接触 Begin 进环 ⑦play_exit 回 edit。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f4-w3-logic-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f4-w3-logic-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
# 期望成功的 MCP 调用:HTTP 非 200 或 isError → throw;返回 content[0].text 解析后 JSON。
function McpCall($tool, $arguments) {
  $outer = McpCallRaw $tool $arguments
  if ($outer.isError -eq $true) { throw "$tool 工具级 isError: $($outer.content[0].text)" }
  return $outer.content[0].text | ConvertFrom-Json
}
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
function Drain() { return @(McpCall "mcp__engine-scene__host_events_drain" @{}) }
function Steps($n) { foreach ($i in 1..$n) { McpCall "mcp__engine-scene__play_step" @{} | Out-Null } }
# 四元数(xyzw)→ 绕 Y yaw 角度制。
function YawDeg($rot) {
  $x = [double]$rot[0]; $y = [double]$rot[1]; $z = [double]$rot[2]; $w = [double]$rot[3]
  return [Math]::Atan2(2.0 * ($w * $y + $x * $z), 1.0 - 2.0 * ($y * $y + $z * $z)) * 180.0 / [Math]::PI
}

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$script:startTime = Get-Date
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f4w3',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 1. 建场景:门(Script door_opener + Trigger box 2³)@原点;player(Tag player + 动态 RigidBody)@远处 ──
  Log "== scene_new + 门/player 实体 =="
  McpCall "mcp__engine-scene__scene_new" @{ name = "f4w3" } | Out-Null
  $door = McpCall "mcp__engine-scene__entity_create" @{
    name = "door"
    components = @(
      @{ type = "Script"; props = @{ module = ""; graphRef = "Content/Graphs/door_opener.rxgraph"; props = @{} } },
      @{ type = "Trigger"; props = @{ kind = "box"; extents = @(2.0, 2.0, 2.0) } }
    )
    translation = @(0.0, 0.0, 0.0)
  }
  $doorId = $door.id
  $player = McpCall "mcp__engine-scene__entity_create" @{
    name = "player"
    components = @(
      @{ type = "Tag"; props = @{ tag = "player" } },
      @{ type = "RigidBody"; props = @{ kind = "dynamic"; mass = 1.0 } }
    )
    translation = @(10.0, 0.0, 0.0)
  }
  $playerId = $player.id
  Log "实体就绪 door=$doorId player=$playerId"

  # ── 2. play_enter → logic.start(on_start 于 load 执行)──
  Log "== play_enter:断言 logic.start =="
  McpCall "mcp__engine-scene__play_enter" @{} | Out-Null
  # play.step 仅 Paused 合法(host 契约),进 Paused 供后续单帧驱动。
  McpCall "mcp__engine-scene__play_pause" @{} | Out-Null
  $evs = Drain
  $start = @($evs | Where-Object { $_.event -eq 'logic.start' -and $_.entityId -eq $doorId })
  if ($start.Count -lt 1) { throw "play_enter 后须含 logic.start(door): $($evs | ConvertTo-Json -Compress -Depth 6)" }
  Log "logic.start PASS(entityId=$doorId graphId=$($start[0].graphId))"

  # ── 3. player 进门 AABB → 90 帧 → trigger enter + yaw≈90° ──
  Log "== transform_set player→[0,0,0] + play_step×90 =="
  Drain | Out-Null
  McpCall "mcp__engine-scene__transform_set" @{ id = $playerId; translation = @(0.0, 0.0, 0.0) } | Out-Null
  Steps 90
  $evs = Drain
  $enter = @($evs | Where-Object { $_.event -eq 'logic.trigger' -and $_.phase -eq 'enter' -and $_.otherEntity -eq $playerId })
  if ($enter.Count -lt 1) { throw "须含 logic.trigger enter(other=player): $($evs | ConvertTo-Json -Compress -Depth 6)" }
  $t = McpCall "mcp__engine-scene__transform_get" @{ id = $doorId }
  $yaw = YawDeg $t.rotation
  if ([Math]::Abs($yaw - 90.0) -gt 1.0) { throw "门 yaw 须 ≈90°(±1°),实际 $yaw" }
  Log "G-F4-3 主链 PASS(trigger enter + yaw=$([Math]::Round($yaw,3))°,openSpeed 默认 90/duration 1.2s=72 帧完成)"

  # ── 4. 人工改常量:play_exit → openSpeed=45 → 重进 → yaw≈45° ──
  Log "== play_exit + component_set openSpeed=45 + 重进重测 =="
  McpCall "mcp__engine-scene__play_exit" @{} | Out-Null
  McpCall "mcp__engine-scene__component_set" @{ id = $doorId; type = "Script"; props = @{ module = ""; graphRef = "Content/Graphs/door_opener.rxgraph"; props = @{ openSpeed = 45.0 } } } | Out-Null
  McpCall "mcp__engine-scene__play_enter" @{} | Out-Null
  McpCall "mcp__engine-scene__play_pause" @{} | Out-Null
  Drain | Out-Null
  McpCall "mcp__engine-scene__transform_set" @{ id = $playerId; translation = @(0.0, 0.0, 0.0) } | Out-Null
  Steps 90
  Drain | Out-Null
  $t = McpCall "mcp__engine-scene__transform_get" @{ id = $doorId }
  $yaw = YawDeg $t.rotation
  if ([Math]::Abs($yaw - 45.0) -gt 1.0) { throw "改常量后门 yaw 须 ≈45°(±1°),实际 $yaw" }
  Log "人工改常量 PASS(yaw=$([Math]::Round($yaw,3))°,openSpeed=45 生效)"

  # ── 5/7. 规范序 + 接触 Begin:门加 static RigidBody,player 挂探针图,自由落体到门顶 ──
  Log "== 装配规范序/接触场景(门 static body + player 探针图)=="
  McpCall "mcp__engine-scene__play_exit" @{} | Out-Null
  # 探针图:on_input/on_contact_begin/on_update 各接 debug.log(规范序断言锚)。
  $probe = @{
    version = 1; id = "g_probe"; name = "Probe"; exposedProps = @()
    nodes = @(
      @{ id = "i"; type = "event.on_input"; pos = @(0, 0) },
      @{ id = "li"; type = "debug.log"; pos = @(1, 0); inputs = @{ message = @{ const = "i" } } },
      @{ id = "c"; type = "event.on_contact_begin"; pos = @(0, 1) },
      @{ id = "lc"; type = "debug.log"; pos = @(1, 1); inputs = @{ message = @{ const = "c" } } },
      @{ id = "u"; type = "event.on_update"; pos = @(0, 2) },
      @{ id = "lu"; type = "debug.log"; pos = @(1, 2); inputs = @{ message = @{ const = "u" } } }
    )
    edges = @(
      @{ from = @("i", "exec"); to = @("li", "exec") },
      @{ from = @("c", "exec"); to = @("lc", "exec") },
      @{ from = @("u", "exec"); to = @("lu", "exec") }
    )
  }
  $gv = McpCall "mcp__code-forge__graph_validate" @{ graph = $probe }
  if ($gv.ok -ne $true) { throw "探针图校验须 ok: $($gv | ConvertTo-Json -Compress -Depth 8)" }
  $gc = McpCall "mcp__code-forge__graph_create" @{ name = "f4w3_probe"; graph = $probe }
  if ($gc.ok -ne $true) { throw "graph_create 探针须 ok: $($gc | ConvertTo-Json -Compress)" }
  McpCall "mcp__engine-scene__component_add" @{ id = $doorId; type = "RigidBody"; props = @{ kind = "static"; mass = 1.0 } } | Out-Null
  McpCall "mcp__engine-scene__component_add" @{ id = $playerId; type = "Script"; props = @{ module = ""; graphRef = "Content/Graphs/f4w3_probe.rxgraph"; props = @{} } } | Out-Null
  McpCall "mcp__engine-scene__play_enter" @{} | Out-Null
  McpCall "mcp__engine-scene__play_pause" @{} | Out-Null
  Drain | Out-Null
  # player 悬门顶上方 1.5m(半高各 0.5 → 落 0.5m 触门顶,~18 帧);每帧注入 input,
  # Begin 帧同帧断言规范序 logic.input < logic.contact(begin) < logic.update(真实 drain 序)。
  McpCall "mcp__engine-scene__transform_set" @{ id = $playerId; translation = @(0.0, 1.5, 0.0) } | Out-Null
  $ordered = $false
  foreach ($f in 1..60) {
    McpCall "mcp__engine-scene__logic_inject_input" @{ action = "jump"; value = 1.0 } | Out-Null
    McpCall "mcp__engine-scene__play_step" @{} | Out-Null
    $evs = Drain
    $ii = -1; $ic = -1; $iu = -1
    for ($i = 0; $i -lt $evs.Count; $i++) {
      if ($ii -lt 0 -and $evs[$i].event -eq 'logic.input') { $ii = $i }
      if ($ic -lt 0 -and $evs[$i].event -eq 'logic.contact' -and $evs[$i].phase -eq 'begin') { $ic = $i }
      if ($iu -lt 0 -and $evs[$i].event -eq 'logic.update') { $iu = $i }
    }
    if ($ic -ge 0) {
      if (-not ($ii -ge 0 -and $iu -ge 0 -and $ii -lt $ic -and $ic -lt $iu)) {
        throw "规范序须 input<contact<update,实际索引 ii=$ii ic=$ic iu=${iu}: $($evs | ConvertTo-Json -Compress -Depth 6)"
      }
      $ordered = $true
      Log "规范序 PASS(第 $f 帧 Begin;input@$ii < contact@$ic < update@$iu)"
      break
    }
  }
  if (-not $ordered) { throw "60 帧内须出现 logic.contact begin(player 落门顶)" }
  Log "接触 Begin 进环 PASS(on_contact_begin 派发到 player 探针图)"

  # ── 6. 热重载:play 态 component_set Script(openSpeed=30)→ logic.start 重发 ──
  Log "== play 态 component_set Script 热重载 =="
  Drain | Out-Null
  McpCall "mcp__engine-scene__component_set" @{ id = $doorId; type = "Script"; props = @{ module = ""; graphRef = "Content/Graphs/door_opener.rxgraph"; props = @{ openSpeed = 30.0 } } } | Out-Null
  $evs = Drain
  $restart = @($evs | Where-Object { $_.event -eq 'logic.start' -and $_.entityId -eq $doorId })
  if ($restart.Count -lt 1) { throw "热重载须重发 logic.start(door): $($evs | ConvertTo-Json -Compress -Depth 6)" }
  Log "热重载 PASS(logic.start 重发,黑板重置)"

  # ── 8. play_exit → edit ──
  McpCall "mcp__engine-scene__play_exit" @{} | Out-Null
  $ps = McpCall "mcp__engine-scene__play_state" @{}
  if ($ps.state -ne "edit") { throw "play_exit 后须 edit,实际 $($ps.state)" }
  Log "play_exit PASS(state=edit)"

  Log "F4 wave.3 logic 栈级冒烟 PASS"
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
