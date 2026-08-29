# F11 wave.2 Skill 管理冒烟(G-F11-2 的栈级腿):CRUD 全周期 + 校验 + 删除 Proposal 两阶段
#   + 启停对索引的影响 + 既有 13 篇向后兼容。
# 流程全经 host http://127.0.0.1:3080(forgeProxy 透传 /api/forge/skills 前缀)。
# 腿:①list 基线(既有技能全解析) ②create(缺省模板)+重名 409+非法名 400 ③read 全文与 front 结构
#   ④validate(合法/非法草稿) ⑤update 覆写 ⑥启停 config/write 往返 ⑦delete 两阶段(409→批准→200)
#   ⑧终态复原核验(临时技能已清、config 已还原)。
# 自清理:临时技能名带 pid 后缀,finally 里无条件删目录;skills-config.json 测前备份测后写回。
# 用法: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\f11-w2-skill-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$logFile = "evidence\f11-w2-skill-smoke-$ts.log"
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line }
$script:pass = 0; $script:fail = 0; $script:failures = @(); $script:skips = @()
function Check($cond, $name) {
  if ($cond) { $script:pass++; Log "  PASS $name" }
  else { $script:fail++; $script:failures += $name; Log "  FAIL $name" }
}
function Skip($name, $reason) { $script:skips += "$name($reason)"; Log "  SKIP=not-triggered $name($reason)" }

function Invoke-Json($method, $url, $bodyObj = $null, $timeoutSec = 30) {
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

function Test-PortFree($port) {
  try {
    $l = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, $port)
    $l.Start(); $l.Stop(); return $true
  } catch { return $false }
}

New-Item -ItemType Directory -Force evidence | Out-Null
$script:procs = @()
$script:startTime = Get-Date
$tmpSkill = "smoke-tmp-$PID"
$tmpSkillDir = Join-Path $root "skills\$tmpSkill"
$cfgPath = Join-Path $root "data\skills-config.json"
$cfgBackup = if (Test-Path $cfgPath) { [IO.File]::ReadAllText($cfgPath) } else { $null }
$utf8 = New-Object System.Text.UTF8Encoding($false)

