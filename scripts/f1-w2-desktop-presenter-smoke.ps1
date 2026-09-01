# F1 wave.2 G-F1-9 桌面腿:viewport-presenter 子窗口嵌入可见 Electron 窗口,
# OS 级截屏(DWM 合成,含原生子窗口)锚点像素与 readback 帧中心像素比对。
# 前提:cargo build(viewport-presenter)+ pnpm -r build(client/host dist)。
# 用法: powershell -NoProfile -File scripts/f1-w2-desktop-presenter-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f1-w2-desktop-presenter-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function McpCall($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 8 -Compress
  $r = Invoke-WebRequest -Uri "http://127.0.0.1:8102/api/forge/mcp/call" -Method POST -Body $body -ContentType "application/json" -Headers @{ Authorization = "Bearer $script:jwt" } -UseBasicParsing -TimeoutSec 60
  if ($r.StatusCode -ne 200) { throw "$tool http=$($r.StatusCode)" }
  $outer = $r.Content | ConvertFrom-Json
  return $outer.content[0].text | ConvertFrom-Json
}

New-Item -ItemType Directory -Force evidence | Out-Null
$rectJson = "apps\desktop\evidence\viewport-presenter-rect.json"
$doneFlag = "apps\desktop\evidence\os-capture-done.flag"
Remove-Item $rectJson, $doneFlag -Force -ErrorAction SilentlyContinue

