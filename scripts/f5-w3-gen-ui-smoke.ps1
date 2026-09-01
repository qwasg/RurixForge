# F5 wave.3 gen UI 冒烟(G-F5-3 前端门):「生成 4 张候选木纹 → 接受 1 张 → 材质引用 → 场景可见」。
# 流程:写 gen-backends.json(local-mock enabled)→ 起 agentd(8103)+ gateway(8102)→
#   ① desktop 冒烟 FORGE_SMOKE_SCENARIO=gen(UI 黑盒链:编辑器 → Assets 右键 Generate... →
#      填 prompt「wood 木纹」→ 提交 → 候选卡 >=4 → Accept 首张 → Assets 列表见 wood-<seed>)
#   ② 截图 + smoke.log 计数断言(取末行,append 留档)
#   ③ gateway MCP 链:asset_list 找 UI 接受的新纹理(Textures/wood-<seed>.png)guid →
#      material_create {textures:{albedo:guid}} → entity_create MeshRenderer{mesh:f5w2_chair guid,
#      material:材质guid} → viewport_frame nonZeroPixels>0(场景可见)→
#      .meta provenance origin=gen-image 断言
# 前置:cargo build --workspace(agentd/gen-image-mcp/asset-pipeline-mcp/engine-scene-mcp exe);
#       gateway-go\forge-gateway.exe;packages\client dist 已构建;projects\demo 有 f5w2_chair.gltf(f5-w2 留档)。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f5-w3-gen-ui-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f5-w3-gen-ui-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
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
function WriteJsonNoBom($path, $text) {
  $dir = Split-Path -Parent $path
  if ($dir) { New-Item -ItemType Directory -Force $dir | Out-Null }
  [IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding($false)))
}

New-Item -ItemType Directory -Force evidence | Out-Null
foreach ($b in @("target\debug\forge-agentd.exe", "gateway-go\forge-gateway.exe", "target\debug\gen-image-mcp.exe", "target\debug\asset-pipeline-mcp.exe", "target\debug\engine-scene-mcp.exe")) {
  if (-not (Test-Path $b)) { throw "缺二进制 $b —— 先 cargo build --workspace" }
}
if (-not (Test-Path "packages\client\dist\index.html")) { throw "client dist 未构建(先 pnpm --filter @forge/client build)" }
if (-not (Test-Path "projects\demo\Content\Meshes\f5w2_chair.gltf")) { throw "缺 f5w2_chair.gltf fixture(先跑 scripts\f5-w2-gen-pipeline-smoke.ps1)" }

