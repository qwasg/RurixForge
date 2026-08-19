---
contract: F7
title: F7 Agent 前端重设计(严格对齐 I:\agent-debug-frontend-backend-copy-20260530)
status: closed
implementation_status: unlocked
active_scope: full
version: 0.1
date: 2026-08-18
rfc_required: []
upstream_docs:
  - I:\agent-debug-frontend-backend-copy-20260530(参考事实源,只读)
  - 13_ROADMAP.md (§F7 追加)
  - 04_AGENT_WORKFLOW.md (会话模型)
  - 14_DECISION_LOG.md (D-020)
implementation_unlock:
  required_all:
    - 用户开工指令(2026-08-18 /goal 原文:「将当前编译器中前端所有agent的部分都(除游戏原生功能之外的所有部分)严格按照 I:\agent-debug-frontend-backend-copy-20260530 中重新设计(允许大规模迁移复用,但是技术栈不变)」)
in_scope:
  - "**wave.1 后端事件基座**:forge-agentd 新 events.rs(EventBus:append-only JSONL data/agent-events/{sessionId}.jsonl + 每会话环缓冲 4096 + seq 会话内单调 + emit/emit_ephemeral 二分[ephemeral 不落盘不回放]);sessions 持久化(data/agent-sessions/sessions.json:GET/POST/PATCH[title/pinned/folderId]/DELETE + POST :fork[克隆事件流,标题「分支 · 」] + POST :revert{messageId,mode} 截断事件流);chat-folders CRUD;SSE GET /api/forge/sessions/{id}/events/stream?fromSeq=(id/event/data 帧 + keep-alive + 超窗合成 stream.gap);GET /api/forge/design-snapshot?sessionId=(sessions/activeSession/events/todos/run/models/latestSeq/chatFolders 聚合);host forgeProxy SSE 透传(stream 端点免 15s 超时);scripts/f7-w1-events-smoke.ps1"
  - "**wave.2 turn 执行事件化**:agentd agent.rs——POST /api/forge/sessions/{id}/ask:execute{userInput,mode}(创建 run[trigger=composer_chat];事件序列 composer.user.message→agent.started→[agent.tool.invoked/completed|failed]→agent.message→agent.completed|failed|cancelled;复用 llm.rs mock|deepseek 工具循环;mode 语义:ask=禁工具纯对话/plan=禁写工具/debug=build+debug 提示/multitask=保留 F3 swarm 确定性模板链);GET /api/forge/runs/{id} + POST :cancel(内存 RunControl);todos REST(GET /sessions/{id}/todos + POST /todos + PATCH /todos/{id})+ todo.* 事件;首条消息自动命名会话标题(48 字)"
  - "**wave.3 前端主题与壳**:theme.css CSS 变量全集(参考 token 明暗两表逐值);themeStore(applyPalette TS 逐行移植:contrast 混色基比 0.42/0.62/0.78、accent_bg/ring alpha 明暗差异、translucent 双 alpha 0xCC/0xEE;10 预设;auto/light/dark;UI/代码字号、字体族;localStorage 持久化);壳三栏(侧栏/对话列/主区+Inspector)可拖可折宽 clamp 持久化;TitleBar(菜单+搜索胶囊+窗口钮)、Sidebar(搜索/New Agent/置顶/分组[文件夹+workspace,12 条上限+More]/状态点/相对时间/hover 动作/双击重命名/底部用户卡)、StatusBar(连接/provider/会话进度/tokens);overlayStore(Esc 全关互斥)+命令面板(Ctrl+K)+toast 体系(右下 2.8s 类型色条);EditorView 嵌入为壳内编辑器视图(游戏原生内部零改动,ChatDock 移除由对话列承接);旧 Home/Agent/Automations/Customize/RightPanel/mock.ts 下线"
  - "**wave.4 聊天体验**:chatStore(applyEvent 全类型/去重[id 或 seq+type+runId+payload]/user-before-assistant 校正/乐观回显 local-*);SSE 客户端(fromSeq 续传/退避 500ms×1.7 封顶 10s/stream.gap→重拉 snapshot);build_timeline TS 移植(连续工具段聚合:Edit 按文件去重/Explore 按路径去重/Search·Command 计数/+added-removed 估算/「· n 失败」/尾部运行中「正在{动词}…」脉冲;milestone 断段[write_todos/subagent];最终回答永不折叠;思考折叠一行「思考 · 摘录 · N 字」;中文动词表);流式 caret;UserMessageCard(通栏卡/时间/mode·model chip/内联编辑→:revert before+重发);AssistantMessage(「月」方块/状态点/模型 label/时间);SubagentRow+Overlay;Composer 全量(TodoStrip[进度条+执行计划+打开看板]/自适应高[44 列估行 clamp 68–200]/五模式+三 AgentKind 支持矩阵/技能 chips/模型菜单/联网开关双写/发送-abort 状态机/Enter 发送);markdown 逐行解析器 TS 化(标题/引用/列表/表格简化/围栏代码/行内剥除;流式 200 块·非流式 800 块);会话 fork/删除/重命名接线"
  - "**wave.5 workbench 与设置**:workbench tab 体系(tabbar 34px/激活顶 2px accent/dirty 点/关闭;内建 tab:plan[助手末消息 md+To-dos 表+开始 Build]/todo[四列看板 Backlog/Running/Review/Done]/diff[适配 F2 proposals:统一 diff 双行号 gutter+符号列+shiki 高亮,应用/拒绝接线]/editor[游戏编辑器]);底部面板(Agent Logs 事件树 120 条/Output;默认高 260 clamp 120–520,Ctrl+J);Inspector 工作区树(agentd 最小 GET /api/forge/workspace/tree?path= 单层懒加载,只读项目根);设置体系(全屏覆盖+左 240 导航+控件族:外观页[主题三卡/10 预设/强调色·背景·前景 hex/UI·代码字体/字号 stepper 11–18·10–20/对比度 slider/差异标记]+Agent 页[Ctrl+Enter 发送/联网开关/permission-mode]+模型页[deepseek key 经 F5 keystore 面配置+gen backends 平移]+技能页[既有平移]+关于);desktop 冒烟三场景;全量回归+close-out"
