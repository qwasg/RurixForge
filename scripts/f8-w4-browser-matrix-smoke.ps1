# F8 wave.4 浏览器真实任务矩阵冒烟(G-F8-4)薄包装:
#   ① 确保 tools/e2e 依赖(playwright-core,workspace 安装)② 跑 Node 矩阵脚本
#   (构建检查→agentd/host 拉起→八任务逐独立断言+截图→finally 清理)③ exit code 透传
#   ④ 兜底孤儿清杀(进程名+启动时间+命令行三条件过滤,防误杀)。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f8-w4-browser-matrix-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$logFile = "evidence\f8-w4-browser-matrix-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
New-Item -ItemType Directory -Force evidence | Out-Null
$script:startTime = Get-Date

try {
  if (-not (Test-Path "tools\e2e\node_modules\playwright-core\package.json")) {
    Log "== tools/e2e 依赖未装,pnpm install(workspace 全量,playwright-core 无浏览器下载)=="
    $ErrorActionPreference = 'Continue'
    pnpm install 2>&1 | Select-Object -Last 5 | ForEach-Object { Log "  pnpm: $_" }
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) { throw "pnpm install 失败(exit=$code)" }
  }
  Log "== 运行浏览器任务矩阵(node tools/e2e/f8-w4-browser-matrix.mjs)=="
  $ErrorActionPreference = 'Continue'
  $out = & node tools\e2e\f8-w4-browser-matrix.mjs 2>&1 | Out-String
  $code = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  $out | Add-Content $logFile
  foreach ($l in ($out -split "`n" | Select-String -Pattern '^==|verdict=|allGreen|orphanCheck|\[FAIL\]|\[FATAL\]')) { Log "  $($l.Line.TrimEnd())" }
  if ($code -ne 0) { throw "浏览器任务矩阵 exit=$code(详 evidence/f8-w4-matrix-*.json)" }
  Log "F8 wave.4 浏览器真实任务矩阵 PASS(G-F8-4)"
  exit 0
} catch {
  Log "FAIL: $_"
  Log "F8 wave.4 浏览器真实任务矩阵 FAIL"
  exit 1
} finally {
  Start-Sleep -Milliseconds 500
  # 兜底孤儿清杀:按进程名+启动时间(agentd/MCP/engine 族)与命令行(host node / playwright 浏览器)
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('forge-agentd','engine-scene-mcp','engine-host','code-forge-mcp','asset-pipeline-mcp','gen-image-mcp','gen-model-mcp') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { Log "  兜底清杀 $($_.Name) pid=$($_.Id)"; $_.Kill() } catch {} }
  Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -match 'packages[\\/]host[\\/]dist[\\/]index\.js' -and $_.CreationDate -ge $script:startTime } |
    ForEach-Object { try { Log "  兜底清杀 host node pid=$($_.ProcessId)"; Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } catch {} }
  Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
    Where-Object { ($_.Name -eq 'msedge.exe' -or $_.Name -eq 'chrome.exe') -and $_.CommandLine -match 'playwright' -and $_.CreationDate -ge $script:startTime } |
    ForEach-Object { try { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } catch {} }
}
