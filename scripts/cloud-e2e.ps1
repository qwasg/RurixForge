# 云模式端到端（15_CLOUD_SERVICE.md）：假上游 → forge-cloud → agentd 全链路。
#   1. 未登录时本地引擎显式失败 CLOUD_LOGIN_REQUIRED（不再静默 mock）
#   2. 管理员配置 API Key 上游账号 + 模型定价 + 兑换码
#   3. agentd 注册（签发设备 Key）→ 兑换 → 选 cloud:<模型> 跑两轮 → 余额扣减、用量入账
#   4. 用户自建 API Key 直连网关：chat 与 responses（透传）
#   5. OAuth 授权回填建 Codex 订阅账号，停用 API Key 账号后 responses 透传与 chat→responses 转换都走 Codex
#   6. 登出
# 用法：pnpm test:cloud-e2e（仓库根）；加 -KeepRunning 保留进程便于手工排查。
param([switch]$KeepRunning)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$build = Join-Path (Split-Path -Parent $repo) 'RurixForge-build'
$env:Path = "$env:USERPROFILE\sdk\go1.26.8\bin;$env:USERPROFILE\go\bin;$env:Path"
$tmp = Join-Path $env:TEMP ("forge-cloud-e2e-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force -Path $tmp, $build | Out-Null

$cloudUrl = 'http://127.0.0.1:8111'
$fakeUrl = 'http://127.0.0.1:8199'
$agentdUrl = 'http://127.0.0.1:8113'
$e2eDb = 'forge_cloud_e2e'
$adminEmail = 'admin@e2e.local'
$adminPassword = 'e2e-admin-pass'
$userEmail = 'user' + (Get-Random -Maximum 99999) + '@e2e.local'
$userPassword = 'e2e-user-pass'
$procs = @()
$failures = @()

function Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Pass($msg) { Write-Host "  PASS $msg" -ForegroundColor Green }
function Fail($msg) { Write-Host "  FAIL $msg" -ForegroundColor Red; $script:failures += $msg }
function Check($cond, $msg) { if ($cond) { Pass $msg } else { Fail $msg } }

function Api {
    param([string]$Method, [string]$Url, $Body = $null, [string]$Token = '', [string]$ApiKey = '')
    $headers = @{}
    if ($Token) { $headers['Authorization'] = "Bearer $Token" }
    if ($ApiKey) { $headers['Authorization'] = "Bearer $ApiKey" }
    $params = @{ Method = $Method; Uri = $Url; Headers = $headers; TimeoutSec = 180 }
    if ($null -ne $Body) {
        $json = $Body | ConvertTo-Json -Depth 12 -Compress
        $params['Body'] = [System.Text.Encoding]::UTF8.GetBytes($json)
        $params['ContentType'] = 'application/json; charset=utf-8'
    }
    try {
        return Invoke-RestMethod @params
    } catch {
        $status = 0
        if ($_.Exception.Response) { $status = [int]$_.Exception.Response.StatusCode }
        $detail = $_.ErrorDetails.Message
        throw "HTTP $status $Method $Url :: $detail"
    }
}

function Wait-Http($url, $seconds = 60) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $r = Invoke-WebRequest -Uri $url -UseBasicParsing -TimeoutSec 3
            if ($r.StatusCode -ge 200) { return }
        } catch {
            # 假上游 /v1/models 无密钥时返回 401，能连上即视为已监听。
            if ($_.Exception.Response) { return }
        }
        Start-Sleep -Milliseconds 500
    }
    throw "等待 $url 超时"
}

function Start-Bg($exe, [string[]]$argv, $name) {
    $out = Join-Path $tmp "$name.out.log"
    $err = Join-Path $tmp "$name.err.log"
    $sp = @{
        FilePath               = $exe
        PassThru               = $true
        NoNewWindow            = $true
        RedirectStandardOutput = $out
        RedirectStandardError  = $err
    }
    if ($argv -and @($argv).Count -gt 0) { $sp.ArgumentList = $argv }
    $p = Start-Process @sp
    $script:procs += $p
    return $p
}

