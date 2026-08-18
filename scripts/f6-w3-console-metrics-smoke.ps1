# F6 wave.3 Console/Metrics 门栈级冒烟(G-F6-3):desktop console-metrics 场景。
# 断言:metrics tab 采样来自 scene_summary 实测(frames 序列)且 PIE 运行中实测变化;
#   Console playtest 报告行注入可见 / 类型过滤 chip 实测可用 / 清空实测清空;截图留档。
# 前置:agentd 本脚本自带起(playtest 矩阵 + MCP 面)。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f6-w3-console-metrics-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f6-w3-console-metrics-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$script:startTime = Get-Date
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪"
  $env:FORGE_SMOKE_SCENARIO = 'console-metrics'
  Log "desktop smoke(scenario=console-metrics)..."
  $ErrorActionPreference = 'Continue'
  $out = & pnpm --filter @forge/desktop smoke 2>&1 | Out-String
  $code = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  $out | Add-Content $logFile
  if ($code -ne 0) { throw "desktop smoke exit=$code`n$out" }
  $passLine = ($out -split "`n" | Select-String -Pattern '\[smoke\] PASS').Line
  Log "desktop smoke: $passLine"
  # 子进程 smoke 日志关键行复读(采样变化/报告注入/过滤/清空)
  $child = Get-Content apps\desktop\evidence\smoke-child.log -Tail 30 -ErrorAction SilentlyContinue
  foreach ($l in $child) { if ($l -match 'console-metrics') { Log "  $l" } }
  Log "F6 wave.3 Console/Metrics 门冒烟 PASS(G-F6-3)"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  Remove-Item Env:\FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-scene-mcp', 'code-forge-mcp', 'asset-pipeline-mcp', 'gen-image-mcp', 'gen-model-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
}
