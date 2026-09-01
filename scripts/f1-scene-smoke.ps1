# F1 场景闭环冒烟(G-F1-1/G-F1-4 栈级):批量创建 + checkpoint 回滚 + 保存/加载确定性。
# 前提:target/debug/forge-agentd.exe 与 gateway-go/forge-gateway.exe 已构建。
# 用法: pwsh scripts/f1-scene-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f1-scene-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function McpCall($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 8 -Compress
  try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json" -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 30
  } catch {
    $resp = $_.Exception.Response
    $errBody = ""
    if ($resp) { $sr = New-Object System.IO.StreamReader($resp.GetResponseStream()); $errBody = $sr.ReadToEnd() }
    throw "$tool 失败 status=$([int]$resp.StatusCode) body=$errBody 请求体=$body"
  }
  if ($r.StatusCode -ne 200) { throw "$tool http=$($r.StatusCode)" }
  $outer = $r.Content | ConvertFrom-Json
  return $outer.content[0].text | ConvertFrom-Json
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
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f1',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  Log "== G-F1-4:entity_batch_apply 创建 10 立方体排一列 =="
  McpCall "mcp__engine-scene__scene_new" @{ name = "f1-smoke" } | Out-Null
  $ops = 0..9 | ForEach-Object { @{ op = "create"; name = "cube-$_"; translation = @([double]$_, 0, 0); components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = "cube.rxmesh"; material = "default" } }) } }
  $batch = McpCall "mcp__engine-scene__entity_batch_apply" @{ ops = $ops }
  Log "batch_apply => $($batch | ConvertTo-Json -Compress -Depth 6)"
  $sum = McpCall "mcp__engine-scene__scene_summary" @{}
  if ($sum.entityCount -ne 10) { throw "entityCount=$($sum.entityCount) ≠ 10" }
  $list = McpCall "mcp__engine-scene__entity_list" @{}
  $xs = @($list.entities | ForEach-Object { [double]$_.transform.translation[0] } | Sort-Object)
  $expected = @(0..9 | ForEach-Object { [double]$_ })
  if (($xs -join ',') -ne ($expected -join ',')) { throw "x 坐标序列异常: $($xs -join ',')" }
  Log "entityCount=10,x=0..9 排一列 PASS"

  Log "== G-F1-2 栈级:checkpoint 回滚 =="
  McpCall "mcp__engine-scene__scene_checkpoint" @{} | Out-Null
  McpCall "mcp__engine-scene__entity_create" @{ name = "temp-entity" } | Out-Null
  $sum2 = McpCall "mcp__engine-scene__scene_summary" @{}
  if ($sum2.entityCount -ne 11) { throw "checkpoint 后创建异常: $($sum2.entityCount)" }
  McpCall "mcp__engine-scene__scene_rollback" @{} | Out-Null
  $sum3 = McpCall "mcp__engine-scene__scene_summary" @{}
  if ($sum3.entityCount -ne 10) { throw "rollback 后 entityCount=$($sum3.entityCount) ≠ 10" }
  Log "checkpoint→+1→rollback→10 PASS"

  Log "== G-F1-1 栈级:save → new → load 跨状态确定性 =="
  $scenePath = "data\f1-smoke.rxscene"
  McpCall "mcp__engine-scene__scene_save" @{ path = $scenePath } | Out-Null
  $before = McpCall "mcp__engine-scene__entity_list" @{}
  McpCall "mcp__engine-scene__scene_new" @{ name = "blank" } | Out-Null
  $sumBlank = McpCall "mcp__engine-scene__scene_summary" @{}
  if ($sumBlank.entityCount -ne 0) { throw "scene.new 后非空: $($sumBlank.entityCount)" }
  McpCall "mcp__engine-scene__scene_load" @{ path = $scenePath } | Out-Null
  $after = McpCall "mcp__engine-scene__entity_list" @{}
  if (($before | ConvertTo-Json -Compress -Depth 8) -ne ($after | ConvertTo-Json -Compress -Depth 8)) { throw "load 后实体表与保存前不一致" }
  Log "save/new/load 后实体表逐字段一致 PASS"

  Log "F1 SMOKE PASS"
  exit 0
} catch {
  Log "F1 SMOKE FAIL: $($_.Exception.Message)"
  exit 1
} finally {
  foreach ($p in $procs) { try { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } catch {} }
}
