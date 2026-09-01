# F4 wave.4 栈级冒烟:code-forge code_* 三工具(code_symbol_search / code_references /
# code_structured_edit)—— rurixc --tooling-server 常驻 LSP 会话 + 文本符号表。
# 链路:HttpReq → gateway(8102,JWT)→ forge-agentd(8103)→ code-forge-mcp
#   (env FORGE_CODE_FORGE_PROJECT 指向临时项目,fixture mathlib.rx 复制入内,完事清理)。
# fixture 单文件双 fn 的诚实注记见 tests/fixtures/f4/lsp/mathlib.rx 头注释
# (上游 ToolingSession 单文档语义,references 限同文件)。
# 断言:①symbol_search "smooth" → symbols 非空(name/kind/file/span 齐)
#   ②references smooth_open → refs 含定义行与调用行 ③structured_edit span replace 41→7
#   → applied:true + newDiagnostics=[] + 读回断言 ④改回 7→41(还原)
#   ⑤越界 span → applied:false SPAN_OUT_OF_RANGE 不写盘
#   ⑥references 不存在符号 → isError SYMBOL_NOT_FOUND;edit symbolQuery 不存在 → applied:false。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f4-w4-lsp-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f4-w4-lsp-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
# 期望成功的 MCP 调用:HTTP 非 200 或 isError → throw;返回 content[0].text 解析后 JSON。
function McpCall($tool, $arguments) {
  $outer = McpCallRaw $tool $arguments
  if ($outer.isError -eq $true) { throw "$tool 工具级 isError: $($outer.content[0].text)" }
  return $outer.content[0].text | ConvertFrom-Json
}
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
  return $r.Content | ConvertFrom-Json
}
# 在临时项目 mathlib.rx 内定位 "smooth_open(<lit>)" 调用点的字面量 span(0 基 line/character)。
function CallLiteralSpan($projFile, $lit) {
  $lines = [IO.File]::ReadAllLines($projFile)
  for ($i = 0; $i -lt $lines.Count; $i++) {
    $needle = "smooth_open($lit)"
    $idx = $lines[$i].IndexOf($needle)
    if ($idx -ge 0) {
      $col = $idx + "smooth_open(".Length
      return @{ start = @{ line = $i; character = $col }; end = @{ line = $i; character = $col + $lit.Length } }
    }
  }
  throw "未找到调用字面量 smooth_open($lit)"
}

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$proj = "$root\data\f4w4-lsp-project"
try {
  # 临时项目:复制 fixture(Copy-Item 保字节,不带 BOM;PS 5.1 Set-Content utf8 会加 BOM 被 .rx lexer 拒)。
  if (Test-Path $proj) { Remove-Item $proj -Recurse -Force }
  New-Item -ItemType Directory -Force $proj | Out-Null
  Copy-Item "$root\tests\fixtures\f4\lsp\mathlib.rx" "$proj\mathlib.rx"
  $projFile = "$proj\mathlib.rx"
  # agentd 无参 spawn code-forge-mcp → 项目根经 env 兜底指向临时项目(子进程继承环境)。
  $env:FORGE_CODE_FORGE_PROJECT = $proj
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪(FORGE_CODE_FORGE_PROJECT=$proj)"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f4w4',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 1. code_symbol_search {query:"smooth"} → symbols 非空,四字段齐 ──
  Log "== code_symbol_search smooth =="
  $s1 = McpCall "mcp__code-forge__code_symbol_search" @{ query = "smooth" }
  $syms = @($s1.symbols)
  if ($syms.Count -lt 1) { throw "symbol_search 须非空: $($s1 | ConvertTo-Json -Compress -Depth 6)" }
  $sym = $syms | Where-Object { $_.name -eq "smooth_open" } | Select-Object -First 1
  if ($null -eq $sym) { throw "缺 smooth_open: $($s1 | ConvertTo-Json -Compress -Depth 6)" }
  if ($sym.kind -ne "fn") { throw "kind 须为 fn: $($sym | ConvertTo-Json -Compress)" }
  if (-not "$($sym.file)".EndsWith("mathlib.rx")) { throw "file 不实: $($sym.file)" }
  if ($null -eq $sym.span.start.line -or $sym.span.start.line -ne 3) { throw "span 须为 line 3(定义行): $($sym | ConvertTo-Json -Compress)" }
  Log "code_symbol_search PASS(name=$($sym.name) kind=$($sym.kind) file=$($sym.file) span=$($sym.span.start.line):$($sym.span.start.character))"

  # ── 2. code_references {symbolQuery:"smooth_open"} → refs 含定义行 3 + 调用行 ──
  Log "== code_references smooth_open =="
  $r1 = McpCall "mcp__code-forge__code_references" @{ symbolQuery = "smooth_open" }
  $refs = @($r1.refs)
  if ($refs.Count -lt 2) { throw "refs 须 >=2(定义+调用): $($r1 | ConvertTo-Json -Compress -Depth 6)" }
  $refLines = @($refs | ForEach-Object { [int]$_.span.start.line })
  $callSpan = CallLiteralSpan $projFile "41"
  $callLine = [int]$callSpan.start.line
  if ($refLines -notcontains 3) { throw "refs 缺定义行 3: lines=$($refLines -join ',')" }
  if ($refLines -notcontains $callLine) { throw "refs 缺调用行 ${callLine}: lines=$($refLines -join ',')" }
  Log "code_references PASS(refs=$($refs.Count) lines=$($refLines -join ',') 定义=3 调用=$callLine)"

  # ── 3. code_structured_edit span replace 41→7 → applied:true + newDiagnostics=[] + 读回 ──
  Log "== code_structured_edit 41→7 =="
  $e1 = McpCall "mcp__code-forge__code_structured_edit" @{ file = "mathlib.rx"; edits = @(@{ kind = "replace"; span = $callSpan; content = "7" }) }
  if ($e1.applied -ne $true) { throw "edit 须 applied:true: $($e1 | ConvertTo-Json -Compress -Depth 6)" }
  if ($null -eq $e1.newDiagnostics) { throw "newDiagnostics 须非 null(diagError 遮蔽?): $($e1 | ConvertTo-Json -Compress -Depth 6)" }
  if (@($e1.newDiagnostics).Count -ne 0) { throw "newDiagnostics 须为空: $($e1 | ConvertTo-Json -Compress -Depth 6)" }
  $after1 = [IO.File]::ReadAllText($projFile)
  if (-not $after1.Contains("smooth_open(7)")) { throw "读回缺 smooth_open(7): $after1" }
  Log "code_structured_edit PASS(applied=true newDiagnostics=[] 41→7 已读回)"

  # ── 4. 改回 7→41(还原 fixture 副本)──
  Log "== code_structured_edit 7→41(还原)=="
  $backSpan = CallLiteralSpan $projFile "7"
  $e2 = McpCall "mcp__code-forge__code_structured_edit" @{ file = "mathlib.rx"; edits = @(@{ kind = "replace"; span = $backSpan; content = "41" }) }
  if ($e2.applied -ne $true -or @($e2.newDiagnostics).Count -ne 0) { throw "还原 edit 异常: $($e2 | ConvertTo-Json -Compress -Depth 6)" }
  $after2 = [IO.File]::ReadAllText($projFile)
  if (-not $after2.Contains("smooth_open(41)")) { throw "还原读回缺 smooth_open(41): $after2" }
  Log "还原 PASS(7→41 已读回)"

  # ── 5. 越界 span → applied:false SPAN_OUT_OF_RANGE,不写盘 ──
  Log "== code_structured_edit 越界拒绝 =="
  $e3 = McpCall "mcp__code-forge__code_structured_edit" @{ file = "mathlib.rx"; edits = @(@{ kind = "replace"; span = @{ start = @{ line = 999; character = 0 }; end = @{ line = 999; character = 1 } }; content = "x" }) }
  if ($e3.applied -ne $false) { throw "越界须 applied:false: $($e3 | ConvertTo-Json -Compress -Depth 6)" }
  if ($e3.error.code -ne "SPAN_OUT_OF_RANGE") { throw "error.code 须 SPAN_OUT_OF_RANGE: $($e3 | ConvertTo-Json -Compress -Depth 6)" }
  $after3 = [IO.File]::ReadAllText($projFile)
  if ($after3 -ne $after2) { throw "失败 edit 不得写盘" }
  Log "越界拒绝 PASS(applied=false SPAN_OUT_OF_RANGE 未写盘)"

  # ── 6. 负例:references 不存在符号 → isError SYMBOL_NOT_FOUND;edit symbolQuery 不存在 → applied:false ──
  Log "== 负例 SYMBOL_NOT_FOUND =="
  $raw = McpCallRaw "mcp__code-forge__code_references" @{ symbolQuery = "ghost_no_such_fn" }
  if ($raw.isError -ne $true) { throw "references ghost 须 isError: $($raw | ConvertTo-Json -Compress -Depth 6)" }
  if (-not "$($raw.content[0].text)".Contains("SYMBOL_NOT_FOUND")) { throw "缺 SYMBOL_NOT_FOUND: $($raw.content[0].text)" }
  $e4 = McpCall "mcp__code-forge__code_structured_edit" @{ file = "mathlib.rx"; edits = @(@{ kind = "replace"; symbolQuery = "ghost_no_such_fn"; content = "x" }) }
  if ($e4.applied -ne $false -or $e4.error.code -ne "SYMBOL_NOT_FOUND") { throw "edit ghost 须 applied:false SYMBOL_NOT_FOUND: $($e4 | ConvertTo-Json -Compress -Depth 6)" }
  Log "负例 PASS(references isError SYMBOL_NOT_FOUND;edit applied:false SYMBOL_NOT_FOUND)"

  Log "F4 wave.4 code-forge code_* 三工具冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Remove-Item Env:FORGE_CODE_FORGE_PROJECT -ErrorAction SilentlyContinue
  try { if (Test-Path $proj) { Remove-Item $proj -Recurse -Force } } catch {}
}
