# 15 · 云服务（forge-cloud）与桌面端云模式

> 本章是 forge-cloud 服务端、agentd 云模式（BFF）、客户端账户面三方的**接口契约**。
> 决策背景见 `14_DECISION_LOG.md` D-041。服务端代码在 `cloud/`（Go），agentd 侧在
> `crates/forge-agentd/src/cloud/`，客户端账户面在 `packages/client`。
> 设计参照 sub2api（账号池 + 平台 API Key 分发 + token 级计费），**只参照设计，不拷贝其源码**
> （其 LGPL-3.0 与本仓 Apache-2.0 不宜直接并入）。

## 0. 定位与拓扑

- agent 循环仍在用户本机（引擎、文件、MCP 都在本地）；云端只负责**账号、模型、资料同步、计费**。
- 前端永远拿不到令牌（R-5 延伸）：agentd 是 BFF，持有 refresh token 与「设备 API Key」（keystore，Windows DPAPI）。
  唯一例外：用户在「账户 → API Key」里**自己创建**的 Key 会在创建响应里明文返回一次（给外部工具用，如 Codex CLI）。
- 端口：forge-cloud `:8110`（开发），PostgreSQL `5432`，Redis `6379`；gateway `:8102`/agentd `:8103`/host `:3080` 不变。

```
客户端 ──/api/forge/account/*──▶ host:3080 ──▶ agentd:8103 ──JWT──▶ forge-cloud /api/v1/*
                                                  │ └─设备 Key──▶ forge-cloud /v1/*（本地引擎）
                                                  └─ codex app-server ─设备 Key─▶ forge-cloud /v1/responses
运营者浏览器 ──▶ forge-cloud /admin/（静态后台）──▶ /api/admin/*
```

## 1. 通用约定

- JSON 字段一律 camelCase；时间一律 RFC3339（UTC，带 `Z`）；ID 为整数，登录设备（会话）ID 为 UUID 字符串。
- **金额**：整数 micros（`1 额度单位 = 1_000_000 micros`），字段名以 `Micros` 结尾；显示货币由系统设置 `currency` 决定（默认 `USD`），只影响展示。
- **错误（`/api/v1`、`/api/admin`）**：HTTP 状态码 + `{"error":{"code":"UPPER_SNAKE","message":"中文说明"}}`，个别错误附加字段（如 409 冲突附 `current`）。
- **错误（`/v1` 网关）**：OpenAI 风格 `{"error":{"message":"…","type":"…","code":"lower_snake"}}`；`/v1/messages` 为 Anthropic 风格 `{"type":"error","error":{"type":"…","message":"…"}}`。
- **分页**：列表 `?limit=50&offset=0`（limit 上限 200）→ `{"items":[…],"total":N}`；同步接口用游标（见 §3.4）。
- 每个响应带 `X-Request-Id`；网关响应另带 `X-Forge-Request-Id`（同值）。
- 健康检查 `GET /healthz` → `{"status":"ok","db":"ok","redis":"ok","version":"…"}`（任一依赖异常 → 503，status=`degraded`）。

### 1.1 服务端配置（环境变量）

| 变量 | 默认 | 说明 |
|---|---|---|
| `FORGE_CLOUD_ENV` | `dev` | `prod` 时拒绝使用默认 JWT 密钥/主密钥启动 |
| `FORGE_CLOUD_ADDR` | `:8110` | 监听地址 |
| `FORGE_CLOUD_PUBLIC_URL` | `http://127.0.0.1:8110` | 对外地址（展示用、生成配置片段） |
| `FORGE_CLOUD_DATABASE_URL` | `postgres://forge:forge@127.0.0.1:5432/forge_cloud?sslmode=disable` | |
| `FORGE_CLOUD_REDIS_URL` | `redis://127.0.0.1:6379/0` | |
| `FORGE_CLOUD_JWT_SECRET` | 开发默认值 | HS256 密钥 |
| `FORGE_CLOUD_MASTER_KEY` | 开发默认值 | 32 字节（base64 或 64 位 hex），加密上游凭据 |
| `FORGE_CLOUD_ADMIN_EMAIL` / `FORGE_CLOUD_ADMIN_PASSWORD` | 空 | 首次启动若无管理员则创建 |
| `FORGE_CLOUD_SMTP_HOST/PORT/USER/PASSWORD/FROM` | 空 | 配置后启用邮箱验证码 |
| `FORGE_CLOUD_WEB_DIR` | 空 | 管理后台静态目录（未内嵌时用；缺省尝试 `web/dist`） |
| `FORGE_CLOUD_CODEX_BASE_URL` | `https://chatgpt.com/backend-api/codex` | Codex 订阅上游（测试可指向假上游） |
| `FORGE_CLOUD_CHATGPT_BASE_URL` | `https://chatgpt.com/backend-api` | `wham/usage` 额度查询 |
| `FORGE_CLOUD_OPENAI_AUTH_URL` | `https://auth.openai.com` | OAuth 授权/换 token/刷新 |
| `FORGE_CLOUD_UPSTREAM_PROXY` | 空 | 全局上游 HTTP 代理（账号级 `proxyUrl` 优先） |
| `FORGE_CLOUD_TRUST_PROXY` | `0` | `1` 时取 `X-Forwarded-For` 首跳作客户端 IP |
| `FORGE_CLOUD_LOG_LEVEL` | `info` | |

## 2. 数据模型（DDL 以 `cloud/internal/db/migrations/0001_init.sql` 为准）

- `groups`：分组（倍率 `rate_multiplier`、每用户并发 `concurrency_limit`、`rpm_limit`、`tpm_limit`、可用模型 `allowed_models`（空 = 全部）、`is_default`）。
- `users`：邮箱（小写唯一）、argon2id 密码哈希、昵称、`role`（user/admin）、`status`（active/disabled）、`group_id`、`balance_micros`、`concurrency_override`。
- `user_avatars`：头像二进制（≤512 KiB）。
- `refresh_sessions`：登录设备 = refresh 会话（当前 token 哈希 + 上一个 token 哈希用于复用检测、设备信息、过期/吊销时间）。
- `api_keys`：只存 SHA-256 哈希 + 展示前缀；`kind`=user/device；device Key 绑定 `session_id`，会话吊销时一并吊销。
- `upstream_accounts` + `account_groups`：上游账号（平台 `openai`/`anthropic`，认证 `oauth`/`apikey`，凭据 AES-256-GCM 密文、优先级/权重/并发、冷却、额度快照、代理）。
- `models`：对外模型目录（平台、上游模型名、能力位、四项单价 per 1M tokens）。
- `plans` / `subscriptions`：套餐与用户订阅（周期额度、每日上限、绑定分组）。
- `redeem_codes` / `redeem_records`：兑换码（balance/plan/invite）。
- `balance_ledger`：余额流水（只追加）。
- `usage_logs`：每次网关请求一条。
- `payment_orders`：在线支付占位。
- `user_settings` / `user_memories` / `user_skills`：资料同步（后两者带全局递增 `change_seq` 游标与墓碑）。
- `system_settings`（键值 JSON）、`audit_logs`、`email_codes`。

## 3. 用户 API `/api/v1`

鉴权：`Authorization: Bearer <accessToken>`（JWT HS256，15 分钟；claims：`sub` 用户 ID、`sid` 会话 ID、`role`）。
服务端对每个请求校验用户未禁用、会话未吊销。

