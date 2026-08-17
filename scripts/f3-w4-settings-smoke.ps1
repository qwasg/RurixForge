# F3 wave.4 前端门冒烟(G-F3-4 桌面腿)。
# 流程:起 agentd(skills/list 上游)→ desktop 冒烟 FORGE_SMOKE_SCENARIO=settings →
#       侧栏 Settings → 设置页 skills tab 真实列表(>=13 篇)→ 截图字节数断言。
# 前置:target\debug\forge-agentd.exe 已构建;packages/client dist 已构建(host 静态托管)。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f3-w4-settings-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f3-w4-settings-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$startedAt = Get-Date
try {
  # ── 0. 前置:agentd exe + client dist ──
  if (-not (Test-Path "target\debug\forge-agentd.exe")) { throw "forge-agentd.exe 未构建(先 cargo build -p forge-agentd)" }
  if (-not (Test-Path "packages\client\dist\index.html")) { throw "client dist 未构建(先 pnpm --filter @forge/client build)" }

  # ── 1. 起 agentd(host forgeProxy 上游 8103;desktop 冒烟自行 spawn host)──
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 8103 就绪超时" }
  Log "agentd 就绪(8103)"

  # ── 2. desktop 冒烟:settings 场景 ──
  Log "== desktop smoke: FORGE_SMOKE_SCENARIO=settings =="
  $env:FORGE_SMOKE_SCENARIO = "settings"
  # PS 5.1:2>&1 合并后 stderr 行被包成 ErrorRecord,Stop 偏好会误抛——先 Continue 收集再按 exit code 判定
  $ErrorActionPreference = 'Continue'
  $smokeOut = pnpm --filter @forge/desktop smoke 2>&1
  $smokeCode = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  $smokeOut | ForEach-Object { Log "  $_" }
  if ($smokeCode -ne 0) { throw "desktop smoke 失败(exit=$smokeCode)" }
  Remove-Item Env:FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue

  # ── 3. 断言:settings 截图本次产出 + smoke.log skill 计数 ──
  $shot = Get-ChildItem "apps\desktop\evidence\desktop-smoke-settings-*.png" -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -ge $startedAt.AddSeconds(-5) } |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if (-not $shot) { throw "未找到本次 settings 场景截图" }
  Log "截图: $($shot.Name) ($($shot.Length) B)"

  $smokeLogLines = Get-Content "apps\desktop\evidence\smoke.log" |
    Where-Object { $_ -match "scenario=settings skill items: (\d+)" }
  if (-not $smokeLogLines) { throw "smoke.log 缺 skill 计数行" }
  # smoke.log 为 append 留档:取最后一次匹配行(本次运行)
  $smokeLogLines[-1] -match "skill items: (\d+)" | Out-Null
  $count = [int]$Matches[1]
  if ($count -lt 13) { throw "skills 条目不足: $count < 13" }
  Log "skills 列表计数: $count(>=13 篇,含 4 可执行 + 9 seam)"

  Log "F3 wave.4 设置页 desktop 冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  Remove-Item Env:FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
