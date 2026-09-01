# 全方向全流程自动化测试薄包装:
#   node tools/e2e/run-all.mjs(--profile stack|core)
# 用法:
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\full-auto-test.ps1
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\full-auto-test.ps1 -Profile core
param(
  [ValidateSet('stack', 'core')]
  [string]$Profile = 'stack'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
New-Item -ItemType Directory -Force evidence | Out-Null
$logFile = "evidence\full-auto-wrapper-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }

try {
  if (-not (Test-Path "tools\e2e\node_modules\playwright-core\package.json")) {
    Log "== tools/e2e 依赖未装,pnpm install =="
    $ErrorActionPreference = 'Continue'
    pnpm install 2>&1 | Select-Object -Last 8 | ForEach-Object { Log "  pnpm: $_" }
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) { throw "pnpm install 失败(exit=$code)" }
  }
  Log "== node tools/e2e/run-all.mjs --profile $Profile =="
  $ErrorActionPreference = 'Continue'
  & node tools\e2e\run-all.mjs --profile $Profile
  $code = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($code -ne 0) { throw "full-auto exit=$code(详 evidence/full-auto-*.json)" }
  Log "full-auto PASS(profile=$Profile)"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
}