$procs = @()
$script:startTime = Get-Date
$dataDir = "$root\data"
$genCfg = "$dataDir\gen-backends.json"
# 配置备份(空 = 原不存在,还原 = 删除)。
$genCfgBak = if (Test-Path $genCfg) { [IO.File]::ReadAllText($genCfg) } else { $null }
$demo = "$root\projects\demo"
$tmpGen = "$demo\.forge\tmp\gen"
try {
  # ── 0. 前置:写 gen-backends.json local-mock enabled(UI 生成链需要已配置后端)──
  WriteJsonNoBom $genCfg '{"backends":[{"id":"local-mock","kind":"local","enabled":true}]}'

  # ── 1. 起 agentd + gateway(desktop 冒烟自行 spawn host → 8103;MCP 断言腿走 8102)──
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f5w3',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # agentd gen REST 面栈级抽查(UI 链的数据源同源):GET backends local-mock configured=true。
  $backends = Invoke-WebRequest -Uri "http://127.0.0.1:8103/api/forge/gen/backends" -UseBasicParsing -TimeoutSec 5 | Select-Object -ExpandProperty Content | ConvertFrom-Json
  $lm = @($backends.backends) | Where-Object { $_.id -eq "local-mock" }
  if (-not $lm -or $lm.configured -ne $true) { throw "gen REST backends local-mock configured 须 true: $($backends | ConvertTo-Json -Compress -Depth 6)" }
  Log "agentd gen REST GET /api/forge/gen/backends PASS(local-mock configured=true,密钥不出)"

  # ── 2. desktop 冒烟:gen 场景(UI 黑盒:4 候选 → Accept 首张 → 资产可见)──
  Log "== desktop smoke: FORGE_SMOKE_SCENARIO=gen =="
  $env:FORGE_SMOKE_SCENARIO = "gen"
  # PS 5.1:2>&1 合并后 stderr 行被包成 ErrorRecord,Stop 偏好会误抛——先 Continue 收集再按 exit code 判定
  $ErrorActionPreference = 'Continue'
  $smokeOut = pnpm --filter @forge/desktop smoke 2>&1
  $smokeCode = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  $smokeOut | ForEach-Object { Log "  $_" }
  if ($smokeCode -ne 0) { throw "desktop smoke 失败(exit=$smokeCode)" }
  Remove-Item Env:FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue

  # ── 3. 断言:gen 截图本次产出 + smoke.log 候选/接受计数(取末行)──
  $shot = Get-ChildItem "apps\desktop\evidence\desktop-smoke-gen-*.png" -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -ge $script:startTime.AddSeconds(-5) } |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if (-not $shot) { throw "未找到本次 gen 场景截图" }
  Log "截图: $($shot.Name) ($($shot.Length) B)"

  $candLines = Get-Content "apps\desktop\evidence\smoke.log" | Where-Object { $_ -match "scenario=gen candidates: (\d+)" }
  if (-not $candLines) { throw "smoke.log 缺 gen candidates 计数行" }
  $candLines[-1] -match "candidates: (\d+)" | Out-Null
  $candCount = [int]$Matches[1]
  if ($candCount -lt 4) { throw "UI 生成候选不足: $candCount < 4" }
  Log "UI 候选计数: $candCount(>=4)"

  $accLines = Get-Content "apps\desktop\evidence\smoke.log" | Where-Object { $_ -match "scenario=gen accepted assets: (\d+)" }
  if (-not $accLines) { throw "smoke.log 缺 gen accepted assets 计数行" }
  $accLines[-1] -match "accepted assets: (\d+)" | Out-Null
  if ([int]$Matches[1] -lt 1) { throw "UI Accept 后 Assets 列表未见新资产" }
  Log "UI Accept 后新资产可见 PASS(wood-<seed> 条目 >=1)"

  # ── 4. gateway MCP 链:材质引用 + 场景可见 + provenance ──
  Log "== 材质引用链:asset_list → material_create(albedo=新纹理 guid) =="
  $list = McpCall "mcp__asset-pipeline__asset_list" @{}
  $genTex = @($list.assets) | Where-Object { $_.path -match '^Textures/wood-\d+\.png$' } | Select-Object -First 1
  if (-not $genTex) { throw "asset_list 未见 UI 接受的新纹理(Textures/wood-<seed>.png)" }
  $texGuid = [string]$genTex.guid
  Log "新纹理: $($genTex.path) guid=$texGuid"
  # provenance:origin=gen-image(UI Accept 真实落 Content/)。
  $metaText = [IO.File]::ReadAllText("$demo\Content\$($genTex.path -replace '/', '\').meta", [Text.Encoding]::UTF8)
  if ($metaText -notmatch "origin:\s*gen-image") { throw ".meta provenance.origin 须 gen-image: $metaText" }
  Log "provenance PASS(origin=gen-image)"

  $mat = McpCall "mcp__asset-pipeline__material_create" @{
    name = "f5w3_gen_mat"
    params = @{ roughness = 0.8 }
    textures = @{ albedo = $texGuid }
  }
  if ($mat.error) { throw "material_create 失败: $($mat.message)" }
  $matGuid = [string]$mat.guid
  if (-not $matGuid) { throw "材质 GUID 为空" }
  Log "材质创建 PASS: $($mat.assetPath) guid=$matGuid(albedo=$texGuid)"

  $meshAsset = @($list.assets) | Where-Object { $_.path -eq "Meshes/f5w2_chair.gltf" } | Select-Object -First 1
  if (-not $meshAsset) { throw "asset_list 缺 Meshes/f5w2_chair.gltf(f5-w2 留档)" }
  $ent = McpCall "mcp__engine-scene__entity_create" @{
    name = "F5W3GenChair"
    translation = @(0, 0.5, 0)
    components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = $meshAsset.guid; material = $matGuid } })
  }
  if (-not $ent.id) { throw "entity_create 失败: $($ent | ConvertTo-Json -Compress)" }
  Log "实体创建 PASS: id=$($ent.id) mesh=$($meshAsset.guid) material=$matGuid"
  McpCall "mcp__engine-scene__viewport_set_camera" @{ target = @(0.0, 0.5, 0.0); yaw = 30.0; pitch = 24.0; dist = 6.5 } | Out-Null

  $frame = McpCall "mcp__engine-scene__viewport_frame" @{ width = 128; height = 96 }
  if ([int]$frame.nonZeroPixels -le 0) { throw "nonZeroPixels=0(场景不可见): $($frame | ConvertTo-Json -Compress)" }
  Log "场景可见 PASS(viewport_frame 128x96 nonZeroPixels=$($frame.nonZeroPixels) draws=$($frame.draws))"

  Log "F5 wave.3 gen UI 冒烟 PASS(G-F5-3:生成 4 候选 → 接受 1 → 材质引用 → 场景可见)"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  Remove-Item Env:FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue
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
  # 接受的 Textures/wood-<seed>.png 与 Materials/f5w3_gen_mat.rxmat 不删(留档证据,命名确定复跑幂等);
  # 清 .forge/tmp/gen/ 候选产物(gen-*)+ accept staging 命名副本(wood-*)。
  try { Remove-Item "$tmpGen\gen-*" -Force -ErrorAction SilentlyContinue } catch {}
  try { Remove-Item "$tmpGen\wood-*" -Force -ErrorAction SilentlyContinue } catch {}
}
