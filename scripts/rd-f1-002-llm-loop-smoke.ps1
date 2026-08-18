# RD-F1-002 真 LLM 工具循环 live 冒烟(G-RDG-1):agentd /api/forge/llm/chat 经 DeepSeek 官方 API
# 真实工具循环(tools/list 五 server → OpenAI tools → tool_calls → 进程内 mcp::call_tool → role:tool 回注)。
# 断言(配 key 环境,live):①provider=deepseek ②toolCalls 非空且含成功项 ③「创建 3 个立方体」后 scene_summary 实体数实增
#   ④密钥红线 R-5:llm/chat 原始响应 + 本冒烟日志均不含密钥子串
# (无 key 环境,mock 恒绿):provider=mock + toolCalls 空,如实 SKIP live 段,exit 0。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\rd-f1-002-llm-loop-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\rd-f1-002-llm-loop-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
# UTF-8 显式编解码 POST(PS Invoke-WebRequest 缺省 ISO-8859-1 会毁中文/特殊字符,实测坑)。
function Post-Json($url, $bodyObj, $timeoutSec = 600) {
  $body = $bodyObj | ConvertTo-Json -Depth 12 -Compress
  $req = [System.Net.HttpWebRequest]::Create($url)
  $req.Method = 'POST'
  $req.ContentType = 'application/json; charset=utf-8'
  $req.Timeout = $timeoutSec * 1000
  $bytes = [Text.Encoding]::UTF8.GetBytes($body)
  $req.ContentLength = $bytes.Length
  $st = $req.GetRequestStream(); $st.Write($bytes, 0, $bytes.Length); $st.Close()
  try { $resp = $req.GetResponse() }
  catch [System.Net.WebException] {
    $resp = $_.Exception.Response
    if ($null -eq $resp) { throw }
    $sr = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
    $errBody = $sr.ReadToEnd(); $sr.Close()
    throw "HTTP $([int]$resp.StatusCode): $errBody"
  }
  $sr = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
  $text = $sr.ReadToEnd(); $sr.Close()
  return $text
}
function McpCall($tool, $arguments) {
  $raw = Post-Json "http://127.0.0.1:8103/api/forge/mcp/call" @{ tool = $tool; arguments = $arguments } 60
  $outer = $raw | ConvertFrom-Json
  if ($outer.isError -eq $true) { throw "$tool 工具级 isError: $($outer.content[0].text)" }
  return $outer.content[0].text | ConvertFrom-Json
}

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$script:startTime = Get-Date
$ksFile = "$root\data\keystore.json"
try {
  # ── 0. 密钥面探测(只判存在性,密钥本体永不进日志)──
  $key = $null
  if ($env:FORGE_LLM_API_KEY) { $key = $env:FORGE_LLM_API_KEY }
  elseif (Test-Path $ksFile) {
    try {
      $ks = Get-Content $ksFile -Raw -Encoding UTF8 | ConvertFrom-Json
      if ($ks.keys -and $ks.keys.deepseek) { $key = [string]$ks.keys.deepseek }
    } catch { Log "keystore 解析失败(按无 key 走 mock): $_" }
  }
  $hasKey = -not [string]::IsNullOrEmpty($key)
  Log ("密钥面: " + $(if ($hasKey) { "检测到(env FORGE_LLM_API_KEY 或 keystore[deepseek]),走 deepseek live" } else { "未检测到,走 mock(如实 SKIP live 段)" }))

  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪(127.0.0.1:8103)"

  # ── 1. 场景基线 ──
  McpCall "mcp__engine-scene__scene_new" @{} | Out-Null
  $base = (McpCall "mcp__engine-scene__scene_summary" @{}).entityCount
  Log "scene_new 完成,基线 entityCount=$base"

  # ── 2. POST /api/forge/llm/chat(工具循环;live 可多轮,超时给足)──
  Log "== POST /api/forge/llm/chat:「创建 3 个立方体」 =="
  $chatRaw = Post-Json "http://127.0.0.1:8103/api/forge/llm/chat" @{ text = "请在场景中创建 3 个立方体实体,命名 Cube1/Cube2/Cube3,完成后如实汇报。"; mode = "build" } 600
  if ($hasKey -and $key -and $chatRaw.Contains($key)) { throw "R-5 红线:llm/chat 响应含密钥子串" }
  $chat = $chatRaw | ConvertFrom-Json
  $calls = @($chat.toolCalls)
  Log ("provider={0} iters={1} toolCalls={2}" -f $chat.provider, $chat.iters, $calls.Count)

  if (-not $hasKey) {
    # mock 恒绿门:provider=mock + 空 toolCalls + 如实标注,exit 0。
    if ($chat.provider -ne "mock") { throw "无 key 环境须 provider=mock,实: $($chat.provider)" }
    if ($calls.Count -ne 0) { throw "mock 须 toolCalls 空: $chatRaw" }
    Log "mock 恒绿门 PASS(provider=mock,toolCalls=0);live 段 SKIP(无 key 如实)"
    Log "RD-F1-002 冒烟 PASS(mock 轨)"
    exit 0
  }

  # ── 3. live 断言(G-RDG-1 主门)──
  if ($chat.provider -ne "deepseek") { throw "live 须 provider=deepseek,实: $($chat.provider) | $chatRaw" }
  if ($calls.Count -lt 1) { throw "live 须 toolCalls 非空(工具循环真实发生): $chatRaw" }
  $okCalls = @($calls | Where-Object { $_.ok -eq $true })
  if ($okCalls.Count -lt 1) { throw "须至少 1 个成功工具调用: $($calls | ConvertTo-Json -Compress)" }
  foreach ($c in $calls) { Log ("  {0} {1} — {2}" -f $(if ($c.ok) { "OK " } else { "ERR" }), $c.name, $c.summary) }
  $created = @($calls | Where-Object { $_.name -eq "mcp__engine-scene__entity_create" -and $_.ok -eq $true })
  if ($created.Count -lt 1) { throw "须含成功 entity_create: $($calls | ConvertTo-Json -Compress)" }

  $after = (McpCall "mcp__engine-scene__scene_summary" @{}).entityCount
  if ($after -le $base) { throw "实体数须实增:基线 $base → 现 $after" }
  Log "实体数实增 PASS($base → $after,+($after-$base);成功 entity_create ×$($created.Count))"

  # ── 4. R-5 红线全文扫描(响应 + 日志)──
  foreach ($c in $calls) { if ($key -and ($c.summary -and $c.summary.Contains($key))) { throw "R-5 红线:toolCalls.summary 含密钥子串" } }
  $logText = [IO.File]::ReadAllText($logFile, [Text.Encoding]::UTF8)
  if ($key -and $logText.Contains($key)) { throw "R-5 红线:冒烟日志含密钥子串" }
  Log "R-5 红线扫描 PASS(响应/日志均无密钥子串)"

  Log "RD-F1-002 真 LLM 工具循环冒烟 PASS(G-RDG-1 live 轨)"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  # 连带清杀本冒烟派生的 MCP/host 子孙进程(孤儿继承 stdout 句柄会挂住调用方管道,实测坑)。
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-scene-mcp', 'code-forge-mcp', 'asset-pipeline-mcp', 'gen-image-mcp', 'gen-model-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
}
