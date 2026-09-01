# 04 · Agent 后端(forge-agentd)

> 架构照搬 `D:\agent-cowork\backend-rs`(agentd):同 crate 划分、同会话/plan/todo/工具循环、
> 同 swarm 集群、同记忆与 checkpoint 机制。本章定义**移植面**与**游戏域扩展面**;
> 与上游语义冲突时以本章为准并登记决策日志。

## 1. crate 划分(移植 + 扩展)

| crate | 来源 | 职责 | 改动 |
|---|---|---|---|
| `agent-config` | 照搬 | 环境变量配置(端口、数据目录、预算、压缩参数) | 变量前缀 `AGENT_DEBUG_*` → `FORGE_AGENT_*`(D-005);新增 `FORGE_AGENT_PROJECT_ROOT` |
| `agent-protocol` | 照搬 | wire 契约:DTO、事件信封、错误码 | 事件类型扩展场景/资产域(`11 §3`) |
| `agent-store` | 照搬 | redb KV、JSONL 事件日志、事件总线、加密 | 新增表 `T_SCENE_SNAPSHOTS`、`T_ASSET_PROPOSALS`(`12 §4`) |
| `agent-providers` | 照搬 | LLM provider 抽象 + 多厂商适配器(OpenAI 兼容/Anthropic/…);无 key 回退 mock | 增图像/3D 生成 provider 抽象?否——生成走 MCP 而非 provider(D-006) |
| `agent-mcp` | 照搬 | MCP 客户端管理器:`mcp.json` 声明、stdio/streamable-HTTP、工具发现注入 `mcp__{server}__{tool}` | 增 `autoStart` 伴生进程拉起(`05 §1.2`) |
| `agent-tools` | 照搬 | 工具注册表:fs/command/edit/web/skill/workspace | 白名单按 profile 收紧;`run_command` 在引擎项目根受限(`12 §2`) |
| `agent-core` | 照搬 + 扩展 | 会话、plan/todo 引擎、回合循环(engine/*)、swarm、subagent、memory、checkpoint、permission、hooks、profile、prompts | 游戏域 prompts 包(`04 §7`);profile 增 `gamedev`?否——沿用 `coding`(D-007) |

## 2. Agent Profile(沿用 agentKind)

| agentKind | 用途 | 工具白名单 | composer 模式 |
|---|---|---|---|
| `coding`(默认) | 游戏开发全能:改代码、搭场景、处理素材 | 全部工具 + MCP 全域 | build / plan / debug / ask / multitask |
| `document` | 设计文档、剧情文本、本地化表 | 只读 + web + write_file/create_document + todo + task | ask / build |
| `general` | 咨询问答 | 只读 + web + task | ask / build |

决策:不新增 `gamedev` kind(D-007)。游戏域差异经 **prompts 包 + MCP 工具面 + skills** 表达,
不复制一套 profile 机器——理由:agent-cowork 已证明 profile 数量应极少,域知识下沉到 skill。

## 3. composer 模式(照搬)

| 模式 | 语义 | 游戏域典型用法 |
|---|---|---|
| `build` | 直接执行 | 「把迷宫加一圈外墙」「导入这个目录的贴图」 |
| `plan` | 先生成 Plan DAG,用户批准后执行 | 「做一个第三人称迷宫原型」 |
| `debug` | 诊断循环:读日志/事件/截图 → 假设 → 验证 | 「角色跳不起来」「第三盏灯没阴影」 |
| `ask` | 只读问答 | 「这个材质为什么这么亮」 |
| `multitask` | swarm 分片并行 | 「给 40 个关卡块批量生成碰撞体」「全场景灯源参数规范化」 |

非 coding 会话传入 plan/debug/multitask 自动降级 build(照搬上游语义)。

## 4. Plan / Todo 引擎(照搬)

- `plan:generate`:LLM 生成结构化 Plan(stage/task DAG,带启发式回退),落 `T_PLANS`;派生 TodoItem。
- 执行:plan_exec 按 DAG 调度;每 todo 可 rerun(`todos:batch-rerun` 供批量重试)。
- 游戏域扩展:PlanTask 增可选字段 `verifyRef`(指向 playtest-mcp 断言或场景 diff 检查),任务完成判定可挂自动验证——默认空,纯事件判定。

## 5. Swarm 集群(照搬 + 扩展)

### 5.1 移植面

- `SwarmCoordinator`:内存态节点注册表 + 分片表;节点注册携带 `capabilities` / `supportedTools` / `maxConcurrency` / `healthStatus` / `loadScore`;分片 round-robin 派工,挂 `parentPlanNodeId` / `parentTodoId`。
- API:`/api/forge/swarm/state`、`/api/forge/swarm/seed-demo`(照搬路由形态)。
- 定位:**单用户多 agent 并行**,不是多机分布式;节点 = 本机额外 agentd worker 或同进程逻辑 worker。

### 5.2 游戏域分片策略(新增 shard_type)

| shardType | 输入 | 适用任务 | 安全约束 |
|---|---|---|---|
| `scene-partition` | 实体 id 集合(按区域/文件夹划分) | 批量摆物、灯光规范化、碰撞体生成 | 分片间实体集不相交(写冲突避免) |
| `asset-batch` | 资产路径集合 | 批量导入/重建/改导入设置 | 同资产不跨片 |
| `code-module` | `.rx` 文件集合 | 批量重构、诊断修复 | 同文件不跨片 |
| `test-matrix` | 断言 × 场景组合 | playtest 回归 | 只读,允许同场景并发(独立 host 实例) |

冲突检测:分片创建时校验输入集两两不相交;相交 = `SWARM_SHARD_OVERLAP` 错误(I-5)。

### 5.3 多 engine-host 并发

- 默认单 host(单 GPU 场景编辑)。`test-matrix` 分片可拉起额外 **headless** host 实例(`sim.runHeadless`),数量上限 = `FORGE_AGENT_MAX_HEADLESS_HOSTS`(默认 2),显存预算守卫。

## 6. Subagent(照搬机制,扩展 profile 集)

机制照搬:磁盘 profile `data/agents/*.md`(front matter: name/description/tools/model/maxSteps + 正文 system prompt,热加载);`task` 工具不进入任何 profile(防递归委派)。

游戏域内建 profile:

| name | description | tools(白名单) | maxSteps |
|---|---|---|---|
| `scene-builder` | 场景搭建与批量摆放 | mcp__engine-scene__*、mcp__project__*、read_file | 24 |
| `asset-wrangler` | 素材导入/构建/引用修复 | mcp__asset-pipeline__*、mcp__gen-image__*、mcp__gen-model__*、read_file | 24 |
| `logic-programmer` | `.rx` 脚本与节点图逻辑 | mcp__code-forge__*、mcp__engine-scene__component.*、read_file、grep | 32 |
| `qa-tester` | 运行验证与回归 | mcp__playtest__*、mcp__engine-scene__viewport.screenshot、read_file | 16 |
| `material-smith` | 材质与灯光调整 | mcp__engine-scene__*(render/component 子集)、mcp__asset-pipeline__* | 16 |

profile 的 `tools` 支持 MCP 工具名前缀通配(`mcp__engine-scene__*` 或方法族粒度 `mcp__engine-scene__component.*`),实现时在 task 工具的过滤层做前缀匹配(D-008)。

## 7. Prompts 包(游戏域)

- `prompts/coding.rs` 在照搬基础上注入「游戏引擎上下文块」:当前项目根、打开场景路径、引擎能力摘要(组件注册表、MCP 域清单)、红线(R-1~R-5)。
- 注入预算沿用上游(`FORGE_AGENT_CONTEXT_WINDOW_MAX_CHARS` 等);组件注册表经 `component.listTypes` 摘要化(名称+字段签名,不含描述长文)。

## 8. 记忆(照搬)

- scope:`global` / `workspace:{root}` / `session:{id}`;kind:`preference` / `fact` / `convention`。
- 工具 `memory_write` / `memory_search` / `memory_delete`;每轮检索 Top-N 注入系统提示。
- 游戏域用法示例:「用户偏好低多边形风格」「项目命名规范 PascalCase」「禁止动 `Content/Shared` 目录」。

## 9. Checkpoint 与 Workspace(照搬 + 扩展)

- 照搬:文件编辑前自动 checkpoint;`workspace/revert` 回滚;会话 checkpoint 列表 API。
- 扩展(对接 `12 §4`):场景级快照 = `.rxscene` 复制 + 引擎状态元数据,存 `T_SCENE_SNAPSHOTS`;agent 批量场景操作前必须先建快照(工具面强制,`05 §2.4`)。

## 10. Providers(照搬)

- 渠道管理 API(`/api/forge/channels`、`channels:fetch-models`);OpenAI 兼容 + Anthropic;无 key = mock provider(开发/CI 恒绿)。
- 密钥存 agentd 数据目录 keystore(加密),永不进前端/项目文件/日志(红线 R-5)。
- 每 profile / subagent 可 `model` 覆盖;会话级 `/api/forge/sessions/{id}/model` 切换。

## 11. 事件与 SSE(照搬)

- JSONL 事件日志 + EventBus;`sessions/{id}/events/stream` SSE;`replay/{id}`、`replay/{id}/since` 回放。
- 扩展事件类型(`11 §3.2`):`scene.changed`、`asset.built`、`playtest.result`、`host.crashed`、`proposal.created` 等。

## 12. Hooks / 权限 / 其他照搬面

- hooks(浏览器/自定义 webhook 通知)、plugins、marketplaces:照搬,默认关闭,游戏域不承诺。
- permission:权限模式 per-session 切换(`sessions/{id}/permission-mode`),规则 API `permissions/rules`;游戏域规则集见 `12 §2`。
- terminal/shells:照搬;IDE 底部 Terminal 面板复用。
