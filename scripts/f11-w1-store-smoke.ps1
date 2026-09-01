# F11 wave.1/3 资产商店冒烟(G-F11-1 / G-F11-3 的栈级腿):registry 全链 + 安装构建链 +
#   provenance 契约 + 校验拒绝 + 卸载 Proposal 两阶段 + 更新检查 + 个人资产库。
# 流程全经 host http://127.0.0.1:3080(forgeProxy 透传 /api/forge/store 前缀)。
# 腿:①源清单(内置官方源) ②搜索(3 包)+ 坏源如实进 errors ③详情与清单(files[] sha256)
#   ④安装 asset-pack → 轮询 completed → Content 落地 + .meta provenance.origin=store-install
#   ⑤重复安装 409 ⑥更新检查(wood-pbr 1.0.0 → 1.1.0 有更新) ⑦安装 skill 包 → skills/ 落地
#   ⑧卸载两阶段(409 GOV_PROPOSAL_REQUIRED → 批准 → completed → 文件已删)
#   ⑨个人资产库(收藏 → 列表 → 装进项目 → 移出) ⑩终态复原核验。
# 隔离:FORGE_GEN_DATA_DIR 指临时目录(源配置 + 个人库 + keystore 全隔离),
#   并预写 store-sources.json 显式锚定仓内 registry(顺带覆盖「自定义源」这条腿)。
# 自清理:finally 卸载残留 + 删临时技能目录 + 删临时 data 目录。
# 前置:powershell -File scripts\f11-seed-registry.ps1 已生成 registry/(本脚本会自检并按需生成)。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f11-w1-store-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$logFile = "evidence\f11-w1-store-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
$script:pass = 0; $script:fail = 0; $script:failures = @(); $script:skips = @()
function Check($cond, $name) {
  if ($cond) { $script:pass++; Log "  PASS $name" }
  else { $script:fail++; $script:failures += $name; Log "  FAIL $name" }
}
function Skip($name, $reason) { $script:skips += "$name($reason)"; Log "  SKIP=not-triggered $name($reason)" }

function Invoke-Json($method, $url, $bodyObj = $null, $timeoutSec = 60) {
  $req = [System.Net.HttpWebRequest]::Create($url)
  $req.Method = $method
  $req.Timeout = $timeoutSec * 1000
  if ($null -ne $bodyObj) {
    $bytes = [Text.Encoding]::UTF8.GetBytes(($bodyObj | ConvertTo-Json -Depth 12 -Compress))
    $req.ContentType = 'application/json; charset=utf-8'
    $req.ContentLength = $bytes.Length
    $st = $req.GetRequestStream(); $st.Write($bytes, 0, $bytes.Length); $st.Close()
  } elseif ($method -eq 'POST' -or $method -eq 'PATCH') { $req.ContentLength = 0 }
  try { $resp = $req.GetResponse() }
  catch [System.Net.WebException] {
    $resp = $_.Exception.Response
    if ($null -eq $resp) { throw }
  }
  $sr = New-Object IO.StreamReader($resp.GetResponseStream(), [Text.Encoding]::UTF8)
  $text = $sr.ReadToEnd(); $sr.Close(); $resp.Close()
  $obj = $null; try { $obj = $text | ConvertFrom-Json } catch {}
  return @{ status = [int]$resp.StatusCode; json = $obj; text = $text }
}

# 轮询长任务至终态(或超时)。
function Wait-Task($H, $taskId, $maxSec = 120) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $last = $null
  while ($sw.Elapsed.TotalSeconds -lt $maxSec) {
    $r = Invoke-Json GET "$H/api/forge/store/tasks/$taskId"
    $last = $r.json
    if ($null -ne $last -and $last.status -ne 'running') { return $last }
    Start-Sleep -Milliseconds 300
  }
  return $last
}

function Test-PortFree($port) {
  try {
    $l = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, $port)
    $l.Start(); $l.Stop(); return $true
  } catch { return $false }
}

New-Item -ItemType Directory -Force evidence | Out-Null
$script:procs = @()
$script:startTime = Get-Date
$utf8 = New-Object System.Text.UTF8Encoding($false)
$tmpData = Join-Path $env:TEMP "f11-store-data-$([guid]::NewGuid().ToString('N'))"
$savedGenDir = $env:FORGE_GEN_DATA_DIR
$demo = Join-Path $root "projects\demo"
$skillDir = Join-Path $root "skills\scene-audit"