### 3.1 认证（公开）

**`GET /api/v1/auth/config`** → `{"registrationMode":"open|invite|closed","requireEmailVerify":false,"smtpEnabled":false,"siteName":"RurixForge Cloud","currency":"USD"}`

**`POST /api/v1/auth/register`**
```json
{"email":"a@b.c","password":"≥8 位","nickname":"可选","inviteCode":"invite 模式必填","emailCode":"requireEmailVerify 时必填",
 "device":{"id":"设备唯一 ID","name":"主机名","platform":"windows","appVersion":"0.1.0"},"issueDeviceKey":true}
```
→ 200 `LoginResponse`（注册即登录；注册赠送额度写流水 `signup_bonus`）。
错误：403 `REGISTRATION_CLOSED`、400 `INVITE_CODE_REQUIRED`/`INVITE_CODE_INVALID`、400 `EMAIL_CODE_REQUIRED`/`EMAIL_CODE_INVALID`、409 `EMAIL_TAKEN`、400 `WEAK_PASSWORD`、400 `INVALID_EMAIL`。

**`POST /api/v1/auth/login`** `{"email","password","device":{…},"issueDeviceKey":true}` → `LoginResponse`。
同一用户同一 `device.id` 再次登录：旧会话与旧设备 Key 吊销。`issueDeviceKey=false`（管理后台用）时不签发设备 Key。
错误：401 `INVALID_CREDENTIALS`、403 `USER_DISABLED`、429 `TOO_MANY_ATTEMPTS`（同 IP+邮箱 15 分钟 10 次）。

```json
LoginResponse = {
  "accessToken":"eyJ…","accessExpiresAt":"…",
  "refreshToken":"rt_…","refreshExpiresAt":"…（30 天）",
  "user": User,
  "deviceKey": {"id":12,"key":"sk-rf-…（明文仅此一次）","prefix":"sk-rf-AbCd12"} | null
}
User = {"id":1,"email":"a@b.c","nickname":"","role":"user|admin","status":"active|disabled",
        "hasAvatar":false,"avatarVersion":0,"groupId":1,"groupName":"default",
        "balanceMicros":0,"createdAt":"…"}
```

**`POST /api/v1/auth/refresh`** `{"refreshToken"}` → `{"accessToken","accessExpiresAt","refreshToken","refreshExpiresAt"}`（每次轮换）。
错误：401 `REFRESH_INVALID`（未知/过期/已吊销）、401 `REFRESH_REUSED`（用了已轮换掉的旧 token → 整个会话及其设备 Key 吊销）。

**`POST /api/v1/auth/logout`**（JWT）→ `{"ok":true}`：吊销当前会话与其设备 Key。

**`POST /api/v1/auth/email-code`** `{"email","purpose":"register|reset"}` → `{"ok":true}`；501 `SMTP_NOT_CONFIGURED`、429 `TOO_MANY_ATTEMPTS`（同邮箱 60 秒 1 次）。

**`POST /api/v1/auth/password/reset`** `{"email","code","newPassword"}` → `{"ok":true}`（吊销该用户全部会话）。

**`GET /api/v1/plans`**（公开）→ `{"items":[{"id","name","description","priceMicros","periodDays","quotaMicros","dailyLimitMicros"}]}`（仅 enabled）。

### 3.2 我的账号（JWT）

- `GET /api/v1/me` → `{"user":User,"subscriptions":[Subscription],"currency":"USD"}`
- `PATCH /api/v1/me/profile` `{"nickname"}` → `User`（昵称 ≤ 32 字）
- `POST /api/v1/me/password` `{"oldPassword","newPassword"}` → `{"ok":true}`（吊销其它会话）
- `GET /api/v1/me/avatar` → 图片二进制（`Content-Type` 原样）；404 `AVATAR_NOT_FOUND`
- `PUT /api/v1/me/avatar` `{"dataUrl":"data:image/png;base64,…"}` → `User`；400 `AVATAR_INVALID`（仅 png/jpeg/webp/gif）、413 `AVATAR_TOO_LARGE`（>512 KiB）
- `DELETE /api/v1/me/avatar` → `User`
- `GET /api/v1/me/devices` → `{"items":[{"id":"uuid","deviceId","deviceName","platform","appVersion","ip","createdAt","lastSeenAt","current":true}]}`
- `DELETE /api/v1/me/devices/{id}` → `{"ok":true}`（吊销会话 + 其设备 Key）
- `GET /api/v1/me/api-keys` → `{"items":[ApiKey]}`
- `POST /api/v1/me/api-keys` `{"name","quotaMicros":0,"expiresAt":null}` → `{"apiKey":ApiKey,"key":"sk-rf-…"}`；400 `API_KEY_LIMIT`（活跃用户 Key 上限 20）
- `DELETE /api/v1/me/api-keys/{id}` → `{"ok":true}`
- `GET /api/v1/me/balance` → `{"balanceMicros","currency","subscriptions":[Subscription]}`
- `GET /api/v1/me/subscription` → `{"items":[Subscription]}`
- `GET /api/v1/me/usage?from=&to=&limit=&offset=` → `{"items":[UsageItem],"total","summary":UsageSummary}`
- `GET /api/v1/me/usage/daily?days=30` → `{"items":[{"date":"2026-09-01","requests","inputTokens","outputTokens","costMicros"}]}`
- `GET /api/v1/me/ledger?limit=&offset=` → `{"items":[{"id","deltaMicros","balanceAfterMicros","kind","note","createdAt"}],"total"}`
- `POST /api/v1/me/redeem` `{"code"}` → `{"kind":"balance|plan","valueMicros","plan":{"id","name"}|null,"subscription":Subscription|null,"balanceMicros"}`；
  404 `REDEEM_CODE_INVALID`、409 `REDEEM_CODE_USED`（已用尽或本人已兑过）、410 `REDEEM_CODE_EXPIRED`、429 `TOO_MANY_ATTEMPTS`
- `POST /api/v1/me/orders` `{"amountMicros","provider"}` → 未注册支付渠道时 501 `PAYMENT_NOT_CONFIGURED`

```json
ApiKey = {"id","name","kind":"user|device","prefix","status":"active|revoked","quotaMicros","usedMicros",
          "expiresAt":null,"lastUsedAt":null,"createdAt","deviceName":"仅 device Key"}
Subscription = {"id","planId","planName","status":"active|expired|cancelled","startsAt","endsAt",
                "quotaMicros","usedMicros","dailyLimitMicros","dailyUsedMicros","groupId":null}
UsageItem = {"id","requestId","model","endpoint":"chat|responses|messages|embeddings","stream",
             "inputTokens","outputTokens","cacheReadTokens","cacheWriteTokens","costMicros",
             "status":"ok|error","errorCode","latencyMs","createdAt","apiKeyName"}
UsageSummary = {"requests","inputTokens","outputTokens","cacheReadTokens","cacheWriteTokens","costMicros"}
```

### 3.3 模型目录（JWT）

