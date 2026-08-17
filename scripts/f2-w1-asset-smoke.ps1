# F2 wave.1 资产管线冒烟(G-F2-1 栈级):经 gateway → agentd → asset-pipeline-mcp 导入/列表/状态/元数据。
# 前提:target\debug\forge-agentd.exe、gateway-go\forge-gateway.exe、asset-pipeline-mcp.exe 已构建;
#        projects\demo 存在(可空,assetd 自动建目录)。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f2-w1-asset-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f2-w1-asset-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function McpCall($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 8 -Compress
  try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json" -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 60
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
  # 准备 projects/demo(若不存在,建空骨架;若存在,保留)
  $projRoot = "projects\demo"
  if (-not (Test-Path $projRoot)) { New-Item -ItemType Directory -Force $projRoot | Out-Null }

  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f2w1',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  Log "== 导入 tri_min.gltf 到 Meshes/ =="
  $src = "H:\rurix\conformance\asset\gltf\accept\tri_min.gltf"
  $out1 = McpCall "mcp__asset-pipeline__asset_import" @{ sourcePaths = @($src); destFolder = "Meshes" }
  if ($out1.failed.Count -gt 0) { throw "导入失败: $($out1.failed[0].error)" }
  if ($out1.imported.Count -ne 1) { throw "imported.Count=$($out1.imported.Count) ≠ 1" }
  $one = $out1.imported[0]
  if ($one.cacheHit -ne $false) { throw "首导入应未命中缓存" }
  if ($one.type -ne "mesh") { throw "type=$($one.type) ≠ mesh" }
  if (-not $one.guid) { throw "guid 为空" }
  if ($one.vertexCount -ne 3) { throw "vertexCount=$($one.vertexCount) ≠ 3" }
  if ($one.triangleCount -ne 1) { throw "triangleCount=$($one.triangleCount) ≠ 1" }
  Log "导入成功: guid=$($one.guid) v=$($one.vertexCount) t=$($one.triangleCount) artifact=$($one.artifact)"

  Log "== 二次导入应命中缓存 =="
  $out2 = McpCall "mcp__asset-pipeline__asset_import" @{ sourcePaths = @($src); destFolder = "Meshes" }
  $two = $out2.imported[0]
  if ($two.cacheHit -ne $true) { throw "二次导入应命中缓存" }
  if ($two.guid -ne $one.guid) { throw "GUID 漂移: $($two.guid) ≠ $($one.guid)" }
  Log "二次导入缓存命中: guid 不变 PASS"

  Log "== asset_list 应见新资产 =="
  $list = McpCall "mcp__asset-pipeline__asset_list" @{}
  $found = $list.assets | Where-Object { $_.guid -eq $one.guid }
  if (-not $found) { throw "asset_list 未见 guid=$($one.guid)" }
  Log "asset_list 见资产: path=$($found.path) size=$($found.size)"

  Log "== asset_get_meta 校验 .meta 内容 =="
  $meta = McpCall "mcp__asset-pipeline__asset_get_meta" @{ assetPath = $one.assetPath }
  if ($meta.meta.guid -ne $one.guid) { throw ".meta guid 不符" }
  if ($meta.meta.type -ne "mesh") { throw ".meta type 不符" }
  if ($meta.meta.importer -ne "gltf") { throw ".meta importer 不符" }
  Log ".meta 校验 PASS: type=$($meta.meta.type) importer=$($meta.meta.importer)"

  Log "== asset_build_status 应报 current =="
  $status = McpCall "mcp__asset-pipeline__asset_build_status" @{ assetPaths = @($one.assetPath) }
  $item = $status.items[0]
  if ($item.state -ne "current") { throw "state=$($item.state) ≠ current" }
  Log "build_status: state=$($item.state) hash=$($item.hash.Substring(0,16))... PASS"

  Log "F2 wave.1 资产管线冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
