---
contract: F3
title: F3 agent 集群与 skills
status: closed
implementation_status: unlocked
active_scope: wave.1
version: 0.1
date: 2026-08-17
timebox: 会话制推进,做不完转 deferred
rfc_required: []
upstream_docs:
  - 02_SYSTEM_ARCHITECTURE.md (§3 进程拓扑)
  - 04_AGENT_BACKEND.md (全量:§5 swarm / §6 subagent / §3 composer 模式)
  - 05_MCP_PROJECTS.md
  - 06_SKILLS_LIBRARY.md (全量:§1 格式契约 / §3 清单 / §4 编写规范)
  - 07_FRONTEND_IDE.md (§5 Chat / §7 设置页)
  - 09_ENTITY_SCENE_MODEL.md (§3 组件注册表)
  - 11_API_CONTRACTS.md (§2.2 swarm/subagent/skills 路由 / §3.2 事件 / §4 错误码)
  - 12_SECURITY_PERMISSIONS.md (§3 Proposal / I-5 / I-6)
  - 13_ROADMAP.md (F3)
  - milestones/f1/F1_CONTRACT.md
  - milestones/f2/F2_CONTRACT.md
implementation_unlock:
  required_all:
    - F2 验收门全绿(wave.1~wave.5 §6 已录,status=closed)
    - 用户开工指令(2026-08-17「启动 F3 agent 集群与 skills 开发」)