`GET /api/v1/models/catalog` → 当前用户**有效分组**可用的模型，价格已乘分组倍率：
```json
{"defaultModel":"gpt-5.5","currency":"USD","rateMultiplier":1.0,
 "models":[{"id":"gpt-5.5","displayName":"GPT-5.5","platform":"openai|anthropic",
   "capabilities":{"vision":true,"reasoningEfforts":["low","medium","high"],"contextWindow":400000,
                   "maxOutput":128000,"tools":true,"responses":true},
   "pricing":{"inputPer1M":1250000,"outputPer1M":10000000,"cacheReadPer1M":125000,"cacheWritePer1M":0},
   "available":true}]}
```
`capabilities.responses=true` 表示该模型可经 `/v1/responses` 调用（Codex 引擎只列这类模型）；`available` = 当前存在可调度的健康账号。
有效分组 = 生效中订阅的套餐分组（多个取最晚到期）> 用户分组 > 默认分组。

### 3.4 资料同步（JWT）

**设置（按命名空间，整块覆盖，版本号乐观锁）**
- `GET /api/v1/me/settings` → `{"items":{"appearance":{"value":{…},"version":3,"updatedAt":"…"}}}`
- `PUT /api/v1/me/settings/{ns}` `{"value":{…},"baseVersion":3,"force":false}` → `{"namespace","value","version","updatedAt"}`
  - `ns` 匹配 `^[a-z][a-z0-9_-]{0,31}$`；value ≤ 64 KiB（413 `SETTINGS_TOO_LARGE`）
  - `baseVersion` 与服务端不一致且 `force=false` → 409 `SETTINGS_VERSION_CONFLICT`，响应附 `"current":{"value","version","updatedAt"}`；`baseVersion=0` 表示「预期不存在」
  - 约定命名空间：`appearance`（主题）、`composer`（发送快捷键等）、`agent`（默认引擎、权限模式）、`models`（默认模型与思考档）

**记忆（增量游标 + 墓碑，后写者胜）**
- `GET /api/v1/me/memories?since=<cursor>&limit=500` → `{"items":[Memory],"cursor":123,"hasMore":false}`（`since` 缺省 0；按 `change_seq` 升序）
- `PUT /api/v1/me/memories` `{"items":[{"id","scope","kind","content","tags":[],"updatedAt","deleted":false}]}`（≤200 条，content ≤ 8 KiB）
  → `{"applied":["id",…],"conflicts":[Memory],"cursor":130}`；
  规则：服务端无此 id 或 `incoming.updatedAt > server.updatedAt` 才写入（`change_seq` 取新值、`version+1`），否则把服务端副本放进 `conflicts`。活跃记忆上限 2000 → 400 `MEMORY_LIMIT`。
- `Memory = {"id":"客户端 UUID","scope":"global|project:<key>","kind":"preference|fact|convention","content","tags":[],"updatedAt","deleted":false,"version":1}`

**个人技能（整包，后写者胜）**
- `GET /api/v1/me/skills?since=<cursor>&limit=50` → `{"items":[Skill],"cursor","hasMore"}`
- `PUT /api/v1/me/skills/{name}` `{"files":{"SKILL.md":"<base64>","scripts/a.py":"<base64>"},"updatedAt"}` → `{"applied":true,"skill":Skill,"cursor"}`；
  服务端副本更新时 `applied:false`，`skill` 为服务端副本（含 files）。
  `name` 匹配 `^[a-z0-9][a-z0-9-]{0,63}$`；必须含 `SKILL.md`（400 `SKILL_INVALID`）；路径为相对正斜杠路径、不含 `..`；解码总量 ≤ 2 MiB（413 `SKILL_TOO_LARGE`）。
- `DELETE /api/v1/me/skills/{name}?updatedAt=<RFC3339>` → `{"applied":true,"cursor"}`（写墓碑）
- `Skill = {"name","files":{…}|null（墓碑为 null）,"sha256","sizeBytes","updatedAt","deleted":false,"version"}`

## 4. 模型网关 `/v1`（API Key）

- 鉴权：`Authorization: Bearer sk-rf-…`，或 `x-api-key: sk-rf-…`（Anthropic 客户端）。
- 端点：`GET /v1/models`（OpenAI 列表格式，只含有效分组可用模型）；`POST /v1/chat/completions`、`/v1/responses`、`/v1/messages`、`/v1/embeddings`。
- 请求里的 `model` 必须是模型目录 `id`；实发上游模型名 = 账号 `modelMapping[id]` > 模型 `upstreamModel` > `id`。
- **粘性键**：`X-Forge-Session` 头 > `session_id` 头 > `conversation_id` 头 > 请求体 `prompt_cache_key`；同一 (用户, 粘性键) 在 TTL（默认 3600 秒）内固定到同一上游账号。
- **管线**：鉴权 → 用户/Key 状态 → 模型存在且在有效分组内 → 计费预检 → Redis 限流（分组 RPM/TPM）→ 并发槽（用户 / Key / 账号三级）→ 选号（粘性 → 优先级数值小者优先 → 负载率低者优先 → 权重随机，跳过冷却与错误账号）→ 协议转换并转发 → 流式回传同时截取 usage → 结算。
- **换号重试**：上游 429 / 5xx / 401（OAuth 先刷新一次）/ 首字节前网络错误 → 冷却该账号（依 `Retry-After`、`x-codex-*-reset-after-seconds` 或 `resets_in_seconds`，缺省 60 秒），换下一个候选重试，最多 `maxFailoverRetries` 次（默认 3）。已向客户端写出字节后不再重试。
- **错误码**（`error.code` → HTTP）：`invalid_api_key` 401、`user_disabled` 403、`insufficient_balance` 402、`key_quota_exceeded` 402、`model_not_found` 404、`model_not_allowed` 403、`endpoint_not_supported` 400、`invalid_request` 400、`rate_limited` 429（带 `Retry-After`）、`concurrency_limited` 429（`Retry-After: 1`）、`no_available_account` 503、`upstream_error` 502。

### 4.1 平台 × 端点矩阵（一期）

| 账号类型 | chat/completions | responses | messages | embeddings |
|---|---|---|---|---|
| openai + oauth（Codex 订阅） | chat→responses 转换 | 透传（整形） | 不支持 | 不支持 |
| openai + apikey（OpenAI 及兼容） | 透传 | 账号 `supportsResponses` 时透传 | 不支持 | 透传 |
| anthropic + apikey | chat→messages 转换 | 不支持 | 透传 | 不支持 |

- apikey 账号的 `baseUrl` **含版本段**（如 `https://api.openai.com/v1`、`https://api.deepseek.com/v1`、`https://api.anthropic.com/v1`），网关只拼 `/chat/completions`、`/responses`、`/messages`、`/embeddings`、`/models`。
- 流式 chat 透传时网关强制 `stream_options.include_usage=true` 以截取用量。
- Codex 订阅上游：`{FORGE_CLOUD_CODEX_BASE_URL}/responses`，头 `Authorization: Bearer <access_token>`、`chatgpt-account-id`、`OpenAI-Beta: responses=experimental`、`originator: codex_cli_rs`、`session_id`（粘性键或随机 UUID）、`Accept: text/event-stream`；请求体强制 `store:false`、`stream:true`（客户端要非流式时由网关聚合 `response.completed`）。chat→responses 转换时：系统设置 `codexInstructions` 非空则作 `instructions`、客户端 system 消息转成 `developer` 输入；为空则 system 消息拼接作 `instructions`；丢弃 `temperature`/`top_p`/`max_tokens`。
- 用量截取：chat 取末块 `usage`；responses 取 `response.completed.response.usage`（`input_tokens_details.cached_tokens` 计缓存读）；messages 取 `message_start.message.usage` + `message_delta.usage`（`cache_read_input_tokens`/`cache_creation_input_tokens`）。
- 转换器（`cloud/internal/gateway/convert`）为纯函数包，chat⇄responses、chat⇄messages 各含请求转换、流式事件转换、非流式聚合，golden 夹具测试。

