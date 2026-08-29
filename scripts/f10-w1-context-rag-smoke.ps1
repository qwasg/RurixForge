# F10 wave.1 语义索引/RAG 检索冒烟:context-mcp 六工具全周期(临时夹具项目)
#   + demo 项目索引实况(描述覆盖/词法检索命中)。
# 协议:JSON-RPC 请求写 UTF-8 文件,经 Start-Process 句柄重定向直连 stdio
#   (勿用 PowerShell 文本管道:会按控制台码页重编码,中文变 '?',实测坑)。
# 前提:cargo build -p context-mcp 可过;projects\demo 已批量回填描述。
# 用法: powershell -ExecutionPolicy Bypass -File scripts\f10-w1-context-rag-smoke.ps1 ; exit 0 = PASS
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $root
$ts = Get-Date -Format "yyyyMMdd-HHmmss"
$logFile = "evidence\f10-w1-context-rag-smoke-$ts.log"
New-Item -ItemType Directory -Force evidence | Out-Null
function Log($m) { $line = "[{0}] {1}" -f (Get-Date -Format "HH:mm:ss"), $m; $line; Add-Content $logFile $line -Encoding UTF8 }

$utf8 = New-Object System.Text.UTF8Encoding($false)
$exe = Join-Path $root "target\debug\context-mcp.exe"

# 请求批 → 响应表(id → 工具内层 JSON;经文件句柄重定向,字节不经 PS 文本层)。
function McpBatch($projectRoot, $calls) {
  $reqPath = Join-Path $env:TEMP "f10-req-$([guid]::NewGuid().ToString('N')).jsonl"
  $respPath = Join-Path $env:TEMP "f10-resp-$([guid]::NewGuid().ToString('N')).jsonl"
  $lines = New-Object System.Collections.Generic.List[string]
  $lines.Add((@{ jsonrpc = "2.0"; id = 1; method = "initialize"; params = @{ protocolVersion = "2024-11-05"; capabilities = @{}; clientInfo = @{ name = "f10-smoke"; version = "0.1.0" } } } | ConvertTo-Json -Depth 8 -Compress))
  $lines.Add((@{ jsonrpc = "2.0"; method = "notifications/initialized"; params = @{} } | ConvertTo-Json -Depth 4 -Compress))
  $id = 10
  foreach ($c in $calls) {
    $lines.Add((@{ jsonrpc = "2.0"; id = $id; method = "tools/call"; params = @{ name = $c.tool; arguments = $c.args } } | ConvertTo-Json -Depth 12 -Compress))
    $id++
  }
  [System.IO.File]::WriteAllLines($reqPath, $lines, $utf8)
  $p = Start-Process -FilePath $exe -ArgumentList "--project", $projectRoot -RedirectStandardInput $reqPath -RedirectStandardOutput $respPath -NoNewWindow -Wait -PassThru
  if ($p.ExitCode -ne 0) { throw "context-mcp 退出码 $($p.ExitCode)" }
  $out = @{}
  foreach ($line in [System.IO.File]::ReadAllLines($respPath, $utf8)) {
    if ($line.Trim() -eq "") { continue }
    $v = $line | ConvertFrom-Json
    if ($v.id -ge 10 -and $v.result.content) {
      $out[[int]$v.id] = ($v.result.content[0].text | ConvertFrom-Json)
    }
  }
  Remove-Item $reqPath, $respPath -ErrorAction SilentlyContinue
  return $out
}

$gates = New-Object System.Collections.Generic.List[object]
function Gate($name, $pass, $detail) {
  $gates.Add(@{ gate = $name; pass = [bool]$pass; detail = "$detail" })
  if ($pass) { Log "PASS $name — $detail" } else { Log "FAIL $name — $detail" }
  if (-not $pass) { throw "门失败: $name" }
}

