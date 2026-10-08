$ErrorActionPreference = 'Stop'
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$logsRoot = [IO.Path]::GetFullPath((Join-Path $project 'Logs/v6'))
$workingOutput = [IO.Path]::GetFullPath((Join-Path $logsRoot 'pressure'))
$previousOutput = [IO.Path]::GetFullPath((Join-Path $logsRoot 'pressure-before-transparent-bounds-20260912'))
$runOutput = [IO.Path]::GetFullPath((Join-Path $logsRoot 'pressure-4bf9973c-f3f576ef-20260912'))
$executable = Join-Path $PSScriptRoot 'runtime-bin/engine-host.exe'
$driver = Join-Path $PSScriptRoot 'pressure-probe.mjs'
$driverLog = Join-Path $PSScriptRoot 'transparent-bounds-pressure-driver-20260912.log'
$expectedHost = '4bf9973cb6bf0618e75e0fb3f9f494c9612aeed44a7ebe180f5dee4e6ecf5ef9'

foreach ($target in @($workingOutput, $previousOutput, $runOutput)) {
    if (-not $target.StartsWith($logsRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Output path escaped the intended Logs/v6 directory: $target"
    }
}
if ((Test-Path -LiteralPath $previousOutput) -or (Test-Path -LiteralPath $runOutput) -or (Test-Path -LiteralPath $driverLog)) {
    throw 'A run destination or driver log already exists. Preserve previous evidence.'
}
$hostBefore = (Get-FileHash -Algorithm SHA256 -LiteralPath $executable).Hash.ToLowerInvariant()
if ($hostBefore -ne $expectedHost) { throw 'The frozen 4B host identity changed.' }
$busy = @(Get-CimInstance Win32_Process -Filter "Name='engine-host.exe' OR Name='cargo.exe' OR Name='rustc.exe'")
if ($busy.Count -gt 0) { throw "Exclusive diagnostic window is not free: $($busy.ProcessId -join ', ')" }

# The fixed native output is moved once, inside the checked evidence root.
# Pixel QA confirms its fixtures are immutable copies elsewhere before this runs.
if (Test-Path -LiteralPath $workingOutput) {
    $resolvedPrior = (Resolve-Path -LiteralPath $workingOutput).Path
    if (-not $resolvedPrior.StartsWith($logsRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Resolved previous output escaped the intended Logs/v6 directory: $resolvedPrior"
    }
    Move-Item -LiteralPath $resolvedPrior -Destination $previousOutput
}
$started = [DateTime]::UtcNow.ToString('o')
$driverHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $driver).Hash.ToLowerInvariant()
$invocationFailure = $null
try {
    & node $driver $project $executable --gpu-telemetry *> $driverLog
    $exitCode = $LASTEXITCODE
} catch {
    $invocationFailure = $_.Exception.Message
    $exitCode = if ($LASTEXITCODE) { $LASTEXITCODE } else { 1 }
    Add-Content -LiteralPath $driverLog -Value $invocationFailure
}
$finished = [DateTime]::UtcNow.ToString('o')
$hostAfter = (Get-FileHash -Algorithm SHA256 -LiteralPath $executable).Hash.ToLowerInvariant()
if (-not (Test-Path -LiteralPath $workingOutput)) { throw 'Pressure process produced no output directory; driver log is preserved.' }

# Resolve both paths again immediately before moving the new run out of the fixed slot.
$actualWorking = (Resolve-Path -LiteralPath $workingOutput).Path
$actualRun = [IO.Path]::GetFullPath($runOutput)
foreach ($target in @($actualWorking, $actualRun)) {
    if (-not $target.StartsWith($logsRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Resolved output escaped the intended Logs/v6 directory: $target"
    }
}
Move-Item -LiteralPath $actualWorking -Destination $actualRun
$receipt = [ordered]@{
    startedAtUtc = $started
    completedAtUtc = $finished
    exitCode = $exitCode
    invocationFailure = $invocationFailure
    command = @('node', $driver, $project, $executable, '--gpu-telemetry')
    driverSha256 = $driverHash
    engineSha256Before = $hostBefore
    engineSha256After = $hostAfter
    executableUnchanged = ($hostBefore -eq $hostAfter)
    scope = 'Strict 60-second CPU plus 60-second eight-layer GPU diagnostic on frozen 4B/F3, after exclusive window coordination. No acceptance for later binaries.'
    finalEligible = $false
    priorOutputPreservedAt = $previousOutput
    output = $actualRun
    driverLog = $driverLog
}
$receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $actualRun 'process-exit.json') -Encoding utf8
$receipt | ConvertTo-Json -Depth 5
if ($hostBefore -ne $hostAfter) { throw 'The executable changed during the run; receipt retained and acceptance refused.' }
exit $exitCode
