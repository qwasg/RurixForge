# F5 wave.2 gen-model 三工具 + 生成管线冒烟(G-F5-2):agentd 第五 ServerKind mcp__gen-model__ →
# gen-model-mcp(内嵌 gend)→ asset_import 同一构建链(rurix-geom-build DAG → .rxmesh)。
# 断言:①gen_texture_set 自动入管线(wave.1 图像面回归:3 map 落 Content/Textures/ +
#   .meta provenance origin=gen-image + detail.map=槽位名)
#   ②gen_mesh 显式门:{prompt:"a chair"} → GEN_BACKEND_NOT_CONFIGURED(注册表无 text2mesh
#   适配器,D-F5-E,不伪造生成能力);{prompt:"",imageRef:""} 双空 → GEN_BAD_PARAMS(参数校验先行)
#   ③gen-model gen_accept:fixture tri_min.gltf 拷入 .forge/tmp/gen/(模拟用户自备生成产物,
#   provenance detail.backendId=user-provided 如实标注)→ Meshes/f5w2_chair.gltf + guid +
#   .rxmesh artifact 落盘 + .meta origin=gen-model + sourceRefs 含 meshFileRef +
#   import_settings.generateLods 透传(缓存键构成)
#   ④确定性复跑:同源同 importSettings 改名 f5w2_chair2 → guid 不同但缓存命中(cacheHit=true,08 §4.3)
#   ⑤清理:Content/Meshes/f5w2_* 与 Content/Textures/wood_*(纹理名 = prompt slug "wood")不删——
#   留作 wave.3 场景可见 fixture(f5w2_chair 给 desktop 冒烟用);.forge/tmp/gen/ 产物清
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f5-w2-gen-pipeline-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f5-w2-gen-pipeline-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
# 原始返回(含 isError 工具错误);McpCall 仅用于期望成功路径。
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
  return $r.Content
}
function McpCall($tool, $arguments) {
  $raw = McpCallRaw $tool $arguments
  $outer = $raw | ConvertFrom-Json
  if ($outer.isError -eq $true) { throw "$tool 工具级 isError: $($outer.content[0].text)" }
  return $outer.content[0].text | ConvertFrom-Json
}
# 期望工具级错误:返回解析后的 {error,message};否则 throw。
function McpCallErr($tool, $arguments) {
  $raw = McpCallRaw $tool $arguments
  $outer = $raw | ConvertFrom-Json
  if ($outer.isError -ne $true) { throw "$tool 须 isError:true,实际成功: $raw" }
  return $outer.content[0].text | ConvertFrom-Json
}
function WriteJsonNoBom($path, $text) {
  $dir = Split-Path -Parent $path
  if ($dir) { New-Item -ItemType Directory -Force $dir | Out-Null }
  [IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding($false)))
}

