# F7 wave.5 workbench 与设置冒烟(G-F7-5):desktop settings + workbench 双场景。
# 场景断言(main.cjs 内逐条 throw 把关):
#   settings  = 开设置(外观页 overlay+五页导航)→ 预设 github 亮表 --accent 实测 rgb(9,105,218)
#               → 模式三卡切深色实测 rgb(68,147,248) → 技能页禁用 asset-cleanup 写回
#               (GET /skills/list 复核 enabled=false)→ 复原(enabled=true)
#   workbench = 建会话 → 造两待办(一完成)→ todo tab 四列分列实测 → 提案 tab pending 行
#               「批准」→ approved → 底部面板 Agent Logs/Output/Metrics 真实派生渲染
#               → Inspector 仓根 crates 展开见 forge-agentd(懒加载)
# 前置:agentd 本脚本自带起(8103);host 由 desktop 主进程 spawn。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f7-w5-workbench-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$logFile = "evidence\f7-w5-workbench-smoke-$ts.log"
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

  Log "== 构建 forge-agentd(workspace/tree + llm/key 新面) =="
  $ErrorActionPreference = 'Continue'
  cargo build -p forge-agentd 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  cargo: $_" }
  $cargoExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($cargoExit -ne 0) { throw "cargo build 失败(exit=$cargoExit)" }

  Log "== 启动 agentd(8103) =="
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪"

  Log "== 直连自检:workspace/tree 根 + llm/key 400 面 =="
  $tree = Invoke-RestMethod -Uri "http://127.0.0.1:8103/api/forge/workspace/tree" -TimeoutSec 5
  if ($tree.entries.Count -lt 1) { throw "workspace/tree 根空" }
  $hasCrates = @($tree.entries | Where-Object { $_.name -eq 'crates' -and $_.kind -eq 'dir' }).Count -eq 1
  if (-not $hasCrates) { throw "workspace/tree 根未见 crates 目录" }
  Log ("workspace/tree 根 entries={0} truncated={1}(crates 目录在)" -f $tree.total, $tree.truncated)
  try {
    Invoke-RestMethod -Uri "http://127.0.0.1:8103/api/forge/workspace/tree?path=.." -TimeoutSec 5 | Out-Null
    throw "path=.. 未拒(应 400)"
  } catch {
    if ($_.Exception.Response.StatusCode.value__ -ne 400) { throw "path=.. 期望 400,实: $($_.Exception.Message)" }
    Log "workspace/tree path=.. → 400 PATH_OUTSIDE_ROOT 如实"
  }
  try {
    Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:8103/api/forge/llm/key" -ContentType "application/json" -Body '{"apiKey":"  "}' -TimeoutSec 5 | Out-Null
    throw "llm/key 空 key 未拒(应 400)"
  } catch {
    if ($_.Exception.Response.StatusCode.value__ -ne 400) { throw "llm/key 空 key 期望 400,实: $($_.Exception.Message)" }
    Log "llm/key 空 key → 400 EMPTY_KEY 如实"
  }

  Log "== 构建 client + host =="
  $ErrorActionPreference = 'Continue'
  pnpm --filter @forge/client build 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  pnpm: $_" }
  $clientExit = $LASTEXITCODE
  pnpm --filter @forge/host build 2>&1 | Select-Object -Last 2 | ForEach-Object { Log "  pnpm: $_" }
  $hostExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($clientExit -ne 0) { throw "client build 失败(exit=$clientExit)" }
  if ($hostExit -ne 0) { throw "host build 失败(exit=$hostExit)" }

  foreach ($scenario in @('settings', 'workbench')) {
    $env:FORGE_SMOKE_SCENARIO = $scenario
    Log "desktop smoke(scenario=$scenario)..."
    $ErrorActionPreference = 'Continue'
    $out = & pnpm --filter @forge/desktop smoke 2>&1 | Out-String
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    $out | Add-Content $logFile
    if ($code -ne 0) { throw "desktop smoke($scenario) exit=$code`n$out" }
    $passLine = ($out -split "`n" | Select-String -Pattern '\[smoke\] PASS').Line
    Log "desktop smoke($scenario): $passLine"
    # 子进程冒烟日志关键行复读(只取最后一次运行,防旧史混淆)
    $child = @(Get-Content apps\desktop\evidence\smoke.log -Tail 400 -ErrorAction SilentlyContinue)
    $startIdx = 0
    $starts = @($child | Select-String -Pattern 'main\(\) enter')
    if ($starts.Count -gt 0) { $startIdx = $starts[-1].LineNumber - 1 }
    foreach ($l in ($child[$startIdx..($child.Count - 1)] | Select-String -Pattern "scenario=$scenario")) { Log "  $($l.Line)" }
    Remove-Item Env:\FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue
  }
  Log "F7 wave.5 workbench 与设置冒烟 PASS(G-F7-5)"
  exit 0
} catch {
  Log "FAIL: $_"
  Log "F7 wave.5 workbench 与设置冒烟 FAIL"
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
