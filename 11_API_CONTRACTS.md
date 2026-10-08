# 11 · API 与数据契约

> 路由形态照搬 agent-cowork agentd(`/api/agent-debug/*`),统一更名 `/api/forge/*`(D-002)。
> 全部响应 JSON;错误 = `{ "error": { "code", "message", "details?" } }` + 语义化 HTTP 状态。
> 事件 = SSE,信封 §3。MCP 工具与同名 HTTP API 一一对应(`05 §1.4`)。

## 1. 约定

- Base:gateway `:8102`(公开)→ agentd `:8103`;鉴权 JWT(本地开发可 `FORGE_SKIP_LOGIN=1`)。
- 命名:资源复数;动作用 `:verb` 后缀(如 `plan:generate`);路径参数 `{id}`。
- 幂等:GET/PUT 幂等;POST 动作除注明外不幂等。
- 大载荷(截图/日志/视频)一律文件引用 `{ fileRef }`(落盘路径句柄),不内联字节。

## 2. 路由总表(按域)

### 2.1 会话与执行(照搬 agentd 语义)

| 路由 | 方法 | 语义 |
|---|---|---|
| `/api/forge/sessions` | GET/POST | 会话列表 / 创建(`agentKind`、`mode`、`projectRoot`) |
| `/api/forge/sessions/{id}` | GET/DELETE | 详情 / 删除 |
| `/api/forge/sessions/{id}/ask:execute` | POST | 发送消息并执行(流式经 SSE) |
| `/api/forge/sessions/{id}/plan:generate` | POST | 生成 Plan DAG |
| `/api/forge/sessions/{id}/todos` | GET | 会话 todo |
| `/api/forge/sessions/{id}/events/stream` | GET(SSE) | 事件流 |
| `/api/forge/sessions/{id}/model` | GET/PUT | 会话模型 |
| `/api/forge/sessions/{id}/permission-mode` | GET/PUT | 权限模式(`12 §2`) |
| `/api/forge/plans/{id}` | GET | Plan 详情 |
| `/api/forge/todos`、`/api/forge/todos/{id}`、`/api/forge/todos:batch-rerun` | GET/PATCH/POST | todo 管理 |
| `/api/forge/runs/{id}`、`/runs/{id}/logs`、`/runs/{id}/metrics`、`/runs/{id}/nodes/{seg}`、`/runs/{id}/todos/{seg}` | GET | 运行观测 |
| `/api/forge/replay/{id}`、`/replay/{id}/since` | GET | 会话回放 |

### 2.2 swarm / subagent / 工具

| 路由 | 方法 | 语义 |
|---|---|---|
| `/api/forge/swarm/state` | GET | 节点 + 分片状态 |
| `/api/forge/swarm/seed-demo` | POST | 演示播种(开发) |
| `/api/forge/subagents` | GET | subagent profile 清单 |
| `/api/forge/tools` | GET | 工具注册表(内建 + MCP 聚合) |
| `/api/forge/mcp/servers`、`/mcp/servers/{seg}` | GET/PUT/DELETE | MCP server 配置与状态 |

### 2.3 记忆 / 技能 / 渠道

| 路由 | 方法 | 语义 |
|---|---|---|
| `/api/forge/memories`、`/memories/{id}` | GET/POST/PATCH/DELETE | 记忆管理 |
| `/api/forge/skills/list`、`/skills/{name}`、`/skills/config/write` | GET/POST | skill 管理 |
| `/api/forge/channels`、`/channels/{id}`、`/channels:fetch-models` | GET/POST/PUT/DELETE | LLM 渠道 |
| `/api/forge/models`、`/model-preferences`、`/provider-status`、`/provider-types` | GET/PUT | 模型与渠道状态 |
| `/api/forge/search-config` | GET/PUT | web 搜索配置 |

### 2.4 工作区 / checkpoint / proposal / 权限

| 路由 | 方法 | 语义 |
|---|---|---|
| `/api/forge/workspace/root`、`/workspace/tree`、`/workspace/file`、`/workspace/browse`、`/workspace/info` | GET | 工作区 |
| `/api/forge/workspace/revert` | POST | 回滚到 checkpoint |
| `/api/forge/sessions/{id}/checkpoints`、`/checkpoints/{id}` | GET/POST/DELETE | checkpoint 管理(含场景快照 `12 §4`) |
| `/api/forge/proposals`、`/proposals/{id}` | GET/POST/PATCH | Proposal 列表/创建/批准/拒绝 |
| `/api/forge/permissions/rules`、`/permissions/{seg}` | GET/PUT/DELETE | 权限规则 |

