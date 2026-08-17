# F3 wave.1 swarm 分片 + multitask 确定性执行器冒烟(G-F3-1 栈级)。
# 流程:scene_new → 40 关卡块 fixture → POST /swarm/execute(scene-partition 4 片 add RigidBody)
#       → 聚合报告校验(40 全覆盖/4 片全 done/无失败不遮蔽)→ component_get 抽验
#       → 重复输入集 409 GOV_SWARM_SHARD_OVERLAP → seed-demo + state 校验。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f3-w1-swarm-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f3-w1-swarm-smoke-$ts.log"
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
function HttpReq($method, $path, $obj) {
  $body = if ($obj) { $obj | ConvertTo-Json -Depth 10 -Compress } else { "" }
  $wc = New-Object System.Net.WebClient
  $wc.Encoding = [Text.Encoding]::UTF8
  $wc.Headers.Add("Content-Type", "application/json; charset=utf-8")
  $wc.Headers.Add("Authorization", "Bearer $script:jwt")
  try {
    # GET 无请求体:UploadString 会被 .NET 以「GET 不可带 body」客户端侧拒绝,须走 DownloadString。
    if ($method -eq "GET") {
      $respText = $wc.DownloadString("http://127.0.0.1:8102$path")
    } else {
      $respText = $wc.UploadString("http://127.0.0.1:8102$path", $method, $body)
    }
    return @{ status = 200; json = ($respText | ConvertFrom-Json) }
  } catch [System.Net.WebException] {
    $resp = $_.Exception.Response
    $code = if ($resp) { [int]$resp.StatusCode } else { 0 }
    $errBody = ""
    if ($resp) { $sr = New-Object System.IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8); $errBody = $sr.ReadToEnd(); $sr.Close() }
    $j = $null; try { $j = $errBody | ConvertFrom-Json } catch {}
    return @{ status = $code; json = $j; raw = $errBody }
  } finally { $wc.Dispose() }
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
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f3w1',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 0. fixture:独立场景 40 个关卡块(不 scene_save,demo 项目零污染) ──
  Log "== fixture:scene_new + 40 关卡块 =="
  McpCall "mcp__engine-scene__scene_new" @{ name = "f3-w1-swarm-smoke" } | Out-Null
  $ids = @()
  foreach ($i in 1..40) {
    $c = McpCall "mcp__engine-scene__entity_create" @{ name = "block-$i" }
    $ids += [uint64]$c.id
  }
  if ($ids.Count -ne 40) { throw "fixture 实体数错: $($ids.Count)" }
  Log "fixture 就位: 40 实体 id $($ids[0])..$($ids[39])"

  # ── 1. multitask 执行:scene-partition 4 片 add RigidBody ──
  Log "== POST /api/forge/swarm/execute(40 块碰撞体,4 片) =="
  $exec = HttpReq "POST" "/api/forge/swarm/execute" @{
    shardType = "scene-partition"
    items = $ids
    shardCount = 4
    operation = @{ kind = "add_component"; type = "RigidBody"; props = @{ kind = "static"; mass = 0 } }
  }
  if ($exec.status -ne 200) { throw "swarm/execute 失败: $($exec.raw)" }
  $agg = $exec.json.aggregate
  if ($agg.totalItems -ne 40) { throw "totalItems≠40: $($agg.totalItems)" }
  if ($agg.succeeded -ne 40) { throw "succeeded≠40: $($agg.succeeded); errors=$($exec.json.shards.errors | ConvertTo-Json -Compress)" }
  if ($agg.failed -ne 0) { throw "failed≠0: $($agg.failed)" }
  if (-not $agg.disjoint) { throw "disjoint≠true" }
  $shards = @($exec.json.shards)
  if ($shards.Count -ne 4) { throw "分片数≠4: $($shards.Count)" }
  foreach ($s in $shards) {
    if ($s.status -ne "done") { throw "分片 $($s.shardId) 非 done: $($s.status)(失败不遮蔽)" }
    if ($s.okCount -ne 10) { throw "分片 $($s.shardId) okCount≠10: $($s.okCount)" }
  }
  Log "分片执行 PASS: 4 片全 done,每片 10 实体,聚合 succeeded=40 failed=0 disjoint=true"

  # ── 2. 抽验:首/中/末实体 RigidBody 真实落上 ──
  foreach ($id in @($ids[0], $ids[19], $ids[39])) {
    $comp = McpCall "mcp__engine-scene__component_get" @{ id = $id; type = "RigidBody" }
    if ($comp.props.kind -ne "static") { throw "实体 $id RigidBody 抽验失败: $($comp | ConvertTo-Json -Compress)" }
  }
  Log "component_get 抽验 PASS(3/40 抽查,RigidBody kind=static)"

  # ── 3. 冲突检测:输入集重复 → 409 GOV_SWARM_SHARD_OVERLAP ──
  Log "== 冲突检测:重复输入集 =="
  $dup = HttpReq "POST" "/api/forge/swarm/execute" @{
    shardType = "scene-partition"
    items = @($ids[0], $ids[0])
    operation = @{ kind = "add_component"; type = "RigidBody"; props = @{ kind = "static"; mass = 0 } }
  }
  if ($dup.status -ne 409) { throw "重复输入集应 409,实: $($dup.status) $($dup.raw)" }
  if ($dup.json.error.code -ne "GOV_SWARM_SHARD_OVERLAP") { throw "错误码错: $($dup.json.error.code)" }
  Log "GOV_SWARM_SHARD_OVERLAP PASS(409)"

  # ── 4. seed-demo + state ──
  Log "== seed-demo + state =="
  $seed = HttpReq "POST" "/api/forge/swarm/seed-demo" @{}
  if ($seed.status -ne 200 -or $seed.json.seeded -ne 2) { throw "seed-demo 异常: $($seed.raw)" }
  $st = HttpReq "GET" "/api/forge/swarm/state" $null
  $nodes = @($st.json.nodes)
  $doneShards = @($st.json.shards | Where-Object { $_.status -eq "done" })
  if ($nodes.Count -ne 3) { throw "节点数≠3: $($nodes.Count)" }
  if ($doneShards.Count -ne 4) { throw "done 分片数≠4: $($doneShards.Count)" }
  Log "state PASS: 3 节点(1 默认 + 2 播种),4 done 分片可见"

  Log "F3 wave.1 swarm 分片 + multitask 执行器冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