$procs = @()
$electron = $null
try {
  $procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  $procs += Start-Process -FilePath "gateway-go\forge-gateway.exe" -PassThru -WindowStyle Hidden
  foreach ($u in @("http://127.0.0.1:8103/health", "http://127.0.0.1:8102/health")) {
    $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri $u -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
    if (-not $ok) { throw "$u 就绪超时" }
  }
  Log "agentd/gateway 就绪"
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f1w2d',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  Log "== 布场:3 立方体 + 相机(中心锚点应命中 cube-a,非底色) =="
  McpCall "mcp__engine-scene__scene_new" @{ name = "f1-w2-desktop" } | Out-Null
  $ops = @(
    @{ op = "create"; name = "cube-a"; translation = @(0.0, 0.5, 0.0); components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = "cube"; material = "a" } }) },
    @{ op = "create"; name = "cube-b"; translation = @(1.6, 0.5, 0.4); rotation = @(0.0, 0.3826834, 0.0, 0.9238795); components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = "cube"; material = "b" } }) },
    @{ op = "create"; name = "cube-c"; translation = @(-1.6, 0.9, -0.4); scale = @(1.0, 1.8, 1.0); components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = "cube"; material = "c" } }) }
  )
  $batch = McpCall "mcp__engine-scene__entity_batch_apply" @{ ops = $ops }
  if ($batch.applied -ne 3) { throw "batch applied=$($batch.applied) ≠ 3" }
  McpCall "mcp__engine-scene__viewport_set_camera" @{ target = @(0.0, 0.6, 0.0); yaw = 30.0; pitch = 24.0; dist = 6.5 } | Out-Null

  Log "== 启动可见 Electron(FORGE_SMOKE_VISIBLE=1, presenter 子窗口嵌入) =="
  # electron 依赖在 apps/desktop 下,须从该目录解析二进制路径
  Push-Location "$root\apps\desktop"
  try { $electronBin = node -e "console.log(require('electron'))" } finally { Pop-Location }
  if (-not (Test-Path $electronBin)) { throw "electron 二进制未找到: $electronBin" }
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $electronBin
  $psi.Arguments = "."
  $psi.WorkingDirectory = "$root\apps\desktop"
  $psi.UseShellExecute = $false
  foreach ($kv in [System.Environment]::GetEnvironmentVariables("Process").GetEnumerator()) { $psi.EnvironmentVariables[$kv.Key] = [string]$kv.Value }
  $psi.EnvironmentVariables["FORGE_SMOKE"] = "1"
  $psi.EnvironmentVariables["FORGE_SMOKE_VISIBLE"] = "1"
  $psi.EnvironmentVariables["FORGE_SMOKE_SCENARIO"] = "editor"
  $electron = New-Object System.Diagnostics.Process
  $electron.StartInfo = $psi
  [void]$electron.Start()
  Log "electron pid=$($electron.Id)"

  Log "== 等 presenter 证据 JSON(rect + readback 锚点像素) =="
  $deadline = (Get-Date).AddSeconds(90)
  while (-not (Test-Path $rectJson)) {
    if ((Get-Date) -gt $deadline) { throw "presenter 证据 JSON 90s 未产出(见 apps/desktop/evidence/smoke.log)" }
    if ($electron.HasExited) { throw "electron 早退 code=$($electron.ExitCode)(见 smoke.log)" }
    Start-Sleep -Milliseconds 500
  }
  $ev = Get-Content $rectJson -Raw | ConvertFrom-Json
  Log "evidence => rect=$($ev.rect.x),$($ev.rect.y) $($ev.rect.w)x$($ev.rect.h) presented=$($ev.presented) centerRgba=$($ev.centerRgba -join ',') framePath=$($ev.framePath) device=$($ev.deviceName)"
  if ($ev.presented -lt 3) { throw "presented=$($ev.presented) < 3" }
  # G-F1-11:桌面腿必须跑零拷贝档(VK import 直渲共享纹理),readback_upload 如实 FAIL
  if ($ev.framePath -ne "zero_copy") { throw "framePath=$($ev.framePath) ≠ zero_copy(零拷贝未生效)" }

  # 非占位断言:锚点像素 ≠ 视口底色(23,24,29) —— 中心应真命中立方体
  $bg = @(23, 24, 29)
  $cr = $ev.centerRgba
  if ($cr[0] -eq $bg[0] -and $cr[1] -eq $bg[1] -and $cr[2] -eq $bg[2]) { throw "锚点像素=底色,场景未渲出(占位疑云)" }

  Log "== OS 级截屏比对(DWM 合成含原生子窗口) =="
  Add-Type -AssemblyName System.Drawing
  $capX = $ev.rect.x + [int]($ev.rect.w / 2)
  $capY = $ev.rect.y + [int]($ev.rect.h / 2)
  # DWM 对新嵌入子窗口首帧合成存在实测滞后(2026-08-17 首跑 500ms 截到 web 底色 flake);
  # 有限重试消化合成延迟,判据本身不放宽(±3),每次尝试如实留痕。
  $tol = 3
  $matched = $false
  foreach ($attempt in 1..4) {
    Start-Sleep -Milliseconds 800
    $bmp = New-Object System.Drawing.Bitmap 16, 16
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($capX - 8, $capY - 8, 0, 0, (New-Object System.Drawing.Size 16, 16))
    $px = $bmp.GetPixel(8, 8)
    $g.Dispose(); $bmp.Dispose()
    Log "screen px@($capX,$capY) attempt#$attempt = R$($px.R) G$($px.G) B$($px.B) vs readback R$($cr[0]) G$($cr[1]) B$($cr[2])"
    if ([Math]::Abs($px.R - $cr[0]) -le $tol -and [Math]::Abs($px.G - $cr[1]) -le $tol -and [Math]::Abs($px.B - $cr[2]) -le $tol) {
      $matched = $true; break
    }
  }
  if (-not $matched) {
    throw "OS 截屏锚点像素与 readback 帧不一致(容差 ±$tol,4 次尝试)——presenter 呈现内容非引擎帧"
  }
  Log "锚点像素一致(±$tol)PASS:presenter 呈现 = 引擎 readback 帧"

  New-Item -ItemType File -Force $doneFlag | Out-Null
  Log "== 等 electron 正常退出 =="
  [void]$electron.WaitForExit(60000)
  if (-not $electron.HasExited) { $electron.Kill(); throw "electron 60s 未退出" }
  if ($electron.ExitCode -ne 0) { throw "electron exit=$($electron.ExitCode)" }
  Log "G-F1-9 桌面腿 PASS(presenter 嵌入 Electron 视口区 + OS 截屏锚点一致 + 生命周期退出码 0)"
  exit 0
} catch {
  Log "FAIL: $_"
  try {
    Add-Type -AssemblyName System.Drawing
    $vs = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bmp = New-Object System.Drawing.Bitmap $vs.Width, $vs.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($vs.Left, $vs.Top, 0, 0, $bmp.Size)
    $dbg = "evidence\f1-w2-desktop-fail-$ts.png"
    $bmp.Save($dbg, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    Log "失败全屏截图: $dbg"
  } catch { Log "失败截图异常: $_" }
  New-Item -ItemType File -Force $doneFlag -ErrorAction SilentlyContinue | Out-Null
  exit 1
} finally {
  if ($electron -and -not $electron.HasExited) { try { $electron.Kill() } catch {} }
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