### 2.5 引擎域(与 MCP 工具同面,`05` 各表)

| 路由前缀 | 对应 server | 例 |
|---|---|---|
| `/api/forge/scene/*` | engine-scene | `POST /api/forge/scene/entity_batch_apply` |
| `/api/forge/assets/*` | asset-pipeline | `POST /api/forge/assets/asset_import` |
| `/api/forge/code/*` | code-forge | `POST /api/forge/code/rx_check` |
| `/api/forge/project/*` | project | `GET /api/forge/project/git_status` |
| `/api/forge/playtest/*` | playtest | `POST /api/forge/playtest/test_run` |
| `/api/forge/gen/*` | gen-image / gen-model | `POST /api/forge/gen/gen_image` |

实现:agentd 侧薄 handler → 转发对应 MCP server 调用;UI 与 agent 因而共享同一行为(I-2 落地机制)。

### 2.6 其他照搬

`auth/login` `auth/logout` `auth/me` `auth/register` `auth/profile`、
`terminal/sessions*`、`shells*`、`hooks*`、`openapi.json`、`design-snapshot`。

### 2.7 资产商店 / 技能管理(F11,D-025)

| 路由 | 方法 | 语义 |
|---|---|---|
| `/api/forge/store/sources` | GET/POST | 源清单 / 添加源(`{id,name,baseUrl,enabled,tokenRef?}`;`file://` 与 `https://` 双驱动) |
| `/api/forge/store/sources/{id}` | PATCH/DELETE | 改源(启停/改名/换 token) / 删源 |
| `/api/forge/store/search` | GET | 多源聚合搜索(`q` / `kind=asset-pack\|skill` / `sourceId?` / `page` / `pageSize`) |
| `/api/forge/store/packages/{sourceId}/{pkgId}` | GET | 包详情 + 版本列表 |
| `/api/forge/store/packages/{sourceId}/{pkgId}/{version}` | GET | 版本清单(manifest + `files[]`) |
| `/api/forge/store/install` | POST | 安装(长任务,返回 `{taskId}`;`{sourceId,pkgId,version,destFolder?}`) |
| `/api/forge/store/uninstall` | POST | 卸载(destructive,须 approved Proposal `kind=store.uninstall`) |
| `/api/forge/store/tasks/{taskId}` | GET | 长任务进度(`{status,phase,done,total,error?}`) |
| `/api/forge/store/installed` | GET | 已安装清单(项目级 `.forge/store/installed.json` + skills 侧) |
| `/api/forge/store/updates` | GET | 更新检查(已安装 × 各源最新版本比对) |
| `/api/forge/store/library` | GET/POST | 个人资产库列表 / 收藏进库(`{assetPath}` 或 `{fileRef}`) |
| `/api/forge/store/library/{id}` | DELETE | 移出个人库 |
| `/api/forge/store/library/{id}:install` | POST | 从个人库装进当前项目 |
| `/api/forge/store/publish` | POST | 打包发布到指定源(`{sourceId,manifest,files[]}`) |
| `/api/forge/skills` | POST | 新建 skill(`{name,content}`;frontmatter 须过 `06 §1` 校验) |
| `/api/forge/skills/{name}` | GET/PUT/DELETE | 读全文 / 覆写全文 / 删除(destructive,须 approved Proposal `kind=skill.delete`) |
| `/api/forge/skills/{name}:validate` | POST | 校验 SKILL.md 格式契约,返回 `{valid,errors[],warnings[]}` |

`store_*` MCP 工具与上表同名同参一一对应(`05 §1.4`);长耗时的 `store_install` / `store_uninstall` 在 MCP 侧为异步提交(返 `taskId`)+ `store_task_status` 轮询,原因见 D-F11-C(MCP 子进程 10s 上限)。

事件扩展(§3.2 补):`store.install.progress` `store.installed` `store.uninstalled` `agent.skills.injected`。

## 3. 事件契约(SSE)

### 3.1 信封(照搬 agentd)

```json
{ "id": "ev_…", "ts": 1780000000, "sessionId": "s_…", "seq": 1234,
  "type": "chat.delta", "payload": { … } }
```

`seq` 单调;断连经 `replay/{id}/since?seq=` 补齐。