## 5. 计费规则

- 单次费用 = ⌈(输入×输入价 + 输出×输出价 + 缓存读×缓存读价 + 缓存写×缓存写价) / 1e6 × 分组倍率⌉ micros。
  OpenAI 口径的 `prompt_tokens` 已含缓存命中部分：计费输入 = `prompt_tokens − cached_tokens`，缓存读 = `cached_tokens`。
- **预检**：用户 active；Key 有额度上限时 `usedMicros < quotaMicros`；存在「有剩余额度且未超每日上限」的生效订阅，或余额 > 0 → 放行，否则 402。
  **（D-042 起由 §11.1 取代：额度按模型所在用量池判定，额度用尽后看按量付费开关与每周期上限。）**
- **结算**（单事务）：按到期时间先后扣订阅周期额度（受每日上限约束），不足部分扣余额（允许单次扣成负数，下次请求预检拦截）；写 `usage_logs`，余额部分写 `balance_ledger(kind=usage)`；累加 Key `used_micros`。失败请求写 `usage_logs(status=error)`、费用 0。
  **（D-042 起只扣模型所在池的额度，含 Hobby 免费额度；档位订阅额度按月重置，见 §11.1。）**
- 兑换码：`balance`（加余额，写流水 `redeem`）、`plan`（新建订阅：从现在起 `periodDays` 天）、`invite`（仅注册用）；`maxUses` 次数、每人每码一次、可过期、可作废。
- 注册赠送 `signupBonusMicros`（系统设置）；注册模式 `open|invite|closed`。
- 在线支付：`billing.PaymentProvider` 接口（`Name()`、`CreateOrder`、`HandleNotify`）+ `payment_orders` 表；一期无实现，下单 501 `PAYMENT_NOT_CONFIGURED`，回调 `POST /api/v1/payments/{provider}/notify` 同样 501。

## 6. 上游账号与 Codex 订阅导入

- 凭据 JSON 用 `FORGE_CLOUD_MASTER_KEY`（AES-256-GCM，随机 nonce）加密落 `upstream_accounts.credentials`，任何接口不回显（apikey 账号只回 `keyHint` 末 4 位）。
- **OAuth 凭据**：`{"accessToken","refreshToken","idToken","accountId","expiresAt"}`；`accountId`/邮箱/套餐取自 id_token 的 `https://api.openai.com/auth` 声明（`chatgpt_account_id`、`chatgpt_plan_type`）与 `email`。
- **导入方式**：①粘贴/上传一个或多个 `~/.codex/auth.json`（`tokens.{id_token,access_token,refresh_token,account_id}`；只有 `OPENAI_API_KEY` 时建 openai apikey 账号）；②OAuth PKCE：后台生成授权链接（client_id `app_EMoamEEZ73f0CkXaXp7hrann`、redirect `http://localhost:1455/auth/callback`、scope `openid profile email offline_access`、`code_challenge_method=S256`、`id_token_add_organizations=true`、`codex_cli_simplified_flow=true`），运营者登录后把浏览器地址栏里的回调 URL 粘回，服务端用 code + verifier 换 token；③CLI `forge-cloud accounts import-codex [--group <id>] <auth.json>...`。同一 `chatgpt_account_id` 重复导入 = 更新凭据。
- **刷新**：后台 worker 每 5 分钟扫描 `token_expires_at < now+24h` 或 `last_refresh_at < now-7d` 的 OAuth 账号，`POST {auth}/oauth/token`（JSON：`client_id`、`grant_type=refresh_token`、`refresh_token`、`scope=openid profile email`）；连续失败 3 次置 `status=error`。上游 401 时即时刷新一次。
- **额度快照**：每次响应头 `x-codex-primary-used-percent` / `x-codex-primary-window-minutes` / `x-codex-primary-reset-after-seconds`（及 secondary 同组）写入 `quota`；后台「刷新额度」调 `GET {chatgpt}/wham/usage` 原样存 `quota.usage`。上游 429 且 `error.type=usage_limit_reached` → 冷却到 `resets_in_seconds` 之后。

## 7. 管理 API `/api/admin`（JWT，role=admin）

列表统一 `?q=&limit=&offset=` → `{"items","total"}`（下列只写条目形状）。所有写操作记审计日志。

- `GET /dashboard` → `{"users":{"total","active7d"},"accounts":{"total","active","coolingDown","error"},"today":{"requests","inputTokens","outputTokens","costMicros","errors"},"daily":[{"date","requests","costMicros","inputTokens","outputTokens"}]（近 14 天）,"topModels":[{"model","requests","costMicros"}]（近 24 小时）,"currency"}`
- **用户**：`GET /users?q=&status=` → `AdminUser = User + {"lastLoginAt","concurrencyOverride"}`；`POST /users` `{"email","password","nickname","role","groupId","balanceMicros"}`；
  `GET /users/{id}` → `{"user","subscriptions","ledger"（近 20 条）,"devices","apiKeys"}`；`PATCH /users/{id}` `{"nickname","role","status","groupId","concurrencyOverride"}`；
  `POST /users/{id}/balance` `{"deltaMicros","note"}` → `{"balanceMicros"}`；`POST /users/{id}/password` `{"password"}`；
  `POST /users/{id}/subscriptions` `{"planId","days"}` → `Subscription`；`DELETE /users/{id}/subscriptions/{subId}`
- **分组**：`GET /groups` → `{"items":[Group]}`（不分页）；`POST /groups`；`PATCH /groups/{id}`；`DELETE /groups/{id}`（被用户/套餐引用 → 409 `GROUP_IN_USE`）。
  `Group = {"id","name","description","rateMultiplier","concurrencyLimit","rpmLimit","tpmLimit","allowedModels":[],"isDefault","userCount","accountCount"}`
