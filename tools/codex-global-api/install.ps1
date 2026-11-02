[CmdletBinding()]
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$runtime = Join-Path $env:LOCALAPPDATA 'RurixForge\CodexApi'
$binary = Join-Path $runtime 'codex-local-api.exe'
[IO.Directory]::CreateDirectory($runtime) | Out-Null
if (-not $SkipBuild) {
    # Only this small Go gateway is built. Go's existing cache/temp live on C:.
    $candidate = Join-Path $runtime 'codex-local-api.next.exe'
    Push-Location (Join-Path $repo 'cloud')
    try {
        & go build -trimpath -ldflags '-s -w' -o $candidate ./cmd/codex-local-api
        if ($LASTEXITCODE -ne 0) { throw 'Gateway build failed.' }
    } finally { Pop-Location }
    & (Join-Path $PSScriptRoot 'service.ps1') stop
    Move-Item -LiteralPath $candidate -Destination $binary -Force
}
& py -3 (Join-Path $PSScriptRoot 'setup.py')
if ($LASTEXITCODE -ne 0) { throw 'Gateway configuration failed.' }
& (Join-Path $PSScriptRoot 'configure-client.ps1')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'service.ps1') -Destination (Join-Path $runtime 'service.ps1') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'use-rurixforge.ps1') -Destination (Join-Path $runtime 'use-rurixforge.ps1') -Force
$startup = [Environment]::GetFolderPath('Startup')
$shortcutPath = Join-Path $startup 'RurixForge Codex API.lnk'
$wsh = New-Object -ComObject WScript.Shell
$shortcut = $wsh.CreateShortcut($shortcutPath)
$shortcut.TargetPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
$shortcut.Arguments = '-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + (Join-Path $runtime 'service.ps1') + '" start'
$shortcut.WorkingDirectory = $runtime
$shortcut.WindowStyle = 7
$shortcut.Description = 'Start the loopback RurixForge Codex API for this Windows user.'
$shortcut.Save()
& (Join-Path $runtime 'service.ps1') start
Write-Output ('Installed per-user login startup: ' + $shortcutPath)