### 3.2 事件类型(沿用 + 游戏域扩展)

| 族 | 类型 |
|---|---|
| 会话(照搬) | `chat.delta` `chat.done` `tool.call` `tool.result` `plan.updated` `todo.updated` `subagent.spawned` `subagent.done` `swarm.shard.assigned` `swarm.shard.done` |
| 引擎(扩展) | `scene.changed`(实体/组件增量摘要) `scene.saved` `scene.checkpoint.created` `play.state.changed` `host.crashed` `host.restarted` |
| 资产(扩展) | `asset.built` `asset.failed` `asset.moved` `asset.deleted` |
| 生成(扩展) | `gen.candidates.ready` `gen.accepted` |
| 测试(扩展) | `playtest.result` |
| 治理(扩展) | `proposal.created` `proposal.resolved` `permission.denied` |
| 商店(F11) | `store.install.progress` `store.installed` `store.uninstalled` |
| 技能(F11) | `agent.skills.injected` |

## 4. 核心 DTO(摘要)

```jsonc
// Session
{ "id", "agentKind": "coding", "mode": "build", "projectRoot", "model", "createdAt", "status" }
// Plan / PlanTask
{ "id", "sessionId", "stages": [ { "id", "title", "tasks": [ { "id", "title", "status", "verifyRef?" } ] } ] }
// TodoItem
{ "id", "sessionId", "title", "status": "pending|in_progress|done|failed", "shardId?" }
// Proposal(12 §3)
{ "id", "kind": "asset.delete|scene.bulk|git.write|external.send", "summary",
  "impact": { "entities?": 0, "assets?": [], "files?": [] }, "status": "pending|approved|rejected",
  "createdBy": { "sessionId", "tool" } }
// SceneSummary
{ "path", "entityCount", "componentCounts": { "MeshRenderer": 12 }, "dirty": false, "playState": "edit" }
```

完整 schema 由 `forge-protocol` crate 以 Rust 类型单源生成 OpenAPI(`/api/forge/openapi.json`,照搬 agentd openapi 机制)。

## 5. 错误码表(冻结前缀)

| 段 | 范围 | 例 |
|---|---|---|
| 通用 | `FORGE_*` | `FORGE_NOT_FOUND` `FORGE_INVALID_ARGS` `FORGE_UNAUTHORIZED` |
| 会话/agent | `AGENT_*` | `AGENT_SESSION_NOT_FOUND` `AGENT_TOOL_LOOP_EXCEEDED` |
| 场景 | `SCENE_*` | `SCENE_NOT_LOADED` `SCENE_HIERARCHY_CYCLE` `SCENE_VALIDATION_FAILED` `SCENE_ENTITY_NOT_FOUND` |
| 组件 | `COMPONENT_*` | `COMPONENT_UNKNOWN_TYPE` `COMPONENT_SINGLETON_VIOLATION` `COMPONENT_FIELD_INVALID` |
| 物理 | `PHYSICS_*` | `PHYSICS_BACKEND_NOT_COMPILED` `PHYSICS_FIXED_STEP_MISMATCH` `PHYSICS_POOL_EXHAUSTED` |
| 资产 | `ASSET_*` | `ASSET_NOT_FOUND` `ASSET_BUILD_FAILED` `ASSET_REFERENCED` `ASSET_IMPORT_UNSUPPORTED` |
| 代码 | `CODE_*` | `CODE_COMPILE_FAILED` `CODE_LSP_UNAVAILABLE` |
| 项目 | `PROJECT_*` | `PROJECT_OUT_OF_ROOT` `PROJECT_GIT_CONFLICT` |
| 工作区 git(D-040) | `GIT_*` | `GIT_NOT_FOUND` `GIT_TIMEOUT` `GIT_FAILED` |
| 生成 | `GEN_*` | `GEN_BACKEND_NOT_CONFIGURED` `GEN_RATE_LIMITED` |
| 测试 | `TEST_*` | `TEST_TIMEOUT` `TEST_ASSERT_FAILED` |
| 治理 | `GOV_*` | `GOV_PROPOSAL_REQUIRED` `GOV_PERMISSION_DENIED` `GOV_SWARM_SHARD_OVERLAP` |
| 宿主 | `HOST_*` | `HOST_UNREACHABLE` `HOST_CRASHED` `HOST_GPU_LOST` |
| 商店(F11) | `STORE_*` | `STORE_SOURCE_UNREACHABLE` `STORE_SOURCE_NOT_FOUND` `STORE_PACKAGE_NOT_FOUND` `STORE_VERSION_NOT_FOUND` `STORE_MANIFEST_INVALID` `STORE_CHECKSUM_MISMATCH` `STORE_ALREADY_INSTALLED` `STORE_NOT_INSTALLED` `STORE_DEPENDENCY_UNRESOLVED` `STORE_PAYMENT_REQUIRED` `STORE_TASK_NOT_FOUND` `STORE_PUBLISH_REJECTED` |
| 技能(F11) | `SKILL_*` | `SKILL_NOT_FOUND` `SKILL_NAME_INVALID` `SKILL_ALREADY_EXISTS` `SKILL_FRONTMATTER_INVALID` `SKILL_BODY_INCOMPLETE` `SKILL_READONLY_DIR` |

