# F6 wave.1 playtest 工具门栈级冒烟(G-F6-1):agentd /api/forge/playtest/run 经 mcp 工具面编排。
# 断言:①绿矩阵(实体计数/transform_near/component_field×2)ok=true passed=4 failed=0
#   ②红矩阵(三类必红)ok=false failed=3 如实标红,负例不充绿
#   ③SSIM 腿:viewport_frame 实拍 → golden PNG(R↔B 通道交换还原真实 RGB)→ 同视角矩阵 ssim≥0.999 过;
#     异视角(yaw 120)矩阵同 golden 必红 —— SSIM 正负双例栈级实证
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f6-w1-playtest-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f6-w1-playtest-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function Post-Json($url, $bodyObj, $timeoutSec = 120) {
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
function RunMatrix($ref) {
  $raw = Post-Json "http://127.0.0.1:8103/api/forge/playtest/run" @{ matrixRef = $ref } 180
  return $raw | ConvertFrom-Json
}

New-Item -ItemType Directory -Force evidence | Out-Null
$procs = @()
$script:startTime = Get-Date
$goldenDir = "$root\tests\playtest\golden"
$goldenPng = "$goldenDir\fixture.png"
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:8103/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }
  Log "agentd 就绪"

  # ── 1. 绿矩阵:四类断言三族(计数/变换/组件字段)全过 ──
  Log "== 绿矩阵 tests/playtest/matrix_green.json =="
  $g = RunMatrix "tests/playtest/matrix_green.json"
  if ($g.ok -ne $true) { throw "绿矩阵须 ok=true: $($g.cases | ConvertTo-Json -Compress -Depth 6)" }
  if ($g.passed -ne 4 -or $g.failed -ne 0) { throw "绿矩阵须 4/0: passed=$($g.passed) failed=$($g.failed)" }
  foreach ($c in $g.cases) { Log ("  OK {0} [{1}] {2}" -f $c.name, $c.kind, $c.detail) }
  Log "绿矩阵 PASS(passed=$($g.passed) durationMs=$($g.durationMs))"

  # ── 2. 红矩阵:必红三条如实标红 ──
  Log "== 红矩阵 tests/playtest/matrix_red.json =="
  $r = RunMatrix "tests/playtest/matrix_red.json"
  if ($r.ok -ne $false) { throw "红矩阵须 ok=false(不充绿)" }
  if ($r.failed -ne 3) { throw "红矩阵须 failed=3: $($r | ConvertTo-Json -Compress -Depth 6)" }
  foreach ($c in $r.cases) {
    if ($c.pass -eq $true) { throw "红矩阵 $($c.name) 不得意外通过" }
    Log ("  RED {0} [{1}] {2}" -f $c.name, $c.kind, $c.detail)
  }
  Log "红矩阵 PASS(failed=$($r.failed) 如实标红;ok=false)"

  # ── 3. SSIM 腿:实拍 → golden → 同视角过 / 异视角红 ──
  Log "== SSIM 腿:viewport_frame 实拍生成 golden =="
  McpCall "mcp__engine-scene__scene_load" @{ path = "tests/playtest/fixture.rxscene" } | Out-Null
  McpCall "mcp__engine-scene__viewport_set_camera" @{ target = @(1.5, 0.5, 1.5); yaw = 45.0; pitch = -20.0; dist = 10.0; fovY = 60.0 } | Out-Null
  $frame = McpCall "mcp__engine-scene__viewport_frame" @{ width = 960; height = 540 }
  $w = [int]$frame.width; $h = [int]$frame.height
  $rgba = [Convert]::FromBase64String($frame.pixelsB64)
  if ($rgba.Length -ne $w * $h * 4) { throw "帧字节数不符: $($rgba.Length) != $w*$h*4" }
  # System.Drawing Format32bppArgb 内存序为 BGRA:先 R↔B 交换,使 PNG 读回 == 原 rgba(SSIM 通道对齐)。
  $bgra = New-Object byte[] $rgba.Length
  for ($i = 0; $i -lt $rgba.Length; $i += 4) {
    $bgra[$i] = $rgba[$i + 2]; $bgra[$i + 1] = $rgba[$i + 1]; $bgra[$i + 2] = $rgba[$i]; $bgra[$i + 3] = $rgba[$i + 3]
  }
  New-Item -ItemType Directory -Force $goldenDir | Out-Null
  Add-Type -AssemblyName System.Drawing
  $bmp = New-Object System.Drawing.Bitmap($w, $h, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $rect = New-Object System.Drawing.Rectangle(0, 0, $w, $h)
  $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::WriteOnly, $bmp.PixelFormat)
  $stride = [Math]::Abs($data.Stride)
  for ($y = 0; $y -lt $h; $y++) {
    [Runtime.InteropServices.Marshal]::Copy($bgra, $y * $w * 4, [IntPtr]($data.Scan0.ToInt64() + $y * $stride), $w * 4)
  }
  $bmp.UnlockBits($data)
  $bmp.Save($goldenPng, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  Log "golden 落盘 tests/playtest/golden/fixture.png($w x $h,nonZeroPixels=$($frame.nonZeroPixels))"

  $sg = RunMatrix "tests/playtest/matrix_ssim_green.json"
  $ssimCase = @($sg.cases)[0]
  Log ("  同视角 ssim actual={0} detail={1}" -f $ssimCase.actual, $ssimCase.detail)
  if ($sg.ok -ne $true) { throw "同视角 SSIM 矩阵须 ok=true: $($ssimCase | ConvertTo-Json -Compress)" }
  if ([double]$ssimCase.actual -lt 0.9999) { Log "  注:ssim=$($ssimCase.actual) 非 1.0(跨运行渲染微差),阈值 0.999 内过" }

  $sr = RunMatrix "tests/playtest/matrix_ssim_red.json"
  $ssimRed = @($sr.cases)[0]
  Log ("  异视角 ssim actual={0} detail={1}" -f $ssimRed.actual, $ssimRed.detail)
  if ($sr.ok -ne $false) { throw "异视角 SSIM 矩阵须 ok=false(不充绿)" }
  if ($ssimRed.pass -eq $true) { throw "异视角不得过 threshold 0.999" }

  Log "SSIM 腿 PASS(同视角过 ssim=$($ssimCase.actual);异视角红 ssim=$($ssimRed.actual))"
  Log "F6 wave.1 playtest 工具门冒烟 PASS(G-F6-1)"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('engine-scene-mcp', 'code-forge-mcp', 'asset-pipeline-mcp', 'gen-image-mcp', 'gen-model-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
  try { Remove-Item $goldenPng -Force -ErrorAction SilentlyContinue } catch {}
}