- **套餐**：`GET /plans`（不分页）、`POST /plans`、`PATCH /plans/{id}`、`DELETE /plans/{id}`；`Plan = {"id","name","description","priceMicros","periodDays","quotaMicros","dailyLimitMicros","groupId","enabled"}`
- **上游账号**：
  - `GET /accounts?platform=&status=&groupId=&q=` → `Account = {"id","name","platform","authType":"oauth|apikey","baseUrl","email","planType","status":"active|disabled|error","priority","weight","concurrencyLimit","currentConcurrency","groupIds","modelMapping":{},"supportsResponses","proxyUrl","cooldownUntil","lastError","lastErrorAt","tokenExpiresAt","lastRefreshAt","lastUsedAt","quota":{},"keyHint","createdAt","updatedAt"}`
  - `POST /accounts`（apikey 账号）`{"name","platform","baseUrl","apiKey","supportsResponses","priority","weight","concurrencyLimit","groupIds","modelMapping","proxyUrl"}`
  - `PATCH /accounts/{id}`（同上字段 + `status`，`apiKey` 非空才替换）；`DELETE /accounts/{id}`
  - `POST /accounts/import-codex` `{"items":[{"name":"可选","authJson":"<auth.json 原文>"}],"groupIds","priority","concurrencyLimit","proxyUrl"}` → `{"created":[Account],"updated":[Account],"errors":[{"index","message"}]}`
  - `POST /accounts/oauth/openai/start` `{}` → `{"sessionId","authUrl","redirectUri","expiresAt"}`（PKCE 会话存 Redis 30 分钟）
  - `POST /accounts/oauth/openai/exchange` `{"sessionId","callbackUrl"（或 "code"）,"name","groupIds","priority","concurrencyLimit","proxyUrl","accountId"（可选：重新授权已有账号）}` → `Account`
  - `POST /accounts/{id}/refresh`（立即刷新 token）、`POST /accounts/{id}/quota`（刷新额度）、`POST /accounts/{id}/clear-cooldown` → `Account`
  - `POST /accounts/{id}/test` `{"model"}` → `{"ok","latencyMs","httpStatus","message"}`
  - `GET /accounts/{id}/models` → `{"items":["model-id"]}`（apikey 账号拉 `{baseUrl}/models`；oauth → 400 `NOT_SUPPORTED`）
- **模型与定价**：`GET /models` → `{"items":[AdminModel]}`；`POST /models`（409 `MODEL_EXISTS`）；`PATCH /models/{id}`、`DELETE /models/{id}`（`id` 需 URL 编码，如 `anthropic%2Fclaude`）。
  `AdminModel = {"id","displayName","platform","upstreamModel","capabilities":{…同 §3.3},"pricing":{…},"enabled","isDefault","sort","availableAccounts"}`
- **兑换码**：`GET /redeem-codes?batch=&status=&kind=&q=` → `RedeemCode = {"id","code","kind":"balance|plan|invite","valueMicros","planId","planName","batch","maxUses","usedCount","status":"active|revoked|exhausted|expired","expiresAt","note","createdAt"}`；
  `POST /redeem-codes` `{"kind","valueMicros","planId","count"（1–1000）,"maxUses","expiresAt","note","batch","prefix"}` → `{"items":[RedeemCode],"batch"}`；`POST /redeem-codes/{id}/revoke`；`GET /redeem-codes/export?batch=` → CSV
- **用量**：`GET /usage?userId=&accountId=&model=&status=&from=&to=` → `{"items":[UsageItem + {"userId","userEmail","accountId","accountName","upstreamModel","httpStatus","firstTokenMs","ip"}],"total","summary"}`
- **系统设置**：`GET /settings`、`PUT /settings`（部分更新）→ `{"siteName","currency","registrationMode","requireEmailVerify","signupBonusMicros","defaultGroupId","defaultModel","maxFailoverRetries","stickyTtlSeconds","codexInstructions","smtpEnabled"（只读）}`
- **审计**：`GET /audit-logs?actorId=&action=` → `{"id","actorId","actorEmail","action","target","detail":{},"ip","createdAt"}`

管理后台（`cloud/web`，React + Vite + Tailwind + Radix）以 `/admin/` 为 base 构建，发布构建时 `go:embed` 进二进制（`-tags embedui`）。管理员用 `/api/v1/auth/login`（`issueDeviceKey:false`）登录。

## 8. agentd 云模式（BFF）

### 8.1 本地状态

- `data/cloud-config.json`：`{"serverUrl","deviceId","deviceName","sync":{"settings":true,"memory":true,"skills":true}}`；
  `serverUrl` 缺省：环境变量 `FORGE_CLOUD_URL` > 构建期 `option_env!("FORGE_CLOUD_URL")` > `http://127.0.0.1:8110`。
- keystore：`cloud:refresh`（refresh token）、`cloud:device-key`（设备 Key），读取走不受 `FORGE_GEN_API_KEY` 覆盖的 `secret_for`；access token 只在内存，过期前或遇 401 自动用 refresh 续期（串行化，避免触发复用检测）。
- `data/cloud-state.json`（非密）：登录用户摘要、设备 Key 前缀、同步游标、设置缓存。
- `FORGE_ALLOW_BYO=0`：禁用自带密钥（DeepSeek/openai-compat/Codex ChatGPT 登录）；`FORGE_AGENT_DEV_MOCK=1`：允许 mock 模型（仅开发/测试）。

### 8.2 `/api/forge/account/*`（经 host 代理，本机无鉴权，永不回显令牌）

- `GET /status` →
```json
{"serverUrl":"http://127.0.0.1:8110","loggedIn":true,"reachable":true,"user":User|null,
 "balanceMicros":0,"currency":"USD","subscriptions":[Subscription],"deviceKeyPrefix":"sk-rf-AbCd12",
 "byoAllowed":true,"byoConfigured":false,"devMock":false,
 "sync":{"settings":true,"memory":true,"skills":true,"lastSyncAt":null,"lastError":null},"lastError":null}
```
  `byoConfigured` = 允许自带密钥且 DeepSeek 或 openai-compat 已配齐。客户端**登录门**条件：`!loggedIn && !byoConfigured && !devMock`。
- `GET /config`、`POST /config` `{"serverUrl","deviceName","sync":{…}}` → 同形状（已登录时改 `serverUrl` → 409 `LOGGED_IN`）
- `GET /auth-config` → 透传云端 `/api/v1/auth/config`
- `POST /register` `{"email","password","nickname","inviteCode","emailCode"}` → `status`；`POST /login` `{"email","password"}` → `status`
- `POST /email-code` `{"email","purpose"}`；`POST /logout` → `status`（云端不可达也清本地令牌）
- 透传（JWT 由 agentd 注入）：`GET /me`、`PATCH /profile`、`POST /password`、`GET|PUT|DELETE /avatar`（GET 返回图片二进制）、`GET /devices`、`DELETE /devices/{id}`（删的是本机会话 → 本地同时登出）、`GET|POST /api-keys`、`DELETE /api-keys/{id}`、`GET /balance`、`GET /subscription`、`GET /usage`、`GET /usage/daily`、`GET /ledger`、`POST /redeem`、`GET /plans`
- `GET /models` → 云端模型目录（agentd 缓存 5 分钟）
- `GET /settings` → `{"items":{ns:{value,version,updatedAt}},"source":"cloud|cache"}`；`PUT /settings/{ns}` `{"value"}` → `{"namespace","value","version","updatedAt","pending":false}`（用户主动改动以 `force:true` 推送；离线时写本地缓存并 `pending:true`，恢复后补推）
- `POST /sync` → 立即全量同步 → `{"ok":true,"sync":{…}}`
- 错误：401 `CLOUD_LOGIN_REQUIRED`（未登录）、401 `CLOUD_UNAUTHORIZED`（续期失败，已本地登出）、502 `CLOUD_UNREACHABLE`；云端业务错误原样透传 `{error:{code,message}}` 与状态码。

### 8.3 `/api/forge/memory`