规则:新错误码进本表 + `forge-protocol` 枚举,同 PR;不允许字符串裸抛。

## Errata(只追加区)

- **E-11-009(2026-10-07,用户指令)——手动压缩上下文**:①新路由 `POST /api/forge/sessions/{id}/compact`(无请求体):本地引擎用会话当前模型把摘要锚点之后的全部已结束轮次连同旧摘要合并成新摘要(与自动压缩共用 `summaries.json`,此后各轮从摘要续);Codex 引擎对已绑定线程发原生 `thread/compact/start` 并等压缩轮结束。压缩期间占会话运行锁,与起轮互斥。成功 200 `{engine, manual:true, turns?, tokensBefore?, tokensAfter?}`(后三项仅本地引擎回报,为估算值);错误 404 `SESSION_NOT_FOUND`、409 `SESSION_BUSY` / `AGENT_COMPACT_NOTHING`(无新对话可压缩,或 Codex 线程不存在)/ `AGENT_COMPACT_CANCELLED`、502 `AGENT_COMPACT_FAILED`(摘要调用或 Codex 压缩轮失败,`message` 带原因;mock 渠道与活跃 Goal 也走此码)。②新事件 `context.compacted`(domain `agent`,落盘):载荷同成功响应;前端据此在时间线画分隔线,上下文计量从分隔线之后重新估算。③`agent.usage` 的前端消费口径修订:Codex 载荷的 `promptTokens` 是线程累计,上下文计量改取 `last.promptTokens`(最近一次请求)与 `modelContextWindow`;本地引擎载荷无 `last`,口径不变。

- **E-11-008(2026-10-06,D-045)——Design 模式契约(as-built)**:①`ask:execute` 的 `mode` 增 `design`;请求体增可选 `design: {id, rev, action, candidate?}`(在场 = 流程动作,`userInput` 可空);动作表、自由文本路由与错误码(409 `DESIGN_STAGE_MISMATCH`、400 `DESIGN_VISION_REQUIRED` / `DESIGN_NEEDS_WRITE`)见文末「Design 模式流程契约」。②新路由:`GET /api/forge/sessions/{id}/design`、`GET …/design/file?path=`、`POST …/design/{select|restart}`。③会话 DTO 增可选 `design`(DesignState,无流程时不出现)。④SSE 新频道 `design`,事件见下文。⑤`viewport.frame` 增 `camera` / `exact` 参数(05 E-05-004)。

- **E-11-006(2026-09-26,D-040)——工作区搜索 / git 状态 / 轻量快照**(E-11-005 已被 D-041 占用,本条顺延):
  - `GET /api/forge/workspace/search?q=&workspaceId=&limit=` → `{query, results[{path,name,dir}], total, truncated, source:"git"|"walk", scanned, scanTruncated}`。git 仓库用 `git ls-files`(遵守 .gitignore),否则有界遍历并跳过 `.git`/`node_modules`/`target`/`dist`;候选缓存 30s,空查询只预热。未知 `workspaceId` → 404 `WORKSPACE_NOT_FOUND`(与 tree/file 同口径)。
  - `GET /api/forge/workspace/git?workspaceId=` → 非仓库 `{isRepo:false, reason}`;仓库 `{isRepo, branch, upstream, ahead, behind, detached, unborn, rootUntracked, files[{path,status,staged,dir,insertions,deletions,origPath}], counts, insertions, deletions, total, truncated}`。`status` ∈ `M|A|D|R|U|C`。工作区目录整体未跟踪时 `rootUntracked:true` 且 `files` 为空。无 git → 501 `GIT_NOT_FOUND`;超时 → 504 `GIT_TIMEOUT`;其余 → 500 `GIT_FAILED`。
  - `GET /api/forge/design-snapshot` 增 `events=0`(或 `false`):跳过事件回放,`latestSeq` 与其余字段照常。`GET /api/forge/health`(host 自有)增 `user.name`、`platform`、`node`、`agentd{ok,version?,uptimeSec?}`(上游 `/health` 探测,800ms 超时,不可达 `ok:false`)。
  - 两条 workspace 路由落在 host 既有 `/api/forge/workspace` 代理前缀内,代理表不改。
