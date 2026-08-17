# F4 wave.1 code-forge(rx 五工具)栈级冒烟:agentd 挂载 mcp__code-forge__ 前缀 →
# code-forge-mcp 子进程包上游 rx CLI/rurixc(H:\rurix\target\debug)。
# 断言:rx_check 干净/诊断结构化 → rx_fmt --check → rx_test 通过/失败捕获 → rx_run 落盘回引。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f4-w1-rx-tools-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f4-w1-rx-tools-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function McpCall($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 8 -Compress
  try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json; charset=utf-8" -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 60
  } catch {
    $resp = $_.Exception.Response
    $errBody = ""
    if ($resp) { $sr = New-Object System.IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8); $errBody = $sr.ReadToEnd(); $sr.Close() }
    throw "$tool 失败 status=$([int]$resp.StatusCode) body=$errBody 请求体=$body"
  }
  if ($r.StatusCode -ne 200) { throw "$tool http=$($r.StatusCode)" }
  $outer = $r.Content | ConvertFrom-Json
  if ($outer.isError -eq $true) { throw "$tool 工具级 isError: $($outer.content[0].text)" }
  return $outer.content[0].text | ConvertFrom-Json
}

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$fx = "$root\tests\fixtures\f4"
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f4w1',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 1. rx_check hello.rx → diagnostics=[] ──
  Log "== rx_check hello.rx(零诊断)=="
  $c1 = McpCall "mcp__code-forge__rx_check" @{ file = "$fx\hello.rx" }
  if (@($c1.diagnostics).Count -ne 0) { throw "hello.rx 须零诊断: $($c1 | ConvertTo-Json -Compress -Depth 6)" }
  Log "rx_check hello.rx PASS(diagnostics=[])"

  # ── 2. rx_check bad.rx → 诊断非空 + code/span/message ──
  Log "== rx_check bad.rx(结构化诊断)=="
  $c2 = McpCall "mcp__code-forge__rx_check" @{ file = "$fx\bad.rx" }
  $d = @($c2.diagnostics)
  if ($d.Count -lt 1) { throw "bad.rx 须有诊断" }
  $d0 = $d[0]
  if (-not "$($d0.code)".StartsWith("RX")) { throw "诊断缺 RX 码: $($d0 | ConvertTo-Json -Compress)" }
  if ($null -eq $d0.severity -or $null -eq $d0.span.start -or -not $d0.message) { throw "诊断缺 severity/span/message: $($d0 | ConvertTo-Json -Compress)" }
  if (-not "$($d0.file)".EndsWith("bad.rx")) { throw "诊断 file 字段不实: $($d0.file)" }
  Log "rx_check bad.rx PASS(code=$($d0.code) severity=$($d0.severity) span=$($d0.span.start.line):$($d0.span.start.character) message=$($d0.message))"

  # ── 3. rx_fmt hello.rx checkOnly → 200 + needsFormat=false ──
  Log "== rx_fmt hello.rx checkOnly =="
  $f1 = McpCall "mcp__code-forge__rx_fmt" @{ file = "$fx\hello.rx"; checkOnly = $true }
  if ($f1.needsFormat -ne $false) { throw "hello.rx 须已格式化: $($f1 | ConvertTo-Json -Compress)" }
  Log "rx_fmt checkOnly PASS(needsFormat=false, formatted=[])"

  # ── 4. rx_test unit_tests.rx → passed>=1 failed=0 ──
  Log "== rx_test unit_tests.rx(全过)=="
  $t1 = McpCall "mcp__code-forge__rx_test" @{ file = "$fx\unit_tests.rx" }
  if ($t1.passed -lt 1 -or $t1.failed -ne 0) { throw "unit_tests.rx 须全过: $($t1 | ConvertTo-Json -Compress)" }
  Log "rx_test unit_tests.rx PASS(passed=$($t1.passed) failed=$($t1.failed))"

  # ── 5. rx_test unit_tests_fail.rx → failed>=1 failures 非空(失败被如实捕获)──
  Log "== rx_test unit_tests_fail.rx(失败捕获)=="
  $t2 = McpCall "mcp__code-forge__rx_test" @{ file = "$fx\unit_tests_fail.rx" }
  if ($t2.failed -lt 1) { throw "unit_tests_fail.rx 须 failed>=1: $($t2 | ConvertTo-Json -Compress)" }
  if (@($t2.failures).Count -lt 1) { throw "failures 须非空: $($t2 | ConvertTo-Json -Compress)" }
  Log "rx_test unit_tests_fail.rx PASS(failed=$($t2.failed) failure=$($t2.failures[0].name): $($t2.failures[0].detail))"

  # ── 6. rx_run hello.rx → exitCode=0 + stdoutRef 落盘存在 ──
  Log "== rx_run hello.rx(落盘回引)=="
  $r1 = McpCall "mcp__code-forge__rx_run" @{ file = "$fx\hello.rx" }
  if ($r1.exitCode -ne 0) { throw "rx_run 须 exitCode=0: $($r1 | ConvertTo-Json -Compress)" }
  if (-not (Test-Path "$root\$($r1.stdoutRef)")) { throw "stdoutRef 未落盘: $($r1.stdoutRef)" }
  $stdoutText = [IO.File]::ReadAllText("$root\$($r1.stdoutRef)")
  if (-not $stdoutText.Contains("hello, rurix")) { throw "stdoutRef 内容缺问候: $stdoutText" }
  Log "rx_run hello.rx PASS(exitCode=0 stdoutRef=$($r1.stdoutRef) 内容=$($stdoutText.Trim()))"

  Log "F4 wave.1 code-forge rx 五工具冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