out_of_scope:
  - auth 登录门/register/JWT/用户体系(本地单机工具;参考自带 skip-login 调试直通)→ RD-F7-001
  - plugins 市场/hooks 浏览器/memories 系统/真 PTY 终端/docforge 内嵌/系统托盘/完成提示音 → RD-F7-002
  - WS 网关层(SSE=参考等价回退腿,单通道已足);多 provider 渠道全家桶(deepseek+mock 先行,渠道 seam 留档)→ RD-F7-003
  - agent 侧 checkpoint 工作区文件快照 rewind(引擎侧 F1 场景 checkpoint 已在;agent 文件快照)→ RD-F7-004
  - 游戏原生功能内部任何改动(Viewport/Assets/NodeGraph/PIE/Console/Metrics/打包/gen 生成链)
  - 代码高亮 syntect 逐字对齐(shiki 等价替代,色系对齐;参考自研 markdown 解析器行为照抄)
deliverables:
  - id: D-F7-1
    name: 事件基座(EventBus/SSE/design-snapshot/会话持久化+fork/revert/host 透传/f7-w1 冒烟)
    evidence: cargo test + evidence/f7-w1-*.log
  - id: D-F7-2
    name: turn 事件化(ask:execute 五模式/runs cancel/todos REST/事件序列/f7-w2 冒烟)
    evidence: cargo test + evidence/f7-w2-*.log
  - id: D-F7-3
    name: 前端主题系统+壳(themeStore 派色/三栏/TitleBar/Sidebar/StatusBar/命令面板/toast/编辑器嵌入/旧面下线)
    evidence: client vitest + desktop 冒烟截图
  - id: D-F7-4
    name: 聊天体验(chatStore/SSE 客户端/build_timeline/消息卡/Composer 全量/markdown)
    evidence: client vitest + desktop 端到端冒烟
  - id: D-F7-5
    name: workbench+设置(tabs/底部面板/Inspector/设置体系)+全量回归+close-out
    evidence: evidence/f7-w5-* + 回归输出
acceptance_gates:
  - id: G-F7-1
    name: 事件基座门
    check: cargo test(agentd:seq 会话内单调/append-only 回放无重无漏/环缓冲窗口/ephemeral 不落盘;会话 CRUD/fork 克隆事件流/revert 截断语义);f7-w1 冒烟实测:SSE 收事件 seq 递增+fromSeq 续传+超窗收 stream.gap;经 host(3080)代理连流≥30s 不断(旧 15s 超时已绕);design-snapshot 字段齐(sessions/activeSession/events/todos/run/models/latestSeq/chatFolders)
  - id: G-F7-2
    name: turn 门
    check: cargo test(mock provider 全事件序列顺序断言 composer.user.message→agent.started→[tool.*]→agent.message→agent.completed;ask 模式零 tool.invoked;plan 模式写工具拒绝如实;cancel→终态;todos REST+todo.* 事件;首消息自动命名);f7-w2 冒烟全链(POST ask:execute→SSE 实测序列齐→run 终态);deepseek 腿有 key 实测/无 key 如实 mock 标注不充绿
  - id: G-F7-3
    name: 壳门
    check: client vitest(themeStore 派色固定输入对照参考算法期望值[contrast 混色/accent_bg·ring alpha/translucent];预设切换;overlay 互斥 Esc 全关;面板 clamp 持久化);pnpm -r typecheck 全绿;desktop 冒烟:三栏布局渲染+明暗切换 CSS 变量实测变化+编辑器视图嵌入存活(ViewportCanvas 出帧或如实降级)
  - id: G-F7-4
    name: 聊天门
    check: client vitest(build_timeline 全规则[段聚合/目标去重/动词表/里程碑断段/最终回答不折叠/+/-估算];chatStore[事件应用/去重/乐观回显/revert];SSE 客户端[续传/退避/gap 重拉 snapshot]);desktop 端到端冒烟:mock provider 发消息→SSE 驱动 UI 断言(用户卡+助手卡+工具段+终态点);编辑重发链(:revert 调用断言)
  - id: G-F7-5
    name: 收官门
    check: 设置写回实测(主题切换 CSS 变量变化/技能禁用写回回归不破 F3);workbench tabs 渲染断言;全量回归 cargo test --workspace / pnpm -r typecheck+test / go build+test 全绿;desktop 三场景冒烟零孤儿进程;契约 §6 close-out 终审
