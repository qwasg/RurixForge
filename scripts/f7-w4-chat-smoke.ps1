# F7 wave.4 聊天体验冒烟(G-F7-4):desktop chat 场景,三腿端到端。
# 断言(main.cjs scenario=chat 内逐条 throw 把关):
#   腿1 New Agent → composer 填发「说一句你好」→ mock 完成:用户卡+助手卡「铸」方块
#       +模型 label mock+终态(无 stream-caret)+文本含 mock 回文
#   腿2 模式切 multitask 填发「给场景加碰撞体 collider」→ 工具段出现,展开见
#       swarm.execute 动词「集群执行」+ completed(真 engine 链:scene_new+3 实体前置)
#   腿3 点腿1 用户卡内联编辑改「说一句你好呀」重发 → revert 生效(消息数回退后重增,
#       腿2 残留消失)+ 最终 assistant 完成
# 前置:agentd 本脚本自带起(8103;会话 REST + SSE + ask:execute + MCP 面);host 由 desktop 主进程 spawn。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f7-w4-chat-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$logFile = "evidence\f7-w4-chat-smoke-$ts.log"
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

  Log "== 构建 forge-agentd / engine-scene-mcp / engine-host(multitask 真 engine 链) =="
  $ErrorActionPreference = 'Continue'
  cargo build -p forge-agentd -p engine-scene-mcp -p engine-host 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  cargo: $_" }
  $cargoExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($cargoExit -ne 0) { throw "cargo build 失败(exit=$cargoExit)" }

  Log "== 启动 agentd(8103,真后端:会话/SSE/ask:execute/MCP 面) =="
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪"

  Log "== 构建 client(chat 产物 = host 静态目录) + host =="
  $ErrorActionPreference = 'Continue'
  pnpm --filter @forge/client build 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  pnpm: $_" }
  $clientExit = $LASTEXITCODE
  pnpm --filter @forge/host build 2>&1 | Select-Object -Last 2 | ForEach-Object { Log "  pnpm: $_" }
  $hostExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($clientExit -ne 0) { throw "client build 失败(exit=$clientExit)" }
  if ($hostExit -ne 0) { throw "host build 失败(exit=$hostExit)" }

  $env:FORGE_SMOKE_SCENARIO = 'chat'
  Log "desktop smoke(scenario=chat)..."
  $ErrorActionPreference = 'Continue'
  $out = & pnpm --filter @forge/desktop smoke 2>&1 | Out-String
  $code = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  $out | Add-Content $logFile
  if ($code -ne 0) { throw "desktop smoke exit=$code`n$out" }
  $passLine = ($out -split "`n" | Select-String -Pattern '\[smoke\] PASS').Line
  Log "desktop smoke: $passLine"
  # 子进程冒烟日志关键行复读(逐腿断言留痕;只取最后一次运行[最后 main() enter 之后],防旧史混淆)
  $child = @(Get-Content apps\desktop\evidence\smoke.log -Tail 400 -ErrorAction SilentlyContinue)
  $startIdx = 0
  $starts = @($child | Select-String -Pattern 'main\(\) enter')
  if ($starts.Count -gt 0) { $startIdx = $starts[-1].LineNumber - 1 }
  foreach ($l in ($child[$startIdx..($child.Count - 1)] | Select-String -Pattern 'scenario=chat')) { Log "  $($l.Line)" }
  Log "F7 wave.4 聊天体验冒烟 PASS(G-F7-4)"
  exit 0
} catch {
  Log "FAIL: $_"
  Log "F7 wave.4 聊天体验冒烟 FAIL"
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