try {
    Step '开发依赖（PostgreSQL + Redis）'
    docker compose -f (Join-Path $repo 'cloud/deploy/docker-compose.dev.yml') up -d --wait | Out-Null
    docker exec forge-cloud-dev-postgres-1 psql -U forge -d postgres -q -c "DROP DATABASE IF EXISTS $e2eDb WITH (FORCE)" | Out-Null
    docker exec forge-cloud-dev-postgres-1 psql -U forge -d postgres -q -c "CREATE DATABASE $e2eDb" | Out-Null
    docker exec forge-cloud-dev-redis-1 redis-cli -n 15 FLUSHDB | Out-Null

    Step '构建 forge-cloud 与 forge-agentd'
    $cloudExe = Join-Path $build 'forge-cloud-e2e.exe'
    go -C (Join-Path $repo 'cloud') build -o $cloudExe ./cmd/forge-cloud
    if ($LASTEXITCODE -ne 0) { throw 'forge-cloud 构建失败' }
    $env:CARGO_TARGET_DIR = Join-Path $build 'target-cloud'
    Push-Location $repo
    cargo build -p forge-agentd --quiet
    $cargoExit = $LASTEXITCODE
    Pop-Location
    if ($cargoExit -ne 0) { throw 'forge-agentd 构建失败' }
    $agentdExe = Join-Path $env:CARGO_TARGET_DIR 'debug\forge-agentd.exe'

    Step '启动假上游与 forge-cloud'
    Start-Bg $cloudExe @('dev', 'fake-upstream', '--addr', '127.0.0.1:8199') 'fake-upstream' | Out-Null
    $env:FORGE_CLOUD_ENV = 'dev'
    $env:FORGE_CLOUD_ADDR = '127.0.0.1:8111'
    $env:FORGE_CLOUD_PUBLIC_URL = $cloudUrl
    $env:FORGE_CLOUD_DATABASE_URL = "postgres://forge:forge@127.0.0.1:5432/$e2eDb" + '?sslmode=disable'
    $env:FORGE_CLOUD_REDIS_URL = 'redis://127.0.0.1:6379/15'
    $env:FORGE_CLOUD_ADMIN_EMAIL = $adminEmail
    $env:FORGE_CLOUD_ADMIN_PASSWORD = $adminPassword
    $env:FORGE_CLOUD_CODEX_BASE_URL = "$fakeUrl/codex"
    $env:FORGE_CLOUD_CHATGPT_BASE_URL = $fakeUrl
    $env:FORGE_CLOUD_OPENAI_AUTH_URL = $fakeUrl
    Start-Bg $cloudExe @('serve') 'forge-cloud' | Out-Null
    Wait-Http "$fakeUrl/v1/models" 30
    Wait-Http "$cloudUrl/healthz" 60
    Pass 'forge-cloud /healthz 200'

    Step '管理员登录并配置上游、模型、兑换码'
    $adminLogin = Api POST "$cloudUrl/api/v1/auth/login" @{ email = $adminEmail; password = $adminPassword; issueDeviceKey = $false; device = @{ id = 'e2e-admin'; name = 'e2e'; platform = 'web'; appVersion = 'e2e' } }
    $admin = $adminLogin.accessToken
    Check ($adminLogin.user.role -eq 'admin') '引导管理员可登录且为 admin'
    $groups = Api GET "$cloudUrl/api/admin/groups" -Token $admin
    $defaultGroup = ($groups.items | Where-Object { $_.isDefault })[0].id
    $apiAccount = Api POST "$cloudUrl/api/admin/accounts" @{ name = 'fake-openai'; platform = 'openai'; baseUrl = "$fakeUrl/v1"; apiKey = 'sk-fake-upstream'; supportsResponses = $true; groupIds = @($defaultGroup); priority = 10 } -Token $admin
    Check (-not ($apiAccount | ConvertTo-Json -Depth 8).Contains('sk-fake-upstream')) '上游账号响应不回显 API Key'
    Api POST "$cloudUrl/api/admin/models" @{ id = 'fake-gpt'; displayName = 'Fake GPT'; platform = 'openai'; upstreamModel = ''; capabilities = @{ vision = $false; reasoningEfforts = @('low', 'medium', 'high'); contextWindow = 128000; maxOutput = 8192; tools = $true; responses = $true }; pricing = @{ inputPer1M = 1000000; outputPer1M = 2000000; cacheReadPer1M = 100000; cacheWritePer1M = 0 }; enabled = $true; isDefault = $true; sort = 0 } -Token $admin | Out-Null
    $codes = Api POST "$cloudUrl/api/admin/redeem-codes" @{ kind = 'balance'; valueMicros = 5000000; count = 1; maxUses = 1; note = 'e2e' } -Token $admin
    $redeemCode = $codes.items[0].code
    Check ([bool]$redeemCode) '生成余额兑换码'

    Step '启动 agentd（隔离数据目录与 keystore）'
    $env:FORGE_AGENTD_ADDR = '127.0.0.1:8113'
    $env:FORGE_AGENTD_DATA_DIR = Join-Path $tmp 'agentd-data'
    $env:FORGE_GEN_DATA_DIR = Join-Path $tmp 'gen-data'
    $env:FORGE_CLOUD_URL = $cloudUrl
    Remove-Item Env:FORGE_LLM_API_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:FORGE_GEN_API_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:FORGE_AGENT_DEV_MOCK -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $env:FORGE_AGENTD_DATA_DIR, $env:FORGE_GEN_DATA_DIR | Out-Null
    Start-Bg $agentdExe @() 'agentd' | Out-Null
    Wait-Http "$agentdUrl/health" 60

    $status = Api GET "$agentdUrl/api/forge/account/status"
    Check (-not $status.loggedIn -and -not $status.byoConfigured) '初始未登录、无自带密钥（客户端应显示登录门）'

    Step '未登录时本地引擎显式失败'
    $s0 = (Api POST "$agentdUrl/api/forge/sessions" @{ title = 'e2e-nologin' }).session
    $r0 = Api POST "$agentdUrl/api/forge/sessions/$($s0.id)/ask:execute" @{ userInput = 'hello'; mode = 'ask' }
    Check ($r0.run.status -eq 'failed' -and ("$($r0.error)" -match 'CLOUD_LOGIN_REQUIRED')) "未登录 → CLOUD_LOGIN_REQUIRED（实际 status=$($r0.run.status) error=$($r0.error)）"
    Check (-not ("$($r0.message.text)" -match '^mock:')) '不再返回 mock 回声'

    Step 'agentd 注册、兑换'
    $status = Api POST "$agentdUrl/api/forge/account/register" @{ email = $userEmail; password = $userPassword; nickname = 'e2e' }
    Check ($status.loggedIn -and $status.user.email -eq $userEmail) '注册即登录'
    $statusJson = $status | ConvertTo-Json -Depth 8
    Check (-not ($statusJson -match 'rt_|eyJ')) 'status 不含 refresh/access token'
    Check ($status.deviceKeyPrefix -like 'sk-rf-*' -and -not ($statusJson -match 'sk-rf-[A-Za-z0-9]{30,}')) '只暴露设备 Key 前缀'
    $redeem = Api POST "$agentdUrl/api/forge/account/redeem" @{ code = $redeemCode }
    Check ($redeem.balanceMicros -eq 5000000) "兑换后余额 5.000000（实际 $($redeem.balanceMicros)）"
    $models = Api GET "$agentdUrl/api/forge/account/models"
    Check (@($models.models | Where-Object { $_.id -eq 'fake-gpt' }).Count -eq 1) '模型目录含 fake-gpt'

    Step '本地引擎经云端跑两轮'
    $s1 = (Api POST "$agentdUrl/api/forge/sessions" @{ title = 'e2e-cloud'; selectedModelId = 'cloud:fake-gpt' }).session
    $r1 = Api POST "$agentdUrl/api/forge/sessions/$($s1.id)/ask:execute" @{ userInput = 'hello cloud'; mode = 'ask' }
    Check ($r1.run.status -eq 'completed' -and "$($r1.message.text)" -match 'fake:') "第一轮完成（status=$($r1.run.status) text=$($r1.message.text) error=$($r1.error)）"
    $r2 = Api POST "$agentdUrl/api/forge/sessions/$($s1.id)/ask:execute" @{ userInput = 'second turn'; mode = 'ask' }
    Check ($r2.run.status -eq 'completed' -and "$($r2.message.text)" -match 'second turn') '第二轮完成'

    $balance = $null
    for ($i = 0; $i -lt 20; $i++) {
        $balance = Api GET "$agentdUrl/api/forge/account/balance"
        if ($balance.balanceMicros -lt 5000000) { break }
        Start-Sleep -Milliseconds 250
    }
    Check ($balance.balanceMicros -lt 5000000 -and $balance.balanceMicros -gt 0) "按 token 扣费（余额 $($balance.balanceMicros)）"
    $usage = Api GET "$agentdUrl/api/forge/account/usage?limit=10"
    Check ($usage.total -ge 2 -and $usage.items[0].model -eq 'fake-gpt') "用量入账（total=$($usage.total)）"

    Step '用户自建 API Key 直连网关'
    $created = Api POST "$agentdUrl/api/forge/account/api-keys" @{ name = 'e2e-external' }
    $userKey = $created.key
    Check ($userKey -like 'sk-rf-*') '创建 API Key 并一次性返回明文'
    $chat = Api POST "$cloudUrl/v1/chat/completions" @{ model = 'fake-gpt'; messages = @(@{ role = 'user'; content = 'ping gateway' }) } -ApiKey $userKey
    Check ("$($chat.choices[0].message.content)" -match 'fake: ping gateway') 'chat/completions 透传'
    $resp = Api POST "$cloudUrl/v1/responses" @{ model = 'fake-gpt'; input = 'ping responses'; stream = $false } -ApiKey $userKey
    Check (($resp | ConvertTo-Json -Depth 12) -match 'ping responses') 'responses 透传（API Key 账号）'
    try {
        Api POST "$cloudUrl/v1/chat/completions" @{ model = 'not-exist'; messages = @(@{ role = 'user'; content = 'x' }) } -ApiKey $userKey | Out-Null
        Fail '未知模型应 404'
    } catch { Check ("$_" -match 'HTTP 404' -and "$_" -match 'model_not_found') '未知模型 → 404 model_not_found' }

    Step 'OAuth 授权回填建 Codex 订阅账号'
    $start = Api POST "$cloudUrl/api/admin/accounts/oauth/openai/start" @{} -Token $admin
    $state = ([regex]::Match($start.authUrl, '[?&]state=([^&]+)')).Groups[1].Value
    Check ($start.authUrl -like "$fakeUrl/oauth/authorize*" -and $state) '生成 PKCE 授权链接'
    $oauthAccount = Api POST "$cloudUrl/api/admin/accounts/oauth/openai/exchange" @{ sessionId = $start.sessionId; callbackUrl = "http://localhost:1455/auth/callback?code=e2e-code&state=$state"; name = 'fake-codex'; groupIds = @($defaultGroup); priority = 20 } -Token $admin
    Check ($oauthAccount.authType -eq 'oauth' -and $oauthAccount.status -eq 'active') "Codex 订阅账号已建（email=$($oauthAccount.email)）"
    Api PATCH "$cloudUrl/api/admin/accounts/$($apiAccount.id)" @{ status = 'disabled' } -Token $admin | Out-Null
    $codexResp = Api POST "$cloudUrl/v1/responses" @{ model = 'fake-gpt'; input = 'via codex'; stream = $false } -ApiKey $userKey
    Check (($codexResp | ConvertTo-Json -Depth 12) -match 'via codex') 'responses 走 Codex 订阅上游（非流式聚合）'
    $codexChat = Api POST "$cloudUrl/v1/chat/completions" @{ model = 'fake-gpt'; messages = @(@{ role = 'system'; content = 'be brief' }, @{ role = 'user'; content = 'chat over codex' }) } -ApiKey $userKey
    Check ("$($codexChat.choices[0].message.content)" -match 'chat over codex') 'chat→responses 转换走 Codex 订阅上游'
    $accounts = Api GET "$cloudUrl/api/admin/accounts" -Token $admin
    $codexRow = @($accounts.items | Where-Object { $_.id -eq $oauthAccount.id })[0]
    Check ($null -ne $codexRow.quota -and ($codexRow.quota | ConvertTo-Json -Depth 6) -match 'usedPercent|primary') '记录 x-codex 额度快照'

    Step '登出'
    $status = Api POST "$agentdUrl/api/forge/account/logout"
    Check (-not $status.loggedIn) '登出后未登录'
} catch {
    Fail "异常中止：$_"
} finally {
    if ($KeepRunning) {
        Write-Host "进程保留（-KeepRunning）。日志目录：$tmp" -ForegroundColor Yellow
    } else {
        foreach ($p in $procs) { if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue } }
        docker exec forge-cloud-dev-postgres-1 psql -U forge -d postgres -q -c "DROP DATABASE IF EXISTS $e2eDb WITH (FORCE)" 2>$null | Out-Null
    }
}

if ($failures.Count -gt 0) {
    Write-Host "`n$($failures.Count) 项失败，日志目录：$tmp" -ForegroundColor Red
    exit 1
}
if (-not $KeepRunning) { Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue }
Write-Host "`n云模式端到端全部通过" -ForegroundColor Green
