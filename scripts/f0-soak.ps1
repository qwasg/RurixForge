# F0 soak(G-F0 稳定门):双轨实测。
# 轨 A 栈抖动:经 gateway 连续交替调用 scene_summary/render_once(每次全新 MCP+host 进程链),统计错误与进程泄漏。
# 轨 B host 长航时:独立 engine-host 固定步长跑,结束取 steps(≈时长×60)与 stepErrors。
# 用法: pwsh scripts/f0-soak.ps1 [-DurationSec 300]; exit 0 = PASS
param([int]$DurationSec = 300)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f0-soak-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$result = [ordered]@{ durationSec = $DurationSec; calls = 0; errors = 0; leakBefore = 0; leakAfter = 0; endurance = $null; verdict = "FAIL" }
try {
  $result.leakBefore = (Get-Process engine-host,forge-agentd -ErrorAction SilentlyContinue | Measure-Object).Count

  Log "== start agentd :8103 / gateway :8102 =="
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  Start-Sleep -Seconds 2
  $jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'soak',exp:Math.floor(Date.now()/1000)+7200});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  Log "== start endurance engine-host :17910 =="
  $procs += Start-Process -FilePath "target\debug\engine-host.exe" -ArgumentList "--port","17910" -PassThru -WindowStyle Hidden
  Start-Sleep -Seconds 2

  $deadline = (Get-Date).AddSeconds($DurationSec)
  $i = 0
  while ((Get-Date) -lt $deadline) {
    $i++
    $tool = if ($i % 2 -eq 0) { "mcp__engine-scene__render_once" } else { "mcp__engine-scene__scene_summary" }
    try {
      $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body (@{ tool = $tool; arguments = @{} } | ConvertTo-Json) -ContentType "application/json" -Headers @{ Authorization = "Bearer $jwt" } -UseBasicParsing -TimeoutSec 30
      if ($r.StatusCode -ne 200) { $result.errors++; Log "call#$i $tool http=$($r.StatusCode)" }
      $result.calls++
    } catch { $result.errors++; Log "call#$i $tool EX: $($_.Exception.Message)" }
    if ($i % 20 -eq 0) { Log "progress: $i calls, errors=$($result.errors)" }
  }

  # 长航时结算:直接打 endurance host 的控制通道太底层,经一次性 MCP 不便(端口独立),用 TCP 长度前缀帧直查
  Log "== endurance host 结算(直连 17910) =="
  $client = New-Object System.Net.Sockets.TcpClient("127.0.0.1", 17910)
  $stream = $client.GetStream()
  $req = '{"jsonrpc":"2.0","id":1,"method":"scene.summary","params":{}}'
  $bytes = [System.Text.Encoding]::UTF8.GetBytes($req)
  $len = [BitConverter]::GetBytes([uint32]$bytes.Length)
  $stream.Write($len, 0, 4); $stream.Write($bytes, 0, $bytes.Length); $stream.Flush()
  $lenBuf = New-Object byte[] 4; $stream.Read($lenBuf, 0, 4) | Out-Null
  $respLen = [BitConverter]::ToUInt32($lenBuf, 0)
  $respBuf = New-Object byte[] $respLen; $read = 0; while ($read -lt $respLen) { $read += $stream.Read($respBuf, $read, $respLen - $read) }
  $summary = [System.Text.Encoding]::UTF8.GetString($respBuf)
  $client.Close()
  Log "endurance scene.summary: $summary"
  $j = ($summary | ConvertFrom-Json).result
  $result.endurance = @{ steps = $j.physics.steps; stepErrors = $j.physics.stepErrors; backend = $j.physics.backend; events = $j.events }
  $expectMin = [int]($DurationSec * 60 * 0.9)
  if ($j.physics.steps -lt $expectMin) { throw "物理步数不足: $($j.physics.steps) < $expectMin(≥10000 帧要求同步核查)" }
  if ($j.physics.stepErrors -ne 0) { throw "stepErrors ≠ 0" }

  Start-Sleep -Seconds 3
  $result.leakAfter = (Get-Process engine-host,forge-agentd -ErrorAction SilentlyContinue | Measure-Object).Count
  # 预期存活:agentd 1 + endurance host 1 = 基线+2(gateway 为 go 进程不计入)
  if ($result.leakAfter -gt $result.leakBefore + 2) { throw "进程泄漏: $($result.leakBefore) -> $($result.leakAfter)" }
  if ($result.errors -ne 0) { throw "调用错误 $($result.errors) 次" }

  $result.verdict = "PASS"
  Log "SOAK PASS: $($result.calls) calls, 0 errors, steps=$($result.endurance.steps)($DurationSec s)"
  exit 0
} catch {
  Log "SOAK FAIL: $($_.Exception.Message)"
  $result.verdict = "FAIL: $($_.Exception.Message)"
  exit 1
} finally {
  $result | ConvertTo-Json | Out-File "evidence\f0-soak-$ts.json"
  Log "metrics => evidence\f0-soak-$ts.json"
  foreach ($p in $procs) { try { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } catch {} }
}
