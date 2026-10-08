param([string]$CheckoutPath)
$ErrorActionPreference = 'Stop'
# Rurix 1478859 vendors Jolt 5.3.0, but its ignore rules omitted Build/CMakeLists.txt.
# Restore only that build file from the exact commit recorded by its VENDOR.md.
# No repository source, dependency revision, compiler flags, or physics implementation changes.
$joltCommit = '0373ec0dd762e4bc2f6acdb08371ee84fa23c6db'
$expectedHash = 'B498D3CF2761354CCD9A2222B920106A8388A92C85365ECFF507A4CA2B7BB2ED'
if (-not $CheckoutPath) {
    $cargoBase = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $candidates = @(Get-ChildItem -LiteralPath (Join-Path $cargoBase 'git/checkouts') -Directory -Filter 'rurix-*' | ForEach-Object { Join-Path $_.FullName '1478859' } | Where-Object { Test-Path -LiteralPath $_ })
    if ($candidates.Count -ne 1) { throw 'Specify -CheckoutPath for the existing Rurix 1478859 Cargo checkout (run cargo fetch first).' }
    $CheckoutPath = $candidates[0]
}
$resolvedCheckout = (Resolve-Path -LiteralPath $CheckoutPath).Path
$actualRevision = (& git -C $resolvedCheckout rev-parse HEAD).Trim()
if ($actualRevision -ne '1478859a8f2ea3a8e17abb06aa0211a6e0871cca') { throw "Unexpected Rurix checkout revision: $actualRevision" }
$vendorRoot = Join-Path $resolvedCheckout 'src/rurix-physics-sys'
if (-not (Get-Content -LiteralPath (Join-Path $vendorRoot 'VENDOR.md') -Raw).Contains($joltCommit)) { throw 'Jolt vendor pin does not match.' }
$targetDirectory = [IO.Path]::GetFullPath((Join-Path $vendorRoot 'vendor/JoltC/JoltPhysics/Build'))
if (-not $targetDirectory.StartsWith($resolvedCheckout + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Dependency target escaped checkout.' }
$targetFile = Join-Path $targetDirectory 'CMakeLists.txt'
if (Test-Path -LiteralPath $targetFile) {
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $targetFile).Hash -ne $expectedHash) { throw 'Existing Jolt build file differs; refusing to overwrite.' }
} else {
    $downloadFile = Join-Path ([IO.Path]::GetTempPath()) ('rurix-jolt-cmake-' + [guid]::NewGuid().ToString() + '.txt')
    try {
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/jrouwe/JoltPhysics/$joltCommit/Build/CMakeLists.txt" -OutFile $downloadFile
        if ((Get-FileHash -Algorithm SHA256 -LiteralPath $downloadFile).Hash -ne $expectedHash) { throw 'Downloaded Jolt build file hash mismatch.' }
        New-Item -ItemType Directory -Path $targetDirectory -Force | Out-Null
        Copy-Item -LiteralPath $downloadFile -Destination $targetFile
    } finally {
        if (Test-Path -LiteralPath $downloadFile) { Remove-Item -LiteralPath $downloadFile }
    }
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (Test-Path -LiteralPath $vswhere) {
    $vsInstall = & $vswhere -latest -products '*' -property installationPath
    if ($vsInstall) {
        $cmakeDirectory = Join-Path $vsInstall 'Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin'
        if (Test-Path -LiteralPath (Join-Path $cmakeDirectory 'cmake.exe')) { $env:PATH = $cmakeDirectory + ';' + $env:PATH }
    }
}
Write-Output "Jolt build bootstrap verified: commit=$joltCommit sha256=$expectedHash file=$targetFile"
