param(
    [Parameter(Mandatory=$true)][string]$Directory,
    [Parameter(Mandatory=$true)][string]$ExpectedUnsignedSha256,
    [Parameter(Mandatory=$true)][string]$ExpectedProvenanceSha256,
    [Parameter(Mandatory=$true)][string]$ExpectedSignerSubject,
    [Parameter(Mandatory=$true)][string]$ProductVersion,
    [Parameter(Mandatory=$true)][string]$FileVersion,
    [Parameter(Mandatory=$true)][string]$CompanyName
)
$ErrorActionPreference = 'Stop'
$signedRoot = (Resolve-Path -LiteralPath $Directory).Path
$enginePath = (Resolve-Path -LiteralPath (Join-Path $signedRoot 'bin/engine-host.exe')).Path
$provenancePath = Join-Path $signedRoot 'build-provenance.json'
if ((Get-FileHash -LiteralPath $provenancePath).Hash.ToLowerInvariant() -ne $ExpectedProvenanceSha256) { throw 'Source-build provenance changed in transit.' }
$provenance = Get-Content -LiteralPath $provenancePath -Raw | ConvertFrom-Json
if ($provenance.unsignedEngineSha256 -ne $ExpectedUnsignedSha256) { throw 'Unsigned artifact identity differs from build job output.' }
if ((Get-FileHash -LiteralPath (Join-Path $signedRoot 'source-manifest.json')).Hash.ToLowerInvariant() -ne $provenance.sourceManifestSha256) { throw 'Source manifest changed in transit.' }
& python -B (Join-Path $PSScriptRoot 'pe_signing_content.py') verify --signed $enginePath --provenance $provenancePath
if ($LASTEXITCODE -ne 0) { throw 'Signed program content is not the exact CI-built unsigned content.' }
$unexpectedExecutables = @(Get-ChildItem -LiteralPath $signedRoot -Recurse -File | Where-Object { ($_.Extension -in @('.exe','.dll')) -and $_.FullName -ne $enginePath })
if ($unexpectedExecutables.Count -gt 0) { throw 'Signing output includes an executable outside the approved engine-host scope.' }
$signature = Get-AuthenticodeSignature -LiteralPath $enginePath
if ($signature.Status -ne 'Valid' -or [string]::IsNullOrWhiteSpace($ExpectedSignerSubject) -or $signature.SignerCertificate.Subject -cne $ExpectedSignerSubject) { throw 'A valid signature from the actually approved publisher is required.' }
$version = [Diagnostics.FileVersionInfo]::GetVersionInfo($enginePath)
if ($version.ProductName -cne 'Code Sentinels V6' -or $version.ProductVersion -cne $ProductVersion -or $version.FileVersion -cne $FileVersion -or $version.CompanyName -cne $CompanyName -or $version.OriginalFilename -cne 'engine-host.exe') { throw 'Product identity changed while signing.' }
$result = [ordered]@{ recordedAtUtc=[DateTimeOffset]::UtcNow.ToString('o'); unsignedSha256=$ExpectedUnsignedSha256; signedSha256=(Get-FileHash -LiteralPath $enginePath).Hash.ToLowerInvariant(); programContentMatchesUnsigned=$true; signingContentScheme=$provenance.signingContent.scheme; signerSubject=$signature.SignerCertificate.Subject; signerThumbprint=$signature.SignerCertificate.Thumbprint; sourceProvenanceSha256=$ExpectedProvenanceSha256; signatureStatus=[string]$signature.Status; productName=$version.ProductName; productVersion=$version.ProductVersion; gameExecuted=$false; releasePublished=$false }
$receiptPath = Join-Path $signedRoot 'signing-receipt.json'
$stream = [System.IO.File]::Open($receiptPath,[System.IO.FileMode]::CreateNew,[System.IO.FileAccess]::Write)
try {
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes(($result | ConvertTo-Json -Depth 6))
    $stream.Write($bytes,0,$bytes.Length)
} finally { $stream.Dispose() }
$result | ConvertTo-Json -Depth 6
