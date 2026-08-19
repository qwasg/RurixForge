# F7 wave.5 回归辅助:单场景 desktop 冒烟驱动(无既有脚本的场景用,如 assets)。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f7-w5-legacy-scenario.ps1 -Scenario assets
param([Parameter(Mandatory=$true)][string]$Scenario)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$logFile = "evidence\f7-w5-legacy-$Scenario-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$script:startTime = Get-Date
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪"
  $env:FORGE_SMOKE_SCENARIO = $Scenario
  Log "desktop smoke(scenario=$Scenario)..."
  $ErrorActionPreference = 'Continue'
  $out = & pnpm --filter @forge/desktop smoke 2>&1 | Out-String
  $code = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  $out | Add-Content $logFile
  if ($code -ne 0) { throw "desktop smoke exit=$code`n$out" }
  $passLine = ($out -split "`n" | Select-String -Pattern '\[smoke\] PASS').Line
  Log "desktop smoke: $passLine"
  $child = @(Get-Content apps\desktop\evidence\smoke.log -Tail 300 -ErrorAction SilentlyContinue)
  $startIdx = 0
  $starts = @($child | Select-String -Pattern 'main\(\) enter')
  if ($starts.Count -gt 0) { $startIdx = $starts[-1].LineNumber - 1 }
  foreach ($l in ($child[$startIdx..($child.Count - 1)] | Select-String -Pattern "scenario=$Scenario")) { Log "  $($l.Line)" }
  Log "legacy scenario=$Scenario PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  Remove-Item Env:\FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
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