- `GET /api/forge/memory?q=&scope=global|project|all` → `{"items":[LocalMemory],"projectKey":"…","sync":{"enabled","lastSyncAt"}}`
- `POST /api/forge/memory` `{"content","kind","scope":"global|project","tags"}` → `LocalMemory`
- `PATCH /api/forge/memory/{id}` `{"content","kind","tags"}` → `LocalMemory`；`DELETE /api/forge/memory/{id}` → `{"ok":true}`
- `LocalMemory = {"id","scope":"global|project:<key>","kind","content","tags","createdAt","updatedAt","source":"agent|user"}`
- agent 工具：`memory_write {content, kind, scope}`、`memory_search {query}`、`memory_delete {id}`；每轮取 Top-N（默认 8）注入本地引擎系统提示与 Codex `developerInstructions`。

### 8.4 模型与引擎

- 本地引擎：会话 `selectedModelId = "cloud:<id>"` → `Provider::Cloud`，请求 `{serverUrl}/v1/chat/completions`，头 `Authorization: Bearer <设备 Key>`、`X-Forge-Session: <会话 ID>`、`X-Forge-Client: forge-agentd/<版本>`；未选模型且已登录 → 云端 `defaultModel`。
- 未登录且无自带密钥：显式失败 `CLOUD_LOGIN_REQUIRED`（不再静默 mock）；显式选了不可用模型 → `MODEL_NOT_CONFIGURED`。
- design-snapshot：`models.models[]` 追加云端条目
  `{"id":"cloud:<id>","label":displayName,"provider":"cloud","availability":"available|unavailable|needs-login","group":"RurixForge 云","supportsThinking","effortOptions","defaultEffort","contextOptions":[{"id":"native","label":"400K","tokens":400000}],"defaultContext":"native","vision","pricing":{…},"currency"}`；
  已登录时 `defaultModelId = "cloud:<defaultModel>"`；`mock` 条目仅 `FORGE_AGENT_DEV_MOCK=1` 时出现；顶层新增 `account: {"loggedIn","nickname","email","balanceMicros","currency","hasAvatar"}`。
- Codex 引擎：`codex-config.json` 新增 `authSource: "auto|cloud|chatgpt"`（auto = 已登录用 cloud）。cloud 模式使用托管 `data/codex-cloud-home/config.toml`（`model_provider = "forge-cloud"`，`base_url = "<serverUrl>/v1"`，`env_key = "FORGE_CLOUD_API_KEY"`，`wire_api = "responses"`），设备 Key 经进程环境注入；
  `GET /api/forge/codex/status` 增 `authSource`（生效值）与 `cloud:{loggedIn,serverUrl}`；cloud 模式下 `GET /api/forge/codex/account` → `{"authMode":"cloud","planType":<套餐名或 "balance">,"email":<用户邮箱>,"rateLimits":null}`，`GET /api/forge/codex/models` 只列 `capabilities.responses=true` 的云端模型（id 仍为 `codex:<id>`）。
- 技能：`POST /api/forge/skills` 新增 `scope: "personal|workspace"`（缺省 personal → `data/user-skills/<name>/`），`GET /api/forge/skills/list` 条目新增 `personal: bool`；个人技能随账号同步。

### 8.5 失败码（`agent.failed` 事件 payload 新增 `code`）

| code | 含义 | 客户端引导 |
|---|---|---|
| `CLOUD_LOGIN_REQUIRED` | 未登录且无可用自带密钥 | 打开登录门 |
| `CLOUD_UNAUTHORIZED` | 设备 Key/会话失效 | 打开登录门 |
| `INSUFFICIENT_BALANCE` | 余额与套餐额度用尽（含 `key_quota_exceeded`） | 跳账户页兑换 |
| `MODEL_NOT_ALLOWED` / `MODEL_NOT_CONFIGURED` | 模型不在分组内 / 选了不可用模型 | 打开模型选择 |
| `RATE_LIMITED` | 限流或并发超限（已按 Retry-After 重试仍失败） | 提示稍后再试 |
| `NO_AVAILABLE_ACCOUNT` / `UPSTREAM_ERROR` / `CLOUD_UNREACHABLE` | 云端无可用账号 / 上游错误 / 云端不可达 | 提示重试 |

重试分类：`CLOUD_UNAUTHORIZED`、`INSUFFICIENT_BALANCE`、`MODEL_NOT_ALLOWED`、`MODEL_NOT_CONFIGURED`、`CLOUD_LOGIN_REQUIRED` 不重试；`RATE_LIMITED` 按 `Retry-After` 重试；其余沿用瞬时错误重试。

## 9. 安全要点

- 密码 argon2id；API Key 与 refresh token 只存 SHA-256；上游凭据 AES-256-GCM；JWT 15 分钟 + refresh 轮换与复用检测。
- 登录/兑换/验证码限速；管理写操作审计；默认不落请求与响应正文。
- `prod` 环境拒绝默认密钥启动；管理后台同源部署。
- **合规提示**：用 ChatGPT/Claude 订阅账号对外售卖额度可能违反上游服务条款，有封号风险；对外商用流量建议以 API Key 账号为主。

## 10. 部署

- 发布镜像：`cloud/deploy/Dockerfile`（node 构建后台 → `go build -tags embedui`）；`cloud/deploy/docker-compose.yml`（forge-cloud + postgres:16 + redis:7，可选 Caddy 自动 HTTPS）；`cloud/deploy/.env.example`。
- 开发：`pnpm dev:cloud`（起 PG/Redis）+ `go -C cloud run ./cmd/forge-cloud serve`；测试 `pnpm test:cloud`。
- nginx 反代需 `underscores_in_headers on;`（否则 Codex 的 `session_id` 头被丢弃，粘性失效）。

## 11. 会员梯度与额度计费（D-042）

参照 Cursor 个人方案（Hobby / Pro / Pro+ / Ultra）。DDL 见 `cloud/internal/db/migrations/0002_membership.sql`；本节覆盖 §5 的预检与结算规则。

### 11.1 规则

- **档位**：`plans.tier` 非空的套餐（内置 `hobby|pro|pro_plus|ultra`，可自定义 `^[a-z][a-z0-9_]{0,31}$`，唯一），`tierRank` 越大越高；`tier` 为空的套餐是「额度包」。内置默认值（单位：额度单位，展示货币见系统设置）：

| 档位 | 月付 | 年付（8 折） | 第三方模型额度/月（api 池） | 平台模型额度/月（forge 池） |
|---|---|---|---|---|
| Hobby | 免费 | — | 0 | 1 |
| Pro | 20 | 192 | 20 | 60 |
| Pro+ | 60 | 576 | 70 | 180 |
| Ultra | 200 | 1920 | 400 | 1200 |

  价格与 api 池额度同 Cursor；forge 池 Cursor 未公布，默认值上线前由运营者在后台调整。
