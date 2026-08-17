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

规则:新错误码进本表 + `forge-protocol` 枚举,同 PR;不允许字符串裸抛。
