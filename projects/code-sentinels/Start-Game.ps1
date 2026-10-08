$ErrorActionPreference = 'Stop'
$projectDirectory = [IO.Path]::GetFullPath($PSScriptRoot)
$repositoryDirectory = [IO.Path]::GetFullPath((Join-Path $projectDirectory '..\..'))
$agentExecutable = Join-Path $repositoryDirectory 'target\debug\forge-agentd.exe'
$hostDirectory = Join-Path $repositoryDirectory 'packages\host'
$outputDirectory = Join-Path $projectDirectory 'game\logs'
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null

function Test-Service([string]$Url) {
    try { $null = Invoke-RestMethod -Uri $Url -TimeoutSec 2; return $true }
    catch { return $false }
}
if (!(Test-Service 'http://127.0.0.1:8103/health')) {
    if (!(Test-Path -LiteralPath $agentExecutable)) { throw '请先构建 RurixForge 原生服务。' }
    Start-Process -FilePath $agentExecutable -WorkingDirectory $repositoryDirectory -WindowStyle Hidden -RedirectStandardOutput (Join-Path $outputDirectory 'agent.stdout.log') -RedirectStandardError (Join-Path $outputDirectory 'agent.stderr.log') | Out-Null
}
if (!(Test-Service 'http://127.0.0.1:3080/api/forge/health')) {
    $nodeExecutable = (Get-Command node -ErrorAction Stop).Source
    Start-Process -FilePath $nodeExecutable -ArgumentList 'dist/index.js' -WorkingDirectory $hostDirectory -WindowStyle Hidden -RedirectStandardOutput (Join-Path $outputDirectory 'host.stdout.log') -RedirectStandardError (Join-Path $outputDirectory 'host.stderr.log') | Out-Null
}
$servicesReady = $false
for ($attempt = 0; $attempt -lt 30; $attempt++) {
    if ((Test-Service 'http://127.0.0.1:8103/health') -and (Test-Service 'http://127.0.0.1:3080/api/forge/health')) { $servicesReady = $true; break }
    Start-Sleep -Milliseconds 500
}
if (!$servicesReady) { throw '服务启动失败，请查看 game/logs 中的日志。' }
$workspaces = (Invoke-RestMethod 'http://127.0.0.1:8103/api/forge/workspaces').workspaces
if (!($workspaces | Where-Object { $_.root -match '[\\/]code-sentinels$' })) {
    $body = @{ name = '编译防线 · Code Sentinels'; root = $projectDirectory } | ConvertTo-Json
    Invoke-RestMethod 'http://127.0.0.1:8103/api/forge/workspaces' -Method Post -ContentType 'application/json; charset=utf-8' -Body ([Text.Encoding]::UTF8.GetBytes($body)) | Out-Null
}
Start-Process 'http://127.0.0.1:3080/?play=code-sentinels'
