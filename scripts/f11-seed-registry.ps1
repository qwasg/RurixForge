# F11 官方源种子生成器(D-025)。
#
# 从仓内真实资产与 registry/_sources/ 生成默认官方源 registry/:
#   registry/index.json                       源元信息 + PackageSummary 列表
#   registry/packages/<pkgId>/<version>.json  PackageManifest(files[] 含逐文件 sha256)
#   registry/blobs/<前2位>/<sha256>           内容寻址 blob
#
# 幂等:重跑覆盖生成物,sha256 由文件内容实算(不硬编码),因此 manifest 与 blob 天然一致。
# 不打包归档(D-F11-A:内容寻址逐文件,零解压依赖)。

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$registry = Join-Path $repo 'registry'
$content = Join-Path $repo 'projects/demo/Content'
$sources = Join-Path $registry '_sources'

$blobsDir = Join-Path $registry 'blobs'
$pkgsDir = Join-Path $registry 'packages'
foreach ($d in @($blobsDir, $pkgsDir)) {
  if (Test-Path $d) { Remove-Item -Recurse -Force $d }
}
New-Item -ItemType Directory -Force -Path $blobsDir | Out-Null
New-Item -ItemType Directory -Force -Path $pkgsDir | Out-Null

$now = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
$publisher = [ordered]@{ id = 'rurixforge'; name = 'RurixForge 官方'; url = 'https://github.com/rurixforge' }

# 落 blob 并返回 {path, sha256, size}
function Add-Blob {
  param([string]$SrcPath, [string]$RelPath)
  if (-not (Test-Path $SrcPath)) { throw "源文件不存在: $SrcPath" }
  $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $SrcPath).Hash.ToLowerInvariant()
  $size = (Get-Item -LiteralPath $SrcPath).Length
  $shard = Join-Path $blobsDir $hash.Substring(0, 2)
  New-Item -ItemType Directory -Force -Path $shard | Out-Null
  Copy-Item -LiteralPath $SrcPath -Destination (Join-Path $shard $hash) -Force
  return [ordered]@{ path = $RelPath; sha256 = $hash; size = $size }
}

# 无 BOM UTF-8 写 JSON(Rust serde_json 读 BOM 会失败)
function Write-Json {
  param([string]$Path, $Obj)
  $dir = Split-Path -Parent $Path
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $json = $Obj | ConvertTo-Json -Depth 12
  [IO.File]::WriteAllText($Path, $json, (New-Object Text.UTF8Encoding $false))
}

$summaries = @()

function New-Package {
  param(
    [string]$Id, [string]$Name, [string]$Version, [string]$Kind, [string]$Description,
    [string[]]$Tags, [string]$LicenseId, [string]$LicenseUrl, [array]$Files
  )
  $license = [ordered]@{ id = $LicenseId; url = $LicenseUrl }
  $pricing = [ordered]@{ amount = 0; currency = 'CNY' }
  $manifest = [ordered]@{
    id            = $Id
    name          = $Name
    version       = $Version
    kind          = $Kind
    description   = $Description
    publisher     = $publisher
    tags          = $Tags
    license       = $license
    pricing       = $pricing
    engineVersion = '>=0.1.0'
    dependencies  = @()
    files         = $Files
    preview       = [ordered]@{ thumbnail = $null; screenshots = @() }
    createdAt     = $now
    updatedAt     = $now
  }
  Write-Json -Path (Join-Path $pkgsDir "$Id/$Version.json") -Obj $manifest
  $script:summaries += [ordered]@{
    id            = $Id
    name          = $Name
    kind          = $Kind
    description   = $Description
    latestVersion = $Version
    tags          = $Tags
    license       = $license
    pricing       = $pricing
    publisher     = $publisher
    thumbnail     = $null
  }
  Write-Host ("  + {0}@{1} ({2} 文件)" -f $Id, $Version, $Files.Count)
}

Write-Host '生成官方源种子...'

