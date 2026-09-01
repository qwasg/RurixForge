# 复现桌面腿 resize 竞态:share 960→674 换尺寸 + 帧尺寸错配窗口期,观察 framePath/错误。
param(
  [int]$W1 = 960,
  [int]$H1 = 540,
  [int]$W2 = 674,
  [int]$H2 = 540
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
function McpCall($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 8 -Compress
  $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json" -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 60
  $outer = $r.Content | ConvertFrom-Json
  return $outer.content[0].text | ConvertFrom-Json
}
function McpCallRaw($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 8 -Compress
  try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json; charset=utf-8" -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 60
    $ms = New-Object System.IO.MemoryStream
    $r.RawContentStream.CopyTo($ms)
    $outer = [System.Text.Encoding]::UTF8.GetString($ms.ToArray()) | ConvertFrom-Json
    if ($outer.isError) { return "TOOL_ERROR: $($outer.content[0].text)" }
    return ($outer.content[0].text | ConvertFrom-Json)
  } catch { return "HTTP_ERR: $($_.Exception.Message)" }
}

$procs = @()
$sleeper = $null
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'repro',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"
  $sleeper = Start-Process -FilePath "notepad.exe" -PassThru -WindowStyle Hidden

  McpCall "mcp__engine-scene__scene_new" @{ name = "repro" } | Out-Null
  McpCall "mcp__engine-scene__entity_create" @{ name = "c"; translation = @(0.0, 0.5, 0.0); components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = "cube"; material = "a" } }) } | Out-Null

  $f0 = McpCallRaw "mcp__engine-scene__viewport_frame" @{ width = $W1; height = $H1 }
  "f0(${W1}x${H1},no share): framePath=$($f0.framePath) cpu=$($f0.cpuUploads) nonzero=$($f0.nonZeroPixels)"

  $so1 = McpCallRaw "mcp__engine-scene__viewport_share_open" @{ pid = $sleeper.Id; width = $W1; height = $H1 }
  "share_open(${W1}x${H1}): $(if ($so1 -is [string]) {$so1} else {$so1 | ConvertTo-Json -Compress})"
  $f1 = McpCallRaw "mcp__engine-scene__viewport_frame" @{ width = $W1; height = $H1 }
  "f1(${W1},share${W1}): $(if ($f1 -is [string]) {$f1} else {"framePath=$($f1.framePath) cpu=$($f1.cpuUploads)"})"

  $f2 = McpCallRaw "mcp__engine-scene__viewport_frame" @{ width = $W2; height = $H2 }
  "f2(${W2}x${H2},share${W1} 错配窗口): $(if ($f2 -is [string]) {$f2} else {"framePath=$($f2.framePath) cpu=$($f2.cpuUploads)"})"

  McpCall "mcp__engine-scene__viewport_share_close" @{} | Out-Null
  $so2 = McpCallRaw "mcp__engine-scene__viewport_share_open" @{ pid = $sleeper.Id; width = $W2; height = $H2 }
  "share_open(${W2}x${H2}): $(if ($so2 -is [string]) {$so2} else {$so2 | ConvertTo-Json -Compress})"
  1..4 | ForEach-Object {
    $f = McpCallRaw "mcp__engine-scene__viewport_frame" @{ width = $W2; height = $H2 }
    "f3#$_ (${W2},share${W2}): $(if ($f -is [string]) {$f} else {"framePath=$($f.framePath) cpu=$($f.cpuUploads) nonzero=$($f.nonZeroPixels)"})"
  }
  McpCall "mcp__engine-scene__viewport_share_close" @{} | Out-Null
} finally {
  if ($sleeper -and -not $sleeper.HasExited) { try { $sleeper.Kill() } catch {} }
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