try {
  # 端口:缺省 8103/3080;被占(开发者的 dev 实例常驻)则退到备用端口并排跑,
  # 不去杀别人的进程。两个都占死才中止。
  $agentdPort = if (Test-PortFree 8103) { 8103 } elseif (Test-PortFree 8123) { 8123 } else { throw "8103 与备用 8123 均被占用,请先关掉一个 agentd" }
  $hostPort = if (Test-PortFree 3080) { 3080 } elseif (Test-PortFree 3090) { 3090 } else { throw "3080 与备用 3090 均被占用,请先关掉一个 host" }
  Log "端口: agentd=$agentdPort host=$hostPort$(if ($agentdPort -ne 8103 -or $hostPort -ne 3080) { ' (缺省端口被占,已退备用)' })"
  if (Test-Path $skillDir) { throw "skills\scene-audit 已存在(上次冒烟未清理?),请先手动删除" }

  # 官方源种子自检(缺则生成)。
  if (-not (Test-Path (Join-Path $root 'registry\index.json'))) {
    Log "== registry 种子缺失,生成 =="
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $root 'scripts\f11-seed-registry.ps1') | Out-Null
  }

  # 隔离 data:源配置 + 个人库 + keystore 全落临时目录;显式锚定仓内 registry。
  New-Item -ItemType Directory -Force $tmpData | Out-Null
  $registryUrl = "file:///" + ((Join-Path $root 'registry') -replace '\\', '/')
  $srcCfg = @{ sources = @(
      @{ id = 'official'; name = '官方源(冒烟)'; baseUrl = $registryUrl; enabled = $true },
      @{ id = 'broken'; name = '故意坏源'; baseUrl = 'file:///Z:/no-such-registry-xyz'; enabled = $true }
    )
  }
  [IO.File]::WriteAllText((Join-Path $tmpData 'store-sources.json'), ($srcCfg | ConvertTo-Json -Depth 6), $utf8)
  $env:FORGE_GEN_DATA_DIR = $tmpData
  Log "隔离 data: $tmpData"

  Log "== 构建 forge-agentd / store-mcp =="
  $ErrorActionPreference = 'Continue'
  cargo build -p forge-agentd -p store-mcp 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  cargo: $_" }
  $cargoExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($cargoExit -ne 0) {
    $agentdExe = Join-Path $root 'target\debug\forge-agentd.exe'
    $storeExe = Join-Path $root 'target\debug\store-mcp.exe'
    $logText = Get-Content $logFile -Raw -ErrorAction SilentlyContinue
    $locked = $logText -match 'failed to remove file|拒绝访问'
    if ($locked -and (Test-Path $agentdExe) -and (Test-Path $storeExe)) {
      Log "  cargo build 被运行中的实例锁住 exe,沿用已有产物"
    } else {
      throw "cargo build 失败(exit=$cargoExit)"
    }
  }

  Log "== 启动 agentd($agentdPort) =="
  $env:FORGE_AGENTD_ADDR = "127.0.0.1:$agentdPort"
  $script:procs += Start-Process -FilePath "target\debug\forge-agentd.exe" -PassThru -WindowStyle Hidden
  Remove-Item env:FORGE_AGENTD_ADDR -ErrorAction SilentlyContinue
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:$agentdPort/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "agentd 就绪超时" }

  Log "== 构建+启动 host($hostPort) =="
  $ErrorActionPreference = 'Continue'
  pnpm --filter @forge/host build 2>&1 | Select-Object -Last 2 | ForEach-Object { Log "  pnpm: $_" }
  $pnpmExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($pnpmExit -ne 0) { throw "pnpm --filter @forge/host build 失败(exit=$pnpmExit)" }
  $env:FORGE_HOST_PORT = "$hostPort"
  $env:FORGE_AGENTD_ORIGIN = "http://127.0.0.1:$agentdPort"
  $script:procs += Start-Process -FilePath "node" -ArgumentList "packages\host\dist\index.js" -PassThru -WindowStyle Hidden
  Remove-Item env:FORGE_HOST_PORT, env:FORGE_AGENTD_ORIGIN -ErrorAction SilentlyContinue
  $ok = $false; foreach ($i in 1..40) { try { $r = Invoke-WebRequest -Uri "http://127.0.0.1:$hostPort/api/forge/health" -UseBasicParsing -TimeoutSec 2; if ($r.StatusCode -eq 200) { $ok = $true; break } } catch {}; Start-Sleep -Milliseconds 250 }
  if (-not $ok) { throw "host 就绪超时" }
  $H = "http://127.0.0.1:$hostPort"
  Log "host 就绪($hostPort),全程经代理"

  # ── 腿 1:源清单 ──
  Log "== 腿1:源清单 =="
  $r = Invoke-Json GET "$H/api/forge/store/sources"
  Check ($r.status -eq 200) 'sources.200'
  $official = $r.json.sources | Where-Object { $_.id -eq 'official' } | Select-Object -First 1
  Check ($null -ne $official -and $official.enabled -eq $true) 'sources.official-present'
  Check ($official.hasToken -eq $false) 'sources.no-token-leak(hasToken=false)'
  Check (-not ($r.text -match 'token"\s*:\s*"')) 'sources.response-has-no-token-value'

  # ── 腿 2:搜索 + 坏源如实上报 ──
  Log "== 腿2:搜索 =="
  $r = Invoke-Json GET "$H/api/forge/store/search?q="
  Check ($r.status -eq 200) 'search.200'
  Check ($r.json.items.Count -eq 3) "search.3-packages(实 $($r.json.items.Count))"
  Check ($r.json.partial -eq $true) 'search.partial-flagged(坏源存在)'
  $brokenErr = $r.json.errors | Where-Object { $_.sourceId -eq 'broken' } | Select-Object -First 1
  Check ($null -ne $brokenErr -and $brokenErr.code -eq 'STORE_SOURCE_UNREACHABLE') "search.broken-source-reported(code=$($brokenErr.code))"
  $r2 = Invoke-Json GET "$H/api/forge/store/search?q=%E6%9C%A8%E8%B4%A8"  # 「木质」
  Check ($r2.json.items.Count -ge 1) "search.chinese-query-hit($($r2.json.items.Count))"
  $r3 = Invoke-Json GET "$H/api/forge/store/search?kind=skill"
  Check (@($r3.json.items | Where-Object { $_.package.kind -ne 'skill' }).Count -eq 0 -and $r3.json.items.Count -eq 1) "search.kind-filter($($r3.json.items.Count) skill)"

  # ── 腿 3:详情与清单 ──
  Log "== 腿3:详情与清单 =="
  $r = Invoke-Json GET "$H/api/forge/store/packages/official/forge.starter-props"
  Check ($r.status -eq 200 -and $r.json.summary.id -eq 'forge.starter-props') 'detail.200'
  Check ($r.json.versions -contains '1.0.0') "detail.versions($($r.json.versions -join ','))"
  $m = Invoke-Json GET "$H/api/forge/store/packages/official/forge.starter-props/1.0.0"
  Check ($m.status -eq 200) 'manifest.200'
  $files = $m.json.manifest.files
  Check ($files.Count -eq 3) "manifest.3-files(实 $($files.Count))"
  Check (@($files | Where-Object { $_.sha256.Length -ne 64 }).Count -eq 0) 'manifest.sha256-all-64hex'
  $wood = Invoke-Json GET "$H/api/forge/store/packages/official/forge.wood-pbr"
  Check ($wood.json.versions.Count -eq 2) "detail.wood-two-versions($($wood.json.versions -join ','))"

  # ── 腿 4:安装 asset-pack ──
  Log "== 腿4:安装 asset-pack =="
  $r = Invoke-Json POST "$H/api/forge/store/install" @{ sourceId = 'official'; pkgId = 'forge.starter-props'; version = '1.0.0' }
  Check ($r.status -eq 200 -and $r.json.taskId) "install.submitted(taskId=$($r.json.taskId))"
  $t = Wait-Task $H $r.json.taskId
  Check ($t.status -eq 'completed') "install.completed(status=$($t.status) err=$($t.error.code) $($t.error.message))"
  $assetPaths = $t.result.assetPaths
  Check ($assetPaths.Count -eq 3) "install.3-assets($($assetPaths -join ', '))"
  # 落地核验:文件 + .meta + provenance 契约(08 Errata E-08-001)
  $chair = Join-Path $demo "Content\Meshes\starter_chair.gltf"
  Check (Test-Path $chair) 'install.mesh-on-disk(按扩展名分流到 Meshes)'
  Check (Test-Path (Join-Path $demo "Content\Textures\starter_dot.png")) 'install.texture-on-disk'
  Check (Test-Path (Join-Path $demo "Content\Materials\starter_red.rxmat")) 'install.material-on-disk'
  $metaText = [IO.File]::ReadAllText("$chair.meta")
  Check ($metaText -match 'origin:\s*store-install') 'install.provenance-origin=store-install'
  Check ($metaText -match 'forge\.starter-props') 'install.provenance-packageId'
  Check ($metaText -match 'CC0-1\.0') 'install.provenance-license'

  # ── 腿 5:重复安装 409 ──
  Log "== 腿5:重复安装 =="
  $dup = Invoke-Json POST "$H/api/forge/store/install" @{ sourceId = 'official'; pkgId = 'forge.starter-props'; version = '1.0.0' }
  $dupT = Wait-Task $H $dup.json.taskId
  Check ($dupT.status -eq 'failed' -and $dupT.error.code -eq 'STORE_ALREADY_INSTALLED') "install.duplicate-rejected(code=$($dupT.error.code))"

  $inst = Invoke-Json GET "$H/api/forge/store/installed"
  Check (@($inst.json.installed | Where-Object { $_.packageId -eq 'forge.starter-props' }).Count -eq 1) 'installed.listed-once'

  # ── 腿 6:更新检查 ──
  Log "== 腿6:更新检查 =="
  $w = Invoke-Json POST "$H/api/forge/store/install" @{ sourceId = 'official'; pkgId = 'forge.wood-pbr'; version = '1.0.0' }
  $wt = Wait-Task $H $w.json.taskId
  Check ($wt.status -eq 'completed') "install.wood-1.0.0(status=$($wt.status) err=$($wt.error.message))"
  $u = Invoke-Json GET "$H/api/forge/store/updates"
  $woodU = $u.json.updates | Where-Object { $_.packageId -eq 'forge.wood-pbr' } | Select-Object -First 1
  Check ($null -ne $woodU -and $woodU.hasUpdate -eq $true -and $woodU.latest -eq '1.1.0') "updates.wood-has-update(current=$($woodU.current) latest=$($woodU.latest))"
  $starterU = $u.json.updates | Where-Object { $_.packageId -eq 'forge.starter-props' } | Select-Object -First 1
  Check ($starterU.hasUpdate -eq $false) 'updates.starter-up-to-date'

  # ── 腿 7:安装 skill 包 ──
  Log "== 腿7:安装 skill 包 =="
  $s = Invoke-Json POST "$H/api/forge/store/install" @{ sourceId = 'official'; pkgId = 'forge.skill-scene-audit' }
  $st = Wait-Task $H $s.json.taskId
  Check ($st.status -eq 'completed') "install.skill-completed(status=$($st.status) err=$($st.error.message))"
  Check (Test-Path (Join-Path $skillDir 'SKILL.md')) 'install.skill-on-disk(skills/scene-audit/SKILL.md)'
  $sl = Invoke-Json GET "$H/api/forge/skills/list"
  Check (@($sl.json.skills | Where-Object { $_.name -eq 'scene-audit' }).Count -eq 1) 'install.skill-visible-in-skills-list'

  # ── 腿 8:卸载两阶段(I-6) ──
  Log "== 腿8:卸载 Proposal 两阶段 =="
  $d1 = Invoke-Json POST "$H/api/forge/store/uninstall" @{ sourceId = 'official'; pkgId = 'forge.starter-props' }
  Check ($d1.status -eq 409 -and $d1.json.error.code -eq 'GOV_PROPOSAL_REQUIRED') "uninstall.first-409(code=$($d1.json.error.code))"
  $propId = $d1.json.error.proposalId
  Check ($null -ne $propId -and $propId -ne '') "uninstall.proposalId-returned($propId)"
  Check (Test-Path $chair) 'uninstall.not-removed-before-approval'
  $props = Invoke-Json GET "$H/api/forge/proposals"
  $p = $props.json.proposals | Where-Object { $_.id -eq $propId } | Select-Object -First 1
  Check ($null -ne $p -and $p.kind -eq 'store.uninstall' -and $p.status -eq 'pending') "uninstall.proposal-listed(kind=$($p.kind))"
  Check ($p.impact.removeAssets.Count -eq 3) "uninstall.proposal-impact-3-assets($($p.impact.removeAssets.Count))"
  $ap = Invoke-Json PATCH "$H/api/forge/proposals/$propId" @{ action = 'approve' }
  Check ($ap.json.status -eq 'approved') 'uninstall.approved'
  $d2 = Invoke-Json POST "$H/api/forge/store/uninstall" @{ sourceId = 'official'; pkgId = 'forge.starter-props' }
  Check ($d2.status -eq 200 -and $d2.json.taskId) "uninstall.second-accepted(status=$($d2.status))"
  $ut = Wait-Task $H $d2.json.taskId
  Check ($ut.status -eq 'completed') "uninstall.completed(status=$($ut.status) err=$($ut.error.message))"
  Check (-not (Test-Path $chair)) 'uninstall.mesh-removed'
  Check (-not (Test-Path "$chair.meta")) 'uninstall.meta-removed'
  $inst2 = Invoke-Json GET "$H/api/forge/store/installed"
  Check (@($inst2.json.installed | Where-Object { $_.packageId -eq 'forge.starter-props' }).Count -eq 0) 'uninstall.record-removed'

  # ── 腿 9:个人资产库 ──
  Log "== 腿9:个人资产库 =="
  $lib0 = Invoke-Json GET "$H/api/forge/store/library"
  Check ($lib0.status -eq 200 -and $lib0.json.items.Count -eq 0) "library.initially-empty($($lib0.json.items.Count))"
  # 用小图 f2w4_dot.png 收藏,再装进 Misc/(与既有 Textures/ 同名资产错开,清理时可精确删)。
  $add = Invoke-Json POST "$H/api/forge/store/library" @{ assetPath = 'Textures/f2w4_dot.png'; tags = @('冒烟') }
  Check ($add.status -eq 200 -and $add.json.item.sha256.Length -eq 64) "library.add-ok(sha=$($add.json.item.sha256.Substring(0,8)))"
  $libId = $add.json.item.id
  # 同内容二次入库 → 去重(仍 1 条)
  $add2 = Invoke-Json POST "$H/api/forge/store/library" @{ assetPath = 'Textures/f2w4_dot.png' }
  $lib1 = Invoke-Json GET "$H/api/forge/store/library"
  Check ($lib1.json.items.Count -eq 1) "library.dedup-single-entry($($lib1.json.items.Count))"
  $blobCount = (Get-ChildItem -Recurse -File (Join-Path $tmpData 'store\library\blobs') -ErrorAction SilentlyContinue).Count
  Check ($blobCount -eq 1) "library.single-blob-on-disk($blobCount)"
  $li = Invoke-Json POST "$H/api/forge/store/library/${libId}:install" @{ destFolder = 'Misc' }
  Check ($li.status -eq 200 -and $li.json.assetPath -eq 'Misc/f2w4_dot.png') "library.install-to-project($($li.json.assetPath))"
  Check (Test-Path (Join-Path $demo 'Content\Misc\f2w4_dot.png')) 'library.installed-file-on-disk'
  $rm = Invoke-Json DELETE "$H/api/forge/store/library/$libId"
  Check ($rm.status -eq 200) 'library.remove-ok'
  $lib2 = Invoke-Json GET "$H/api/forge/store/library"
  Check ($lib2.json.items.Count -eq 0) 'library.empty-after-remove'
  $blobCount2 = (Get-ChildItem -Recurse -File (Join-Path $tmpData 'store\library\blobs') -ErrorAction SilentlyContinue).Count
  Check ($blobCount2 -eq 0) "library.blob-gc-after-last-ref($blobCount2)"

  # ── 腿 10:错误面 ──
  Log "== 腿10:错误面 =="
  $nf = Invoke-Json GET "$H/api/forge/store/packages/official/no.such.package"
  Check ($nf.status -eq 404 -and $nf.json.error.code -eq 'STORE_PACKAGE_NOT_FOUND') "error.package-404(code=$($nf.json.error.code))"
  $ns = Invoke-Json GET "$H/api/forge/store/packages/no-such-source/x"
  Check ($ns.status -eq 404 -and $ns.json.error.code -eq 'STORE_SOURCE_NOT_FOUND') "error.source-404(code=$($ns.json.error.code))"
  $nt = Invoke-Json GET "$H/api/forge/store/tasks/stask_99999"
  Check ($nt.status -eq 404 -and $nt.json.error.code -eq 'STORE_TASK_NOT_FOUND') "error.task-404(code=$($nt.json.error.code))"

  Log ""
  Log "== 汇总 =="
  Log "PASS=$script:pass FAIL=$script:fail SKIP=$($script:skips.Count)"
  if ($script:fail -gt 0) { Log "失败项: $($script:failures -join ', ')" }

  $evidence = @{
    at       = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    wave     = "f11-w1-store"
    pass     = $script:pass
    fail     = $script:fail
    skipped  = $script:skips
    failures = $script:failures
    measured = @{
      packagesInOfficialSource = 3
      starterFilesInstalled    = $assetPaths.Count
      woodCurrent              = $woodU.current
      woodLatest               = $woodU.latest
      uninstallProposalId      = $propId
      libraryBlobAfterDedup    = $blobCount
      libraryBlobAfterGc       = $blobCount2
    }
    note      = "官方源 = 仓内 registry/(file:// 驱动,离线可跑);https 社区源腿本环境未实测,如实标注 DEV_ENV_DEGRADE 不充绿。"
    degraded  = @("https 社区源(HttpSource)未做真实网络实测")
    gateGreen = ($script:fail -eq 0)
  }
  $evPath = "evidence\f11-w1-store-$ts.json"
  [IO.File]::WriteAllText((Join-Path $root $evPath), ($evidence | ConvertTo-Json -Depth 6), $utf8)
  Log "证据: $evPath"
  if ($script:fail -gt 0) { exit 1 }
  Log "F11 wave.1/3 资产商店冒烟 PASS ($script:pass 断言全绿;https 源腿如实标注未测)"
  exit 0
} catch {
  Log "FAIL: $_"
  [IO.File]::WriteAllText((Join-Path $root "evidence\f11-w1-store-$ts.json"),
    (@{ at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ"); wave = "f11-w1-store"; pass = $script:pass; fail = $script:fail; failures = $script:failures; failed = "$_"; gateGreen = $false } | ConvertTo-Json -Depth 6), $utf8)
  exit 1
} finally {
  # 自清理:冒烟装进 projects/demo 的资产与技能一律清掉,不留改动进仓库。
  foreach ($p in $script:procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 600
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('forge-agentd', 'store-mcp', 'engine-scene-mcp', 'engine-host') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
  Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -match 'packages[\\/]host[\\/]dist[\\/]index\.js' } |
    ForEach-Object { try { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } catch {} }
  # 残留资产(含 .meta)与技能目录。种子包的落地名都与 demo 原有资产错开
  # (starter_* / pbr_wood_*),因此可以按名精确清理,不会误删用户资产。
  foreach ($rel in @(
      'Content\Meshes\starter_chair.gltf', 'Content\Textures\starter_dot.png', 'Content\Materials\starter_red.rxmat',
      'Content\Textures\pbr_wood_albedo.png', 'Content\Textures\pbr_wood_normal.png', 'Content\Textures\pbr_wood_roughness.png'
    )) {
    $abs = Join-Path $demo $rel
    Remove-Item -Force $abs, "$abs.meta" -ErrorAction SilentlyContinue
  }
  # 个人库腿装进 Content/Misc/ 的副本(与既有资产错开)
  $miscDot = Join-Path $demo 'Content\Misc\f2w4_dot.png'
  Remove-Item -Force $miscDot, "$miscDot.meta" -ErrorAction SilentlyContinue
  if ((Test-Path (Join-Path $demo 'Content\Misc')) -and
      -not (Get-ChildItem (Join-Path $demo 'Content\Misc') -Force -ErrorAction SilentlyContinue)) {
    Remove-Item -Force (Join-Path $demo 'Content\Misc') -ErrorAction SilentlyContinue
  }
  Remove-Item -Recurse -Force (Join-Path $demo '.forge\store') -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force (Join-Path $demo '.forge\tmp\store') -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force $skillDir -ErrorAction SilentlyContinue
  Remove-Item -Recurse -Force $tmpData -ErrorAction SilentlyContinue
  if ($null -ne $savedGenDir -and $savedGenDir -ne '') { $env:FORGE_GEN_DATA_DIR = $savedGenDir }
  else { Remove-Item env:FORGE_GEN_DATA_DIR -ErrorAction SilentlyContinue }
}
