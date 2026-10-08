$ErrorActionPreference = 'Stop'
$cancelProject = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$cancelBase = Join-Path $cancelProject 'game/v6/final-balance-6828c5b7-20260914'
$cancelDirectory = Join-Path $cancelBase 'root-requested-cancellation'
$null = New-Item -ItemType Directory -Path $cancelDirectory -ErrorAction Stop
$cancelSchedulerId = 35068
$cancelRunnerPath = [IO.Path]::GetFullPath((Join-Path $cancelBase 'artifact/sentinels-v6-balance-runner.exe'))
function Write-CancelJson([string]$name, $value) {
    $cancelFile = Join-Path $cancelDirectory $name
    $cancelBytes = [Text.Encoding]::UTF8.GetBytes(($value | ConvertTo-Json -Depth 18) + "`n")
    $cancelStream = [IO.File]::Open($cancelFile, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $cancelStream.Write($cancelBytes, 0, $cancelBytes.Length) } finally { $cancelStream.Dispose() }
}
function Get-OwnedRunners {
    @(Get-CimInstance Win32_Process -Filter "Name = 'sentinels-v6-balance-runner.exe'" | Where-Object {
        $_.ParentProcessId -eq $cancelSchedulerId -and $_.ExecutablePath -eq $cancelRunnerPath -and $_.CommandLine -like '*final-balance-6828c5b7-20260914*'
    })
}
function Record-Process($process) {
    $cancelCase = $null
    if ($process.CommandLine -match '--first\s+"?(\d+)') { $cancelCase = [int]$Matches[1] }
    $cancelMatrix = if ($process.CommandLine -match '[\\/]strategy[\\/]results-') { 'strategy' } elseif ($process.CommandLine -match '[\\/]branch[\\/]results-') { 'branch' } else { 'orchestrator' }
    @{ pid = $process.ProcessId; parentPid = $process.ParentProcessId; name = $process.Name; executable = $process.ExecutablePath; creationDate = $process.CreationDate.ToUniversalTime().ToString('o'); commandLine = $process.CommandLine; matrix = $cancelMatrix; currentCase = $cancelCase }
}
$cancelBefore = Get-Content -LiteralPath (Join-Path $cancelBase 'source-before.json') -Raw -Encoding UTF8 | ConvertFrom-Json -AsHashtable
$cancelCurrent = @($cancelBefore | ForEach-Object {
    $cancelSourcePath = Join-Path $cancelProject $_.path
    @{ path = $_.path; originalSha256 = $_.sha256; currentSha256 = (Get-FileHash -LiteralPath $cancelSourcePath -Algorithm SHA256).Hash.ToLowerInvariant(); bytes = (Get-Item -LiteralPath $cancelSourcePath).Length }
})
$cancelScheduler = Get-CimInstance Win32_Process -Filter "ProcessId = $cancelSchedulerId"
if (-not $cancelScheduler -or $cancelScheduler.Name -ne 'python.exe' -or $cancelScheduler.CommandLine -notlike '*run_full_6828_shared_20260914.py*') { throw 'Scheduler identity mismatch; nothing stopped.' }
$cancelInitialRunners = @(Get-OwnedRunners)
$cancelInitial = @((Record-Process $cancelScheduler)) + @($cancelInitialRunners | ForEach-Object { Record-Process $_ })
Write-CancelJson 'pre-stop-record.json' @{
    recordedAtUtc = [DateTime]::UtcNow.ToString('o')
    reason = 'Root-requested cancellation for newly confirmed approved-plan inconsistencies: linear room HP and DC merge capacity conservation. This is not selection by partial win rates. No Game source edit is authorized in this stop script.'
    originalRulesFingerprint = '6828c5b7f825b7e27b3e9df3c22d133ecefd8195a436f856c0682a33e580d781'
    originalSourceManifestSha256 = (Get-FileHash -LiteralPath (Join-Path $cancelBase 'source-before.json') -Algorithm SHA256).Hash.ToLowerInvariant()
    currentSourceRecords = $cancelCurrent
    allRecordedSourcesMatchOriginal = (@($cancelCurrent | Where-Object { $_.currentSha256 -ne $_.originalSha256 }).Count -eq 0)
    processes = $cancelInitial
}
$cancelActions = [Collections.Generic.List[object]]::new()
function Stop-VerifiedProcess($record) {
    $cancelLive = Get-CimInstance Win32_Process -Filter "ProcessId = $($record.pid)"
    if (-not $cancelLive) { $cancelActions.Add(@{ pid = $record.pid; action = 'already-exited-before-stop'; observedAtUtc = [DateTime]::UtcNow.ToString('o') }); return }
    if ($cancelLive.Name -ne $record.name -or $cancelLive.ExecutablePath -ne $record.executable -or $cancelLive.CreationDate.ToUniversalTime().ToString('o') -ne $record.creationDate -or $cancelLive.CommandLine -ne $record.commandLine) {
        $cancelActions.Add(@{ pid = $record.pid; action = 'identity-changed-not-stopped'; observedAtUtc = [DateTime]::UtcNow.ToString('o') }); return
    }
    Stop-Process -Id $record.pid -ErrorAction Stop
    $cancelActions.Add(@{ pid = $record.pid; action = 'root-requested-stop'; matrix = $record.matrix; currentCase = $record.currentCase; executable = $record.executable; observedAtUtc = [DateTime]::UtcNow.ToString('o') })
}
# Stop the verified scheduler first, preventing further worker replenishment.
Stop-VerifiedProcess $cancelInitial[0]
foreach ($cancelRecord in $cancelInitial | Select-Object -Skip 1) { Stop-VerifiedProcess $cancelRecord }
# Re-observe any worker created between initial capture and scheduler termination.
$cancelLate = @(Get-OwnedRunners | ForEach-Object { Record-Process $_ })
Write-CancelJson 'late-worker-pre-stop-record.json' @{ recordedAtUtc = [DateTime]::UtcNow.ToString('o'); processes = $cancelLate }
foreach ($cancelRecord in $cancelLate) { Stop-VerifiedProcess $cancelRecord }
Start-Sleep -Milliseconds 500
$cancelRemaining = @(Get-OwnedRunners | ForEach-Object { Record-Process $_ })
$cancelSchedulerRemaining = Get-CimInstance Win32_Process -Filter "ProcessId = $cancelSchedulerId"
$cancelOriginalSchedulerAlive = [bool]($cancelSchedulerRemaining -and $cancelSchedulerRemaining.Name -eq 'python.exe' -and $cancelSchedulerRemaining.CommandLine -like '*run_full_6828_shared_20260914.py*')
Write-CancelJson 'stop-actions.json' $cancelActions
Write-CancelJson 'process-exit-verification.json' @{ observedAtUtc = [DateTime]::UtcNow.ToString('o'); schedulerStillRunning = $cancelOriginalSchedulerAlive; remainingOwnedRunners = $cancelRemaining; allOwnedProcessesExited = (-not $cancelOriginalSchedulerAlive -and $cancelRemaining.Count -eq 0); scope = 'Owned processes only. Stopped for Root cancellation, not labelled failed test/game outcomes.' }
if ($cancelOriginalSchedulerAlive -or $cancelRemaining.Count -gt 0) { throw 'Some verified owned processes remain; inspect retained records.' }
@{ cancellationDirectory = $cancelDirectory; recordedRunnerCount = $cancelInitialRunners.Count; lateRunnerCount = $cancelLate.Count; allOwnedProcessesExited = $true } | ConvertTo-Json
