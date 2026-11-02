[CmdletBinding()]
param([ValidateSet('start', 'stop', 'restart', 'status')][string]$Action = 'status')
$ErrorActionPreference = 'Stop'
$runtime = Join-Path $env:LOCALAPPDATA 'RurixForge\CodexApi'
$binary = Join-Path $runtime 'codex-local-api.exe'
$config = Join-Path $env:USERPROFILE '.config\rurixforge\codex-api.json'

function Get-ApiProcess {
    $processes = @(Get-CimInstance Win32_Process -Filter "Name = 'codex-local-api.exe'" |
        Where-Object { $_.ExecutablePath -and [IO.Path]::GetFullPath($_.ExecutablePath) -eq $binary })
    return $processes
}

function Test-ApiListener {
    $listeners = @(Get-NetTCPConnection -LocalPort 8111 -State Listen -ErrorAction SilentlyContinue)
    if ($listeners.Count -eq 0) { return $false }
    $ownedIds = @((Get-ApiProcess) | ForEach-Object { $_.ProcessId })
    foreach ($listener in $listeners) {
        if ($listener.LocalAddress -ne '127.0.0.1' -or $listener.OwningProcess -notin $ownedIds) {
            throw 'Port 8111 is owned by another service. The Codex API was not started.'
        }
    }
    return $true
}

if ($Action -in @('stop', 'restart')) {
    foreach ($proc in @(Get-ApiProcess)) { Stop-Process -Id $proc.ProcessId -ErrorAction Stop }
    Write-Output 'RurixForge Codex API stopped.'
}
if ($Action -in @('start', 'restart')) {
    if (-not (Test-Path -LiteralPath $binary) -or -not (Test-Path -LiteralPath $config)) {
        throw 'Run install.ps1 to install the gateway and private configuration first.'
    }
    $alreadyListening = Test-ApiListener
    if (-not $alreadyListening -and @(Get-ApiProcess).Count -eq 0) {
        # Startup diagnostics only; the gateway never logs request bodies or keys.
        Start-Process -FilePath $binary -ArgumentList @('--config', ('"' + $config + '"')) `
            -WorkingDirectory $runtime -WindowStyle Hidden `
            -RedirectStandardOutput (Join-Path $runtime 'gateway.stdout.log') `
            -RedirectStandardError (Join-Path $runtime 'gateway.stderr.log') | Out-Null
    }
    $ready = $false
    for ($attempt = 0; $attempt -lt 25; $attempt++) {
        try {
            $health = Invoke-RestMethod 'http://127.0.0.1:8111/healthz' -TimeoutSec 2
            if ($health.status -eq 'ok' -and (Test-ApiListener)) { $ready = $true; break }
        } catch {}
        Start-Sleep -Milliseconds 200
    }
    if (-not $ready) { throw 'The local gateway did not become healthy; check its startup log.' }
}
if ($Action -ne 'stop') {
    if (-not (Test-ApiListener)) { throw 'The Codex API is not listening.' }
    $health = Invoke-RestMethod 'http://127.0.0.1:8111/healthz' -TimeoutSec 5
    Write-Output ('RurixForge Codex API: ' + $health.status + ' · http://127.0.0.1:8111/v1')
}
