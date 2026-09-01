# F2 wave.4 材质与贴图冒烟(G-F2-4 栈级):png 解码尺寸 / texture_process resize /
# material_create + 纹理引用边 / MeshRenderer.material 绑定 GUID + 场景落盘引用不断链 / mesh_inspect。
# 前提:target\debug\forge-agentd.exe、gateway-go\forge-gateway.exe、asset-pipeline-mcp.exe 已构建;
#        projects\demo 已有至少一个 mesh 资产(wave.1/2 冒烟已导入 tri_min.gltf)。
# 注意:本冒烟会在 demo 项目内沉淀真实资产(Textures/w4_dot*.png、Materials/w4_mat.rxmat)
#       并保存 Main.rxscene(含一个绑定网格+材质的实体)——demo fixture 有意充实,非污染。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f2-w4-material-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f2-w4-material-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
function McpCall($tool, $arguments) {
  $body = @{ tool = $tool; arguments = $arguments } | ConvertTo-Json -Depth 10 -Compress
  # WebClient 双向 UTF-8(请求体路径含非 ASCII;响应 edge_type 含 "→",
  # Invoke-WebRequest 缺省按 ISO-8859-1 解码会把 "material→texture" 变成乱码导致比较失败——实测踩中)。
  $wc = New-Object System.Net.WebClient
  $wc.Encoding = [Text.Encoding]::UTF8
  $wc.Headers.Add("Content-Type", "application/json; charset=utf-8")
  $wc.Headers.Add("Authorization", "Bearer $script:jwt")
  try {
    $respText = $wc.UploadString("http://127.0.0.1:8102/api/forge/mcp/call", "POST", $body)
  } catch [System.Net.WebException] {
    $resp = $_.Exception.Response
    $errBody = ""
    if ($resp) { $sr = New-Object System.IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8); $errBody = $sr.ReadToEnd() }
    throw "$tool 失败 body=$errBody 请求体=$body"
  } finally { $wc.Dispose() }
  $outer = $respText | ConvertFrom-Json
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
  $script:jwt = node -e "const c=require('crypto');const b=o=>Buffer.from(JSON.stringify(o)).toString('base64url');const h=b({alg:'HS256',typ:'JWT'}),p=b({sub:'f2w4',exp:Math.floor(Date.now()/1000)+3600});const s=c.createHmac('sha256','forge-dev-secret').update(h+'.'+p).digest('base64url');console.log(h+'.'+p+'.'+s)"

  # ── 1. png 导入解码尺寸(G-F2-4:png/jpg 导入解码尺寸正确) ──
  Log "== 导入 2x2 png,校验解码尺寸 =="
  # 真实 PNG 由 System.Drawing 现编(硬编码字节数组曾踩"头合法但 IDAT 损坏":image_dimensions 只读头放过,全量解码才暴露)。
  Add-Type -AssemblyName System.Drawing
  $tmpSrc = Join-Path $env:TEMP "f2w4_dot.png"
  $bmp = New-Object System.Drawing.Bitmap 2, 2
  $bmp.SetPixel(0, 0, [System.Drawing.Color]::FromArgb(255, 255, 0, 0))
  $bmp.SetPixel(1, 0, [System.Drawing.Color]::FromArgb(255, 0, 255, 0))
  $bmp.SetPixel(0, 1, [System.Drawing.Color]::FromArgb(255, 0, 0, 255))
  $bmp.SetPixel(1, 1, [System.Drawing.Color]::FromArgb(255, 255, 255, 0))
  $bmp.Save($tmpSrc, [System.Drawing.Imaging.ImageFormat]::Png)
  $bmp.Dispose()
  $imp = McpCall "mcp__asset-pipeline__asset_import" @{ sourcePaths = @($tmpSrc); destFolder = "Textures" }
  if ($imp.failed.Count -gt 0) { throw "png 导入失败: $($imp.failed[0].error)" }
  $tex = $imp.imported[0]
  if ($tex.type -ne "texture") { throw "type=$($tex.type) ≠ texture" }
  if ($tex.width -ne 2 -or $tex.height -ne 2) { throw "解码尺寸错误: $($tex.width)x$($tex.height) ≠ 2x2" }
  Log "png 导入解码尺寸 PASS: $($tex.assetPath) $($tex.width)x$($tex.height) guid=$($tex.guid)"

  # ── 2. texture_process resize 实测(G-F2-4:resize 实测尺寸正确) ──
  Log "== texture_process resize 2x2 → 4x4 =="
  $proc = McpCall "mcp__asset-pipeline__texture_process" @{ assetPath = $tex.assetPath; ops = @{ resize = @{ width = 4; height = 4 } } }
  if ($proc.error) { throw "texture_process 失败: $($proc.message)" }
  if ($proc.width -ne 4 -or $proc.height -ne 4) { throw "resize 尺寸错误: $($proc.width)x$($proc.height) ≠ 4x4" }
  if ($proc.outputAssetPath -notmatch '@4x4\.png$') { throw "输出命名不符: $($proc.outputAssetPath)" }
  if ($proc.bytes -le 0) { throw "输出字节数异常: $($proc.bytes)" }
  Log "texture_process resize PASS: $($proc.outputAssetPath) $($proc.width)x$($proc.height) $($proc.bytes)B"
  $list1 = McpCall "mcp__asset-pipeline__asset_list" @{}
  if (-not ($list1.assets | Where-Object { $_.path -eq $proc.outputAssetPath })) { throw "输出资产未登记: $($proc.outputAssetPath)" }
  Log "输出资产已登记进 Content PASS"

  # ── 3. material_create + 纹理引用边(G-F2-4:材质资产创建 .rxmat) ──
  Log "== material_create(textures.albedo = 贴图 GUID) =="
  $mat = McpCall "mcp__asset-pipeline__material_create" @{
    name = "w4_mat"
    params = @{ baseColor = @(1.0, 0.8, 0.6, 1.0); roughness = 0.7 }
    textures = @{ albedo = $tex.guid }
  }
  if ($mat.error) { throw "material_create 失败: $($mat.message)" }
  if ($mat.assetPath -ne "Materials/w4_mat.rxmat") { throw "材质路径不符: $($mat.assetPath)" }
  if (-not $mat.guid) { throw "材质 GUID 为空" }
  if ($mat.textureRefs.Count -ne 1 -or $mat.textureRefs[0].guid -ne $tex.guid) { throw "textureRefs 不符" }
  Log "材质创建 PASS: $($mat.assetPath) guid=$($mat.guid)"
  $matRefs = McpCall "mcp__asset-pipeline__asset_refs" @{ assetPath = $mat.assetPath; direction = "refs" }
  $mtEdge = $matRefs.edges | Where-Object { $_.to -eq $tex.guid -and $_.type -eq "material→texture" }
  if (-not $mtEdge) { throw "缺 material→texture 引用边" }
  Log "material→texture 引用边 PASS"

  # ── 4. MeshRenderer.material 绑定材质 GUID + 场景落盘引用不断链(G-F2-4) ──
  Log "== entity_create 绑定 material GUID + scene_save + referencedBy 校验 =="
  $meshAsset = $list1.assets | Where-Object { $_.type -eq "mesh" } | Select-Object -First 1
  if (-not $meshAsset) { throw "demo 项目无 mesh 资产(先跑 wave.1/2 冒烟)" }
  $ent = McpCall "mcp__engine-scene__entity_create" @{
    name = "W4Tri"
    translation = @(0, 0, 0)
    components = @(@{ type = "MeshRenderer"; enabled = $true; props = @{ mesh = $meshAsset.guid; material = $mat.guid } })
  }
  if (-not $ent.id) { throw "entity_create 失败: $($ent | ConvertTo-Json -Compress)" }
  Log "实体创建 PASS: id=$($ent.id) mesh=$($meshAsset.guid) material=$($mat.guid)"
  $sceneAbs = (Resolve-Path "projects\demo\Content\Scenes\Main.rxscene").Path
  $save = McpCall "mcp__engine-scene__scene_save" @{ path = $sceneAbs }
  if ($save.error) { throw "scene_save 失败: $($save.message)" }
  Log "场景已落盘: $sceneAbs"
  $inbound = McpCall "mcp__asset-pipeline__asset_refs" @{ assetPath = $mat.assetPath; direction = "referencedBy" }
  $smEdge = $inbound.edges | Where-Object { $_.type -eq "scene→material" }
  if (-not $smEdge) { throw "缺 scene→material 边(场景未引用材质 GUID)" }
  Log "scene→material 引用边 PASS(绑定 GUID 落盘不断链)"

  # ── 5. mesh_inspect 统计(05 §3 返回面) ──
  Log "== mesh_inspect 网格统计 =="
  $insp = McpCall "mcp__asset-pipeline__mesh_inspect" @{ assetPath = $meshAsset.path }
  if ($insp.error) { throw "mesh_inspect 失败: $($insp.message)" }
  if ($insp.vertices -ne 3 -or $insp.triangles -ne 1) { throw "统计错误: v=$($insp.vertices) t=$($insp.triangles) ≠ 3/1" }
  if (-not $insp.bounds) { throw "bounds 缺失" }
  Log "mesh_inspect PASS: v=$($insp.vertices) t=$($insp.triangles) meshlets=$($insp.meshlets) lods=$($insp.lods) bounds=[$($insp.bounds.min -join ',')]→[$($insp.bounds.max -join ',')]"

  Log "F2 wave.4 材质与贴图冒烟 PASS"
  exit 0
} catch {
  Log "FAIL: $_"
  exit 1
} finally {
  foreach ($p in $procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
}
