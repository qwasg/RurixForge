param(
    [Parameter(Mandatory=$true)][string]$Path,
    [Parameter(Mandatory=$true)][string]$ProductName,
    [Parameter(Mandatory=$true)][string]$ProductVersion,
    [Parameter(Mandatory=$true)][string]$FileVersion,
    [Parameter(Mandatory=$true)][string]$CompanyName
)
$ErrorActionPreference = 'Stop'
$resolvedInput = (Resolve-Path -LiteralPath $Path).Path
$version = [Diagnostics.FileVersionInfo]::GetVersionInfo($resolvedInput)
$expected = @{
    ProductName=$ProductName; ProductVersion=$ProductVersion; FileVersion=$FileVersion;
    CompanyName=$CompanyName; FileDescription='Code Sentinels V6 native engine';
    OriginalFilename='engine-host.exe'; InternalName='engine-host'
}
foreach ($field in $expected.Keys) {
    if ([string]::IsNullOrWhiteSpace($expected[$field]) -or $version.$field -cne $expected[$field]) {
        throw "PE metadata missing or differs from reviewed source configuration: $field"
    }
}
$signature = Get-AuthenticodeSignature -LiteralPath $resolvedInput
if ($signature.Status -ne 'NotSigned') { throw 'Fresh CI build must be unsigned before the SignPath signing step.' }
[ordered]@{ metadataVerified=$true; productName=$version.ProductName; productVersion=$version.ProductVersion; fileVersion=$version.FileVersion; gameExecuted=$false } | ConvertTo-Json
