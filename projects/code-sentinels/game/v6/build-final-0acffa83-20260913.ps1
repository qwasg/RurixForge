$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../../..'))
$run = Join-Path $PSScriptRoot 'native-final-0acffa83-20260913-a'
if (Test-Path -LiteralPath $run) { throw 'Preserve the existing build attempt.' }
New-Item -ItemType Directory -Path $run | Out-Null
$previous = Join-Path $run 'previous-4bf9973c-runtime'
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'runtime-bin') -Destination $previous -Recurse
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'native-build-receipt.json') -Destination (Join-Path $run 'previous-native-build-receipt.json')
$before = Join-Path $run 'source-before.json'
$after = Join-Path $run 'source-after.json'
$log = Join-Path $run 'native-build.log'
$artifactDirectory = Join-Path $run 'runtime'
New-Item -ItemType Directory -Path $artifactDirectory | Out-Null
$artifact = Join-Path $artifactDirectory 'engine-host.exe'
& (Join-Path $PSScriptRoot 'capture-native-source.ps1') -Output $before
$started = [DateTime]::UtcNow.ToString('o')
Push-Location -LiteralPath $repository
try {
    & cargo rustc -p engine-host --bin engine-host -- -C opt-level=2 -o $artifact *> $log
    $buildExit = $LASTEXITCODE
} finally { Pop-Location }
& (Join-Path $PSScriptRoot 'capture-native-source.ps1') -Output $after
$beforeSha = (Get-FileHash -LiteralPath $before -Algorithm SHA256).Hash.ToLowerInvariant()
$afterSha = (Get-FileHash -LiteralPath $after -Algorithm SHA256).Hash.ToLowerInvariant()
$result = [ordered]@{
    startedAtUtc = $started
    completedAtUtc = [DateTime]::UtcNow.ToString('o')
    buildExit = $buildExit
    buildCommand = @('cargo','rustc','-p','engine-host','--bin','engine-host','--','-C','opt-level=2','-o',$artifact)
    artifact = if (Test-Path -LiteralPath $artifact) { [ordered]@{path=$artifact; sha256=(Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant(); bytes=(Get-Item -LiteralPath $artifact).Length} } else { $null }
    source = [ordered]@{beforeSha256=$beforeSha;afterSha256=$afterSha;unchangedDuringBuild=($beforeSha -eq $afterSha);fileCount=@(Get-Content -LiteralPath $after -Raw | ConvertFrom-Json).Count}
}
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $run 'build-result.json') -Encoding utf8
$result | ConvertTo-Json -Depth 6
if ($buildExit -ne 0) { throw 'Native build failed; attempt preserved.' }
if ($beforeSha -ne $afterSha) { throw 'Sources changed during native build; receipt cannot approve this artifact.' }
