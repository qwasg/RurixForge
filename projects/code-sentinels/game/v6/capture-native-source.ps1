param([Parameter(Mandatory=$true)][string]$Output)
$ErrorActionPreference='Stop'
$repoRoot=(Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$previous=Get-Content -LiteralPath (Join-Path $PSScriptRoot 'death-depth-source-before.json') -Raw | ConvertFrom-Json
$paths=[System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
foreach($item in $previous){[void]$paths.Add($item.path)}
foreach($dir in @('projects/code-sentinels/native-v6/src','projects/code-sentinels/native-v6/tests','crates/engine-host/src')){
  foreach($file in Get-ChildItem -LiteralPath (Join-Path $repoRoot $dir) -Recurse -File -Filter '*.rs'){
    [void]$paths.Add([IO.Path]::GetRelativePath($repoRoot,$file.FullName).Replace('\','/'))
  }
}
foreach($path in @('projects/code-sentinels/native-v6/build.rs','projects/code-sentinels/native-v6/Cargo.toml','projects/code-sentinels/native-v6/Cargo.lock')){[void]$paths.Add($path)}
$rows=@(foreach($relative in ($paths | Sort-Object)){
  $file=Get-Item -LiteralPath (Join-Path $repoRoot $relative)
  [ordered]@{path=$relative;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash;bytes=$file.Length}
})
$rows | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $Output -Encoding utf8
[ordered]@{fileCount=$rows.Count;sha256=(Get-FileHash -LiteralPath $Output -Algorithm SHA256).Hash;manifest=(Resolve-Path -LiteralPath $Output).Path} | ConvertTo-Json -Compress
