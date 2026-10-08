param()

$ErrorActionPreference = 'Stop'
$repoPath = Split-Path -Parent $PSScriptRoot
$standardBinary = Join-Path $repoPath 'target\debug\forge-agentd.exe'
$claudeBinary = Join-Path $repoPath 'target\debug\forge-agentd-claude-20261007.exe'
$binaryPath = if (Test-Path -LiteralPath $claudeBinary) { $claudeBinary } else { $standardBinary }
$rollbackPath = Join-Path $repoPath 'target\debug\forge-agentd-before-gpt61fix-20261007.exe'
$storageCheck = 'D:\Rurix\ci\storage_health.py'
$endpoint = 'http://127.0.0.1:8103'

function Start-Agentd([string]$Path) {
    $env:FORGE_AGENTD_ADDR = '127.0.0.1:8103'
    $env:FORGE_AGENTD_DATA_DIR = Join-Path $repoPath 'data'
    $env:FORGE_GEN_DATA_DIR = Join-Path $repoPath 'data'
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
    Start-Process -FilePath $Path -WorkingDirectory $repoPath -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $repoPath "data\cloud-server\agentd-restart-$stamp.stdout.log") `
        -RedirectStandardError (Join-Path $repoPath "data\cloud-server\agentd-restart-$stamp.stderr.log") -PassThru
}

function Wait-Agentd {
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        try {
            Invoke-RestMethod -Uri "$endpoint/health" -TimeoutSec 2 | Out-Null
            return
        } catch { Start-Sleep -Milliseconds 500 }
    }
    throw '本地代理未恢复监听。'
}

if (-not (Test-Path -LiteralPath $binaryPath -PathType Leaf)) { throw '缺少已构建的 forge-agentd.exe。' }
if (Test-Path -LiteralPath $storageCheck) { & py -3 $storageCheck --phase start --record }
try {
    $sessions = Invoke-RestMethod -Uri "$endpoint/api/forge/sessions" -TimeoutSec 5
    if (@($sessions.sessions | Where-Object { $_.activeRunId }).Count -gt 0) {
        throw '当前有 IDE 任务运行，请等任务结束后重启。'
    }
    $listeners = @(Get-NetTCPConnection -LocalPort 8103 -State Listen -ErrorAction Stop)
    $ownerIds = @($listeners.OwningProcess | Sort-Object -Unique)
    if ($ownerIds.Count -ne 1) { throw '8103 端口的进程身份不唯一。' }
    $processId = $ownerIds[0]
    $current = Get-CimInstance Win32_Process -Filter "ProcessId = $processId"
    if ($current.ExecutablePath -notin @($standardBinary, $claudeBinary, $rollbackPath)) {
        throw '8103 端口不属于本工作区代理，拒绝操作。'
    }
    Stop-Process -Id $processId -ErrorAction Stop
    Wait-Process -Id $processId -Timeout 10 -ErrorAction SilentlyContinue
    try {
        $newProcess = Start-Agentd $binaryPath
        Wait-Agentd
        Write-Output "本地代理已更新，PID=$($newProcess.Id)。"
    } catch {
        if (Test-Path -LiteralPath $rollbackPath -PathType Leaf) {
            if ($newProcess -and -not $newProcess.HasExited) { Stop-Process -Id $newProcess.Id }
            Start-Agentd $rollbackPath | Out-Null
            Wait-Agentd
            throw '新版代理启动失败，已恢复原版代理。'
        }
        throw
    }
} finally {
    if (Test-Path -LiteralPath $storageCheck) { & py -3 $storageCheck --phase finish --record }
}
