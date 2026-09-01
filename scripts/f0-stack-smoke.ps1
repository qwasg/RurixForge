# F0 栈级冒烟(G-F0-9):gateway → agentd → engine-scene-mcp → engine-host 全链路实测。
# 用法: pwsh scripts/f0-stack-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f0-stack-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
try {
  Log "== build =="
  go -C gateway-go build -o forge-gateway.exe . 2>&1 | ForEach-Object { Log $_ }
  if ($LASTEXITCODE -ne 0) { throw "go build failed" }

  $hostCountBefore = (Get-Process engine-host -ErrorAction SilentlyContinue | Measure-Object).Count
  Log "engine-host baseline processes: $hostCountBefore"

  Log "== start forge-agentd :8103 =="
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd health timeout" }
  Log "agentd /health 200"

  Log "== start forge-gateway :8102 =="
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; $health = $r.Content; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "gateway health timeout" }
  Log "gateway /health 200: $health"
  if ($health -notmatch '"agentd"\s*:\s*"ok"') { throw "gateway 聚合未报 agentd ok" }

  Log "== JWT 负向:无 token 访问 /api/forge/* =="
  try { Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/sessions" -UseBasicParsing -TimeoutSec 3 | Out-Null; throw "预期 401 未触发" } catch [System.Net.WebException] { if ($_.Exception.Response.StatusCode.value__ -ne 401) { throw } ; Log "无 JWT => 401 PASS" }

  Log "== mint HS256 JWT (node) =="
  $jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'smoke',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"
  Log "jwt minted (len=$($jwt.Length))"

  Log "== mcp__engine-scene__scene_summary 经 gateway+agentd 工具循环 =="
  $body = '{"tool":"mcp__engine-scene__scene_summary","arguments":{}}'
  $resp = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json" -Headers @{ Authorization = "Bearer $jwt" } -UseBasicParsing -TimeoutSec 30
  Log "mcp/call => $($resp.StatusCode) $($resp.Content)"
  if ($resp.StatusCode -ne 200 -or $resp.Content -notmatch 'entityCount') { throw "scene_summary 断言失败" }
  Log "scene_summary 含 entityCount PASS"

  $hostCountAfter = (Get-Process engine-host -ErrorAction SilentlyContinue | Measure-Object).Count
  Log "engine-host processes after calls: $hostCountAfter (baseline $hostCountBefore)"
  if ($hostCountAfter -gt $hostCountBefore) { throw "engine-host 孤儿泄漏: +$($hostCountAfter-$hostCountBefore)" }
  Log "无 engine-host 泄漏 PASS"

  Log "SMOKE PASS"
  exit 0
} catch {
  Log "SMOKE FAIL: $($_.Exception.Message)"
  exit 1
} finally {
  foreach ($p in $procs) { try { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } catch {} }
}
