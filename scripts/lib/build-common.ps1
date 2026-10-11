<#
  setup.ps1 与 package-windows.ps1 共用的小工具(dot-source 引入)。
  兼容 Windows PowerShell 5.1:不用 &&、??、三元运算符。
#>

$script:ForgeRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$script:PnpmVersion = ((Get-Content (Join-Path $script:ForgeRoot 'package.json') -Raw | ConvertFrom-Json).packageManager -replace '^pnpm@', '')

function Write-Step([string]$Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Note([string]$Message) {
    Write-Host "    $Message" -ForegroundColor DarkGray
}

function Write-Warn([string]$Message) {
    Write-Host "    [!] $Message" -ForegroundColor Yellow
}

function Test-Command([string]$Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

# 运行外部程序并检查退出码。不重定向 stderr:5.1 下 2>&1 会把 cargo 的警告变成终止错误。
function Invoke-Native {
    param(
        [Parameter(Mandatory)][string]$File,
        [string[]]$Arguments = @(),
        [string]$WorkingDirectory
    )
    if ($WorkingDirectory) { Push-Location $WorkingDirectory }
    try {
        & $File @Arguments
        $code = $LASTEXITCODE
    } finally {
        if ($WorkingDirectory) { Pop-Location }
    }
    if ($code -ne 0) { throw "命令失败(退出码 $code):$File $($Arguments -join ' ')" }
}

# 安装/升级工具后,把注册表里最新的 PATH 合并进当前会话(新开终端才会自动生效)。
function Update-SessionPath {
    $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
    $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    $parts = @($machine, $user, $cargoBin, $env:Path) -join ';'
    $seen = @{}
    $merged = foreach ($p in ($parts -split ';')) {
        $t = $p.Trim()
        if ($t -and -not $seen.ContainsKey($t.ToLowerInvariant())) { $seen[$t.ToLowerInvariant()] = $true; $t }
    }
    $env:Path = $merged -join ';'
}

function Get-NodeMajor {
    if (-not (Test-Command node)) { return 0 }
    $v = (& node --version) -replace '^v', ''
    return [int]($v.Split('.')[0])
}

# pnpm:PATH 上有就用;否则经 corepack(Node 24 及以前自带),再不行用 npx 拉固定版本。
function Get-PnpmLauncher {
    if (Test-Command pnpm) { return , @('pnpm') }
    if (Test-Command corepack) { return , @('corepack', "pnpm@$script:PnpmVersion") }
    return , @('npx', '--yes', "pnpm@$script:PnpmVersion")
}

# 参数以数组传入(Invoke-Pnpm @('--filter', '@forge/host', 'build')),免得 -- 开头的参数被当成函数参数解析。
function Invoke-Pnpm([string[]]$Arguments) {
    # corepack 首次下载 pnpm 时会交互确认;脚本里一律直接下载。
    $env:COREPACK_ENABLE_DOWNLOAD_PROMPT = '0'
    $launcher = Get-PnpmLauncher
    $file = $launcher[0]
    $prefix = @()
    if ($launcher.Count -gt 1) { $prefix = $launcher[1..($launcher.Count - 1)] }
    Invoke-Native -File $file -Arguments ($prefix + $Arguments) -WorkingDirectory $script:ForgeRoot
}

function Get-VsWhere {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $vswhere) { return $vswhere }
    return $null
}

# 带 C++ x64 工具链的 Visual Studio / Build Tools 安装目录;没有返回 $null。
function Find-VisualStudio {
    $vswhere = Get-VsWhere
    if (-not $vswhere) { return $null }
    $path = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($path) { return ($path | Select-Object -First 1) }
    return $null
}

# cmake:Visual Studio 自带的优先,其次 PATH 上的;都没有返回 $null。
function Find-CMake {
    $vs = Find-VisualStudio
    if ($vs) {
        $bundled = Join-Path $vs 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe'
        if (Test-Path -LiteralPath $bundled) { return $bundled }
    }
    $cmd = Get-Command cmake -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    return $null
}

# VC++ 运行库(app-local 部署用):<VS>\VC\Redist\MSVC\<最新版>\x64\Microsoft.VC14x.CRT。
function Get-VcRuntimeDir {
    $vs = Find-VisualStudio
    if (-not $vs) { return $null }
    $redist = Join-Path $vs 'VC\Redist\MSVC'
    if (-not (Test-Path -LiteralPath $redist)) { return $null }
    $versions = Get-ChildItem -LiteralPath $redist -Directory | Where-Object { $_.Name -match '^\d+(\.\d+)+$' } |
        Sort-Object { [version]$_.Name } -Descending
    foreach ($v in $versions) {
        $crt = Get-ChildItem -LiteralPath (Join-Path $v.FullName 'x64') -Directory -Filter 'Microsoft.VC14*.CRT' -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($crt -and (Test-Path -LiteralPath (Join-Path $crt.FullName 'vcruntime140.dll'))) { return $crt.FullName }
    }
    return $null
}

# 把 git 跟踪的文件(按路径前缀)原样复制到目标根下,保持相对路径。未跟踪的本地文件一律不带。
function Copy-TrackedFiles {
    param(
        [Parameter(Mandatory)][string[]]$Paths,
        [Parameter(Mandatory)][string]$Destination,
        [string]$StripPrefix = ''
    )
    # git 输出 UTF-8;5.1 默认按控制台代码页解码,中文文件名会乱码,读取期间临时切到 UTF-8。
    $previous = $null
    try { $previous = [Console]::OutputEncoding; [Console]::OutputEncoding = New-Object Text.UTF8Encoding($false) } catch { }
    try {
        $raw = & git -C $script:ForgeRoot ls-files -z -- @Paths
        $code = $LASTEXITCODE
    } finally {
        if ($previous) { try { [Console]::OutputEncoding = $previous } catch { } }
    }
    if ($code -ne 0) { throw "git ls-files 失败:$($Paths -join ' ')" }
    $files = @($raw -split "`0" | Where-Object { $_ })
    if ($files.Count -eq 0) { throw "没有找到受 git 跟踪的文件:$($Paths -join ' ')" }
    foreach ($rel in $files) {
        $target = $rel
        if ($StripPrefix -and $target.StartsWith($StripPrefix)) { $target = $target.Substring($StripPrefix.Length) }
        $dst = Join-Path $Destination ($target -replace '/', '\')
        New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dst) | Out-Null
        Copy-Item -LiteralPath (Join-Path $script:ForgeRoot $rel) -Destination $dst -Force
    }
    return $files.Count
}

# 整目录镜像复制(robocopy 退出码 < 8 都算成功)。
function Copy-Tree([string]$Source, [string]$Destination) {
    if (-not (Test-Path -LiteralPath $Source)) { throw "目录不存在:$Source" }
    & robocopy $Source $Destination /MIR /NFL /NDL /NJH /NJS /NP /R:2 /W:1 | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "复制失败(robocopy $LASTEXITCODE):$Source -> $Destination" }
    $global:LASTEXITCODE = 0
}

# pnpm 的 node_modules 是目录联接;解析到真实目录(electron-builder 需要实路径)。
function Resolve-LinkTarget([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.LinkType) {
        $target = @($item.Target)[0]
        if (-not [IO.Path]::IsPathRooted($target)) { $target = Join-Path (Split-Path -Parent $Path) $target }
        return [IO.Path]::GetFullPath($target)
    }
    return $item.FullName
}