try {
  Log "== 构建 context-mcp =="
  # cmd 合并流:$ErrorActionPreference=Stop 下 PS 的 2>&1 会把 stderr 行变 ErrorRecord 直接抛(实测坑)。
  cmd /c "cargo build -p context-mcp 2>&1" | Out-Null
  if ($LASTEXITCODE -ne 0) { throw "cargo build -p context-mcp 失败" }
  if (-not (Test-Path $exe)) { throw "缺二进制 $exe" }

  # ---------- 腿 1:临时夹具项目全周期 ----------
  Log "== 腿 1:夹具项目六工具全周期 =="
  $fix = Join-Path $env:TEMP "f10-smoke-fixture-$ts"
  New-Item -ItemType Directory -Force (Join-Path $fix "Content\Scripts") | Out-Null
  [System.IO.File]::WriteAllText((Join-Path $fix "Content\Scripts\door.rx"),
    "// 开门逻辑:角度计算`n#[export(c)]`npub fn door_open_angle() -> f32 { 90.0 }`n", $utf8)

  $r = McpBatch $fix @(
    @{ tool = "context_search"; args = @{ query = "开门" } },                 # 10:未建 → INDEX_NOT_BUILT
    @{ tool = "context_index_build"; args = @{} },                            # 11
    @{ tool = "context_search"; args = @{ query = "开门角度" } },             # 12
    @{ tool = "asset_describe_batch"; args = @{ mode = "missing" } },         # 13
    @{ tool = "asset_set_description"; args = @{ assetPath = "Scripts/door.rx"; description = "开门逻辑脚本,输出门的开启角度"; tags = @("逻辑", "门"); source = "agent-facts"; model = "smoke" } }, # 14
    @{ tool = "context_index_build"; args = @{} },                            # 15
    @{ tool = "context_search"; args = @{ query = "开启角度脚本" } },         # 16
    @{ tool = "asset_describe_batch"; args = @{ mode = "missing" } },         # 17
    @{ tool = "context_index_status"; args = @{} }                            # 18
  )
  Gate "fixture.unbuilt-explicit" ($r[10].error -eq "INDEX_NOT_BUILT") "error=$($r[10].error)"
  Gate "fixture.build-lexical" ($r[11].tier -eq "lexical" -and $r[11].docs -gt 0) "docs=$($r[11].docs) tier=$($r[11].tier)"
  Gate "fixture.search-hit-with-tier-note" ($r[12].tier -eq "lexical" -and $r[12].hits.Count -gt 0 -and $r[12].note) "hits=$($r[12].hits.Count) note非空=$([bool]$r[12].note)"
  Gate "fixture.describe-batch-missing" ($r[13].items.Count -gt 0 -and $r[13].items[0].missing) "items=$($r[13].items.Count)"
  Gate "fixture.set-description-ok" ($r[14].ok -eq $true) "ok=$($r[14].ok)"
  Gate "fixture.rebuild" ($r[15].docs -gt 0) "docs=$($r[15].docs)"
  $hitDoor = $r[16].hits | Where-Object { $_.path -like "*door.rx" } | Select-Object -First 1
  Gate "fixture.search-hits-description" ($null -ne $hitDoor -and $hitDoor.description -like "*开门*") "top=$($r[16].hits[0].path)"
  Gate "fixture.missing-cleared" ($r[17].totalMissing -eq 0) "totalMissing=$($r[17].totalMissing)"
  Gate "fixture.status-built" ($r[18].built -eq $true) "docCount=$($r[18].docCount)"
  $fixtureDocs = $r[11].docs
  Remove-Item -Recurse -Force $fix -ErrorAction SilentlyContinue

  # ---------- 腿 2:demo 项目实况 ----------
  Log "== 腿 2:demo 项目索引实况 =="
  $demo = Join-Path $root "projects\demo"
  $r2 = McpBatch $demo @(
    @{ tool = "context_index_build"; args = @{} },                            # 10
    @{ tool = "context_index_status"; args = @{} },                           # 11
    @{ tool = "context_search"; args = @{ query = "木质 椅子 家具"; topK = 5 } },   # 12
    @{ tool = "context_search"; args = @{ query = "迷宫 钥匙 开门"; topK = 5; kinds = @("graph") } }, # 13(图级文档继承 .meta 中文描述)
    # 词法档限界(如实):实体名为英文,中文查询跨语言不命中——用 player 词;hybrid 档(embedding)可跨语言。
    @{ tool = "context_search"; args = @{ query = "player"; topK = 5; kinds = @("entity") } }         # 14
  )
  Gate "demo.build" ($r2[10].docs -gt 0) "docs=$($r2[10].docs) tier=$($r2[10].tier) durationMs=$($r2[10].durationMs)"
  Gate "demo.descriptions-covered" ($r2[11].assetsMissingDescription -eq 0) "missing=$($r2[11].assetsMissingDescription) docCount=$($r2[11].docCount)"
  $chairTop = $r2[12].hits | Select-Object -First 2
  Gate "demo.search-chair" (($chairTop | Where-Object { $_.path -like "*chair*" }).Count -ge 1) "top1=$($r2[12].hits[0].path)"
  Gate "demo.search-graph-kind-filter" ($r2[13].hits.Count -gt 0 -and ($r2[13].hits | Where-Object { $_.kind -ne "graph" }).Count -eq 0) "hits=$($r2[13].hits.Count) top=$($r2[13].hits[0].path)"
  Gate "demo.search-entity-kind-filter" ($r2[14].hits.Count -gt 0 -and ($r2[14].hits | Where-Object { $_.kind -ne "entity" }).Count -eq 0) "hits=$($r2[14].hits.Count) top=$($r2[14].hits[0].title)"

  # ---------- 证据落盘 ----------
  $evidence = @{
    at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    wave = "f10-w1-context-rag"
    gates = $gates
    measured = @{
      fixtureDocs = $fixtureDocs
      demoDocCount = $r2[11].docCount
      demoTier = $r2[11].tier
      demoMissingDescription = $r2[11].assetsMissingDescription
      demoEmbeddingConfigured = $r2[11].embeddingConfigured
      demoVectorCount = $r2[11].vectorCount
      buildDurationMs = $r2[10].durationMs
      chairTop1 = $r2[12].hits[0].path
      graphTop1 = $r2[13].hits[0].path
      entityTop1 = $r2[14].hits[0].title
    }
    note = "tier=lexical 为如实档位(未配 embedding 渠道);配置 data/llm-embedding.json + keystore[embedding] 后 context_index_build 自动升 hybrid。"
  }
  $evPath = "evidence\f10-w1-context-rag-$ts.json"
  [System.IO.File]::WriteAllText((Join-Path $root $evPath), ($evidence | ConvertTo-Json -Depth 6), $utf8)
  Log "证据: $evPath"
  Log "F10 wave.1 语义索引/RAG 检索冒烟 PASS ($($gates.Count) 门全绿)"
  exit 0
} catch {
  Log "FAIL: $_"
  $evidence = @{
    at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    wave = "f10-w1-context-rag"
    gates = $gates
    failed = "$_"
  }
  [System.IO.File]::WriteAllText((Join-Path $root "evidence\f10-w1-context-rag-$ts.json"), ($evidence | ConvertTo-Json -Depth 6), $utf8)
  exit 1
}
