# F3 wave.2 subagent profile + skills 管理 API 冒烟(G-F3-2 栈级)。
# 流程:/api/forge/subagents 五 profile 校验 → 热加载(改文件不重启即反映)→
#       /api/forge/skills/{name} 全文逐字节一致 → config/write 禁用/还原 → list 反映 enabled。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f3-w2-subagent-skills-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f3-w2-subagent-skills-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function HttpReq($method, $path, $obj) {
  $body = if ($obj) { $obj | ConvertTo-Json -Depth 10 -Compress } else { "" }
  $wc = New-Object System.Net.WebClient
  $wc.Encoding = [Text.Encoding]::UTF8
  $wc.Headers.Add("Content-Type", "application/json; charset=utf-8")
  $wc.Headers.Add("Authorization", "Bearer $script:jwt")
  try {
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
$cfgPath = "data\skills-config.json"
$cfgBackup = if (Test-Path $cfgPath) { Get-Content $cfgPath -Raw } else { $null }
$hotFile = "data\agents\zz-smoke-hot.md"
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f3w2',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 1. /api/forge/subagents 五 profile ──
  Log "== GET /api/forge/subagents =="
  $r = HttpReq "GET" "/api/forge/subagents" $null
  if ($r.status -ne 200) { throw "subagents 失败: $($r.raw)" }
  if ($r.json.errors.Count -gt 0) { throw "profile 解析错误: $($r.json.errors -join ';')" }
  $subs = @($r.json.subagents)
  if ($subs.Count -ne 5) { throw "profile 数≠5: $($subs.Count)" }
  foreach ($n in @("scene-builder","asset-wrangler","logic-programmer","qa-tester","material-smith")) {
    $p = $subs | Where-Object { $_.name -eq $n }
    if (-not $p) { throw "缺 profile $n" }
    if ($p.tools.Count -lt 2) { throw "$n tools 过少" }
    if (-not $p.prompt.Contains("必须遵守")) { throw "$n 缺 system prompt 正文" }
  }
  $lp = $subs | Where-Object { $_.name -eq "logic-programmer" }
  if (-not ($lp.tools -contains "mcp__engine-scene__component.*")) { throw "logic-programmer 缺 component.* 族白名单" }
  Log "五 profile PASS(scene-builder/asset-wrangler/logic-programmer/qa-tester/material-smith)"

  # ── 2. 热加载:新 profile 落盘即现;改 maxSteps 不重启反映 ──
  Log "== 热加载:新增 + 修改不重启 =="
  $hotBody = @"
---
name: zz-smoke-hot
description: 冒烟热加载临时 profile
tools: ["read_file"]
model: default
maxSteps: 7
---
临时 profile,冒烟后删除。
"@
  [IO.File]::WriteAllText("$root\$hotFile", $hotBody)
  $h1 = HttpReq "GET" "/api/forge/subagents" $null
  $p1 = $h1.json.subagents | Where-Object { $_.name -eq "zz-smoke-hot" }
  if (-not $p1) { throw "新 profile 未即现(热加载失效)" }
  if ($p1.maxSteps -ne 7) { throw "maxSteps≠7: $($p1.maxSteps)" }
  [IO.File]::WriteAllText("$root\$hotFile", ($hotBody -replace "maxSteps: 7", "maxSteps: 42"))
  $h2 = HttpReq "GET" "/api/forge/subagents" $null
  $p2 = $h2.json.subagents | Where-Object { $_.name -eq "zz-smoke-hot" }
  if ($p2.maxSteps -ne 42) { throw "修改未反映(热加载失效): $($p2.maxSteps)" }
  Remove-Item $hotFile -Force
  Log "热加载 PASS(落盘即现,改 maxSteps 7→42 不重启反映)"

  # ── 3. /api/forge/skills/{name} 全文逐字节一致 ──
  Log "== GET /api/forge/skills/asset-cleanup 全文 =="
  $sk = HttpReq "GET" "/api/forge/skills/asset-cleanup" $null
  if ($sk.status -ne 200) { throw "skills/{name} 失败: $($sk.raw)" }
  $disk = [IO.File]::ReadAllText("$root\skills\asset-cleanup\SKILL.md")
  if ($sk.json.content -ne $disk) { throw "返回全文与磁盘不一致" }
  $nf = HttpReq "GET" "/api/forge/skills/no-such-skill" $null
  if ($nf.status -ne 404) { throw "不存在 skill 应 404,实: $($nf.status)" }
  Log "skills/{name} PASS(逐字节一致 + 404)"

  # ── 4. config/write:禁用 → list 反映 → 还原 ──
  Log "== skills/config/write 禁用/还原 =="
  $w = HttpReq "POST" "/api/forge/skills/config/write" @{ disabled = @("asset-cleanup") }
  if ($w.status -ne 200 -or -not $w.json.written) { throw "config/write 失败: $($w.raw)" }
  $l1 = HttpReq "GET" "/api/forge/skills/list" $null
  $sc1 = $l1.json.skills | Where-Object { $_.name -eq "asset-cleanup" }
  if ($sc1.enabled -ne $false) { throw "禁用未反映: $($l1.json.skills | ConvertTo-Json -Compress)" }
  $w2 = HttpReq "POST" "/api/forge/skills/config/write" @{ disabled = @() }
  if ($w2.status -ne 200) { throw "还原失败: $($w2.raw)" }
  $l2 = HttpReq "GET" "/api/forge/skills/list" $null
  $sc2 = $l2.json.skills | Where-Object { $_.name -eq "asset-cleanup" }
  if ($sc2.enabled -ne $true) { throw "还原未反映" }
  $bad = HttpReq "POST" "/api/forge/skills/config/write" @{ disabled = @("Bad_Name") }
  if ($bad.status -ne 400) { throw "非法名应 400,实: $($bad.status)" }
  Log "config/write PASS(禁用→enabled=false→还原→true;非法名 400)"

  Log "F3 wave.2 subagent + skills 管理冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  if (Test-Path $hotFile) { Remove-Item $hotFile -Force -ErrorAction SilentlyContinue }
  if ($null -ne $cfgBackup) { [IO.File]::WriteAllText("$root\$cfgPath", $cfgBackup) } elseif (Test-Path $cfgPath) { Remove-Item $cfgPath -Force -ErrorAction SilentlyContinue }
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
