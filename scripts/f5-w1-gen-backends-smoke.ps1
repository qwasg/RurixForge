# F5 wave.1 gen-image 后端配置门栈级冒烟(G-F5-1):agentd 第四 ServerKind mcp__gen-image__ →
# gen-image-mcp(内嵌 gend)→ local-mock 确定性 PNG / remote-openai-compatible 真实 HTTP 面。
# 断言:①无 gen-backends.json → gen_image/gen_texture_set/gen_variations 全 GEN_BACKEND_NOT_CONFIGURED,
#   gen_backends_list 两适配器 configured=false,gen_accept 不依赖后端(fixture 直入)
#   ②写入 local-mock 配置 → configured=true + capabilities 非空
#   ③gen_image n=4 seed=42 → 4 候选落 .forge/tmp/gen/;同参复跑逐字节一致(种子确定性)
#   ④gen_accept → Content/Textures/f5w1_wood.png + .meta provenance 全字段(I-7)
#   ⑤密钥红线 R-5:keystore 假密钥不进任何工具返回;remote 假端点 → 如实 GEN_BACKEND_ERROR(非伪装成功)
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f5-w1-gen-backends-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f5-w1-gen-backends-smoke-$ts.log"
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
$procs = @()
$script:startTime = Get-Date
$dataDir = "$root\data"
$genCfg = "$dataDir\gen-backends.json"
$ksFile = "$dataDir\keystore.json"
# 配置备份(空串 = 原不存在,还原 = 删除)。
$genCfgBak = if (Test-Path $genCfg) { [IO.File]::ReadAllText($genCfg) } else { $null }
$ksBak = if (Test-Path $ksFile) { [IO.File]::ReadAllText($ksFile) } else { $null }
$demo = "$root\projects\demo"
$tmpGen = "$demo\.forge\tmp\gen"
# 冒烟产物清单(finally 连带清理)。
$createdAssets = @("$demo\Content\Textures\f5w1_fixture.png", "$demo\Content\Textures\f5w1_wood.png")
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f5w1',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 1. 无 gen-backends.json:NOT_CONFIGURED 门(G-F5-1 主断言)──
  Log "== 删除 data/gen-backends.json → 全工具面 NOT_CONFIGURED 门 =="
  if (Test-Path $genCfg) { Remove-Item $genCfg -Force }
  if (Test-Path $ksFile) { Remove-Item $ksFile -Force }
  $e1 = McpCallErr "mcp__gen-image__gen_image" @{ prompt = "wood" }
  if ($e1.error -ne "GEN_BACKEND_NOT_CONFIGURED") { throw "gen_image 须 NOT_CONFIGURED: $($e1 | ConvertTo-Json -Compress)" }
  $e2 = McpCallErr "mcp__gen-image__gen_texture_set" @{ prompt = "wood"; materialKind = "pbr"; maps = @("albedo") }
  if ($e2.error -ne "GEN_BACKEND_NOT_CONFIGURED") { throw "gen_texture_set 须 NOT_CONFIGURED: $($e2 | ConvertTo-Json -Compress)" }
  $e3 = McpCallErr "mcp__gen-image__gen_variations" @{ sourceImageRef = ".forge/tmp/gen/none.png"; strength = 0.5; n = 1 }
  if ($e3.error -ne "GEN_BACKEND_NOT_CONFIGURED") { throw "gen_variations 须 NOT_CONFIGURED: $($e3 | ConvertTo-Json -Compress)" }
  Log "NOT_CONFIGURED 门 PASS(gen_image/gen_texture_set/gen_variations 三工具)"

  $bl = McpCall "mcp__gen-image__gen_backends_list" @{}
  $bs = @($bl.backends)
  if ($bs.Count -ne 2) { throw "backends 须 2 适配器: $($bl | ConvertTo-Json -Compress -Depth 6)" }
  foreach ($b in $bs) { if ($b.configured -ne $false) { throw "无配置时 $($b.id) 须 configured=false" } }
  Log "gen_backends_list PASS(两适配器 configured=false:id=$($bs[0].id),$($bs[1].id))"

  # gen_accept 不依赖后端:fixture png 预放 .forge/tmp/gen/ 直入管线。
  New-Item -ItemType Directory -Force $tmpGen | Out-Null
  Copy-Item "$demo\Content\Textures\f2w4_dot.png" "$tmpGen\f5w1_fixture.png" -Force
  $acc0 = McpCall "mcp__gen-image__gen_accept" @{ imageFileRef = ".forge/tmp/gen/f5w1_fixture.png"; destFolder = "Textures"; name = "f5w1_fixture" }
  if ($acc0.assetPath -ne "Textures/f5w1_fixture.png") { throw "fixture accept assetPath 异常: $($acc0 | ConvertTo-Json -Compress)" }
  if (-not $acc0.guid) { throw "fixture accept 缺 guid" }
  if (-not (Test-Path "$demo\Content\Textures\f5w1_fixture.png")) { throw "fixture 未落 Content/Textures" }
  Log "gen_accept(无后端)PASS(assetPath=$($acc0.assetPath) guid=$($acc0.guid))"

  # ── 2. 写入 local-mock 配置 → configured=true ──
  Log "== 写 gen-backends.json(local-mock enabled)=="
  WriteJsonNoBom $genCfg '{"backends":[{"id":"local-mock","kind":"local","enabled":true}]}'
  $bl2 = McpCall "mcp__gen-image__gen_backends_list" @{}
  $lm = @($bl2.backends) | Where-Object { $_.id -eq "local-mock" }
  if ($lm.configured -ne $true) { throw "local-mock 须 configured=true: $($bl2 | ConvertTo-Json -Compress -Depth 6)" }
  $caps = $lm.capabilities
  if (-not $caps -or @($caps.kinds).Count -lt 1) { throw "local-mock capabilities 须非空: $($lm | ConvertTo-Json -Compress -Depth 6)" }
  $rm0 = @($bl2.backends) | Where-Object { $_.id -eq "remote-openai-compatible" }
  if ($rm0.configured -ne $false) { throw "remote 无端点/key 须 configured=false" }
  Log "local-mock configured=true PASS(kinds=$($caps.kinds -join '/') sizes=$($caps.sizes -join ','))"

  # ── 3. gen_image n=4 seed=42 → 4 候选;同参复跑逐字节一致 ──
  Log "== gen_image 木纹 n=4 size=512 seed=42(确定性)=="
  $giArgs = @{ prompt = "wood grain 木纹"; n = 4; size = 512; seed = 42 }
  $g1 = McpCall "mcp__gen-image__gen_image" $giArgs
  $c1 = @($g1.candidates)
  if ($c1.Count -ne 4) { throw "须 4 候选: $($g1 | ConvertTo-Json -Compress -Depth 6)" }
  $hashes1 = @()
  foreach ($c in $c1) {
    $fp = "$demo\$($c.imageFileRef -replace '/', '\')"
    if (-not (Test-Path $fp)) { throw "候选未落盘: $($c.imageFileRef)" }
    if ($c.backendId -ne "local-mock") { throw "backendId 须 local-mock: $($c | ConvertTo-Json -Compress)" }
    $hashes1 += (Get-FileHash $fp -Algorithm SHA256).Hash
  }
  $g2 = McpCall "mcp__gen-image__gen_image" $giArgs
  $c2 = @($g2.candidates)
  if ($c2.Count -ne 4) { throw "复跑须 4 候选" }
  for ($i = 0; $i -lt 4; $i++) {
    $fp2 = "$demo\$($c2[$i].imageFileRef -replace '/', '\')"
    if (-not (Test-Path $fp2)) { throw "复跑候选未落盘: $($c2[$i].imageFileRef)" }
    $h2 = (Get-FileHash $fp2 -Algorithm SHA256).Hash
    if ($h2 -ne $hashes1[$i]) { throw "候选 $i 复跑字节不一致(hash $($hashes1[$i]) vs $h2)" }
    if ($c2[$i].seed -ne $c1[$i].seed) { throw "候选 $i seed 不一致" }
  }
  Log "gen_image PASS(4 候选 seed=$($c1[0].seed)..$($c1[3].seed);复跑逐字节一致)"

  # ── 4. gen_accept 第 1 候选 → Content/Textures + provenance 全字段(I-7)──
  Log "== gen_accept → Textures/f5w1_wood =="
  $acc = McpCall "mcp__gen-image__gen_accept" @{ imageFileRef = $c1[0].imageFileRef; destFolder = "Textures"; name = "f5w1_wood" }
  if ($acc.assetPath -ne "Textures/f5w1_wood.png") { throw "assetPath 须 Textures/f5w1_wood.png: $($acc | ConvertTo-Json -Compress)" }
  if (-not $acc.guid) { throw "缺 guid" }
  $pngPath = "$demo\Content\Textures\f5w1_wood.png"
  if (-not (Test-Path $pngPath)) { throw "资产未落 Content/Textures/f5w1_wood.png" }
  $metaText = [IO.File]::ReadAllText("$pngPath.meta", [Text.Encoding]::UTF8)
  if ($metaText -notmatch "origin:\s*gen-image") { throw ".meta provenance.origin 须 gen-image: $metaText" }
  if ($metaText -notmatch "backendId:\s*local-mock") { throw "detail.backendId 须 local-mock: $metaText" }
  if (-not $metaText.Contains("木纹")) { throw "detail.prompt 须含木纹: $metaText" }
  if ($metaText -notmatch "seed:\s*42") { throw "detail.seed 须 42: $metaText" }
  if ($metaText -notmatch "generatedAt:\s*\S") { throw "detail.generatedAt 须非空: $metaText" }
  Log "gen_accept PASS(assetPath=$($acc.assetPath);provenance origin/backendId/prompt/seed/generatedAt 全字段)"

  # ── 5. 密钥红线 R-5 + remote 假端点如实报错 ──
  Log "== remote 条目 + keystore 假密钥(红线扫描)=="
  WriteJsonNoBom $genCfg '{"backends":[{"id":"local-mock","kind":"local","enabled":true},{"id":"remote-openai-compatible","kind":"remote","enabled":true,"endpoint":"http://127.0.0.1:1"}]}'
  WriteJsonNoBom $ksFile '{"keys":{"remote-openai-compatible":"sk-test-SECRET-12345"}}'
  $listRaw = McpCallRaw "mcp__gen-image__gen_backends_list" @{}
  if ($listRaw.Contains("sk-test")) { throw "R-5 红线:gen_backends_list 返回含密钥子串" }
  $bl3 = $listRaw | ConvertFrom-Json
  $rm = @(($bl3.content[0].text | ConvertFrom-Json).backends) | Where-Object { $_.id -eq "remote-openai-compatible" }
  if ($rm.configured -ne $true) { throw "remote(endpoint+key)须 configured=true: $listRaw" }
  Log "gen_backends_list 红线 PASS(remote configured=true;响应无密钥子串)"

  $errRaw = McpCallRaw "mcp__gen-image__gen_image" @{ prompt = "x"; backend = "remote-openai-compatible" }
  if ($errRaw.Contains("sk-test")) { throw "R-5 红线:gen_image 错误返回含密钥子串" }
  $errOuter = $errRaw | ConvertFrom-Json
  if ($errOuter.isError -ne $true) { throw "假端点须 isError(不得伪装成功): $errRaw" }
  $errBody = $errOuter.content[0].text | ConvertFrom-Json
  if (@("GEN_BACKEND_ERROR", "GEN_RATE_LIMITED", "GEN_BACKEND_NOT_CONFIGURED") -notcontains $errBody.error) {
    throw "假端点须 GEN_BACKEND_ERROR 族: $($errBody | ConvertTo-Json -Compress)"
  }
  Log "remote 假端点 PASS(如实 $($errBody.error):连接拒绝不伪装成功;错误无密钥)"

  Log "F5 wave.1 gen-image 后端配置门冒烟 PASS(G-F5-1)"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  # 连带清杀本冒烟派生的 MCP/host 子孙进程(孤儿继承 stdout 句柄会挂住调用方管道,实测坑)。
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-scene-mcp', 'code-forge-mcp', 'asset-pipeline-mcp', 'gen-image-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
  # 还原 gen 配置与 keystore(备份为 null = 原不存在 → 删除)。
  try {
    if ($null -ne $genCfgBak) { WriteJsonNoBom $genCfg $genCfgBak } elseif (Test-Path $genCfg) { Remove-Item $genCfg -Force }
    if ($null -ne $ksBak) { WriteJsonNoBom $ksFile $ksBak } elseif (Test-Path $ksFile) { Remove-Item $ksFile -Force }
  } catch { Log "配置还原异常: $_" }
  # 清理冒烟资产与临时产物(.forge/tmp/gen 已被 .gitignore 覆盖,仍清)。
  foreach ($a in $createdAssets) { try { Remove-Item $a -Force -ErrorAction SilentlyContinue; Remove-Item "$a.meta" -Force -ErrorAction SilentlyContinue } catch {} }
  try { Remove-Item "$tmpGen\f5w1_*" -Force -ErrorAction SilentlyContinue } catch {}
  try { Remove-Item "$tmpGen\gen-*" -Force -ErrorAction SilentlyContinue } catch {}
}
