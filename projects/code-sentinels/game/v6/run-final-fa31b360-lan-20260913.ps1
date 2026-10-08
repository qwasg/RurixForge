param(
    [Parameter(Mandatory=$true)][string]$Pack,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedPayload,
    [string]$RunName = 'final-fa31b360-20260913-a'
)
$ErrorActionPreference = 'Stop'
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$packDirectory = (Resolve-Path -LiteralPath $Pack).Path
$outRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'lan-runs'))
$output = [IO.Path]::GetFullPath((Join-Path $outRoot $RunName))
if (-not $output.StartsWith($outRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'LAN output escaped its evidence directory.' }
$log = Join-Path $PSScriptRoot ($RunName + '-driver.log')
$receiptPath = Join-Path $PSScriptRoot ($RunName + '-process-exit.json')
foreach ($path in @($output,$log,$receiptPath)) { if (Test-Path -LiteralPath $path) { throw "Preserve prior attempt: $path" } }
$expectedEngine = 'fa31b3608a0f418f6bed58d0cac70c2c09885ba2d9912733c1c8aff07c0f1a1e'
$expectedRules = '0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a'
$marker = Get-Content -LiteralPath (Join-Path $packDirectory 'v6-candidate.json') -Raw | ConvertFrom-Json
if ($marker.target.engineSha256 -ne $expectedEngine -or $marker.target.rulesVersion -ne 'v6.2' -or $marker.target.rulesFingerprint -ne $expectedRules -or $marker.target.payloadSha256 -ne $ExpectedPayload.ToLowerInvariant()) { throw 'Final candidate identity mismatch.' }
if ($marker.mediaReady -ne $true) { throw 'Final LAN needs completed actual media.' }
$engine = Join-Path $packDirectory 'bin/engine-host.exe'
if ((Get-FileHash -LiteralPath $engine -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedEngine) { throw 'Candidate engine bytes differ.' }
$expectedTools = @{
    'lan-soak.mjs' = '0229f87c8fdbd499c467649752db1ae01d5b1f5e9352e4f3558fddcc1aa32e5e'
    'lan-observations.mjs' = 'd460a2498cb01a708756c22bf9b31f0c12ce3fb5f826ba7d99e9c62b809b3c3d'
    'view-contract.mjs' = '9afc4e32ff4708cd7d7d3ee38ae63f2aee49aa7598b652c59b754fbb96c2ff4a'
}
foreach ($name in $expectedTools.Keys) {
    if ((Get-FileHash -LiteralPath (Join-Path $PSScriptRoot $name) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedTools[$name]) { throw "Frozen LAN observation source changed: $name" }
}
$busy = @(Get-CimInstance Win32_Process -Filter "Name='engine-host.exe' OR Name='cargo.exe' OR Name='rustc.exe'")
if ($busy.Count -gt 0) { throw "Exclusive LAN window is not free: $($busy.ProcessId -join ', ')" }
$started = [DateTime]::UtcNow.ToString('o')
$driver = Join-Path $PSScriptRoot 'lan-soak.mjs'
$invocationFailure = $null
try {
    & node $driver --pack $packDirectory --seconds 2700 --out $output *> $log
    $exitCode = $LASTEXITCODE
} catch {
    $invocationFailure = $_.Exception.Message
    $exitCode = if ($LASTEXITCODE) { $LASTEXITCODE } else { 1 }
    Add-Content -LiteralPath $log -Value $invocationFailure
}
$afterEngine = (Get-FileHash -LiteralPath $engine -Algorithm SHA256).Hash.ToLowerInvariant()
$receipt = [ordered]@{
    startedAtUtc=$started;completedAtUtc=[DateTime]::UtcNow.ToString('o')
    exitCode=$exitCode;invocationFailure=$invocationFailure
    command=@('node',$driver,'--pack',$packDirectory,'--seconds','2700','--out',$output)
    target=$marker.target;engineSha256After=$afterEngine;executableUnchanged=($afterEngine -eq $expectedEngine)
    tools=$expectedTools;output=$output;driverLog=$log
    scope='Actual final-package dual-independent-native LAN run, requiring 2700 active seconds and unchanged 57..63 Hz and 30 FPS thresholds. This process receipt alone does not approve the separately observed acceptance report.'
}
$receipt | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $receiptPath -Encoding utf8
$receipt | ConvertTo-Json -Depth 6
if ($afterEngine -ne $expectedEngine) { throw 'Candidate engine changed during LAN run; preserve and refuse acceptance.' }
exit $exitCode