New-Item -ItemType Directory -Force evidence | Out-Null
# 前置二进制检查(gen-model-mcp 为懒加载 spawn,缺失时给明确指引)。
foreach ($b in @("target\debug\forge-agentd.exe", "gateway-go\forge-gateway.exe", "target\debug\gen-model-mcp.exe", "target\debug\gen-image-mcp.exe")) {
  if (-not (Test-Path $b)) { throw "缺二进制 $b —— 先 cargo build --workspace" }
}
$procs = @()
$script:startTime = Get-Date
$dataDir = "$root\data"
$genCfg = "$dataDir\gen-backends.json"
# 配置备份(空 = 原不存在,还原 = 删除)。
$genCfgBak = if (Test-Path $genCfg) { [IO.File]::ReadAllText($genCfg) } else { $null }
$demo = "$root\projects\demo"
$tmpGen = "$demo\.forge\tmp\gen"
# "wood 木纹"(逐字契约 prompt;[char] 构造避免 .ps1 编码歧义)。
$woodPrompt = "wood " + [char]0x6728 + [char]0x7EB9
# 保留资产清单(不删,留作 wave.3 fixture;如实注释):
#   Content/Meshes/f5w2_chair.gltf(.meta)+ f5w2_chair2.gltf(.meta) —— desktop 冒烟用
#   Content/Textures/wood_{albedo,normal,roughness}.png(.meta) —— gen_texture_set 产物
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f5w2',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # 前置:写 gen-backends.json local-mock enabled(gen_texture_set 需要已配置图像后端)。
  WriteJsonNoBom $genCfg '{"backends":[{"id":"local-mock","kind":"local","enabled":true}]}'

  # ── 1. gen_texture_set 自动入管线(图像面回归;3 map 落 Textures + provenance detail.map)──
  Log "== gen_texture_set 三 map 自动入管线 =="
  $ts1 = McpCall "mcp__gen-image__gen_texture_set" @{ prompt = $woodPrompt; materialKind = "pbr"; maps = @("albedo", "normal", "roughness"); size = 256 }
  $tas = @($ts1.textureAssets)
  if ($tas.Count -ne 3) { throw "须 3 textureAssets: $($ts1 | ConvertTo-Json -Compress -Depth 6)" }
  foreach ($m in @("albedo", "normal", "roughness")) {
    $ta = @($tas) | Where-Object { $_.map -eq $m }
    if (-not $ta) { throw "textureAssets 缺 map=$m" }
    $ap = [string]$ta.assetPath
    if ($ap -ne "Textures/wood_$m.png") { throw "map $m assetPath 须 Textures/wood_$m.png,实: $ap" }
    $fp = "$demo\Content\$($ap -replace '/', '\')"
    if (-not (Test-Path $fp)) { throw "纹理未落盘: $ap" }
    $mt = [IO.File]::ReadAllText("$fp.meta", [Text.Encoding]::UTF8)
    if ($mt -notmatch "origin:\s*gen-image") { throw "$m provenance.origin 须 gen-image: $mt" }
    if ($mt -notmatch "map:\s*$m\b") { throw "${m} detail.map 须 ${m}: $mt" }
  }
  Log "gen_texture_set PASS(3 map 落 Content/Textures/wood_*;provenance origin=gen-image + detail.map 逐槽位)"

  # ── 2. gen_mesh 显式门(NOT_CONFIGURED / 双空 BAD_PARAMS)──
  Log "== gen_mesh 门:text2mesh 无后端显式 NOT_CONFIGURED =="
  $e1 = McpCallErr "mcp__gen-model__gen_mesh" @{ prompt = "a chair" }
  if ($e1.error -ne "GEN_BACKEND_NOT_CONFIGURED") { throw "gen_mesh 须 NOT_CONFIGURED: $($e1 | ConvertTo-Json -Compress)" }
  $e2 = McpCallErr "mcp__gen-model__gen_mesh" @{ prompt = ""; imageRef = "" }
  if ($e2.error -ne "GEN_BAD_PARAMS") { throw "gen_mesh 双空须 GEN_BAD_PARAMS: $($e2 | ConvertTo-Json -Compress)" }
  Log "gen_mesh 门 PASS(NOT_CONFIGURED 显式不伪造;双空 BAD_PARAMS 参数校验先行)"

  # ── 3. gen-model gen_accept 走 asset_import 同链(fixture gltf → Meshes + .rxmesh)──
  Log "== gen-model gen_accept:tri_min.gltf → Meshes/f5w2_chair(importSettings 透传)=="
  New-Item -ItemType Directory -Force $tmpGen | Out-Null
  Copy-Item "$demo\Content\Prefabs\tri_min.gltf" "$tmpGen\f5w2-gen-chair.gltf" -Force
  $meshRef = ".forge/tmp/gen/f5w2-gen-chair.gltf"
  $accArgs = @{ meshFileRef = $meshRef; destFolder = "Meshes"; name = "f5w2_chair"; importSettings = @{ generateLods = @(0.5) } }
  $acc = McpCall "mcp__gen-model__gen_accept" $accArgs
  if ($acc.assetPath -ne "Meshes/f5w2_chair.gltf") { throw "assetPath 须 Meshes/f5w2_chair.gltf: $($acc | ConvertTo-Json -Compress)" }
  if (-not $acc.guid) { throw "缺 guid" }
  $art = [string]$acc.artifact
  if (-not ($art.StartsWith("rxmesh/") -and $art.EndsWith(".rxmesh"))) { throw "artifact 须 rxmesh/*.rxmesh: $($acc | ConvertTo-Json -Compress)" }
  $artAbs = "$demo\.forge\cache\$($art -replace '/', '\')"
  if (-not (Test-Path $artAbs)) { throw ".rxmesh 产物未落盘: $artAbs" }
  $gltfPath = "$demo\Content\Meshes\f5w2_chair.gltf"
  if (-not (Test-Path $gltfPath)) { throw "资产未落 Content/Meshes/f5w2_chair.gltf" }
  $metaText = [IO.File]::ReadAllText("$gltfPath.meta", [Text.Encoding]::UTF8)
  if ($metaText -notmatch "origin:\s*gen-model") { throw ".meta provenance.origin 须 gen-model: $metaText" }
  if ($metaText -notmatch "backendId:\s*user-provided") { throw "detail.backendId 须 user-provided(用户自备如实标注): $metaText" }
  if (-not $metaText.Contains(".forge/tmp/gen/f5w2-gen-chair.gltf")) { throw "detail.sourceRefs 须含 meshFileRef: $metaText" }
  if ($metaText -notmatch "generatedAt:\s*\S") { throw "detail.generatedAt 须非空: $metaText" }
  if ($metaText -notmatch "import_settings:[\s\S]*generateLods:[\s\S]*0\.5") { throw "import_settings.generateLods 须透传: $metaText" }
  Log "gen_accept PASS(assetPath=$($acc.assetPath) guid=$($acc.guid);artifact=$art 落盘;origin=gen-model/backendId=user-provided/sourceRefs/generateLods 全断言)"

  # ── 4. 确定性复跑:改名 f5w2_chair2 → guid 不同,同源同设置 → 缓存命中 ──
  Log "== 确定性复跑:f5w2_chair2 缓存命中(08 §4.3)=="
  $acc2 = McpCall "mcp__gen-model__gen_accept" @{ meshFileRef = $meshRef; destFolder = "Meshes"; name = "f5w2_chair2"; importSettings = @{ generateLods = @(0.5) } }
  if ($acc2.assetPath -ne "Meshes/f5w2_chair2.gltf") { throw "复跑 assetPath 异常: $($acc2 | ConvertTo-Json -Compress)" }
  if ($acc2.guid -eq $acc.guid) { throw "改名复跑 guid 须不同" }
  if ($acc2.cacheHit -ne $true) { throw "同源字节+同 importSettings 须缓存命中 cacheHit=true: $($acc2 | ConvertTo-Json -Compress)" }
  if ([string]$acc2.artifact -ne $art) { throw "同缓存键须同 .rxmesh 产物: $($acc2.artifact) vs $art" }
  Log "复跑 PASS(guid 不同;cacheHit=true;artifact 同 $art)"

  Log "F5 wave.2 gen-model 三工具 + 生成管线冒烟 PASS(G-F5-2)"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  # 连带清杀本冒烟派生的 MCP/host 子孙进程(孤儿继承 stdout 句柄会挂住调用方管道,实测坑)。
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-scene-mcp', 'code-forge-mcp', 'asset-pipeline-mcp', 'gen-image-mcp', 'gen-model-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
  # 还原 gen-backends.json(备份为 null = 原不存在 → 删除)。
  try {
    if ($null -ne $genCfgBak) { WriteJsonNoBom $genCfg $genCfgBak } elseif (Test-Path $genCfg) { Remove-Item $genCfg -Force }
  } catch { Log "配置还原异常: $_" }
  # Content/Meshes/f5w2_* 与 Content/Textures/wood_* 不删(留作 wave.3 场景可见 fixture);
  # 仅清 .forge/tmp/gen/ 冒烟产物(staging 命名副本 f5w2_*/wood_* + fixture 拷入 f5w2-* + 候选 gen-*)。
  try { Remove-Item "$tmpGen\f5w2_*" -Force -ErrorAction SilentlyContinue } catch {}
  try { Remove-Item "$tmpGen\f5w2-*" -Force -ErrorAction SilentlyContinue } catch {}
  try { Remove-Item "$tmpGen\wood_*" -Force -ErrorAction SilentlyContinue } catch {}
  try { Remove-Item "$tmpGen\gen-*" -Force -ErrorAction SilentlyContinue } catch {}
}
