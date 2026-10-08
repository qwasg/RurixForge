$ErrorActionPreference = 'Stop'
$balanceProject = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$balanceBase = Join-Path $balanceProject 'game/v6/final-balance-0acffa83-20260913'
function Read-BalanceJson([string]$relative) {
    Get-Content -LiteralPath (Join-Path $balanceBase $relative) -Raw -Encoding UTF8 | ConvertFrom-Json -AsHashtable
}
$strategyCompletion = Read-BalanceJson 'strategy/completion-receipt.json'
$branchCompletion = Read-BalanceJson 'branch/completion-receipt.json'
$strategyAudit = Read-BalanceJson 'strategy/strict-integrity-check.json'
$branchAudit = Read-BalanceJson 'branch/strict-integrity-check.json'
$branchReview = Read-BalanceJson 'branch/branch-review-data.json'
if (-not $strategyCompletion.complete -or $strategyCompletion.completedFreshCases -ne 120) { throw 'Strategy completion missing.' }
if (-not $branchCompletion.complete -or $branchCompletion.completedFreshCases -ne 1000) { throw 'Branch completion missing.' }
if (-not $strategyAudit.passed -or -not $branchAudit.passed -or $branchAudit.games -ne 1000) { throw 'Strict canonical/statistical audit missing.' }
if (-not $branchReview.integrityPassed -or $branchReview.finalSaves.Count -ne 1000 -or $branchReview.primaryObservations.Count -ne 2000) { throw 'Final supplemental Save/statistics audit missing.' }
$balanceScriptPattern = 'run_final_balance_20260913\.py|aggregate_final_balance_20260913\.py|audit_final_matrix_20260913\.py|summarize_final_branches_20260913\.py|summarize_final_strategy_20260913\.py|replay_final_strategy_110_20260913\.py'
$balanceProcesses = @(Get-CimInstance Win32_Process | Where-Object {
    $_.Name -in @('sentinels-v6-balance-runner.exe', 'sentinels-v6-formation-purchase-observer.exe') -or
    ($_.Name -in @('python.exe', 'pythonw.exe') -and $_.CommandLine -match $balanceScriptPattern)
} | Select-Object Name, ProcessId, ParentProcessId, CreationDate, CommandLine)
if ($balanceProcesses.Count -gt 0) { $balanceProcesses | ConvertTo-Json -Depth 8; throw 'Balance runner, observer or statistics processes remain active; CPU not released.' }
$balanceEvidence = @('measurement-manifest.json', 'source-before.json', 'strategy/completion-receipt.json', 'strategy/strict-integrity-check.json', 'branch/completion-receipt.json', 'branch/strict-integrity-check.json', 'branch/branch-review-data.json') | ForEach-Object {
    $balanceEvidencePath = Join-Path $balanceBase $_
    @{ path = [IO.Path]::GetRelativePath($balanceProject, $balanceEvidencePath).Replace('\','/'); sha256 = (Get-FileHash -LiteralPath $balanceEvidencePath -Algorithm SHA256).Hash.ToLowerInvariant() }
}
$balanceRelease = @{
    observedAtUtc = [DateTime]::UtcNow.ToString('o')
    scope = 'All authorized126+1000 balance observations and final statistical/Save audits are complete. No matching balance runner, observer or Python statistics processes remain. CPU released for separately authorized exclusive native LAN retry; no global balance approval.'
    rulesFingerprint = '0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a'
    strategyCases = 126
    branchCases = 1000
    activeBalanceProcesses = $balanceProcesses
    cpuReleased = $true
    finalBalanceAcceptance = $false
    evidence = $balanceEvidence
}
$balanceReleasePath = Join-Path $balanceBase 'cpu-release.json'
$balanceReleaseBytes = [Text.Encoding]::UTF8.GetBytes(($balanceRelease | ConvertTo-Json -Depth 12) + "`n")
$balanceReleaseStream = [IO.File]::Open($balanceReleasePath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
try { $balanceReleaseStream.Write($balanceReleaseBytes, 0, $balanceReleaseBytes.Length) } finally { $balanceReleaseStream.Dispose() }
@{ cpuReleased = $true; activeBalanceProcesses = 0; receipt = $balanceReleasePath; sha256 = (Get-FileHash -LiteralPath $balanceReleasePath -Algorithm SHA256).Hash.ToLowerInvariant() } | ConvertTo-Json