guardrails:
  - 诚实优先:mock/deepseek 腿如实标注不充绿;数字必须来自命令输出;契约 §6 只追加
  - 技术栈不变红线:React 18+Tailwind+zustand+vite / Node host / Rust agentd / Electron 壳;禁引入新框架(库级依赖 lucide-react/shiki 允许,本契约登记)
  - 游戏原生零改动:仅壳嵌入接线;Viewport/Assets/NodeGraph/PIE/Console/Metrics/打包/gen 内部逻辑不动
  - 视觉保真:token 色值/尺寸/圆角/字号梯度对照参考实测值;派色算法逐行移植;不可对齐处如实留档
  - 参考仓只读:I:\agent-debug-frontend-backend-copy-20260530 保持 0-byte 修改
  - 密钥红线(R-5 继承):LLM/gen key 只进 Authorization 头,永不进日志/事件/工具返回/错误消息
---

# F7 契约:Agent 前端重设计

## 1. 目标与双门状态

将 client 前端所有 agent 部分(除游戏原生功能外)严格按 I:\agent-debug-frontend-backend-copy-20260530(Moonlit Agent IDE)重新设计:事件驱动架构(append-only 事件日志 + SSE + design-snapshot 引导),前端 React 全量重做(主题系统/三栏壳/聊天时间线/Composer/workbench tabs/设置体系),技术栈不变。status=active;implementation_status=unlocked(用户 /goal 原文开工指令留痕,见 front matter)。

## 2. 波次

- wave.1 → G-F7-1(后端事件基座)。
- wave.2 → G-F7-2(turn 执行事件化)。
- wave.3 → G-F7-3(前端主题与壳)。
- wave.4 → G-F7-4(聊天体验)。
- wave.5 → G-F7-5(workbench 与设置 + 全量回归 + close-out)。

## 3. 架构决策(立项裁决)

- **D-F7-A(前后端协同迁移)**:参考前端是事件驱动架构(design-snapshot 引导 + SSE 增量 + 会话 fork/revert),纯前端适配现有单轮 llm/chat 只能是空壳。参考后端与本仓 forge-agentd 同为 Rust axum,语义级移植(不 fork 代码,同 D-015 借鉴自研原则):EventBus/会话持久化/SSE/design-snapshot/turn 事件化进 forge-agentd;WS 网关层不迁(SSE 即参考等价回退腿)。
- **D-F7-B(Agent workbench 主壳)**:新壳=侧栏(会话)+对话列+中央 workbench tab+底部面板+Inspector+titlebar/statusbar;游戏编辑器(EditorView)降级为壳内编辑器视图/内建 tab,游戏原生区内部逻辑零改动,仅壳嵌入接线;ChatDock 从编辑器移除,agent 对话统一由对话列承接。
- **D-F7-C(功能裁剪)**:核心全量(壳/会话/聊天时间线/Composer/主题/设置/命令面板/toast)+ 既有面适配(models→F5 keystore+deepseek;diff→F2 proposals;swarm→F3;skills→F3);auth 登录/plugins 市场/hooks/memories/真 PTY/docforge/WS 网关 defer RD-F7-001~003。
- **D-F7-D(旧面下线)**:Home/Agent/Automations/Customize 旧视图、RightPanel 静态四页(含 FilesView FILE_TREE 空数组崩溃隐患)、mock.ts 假数据体系、旧 SettingsView 全部下线,由新壳取代;skills/generation 两真实功能平移进新设置体系。
- **D-F7-E(数据目录)**:agent 事件日志 data/agent-events/{sessionId}.jsonl(append-only);会话 data/agent-sessions/sessions.json(读-改-写,同 gen-backends.json 纪律);两者入 .gitignore 检查面,不含密钥。

## 4. Deferred 处置

- **RD-F7-001(auth 登录门)**:参考 auth/register/login/JWT/登录卡全套;本地单机工具无多用户语义,参考自身亦有 skip-login 直通。回填条件:多用户/云端部署需求出现。
- **RD-F7-002(plugins 市场/hooks 浏览器/memories/真 PTY 终端/docforge/托盘/提示音)**:参考功能面,本仓无对应后端概念且非 agent 对话核心。回填条件:逐项需求驱动立项。
- **RD-F7-003(WS 网关+多 provider 渠道)**:参考 WS 为网关封装(SSE 等价回退腿已落地);渠道框架(openai/anthropic/google 等 11 家)留 seam 不落地。回填条件:真实多 provider 需求。
- **RD-F7-004(agent 侧 checkpoint 文件快照 rewind)**:参考 checkpoints 快照工作区文件;本仓 F1 已有引擎场景 checkpoint(MCP 面)。回填条件:agent 改文件链落地后按需。

## 5. 修订

