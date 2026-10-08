param(
    [Parameter(Position = 0)]
    [ValidateSet('start', 'stop', 'status', 'logs')]
    [string]$Action = 'status',
    [switch]$Rebuild
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$compose = Join-Path $repo 'cloud/deploy/docker-compose.yml'
$configFile = Join-Path $repo 'cloud/deploy/.env'
$storageCheck = 'D:\Rurix\ci\storage_health.py'

if (-not (Test-Path -LiteralPath $configFile)) {
    throw '缺少 cloud/deploy/.env。请保留当前部署配置与密钥。'
}

function Invoke-DockerCommand([string[]]$DockerArgs) {
    & docker @DockerArgs
    if ($LASTEXITCODE -ne 0) {
        throw "Docker 命令失败（退出码 $LASTEXITCODE）。"
    }
}

if (Test-Path -LiteralPath $storageCheck) {
    & py -3 $storageCheck --phase start --record
}

try {
    switch ($Action) {
        'start' {
            & docker info --format '{{.ServerVersion}}' *> $null
            if ($LASTEXITCODE -ne 0) {
                Invoke-DockerCommand @('desktop', 'start', '--timeout', '60')
            }
            $composeArgs = @('compose', '-f', $compose, 'up', '-d', '--wait', '--wait-timeout', '120')
            if ($Rebuild) {
                if (Test-Path -LiteralPath $storageCheck) {
                    & py -3 $storageCheck --phase preflight --need-gib D=1 --record
                    if ($LASTEXITCODE -ne 0) { throw '容量检查未通过，请先确认写入预算。' }
                }
                $composeArgs += '--build'
            }
            Invoke-DockerCommand $composeArgs
            Invoke-RestMethod -Uri 'http://127.0.0.1:8110/healthz' -TimeoutSec 10 | ConvertTo-Json
            Write-Output '管理后台：http://127.0.0.1:8110/admin/'
            Write-Output '账号凭据：data/cloud-server/access.txt'
        }
        'stop' {
            Invoke-DockerCommand @('compose', '-f', $compose, 'stop')
            Write-Output '服务已停止；数据库、账号和部署密钥保留。'
        }
        'status' {
            Invoke-DockerCommand @('compose', '-f', $compose, 'ps')
            Invoke-RestMethod -Uri 'http://127.0.0.1:8110/healthz' -TimeoutSec 10 | ConvertTo-Json
        }
        'logs' {
            Invoke-DockerCommand @('compose', '-f', $compose, 'logs', '--tail', '80', 'forge-cloud')
        }
    }
}
finally {
    if (Test-Path -LiteralPath $storageCheck) {
        & py -3 $storageCheck --phase finish --record
    }
}