- **E-11-005(2026-09-26,D-041)——账户 / 记忆 / 云模式契约面**:
  - 新增 `/api/forge/account/*`(agentd BFF,经 host 代理;永不回显 refresh/access token 与设备 Key)与 `/api/forge/memory`,形状见 `15_CLOUD_SERVICE.md` §8.2 / §8.3。
  - **删除** `GET /api/forge/llm/complete`(恒 mock 桩)。
  - `agent.failed` payload 增 `code`(`CLOUD_LOGIN_REQUIRED` / `CLOUD_UNAUTHORIZED` / `INSUFFICIENT_BALANCE` / `MODEL_NOT_ALLOWED` / `MODEL_NOT_CONFIGURED` / `RATE_LIMITED` / `NO_AVAILABLE_ACCOUNT` / `UPSTREAM_ERROR` / `CLOUD_UNREACHABLE`,见 15 §8.5)。
  - design-snapshot:`models.models[]` 追加云端条目(`id:"cloud:<id>"`、`provider:"cloud"`、`pricing`),已登录时 `defaultModelId` 为云端默认模型;顶层新增 `account` 摘要;`mock` 条目仅开发模式出现。
  - `POST /api/forge/skills` 增 `scope: personal|workspace`(缺省 personal),列表条目增 `personal`;`/api/forge/codex/{config,status}` 增 `authSource`。
  - 第 9 行「gateway JWT 鉴权」与第 78–79 行 `auth/*` 照搬口径由 forge-cloud `/api/v1/auth/*` 兑现(用户在云端鉴权;本机 agentd/host 仍是无鉴权的 localhost 服务);`/api/forge/channels` 维持不建。
- **E-11-003(2026-09-03,PvZ 可玩化波 / D-037)——`POST /api/forge/mcp/call` 请求体增可选 `workspaceId`**:
  - 此前 REST 透传面恒锚 `projects/demo`(mcp.rs `default_project_root`),IDE 视口 / 层级 / 资产面板与会话 turn 面各看各的项目(双真相源):在 pvz 工作区里打开关卡场景后 `play_enter` 会到 demo 根下找图而失败。现同请求体带 `workspaceId` 时,项目根经 `scope::project_of` 解析(与 turn 面同一事实源、同一 engine-host 连接池槽);缺省 / 未注册 id → 默认工作区 → `projects/demo` 兜底,旧客户端零改动。
  - 客户端 `callTool*` 全部自动附带当前工作区 id(`lib/activeWorkspace.ts` 读 localStorage 镜像键,免 forgeApi ↔ workspaceStore 环依赖);切工作区时视口流通道按新工作区的 `viewport_stream_info` 重连,编辑器场景 / 实体 / 相机 / 资产面重拉。
  - 未知工具仍先 404 `TOOL_NOT_FOUND`(作用域解析在其后);destructive 强制门(asset_delete force)语义不变。
- **E-11-004(2026-09-03,D-038)——回执唤醒契约面 as-built(修订 E-11-002)**:
  - **`ask:execute` 新增 409 `SESSION_BUSY`**:会话已有运行中的 run(典型 = 服务端自起的回执唤醒轮刚起、`agent.started` 尚未推到前端的那几毫秒内用户点了发送)→ `{error:{code:"SESSION_BUSY",message}}`,**不发任何事件、不建用户卡**;前端撤乐观回显并提示。其余状态码不变。
  - **`composer.user.message` 增可选字段**:`source:"receipt"`(该 turn 是系统唤醒,正文由服务端生成、以「【系统唤醒】」开头)、`receiptIds:[]`(本轮送达的回执 id)。无 `source` = 用户发的(原语义)。对应 run 的 `trigger` = `receipt_wake`(此前只有 `composer_chat` / `multitask_dispatch`)。
  - **`agent.receipts.injected` 增字段 `midTurn:bool`**:true = 主 agent 正在跑时中途插入(runId 为正在跑的那条 turn);false = 开轮取件(用户轮或唤醒轮)。E-11-002 所述「回执只在下一轮用户发言时注入」口径作废。