- 2026-08-18 立项:F6 closed 后用户 /goal 开工。三路并行勘探实测留档:①参考前端(GPUI/Rust,Moonlit Agent IDE)——布局/会话/聊天时间线/Composer/设置九页/主题派色算法/浮层体系/状态数据流全量剖析,仓内无旧 React 代码(GPUI 版即唯一前端),迁移=设计+纯函数逻辑移植为 TS;②现状 client——AgentView/Composer/SubagentList/SearchPalette/RightPanel 全 mock 零后端,唯一真实通路=EditorView ChatDock 单轮 llm/chat,无会话/消息后端模型,无 SSE/WS 全靠轮询,react-router/radix/xterm 已装未用;③参考后端——Rust axum 与本仓 agentd 同栈,全量路由表+SSE 事件模型(事件 wire{ id,sessionId,seq,type,ts,source,correlationId,channel,payload })+ design-snapshot 聚合字段穷举。现状 agentd 面实测:单 Router /api/forge/*(health/sessions/mcp/llm/playtest/pack/proposals/skills/subagents/gen/swarm),llm.rs 单轮工具循环(mock|deepseek,MAX_ITERS=16);host forgeProxy 前缀透传(pipe 流式,15s 超时对 SSE 需开口)。

## 6. Close-out(只追加区)

<!-- 只追加。禁止预填 PASS;每波验收后按五块模板追加:独立断言全绿清单/波聚合门实测输出/验收命令逐字输出/门序与 no-go 登记/签署块 -->

### wave.1 验收记录(2026-08-18,后端事件基座)→ G-F7-1 PASS

- 交付:crates/forge-agentd/src/events.rs(EventBus:wire{id/sessionId/seq/type/ts/source/correlationId/channel/payload},seq 会话内单调,环缓冲默认 4096 可 env FORGE_AGENTD_EVENT_BUFFER 调,append-only JSONL data/agent-events/{sid}.jsonl 启动重建,emit/emit_ephemeral 二分,replay/gap,每会话 broadcast,fork_events/truncate/purge)、sessions.rs(DebugSession/ChatFolder 模型+sessions.json/chat-folders.json 读-改-写 Mutex 原子写;REST 全量:CRUD/fork/revert/folders 级联清 folderId;F0 恒[] stub 下线)、sse.rs(先订阅后回放消窗+live seq 去重;超窗合成 stream.gap;Lagged→subscriber-lagged;keep-alive 15s)、snapshot.rs(八字段聚合;models availability 复用 llm key 判定)、llm.rs(resolve_deepseek_key 抽出,R-5 行为不变)、main.rs(接线+6 条 F7 路由级测试替 stub 测试)、host forgeProxy(前缀+sessions/chat-folders/design-snapshot;events/stream setTimeout(0) 豁免;纯函数导出)、scripts/f7-w1-events-smoke.ps1(5 腿 38 断言)。
- 差异留痕:①路由 {id}:fork/:revert → {id}/fork、{id}/revert(axum 0.8 matchit 不支持段内冒号参数,动作语义不变);②ephemeral 口径严于参考——replay 基于持久化序列,ephemeral 只 live+耗 seq,不进环缓冲不落盘;③SSE 比参考多「先订阅后回放+live 去重」消丢事件窗;④revert 对不存在 messageId 返 404 EVENT_NOT_FOUND(参考静默 no-op,本仓取诚实口径);⑤snapshot 只落契约八字段(todos/run 占位,wave.2 填真)。
- 测试数字:cargo test -p forge-agentd **74/74**(events 8+sessions 4+路由级 6 新增);cargo test --workspace **198/198**;pnpm --filter @forge/host test **20/20**(forgeProxy 遮蔽断言反转+纯函数边界;http.test F0 echo 断言随 stub 退役移除,插件代码未动);pnpm --filter @forge/host typecheck 全绿。
- 冒烟(evidence/f7-w1-events-smoke-20260818T144030Z.log,exit 0,**pass=38 fail=0**):腿1 直连 CRUD/fork/revert 12/12(fork 克隆 seq 1-4 单调;revert before 后 latestSeq=2);腿2 SSE 直连 6/6(seq 递增/id·event·data 帧齐/续传零重/live 推送);腿3 gap(buffer=4)3/3(首帧 stream.gap replay-window-exceeded,窗口 seq4..7);腿4 host 代理 9/9(CRUD 全透传;**SSE 经 3080 保持 22s 不断收 keep-alive**——15s 旧超时豁免生效);腿5 design-snapshot 经 3080 8/8(字段穷举齐;响应面无 sk- 串;空 sessionId 三态)。冒烟后实测零孤儿进程。
- 门序:no-go/SKIP 无。**G-F7-1 PASS**。
- 签署:Assisted-by: TraeAgent:Kimi-K3;影响范围:agentd 三新模块+main/llm 接线、host forgeProxy+两测试文件、冒烟脚本新增;验证方式:上述命令逐字输出+证据日志。

### wave.2 验收记录(2026-08-18,turn 执行事件化)→ G-F7-2 PASS

- 交付:crates/forge-agentd/src/agent.rs(新,~1540 行:execute_turn 五模式 + RunRegistry/CancelToken + TodoStore[todos.json 读-改-写] + ask:execute/runs/todos REST + 首消息 48 字自动命名);llm.rs 重构(run_tool_loop 五注入点[step/executor/sink/forbidden/cancelled],chat handler 行为不变 mock 文案逐字保留,RD-F1-002 不破);main.rs(6 路由+AppState runs/todos);snapshot.rs(todos/run 填真);host forgeProxy(前缀+runs/todos;isStreamPath→isLongLivedPath,ask:execute 并列豁免 15s);scripts/f7-w2-turn-smoke.ps1(八腿 43 断言)。
- 模式语义:ask=tools 空纯对话;build=原循环;debug=build+DEBUG_PROMPT_SUFFIX;plan=WRITE_TOOLS 45 名显式集合(按 KNOWN_TOOLS 75 全量核对,单测守门)只读工具集+TOOL_FORBIDDEN 双门;multitask=F3 模板服务端化(碰撞|collider → entity_list → 进程内 swarm 协调器,非 HTTP)。
- 差异留痕:①multitask 生产 executor 成败口径 !isError(F3 HTTP 面另解析 content text 内 error 字段,轻微差异);②cancel 仅工具循环每迭代开头检查,multitask 快速确定性不查;③部分分片失败 completed+计数不遮蔽,全失败 failed;④deepseek 工具面拉取失败在 llm/chat=502,在 agent 语义=HTTP 200+run failed;⑤ask:execute 静态段含冒号在 matchit 0.8 正常(wave.1 {id}:fork 参数形态才不支持)。
- 测试数字:cargo test -p forge-agentd **91/91**(+17);cargo test --workspace **251/251**;pnpm --filter @forge/host test **21/21**;pnpm --filter @forge/client test **82/82**(未受影响);typecheck 全绿。
- 冒烟(evidence/f7-w2-turn-smoke-20260818T154050Z.log,exit 0,**pass=43 fail=0 skip=0**):腿1 build(mock)13/13(SSE 序列齐且序对/run completed/title=输入前 48 字/activeRunId 清);腿2 ask 3/3(零 tool.invoked);腿3 multitask 8/8(真 engine 链 3 实体+swarm.execute invoked/completed;未命中 run failed「模板未命中」);腿4 todos 8/8(todo.created/updated 载荷+snapshot 填真+400/404 矩阵);腿5 cancel 2/2(非 running ok:false+404);腿6 revert 4/4(截断后 latestSeq=2);腿7 **deepseek live 实测**(availability=available:run completed+provider=deepseek+agent.usage 到达)。零孤儿进程。
- 门序:no-go/SKIP 无。**G-F7-2 PASS**。
- 签署:Assisted-by: TraeAgent:Kimi-K3;影响范围:agent.rs 新增+llm.rs 重构+main/sessions/snapshot 接线、host forgeProxy+测试、冒烟脚本新增;验证方式:上述命令逐字输出+证据日志。

### wave.3 验收记录(2026-08-18,前端主题与壳)→ G-F7-3 PASS

- 交付:packages/client/src/styles/theme.css(moonlit 明暗两表静态默认,与派色输出逐值一致);lib/themeStore.ts(applyPalette 逐行移植参考 appearance.rs:contrast 混色基比 0.42/0.62/0.78、accent_bg/ring alpha 亮 0x14·0x47/暗 0x24·0x6B、translucent 双 alpha 0xCC/0xEE、截断语义 rust as u8→Math.trunc;10 预设逐值;auto/light/dark + matchMedia 监听;字号 clamp 11–18/10–20;localStorage forge:appearance 即改即存);lib/sessionStore.ts(真实 REST:CRUD/pin/folderId 三态/fork/文件夹,乐观更新+失败回滚+toast)、workbenchStore.ts(tabs+三栏宽 clamp 200–360/300–560/240–420+折叠,forge:paneSizes 持久化)、overlayStore.ts(互斥+closeAll)、toastStore.ts(2.8s)、commands.ts(命令注册表 agent/导航/视图);components/shell/(Shell 三栏+9px 分隔条 11×24 折叠胶囊拖拽、TitleBar 36[「铸」logo+四菜单玻璃下拉+520 搜索胶囊+窗口三钮]、Sidebar[搜索/New Agent/PINNED/CHAT FOLDERS 内联建夹/12 条上限 More(N)/状态点/相对时间/hover 三钮/双击重命名/底部用户卡]、StatusBar 26[health 5s 轮询+provider 段+会话进度]、ChatColumn[38px 头+fork+more;消息区/Composer 诚实占位 wave.4]、Workbench[tabbar 34 顶 2px accent+编辑器 tab 嵌 EditorView+空态卡]、Inspector[wave.5 占位]、overlays/[CommandPalette Ctrl+K 600px 15% 挂顶/ToastStack 右下 46/20/Modals])。
- 旧面下线(D-F7-D):views/Home/Agent/Automations/Customize/Settings、components/RightPanel+panel/整目录+SearchPalette+Composer+旧 Sidebar/TitleBar、lib/mock.ts+store.ts+forgeSettingsTabs.ts 删除;EditorView 移除 ChatDock 与 chat 开关钮(游戏原生其余零改动),editorStore chat 子集退役(chatPrefill 三件套保留);bridge.ts MOCK_FORGE_API 自 mock.ts 迁入;forgeApi +apiPatch/apiDelete;forgeMock 扩 query 剥离+最长前缀匹配+text() 同构。
- 差异留痕:①参考 moonlit-uikit crate(Tokens 默认的 sage/danger/warn/info 与 dot_done/idle/blocked/queued 色值、字体常量)未随拷贝仓提供(path dep 缺失)——语义色为本仓派生同族值(theme.css 头注),apply_palette 派生面与参考逐值一致;②参考 appearance.rs 的 hover/active 四个 rgba 字面量字节序与 rgba_from_hex 的 RRGGBBAA 主张不一致(疑参考笔误),按任务书语义值移植(亮 rgba(42,39,36,0.04/0.07)、暗白 0x0d/0x14);③workspace 分组依赖会话 workspaceRoot 字段(本仓未落地)→ 仅 CHAT FOLDERS 分组+单一「会话」组;④logo 用「铸」(RurixForge)替参考「月」;⑤tailwind 新增语义 token 映射 CSS 变量,旧 ink/muted/line/panel token 保留——EditorView(游戏原生)视觉与新壳有缝,wave.5 再评统一;⑥品牌/命令文案按本仓(关于 RurixForge);⑦TitleBar 菜单玻璃底用 --menu-glass(参考 menubar_glass_bg 0xFFFFFF88/0x23222088 近似);⑧f3-w4-settings 冒烟场景随旧 Settings 视图退役(main.cjs 注释留痕),wave.5 设置体系重建后换新场景。
- 测试数字:pnpm --filter @forge/client test **104/104**(15 文件;新增 themeStore 13[派色固定输入对照/alpha 明暗/contrast 两极端/translucent/预设/matchMedia mock/持久化读写]+sessionStore 9[CRUD/乐观/回滚/fork]+overlays 6+shell 7[三栏/折叠持久化/clamp/拖拽/编辑器 tab 嵌入/主题实测/会话流]+commandPalette 5;editorView/editorStore 去 chat 用例);pnpm --filter @forge/client typecheck 全绿;pnpm -r typecheck 全绿;pnpm --filter @forge/host test **21/21**(未受影响);cargo test -p forge-agentd **91/91**(未受影响)。
- 冒烟(evidence/f7-w3-shell-smoke-20260818T170255Z.log,exit 0;截图 apps/desktop/evidence/desktop-smoke-shell-2026-08-18T17-03-22-620Z.png,126218 bytes):三栏+titlebar+statusbar+分隔条 9 锚点齐;setMode(light) data-theme=light,--accent=#C96442/--bg=#FAF9F5(computed rgb(201,100,66)/rgb(250,249,245));setMode(dark) data-theme=dark,--accent=rgb(226,136,106)/--bg=rgb(28,27,24)(=#E2886A/#1C1B18);开编辑器 tab → EditorView 骨架(Hierarchy)+ViewportCanvas **真出帧**(canvas:true,degrade:null,非降级充绿);New Agent 经真后端建行 sess_1787072600487_bd3321bf,侧栏行实测出现。冒烟后零孤儿进程。
- 门序:no-go/SKIP 无。**G-F7-3 PASS**。
- 签署:Assisted-by: TraeAgent:Kimi-K3;影响范围:client 新壳+主题+store 层新增、旧面删除、EditorView/editorStore 嵌改、desktop main.cjs 场景注册表+smoke.mjs 默认场景、测试新增/改写、冒烟脚本新增;验证方式:上述命令逐字输出+证据日志+截图。

### wave.4 验收记录(2026-08-18,聊天体验)→ G-F7-4 PASS

- 交付:packages/client/src/lib/sseClient.ts(fetch+ReadableStream 自研 SSE:fromSeq 续传/退避 500ms×1.7 封顶 10s/stream.gap→重拉 snapshot/close 幂等)、chatStore.ts(applyEvent 全类型/签名去重 cap4096/乐观 local-* 替换/user-before-assistant 校正/selectSession 快照回放+订阅/sendMessage/cancelRun/editAndResend/pickModel)、timeline.ts(build_timeline 全规则 TS 移植:段聚合/同类并组/Edit·Explore 目标去重/+-LCS 估算/「· n 失败」/运行中「正在{动词}…」/milestone 断段/中文动词表按本仓 KNOWN_TOOLS 重写)、markdown.ts(逐行解析器,流式 200·非流式 800 块)、inputHeight.ts(44 列估行 clamp 68–200/编辑 56–200);components/chat/(MarkdownFlat/MessageList 吸底/UserMessageCard 内联编辑→revert+resend/AssistantMessage 铸方块+状态点+时间线/ActivitySegment/SubagentRow+Overlay/StreamCaret/Composer 全量[TodoStrip/五模式/技能 chips 前缀/模型菜单 needs-key 置灰/发送-abort 状态机]);ChatColumn 占位全替换;后端小补:sessions PATCH+selectedModelId 三态、agent.rs provider_for_session(mock 强制)。
- 差异留痕:①本仓事件面无 reasoning/subagent/token delta/web 搜索——思考折叠/SubagentRow 组件就绪有单测,自然不触发不伪造;②工具事件无 result 载荷→完成行无结果摘要,展开细节=args+「完成 · Nms」/错误;③动词表按本仓工具面重写,未知 MCP 名回退「server / tool」;④+/-估算行级 LCS(参考 Myers 同语义);⑤代码块未接高亮库(wave.5 再评),AgentKind 三节菜单未落(后端恒 coding 全模式可用),联网开关/执行计划/打开看板留 wave.5;⑥**关键修复**:revert 截断使服务端 seq 回退,既有 SSE live 去重(seq>已发上限)会滤掉后续新事件——editAndResend revert 后显式 resync(参考 revert_to 同语义),注释留痕。
- 测试数字:pnpm --filter @forge/client test **169/169**(21 文件,+65:sseClient 9/timeline 14/markdown 7/chatStore 14/composer 8/messages 13);pnpm -r typecheck 全绿;pnpm --filter @forge/host test **21/21**;cargo test -p forge-agentd **93/93**(+2)。
- 冒烟(evidence/f7-w4-chat-smoke-20260818T183340Z.log,exit 0;截图 apps/desktop/evidence/desktop-smoke-chat-2026-08-18T18-35-37-516Z.png 97340 bytes 已目检):腿1 build(mock 强制,模型菜单选 Mock provider)→用户卡+assistant 铸方块+终态点+mock 回文断言全中;腿2 multitask DOM 切换+「给场景加碰撞体 collider」真 engine 链→工具段「执行 1 条命令」+「集群执行」动词行+completed;腿3 编辑重发链→消息数回退后重增(2|2→1|1),revert 生效残留消失。**w3 冒烟复跑 PASS**(ViewportCanvas 真出帧)。零孤儿进程。
- 门序:no-go/SKIP 无。**G-F7-4 PASS**。
- 签署:Assisted-by: TraeAgent:Kimi-K3;影响范围:client chat 全层新增、ChatColumn 升级、sessions/agent.rs 小补、desktop chat 场景+冒烟脚本新增;验证方式:上述命令逐字输出+证据日志+截图。

### wave.5 验收记录(2026-08-18,workbench 与设置 + 全量回归)→ G-F7-5 PASS

- 交付:agentd workspace.rs 新模块(GET /api/forge/workspace/tree:confined[canonicalize+starts_with,PATH_OUTSIDE_ROOT 400/PATH_NOT_FOUND 404]/目录优先+名称小写排序/单层超 500 截断 truncated:true 如实/hidden=.前缀标记;env FORGE_AGENTD_WORKSPACE_ROOT 测试隔离)+ llm.rs POST /api/forge/llm/key(写 keystore["deepseek"] 复用 gend set_key 面,响应仅 {ok,configured},空 key 400 EMPTY_KEY,R-5 不回显);host forgeProxy +'/api/forge/workspace' 前缀(llm/key 落既有 llm 前缀内);client——settingsStore.ts(五页导航 forge:settingsPage 持久化 + forge:submitCtrl)+ components/settings/(controls.tsx 控件族[SetCard/SetRow/SetToggle 32×18 胶囊 sage/SetSelect/SetStepper 26×26/SetSlider 4px 轨 12px knob/SetInput/SmBtn 按参考 settings.rs 尺寸] + SettingsOverlay[全屏 top-36,左 240 导航+内容 max-w 720/外观 900] + AppearancePage[主题三卡 72px 缩略窗/10 预设 Aa 色板下拉/深浅两块 hex 行 18px 色板+116px 输入非法 toast 不应用/半透明 toggle/对比度 slider 0–100 读数/字体输入/字号 stepper 11–18·10–20/差异标记 seg] + AgentPage[Ctrl+Enter + 执行权限如实静态行] + ModelsPage[deepseek 渠道卡 availability 实测+key 配置展开;gen backends 平移] + SkillsPage[F3 面平移 toggle 全量写回] + AboutPage[版本/host·agentd health 实测]);workbench tabs(PlanTab[最近 plan 模式 assistant 终稿 md+To-dos 表+开始 Build=composer 预填 build 真实行为]/TodoTab[四列看板只读]/ProposalsTab[F2 提案列表+impact pretty JSON 展开+pending 批准/拒绝 PATCH 接线]);底部面板 BottomPanel(Agent Logs 事件树末 120 条可折叠 pretty JSON 前 40 行/Output 派生行末 200/Metrics 本地派生卡;默认 260 clamp 120–520 顶 4px 拖拽;Ctrl+J+statusbar 钮+命令面板接线;forge:bottomPanel 持久化);chatStore eventsRing(cap 200 FIFO,clearEventsRing);Inspector 工作区树填真(搜索本地过滤+懒加载 chevron 展开 pl=12+depth×12+隐藏降档+文件点击 toast「文件预览留待后续」);Composer 收尾(TodoStrip「打开看板 ↗」/Ctrl+Enter 消费/prefill seam);入口三处(侧栏齿轮/命令面板 settings.open/TitleBar File 菜单);旧设置占位 modal 下线;desktop main.cjs +settings/workbench 双场景;scripts/f7-w5-workbench-smoke.ps1 + f7-w5-legacy-scenario.ps1。
- 差异留痕:①**提案 tab 适配**——本仓 proposals=F2 治理确认单(实测 wire{id,kind,summary,impact,status,createdBy},无代码 diff 文本,亦无 createdAt——按实测面渲染不伪造时间),双行号 gutter diff 渲染器不造无数据之源,留 RD-F7-004;②**Plan tab 语义**——本仓无 Plan 实体,展示最近 plan 模式 turn 的 assistant 终稿 md;参考的 breadcrumb/模型菜单/find 栏/DAG/timeline/diff 历史无数据面不落;③**底部面板裁剪**——Terminal/Problems tab 不落(无 PTY/无诊断面,RD-F7-002),不加占位空 tab;Metrics 卡为本地派生四卡(tokens/tool calls/todo 完成比/Run 状态);参考 maximize 空操作钮不落;④**设置九页→五页**——通用/套餐/自动补全/工具 MCP/记忆无后端语义面不落空页(RD-F7-001/002);permission-mode 无后端面,如实静态「bypass · 工具全部自动执行」;预设选择器为单列全局行(themeStore.applyPreset 双表整块替换,wave.3 既有语义;参考为按侧预设);参考主题 diff 预览卡/导入/复制主题不落(无主题包协议面);⑤**shiki 不引**——代码块高亮评估成本>收益(自研 MarkdownFlat 已覆盖围栏代码块;两主题色对齐+懒加载+jsdom 测试基建在收官波风险不实),如实留档不引;⑥Inspector 无 git 徽章/branch 胶囊(本仓无 git 面);⑦workspace/tree 排序=目录优先+名称小写(参考未规定,取资源管理器惯例);⑧eventsRing cap=200(Agent Logs 取末 120,Output 取末 200,一环两用);会话切换清环(快照回放重填,环随活跃会话);⑨**engine-host rpc.rs 回归修复**——resolve_scene_path 未提交前改「相对一律项目根」破坏 playtest 契约「workspace 相对或绝对」(projects/demo 双前缀 → scene_load os error 3),console-metrics 复跑确定性 502 显形;修为 projects/ 前缀走 workspace 根、其余走项目根双契约(最小改动,源码注释留痕);⑩f3-w4-settings-smoke.ps1 退役(断言面=已下线旧 SettingsView,脚本头留痕+运行即诚实报错),由 f7-w5 脚本取代。
- 测试数字:cargo test -p forge-agentd **97/97**(+4:tree 单层排序+隐藏/confined 双 400+双 404/截断 500 标记/llm key 翻转+R-5 响应面无 key 串+覆盖写);cargo test --workspace **256/256**(32 suites,0 failed);client vitest **215/215**(28 文件,+46:settingsStore 6/外观页 7/设置壳+四页 6/workbench tabs 7/bottomPanel 10/inspector 4/composerW5 4);host vitest **21/21**;protocol **5/5**;pnpm -r typecheck 全绿;go build ./... + go test ./... 绿(ok forge-gateway)。
- 冒烟(evidence/f7-w5-workbench-smoke-20260818T194332Z.log,exit 0;首跑 194230Z 因场景断言时序 FAIL 留痕,修后本跑全绿):直连自检 workspace/tree 根 43 条(目录优先,crates 在)+ path=.. 400 PATH_OUTSIDE_ROOT + llm/key 空 key 400 EMPTY_KEY;**settings 场景**(截图 apps/desktop/evidence/desktop-smoke-settings-2026-08-18T19-43-45-915Z.png 59510B 已目检):开 overlay 五页导航齐 → 预设 github 亮表 --accent 实测 rgb(9,105,218) → 模式三卡切深色实测 rgb(68,147,248) → 技能页禁用 asset-cleanup GET 复核 enabled=false → 复原 true;**workbench 场景**(截图 desktop-smoke-workbench-2026-08-18T19-43-55-497Z.png 136729B 已目检):建会话 → 两待办(一完成)→ todo 看板 Backlog/Done 分列实测 → 提案 pending 行「批准」→ approved → 底部面板 Agent Logs 4 行/Output 4 行派生/Metrics Sessions 1/2 → Inspector 仓根 crates 展开见 forge-agentd。**矩阵复跑**:f7-w1 pass=38 fail=0;f7-w2 pass=43 fail=0 skip=0;f7-w3 PASS(ViewportCanvas 真出帧);f7-w4 PASS(三腿全绿);f7-w5 PASS。**既有场景复跑**:editor(f1-w2 脚本 PASS,presenter selftest 含)/assets(22 资产条目)/nodegraph(4 节点)/gen(全链 PASS)/console-metrics(修复后 PASS)——零 regress。冒烟后零孤儿进程。
- 门序:no-go/SKIP 无。**G-F7-5 PASS**。五门全绿,F7 收官(status flip active→closed 随本记录)。
- 签署:Assisted-by: TraeAgent:Kimi-K3;影响范围:agentd workspace.rs 新增+main/llm 接线、host forgeProxy+测试、client settings/workbench/bottomPanel/Inspector/Composer/chatStore/workbenchStore/App/Shell/StatusBar/TitleBar/commands/Modals、desktop main.cjs 双场景、engine-host rpc.rs 回归修复、冒烟脚本×2、f3-w4 退役留痕、测试六文件新增;验证方式:上述命令逐字输出+证据日志+截图目检。
