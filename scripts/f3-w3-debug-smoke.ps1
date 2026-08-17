# F3 wave.3 debug 三件套 + 「第三盏灯没阴影」全链冒烟(G-F3-3 栈级)。
# 流程:fixture 三盏灯(第三盏 castShadow=false)→ 三件套收集(graph_dump + viewport_frame + host_events)
#       → 根因定位(light3 castShadow=false)→ component_set 修复 → component_get 断言 + PIE 双态无错
#       → 修复后截图;另验 skills/list 13 篇与 debug-scene-issue 全文可读。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f3-w3-debug-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f3-w3-debug-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function McpCall($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 10 -Compress
  $wc = New-Object System.Net.WebClient
  $wc.Encoding = [Text.Encoding]::UTF8
  $wc.Headers.Add("Content-Type", "application/json; charset=utf-8")
  $wc.Headers.Add("Authorization", "Bearer $script:jwt")
  try {
    $respText = $wc.UploadString("http://127.0.0.1:8102/api/forge/mcp/call", "POST", $body)
  } catch [System.Net.WebException] {
    $resp = $_.Exception.Response
    $errBody = ""
    if ($resp) { $sr = New-Object System.IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8); $errBody = $sr.ReadToEnd() }
    throw "$tool 失败 body=$errBody 请求体=$body"
  } finally { $wc.Dispose() }
  $outer = $respText | ConvertFrom-Json
  return $outer.content[0].text | ConvertFrom-Json
}
function HttpGet($path) {
  $wc = New-Object System.Net.WebClient
  $wc.Encoding = [Text.Encoding]::UTF8
  $wc.Headers.Add("Authorization", "Bearer $script:jwt")
  try { return $wc.DownloadString("http://127.0.0.1:8102$path") | ConvertFrom-Json } finally { $wc.Dispose() }
}

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f3w3',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 0. fixture:三盏灯,第三盏 castShadow=false(「没阴影」bug 现场) ──
  Log "== fixture:三盏灯(第三盏 castShadow=false) =="
  McpCall "mcp__engine-scene__scene_new" @{ name = "f3-w3-debug" } | Out-Null
  $lightIds = @()
  foreach ($i in 1..3) {
    $shadow = ($i -ne 3)
    $c = McpCall "mcp__engine-scene__entity_create" @{
      name = "light-$i"
      components = @(@{ type = "Light"; props = @{ kind = "point"; color = @(1.0, 0.9, 0.8); intensity = 2.0; castShadow = $shadow } })
    }
    $lightIds += [uint64]$c.id
  }
  Log "fixture 就位: light-1/2/3 id=$($lightIds -join ',')(light-3 castShadow=false)"

  # ── 1. 三件套收集(debug-scene-issue §步1) ──
  Log "== 三件套:scene_graph_dump + viewport_frame + host_events =="
  $dump = McpCall "mcp__engine-scene__scene_graph_dump" @{}
  if ($dump.entityCount -ne 3) { throw "graph_dump 实体数≠3: $($dump.entityCount)" }
  $dumpLight3 = $dump.entities | Where-Object { $_.name -eq "light-3" }
  if (-not $dumpLight3) { throw "graph_dump 缺 light-3" }
  $light3Comp = $dumpLight3.components | Where-Object { $_.type -eq "Light" }
  if ($light3Comp.props.castShadow -ne $false) { throw "graph_dump 未如实呈现 light3 castShadow=false" }
  Log "graph_dump PASS: light-3 castShadow=false 入证(单次调用全量)"
  $frame1 = McpCall "mcp__engine-scene__viewport_frame" @{ width = 320; height = 240 }
  if ($frame1.error) { throw "viewport_frame 失败: $($frame1.error)(DEV_ENV_DEGRADE 须如实上报)" }
  if ($frame1.format -ne "rgba8" -or -not $frame1.pixelsB64) { throw "viewport_frame 帧数据异常: $($frame1 | ConvertTo-Json -Compress)" }
  if ($frame1.width -ne 320 -or $frame1.height -ne 240) { throw "帧尺寸错: $($frame1.width)x$($frame1.height)" }
  Log "viewport_frame PASS(320x240 rgba8 截图入证,pixelsB64=$($frame1.pixelsB64.Length) 字符)"
  $events = McpCall "mcp__engine-scene__host_events_drain" @{}
  $evCreated = @($events | Where-Object { $_.event -eq "entity.created" -and $_.name -like "light-*" })
  if ($evCreated.Count -ne 3) { throw "事件环缺三灯 entity.created(内联组件不另发 component.added): $($events | ConvertTo-Json -Compress)" }
  if (-not ($events | Where-Object { $_.event -eq "scene.created" })) { throw "缺 scene.created 事件" }
  Log "host_events_drain PASS(scene.created + 三灯 entity.created 在环)"

  # ── 2. 根因定位(证据链:graph_dump 显示 light3 castShadow=false) ──
  Log "== 根因:第三盏灯 Light.castShadow=false(graph_dump 证据) =="

  # ── 3. 修复:component_set 单字段(castShadow=true) ──
  McpCall "mcp__engine-scene__component_set" @{
    id = $lightIds[2]; type = "Light"
    props = @{ kind = "point"; color = @(1.0, 0.9, 0.8); intensity = 2.0; castShadow = $true }
  } | Out-Null
  $got = McpCall "mcp__engine-scene__component_get" @{ id = $lightIds[2]; type = "Light" }
  if ($got.props.castShadow -ne $true) { throw "修复未生效: $($got | ConvertTo-Json -Compress)" }
  if ($got.props.intensity -ne 2.0) { throw "修复误改其他字段: $($got.props | ConvertTo-Json -Compress)" }
  Log "修复 PASS: castShadow=true 生效,其余字段未动"

  # ── 4. 回归断言:PIE 双态无错 + 修复后截图 ──
  Log "== 断言:play_enter/step/exit + 修复后截图 =="
  $pe = McpCall "mcp__engine-scene__play_enter" @{}
  if ($pe.error) { throw "play_enter 失败: $($pe.error)" }
  McpCall "mcp__engine-scene__play_step" @{} | Out-Null
  $ps = McpCall "mcp__engine-scene__play_state" @{}
  if ($ps.state -notin @("play_running", "play_paused")) { throw "play_state 异常: $($ps | ConvertTo-Json -Compress)" }
  McpCall "mcp__engine-scene__play_exit" @{} | Out-Null
  $ps2 = McpCall "mcp__engine-scene__play_state" @{}
  if ($ps2.state -ne "edit") { throw "play_exit 后应回 edit 态: $($ps2 | ConvertTo-Json -Compress)" }
  $frame2 = McpCall "mcp__engine-scene__viewport_frame" @{ width = 320; height = 240 }
  if ($frame2.error) { throw "修复后截图失败" }
  Log "断言 PASS(PIE 双态无错 + 修复后 rgba8 帧)"

  # ── 5. skills 注册面:13 篇全量 + debug-scene-issue 可读 ──
  Log "== skills/list 13 篇 + debug-scene-issue 全文 =="
  $list = HttpGet "/api/forge/skills/list"
  $skills = @($list.skills)
  if ($skills.Count -ne 13) { throw "skills 数≠13: $($skills.Count)——$(($skills | ForEach-Object { $_.name }) -join ',')" }
  foreach ($n in @("scene-greybox","asset-import-batch","debug-scene-issue","skill-creator","perf-budget-check")) {
    if (-not ($skills | Where-Object { $_.name -eq $n })) { throw "skills/list 缺 $n" }
  }
  $seamCount = @($skills | Where-Object { $_.name -in @("scene-dressing","prefab-workflow","material-tuning","gen-asset-fill","logic-blueprint-gen","code-rx-migration","playtest-regression","perf-budget-check","skill-creator") }).Count
  if ($seamCount -ne 9) { throw "seam 篇数≠9: $seamCount" }
  $dbg = HttpGet "/api/forge/skills/debug-scene-issue"
  if (-not $dbg.content.Contains("scene_graph_dump")) { throw "debug-scene-issue 全文缺 graph_dump 步骤" }
  Log "skills 注册面 PASS(13 篇:4 可执行 + 9 seam;debug-scene-issue 全文可读)"

  Log "F3 wave.3 debug 三件套 + 灯影修复全链冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
