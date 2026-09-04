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
| 生成 | `GEN_*` | `GEN_BACKEND_NOT_CONFIGURED` `GEN_RATE_LIMITED` |
| 测试 | `TEST_*` | `TEST_TIMEOUT` `TEST_ASSERT_FAILED` |
| 治理 | `GOV_*` | `GOV_PROPOSAL_REQUIRED` `GOV_PERMISSION_DENIED` `GOV_SWARM_SHARD_OVERLAP` |
| 宿主 | `HOST_*` | `HOST_UNREACHABLE` `HOST_CRASHED` `HOST_GPU_LOST` |
| 商店(F11) | `STORE_*` | `STORE_SOURCE_UNREACHABLE` `STORE_SOURCE_NOT_FOUND` `STORE_PACKAGE_NOT_FOUND` `STORE_VERSION_NOT_FOUND` `STORE_MANIFEST_INVALID` `STORE_CHECKSUM_MISMATCH` `STORE_ALREADY_INSTALLED` `STORE_NOT_INSTALLED` `STORE_DEPENDENCY_UNRESOLVED` `STORE_PAYMENT_REQUIRED` `STORE_TASK_NOT_FOUND` `STORE_PUBLISH_REJECTED` |
| 技能(F11) | `SKILL_*` | `SKILL_NOT_FOUND` `SKILL_NAME_INVALID` `SKILL_ALREADY_EXISTS` `SKILL_FRONTMATTER_INVALID` `SKILL_BODY_INCOMPLETE` `SKILL_READONLY_DIR` |

规则:新错误码进本表 + `forge-protocol` 枚举,同 PR;不允许字符串裸抛。

## Errata(只追加区)

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
