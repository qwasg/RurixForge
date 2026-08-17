# F1 wave.2 Viewport 冒烟(G-F1-6/7/8 栈级 + desktop 截图证据;G-F1-9 共享纹理段独立标注)。
# 前提:target\debug\forge-agentd.exe 与 gateway-go\forge-gateway.exe 已构建(cargo/go build)。
# 用法: pwsh scripts\f1-w2-viewport-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f1-w2-viewport-smoke-$ts.log"
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
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f1w2',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  Log "== 布场:3 立方体(平移/旋转/缩放各异) =="
  McpCall "mcp__engine-scene__scene_new" @{ name = "f1-w2" } | Out-Null
  $ops = @(
    @{ op = "create"; name = "cube-a"; translation = @(0.0, 0.5, 0.0); components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = "cube"; material = "a" } }) },
    @{ op = "create"; name = "cube-b"; translation = @(1.6, 0.5, 0.4); rotation = @(0.0, 0.3826834, 0.0, 0.9238795); components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = "cube"; material = "b" } }) },
    @{ op = "create"; name = "cube-c"; translation = @(-1.6, 0.9, -0.4); scale = @(1.0, 1.8, 1.0); components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = "cube"; material = "c" } }) }
  )
  $batch = McpCall "mcp__engine-scene__entity_batch_apply" @{ ops = $ops }
  if ($batch.applied -ne 3) { throw "batch applied=$($batch.applied) ≠ 3" }
  $cam = McpCall "mcp__engine-scene__viewport_set_camera" @{ target = @(0.0, 0.6, 0.0); yaw = 30.0; pitch = 24.0; dist = 6.5 }
  Log "camera => $($cam | ConvertTo-Json -Compress)"

  Log "== G-F1-6 栈级:GPU 帧 + 两帧确定性 + 移动帧变 =="
  $f1 = McpCall "mcp__engine-scene__viewport_frame" @{ width = 128; height = 96 }
  if ($f1.draws -ne 3) { throw "draws=$($f1.draws) ≠ 3" }
  if ([int]$f1.nonZeroPixels -le 0) { throw "nonZeroPixels=0(GPU 帧全空)" }
  Log "frame1: device=$($f1.deviceName) draws=3 nonzero=$($f1.nonZeroPixels)"
  $f2 = McpCall "mcp__engine-scene__viewport_frame" @{ width = 128; height = 96 }
  if ($f1.pixelsB64 -ne $f2.pixelsB64) { throw "同场景两帧非逐字节一致" }
  Log "两帧逐字节一致 PASS"
  $list = McpCall "mcp__engine-scene__entity_list" @{}
  $cubeA = @($list.entities | Where-Object { $_.name -eq "cube-a" })[0]
  McpCall "mcp__engine-scene__transform_set" @{ id = $cubeA.id; translation = @(50.0, 0.5, 0.0) } | Out-Null
  $f3 = McpCall "mcp__engine-scene__viewport_frame" @{ width = 128; height = 96 }
  if ($f1.pixelsB64 -eq $f3.pixelsB64) { throw "实体移走后帧未变" }
  McpCall "mcp__engine-scene__transform_set" @{ id = $cubeA.id; translation = @(0.0, 0.5, 0.0) } | Out-Null
  Log "移动后帧变 PASS"

  Log "== G-F1-7 栈级:点选命中/未命中 =="
  $pick = McpCall "mcp__engine-scene__viewport_pick" @{ x = 64.0; y = 48.0; width = 128; height = 96 }
  if (-not $pick.hit) { throw "视口中心应命中 cube-a" }
  if ($pick.entityId -ne $cubeA.id) { throw "命中实体错: $($pick.entityId) ≠ $($cubeA.id)" }
  $miss = McpCall "mcp__engine-scene__viewport_pick" @{ x = 2.0; y = 2.0; width = 128; height = 96 }
  if ($miss.hit) { throw "角落应未命中" }
  Log "pick center hit=$($pick.entityId) / corner miss PASS"

  Log "== G-F1-8 栈级:相机 orbit 帧变 + batchSet/undo 回滚 =="
  McpCall "mcp__engine-scene__viewport_set_camera" @{ yaw = 90.0 } | Out-Null
  $f4 = McpCall "mcp__engine-scene__viewport_frame" @{ width = 128; height = 96 }
  if ($f1.pixelsB64 -eq $f4.pixelsB64) { throw "orbit 60° 后帧未变" }
  McpCall "mcp__engine-scene__viewport_set_camera" @{ yaw = 30.0 } | Out-Null
  Log "orbit 帧变 PASS"
  $before = (McpCall "mcp__engine-scene__transform_get" @{ id = $cubeA.id }).transform
  McpCall "mcp__engine-scene__transform_batch_set" @{ items = @(@{ id = $cubeA.id; translation = @(2.0, 0.5, 0.0) }) } | Out-Null
  McpCall "mcp__engine-scene__edit_undo" @{} | Out-Null
  $after = (McpCall "mcp__engine-scene__transform_get" @{ id = $cubeA.id }).transform
  if (($after.translation -join ',') -ne ($before.translation -join ',')) { throw "undo 后 translation 未回滚" }
  Log "gizmo 提交面(batchSet→undo 回滚)PASS"

  Log "== G-F1-9 共享纹理:presenter 跨进程呈现 + 句柄生命周期 =="
  if (-not (Test-Path "target\debug\viewport-presenter.exe")) { throw "viewport-presenter 未构建" }
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = "$root\target\debug\viewport-presenter.exe"
  $psi.Arguments = "--selftest --w 128 --h 96 --expect-frames 3"
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.CreateNoWindow = $true
  $proc = New-Object System.Diagnostics.Process
  $proc.StartInfo = $psi
  [void]$proc.Start()
  try {
    $share = McpCall "mcp__engine-scene__viewport_share_open" @{ pid = $proc.Id; width = 128; height = 96 }
    Log "shareOpen => tex=$($share.texHandle) fence=$($share.fenceHandle) 128x96"
    $proc.StandardInput.WriteLine("bind $($share.texHandle) $($share.fenceHandle) 128 96")
    $proc.StandardInput.Flush()
    # 渲 4 帧(expect 3,留 1 帧冗余);每帧 RPC 内同步写共享纹理 + fence 递增
    1..4 | ForEach-Object {
      McpCall "mcp__engine-scene__viewport_frame" @{ width = 128; height = 96 } | Out-Null
      Start-Sleep -Milliseconds 120
    }
    [void]$proc.WaitForExit(20000)
    if (-not $proc.HasExited) { $proc.Kill(); throw "presenter selftest 超时" }
    $pout = $proc.StandardOutput.ReadToEnd()
    $perr = $proc.StandardError.ReadToEnd()
    Log "presenter stdout: $($pout.Trim())"
    if ($perr.Trim()) { Log "presenter stderr: $($perr.Trim())" }
    if ($proc.ExitCode -ne 0) { throw "presenter selftest exit=$($proc.ExitCode)" }
    # 句柄生命周期:关闭幂等(两次 close 均成功)
    McpCall "mcp__engine-scene__viewport_share_close" @{} | Out-Null
    McpCall "mcp__engine-scene__viewport_share_close" @{} | Out-Null
    Log "G-F1-9 PASS(跨进程共享纹理呈现 presented>=3 + 句柄关闭幂等)"
  } finally {
    try { if (-not $proc.HasExited) { $proc.Kill() } } catch {}
    try { McpCall "mcp__engine-scene__viewport_share_close" @{} | Out-Null } catch {}
  }

  Log "== desktop 截图证据(editor 场景,GPU 帧上屏) =="
  $env:FORGE_SMOKE_SCENARIO = "editor"
  pnpm --filter @forge/desktop smoke | Tee-Object -Variable smokeOut
  if ($LASTEXITCODE -ne 0) { throw "desktop smoke 失败" }
  Remove-Item Env:FORGE_SMOKE_SCENARIO -ErrorAction SilentlyContinue
  $shot = Get-ChildItem "apps\desktop\evidence\desktop-smoke-editor-*.png" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
  Log "截图: $($shot.Name) ($($shot.Length) bytes)"

  Log "F1 wave.2 视口冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
