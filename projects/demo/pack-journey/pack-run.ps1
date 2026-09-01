$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$env:FORGE_PROJECT_ROOT = $root
& "$root\bin\engine-host.exe" --port 17890 --game "Content/Scenes/journey.rxscene"