- **用量池**：模型 `pool`：`api`（第三方前沿模型，按 API 价计费）/ `forge`（平台模型）。套餐与订阅各有两池额度：`quotaMicros`（api）、`forgeQuotaMicros`（forge）。
- **用量周期**：档位订阅 `usageCycle=month`，额度按月重置，锚定订阅开始时刻（1/31 开始 → 2/28(29) → 3/31…，不超过订阅结束）；额度包 `usageCycle=period`，整段有效期一个周期；Hobby 免费额度与无档位订阅的用户按 UTC 自然月。
- **有效档位**：生效中的档位订阅里 `tierRank` 最高者（同级取晚到期）；没有则为 Hobby。其余生效订阅（额度包、低档赠送）照常作为额度来源（`Membership.packs`）。
- **结算**：按到期先后扣「模型所在池」的套餐内额度（含 Hobby 免费额度，受每日上限约束），不足部分按「按量付费」扣余额（允许单次扣成负数）。
- **按量付费（on-demand）**：用户级开关（默认开）与每周期上限 `limitMicros`（0 = 不设上限）；统计周期 = 有效档位订阅的当前用量周期，Hobby 为 UTC 自然月；已用 = 该周期 `usage_logs.charged_balance_micros` 之和。关闭时额度用尽即拦截（跨过额度的那一次请求，超出部分记入最后一个有该池额度的桶）。
- **预检**：用户 active；Key 额度；模型所在池还有套餐内额度 → 放行；否则按量付费关闭 → 402 `INCLUDED_USAGE_EXHAUSTED`；余额 ≤ 0 → 402 `INSUFFICIENT_BALANCE`；设了上限且本周期已达上限 → 402 `SPEND_LIMIT_REACHED`。网关 `error.code` 为小写 `included_usage_exhausted` / `spend_limit_reached`。
- **购买（报价规则）**：
  - `new`：没有生效中的档位订阅，立即开始。
  - `upgrade`：目标档位更高，立即开始；作废全部**已购**（`source=purchase`）档位订阅，生效中的按剩余秒数折算 `valueMicros`、已预约的全额，合计为抵扣 `creditMicros`；应付 = max(标价 − 抵扣, 0)，抵扣超出标价的部分生效时退回余额（`refundMicros`）。赠送/兑换来的档位订阅不作废、不折算，照常用到期。
  - `renew`（同档，含改付费周期）/ `downgrade`（更低档）：接在当前档位订阅链末尾（未结束档位订阅的最晚 `endsAt`）开始，即下个周期生效。
  - 期限：按月 1 个自然月，按年 12 个；新订阅 `valueMicros` = 标价、`source=purchase`、`billingInterval` = `month|year`。
  - 支付：`provider=balance` 用余额支付（余额须 ≥ 应付，不扣成负数）；应付为 0 直接生效；在线渠道下单为 `pending`，渠道回调或管理员「标记已支付」后生效；同一用户只保留一张待支付会员单（新下单取消旧单）；已取消的订单事后到账 → 款项转入余额。
  - 取消预约：只能取消排在最后、尚未开始的档位订阅；已购价值全额退回余额。
- 余额流水 `kind` 新增 `subscription`（余额购买档位）与 `refund`（升级抵扣溢出、取消预约）。


### 11.2 用户 API（JWT）

- `GET /api/v1/tiers`（公开）→ `{"currency","payment":{"enabled","providers":[]},"items":[Tier]}`（仅启用的档位，按 `rank`）
- `GET /api/v1/me/membership` → `Membership`
- `PATCH /api/v1/me/membership/on-demand` `{"enabled"?,"limitMicros"?}` → `{"enabled","limitMicros","usedMicros"}`（`limitMicros` 0–1e12）
- `GET /api/v1/me/membership/usage?from=&to=` → `{"from","to","currency","items":[ModelUsage],"totals":ModelUsage}`（缺省当前用量周期，跨度 ≤ 400 天，按费用倒序）
- `POST /api/v1/me/membership/quote` `{"planId","interval":"month|year"}` → `Quote`（只读）
- `POST /api/v1/me/membership/checkout` `{"planId","interval","provider":"balance|<在线渠道>"}` → `{"order":Order,"membership":Membership}`
- `DELETE /api/v1/me/membership/scheduled/{id}` → `Membership`（取消预约并退款）
- `GET /api/v1/me/orders?limit=&offset=` → `{"items":[Order],"total"}`；`POST /api/v1/me/orders/{id}/cancel` → `Order`
- 错误：402 `INSUFFICIENT_BALANCE`（顶层附 `balanceMicros`、`amountMicros`）、501 `PAYMENT_NOT_CONFIGURED`、502 `PAYMENT_FAILED`、400 `PLAN_NOT_PURCHASABLE` / `INVALID_INTERVAL`、404 `PLAN_NOT_FOUND` / `ORDER_NOT_FOUND` / `SUBSCRIPTION_NOT_FOUND`、409 `ORDER_NOT_PENDING` / `NOT_SCHEDULED` / `SCHEDULE_NOT_LAST`。
- 既有形状新增字段：`Subscription`（见下）；`UsageItem.pool`；模型目录 `models[].pool`；`GET /api/v1/plans` 条目加 `tier`、`tierRank`、`priceYearlyMicros`、`forgeQuotaMicros`、`tagline`、`features`、`highlight`。

```json
Tier = {"planId","tier":"hobby|pro|pro_plus|ultra","name","tagline","description","features":[],
        "priceMonthlyMicros","priceYearlyMicros","includedApiMicros","includedForgeMicros","dailyLimitMicros",
        "highlight":false,"rank"}
Membership = {"currency","tier":{"planId","tier","name","rank"},"subscription":Subscription|null,
              "scheduled":[Subscription],"packs":[Subscription],"cycle":{"start","end"},
              "pools":{"api":Pool,"forge":Pool},"onDemand":{"enabled","limitMicros","usedMicros"},
              "balanceMicros","payment":{"enabled","providers":[]},"pendingOrder":Order|null}
Pool = {"includedMicros","usedMicros","remainingMicros"}
Subscription = §3.2 字段 + {"tier","forgeQuotaMicros","forgeUsedMicros","usageCycle":"month|period",
               "cycleStart","cycleEnd","billingInterval":"|month|year","valueMicros","source":"grant|redeem|purchase"}
               （usedMicros / forgeUsedMicros 为当前用量周期的已用量）
ModelUsage = {"model","pool":"api|forge","requests","errors","inputTokens","outputTokens","cacheReadTokens",
              "cacheWriteTokens","costMicros","includedMicros","onDemandMicros"}
Quote = {"planId","tier","planName","interval","mode":"new|renew|upgrade|downgrade","listPriceMicros",
         "creditMicros","amountMicros","refundMicros","startsAt","endsAt","currentSubscriptionId",
         "replacesSubscriptionIds":[],"balanceMicros","currency","payment"}
Order = {"id","kind":"topup|subscription","provider","status":"pending|paid|cancelled|failed","amountMicros",
         "listPriceMicros","creditMicros","planId","planName","tier","interval","mode","replacesSubscriptionId",
         "subscriptionId","payUrl","note","createdAt","paidAt"}
```

### 11.3 管理 API

- `Plan` 新增 `tier`、`tierRank`（0–1000）、`tagline`（≤ 64 字）、`features`（≤ 12 条，每条 ≤ 80 字）、`priceYearlyMicros`、`forgeQuotaMicros`、`highlight` 与只读 `subscriberCount`（生效订阅数）；`tier` 重复 → 409 `TIER_TAKEN`。
- `AdminModel` 新增 `pool`（`api|forge`），`POST/PATCH /models` 可写。
- `GET /api/admin/orders?userId=&status=&kind=&q=`（`q` = 邮箱包含）→ `{"items":[AdminOrder],"total"}`，`AdminOrder = Order + {"userId","userEmail"}`。
- `POST /api/admin/orders/{id}/mark-paid` `{"note"?}`（≤ 200 字）→ `AdminOrder`：线下确认收款，订单按 §11.1 生效；`POST /api/admin/orders/{id}/cancel` → `AdminOrder`；非 pending → 409 `ORDER_NOT_PENDING`。审计 `order.mark_paid` / `order.cancel`。

