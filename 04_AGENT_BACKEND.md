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

## Errata(只追加区)

- **E-04-005(2026-09-03,用户指令波)——运行时工具 `read_file` 增可选行区间参数**:schema 加 `offset`(1 基起行)+ `limit`(最多读取行数),实现复用 `native_tools::read_lines`(与 `resource_get` 文档腿同一 confine 口径),越界收敛到文件末尾而非报错。**两参都缺省时行为与此前逐字节一致**(全文原样返回),故既有调用与 profile 白名单不受影响。动因有二:大文件按需分段读以控 token;前端过程链据此如实显示「`Read timeline.ts L90-625`」的行区间——没有参数就不显示区间,不按内容长度伪造(见 07 E-07-008)。

- **E-04-002(2026-09-03,D-035)——§3 plan 模式语义重定义 + §4 计划产物改为工作区文件 + §6 工种 8→9(as-built)**:①**§3 表中 `plan` 行「先生成 Plan DAG,用户批准后执行」的落地形态确定为**:调研四阶段(了解项目 → 同一轮并发派 2–4 个 `explore` 子代理 → 读核心文件 → 出计划)+ 计划文件产物,「用户批准」= Plan 页签的 Build 按钮(`ask:execute` 带结构化 `planPath`),不是聊天里的批准卡。②**§4「plan:generate 落 T_PLANS」正式作废**(E-04-001 已把 Plan DAG 折进 TodoItem;本波进一步确认不建 plan 实体表):plan 模式产物 = 工作区文件 `.forge/plans/<slug>.plan.md`(front matter `name`/`overview`/`todos[id,content,status]` + Markdown 正文),唯一写入口是新原生工具 `create_plan`(实现 `crates/forge-agentd/src/plan_doc.rs`);**plan 模式工具面不再含 `plan_write`/`todo_write`/`todo_update`**——计划待办随文件走,Build 时才按 `TodoItem.plan_todo_id` 幂等物化进 TodoStore(`source="plan"`)。`team` 模式的 `plan_write` + `plan.rs` 波次编排原样不变,两条路径互不影响。③**新增事件** `plan.created` / `plan.updated` / `plan.build.started`(`events::channel_for` 的 `plan` 频道自 F7 起预留,本波首次实发);会话新增 `activePlanPath` 指针(serde default,旧 sessions.json 兼容,fork 时继承)。④**§6 内建 profile 8→9**:新增只读工种 `explore`(`data/agents/explore.md`,maxSteps 20)——只回答一个明确调研问题、汇报带路径+行号+引用片段,不出计划不派单(与偏「游戏立项分阶段任务清单」的 `planner` 分工不同,后者保留)。⑤**修补只读纪律漏洞(as-built 缺陷)**:此前无论父轮什么模式,`run_nested_task` 一律给子代理 build 全量工具面且 `forbidden: None`,只靠 profile 白名单兜底——plan 模式下不带 `subagent_type` 的通用子代理能写盘;现 `SubTaskCtx.read_only` 随父轮 mode 传递,spec 侧剔写工具与 `create_plan`、exec 侧 `TOOL_FORBIDDEN`,与父循环双门同纪律。⑥**turn 无历史的补偿**:`run_tool_loop` 每轮只发 `[system, preamble, user]`,故 Build 轮注入计划全文 +「待办真 id ↔ 标题」映射,plan 迭代轮注入现有计划全文作基线(注入序:技能 → 计划 → 检索上下文,按约束强度排)。
- **E-04-004(2026-09-03,D-038)——回执送达改为即时 + 唤醒(修订 E-04-003 已知边界①)**:E-04-003 记的「回执注入是下一轮用户发言时,没有下一轮就只在对话流里可见」**不再成立**。现三条送达路径:主 agent 正在跑 → 其 `run_tool_loop` 每迭代开头经 `ToolLoopCfg.inbox` 取件,以 user 消息插入(`agent.receipts.injected{midTurn:true}`);主 agent 空闲 → 子代理终态起**唤醒轮**(`run.trigger=receipt_wake`,`composer.user.message{source:"receipt"}`,正文「【系统唤醒】…」,回执走 preamble),以派发时抓拍的 `WakeCtx`(内存 `WakeRegistry`)重建 TurnInput,不重解析模型、不重拉 MCP;都没赶上 → turn 收尾清点再起唤醒轮。互斥:`SessionStore::claim_active_run/release_active_run`(CAS),`execute_turn` 认领失败返回 `SESSION_BUSY`(HTTP 409)。落点:`agent.rs`(`TurnOrigin`/`WakeCtx`/`WakeRegistry`/`schedule_wake`/`run_wake_turn`/`wake_user_text`)、`llm.rs`(`ToolLoopCfg.inbox`)、`sessions.rs`(claim/release)。**已知边界**:①进程重启后没有 WakeCtx,清扫出的 failed 回执退回「下一轮用户发言注入」,不凭空猜模型;②唤醒轮用派发时的模型,用户中途换模型不影响它(再发一句话即用新模型);③唤醒轮是完整 turn,期间输入框按常规锁定,用户抢发得 409 + 提示。
- **E-04-003(2026-09-03,D-036)——§3 multitask 语义改写 + §5 swarm 定位收缩(as-built)**:§3 表中 `multitask` 的「swarm 分片并行」语义**退役**,改为**异步子代理委派**:主 agent 拿只读侦察面 + 新原生工具 `dispatch`,一轮内把需求拆成互不重叠的自足子任务并全部派出,`dispatch` 立即返回受理回执(父轮不等)、后台 run 各自跑完再把回执落收件箱。原「正则命中碰撞体 → entity_list → 四分片加 RigidBody」的服务端模板链(F7 wave.2 落地)整体删除,含「模板未命中 → agent.failed」口径。**§5 swarm 随之收缩为纯 API 面**:`SwarmCoordinator` 与 `/api/forge/swarm/{state,seed-demo,execute}` 保留(确定性批量执行仍走它),但不再是任何 composer 模式的实现;§5.2 四分片策略与不相交校验不变。落点:`crates/forge-agentd/src/receipts.rs`(新,收件箱 + 注入段装配 + 崩溃清扫)、`agent.rs`(`MULTITASK_PROMPT_SUFFIX` / `spawn_detached_subagent` / `DetachedCtx` / `take_receipts_for_turn`)、`engine/mod.rs`(`DISPATCH_TOOL` + multitask 工具面)。并发上限 env `FORGE_AGENT_MAX_BG_SUBAGENTS`(缺省 4)。已知边界:①回执注入是「下一轮用户发言时」,没有下一轮就只在对话流里可见(不自动触发复盘轮);②后台 run 只存内存,进程重启由启动清扫置 failed 并补发 `subagent.failed`;③排队中的派发在拿到许可前不出卡片(如实,不假装在跑)。
- **E-04-001(2026-08-31,F-GAME-4 / D-031)——§4 plan/todo 引擎兑现 + team 模式代码级编排 + 子代理并行与按角色配模型(as-built)**:D-017 遗留的「plan/todo DAG 按需立项」在本波兑现,落点与 §4 原设计的差异如实记录:①**Plan DAG 落在 TodoItem 扩展而非独立 T_PLANS 表**——`TodoItem` 加可选字段 `stage/deps[]/role/prompt/verify(none|qa|reviewer)`(serde default,旧 todos.json 兼容,未设置不进 wire);`plan_write`/`todo_write` schema 同步五字段,plan 模式产物即可执行 DAG;deps 引用约定 = id 优先、title 次之(同批任务互引用 title,id 落库才生成)。②**调度器** `crates/forge-agentd/src/plan.rs`:纯函数核心(就绪集/同阶段波次/失败逐层传播/停摆诊断——循环依赖与悬空依赖如实报停摆不死循环)+ 波次执行器(波内并行上限 4,todo.updated 事件推进,状态机即 TodoStore 本身不设内存副本)。③**team 模式从纯提示词纪律升级为代码级编排** `run_team_flow`:leader 轮产结构化计划 → 波次并行派发(复用 run_nested_task,合成 parentToolCallId `team-<todoId>`)→ verify=qa 任务完成自动派 qa-tester 复测(轮内去重)→ 问题回注 leader 修复轮(`agent.steered` 留痕)→ 全部完成派 reviewer 终审,最终消息强制 `VERDICT: APPROVE|REJECT`——REJECT 回修复轮,qa 失败与 REJECT 共享修复轮上限 3,超限**如实 agent.failed**(I-5);缺 VERDICT/QA_RESULT 标记一律按未通过(宁严不假);leader 轮无计划(如 mock provider)维持原路径直接收束(CI 恒绿)。④**task 工具并行**(llm.rs):同轮 tool_calls 全为 task(≥2)时分段并发(上限 4,join_all,段间查取消令牌),结果按原 call 顺序回注;并发下父时间线归属经 executor args 注入 `_toolCallId` 解决(串行路径不注入,原语义逐字不变);混合轮维持串行(后续按需再扩)。⑤**§10「每 subagent 可 model 覆盖」兑现**:`run_nested_task` 按 profile.model 走决议(`resolve_profile_provider`),未知/未配齐回落父 provider 并在 subagent.started 的 `modelNote` 字段如实说明;父会话 mock 恒 mock。⑥新增角色档案 `data/agents/planner.md`(只读调研/计划,maxSteps 24)与 `reviewer.md`(对抗式终审:Functionality→Visual→Playability 递进、首个否决点即停、强制 VERDICT 结尾;只读 + play/viewport 查询白名单,maxSteps 32),工种 6→8。已知边界:子代理循环内无取消令牌(取消在波间/段间/leader 轮生效);修复轮 leader 为无历史独立轮(问题清单随提示回注)。
