# rurix-pin-check:上游事实记录 vs 实际 HEAD 对账(D-029)。
# 用法:pwsh scripts/rurix-pin-check.ps1   # 退出码 0 = MATCH,1 = DRIFT(漂移如实报,不静默)
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$pin = Get-Content (Join-Path $root "RURIX_PIN.json") -Raw | ConvertFrom-Json
$head = (git -C $pin.upstream rev-parse HEAD).Trim()
if ($head -eq $pin.head) {
    Write-Output "RURIX_PIN MATCH head=$head"
    exit 0
}
$subject = (git -C $pin.upstream log -1 --format="%s").Trim()
Write-Output "RURIX_PIN DRIFT"
Write-Output "  pinned: $($pin.head) ($($pin.head_date))"
Write-Output "  actual: $head"
Write-Output "  subject: $subject"
Write-Output "处置:上游已前进——按需刷新 RURIX_PIN.json(重收割对账)或立项 tag 锚定切换(D-003/D-016/D-029)"
exit 1