try {
  # 端口:缺省 8103/3080;被占(开发者的 dev 实例常驻)则退到备用端口并排跑,不杀别人的进程。
  $agentdPort = if (Test-PortFree 8103) { 8103 } elseif (Test-PortFree 8123) { 8123 } else { throw "8103 与备用 8123 均被占用,请先关掉一个 agentd" }
  $hostPort = if (Test-PortFree 3080) { 3080 } elseif (Test-PortFree 3090) { 3090 } else { throw "3080 与备用 3090 均被占用,请先关掉一个 host" }
  Log "端口: agentd=$agentdPort host=$hostPort$(if ($agentdPort -ne 8103 -or $hostPort -ne 3080) { ' (缺省端口被占,已退备用)' })"

  Log "== 构建 forge-agentd =="
  $ErrorActionPreference = 'Continue'
  cargo build -p forge-agentd 2>&1 | Select-Object -Last 3 | ForEach-Object { Log "  cargo: $_" }
  $cargoExit = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($cargoExit -ne 0) {
    $logText = Get-Content $logFile -Raw -ErrorAction SilentlyContinue
    $locked = $logText -match 'failed to remove file|拒绝访问'
    if ($locked -and (Test-Path (Join-Path $root 'target\debug\forge-agentd.exe'))) {
      Log "  cargo build 被运行中的实例锁住 exe,沿用已有产物"
    } else {
      throw "cargo build -p forge-agentd 失败(exit=$cargoExit)"
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

  # ── 腿 1:list 基线(既有技能全解析,向后兼容) ──
  Log "== 腿1:list 基线 =="
  $r = Invoke-Json GET "$H/api/forge/skills/list"
  $baseline = $r.json.skills
  $diskCount = @(Get-ChildItem (Join-Path $root 'skills') -Directory | Where-Object { Test-Path (Join-Path $_.FullName 'SKILL.md') }).Count
  Check ($r.status -eq 200) 'list.200'
  Check ($baseline.Count -ge 13) "list.count>=13(实 $($baseline.Count),磁盘 $diskCount)"
  Check ($baseline.Count -eq $diskCount) "list.backward-compat(既有 SKILL.md 全解析:$($baseline.Count)/$diskCount)"
  $ac = $baseline | Where-Object { $_.name -eq 'asset-cleanup' } | Select-Object -First 1
  Check ($null -ne $ac -and $ac.enabled -eq $true) 'list.asset-cleanup-enabled'
  Check ($null -ne $ac -and $ac.builtin -eq $true) 'list.builtin-flag'

  # ── 腿 2:create ──
  Log "== 腿2:create =="
  $r = Invoke-Json POST "$H/api/forge/skills" @{ name = $tmpSkill }
  Check ($r.status -eq 200 -or $r.status -eq 201) "create.ok(status=$($r.status))"
  Check (Test-Path (Join-Path $tmpSkillDir 'SKILL.md')) 'create.file-on-disk'
  $dup = Invoke-Json POST "$H/api/forge/skills" @{ name = $tmpSkill }
  Check ($dup.status -eq 409 -and $dup.json.error.code -eq 'SKILL_ALREADY_EXISTS') "create.dup-409(code=$($dup.json.error.code))"
  $bad = Invoke-Json POST "$H/api/forge/skills" @{ name = 'Bad_Name' }
  Check ($bad.status -eq 400 -and $bad.json.error.code -eq 'SKILL_NAME_INVALID') "create.badname-400(code=$($bad.json.error.code))"

  # ── 腿 3:read ──
  Log "== 腿3:read =="
  $r = Invoke-Json GET "$H/api/forge/skills/$tmpSkill"
  $disk = [IO.File]::ReadAllText((Join-Path $tmpSkillDir 'SKILL.md'))
  Check ($r.status -eq 200) 'read.200'
  Check ($r.json.content -eq $disk) 'read.byte-identical-to-disk'
  Check ($r.json.front.name -eq $tmpSkill) "read.front.name=$($r.json.front.name)"
  Check ($null -ne $r.json.front.description -and $r.json.front.description -ne '') 'read.front.description-nonempty'
  $miss = Invoke-Json GET "$H/api/forge/skills/no-such-skill-xyz"
  Check ($miss.status -eq 404) "read.missing-404(status=$($miss.status))"

  # ── 腿 4:validate(缺省模板自身合法 + 残缺草稿报错) ──
  Log "== 腿4:validate =="
  $r = Invoke-Json POST "$H/api/forge/skills/${tmpSkill}:validate" @{}
  Check ($r.status -eq 200 -and $r.json.valid -eq $true) "validate.template-self-valid(valid=$($r.json.valid) errors=$($r.json.errors.Count))"
  $draft = "---`nname: $tmpSkill`ndescription: 残缺草稿`n---`n`n# 标题`n只有一句话。`n"
  $r2 = Invoke-Json POST "$H/api/forge/skills/${tmpSkill}:validate" @{ content = $draft }
  Check ($r2.status -eq 200 -and $r2.json.valid -eq $false -and $r2.json.errors.Count -gt 0) "validate.incomplete-body-rejected(errors=$($r2.json.errors.Count))"

  # ── 腿 5:update ──
  Log "== 腿5:update =="
  $marker = "冒烟覆写标记-$ts"
  $newContent = $disk -replace '# ', "# $marker "
  $r = Invoke-Json PUT "$H/api/forge/skills/$tmpSkill" @{ content = $newContent }
  Check ($r.status -eq 200) "update.200(status=$($r.status))"
  $after = [IO.File]::ReadAllText((Join-Path $tmpSkillDir 'SKILL.md'))
  Check ($after.Contains($marker)) 'update.persisted'
  $miss2 = Invoke-Json PUT "$H/api/forge/skills/no-such-skill-xyz" @{ content = $newContent }
  Check ($miss2.status -eq 404) "update.missing-404(status=$($miss2.status))"

  # ── 腿 6:启停往返 ──
  Log "== 腿6:启停往返 =="
  $r = Invoke-Json POST "$H/api/forge/skills/config/write" @{ disabled = @($tmpSkill) }
  Check ($r.status -eq 200) 'toggle.write-200'
  $l = Invoke-Json GET "$H/api/forge/skills/list"
  $t = $l.json.skills | Where-Object { $_.name -eq $tmpSkill } | Select-Object -First 1
  Check ($null -ne $t -and $t.enabled -eq $false) "toggle.disabled-reflected(enabled=$($t.enabled))"
  $r = Invoke-Json POST "$H/api/forge/skills/config/write" @{ disabled = @() }
  $l2 = Invoke-Json GET "$H/api/forge/skills/list"
  $t2 = $l2.json.skills | Where-Object { $_.name -eq $tmpSkill } | Select-Object -First 1
  Check ($null -ne $t2 -and $t2.enabled -eq $true) 'toggle.reenabled'

  # ── 腿 7:delete 两阶段(I-6 人类确认门) ──
  Log "== 腿7:delete Proposal 两阶段 =="
  $d1 = Invoke-Json DELETE "$H/api/forge/skills/$tmpSkill"
  Check ($d1.status -eq 409 -and $d1.json.error.code -eq 'GOV_PROPOSAL_REQUIRED') "delete.first-409(code=$($d1.json.error.code))"
  $pid1 = $d1.json.error.proposalId
  Check ($null -ne $pid1 -and $pid1 -ne '') "delete.proposalId-returned($pid1)"
  Check (Test-Path $tmpSkillDir) 'delete.not-deleted-before-approval'
  $props = Invoke-Json GET "$H/api/forge/proposals"
  $p = $props.json.proposals | Where-Object { $_.id -eq $pid1 } | Select-Object -First 1
  Check ($null -ne $p -and $p.kind -eq 'skill.delete' -and $p.status -eq 'pending') "delete.proposal-listed(kind=$($p.kind) status=$($p.status))"
  $ap = Invoke-Json PATCH "$H/api/forge/proposals/$pid1" @{ action = 'approve' }
  Check ($ap.status -eq 200 -and $ap.json.status -eq 'approved') "delete.approved(status=$($ap.json.status))"
  $d2 = Invoke-Json DELETE "$H/api/forge/skills/$tmpSkill"
  Check ($d2.status -eq 200) "delete.second-200(status=$($d2.status))"
  Check (-not (Test-Path $tmpSkillDir)) 'delete.dir-removed'
  $l3 = Invoke-Json GET "$H/api/forge/skills/list"
  Check (@($l3.json.skills | Where-Object { $_.name -eq $tmpSkill }).Count -eq 0) 'delete.gone-from-list'

  # ── 腿 8:终态复原 ──
  Log "== 腿8:终态复原 =="
  Check ($l3.json.skills.Count -eq $baseline.Count) "final.count-restored($($l3.json.skills.Count) == $($baseline.Count))"

  Log ""
  Log "== 汇总 =="
  Log "PASS=$script:pass FAIL=$script:fail SKIP=$($script:skips.Count)"
  if ($script:fail -gt 0) { Log "失败项: $($script:failures -join ', ')" }
  if ($script:skips.Count -gt 0) { Log "跳过项: $($script:skips -join ', ')" }

  $evidence = @{
    at      = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    wave    = "f11-w2-skill"
    pass    = $script:pass
    fail    = $script:fail
    skipped = $script:skips
    failures = $script:failures
    measured = @{
      baselineSkillCount = $baseline.Count
      diskSkillDirCount  = $diskCount
      finalSkillCount    = $l3.json.skills.Count
      tmpSkillName       = $tmpSkill
      proposalId         = $pid1
    }
    gateGreen = ($script:fail -eq 0)
  }
  $evPath = "evidence\f11-w2-skill-$ts.json"
  [IO.File]::WriteAllText((Join-Path $root $evPath), ($evidence | ConvertTo-Json -Depth 6), $utf8)
  Log "证据: $evPath"
  if ($script:fail -gt 0) { exit 1 }
  Log "F11 wave.2 Skill 管理冒烟 PASS ($script:pass 断言全绿)"
  exit 0
} catch {
  Log "FAIL: $_"
  [IO.File]::WriteAllText((Join-Path $root "evidence\f11-w2-skill-$ts.json"),
    (@{ at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ"); wave = "f11-w2-skill"; pass = $script:pass; fail = $script:fail; failures = $script:failures; failed = "$_"; gateGreen = $false } | ConvertTo-Json -Depth 6), $utf8)
  exit 1
} finally {
  # 自清理:临时技能目录 + skills-config.json 还原(冒烟绝不留改动进仓库)
  if (Test-Path $tmpSkillDir) { Remove-Item -Recurse -Force $tmpSkillDir -ErrorAction SilentlyContinue }
  if ($null -ne $cfgBackup) { [IO.File]::WriteAllText($cfgPath, $cfgBackup, $utf8) }
  elseif (Test-Path $cfgPath) { Remove-Item $cfgPath -ErrorAction SilentlyContinue }
  foreach ($p in $script:procs) { try { if (-not $p.HasExited) { $p.Kill() } } catch {} }
  Start-Sleep -Milliseconds 500
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { @('forge-agentd', 'engine-scene-mcp', 'engine-host', 'store-mcp') -contains $_.Name -and $_.StartTime -ge $script:startTime } |
    ForEach-Object { try { $_.Kill() } catch {} }
  Get-CimInstance Win32_Process -Filter "Name='node.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -match 'packages[\\/]host[\\/]dist[\\/]index\.js' } |
    ForEach-Object { try { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue } catch {} }
}
