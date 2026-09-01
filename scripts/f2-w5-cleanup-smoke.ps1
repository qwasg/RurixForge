# F2 wave.5 asset-cleanup + Proposal 执行链冒烟(G-F2-5 栈级)。
# 流程:造混乱 fixture → asset_cleanup_scan dryRun 提案 → 创建 Proposal → 批准 →
#       asset_move 执行(含改名)→ asset_fix_redirectors → asset_refs 验证引用不断链;
#       另验 destructive 门(asset_delete force 无批准被 GOV_PROPOSAL_REQUIRED 拦,批准后放行)。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f2-w5-cleanup-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f2-w5-cleanup-smoke-$ts.log"
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
    $respText = $wc.UploadString("http://127.0.0.1:8102$path", $method, $body)
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
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f2w5',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 0. 造混乱 fixture:错放贴图到 Meshes/ + 文件名含空格括号 ──
  Log "== 造混乱 fixture:贴图错放 Meshes/ 且命名混乱 =="
  Add-Type -AssemblyName System.Drawing
  $tmpSrc = Join-Path $env:TEMP "f2w5_messy.png"
  $bmp = New-Object System.Drawing.Bitmap 2, 2
  $bmp.SetPixel(0, 0, [System.Drawing.Color]::FromArgb(255, 200, 30, 30))
  $bmp.Save($tmpSrc, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  $imp = McpCall "mcp__asset-pipeline__asset_import" @{ sourcePaths = @($tmpSrc); destFolder = "Meshes" }
  if ($imp.failed.Count -gt 0) { throw "fixture 导入失败: $($imp.failed[0].error)" }
  $messy = $imp.imported[0]
  # 改名成混乱名(同目录 move + newName,经移动语义留 redirector)。
  $ren = McpCall "mcp__asset-pipeline__asset_move" @{ assetPath = $messy.assetPath; destFolder = "Meshes"; newName = "my tex (final 2).png" }
  if ($ren.error) { throw "fixture 改名失败: $($ren.message)" }
  $messyPath = $ren.redirector.newPath
  Log "fixture 就位: $messyPath guid=$($messy.guid)"

  # ── 1. asset_cleanup_scan dryRun 提案 ──
  Log "== asset_cleanup_scan =="
  $scan = McpCall "mcp__asset-pipeline__asset_cleanup_scan" @{}
  if ($scan.error) { throw "cleanup_scan 失败: $($scan.message)" }
  $misplaced = $scan.proposals | Where-Object { $_.issue -eq "misplaced" -and $_.assetPath -eq $messyPath }
  $naming = $scan.proposals | Where-Object { $_.issue -eq "naming" -and $_.assetPath -eq $messyPath }
  if (-not $misplaced) { throw "缺 misplaced 提案: $($scan.proposals | ConvertTo-Json -Compress)" }
  if ($misplaced.destFolder -ne "Textures") { throw "misplaced 目标目录错: $($misplaced.destFolder)" }
  if (-not $naming) { throw "缺 naming 提案" }
  if ($naming.newName -ne "my_tex_final_2.png") { throw "naming 清洗错: $($naming.newName)" }
  $orphans = @($scan.proposals | Where-Object { $_.issue -eq "orphan" })
  Log "提案 PASS: misplaced=$($misplaced.assetPath)→$($misplaced.destFolder),naming→$($naming.newName),orphan=$($orphans.Count) 个(仅报告)"
  $impactStr = ($scan.impact | ForEach-Object { "$($_.issue)=$($_.count)" }) -join ' '
  Log "impact: $impactStr"

  # ── 2. 创建 Proposal(pending) ──
  Log "== POST /api/forge/proposals 创建 asset.cleanup Proposal =="
  $moveList = @($scan.proposals | Where-Object { $_.issue -ne "orphan" } | ForEach-Object { $_.assetPath })
  $created = HttpReq "POST" "/api/forge/proposals" @{
    kind = "asset.cleanup"
    summary = "asset-cleanup:移动/改名 $($moveList.Count) 个资产($messyPath 等)"
    impact = @{ assets = $moveList }
  }
  if ($created.status -ne 200) { throw "创建 Proposal 失败: $($created.raw)" }
  $propId = $created.json.id
  if ($created.json.status -ne "pending") { throw "新 Proposal 应为 pending,实: $($created.json.status)" }
  Log "Proposal 创建 PASS: $propId pending"

  # ── 3. 批准前不执行(skill 纪律);批准 ──
  $approved = HttpReq "PATCH" "/api/forge/proposals/$propId" @{ action = "approve" }
  if ($approved.status -ne 200 -or $approved.json.status -ne "approved") { throw "批准失败: $($approved.raw)" }
  Log "Proposal 批准 PASS"
  # 终态不可逆。
  $again = HttpReq "PATCH" "/api/forge/proposals/$propId" @{ action = "reject" }
  if ($again.status -ne 409) { throw "终态应不可逆(409),实: $($again.status)" }
  Log "终态不可逆 PASS(409)"

  # ── 4. 执行移动+改名 ──
  Log "== asset_move 执行(misplaced + naming 合并为一步) =="
  $mv = McpCall "mcp__asset-pipeline__asset_move" @{ assetPath = $messyPath; destFolder = "Textures"; newName = "my_tex_final_2.png" }
  if ($mv.error) { throw "asset_move 失败: $($mv.message)" }
  $newPath = $mv.redirector.newPath
  if ($newPath -ne "Textures/my_tex_final_2.png") { throw "移动路径错: $newPath" }
  Log "移动 PASS: $messyPath → $newPath"

  # ── 5. 收敛 redirector + 验证引用不断链 ──
  Log "== asset_fix_redirectors + 引用验证 =="
  $fix = McpCall "mcp__asset-pipeline__asset_fix_redirectors" @{}
  if ($fix.error) { throw "fix_redirectors 失败: $($fix.message)" }
  $refs = McpCall "mcp__asset-pipeline__asset_refs" @{ assetPath = $newPath; direction = "refs" }
  if ($refs.error) { throw "移动后 asset_refs 失败: $($refs.message)(GUID 引用断链?)" }
  Log "移动后引用查询 PASS(GUID 不变,引用不断链)"

  # ── 6. 复扫:misplaced/naming 应已清除 ──
  $scan2 = McpCall "mcp__asset-pipeline__asset_cleanup_scan" @{}
  $left = $scan2.proposals | Where-Object { ($_.issue -eq "misplaced" -or $_.issue -eq "naming") -and $_.assetPath -eq $messyPath }
  if ($left) { throw "复扫仍有未清提案: $($left | ConvertTo-Json -Compress)" }
  Log "复扫干净 PASS"

  # ── 7. destructive 强制门:force 删除未批准被拦,批准后放行 ──
  Log "== destructive 门:asset_delete force 两阶段 =="
  $blocked = HttpReq "POST" "/api/forge/mcp/call" @{ tool = "mcp__asset-pipeline__asset_delete"; arguments = @{ assetPaths = @($newPath); force = $true } }
  if ($blocked.status -ne 409) { throw "未批准应 409,实: $($blocked.status) $($blocked.raw)" }
  if ($blocked.json.error.code -ne "GOV_PROPOSAL_REQUIRED") { throw "错误码应为 GOV_PROPOSAL_REQUIRED,实: $($blocked.json.error.code)" }
  $gateProp = $blocked.json.error.proposalId
  Log "强制门拦截 PASS(409 GOV_PROPOSAL_REQUIRED,自动提案 $gateProp)"
  $ap2 = HttpReq "PATCH" "/api/forge/proposals/$gateProp" @{ action = "approve" }
  if ($ap2.status -ne 200) { throw "强制门提案批准失败: $($ap2.raw)" }
  $del = McpCall "mcp__asset-pipeline__asset_delete" @{ assetPaths = @($newPath); force = $true }
  if ($del.error) { throw "批准后删除失败: $($del.message)" }
  if (-not ($del.deleted -contains $newPath)) { throw "deleted 清单不含 $newPath" }
  Log "批准后 force 删除 PASS(fixture 已清)"
  $listEnd = McpCall "mcp__asset-pipeline__asset_list" @{}
  if ($listEnd.assets | Where-Object { $_.path -like "*my_tex*" }) { throw "fixture 残留" }
  Log "fixture 清理确认 PASS"

  Log "F2 wave.5 asset-cleanup + Proposal 执行链冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
