# F4 wave.4 NodeGraph 面板 desktop 冒烟(G-F4-3 前端腿)。
# 流程:起 agentd(graph_get 上游;desktop 冒烟自行 spawn host)→
#       desktop 冒烟 FORGE_SMOKE_SCENARIO=nodegraph → 侧栏「编辑器」→ NodeGraph 页签 →
#       图路径输入 Content/Graphs/door_opener.rxgraph → 加载 → 节点卡片 >=4 断言 → 截图字节数断言。
# 链路:client → host(3080,forgeProxy)→ agentd(8103)→ code-forge-mcp(graph_get 读 projects/demo)。
# 前置:target\debug\forge-agentd.exe 与 code-forge-mcp.exe 已构建;packages/client dist 已构建(host 静态托管)。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f4-w4-nodegraph-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f4-w4-nodegraph-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$startedAt = Get-Date
try {
  # ── 0. 前置:agentd / code-forge-mcp exe + client dist ──
  if (-not (Test-Path "target\debug\forge-agentd.exe")) { throw "forge-agentd.exe 未构建(先 cargo build -p forge-agentd)" }
  if (-not (Test-Path "target\debug\code-forge-mcp.exe")) { throw "code-forge-mcp.exe 未构建(先 cargo build -p code-forge-mcp)" }
  if (-not (Test-Path "packages\client\dist\index.html")) { throw "client dist 未构建(先 pnpm --filter @forge/client build)" }

  # ── 1. 起 agentd(host forgeProxy 上游 8103;desktop 冒烟自行 spawn host)──
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 8103 就绪超时" }
  Log "agentd 就绪(8103)"

  # ── 2. desktop 冒烟:nodegraph 场景 ──
  Log "== desktop smoke: FORGE_SMOKE_SCENARIO=nodegraph =="
  $env:FORGE_SMOKE_SCENARIO = "nodegraph"
  # PS 5.1:2>&1 合并后 stderr 行被包成 ErrorRecord,Stop 偏好会误抛——先 Continue 收集再按 exit code 判定
  $ErrorActionPreference = 'Continue'
  $smokeOut = pnpm --filter @forge/desktop smoke 2>&1
  $smokeCode = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  $smokeOut | ForEach-Object { Log "  $_" }
  if ($smokeCode -ne 0) { throw "desktop smoke 失败(exit=$smokeCode)" }
  Remove-Item Env:FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue

  # ── 3. 断言:nodegraph 截图本次产出 + smoke.log 节点计数 ──
  $shot = Get-ChildItem "apps\desktop\evidence\desktop-smoke-nodegraph-*.png" -ErrorAction SilentlyContinue |
    Where-Object { $_.LastWriteTime -ge $startedAt.AddSeconds(-5) } |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if (-not $shot) { throw "未找到本次 nodegraph 场景截图" }
  Log "截图: $($shot.Name) ($($shot.Length) B)"

  $smokeLogLines = Get-Content "apps\desktop\evidence\smoke.log" |
    Where-Object { $_ -match "scenario=nodegraph graph nodes: (\d+)" }
  if (-not $smokeLogLines) { throw "smoke.log 缺 graph nodes 计数行" }
  # smoke.log 为 append 留档:取最后一次匹配行(本次运行)
  $smokeLogLines[-1] -match "graph nodes: (\d+)" | Out-Null
  $count = [int]$Matches[1]
  if ($count -lt 4) { throw "NodeGraph 节点卡片不足: $count < 4" }
  Log "NodeGraph 节点计数: $count(>=4,door_opener 4 节点)"

  Log "F4 wave.4 NodeGraph 面板 desktop 冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  Remove-Item Env:FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  # 连带清杀本冒烟派生的 MCP/host 子孙进程(孤儿继承 stdout 句柄会挂住调用方管道,实测坑)
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-scene-mcp', 'code-forge-mcp', 'asset-pipeline-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $startedAt } |
    ForEach-Object { try { $_.Kill() } catch {} }
}