- **E-11-002(2026-09-03,D-036)——multitask 异步委派契约面 as-built**:
  - **`ask:execute` 请求体不变**(`multitask` 仍是既有 `mode` 取值之一);**响应体语义变**:multitask 轮的 `message.text` 是**派单说明**而非执行结果,`run.status=completed` 只代表「派发完成」,后台子代理此时通常仍在跑。原「模板未命中 → `run.status=failed` + `error` 含『模板未命中』」的口径**作废**(模板已退役,见 04 E-04-002)。
  - **`subagent.*` 事件三件套增字段**:`detached: bool`(true = multitask 后台腿)、`dispatchedBy: string|null`(派它的父 runId)。后台腿的 `parentRunId`/`subRunId`/`parentToolCallId` **三者同值 = 该后台 run 自己的 id**——前端据此单开卡片、归组子事件、并把它当作取消用的 runId(`POST /api/forge/runs/{id}/cancel` 原样适用,该 run 的 `trigger` = `multitask_dispatch`)。后台腿终态另发 `agent.message` / `agent.completed|failed`(同 runId,payload 带 `detached:true`)落回执正文;**刻意不发 `agent.started`**——它会把前端 `activeRunId` 顶上、锁死输入框。
  - **§3 事件新增一类**:`agent.receipts.injected {runId, receiptIds[], injected, pending, deferred, chars}`(本轮把哪几条后台回执喂进了主 agent 上下文;`deferred>0` = 有条目因预算留到下一次取件)。与既有 `agent.skills.injected` / `agent.context.injected` 同体例。
  - **无新增 REST 前缀**:回执是服务端内部收件箱(`data/agent-sessions/receipts.json`),对外可见面 = 上述事件流;`/api/forge/swarm/*` 三端点保留且语义不变(不再被 multitask 使用)。
  - 送达时机以 E-11-004(D-038)为准:本条写就时为「下一轮用户发言时注入」,已被即时送达 + 唤醒取代。
- **E-11-001(2026-09-03,D-035)——plan 契约面 as-built**:
  - **§2.1 `/api/forge/sessions/{id}/plan:generate` 与 `/api/forge/plans/{id}` 正式作废,不实现**。计划是工作区文件(`.forge/plans/<slug>.plan.md`),读写复用既有 `GET|PUT /api/forge/workspace/file`;有意不开 `/api/forge/plans` 顶级前缀(会同时要动 host `PROXY_PREFIXES`,且与「文件即事实源」重复)。生成入口 = `ask:execute` 的 `plan` 模式 + 原生工具 `create_plan`。
  - **`ask:execute` 请求体增可选 `planPath`**(工作区相对路径):mode=build 且带此字段 = 按该计划实施(读计划 → 幂等物化 front matter 待办 → 计划全文注入本轮上下文)。校验:须为 `.forge/plans/` 下单层 `.plan.md`、无 `..` 逃逸,且文件可读可解析,否则 **400 `PLAN_NOT_READABLE`**(不静默降级成一次没有计划的普通 build,I-5)。同请求体既有字段(`userInput`/`mode`/`skills`/`readonlyWorkspaceIds`/`includeLibrary`)不变。
  - **§3 事件新增三类**(前缀 `plan` → `plan` 频道,自 F7 预留本波首发):`plan.created {runId,path,name,overview,todoCount}`(计划文件首次落盘)、`plan.updated {同上}`(同路径覆盖迭代)、`plan.build.started {runId,path,name,todos:[{id,planTodoId,title}]}`(Build 轮起步,待办已物化)。§3.2 表中规划过的 `plan.updated` 至此兑现,命名保持。
  - **§4 DTO 增补**:`TodoItem` 加可选 `planTodoId`(来源计划文件的待办 id;Build 按 `(sessionId, planTodoId)` 去重,重复 Build 不重建)与既有 `source`(计划物化时为 `"plan"`);会话 DTO 加可选 `activePlanPath`(当前计划文件路径,随 `design-snapshot.activeSession` 下发,fork 时继承)。两者均 serde default + 未设置不序列化,旧 `todos.json`/`sessions.json` 兼容。
  - **计划文件格式**(前后端共同契约,写方 agentd `plan_doc.rs`,读方另有前端 `lib/planFile.ts`):YAML front matter `name`(必填)/`overview`/`todos[{id,content,status}]` + `---` 后的 Markdown 正文;front matter 内的值一律压成单行标量(不使用块标量/锚点等高级语法),两侧解析器据此保持一致。
  - **§2.1 `/api/forge/sessions/{id}/model`** 维持不实现(as-built:模型经 `PATCH /api/forge/sessions/{id}` 的 `selectedModelId` 切换);Plan 页签的模型切换器复用该路径,故「换模型后再 Build」即以新模型实施,无需 per-message 覆盖字段。