### 11.4 agentd 与客户端

- `/api/forge/account/*` 透传新增：`GET /tiers`、`GET /membership`、`PATCH /membership/on-demand`、`GET /membership/usage`、`POST /membership/quote`、`POST /membership/checkout`、`DELETE /membership/scheduled/{id}`、`GET /orders`、`POST /orders/{id}/cancel`。
- `agent.failed` 新增 code `INCLUDED_USAGE_EXHAUSTED`、`SPEND_LIMIT_REACHED`（不重试；客户端引导到「设置 → 套餐与用量」）。
- 客户端设置新增「套餐与用量」页：当前档位与两池用量、按量付费开关与上限、档位对比（按月/按年）与升降级、本周期按模型用量、订单记录。管理后台新增「订单」页，「套餐」页分「会员档位 / 额度包」，模型编辑加用量池。

## 12. 渠道模型范围（2026-10-07，追加）

- 管理账号视图和 `POST/PATCH /api/admin/accounts` 新增 `allowedModels: string[]`。省略时保持默认或现有配置；空数组表示不限制，以兼容已有 API Key 和 OAuth 账号。
- 非空时最多 500 项；模型 ID 去除首尾空白、去重，不得为空、超过 128 字节或包含空白/控制字符。错误为 400 `INVALID_REQUEST`。
- 调度候选须同时满足分组、平台、健康状态、模型范围和端点能力。模型范围匹配目录 ID 或按既有 `modelMapping > upstreamModel > id` 解析后的上游模型名。模型映射本身继续只负责名称转换。
- 无支持该模型的健康账号返回 503 `NO_AVAILABLE_ACCOUNT`；存在支持模型的账号但都不支持端点时返回 400 `ENDPOINT_NOT_SUPPORTED`。
- 管理模型的 `availableAccounts` 与用户目录的 `available` 按实际支持该模型的健康账号计算。数据库仅追加 `upstream_accounts.allowed_models`，不修改已有凭据、分组或用量。

## 13. 零定价模型预检（2026-10-07，追加）

- 模型的输入、输出、缓存读和缓存写单价全部为 0 时，调用不要求余额或套餐剩余额度，也不受按量付费开关与金额上限拦截。任一单价非零或模型缺失时，继续执行既有收费预检。
- 用户状态与 API Key 额度检查仍先执行；网关鉴权、模型权限、限流与并发限制继续适用。
- 零费用调用仍记录真实 token 用量；不扣余额、套餐额度或 Key 的已用金额，不新增扣费流水。
- 本机测试部署的 GPT-6.1 Sol 曾因零余额返回 402 `INSUFFICIENT_BALANCE`，IDE 已记录 `agent.failed`。此修复使服务器预检与零费用结算一致，不需要修改测试账号余额。

## 14. 云模型思考档位传递（2026-10-07，追加）

- 本地引擎通过动态云模型目录解析 `cloud:<id>` 的思考档位，聊天和手动上下文压缩共用同一解析逻辑。显式选择的有效档位进入上游 `reasoning_effort`；缺失或已不支持的选择回落目录首项，与模型菜单默认档一致。
- 云模型关闭思考时显式传目录支持的最低强度（优先 none / minimal / low），避免省略字段后继承上游默认思考。没有可识别档位的云模型继续省略该参数；已有静态渠道的规格行为保持原契约。
- 只有目录中列出且可识别的档位可传递，模型 ID 继续由实际渠道决定。深度规划仍按 §8.4 的既有最强档位策略执行。

## 15. Claude Messages 与 Thinking（2026-10-07，追加）

- Claude API Key 渠道与模型平台应为 `anthropic`。本机 KCNE Claude 原生渠道为账号 6；账号 1 的旧 OpenAI 协议配置保留为停用状态，凭据与历史记录保留。四个模型 ID 和既有价格、分组范围不变。
- 同一平台用户 API Key 可调用 `POST http://127.0.0.1:8110/v1/messages` 或 `/v1/chat/completions`。原生 Messages 接口接受 `x-api-key`、`anthropic-version`、`anthropic-beta`；请求内容块、`thinking`、`output_config` 和流式事件保留，仅按渠道映射模型名。鉴权使用平台 Key，不向客户端提供上游 Key。
- 模型能力增加可选 `thinkingMode: manual|adaptive` 和 `thinkingAlwaysOn: bool`。Opus 5.5 / Fable 5.1 为始终启用自适应思考；IDE 显示“始终开启”，强度仍可选择。Sonnet 5.5 开启时使用 adaptive，关闭入口对应 `between_tools` 与 low effort。Haiku 4.5 使用手动预算模式，本平台 low / medium / high 对应 2048 / 8192 / 16384 个思考 token。
- OpenAI 兼容请求的 `reasoning_effort` 在自适应模型上转换成 `output_config.effort`，思考摘要通过 `thinking.display=summarized` 返回。自适应模式不发送 `budget_tokens`，不增大调用方给定的 `max_tokens`。当前三种新模型不接受强制工具选择，转换器返回明确的 400 错误并建议 `tool_choice=auto`。
- 本地代理仅向 Anthropic 云模型附带 `thinking_enabled`，区分开启与关闭时相同的 low effort；始终思考的模型按目录强制开启。聊天和手动压缩共用同一规格解析。
- 兼容聊天响应的 `reasoning_content` 用于显示摘要，`anthropic_content` 保存完整 assistant 内容块及顺序，包括原始 thinking/signature、redacted_thinking 与工具参数。流式响应在完成时附带该数组，本地代理在工具续接时原样回注，避免签名丢失。外部兼容客户端若使用工具续接，也须保留此扩展；使用原生 Messages 客户端时直接回注原始 content 数组。
- 原生请求示例：`{"model":"claude-sonnet-5-5","max_tokens":8192,"thinking":{"type":"adaptive","display":"summarized"},"output_config":{"effort":"medium"},"messages":[{"role":"user","content":"你好"}]}`。Haiku 4.5 改用 `thinking:{"type":"enabled","budget_tokens":2048,"display":"summarized"}`，不发送 `output_config.effort`，且 max_tokens 大于预算。
- Anthropic 官方说明：[Thinking](https://platform.claude.com/docs/en/build-with-claude/thinking)、[Effort](https://platform.claude.com/docs/en/build-with-claude/effort)。本机实测通过四模型的两种非流式接口、两种流式接口及两种工具续接，共 12 项；验证记录位于 `evidence/cloud-server-20261007/claude-messages-verified-*.json`。
- 本地代理的已验证二进制为 `target/debug/forge-agentd-claude-20261007.exe`；隔离实例验证了 Sonnet 开关、Opus 始终开启与 Haiku 手动思考。当前 8103 代理仍为旧进程，自动审批拒绝了重启（仅返回 `blocked by policy`）。待 IDE 任务结束后由用户执行 `powershell -File D:\RurixForge\scripts\restart-agentd.ps1`，脚本检查进程身份、保留账号数据并在启动失败时回滚。本机当前登录已于 2026-10-07 11:16:26 UTC 因 `REFRESH_REUSED` 失效，加载新版后还需重新登录。
