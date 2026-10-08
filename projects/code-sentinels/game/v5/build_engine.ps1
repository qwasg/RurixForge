$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot '..\..'))
$binaryDirectory = Join-Path $PSScriptRoot 'runtime-bin'
New-Item -ItemType Directory -Force -Path $binaryDirectory | Out-Null
$binaryPath = Join-Path $binaryDirectory 'engine-host.exe'
Push-Location $repositoryRoot
try {
    # Cargo's ordinary dependency cache is reused; only this binary's output
    # goes to the V5 directory so an open editor executable is not overwritten.
    & cargo rustc -p engine-host --bin engine-host -- -C opt-level=2 -o $binaryPath
    if ($LASTEXITCODE -ne 0) { throw 'V5 engine compilation failed.' }
} finally { Pop-Location }
