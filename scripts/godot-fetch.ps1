<#
.SYNOPSIS
  下载并校验 GODOT_PIN.json 钉定的官方 Godot 构建,落到 vendor/godot/<tag>/(D-043)。

.DESCRIPTION
  - 事实源 = 仓库根 GODOT_PIN.json;校验和取同一 release 的 SHA512-SUMS.txt,
    SHA-512 不匹配即失败退出(不静默、不重试成功假象)。
  - 幂等:产物已存在且校验通过则跳过下载。
  - 编辑器 zip 解出 editor/(Godot_v<tag>_win64.exe + _console.exe);
    -Templates 时下载导出模板 .tpz 并只解出 Windows x86_64 运行时模板到 templates/。
  - 结束写 vendor/godot/<tag>/fetch-manifest.json(文件、SHA-512、路径),供 pin-check 与打包消费。

.PARAMETER Templates
  同时下载导出模板(.tpz,约 1 GB 以上)。

.PARAMETER Force
  忽略已存在产物,重新下载。
#>
param(
    [switch]$Templates,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue' # Invoke-WebRequest 进度条会把大文件下载拖慢一个数量级
Add-Type -AssemblyName System.IO.Compression.FileSystem

$root = Split-Path -Parent $PSScriptRoot
$pinPath = Join-Path $root 'GODOT_PIN.json'
if (-not (Test-Path $pinPath)) { throw "GODOT_PIN.json 不存在: $pinPath" }
$pin = Get-Content $pinPath -Raw -Encoding UTF8 | ConvertFrom-Json
$tag = $pin.godot.tag
if ($tag -notmatch '^\d+\.\d+(\.\d+)?-[a-z0-9]+$') { throw "GODOT_PIN.json godot.tag 非法: $tag" }

$base = "https://github.com/godotengine/godot-builds/releases/download/$tag"
$dest = Join-Path $root "vendor\godot\$tag"
$dl = Join-Path $dest 'downloads'
New-Item -ItemType Directory -Force $dl | Out-Null

function Invoke-Download([string]$Name, [string]$OutFile) {
    $tmp = "$OutFile.part"
    if (Test-Path $tmp) { Remove-Item $tmp -Force }
    Write-Host "下载 $Name ..."
    Invoke-WebRequest -Uri "$base/$Name" -OutFile $tmp -UseBasicParsing
    Move-Item $tmp $OutFile -Force
}

# 1) 校验和清单(每次都取最新,保证与 release 一致)
$sumsFile = Join-Path $dl $pin.godot.assets.checksums
Invoke-Download $pin.godot.assets.checksums $sumsFile
$sums = @{}
foreach ($line in Get-Content $sumsFile) {
    if ($line -match '^([0-9a-fA-F]{128})\s+\*?(.+)$') { $sums[$Matches[2].Trim()] = $Matches[1].ToLowerInvariant() }
}

function Get-VerifiedAsset([string]$Name) {
    if (-not $sums.ContainsKey($Name)) { throw "SHA512-SUMS.txt 中无 $Name(pin 与 release 不一致)" }
    $file = Join-Path $dl $Name
    $want = $sums[$Name]
    if ((Test-Path $file) -and -not $Force) {
        $have = (Get-FileHash -Algorithm SHA512 $file).Hash.ToLowerInvariant()
        if ($have -eq $want) { Write-Host "已存在且校验通过: $Name"; return $file }
        Write-Host "已存在但校验不符,重新下载: $Name"
    }
    Invoke-Download $Name $file
    $have = (Get-FileHash -Algorithm SHA512 $file).Hash.ToLowerInvariant()
    if ($have -ne $want) {
        Remove-Item $file -Force
        throw "SHA-512 不匹配: $Name`n  期望 $want`n  实际 $have"
    }
    Write-Host "校验通过: $Name"
    return $file
}

function Expand-Selected([string]$Zip, [string]$OutDir, [string]$Pattern) {
    New-Item -ItemType Directory -Force $OutDir | Out-Null
    $archive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
    try {
        $hit = @()
        foreach ($entry in $archive.Entries) {
            if ($entry.FullName -match $Pattern) {
                $target = Join-Path $OutDir $entry.Name
                [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
                $hit += $target
            }
        }
        if ($hit.Count -eq 0) { throw "$Zip 中没有匹配 $Pattern 的条目" }
        return $hit
    } finally { $archive.Dispose() }
}

$manifest = [ordered]@{ schema = 'forge.godot_fetch.v1'; tag = $tag; fetched_at = (Get-Date).ToString('o'); files = @() }

# 2) 编辑器(开发期运行时:--path 直跑运行时工程)
$editorZip = Get-VerifiedAsset $pin.godot.assets.editor_win64
$editorFiles = Expand-Selected $editorZip (Join-Path $dest 'editor') '^Godot_v[^/]+_win64(_console)?\.exe$'
$manifest.files += [ordered]@{ asset = $pin.godot.assets.editor_win64; sha512 = $sums[$pin.godot.assets.editor_win64]; extracted = $editorFiles }

# 3) 导出模板(打包期运行时,可选)
if ($Templates) {
    $tpz = Get-VerifiedAsset $pin.godot.assets.export_templates
    $tplFiles = Expand-Selected $tpz (Join-Path $dest 'templates') '^templates/(windows_(release|debug)_x86_64(_console)?\.exe|version\.txt)$'
    $manifest.files += [ordered]@{ asset = $pin.godot.assets.export_templates; sha512 = $sums[$pin.godot.assets.export_templates]; extracted = $tplFiles }
}

$manifest | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $dest 'fetch-manifest.json') -Encoding UTF8
Write-Host "完成: $dest"