# 1) 入门示例道具包:mesh + texture + material 三类齐,验证多类型分流落地
$starter = @(
  (Add-Blob -SrcPath (Join-Path $content 'Meshes/f5w2_chair.gltf') -RelPath 'starter_chair.gltf'),
  (Add-Blob -SrcPath (Join-Path $content 'Textures/f2w4_dot.png') -RelPath 'starter_dot.png'),
  (Add-Blob -SrcPath (Join-Path $content 'Materials/JourneyBoxRed.rxmat') -RelPath 'starter_red.rxmat')
)
New-Package -Id 'forge.starter-props' -Name '入门示例道具包' -Version '1.0.0' -Kind 'asset-pack' `
  -Description '三类资产各一件(网格/贴图/材质),用于验证商店安装链路与按扩展名分流落地。' `
  -Tags @('示例', '道具', '入门') -LicenseId 'CC0-1.0' -LicenseUrl 'https://creativecommons.org/publicdomain/zero/1.0/' `
  -Files $starter

# 2) 木质 PBR 贴图包:单类型多文件,体积接近真实素材包。
# 落地名带 pbr_ 前缀:demo 项目本就有同名 wood_albedo.png 等,同名会被导入链按 reimport
# 复用 GUID 并覆写 provenance,把用户原有资产的溯源改掉。包内命名与项目现有资产隔开。
$wood = @(
  (Add-Blob -SrcPath (Join-Path $content 'Textures/wood_albedo.png') -RelPath 'pbr_wood_albedo.png'),
  (Add-Blob -SrcPath (Join-Path $content 'Textures/wood_normal.png') -RelPath 'pbr_wood_normal.png'),
  (Add-Blob -SrcPath (Join-Path $content 'Textures/wood_roughness.png') -RelPath 'pbr_wood_roughness.png')
)
New-Package -Id 'forge.wood-pbr' -Name '木质 PBR 贴图组' -Version '1.0.0' -Kind 'asset-pack' `
  -Description '一套木质 PBR 贴图(albedo / normal / roughness),可直接挂到材质三槽。' `
  -Tags @('贴图', 'PBR', '木质') -LicenseId 'CC-BY-4.0' -LicenseUrl 'https://creativecommons.org/licenses/by/4.0/' `
  -Files $wood

# 2b) 木质包 1.1.0:同包多版本,供更新检查(check_updates)验收
New-Package -Id 'forge.wood-pbr' -Name '木质 PBR 贴图组' -Version '1.1.0' -Kind 'asset-pack' `
  -Description '一套木质 PBR 贴图(albedo / normal / roughness),可直接挂到材质三槽。1.1.0 修正 roughness 通道。' `
  -Tags @('贴图', 'PBR', '木质') -LicenseId 'CC-BY-4.0' -LicenseUrl 'https://creativecommons.org/licenses/by/4.0/' `
  -Files $wood

# 3) skill 包:分发物仅 SKILL.md 文本(D-025 不执行第三方代码红线)
$skillFiles = @(
  (Add-Blob -SrcPath (Join-Path $sources 'scene-audit/SKILL.md') -RelPath 'SKILL.md')
)
New-Package -Id 'forge.skill-scene-audit' -Name '技能:场景健康度审计' -Version '1.0.0' -Kind 'skill' `
  -Description '交付前对场景做只读体检,产出 blocker/warn/info 三级问题清单。安装后出现在 Skill 管理与 Composer 技能菜单中。' `
  -Tags @('技能', '场景', '审计') -LicenseId 'CC0-1.0' -LicenseUrl 'https://creativecommons.org/publicdomain/zero/1.0/' `
  -Files $skillFiles

# summaries 去重:同 id 多版本只在索引里留最新一条
$latest = [ordered]@{}
foreach ($s in $summaries) { $latest[$s.id] = $s }
$indexPkgs = @($latest.Values)

$index = [ordered]@{
  name         = 'RurixForge 官方源'
  protocol     = 1
  packageCount = $indexPkgs.Count
  updatedAt    = $now
  packages     = $indexPkgs
}
Write-Json -Path (Join-Path $registry 'index.json') -Obj $index

$blobCount = (Get-ChildItem -Recurse -File $blobsDir).Count
Write-Host ''
Write-Host ("完成:{0} 个包 / {1} 个 blob(内容寻址已去重)" -f $indexPkgs.Count, $blobCount)
Write-Host ("索引:{0}" -f (Join-Path $registry 'index.json'))