## Agent 消息与 Team 协作契约（2026-10-03）

持久化 `agentId` 标识参与者，`activeRunId` 标识其当前可取消执行；切换轮次不更换参与者身份。前端 DTO 位于 `packages/protocol/src/collaboration.ts`，实现位于 `crates/forge-agentd/src/collaboration.rs`。以下路径均由现有 Host `/api/forge/sessions` 代理透传，路径参数须编码。

- `GET /api/forge/sessions/{sid}/agents` → `{agents: AgentParticipant[]}`；条目含 `id/sessionId/name/role/engine/status/parentAgentId/teamId/activeRunId`，角色为 `root/member/subagent`，状态为 `idle/running/stopped/recoveryRequired`。
- `GET /api/forge/sessions/{sid}/agents/{agentId}/messages` → `{messages: AgentMessage[]}`，返回该参与者相关的发送及接收历史。
- `POST /api/forge/sessions/{sid}/agents/{agentId}/messages` 正文 `{text, clientMessageId?, expectedRunId?}` → `{message}`。`text` 为 1–16000 字符；客户端重试使用同一 `clientMessageId`，同一键对应不同目标或正文返回 `MESSAGE_ID_CONFLICT`。提供 `expectedRunId` 时目标轮次变化返回 `AGENT_RUN_CONFLICT`。消息来源和发送者由宿主确定，请求不接受 `fromAgentId/source/kind/wake` 等身份字段。
- `GET /api/forge/sessions/{sid}/team` → `{team: TeamState | null}`；`GET /api/forge/sessions/{sid}/teams/{teamId}` → `{team}`；`PATCH` 后一路径，正文 `{action: "pause" | "resume" | "stop" | "complete"}` → `{team}`。恢复和完成仍受服务端状态与任务约束校验。

`AgentMessage` 包含 `id/sessionId/fromAgentId/toAgentId/source/text/clientMessageId/status/createdAt/injectedAt/runId/error`；`source` 为 `user/agent`，状态为 `queued/leased/injected/recoveryRequired/failed`。`injected` 表示目标已接收上下文，不表示任务完成。旧记录缺省 `kind: message, wake: true`；宿主自动终态回执使用 `kind: receipt, wake: false`。不能确认是否已注入的恢复记录保留原状，明确重发创建新的消息和幂等键，不自动重复注入。

`TeamState` 包含 `id/sessionId/name/leaderAgentId/memberAgentIds/revision/maxParallel/maxFixRounds/fixRounds/tasks/createdAt/updatedAt`。状态为 `active/paused/blocked/stopped/completed/recoveryRequired`；任务含 `id/title/prompt/role/stage/deps/ownerAgentId/status/result/attempts`，任务状态为 `queued/running/completed/failed/blocked`。暂停不启动新的任务或消息唤醒，当前轮次可完成；停止取消现有执行。自由 Team 按显式依赖、阶段、并发与修复边界执行，不增加必需人工关卡；UltraPlan 的原有阶段约束保持有效。

会话 SSE 新增 `agent.participant.updated`（payload 为完整参与者）、`agent.message.queued` / `agent.message.injected` / `agent.message.failed`（payload 为完整消息，恢复状态也经失败事件表达）、`team.updated`（payload 为 `{team}`）。客户端按持久消息 id 去重，已确认注入状态不因迟到 HTTP 响应回退；按同一 Team 的 `revision` 合并，切换 Team 后不沿用旧版本号。事件必须与当前会话及 payload 的 `sessionId` 相符；fork 复制的历史事件不恢复原会话的可操作参与者或团队。