in_scope:
  - forge-agentd swarm 模块(04 §5):SwarmCoordinator 内存态(节点注册表 + 分片表);四分片策略(scene-partition 实体 id 集 / asset-batch 资产路径集 / code-module .rx 文件集 / test-matrix 断言×场景组合);分片创建两两不相交校验,相交 = GOV_SWARM_SHARD_OVERLAP(11 §4);路由 GET /api/forge/swarm/state + POST /api/forge/swarm/seed-demo(11 §2.2)
  - multitask 确定性执行器(04 §3 composer multitask 模式):POST /api/forge/swarm/execute 接收结构化任务 → 按 shardType 切片 → 同进程逻辑 worker 并行执行(04 §5.1「节点 = 本机额外 agentd worker 或同进程逻辑 worker」,本里程碑取后者,零端口扩张)→ 分片报告聚合(shardId/items/ok/errors);无 LLM 依赖(RD-F1-002 仍 open,诚实落地)
  - subagent(04 §6):磁盘 profile data/agents/*.md(front matter:name/description/tools/model/maxSteps + 正文 system prompt)热加载(mtime 变化即重读,无需重启);内建五 profile(scene-builder/asset-wrangler/logic-programmer/qa-tester/material-smith);GET /api/forge/subagents 清单;tools 前缀通配过滤层(mcp__engine-scene__* / mcp__engine-scene__component.* 两形态,D-008)+ task 工具不入任何 profile(防递归委派)
  - skills 管理补全(06 §2):GET /api/forge/skills/{name} 返回 SKILL.md 全文(read_skill 的 HTTP 面);POST /api/forge/skills/config/write(启用/禁用 + 目录配置,落 data/skills-config.json);skills/list 反映 config(禁用项 enabled=false)
  - 首发 skills 文档全量(06 §3 表 13 行——标题「12 个」为文档笔误,按 13 行全量落地并登记):新增 12 篇 SKILL.md(asset-cleanup 已在 F2 落地);其中 9 篇依赖未落地工具面,文档落地 + seam 标注(见 §3 决策 D-F3-A)
  - debug 三件套(13_ROADMAP F3 交付 4):scene_graph_dump 新 MCP 工具(engine-scene;实体全量 + 组件快照 + transform,单次调用)+ viewport_frame 截图 + host_events 事件流 → debug-scene-issue skill 执行链
  - Light 组件注册表扩展 castShadow: bool(09 §3 首发字段集扩展;debug 门「没阴影」语义载体;登记 §3 决策 D-F3-B)
  - 前端:Composer 五模式切换器(build/plan/debug/ask/multitask,07 §5);multitask 分片进度卡片(swarm 执行结果渲染:分片清单/进度/聚合结论);设置页骨架(07 §7.1:左侧菜单 260px + URL ?tab= 深链 + forgeSettingsTabs 单一事实源)+ skills tab(列表/启用禁用/目录配置跳新口径)
  - scripts/f3-w*-*.ps1 栈级冒烟三件套(swarm/subagent+skills/debug)
out_of_scope:
  - 真 LLM 驱动的 multitask 规划与 subagent 委派循环(RD-F1-002 承接;本里程碑确定性模板执行器)
  - test-matrix 分片的多 headless host 并发拉起(04 §5.3;F6 playtest 波,依赖显存预算守卫)
  - 设置页其余 10 tab(general/providers/mcp/permissions/viewport/generation/shortcuts/storage/about/subagent-models;后续里程碑逐个落,如 F5 generation)
  - plan/todo DAG 引擎、session 持久化、memory、checkpoint UI(04 §4/§8/§9;RD-F0-003 agent-cowork 七 crate 移植承接)
  - hooks/plugins/marketplaces(04 §12:照搬面默认关闭,游戏域不承诺)
  - 真实多机分布式 swarm(04 §5.1 逐字:单用户多 agent 并行,非多机分布式)
deferred_refs: [RD-F0-003, RD-F1-002, RD-F1-004]
deliverables:
  - id: D-F3-1
    name: swarm 核心 + multitask 确定性执行器
    evidence: cargo test 输出 + scripts/f3-w1-swarm-smoke.ps1 输出
  - id: D-F3-2
    name: subagent 五 profile 热加载 + skills 管理 API 补全
    evidence: cargo test + scripts/f3-w2-subagent-skills-smoke.ps1 输出
  - id: D-F3-3
    name: 13 篇 SKILL.md + debug 三件套 + Light.castShadow
    evidence: cargo test + scripts/f3-w3-debug-smoke.ps1 输出
  - id: D-F3-4
    name: Composer 五模式 + multitask 卡片 + 设置页骨架 skills tab
    evidence: client vitest + desktop 冒烟截图
acceptance_gates:
  - id: G-F3-1
    name: swarm 分片门
    check: 「给 40 个关卡块生成碰撞体」经 multitask 分片并行完成,分片报告一致、无重叠写(13_ROADMAP F3 逐字)。机验:fixture 40 实体 → POST /swarm/execute(scene-partition)→ 分片两两不相交(创建时校验)+ 40 实体全覆盖 → 每片 component_add RigidBody 实测生效(component_get 抽验)→ 聚合报告一致;构造相交分片请求 → GOV_SWARM_SHARD_OVERLAP 拒绝;GET /swarm/state 返回节点 + 分片状态
  - id: G-F3-2
    name: subagent/skills 注册门
    check: data/agents/ 五 profile .md 落盘即现(热加载:运行中改文件,再查 /api/forge/subagents 反映新值,不重启);tools 前缀通配过滤层 cargo 单测(mcp__engine-scene__* 整族放行 / component.* 族粒度 / 非白名单拦截 / task 工具恒不在任何 profile);GET /api/forge/skills/{name} 返回全文且与磁盘逐字节一致;POST /skills/config/write 禁用后 skills/list 反映 enabled=false
  - id: G-F3-3
    name: debug 门
    check: 「第三盏灯没阴影」debug 会话给出根因与修复,断言通过(13_ROADMAP F3 逐字;playtest 断言以 PIE play_enter/step + 组件态断言落地,playtest-mcp 断言库属 F6,登记 §3 决策 D-F3-C)。机验:fixture 三盏灯(第三盏 castShadow=false)→ debug-scene-issue 三件套收集(scene_graph_dump 输出含 light3 castShadow=false + viewport_frame 截图 + host_events)→ 根因报告 → component_set 修复 → component_get 断言 castShadow==true + play_enter/step/exit 无错
  - id: G-F3-4
    name: 前端门
    check: Composer 五模式切换器真实生效(选中模式随发送载荷);multitask 模式发送后分片卡片渲染分片进度与聚合结论(数据来自 /swarm/* 真实响应);设置页 ?tab=skills 深链直达,skills 列表来自 /api/forge/skills/list 真实数据,禁用写 config 后列表反映;client vitest 全绿;desktop 冒烟截图可见
guardrails:
  - 诚实优先:任何门不过如实报 FAIL/DEV_ENV_DEGRADE,不回写 PASS
  - 数字必须来自命令输出
  - seam skill 文档必须在正文首行标注依赖未落地工具面与承接里程碑,禁止伪装可执行
  - 聚合报告不得遮蔽任一分片失败(分片级布尔独立断言)
  - 双状态机:status/implementation_status 严格分离;契约 §6 只追加
---

# F3 契约:agent 集群与 skills

## 1. 目标与双门状态

实现 13_ROADMAP F3:swarm 四分片策略 + 冲突检测 + multitask 模式 UI;内建 subagent 五 profile + `data/agents/*.md` 热加载;首发 skills 全量 + skill 管理设置页 tab;debug 模式三件套进 `debug-scene-issue`。status=active;implementation_status=unlocked。

## 2. 范围与波次

- wave.1:swarm 核心(Coordinator + 四分片 + 冲突检测 + /swarm/state + /swarm/seed-demo + /swarm/execute)+ multitask 确定性执行器 + f3-w1 冒烟 → G-F3-1。
- wave.2:subagent 五 profile + 热加载 + /api/forge/subagents + 前缀通配过滤层 + skills/{name} + skills/config/write + f3-w2 冒烟 → G-F3-2。
- wave.3:13 篇 SKILL.md 全量(9 篇 seam 标注)+ Light.castShadow 注册表扩展 + scene_graph_dump 工具 + debug-scene-issue 执行链 + f3-w3 冒烟 → G-F3-3。
- wave.4:Composer 五模式 + multitask 分片卡片 + 设置页骨架 + skills tab + desktop 冒烟 → G-F3-4。

## 3. 架构决策(wave.1 立项裁决)

- **D-F3-A(seam skill 处置)**:06 §3 表实为 13 行(标题「首发 12 个」系文档笔误,如实登记,不改冻结文档)。13 篇中仅 4 篇工具面已齐(asset-cleanup 已验 / scene-greybox / asset-import-batch / debug-scene-issue 本里程碑补 graph_dump 后可验);其余 9 篇(skill-creator 缺 fs 工具面、scene-dressing 缺 physics_overlap/cast_ray、prefab-workflow 缺 prefab 工具、material-tuning 缺 render_get_settings、gen-asset-fill 缺 F5 gen 面、logic-blueprint-gen/code-rx-migration 缺 F4 code-forge、playtest-regression/perf-budget-check 缺 F6 playtest/帧统计)文档全量落地,正文首行 seam 标注「依赖 <工具面>(<承接里程碑>),当前不可执行」。验收门只考可执行篇。
- **D-F3-B(Light.castShadow)**:09 §3 Light 首发字段 kind/color/intensity 无阴影语义;debug 门「没阴影」需载体。扩展注册表 Light 增 `castShadow: bool`(默认 true)。渲染内核不消费该字段(rurix-rt 无阴影贴图面,如实标注:字段为场景数据语义,渲染效果待渲染波)。登记为对 09 §3 的字段集扩展,不改冻结文档。
- **D-F3-C(playtest 断言降级)**:roadmap G-F3 debug 门字面「playtest 断言通过」;playtest-mcp 断言库属 F6。本里程碑以 PIE 双态(play_enter/step/exit)+ component_get 组件态断言 + viewport_frame 截图为等价机验,登记偏差;F6 落地断言库后回填。
- **D-F3-D(swarm 节点形态)**:04 §5.1 允许「同进程逻辑 worker」;本里程碑 swarm 节点 = agentd 同进程 tokio task 逻辑 worker(零端口扩张、零进程守护面),SwarmCoordinator 内存态注册表 + 分片表,API 形态照 11 §2.2。多 host/多进程 worker 留 F6 test-matrix。
- **D-F3-E(multitask 无 LLM)**:RD-F1-002(真 LLM 工具循环)仍 open;multitask 执行器为确定性模板:/swarm/execute 收结构化任务描述(shardType + items + operation),模板化「40 关卡块碰撞体」类指令的解析在前端/调用方完成结构化,agentd 不伪造 LLM 规划。

## 4. Deferred 处置
本波新增 deferred 追加于下方。

deferred:
  - id: RD-F3-001
    content: 06 §3 标题「首发 12 个」与表格 13 行不一致
    reason: 文档集冻结(只勘误经独立 errata 流程);本契约按 13 行全量落地
    refill: errata 流程勘误 06 §3 标题为 13 个
    owner: 下次文档勘误窗口
    status: OPEN

## 5. 修订
- 2026-08-17 立项:F2 全绿(wave.1~5 §6,status=closed)后用户指令开工「启动 F3 agent 集群与 skills 开发」。

## 6. Close-out(只追加区)
<!-- 禁止预填 PASS -->

### wave.1 验收记录(2026-08-17)

- 验收门:G-F3-1(swarm 分片门)
- 结果:PASS
- 证据:
  - scripts/f3-w1-swarm-smoke.ps1 PASS(全链经 gateway→agentd→engine-scene-mcp):scene_new 独立场景 + 40 关卡块 fixture(id 1..40,不 scene_save,demo 项目零污染)→ POST /api/forge/swarm/execute(scene-partition,4 片,add_component RigidBody{kind:static,mass:0})→ 4 片全 done、每片 okCount=10、聚合 totalItems=40 succeeded=40 failed=0 disjoint=true → component_get 抽验 3/40(首/中/末)RigidBody kind=static 真实落上 → 重复输入集 409 GOV_SWARM_SHARD_OVERLAP → seed-demo +2 节点、state 3 节点 + 4 done 分片可见
  - cargo test --workspace 71/71 PASS(forge-agentd 12→25:swarm.rs 8 单测——round-robin 切片全覆盖不相交/重复项拒绝/活动分片相交拒绝/终态释放/无效域/seed 幂等/失败不遮蔽;main.rs 5 HTTP 层——state 默认节点/seed-demo/409/400 seam/40 块全链集成)
  - 冲突检测双层:输入集自身重复 + 与非终态同类型分片相交,均 GOV_SWARM_SHARD_OVERLAP(04 §5.2「创建时校验两两不相交」逐字)
- 交付:
  - crates/forge-agentd/src/swarm.rs:SwarmCoordinator 内存态(节点注册表 capabilities/maxConcurrency/healthStatus/loadScore + 分片表 pending/running/done/failed + report 回填);四分片策略常量;round-robin 切片(天然不相交)
  - 路由:GET /api/forge/swarm/state、POST /swarm/seed-demo、POST /swarm/execute(11 §2.2 形态)
  - multitask 确定性执行器(D-F3-E):operation_call 域分发——scene-partition 支持 add_component/remove_component/set_component;asset-batch 支持 asset_reimport;code-module/test-matrix 返回「域未落地(F4/F6 承接)」seam 错误;逻辑 worker = 每片一个 tokio task(04 §5.1 同进程逻辑 worker;MCP stdio 单连接 Mutex 串行化,分片间为逻辑并行,如实标注)
  - scripts/f3-w1-swarm-smoke.ps1
- 踩坑:
  1. PowerShell WebClient.UploadString(url,"GET","") 被 .NET 以「GET 不可带 body」**客户端侧**拒绝(请求根本未发出),冒烟误判「节点数 1」——实为 code=0 + $null 计数假象;对策:GET 走 DownloadString。服务端零缺陷(curl 实测同请求 200)。
  2. cargo test --workspace 首跑报上游 rurix-render E0308 编译错;`cargo build -p rurix-render` 重建后清除,后续两跑全绿——判定为陈旧增量产物伪错(未复现,如实留痕)。

### wave.2 验收记录(2026-08-17)

- 验收门:G-F3-2(subagent/skills 注册门)
- 结果:PASS
- 证据:
  - scripts/f3-w2-subagent-skills-smoke.ps1 PASS(全链经 gateway→agentd):GET /api/forge/subagents 返回内建五 profile(errors=0;logic-programmer 含 `mcp__engine-scene__component.*` 族白名单逐字;qa-tester maxSteps=16)→ 热加载实测:新 profile `zz-smoke-hot.md` 落盘即现、改 maxSteps 7→42 不重启再查即反映 → GET /api/forge/skills/asset-cleanup 返回全文与磁盘**逐字节一致**(不存在 404 / 非法名 400)→ POST /skills/config/write 禁用 asset-cleanup 后 list 反映 enabled=false、还原后 true、非法名 400
  - cargo test -p forge-agentd 35/35 PASS(新增 subagents.rs 6 单测——front matter 全字段解析/缺字段与 task 名拒绝/整族通配/component.* 点号族/task 恒拒(全通配也不放行)/临时目录热读;main.rs 4 HTTP 层——五 profile 校验/热加载/全文逐字节/config 禁用还原)
- 交付:
  - data/agents/*.md 五 profile(04 §6 逐字白名单:scene-builder 24 / asset-wrangler 24 / logic-programmer 32 / qa-tester 16 / material-smith 16;正文含「必须遵守」纪律:先查询后修改/dryRun 优先/删除防护不绕过/seam 如实报)
  - crates/forge-agentd/src/subagents.rs:parse_profile(front matter + 正文;BOM 容忍;task 名拒绝)/ list_subagents(每次重读盘 = 热加载;解析失败进 errors 不遮蔽)/ tool_allowed(D-008 前缀通配:整族 `*` + 方法族 `component.*` 点号归一;task 恒 false)
  - 路由:GET /api/forge/subagents、GET /api/forge/skills/{name}(read_skill HTTP 面)、POST /api/forge/skills/config/write(data/skills-config.json:disabled + extraDirs;写盘即生效)
  - skills/list 升级:enabled 标注 + extraDirs 追加扫描 + 同名去重
  - scripts/f3-w2-subagent-skills-smoke.ps1
- 踩坑:PS 5.1 `Set-Content -Encoding UTF8` 写 BOM 会破坏 front matter 首行 `---` 判定——解析器加 BOM 容忍;冒烟侧一律 `[IO.File]::WriteAllText` 无 BOM 落盘。

### wave.3 验收记录(2026-08-17)

- 验收门:G-F3-3(debug 门)
- 结果:PASS
- 证据:
  - scripts/f3-w3-debug-smoke.ps1 PASS(全链经 gateway→agentd→engine-scene-mcp):fixture 三盏灯(light-3 castShadow=false)→ **三件套**收集:scene_graph_dump 单次调用全量呈现 light-3 Light.castShadow=false(根因入证)+ viewport_frame 320×240 rgba8 截图(pixelsB64=409,600 字符)+ host_events_drain 内存事件环(scene.created + 三灯 entity.created 在环)→ component_set 修复(castShadow=true,其余字段未动,component_get 复核)→ 断言:play_enter/step/play_state(play_running)/play_exit→edit 双态无错 + 修复后截图 → skills/list 13 篇全量(4 可执行 + 9 seam),debug-scene-issue 全文可读
  - cargo test --workspace 80/80 PASS(forge-scene castShadow bool 校验正反例;engine-scene-mcp watchdog 40 工具断言;forge-agentd 35)
- 交付:
  - forge-scene:Light 注册表扩展 `castShadow: bool`(D-F3-B;validate_props 新增 bool 类型臂;09 §3 字段集扩展登记,渲染内核不消费如实标注)
  - engine-host rpc:`scene.graph_dump`(实体 id/name/transform/组件快照单次调用)
  - engine-scene-mcp:`scene_graph_dump` + `host_events_drain`(→ events.drain 内存事件环;原 host_events 为 supervisor 崩溃日志,语义已在工具描述与 skill 内区分);**recursion_limit 提额 256**(tools/list 巨型 json! 字面量随工具数增长触及默认上限,crate root 留档)
  - agentd KNOWN_TOOLS 53→55(main.rs 断言同步);skills/ 新增 12 篇 SKILL.md(可执行:scene-greybox/asset-import-batch/debug-scene-issue;seam 标注 9 篇:skill-creator/scene-dressing/prefab-workflow/material-tuning/gen-asset-fill/logic-blueprint-gen/code-rx-migration/playtest-regression/perf-budget-check——首行标注依赖与承接里程碑,D-F3-A)
  - scripts/f3-w3-debug-smoke.ps1
- 踩坑:
  1. host_events ≠ events_drain:host_events 读 supervisor 崩溃日志 jsonl(无场景域事件),roadmap「events_drain」字面指 engine-host 内存事件环——补 host_events_drain 工具而非篡改断言充绿。
  2. entity_create 内联 components 只发 entity.created,不另发 component.added(事件语义实测,冒烟断言按真实语义)。
  3. viewport_frame 返回字段为 pixelsB64(format=rgba8),非 rgba8 键;play_state 词汇为 play_running/edit,非 playing。

### wave.4 验收记录(2026-08-17)

- 验收门:G-F3-4(前端门)
- 结果:PASS
- 证据:
  - client vitest 50/50 PASS(新增 8:editorStore 五模式载荷/multitask 卡片/模板未命中如实报错 3,settingsView 深链/禁用写 config/tab 切换/非法 tab 回落 4,app settings 路由 1;editorView UI 层五模式切换 + swarm-card 渲染 1)
  - pnpm --filter @forge/client typecheck/build 绿;host vitest 18/18 PASS
  - scripts/f3-w4-settings-smoke.ps1 PASS:agentd 8103 → desktop FORGE_SMOKE_SCENARIO=settings → 侧栏 Settings → skills tab 真实列表 13 篇(skill items: 13)→ 截图 desktop-smoke-settings-2026-08-17T13-34-16-533Z.png(167,764 B,左 11 tab 菜单 + 13 skill 行 + switch 开关真实渲染可见)
- 交付:
  - client:editorStore ComposerMode 五模式 + SwarmReport + executeMultitask(碰撞模板 → /api/forge/swarm/execute;未命中如实报错,D-F3-E 不伪造 LLM);sendChat mode 载荷 + multitask 分片卡片消息;EditorView ChatDock 五模式切换器(data-mode 锚点/选中态/placeholder 随模式)+ swarm-card 分片进度与聚合结论渲染;SettingsView 骨架(左 260px 11 tab 菜单 + 右内容区,07 §7.1 cindy 模式)+ skills tab 真实功能(list/switch 禁用/写 config 即生效)+ ?tab= 深链双向同步(replaceState 无历史堆栈);forgeSettingsTabs 单一事实源(11 tab,skills=F3 落地/generation=F5/余如实占位);Sidebar Settings 钮 + store openSettings 路由
  - host forgeProxy:PROXY_PREFIXES 扩 skills/subagents/swarm/proposals(desktop/web 场景 client 单源 3080,agentd REST 面必须经 host 透传——desktop settings 冒烟首跑 count=0 根因即此,F2/F3 栈级冒烟直连 gateway 未暴露)
  - desktop main.cjs:settings 冒烟场景(nav click → 3s 等列表 → data-skill-name 计数 >=13 断言)
  - scripts/f3-w4-settings-smoke.ps1
- 踩坑:
  1. PS 5.1 `2>&1` + `$ErrorActionPreference='Stop'`:pnpm stderr 命令回显行被包成 ErrorRecord 误抛——收集段切 Continue 再按 exit code 判定。
  2. desktop smoke.log 为 append 留档:断言取值必须取最后一次匹配行(-match 默认取首行,旧运行残留行会代绿/误红)。
  3. editorStore.ts 磁盘存 `slice(0, 40]` 语法错误(波内工具编辑期间未跑 typecheck 未暴露)——wave 收尾必须 typecheck 先行。

### wave.5 验收记录(2026-08-17):全量回归 + close-out 终审

- 全量回归数字(均来自命令输出):
  - cargo test --workspace **80/80 PASS**(engine-host 含 f1/f2 集成、engine-scene-mcp watchdog、forge-agentd 35、forge-scene 7、assetd 15 等)
  - pnpm -r typecheck 全绿;pnpm -r test **73/73 PASS**(protocol 5 + client 50 + host 18);pnpm -r build 全绿
  - go test ./...(gateway-go)ok
  - desktop 冒烟:home 场景 PASS(75,681 B)+ settings 场景 PASS(167,764 B,skills 13 篇)
- 四门终审:
  - G-F3-1 swarm 分片门:PASS(wave.1 §6)——40 实体 scene-partition 4 片两两不相交全覆盖,RigidBody 实测生效,相交构造 GOV_SWARM_SHARD_OVERLAP 拒绝
  - G-F3-2 subagent/skills 注册门:PASS(wave.2 §6)——五 profile 热加载,前缀通配过滤层(task 恒拒),skills/{name} 逐字节,config/write 禁用反映
  - G-F3-3 debug 门:PASS(wave.3 §6)——三件套收集(graph_dump/viewport_frame/host_events_drain)→ castShadow 根因 → 修复 → PIE 双态断言
  - G-F3-4 前端门:PASS(wave.4 §6)——五模式载荷 + multitask 分片卡片 + 设置页 ?tab=skills 深链 + 禁用写 config + desktop 截图可见
- 结论:**F3 四门全绿,里程碑 close-out。status: active → closed。**
- open RD 不阻收官(均有 refill 路径):RD-F3-001(06 §3 标题 12 vs 表 13 行笔误,待 errata 窗口)。
- 下一里程碑可选:**F4 code-forge**(rx 脚本编译/热重载/蓝图,承接 logic-blueprint-gen/code-rx-migration 两篇 seam skill)或先消化 open RD(RD-F1-002 真 LLM 工具循环为 agent 集群真核)。

### deferred 终态同步(2026-08-18,RD 消化波;只追加)

- **RD-F3-001 → CLOSED**:errata 流程执行完毕——06_SKILLS_LIBRARY.md Errata(只追加区)E-06-001 追加:§3 标题「首发 12 个」为笔误,表实列 13 行与仓内 skills/ 目录 13 个 SKILL.md 实测一致,正确表述「首发 13 个」。
