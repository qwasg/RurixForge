# F2 wave.2 引用防护冒烟(G-F2-2 栈级):经 gateway → agentd → asset-pipeline-mcp。
# 前提:target\debug\forge-agentd.exe、gateway-go\forge-gateway.exe、asset-pipeline-mcp.exe 已构建;
#        projects\demo 存在。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f2-w2-refs-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f2-w2-refs-smoke-$ts.log"
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
  $projRoot = "projects\demo"
  if (-not (Test-Path $projRoot)) { New-Item -ItemType Directory -Force $projRoot | Out-Null }

  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f2w2',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  Log "== 导入网格 + 建场景引用 =="
  $src = "H:\rurix\conformance\asset\gltf\accept\tri_min.gltf"
  $out = McpCall "mcp__asset-pipeline__asset_import" @{ sourcePaths = @($src); destFolder = "Meshes" }
  $meshPath = $out.imported[0].assetPath
  $meshGuid = $out.imported[0].guid
  Log "网格: $meshPath guid=$meshGuid"

  # 建场景文件引用该 GUID,并写 .meta 使其进入已知 GUID 表。
  $sceneDir = "projects\demo\Content\Scenes"
  New-Item -ItemType Directory -Force $sceneDir | Out-Null
  $sceneFile = "$sceneDir\Main.rxscene"
  $sceneContent = '{"entities":[{"components":[{"props":{"mesh":"' + $meshGuid + '"},"type":"MeshRenderer"}]}]}'
  [IO.File]::WriteAllText("$root\$sceneFile", $sceneContent)
  $sceneMeta = "guid: $(New-Guid)`ntype: scene`nimporter: scene`n"
  [IO.File]::WriteAllText("$root\$sceneDir\Main.rxscene.meta", $sceneMeta)

  Log "== asset_refs 应见场景→网格边 =="
  $refs = McpCall "mcp__asset-pipeline__asset_refs" @{ assetPath = $meshPath; direction = "referencedBy" }
  if ($refs.edges.Count -eq 0) { throw "referencedBy 为空,场景未引用网格" }
  $fromGuid = $refs.edges[0].from
  Log "referencedBy: from=$fromGuid type=$($refs.edges[0].type)"

  Log "== asset_delete 应被阻断 =="
  $del = McpCall "mcp__asset-pipeline__asset_delete" @{ assetPaths = @($meshPath) }
  if ($del.deleted.Count -ne 0) { throw "删除未被阻断" }
  if ($del.blockedByRefs.Count -eq 0) { throw "blockedByRefs 为空" }
  $blockers = $del.blockedByRefs[0].referencedBy
  Log "删除阻断: blockedBy=$($blockers -join ',') PASS"

  Log "== asset_move 应留 redirector =="
  $mv = McpCall "mcp__asset-pipeline__asset_move" @{ assetPath = $meshPath; destFolder = "Prefabs" }
  if ($mv.moved -ne $true) { throw "移动失败" }
  $red = $mv.redirector
  if ($red.guid -ne $meshGuid) { throw "redirector guid 不符" }
  Log "redirector: $($red.oldPath) → $($red.newPath) PASS"

  Log "== asset_fix_redirectors 应清除 =="
  $fix = McpCall "mcp__asset-pipeline__asset_fix_redirectors" @{}
  if ($fix.fixed.Count -ne 1) { throw "fixed.Count=$($fix.fixed.Count) ≠ 1" }
  if ($fix.fixed[0] -ne $meshGuid) { throw "fixed guid 不符" }
  Log "fix_redirectors: 清除 guid=$($fix.fixed[0]) PASS"

  Log "== 移动后引用仍不断链(GUID 不变) =="
  $refs2 = McpCall "mcp__asset-pipeline__asset_refs" @{ assetPath = $red.newPath; direction = "referencedBy" }
  if ($refs2.edges.Count -eq 0) { throw "移动后引用断链" }
  Log "移动后引用仍存 PASS"

  Log "F2 wave.2 引用防护冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