成员的工具、审批、文本和用量事件携带 `agentId/agentRunId/parentAgentId/teamId/taskId` 等归属字段；`agentRunId` 是取消和审批清理所用真实运行 id，旧 `runId/parentToolCallId` 可继续用于显示归组。主运行及成员运行的终态、用量和审批队列分别处理；同一主运行内多条用户引导按 `messageId/clientMessageId` 保存，不能再仅按 `runId` 合并为一条用户消息。

## UltraPlan 完整流程契约（2026-10-03）

实现及请求样例见 [UltraPlan 工作流](docs/ultraplan.md)。`ask:execute` 的 `mode: ultraplan` 处理探索、问答、Demo 和计划；制作动作必须使用 `mode: team` 并携带当前 `ultraplan.id/action/rev`。Forge 与 Codex 复用同一阶段机。父流程持有会话运行锁，过期版本或重复制作请求不会产生第二组任务。

`GET /api/forge/sessions/{id}/ultraplan` 返回 `ultraplan/questionnaire/answers/demo/checks/production/acceptance/target/delivery`。`POST .../ultraplan/acceptance` 正文为 `{id, rev: planRev, round, results}`；`rollback_demo` 为 `{id, rev: demoIteration}`；`restart` 允许空正文并保留已生成文件。人工必需项必须通过，可选跳过和失败项需注明原因；有失败项保持验收阶段，由 `fix_production` 创建定向修复任务。客户端提交失败验收后自动发起该制作动作。

计划正文、任务图、检查清单、目标后端和交付说明共同参与哈希；流程外改动后必须修订并重新确认。自动检查必须来自真实工具执行，并绑定报告、截图及正式项目文件指纹；最终完成再次核验。缺证据、未验证、终审拒绝或必需人工检查未过，均不能进入 `done`。新增事件使用既有 `ultraplan.*` 命名空间，详见工作流文档。

## Design 模式流程契约（2026-10-06）

实现及工序见 [Design 模式](docs/design-mode.md)。阶段 `concept → design_review → replication → done`,相位 `waiting / running / failed`;状态挂会话 `design` 字段(`id, slug, dir, title, workspaceId, stage, phase, running, lastError, designRev, candidates, selected, designType, aspect, approved, replicationRound, layoutReady, assetsReady, scenePath, verifyCount, lastVerify, passed`)。

| action | 可用阶段 | rev 须等于 | 正文 |
|---|---|---|---|
| `approve_design` | design_review | designRev | 可空;`candidate` 缺省为 `selected` |
| `revise_design` | design_review | designRev | 必填 |
| `regenerate_design` | design_review | designRev | 可空 |
| `resume_replication` | replication | replicationRound | 可空 |
| `fix_replication` | replication / done | replicationRound | 必填 |

`mode: design` 的自由文本:无流程或 done → 新流程;concept → 补充说明重试;design_review → 对 `selected` 候选修改;replication → 409。带动作的请求永不开新流程;id / rev / 阶段 / 候选不对、流程正在运行或工作区已换 → 409 `DESIGN_STAGE_MISMATCH`(`details: {stage, allowed}`)。

`GET …/design` 返回 `{design, review, layout, assets, verify, result}`:`review` = 当前批次 `submission.json`,`verify` = 最近一次 `report.json`。`GET …/design/file?path=<流程目录相对或带 .forge/design/<slug>/ 前缀>` 只出 `.png` / `.json`。`POST …/design/select {id, rev, candidate}` 只在 design_review 生效;`POST …/design/restart` 要求会话空闲。

事件(频道 `design`):`design.started{id,title,slug,dir}`、`design.stage{id,stage,phase,running,designRev,replicationRound,lastError?}`、`design.candidates.generated{id,rev,candidates}`、`design.review.ready{id,rev,candidates[{index,path,prompt,op}],summary,designType,aspect,width,height,base?}`、`design.decision{id,action,rev,candidate?,feedback?}`、`design.layout.ready{id,round,canvas,count,overlay,elements}`、`design.assets.ready{id,round,produced,errors,missing}`、`design.scene.built{id,scenePath,entities}`、`design.verify.result{id,round,n,passed,global,failed,sceneProblems,screenshots,reportPath}`、`design.done{id,round,passed,verify,summary,acceptedFailures,scenePath}`、`design.notice{id,code,message}`。轮次内事件均带 `runId`。

验收结论只来自服务端截帧与场景文件核对,截图按 sha256 记入报告;未通过只能带原因收尾(`passed: false`),不会记为通过。
