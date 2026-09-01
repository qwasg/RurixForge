# F1 wave.4 H.264 流腿冒烟(G-F1-13 栈级):
#   1) 经 gateway 取 format=h264 帧 → nalB64 非空 + Annex B 起始码 + SPS/PPS/IDR + 首帧关键帧、次帧非关键帧;
#   2) rgba8 缺省回退腿回归(pixelsB64 非空);
#   3) 码流经 Electron Chromium WebCodecs VideoDecoder(annexb)解码 → 解码帧尺寸与请求一致(纯 web 可解码证据)。
# 前提:cargo build --workspace 已跑(forge-agentd.exe / engine-host.exe 为最新)。
# 用法: pwsh scripts\f1-w4-h264-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f1-w4-h264-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function McpCall($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 8 -Compress
  try {
    $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json" -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 60
  } catch {
    $resp = $_.Exception.Response
    $errBody = ""
    if ($resp) { $sr = New-Object System.IO.StreamReader($resp.GetResponseStream()); $errBody = $sr.ReadToEnd() }
    throw "$tool 失败 status=$([int]$resp.StatusCode) body=$errBody 请求体=$body"
  }
  if ($r.StatusCode -ne 200) { throw "$tool http=$($r.StatusCode)" }
  $outer = $r.Content | ConvertFrom-Json
  return $outer.content[0].text | ConvertFrom-Json
}
function Get-NalTypes([byte[]]$buf) {
  $types = @()
  for ($i = 0; $i -lt $buf.Length - 4; $i++) {
    if ($buf[$i] -eq 0 -and $buf[$i + 1] -eq 0 -and $buf[$i + 2] -eq 1) { $types += ($buf[$i + 3] -band 0x1F) }
    elseif ($buf[$i] -eq 0 -and $buf[$i + 1] -eq 0 -and $buf[$i + 2] -eq 0 -and $buf[$i + 3] -eq 1) { $types += ($buf[$i + 4] -band 0x1F) }
  }
  return $types
}

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f1w4',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  Log "== 布场:单立方体 =="
  McpCall "mcp__engine-scene__scene_new" @{ name = "f1-w4" } | Out-Null
  McpCall "mcp__engine-scene__entity_create" @{ name = "cube"; translation = @(0.0, 0.5, 0.0); components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = "cube"; material = "a" } }) } | Out-Null

  Log "== G-F1-13 栈级:viewport_frame format=h264 =="
  $f1 = McpCall "mcp__engine-scene__viewport_frame" @{ width = 320; height = 240; format = "h264" }
  if ($f1.format -ne "h264") { throw "format=$($f1.format) ≠ h264" }
  if (-not $f1.nalB64) { throw "nalB64 为空" }
  $nal = [Convert]::FromBase64String($f1.nalB64)
  if ($nal.Length -le 0) { throw "h264 码流为空" }
  if ($f1.keyframe -ne $true) { throw "首帧应为关键帧" }
  if ($f1.width -ne 320 -or $f1.height -ne 240) { throw "尺寸回包异常: $($f1.width)x$($f1.height)" }
  $types = Get-NalTypes $nal
  foreach ($t in @(7, 8, 5)) { if ($types -notcontains $t) { throw "缺 NAL type $t,实际: $($types -join ',')" } }
  Log "frame1: nalBytes=$($nal.Length) nalTypes=$($types -join '/') keyframe=true device=$($f1.deviceName) PASS"

  $f2 = McpCall "mcp__engine-scene__viewport_frame" @{ width = 320; height = 240; format = "h264" }
  if ($f2.keyframe -ne $false) { throw "次帧应为非关键帧" }
  $nal2 = [Convert]::FromBase64String($f2.nalB64)
  if ($nal2.Length -le 0) { throw "次帧码流为空" }
  Log "关键帧周期:keyframe[0]=true keyframe[1]=false PASS"

  Log "== rgba8 缺省回退腿回归 =="
  $f0 = McpCall "mcp__engine-scene__viewport_frame" @{ width = 320; height = 240 }
  if ($f0.format -ne "rgba8") { throw "缺省 format=$($f0.format) ≠ rgba8" }
  if (-not $f0.pixelsB64) { throw "pixelsB64 为空" }
  Log "rgba8 回退(pixelsB64 $($f0.pixelsB64.Length) chars)PASS"

  Log "== G-F1-13 解码腿:WebCodecs VideoDecoder(annexb) =="
  $nalFile = "evidence\f1-w4-frame-$ts.annexb"
  $outJson = "evidence\f1-w4-decode-$ts.json"
  [IO.File]::WriteAllBytes("$root\$nalFile", $nal)
  $electronBin = node -e "const r=require('module').createRequire(process.cwd()+'/apps/desktop/package.json');process.stdout.write(r('electron'))"
  if (-not (Test-Path $electronBin)) { throw "electron 二进制解析失败: $electronBin" }
  $decMain = "apps\desktop\scripts\h264-decode-main.cjs"
  $p = Start-Process -FilePath $electronBin -ArgumentList @($decMain, "--nal", $nalFile, "--out", $outJson, "--expect-w", "320", "--expect-h", "240") -WorkingDirectory $root -PassThru -WindowStyle Hidden
  if (-not $p.WaitForExit(60000)) { try { $p.Kill() } catch {}; throw "WebCodecs 解码 60s 超时" }
  if (-not (Test-Path $outJson)) { throw "解码结果文件缺失: $outJson(exit=$($p.ExitCode))" }
  $dec = Get-Content $outJson -Raw | ConvertFrom-Json
  if ($p.ExitCode -ne 0 -or -not $dec.ok) { throw "WebCodecs 解码失败: exit=$($p.ExitCode) $($dec | ConvertTo-Json -Compress -Depth 6)" }
  if (-not $dec.sizeMatch) { throw "解码尺寸不一致: $($dec.frames[0].w)x$($dec.frames[0].h) ≠ 320x240" }
  Log "WebCodecs 解码:codec=$($dec.codec) nalTypes=$($dec.nalTypes -join '/') decoded=$($dec.decoded) 尺寸=$($dec.frames[0].w)x$($dec.frames[0].h)==320x240 PASS"

  Log "F1 wave.4 H.264 流腿冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
