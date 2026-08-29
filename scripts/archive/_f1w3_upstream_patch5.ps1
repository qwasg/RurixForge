# F1 wave.3 上游补丁 round 5(实验):import 图像 usage 收窄为 COLOR_ATTACHMENT|TRANSFER_SRC。
# 假设:恒附加的 TRANSFER_DST(及未来 SAMPLED)使驱动为 external 图像选择大 padding 布局
# (960x540 RGBA8: vk req=24,576,000 vs d3d12 alloc=2,228,224)。收窄后重测 req。
$ErrorActionPreference = 'Stop'
$f = 'H:\rurix\src\rurix-rt\src\render_exec.rs'
$t = [System.IO.File]::ReadAllText($f)
$eol = if ($t.Contains("`r`n")) { "`r`n" } else { "`n" }
function Norm($s) { ($s -split "`r`n|`n") -join $script:eol }
function Patch($name, $old, $new) {
  $o = Norm $old; $n = Norm $new
  $hit = ([regex]::Matches($script:t, [regex]::Escape($o))).Count
  if ($hit -ne 1) { throw "$name 命中 $hit ≠ 1" }
  $script:t = $script:t.Replace($o, $n)
  Write-Host "[patch] $name"
}

Patch 'R5 import 图像 usage 收窄' @'
                        tiling: IMAGE_TILING_OPTIMAL,
                        usage: texture_usage_flags(t.usage),
                        sharing_mode: SHARING_MODE_EXCLUSIVE,
'@ @'
                        tiling: IMAGE_TILING_OPTIMAL,
                        // F1 wave.3:external import 图像收窄 usage —— 色目标只需
                        // COLOR_ATTACHMENT|TRANSFER_SRC(readback);恒附加的 TRANSFER_DST
                        // 等会使驱动选择大 padding 布局(960x540 实测 req 11x 于 d3d12 alloc)。
                        usage: if t.external_import.is_some() {
                            0x1 | 0x10 // TRANSFER_SRC | COLOR_ATTACHMENT
                        } else {
                            texture_usage_flags(t.usage)
                        },
                        sharing_mode: SHARING_MODE_EXCLUSIVE,
'@

[System.IO.File]::WriteAllText($f, $t, (New-Object System.Text.UTF8Encoding $false))
Write-Host "ROUND5 OK"
