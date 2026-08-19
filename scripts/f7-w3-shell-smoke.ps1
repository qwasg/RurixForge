# F7 wave.3 前端主题与壳冒烟(G-F7-3):desktop shell 场景。
# 断言(main.cjs scenario=shell 内逐条 throw 把关):
#   ① 三栏+titlebar+statusbar+分隔条 DOM 存在
#   ② setMode('light') → data-theme=light + --accent=#C96442 + --bg=#FAF9F5(getComputedStyle 实测)
#   ③ setMode('dark') → data-theme=dark + --accent=#E2886A + --bg=#1C1B18
#   ④ 开编辑器 tab → EditorView 骨架(Hierarchy)+ ViewportCanvas 出帧或如实 DEV_ENV_DEGRADE 文本
#   ⑤ New Agent 经真后端(agentd 前置,POST /sessions 经 host 3080)→ 侧栏出现会话行
# 前置:agentd 本脚本自带起(会话 REST 面);host 由 desktop 主进程 spawn。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f7-w3-shell-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$logFile = "evidence\f7-w3-shell-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$script:startTime = Get-Date

function Test-PortFree($port) {
  try {
    $l = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, $port)
    $l.Start(); $l.Stop(); return $true
  } catch { return $false }
}

try {
  if (-not (Test-PortFree 8103)) { throw "8103 被占用(疑有运行中的 agentd),请先关闭再跑冒烟" }
  if (-not (Test-PortFree 3080)) { throw "3080 被占用(疑有运行中的 host),请先关闭再跑冒烟" }

  Log "== 构建 forge-agentd / engine 二进制(编辑器嵌入腿需要 engine MCP 面) =="
  $ErrorActionPreference = 'Continue'
  cargo build -p forge-agentd -p engine-scene-mcp -p engine-host 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  cargo: $_" }
  $cargoExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($cargoExit -ne 0) { throw "cargo build 失败(exit=$cargoExit)" }

  Log "== 启动 agentd(8103,真后端:会话 REST + MCP 面) =="
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪"

  Log "== 构建 client(新壳产物 = host 静态目录) + host =="
  $ErrorActionPreference = 'Continue'
  pnpm --filter @forge/client build 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  pnpm: $_" }
  $clientExit = $LASTEXITCODE
  pnpm --filter @forge/host build 2>&1 | Select-Object -Last 2 | ForEach-Object { Log "  pnpm: $_" }
  $hostExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($clientExit -ne 0) { throw "client build 失败(exit=$clientExit)" }
  if ($hostExit -ne 0) { throw "host build 失败(exit=$hostExit)" }

  $env:FORGE_SMOKE_SCENARIO = 'shell'
  Log "desktop smoke(scenario=shell)..."
  $ErrorActionPreference = 'Continue'
  $out = & pnpm --filter @forge/desktop smoke 2>&1 | Out-String
  $code = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  $out | Add-Content $logFile
  if ($code -ne 0) { throw "desktop smoke exit=$code`n$out" }
  $passLine = ($out -split "`n" | Select-String -Pattern '\[smoke\] PASS').Line
  Log "desktop smoke: $passLine"
  # 子进程冒烟日志关键行复读(逐项断言留痕)
  $child = Get-Content apps\desktop\evidence\smoke.log -Tail 60 -ErrorAction SilentlyContinue
  foreach ($l in $child) { if ($l -match 'scenario=shell') { Log "  $l" } }
  Log "F7 wave.3 前端主题与壳冒烟 PASS(G-F7-3)"
  exit 0
} catch {
  Log "FAIL: $_"
  Log "F7 wave.3 前端主题与壳冒烟 FAIL"
  exit 1
} finally {
  Remove-Item Env:\FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
  # 连带清杀:按启动时间过滤(engine MCP/host 孤儿会继承 stdout 句柄假死管道)
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('forge-agentd','engine-scene-mcp','engine-host','code-forge-mcp','asset-pipeline-mcp','gen-image-mcp','gen-model-mcp') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
  Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -match 'packages[\\/]host[\\/]dist[\\/]index\.js' } |
    ForEach-Object { try { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } catch {} }
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match 'electron' -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
}
