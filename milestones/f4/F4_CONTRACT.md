---
contract: F4
title: F4 code-forge 交互逻辑(rx 脚本 + 节点图)
status: closed
implementation_status: unlocked
active_scope: wave.1
version: 0.1
date: 2026-08-17
timebox: 会话制推进,做不完转 deferred
rfc_required: []
upstream_docs:
  - 03_ENGINE_LAYER.md (§3.3 drain_contacts 规范序 / §6 物理确定性)
  - 04_AGENT_BACKEND.md (§3 composer 模式 / §6 logic-programmer profile)
  - 05_MCP_PROJECTS.md (§4 code-forge 九工具 / §6 playtest)
  - 06_SKILLS_LIBRARY.md (§3 logic-blueprint-gen / code-rx-migration 行)
  - 07_FRONTEND_IDE.md (§1 G 区 NodeGraph 同位页签 / §5 Chat)
  - 09_ENTITY_SCENE_MODEL.md (§3 组件注册表)
  - 10_INTERACTION_LOGIC.md (全量:§3 事件模型 / §4 .rxgraph 格式 / §5 NodeGraph 面板 / §6 agent 生成路径)
  - 11_API_CONTRACTS.md (§2.2 /api/forge/code/* 路由 / §4 错误码)
  - 13_ROADMAP.md (F4)
  - milestones/f3/F3_CONTRACT.md
implementation_unlock:
  required_all:
    - F3 验收门全绿(wave.1~wave.5 §6 已录,status=closed)
    - 用户开工指令(2026-08-17「启动 F4 code-forge 开发,推进 rx 脚本编译与热重载」)
in_scope:
  - code-forge MCP server(crates/mcp/code-forge-mcp,server 名 code-forge;11 §2.2 /api/forge/code/* 已由 gateway 路由族承接):rx_check/rx_build/rx_run/rx_fmt/rx_test 五工具(05 §4)——子进程包上游 rx CLI / rurixc(H:\rurix target),超时 + 输出截断 + 结构化 JSON 解析(05 §4 实现注记逐字);FORGE_RX_CLI/FORGE_RURIXC env 可配,缺省 H:\rurix\target\debug\{rx,rurixc}.exe
  - `.rxgraph` schema + 节点类型注册表(10 §4.2 首发冻结子集:event.*/flow.*/entity.*/transform.*/physics.*/audio.*/var.*/call.*/debug.* 九族)+ graph_validate 校验器(schema / 值来源三态 const|node+pin|ref / 悬空输入 / 类型匹配 / 环检测:执行边禁环 + 数据边禁环 / 每图每事件至多一个入口)
  - Script 组件(09 §3 注册表扩展;10 §1 Unity 决策行:挂 .rx 模块或 .rxgraph,exposedProps dict Inspector 可编辑)
  - 图解释执行运行时(engine-host logic 模块):10 §4.3 lowering → 中间 IR → 解释执行;事件调度契约 10 §3.2 逐字(输入 → 接触规范序 → timer → update → message 队列清空;同实体多图按挂载序,跨实体按实体 id 升序)
  - 事件全线贯通:on_start/on_update/on_contact_begin|persist|end(03 §3.3 drain_contacts)/on_trigger_enter|exit/on_input/on_message/on_timer
  - call_function 互绑(10 §4.3):图 → .rx 导出函数(#[export(c)],签名经 rurixc --emit=reflection 函数注册表;纯值进/出);脚本 → 图 send_message → event.on_message
  - 热重载:Script 组件 module/graphRef 变更时 PIE 内重挂载(on_start 重发,黑板变量重置;语义见 §3 D-F4-B)
  - logic-blueprint-gen skill seam 摘除(06 §3 行:需求 → 图 JSON → graph_validate → 挂载 Script → playtest 验证);code-rx-migration skill seam 摘除(code_symbol_search → code_structured_edit → rx_check → rx_test)
  - code_* LSP 三工具(code_symbol_search/code_references/code_structured_edit;rurixc --tooling-server 常驻会话,05 §4 注记)
  - NodeGraph 面板(07 §1 G 区 Viewport 同位页签):网格画布 + 节点/连线渲染 + 常量内联编辑 + 保存即全图校验(错误节点红框 + Problems 汇总);人在回路定位 = 审阅/微调/改常量(10 §5),从零生成走 Chat agent
  - scripts/f4-w*-*.ps1 栈级冒烟
out_of_scope:
  - rx CLI fix|watch 子命令(上游 seam:main.rs 逐字「后续小里程碑承接」)
  - .rxgraph 编译到 .rx(10 §4.3 D-011 评估项;本里程碑解释执行)
  - playtest test_run/assert_library(SSIM 截图断言属 F6;本里程碑 PIE 注入 + component_get 断言等价,§3 D-F4-C)
  - rx_doc 工具(05 §4 表有 rx_doc;F4 不承诺,doc 站生成与引擎文档流无涉,登记 deferred)
  - GPU 侧 rx(kernel/ptx/dxil codegen;rurix-rt 渲染内核已直用,不经 code-forge)
  - 多 headless host playtest 并发(F6 test-matrix)
deferred_refs: [RD-F1-002, RD-F3-001]
deliverables:
  - id: D-F4-1
    name: code-forge server + rx 五工具(子进程包装/超时/截断/JSON 诊断)
    evidence: cargo test + scripts/f4-w1-rx-tools-smoke.ps1 输出
  - id: D-F4-2
    name: .rxgraph schema + 节点注册表 + graph_validate + Script 组件
    evidence: cargo test + scripts/f4-w2-graph-smoke.ps1 输出
  - id: D-F4-3
    name: 事件运行时全贯通 + call_function 互绑 + 热重载 + logic-blueprint-gen 端到端
    evidence: cargo test + scripts/f4-w3-logic-smoke.ps1 输出
  - id: D-F4-4
    name: NodeGraph 面板 + code_* LSP 三工具
    evidence: client vitest + desktop 冒烟截图 + scripts/f4-w4-lsp-smoke.ps1 输出
acceptance_gates:
  - id: G-F4-1
    name: rx 工具门
    check: 「.rx 脚本 rx_test 在 CI 恒绿」(13_ROADMAP F4 逐字)。机验:fixture .rx(含 #[test] 正反两例)→ MCP rx_test 全过且失败例如实报 failures;rx_check 坏文件返结构化 diagnostics(code/severity/span/suggestion);rx_fmt --check-idempotent 0;rx_build/run 正常产物 exitCode 透传;工具数断言 KNOWN_TOOLS 55→60
  - id: G-F4-2
    name: 图校验门
    check: agent 生成「触发开门」.rxgraph JSON → graph_validate PASS(schema + 注册表 + 类型);四类坏图逐类拒绝:悬空输入/类型不匹配/执行边环/数据边环(10 §5 校验逐字);每图每事件至多一个入口拒绝;Script 组件 component_set 挂载 graphRef 经引用防护(graph 文件须存在,照 F2 资产引用先例)
  - id: G-F4-3
    name: 触发开门门
    check: 「agent 生成触发开门图 → 校验 → 挂载 → playtest 注入断言通过;人工在图上改常量生效」(13_ROADMAP F4 逐字;playtest 注入以 PIE 双态 + 事件注入 + component_get 断言落地,D-F4-C)。机验:fixture 场景(门实体 + 触发区 + player tag 实体)→ 挂载图(on_trigger_enter → has_tag → rotate_tween)→ play_enter → 注入 player 进触发区 → step N 帧 → component_get 门 transform 断言旋转生效 → 改 exposedProps openSpeed 常量 → 重跑断言旋转量随常量变化;事件规范序断言(同帧 input 先于 contact 先于 timer 先于 update);热重载:改图文件 → 组件重挂载 on_start 重发
  - id: G-F4-4
    name: 前端/LSP 门
    check: NodeGraph 页签真实渲染节点/边(数据来自 graph_get 真实响应);常量内联编辑写回图文件且 graph_validate 通过;保存校验失败节点红框 + Problems 汇总;code_symbol_search/code_references/code_structured_edit 三工具栈级冒烟(rurixc --tooling-server 常驻会话,崩溃重连);client vitest 全绿;desktop 冒烟截图可见
guardrails:
  - 诚实优先:任何门不过如实报 FAIL/DEV_ENV_DEGRADE,不回写 PASS
  - 数字必须来自命令输出
  - rx 工具一律子进程包装真实 rx/rurixc,禁止自写解析器代绿;诊断 JSON 以 --error-format=json 为单一事实源
  - 图运行时事件顺序必须 10 §3.2 逐字(规范序契约,playtest 回放依赖)
  - seam skill 摘除以端到端冒烟为准,禁止文档摘除而工具面未通
  - 双状态机:status/implementation_status 严格分离;契约 §6 只追加
---

# F4 契约:code-forge 交互逻辑

## 1. 目标与双门状态

实现 13_ROADMAP F4:`.rxgraph` schema + 节点注册表 + 校验器 + 解释执行运行时;NodeGraph 面板 + call_function 互绑;接触/触发/input/timer 事件全线贯通(规范序)+ logic-blueprint-gen skill 端到端;code-forge 全工具(rx_check/build/run/fmt/test + LSP 编辑)。status=active;implementation_status=unlocked。

## 2. 范围与波次

- wave.1:code-forge MCP server 骨架 + rx 五工具(rx_check/rx_build/rx_run/rx_fmt/rx_test 子进程包装)+ f4-w1 冒烟 → G-F4-1。
- wave.2:.rxgraph schema + 节点注册表 + graph_validate 校验器 + Script 组件 + graph MCP 工具 + f4-w2 冒烟 → G-F4-2。
- wave.3:图解释执行运行时(事件规范序全贯通)+ call_function 互绑 + 热重载 + logic-blueprint-gen seam 摘除 + f4-w3 冒烟 → G-F4-3。
- wave.4:NodeGraph 面板(查看/微调/常量编辑/保存即校验)+ code_* LSP 三工具 + code-rx-migration seam 摘除(依赖 code_* 面)+ desktop 冒烟 → G-F4-4。
- wave.5:全量回归 + close-out。

## 3. 架构决策(wave.1 立项裁决)

- **D-F4-A(rx_check 诊断 JSON 源)**:rx CLI 不透传 --error-format(main.rs CompileOptions.error_format=None);rurixc.exe 支持 `--error-format=json`(RXS-0099,tooling::diag_json)。rx_check 子进程直包 rurixc.exe(诊断结构化单一事实源);rx_build/run/fmt/test 包 rx CLI(05 §4 注记「子进程包 rx CLI」语义不变——rurixc 是 rx 的同一前端 driver,07 §2 单一前端纪律不破)。
- **D-F4-B(热重载语义)**:用户指令点名「热重载」;上游 rx CLI fix|watch 未实现( seam 如实登记)。本里程碑热重载 = Script 组件 module/graphRef 变更(component_set 或文件改动经 asset 事件)→ PIE 内重挂载:解释器实例重建 + on_start 重发 + 黑板变量重置(不保留运行时态——确定性红线 I-5 优先;状态保留迁移属后续里程碑,登记 deferred)。edit 态改图即重校验(Problems),play 态改图下次 play_enter 生效。
- **D-F4-C(playtest 断言降级)**:roadmap G-F4「playtest 注入断言通过」;05 §6 test_run/assert_library 属 F6。本里程碑以 PIE 双态 + MCP 事件注入(logic_inject_input/logic_fire_trigger)+ component_get 组件态断言为等价机验,登记偏差;F6 test_run 落地后回填(照 F3 D-F3-C 先例)。
- **D-F4-D(crate 布局)**:code-forge 为独立 MCP server crate(crates/mcp/code-forge-mcp,照 engine-scene-mcp 族:stdin/stdout JSON-RPC + supervisor 看门狗);agentd 挂载 mcp__code-forge__ 前缀;图 schema/注册表/解释器进 forge-scene 邻接新 crate crates/forge-logic(场景组件态归 forge-scene,图 IR/解释器/事件队列归 forge-logic,engine-host 接线)。
- **D-F4-E(exposedProps 类型)**:10 §4.1 exposedProps kind 枚举首发:F32/I32/Bool/String/Vec3(冻结子集;实体引用类经 $self/$parent/标签查询,不直存实体 id)。
- **D-F4-F(Tag 组件形态,wave.3 立项裁决)**:entity.has_tag/add_tag 节点需要标签载体;09 §3 注册表无 Tag。注册表加 Tag 组件(fields: tag: string;一组件一标签,多标签挂多组件)——注册表无数组类型臂,不为 Tag 单开;has_tag = 组件存在性查询。09 §3 字段集扩展登记,不改冻结文档。
- **D-F4-G(trigger 语义,wave.3 立项裁决)**:上游 rurix-physics 冻结接口无 sensor 概念(BodyDesc 无 is_sensor;BodySemantic 无标志位)。on_trigger_enter/exit 落地为**逻辑层 AABB overlap 沿检测**:Trigger 组件(fields: kind: enum:box, extents: [f32;3])不建物理 body;每逻辑帧(物理 step 后)Trigger 实体 AABB × 其他实体 AABB 重叠集与上帧集合求差 → enter/exit 沿。contact 系事件仍走 PhysicsWorld::drain_contacts 规范序(03 §3.3 兑现)。上游 sensor 落地后切物理腿(登记 deferred)。
- **D-F4-H(call_function 运行时形态,wave.3 立项裁决)**:.rx 为编译语言无解释器;运行时调用 = rx build --emit=dll(#[export(c)] C ABI,上游 EI1.2)→ LoadLibrary 动态加载,纯值签名(f32/i32/bool 标量与数组)按 reflection 函数注册表编组。校验期互绑(module 存在 + fn 在 reflection 导出表 + 签名匹配)随 graph_validate 落地;dll 腿 timebox 内未竟则 seam 如实登记(验收门 G-F4-3 不考 call_function 运行时)。
- **D-F4-I(RigidBody→body 接线)**:play_enter 按 RigidBody 组件建 body(kind: static/dynamic/kinematic → BodyKind;shape=Box half_extents=|scale|/2 盒体近似,无碰撞形状字段如实标注;mass→MassProps),play_exit 移除;step 后 active_transforms 回写 run_scene 实体 transform(物理驱动渲染)。entity↔BodyId 映射存 HostState。

## 4. Deferred 处置
本波新增 deferred 追加于下方。

deferred:
  - id: RD-F4-001
    content: rx_doc 工具未落地(05 §4 表内);rx CLI fix|watch 上游 seam
    reason: doc 站生成与引擎文档流无涉;fix|watch 上游逐字「后续小里程碑承接」
    refill: 上游 rx fix|watch 落地后回填;rx_doc 随文档站需求立项
    owner: 下次 code-forge 波
    status: OPEN
  - id: RD-F4-002
    content: code_references 跨文件给不出(上游 LSP ToolingSession 单文档语义,无跨文件装载)
    reason: 上游 rurixc --tooling-server 每 uri 独立 analyze;session 无多文档图
    refill: 上游 LSP 多文档会话落地后回填跨文件 references;code_symbol_search 文本级扫描同期可切语义级
    owner: 下次 code-forge 波
    status: OPEN
  - id: RD-F4-003
    content: on_trigger_* 为逻辑层 AABB 沿检测(D-F4-G),非物理 sensor
    reason: 上游 rurix-physics 冻结接口无 sensor 概念(BodyDesc 无 is_sensor)
    refill: 上游 sensor 落地后切物理腿(接触过滤进 drain_contacts 规范序)
    owner: 物理内核波
    status: OPEN
  - id: RD-F4-004
    content: call.call_function 互绑未落地:校验期 module/fn/签名校验未接入 graph_validate;运行时 dll 腿(rx build --emit=dll + LoadLibrary)未实现;节点执行现 logic.unsupported 如实上报
    reason: timebox;G-F4-3/G-F4-4 均不考;10 §4.3「互绑」契约核心(校验期)需在下一 code-forge 波优先回填
    refill: graph_validate 加 call_function 校验臂(module 存在 + #[export(c)] fn 表 + 签名匹配)+ dll 运行时加载
    owner: 下次 code-forge 波
    status: OPEN

## 5. 修订
- 2026-08-17 立项:F3 全绿(wave.1~5 §6,status=closed)后用户指令开工「启动 F4 code-forge 开发,推进 rx 脚本编译与热重载」。上游盘点:rx CLI(build/check/run/fmt/test/vendor/doc/bench)+ rurixc(lexer→parser→HIR→MIR→codegen 全管线,--error-format=json,--tooling-server LSP,#[export(c)] EI1.2)均已就绪,H:\rurix\target\debug\{rx,rurixc}.exe 在库。

## 6. Close-out(只追加区)

### wave.1 验收记录(2026-08-17)

- 验收门:G-F4-1(rx 工具门)
- 结果:PASS
- 证据:
  - scripts/f4-w1-rx-tools-smoke.ps1 PASS(全链经 gateway→agentd→code-forge-mcp→上游 rx/rurixc 子进程):rx_check hello.rx → diagnostics=[];rx_check bad.rx → RX0008 诊断含 code/severity/span/message(file 入参回填,见踩坑 2);rx_fmt checkOnly → needsFormat=false;**rx_test unit_tests.rx → passed=2 failed=0(「.rx 脚本 rx_test CI 恒绿」兑现)**;rx_test unit_tests_fail.rx → failed=1 failures 含 fails_nonzero(失败如实捕获,不遮蔽);rx_run hello.rx → exitCode=0 + stdoutRef 落盘 data/code-runs/(内容=hello, rurix)
  - cargo test -p code-forge-mcp 14/14 PASS(8 纯解析:PASS/FAIL 汇总行/诊断 JSON/截断/超时;6 集成:真实 rx.exe,exe 缺失 [SKIP] 照 agentd 先例);cargo test -p forge-agentd 35/35(KNOWN_TOOLS 55→60 断言 + code-forge 存在性);cargo test --workspace 95/95 全绿无破损
- 交付:
  - crates/mcp/code-forge-mcp(std-only 无 tokio):rxtool.rs(env FORGE_RX_CLI/FORGE_RURIXC,缺省 H:\rurix\target\debug\{rx,rurixc}.exe;spawn 双管道读线程 + 超时杀进程 RX_TIMEOUT;stdout/stderr 各 1MB 截断;RX_CLI_NOT_FOUND 结构化错误)+ mcp.rs(tools/list 五工具,serverInfo name=code-forge)+ tests 14
  - rx_check 直包 rurixc --emit=check --error-format=json(D-F4-A 结构化诊断单一事实源);rx_build/rx_run/rx_fmt/rx_test 包 rx CLI(05 §4 注记)
  - agentd mcp.rs:ServerKind::CodeForge + mcp__code-forge__ 前缀 + 第三 OnceLock 长连接槽 + env FORGE_CODE_FORGE_MCP_BIN
  - tests/fixtures/f4/{hello,bad,unit_tests,unit_tests_fail}.rx;scripts/f4-w1-rx-tools-smoke.ps1
- 踩坑(如实登记):
  1. rx run 不透传程序参数(上游 parse_input_out 仅 <input> [-o])→ rx_run 的 args 非空时如实返 USAGE 错误,不静默丢弃。
  2. rurixc JSON 诊断无 file 字段(仅 level/message/code/labels/suggestions,span 0 基 LSP 行列)→ file 由入参回填。
  3. rx 语言无 assert/assert_eq 内建(仅 println)→ #[test] 成败以 fn -> i32 返回值=进程退出码表达(反例返 1 被 RX7011 捕获)。
  4. rx fmt 保留单行函数体为已格式化;仅真实排版偏差使 --check 退 1。
  5. 冒烟后立刻 cargo test 曾 exit 101:agentd 被杀瞬间其 code-forge-mcp 子进程短暂存活锁 exe,stdin 关闭后自行退出(顺带验证 stdio 循环退出语义);流水线需留意秒级窗口。
  6. PS 5.1 控制台中文乱码(代码页)仅显示层,JSON 断言与落盘内容正确。

### wave.2 验收记录(2026-08-17)

- 验收门:G-F4-2(图校验门)
- 结果:PASS
- 证据:
  - scripts/f4-w2-graph-smoke.ps1 PASS(经 gateway→agentd→code-forge-mcp / engine-scene-mcp):door_opener.rxgraph(10 §4.1 DoorOpener 逐字蓝本:on_trigger_enter → branch(has_tag otherEntity "player") → rotate_tween $self ref openSpeed)→ graph_validate ok=true;**四类坏图逐类拒绝**(GRAPH_DANGLING_INPUT/GRAPH_TYPE_MISMATCH/GRAPH_EXEC_CYCLE/GRAPH_DATA_CYCLE,nodeId 正确);同事件双入口 GRAPH_DUP_EVENT;graph_create 落盘 projects/demo/Content/Graphs/door_opener.rxgraph;graph_get 读回等值(nodes=4 edges=2);**component_set 挂 Script(graphRef)成功 + 不存在 graphRef 拒绝 SCRIPT_REF_NOT_FOUND(引用防护兑现)**;component_get 复核(graphRef + props openSpeed=120 覆盖)
  - cargo test:forge-logic 17/17(schema/注册表/八类错误码正反例/DFS 三色双环检测);forge-scene 8/8(Script dict 臂);code-forge-mcp graphtool 5 + 既有 12;forge-agentd 35/35(KNOWN_TOOLS 60→63);workspace 全绿无破损
- 交付:
  - crates/forge-logic 新 crate:graph.rs(GraphDoc/ValueSource untagged 三态/BTreeMap inputs 保确定性序列化)+ registry.rs(九族 40 节点冻结子集照抄 10 §4.2,NodeSpec 含 exec_in/exec_out/inputs/outputs pin 类型表;has_tag 输出 pin 名 out 照 §4.1 示例)+ validate.rs(validate_graph 纯函数,八类错误码:GRAPH_SCHEMA/UNKNOWN_NODE/DUP_EVENT/DANGLING_INPUT/BAD_SOURCE/TYPE_MISMATCH/EXEC_CYCLE/DATA_CYCLE)
  - forge-scene:Script 组件(module/graphRef string 二选一非空 + props dict 暴露属性覆盖;validate_props 新增 dict 类型臂;09 §3 字段集扩展标注 F4)
  - code-forge-mcp graphtool.rs:graph_validate(graph|path 二选一)/graph_create(校验通过才落盘,覆盖即更新;GRAPH_BAD_NAME)/graph_get(GRAPH_NOT_FOUND);--project 缺省 <workspace>/projects/demo
  - engine-scene-mcp:Script 挂载引用防护(component_set/entity_create 内联 components;graphRef/module 非空校验 <projects/demo> 相对路径存在;mcp 层落地——F2 确认未做 component_set 级资产引用防护(仅 asset_delete 阻断),无双边先例,落点如实登记;component_add/entity_batch_apply 内 Script 防护未接线,注释标注)
  - tests/fixtures/f4/{door_opener,bad_dangling,bad_type_mismatch,bad_exec_cycle,bad_data_cycle}.rxgraph;scripts/f4-w2-graph-smoke.ps1
- 踩坑:
  1. serde_json::Map 方法仅实现于 Map<String, Value>——Node.inputs 改 BTreeMap(key 排序顺带保确定性序列化)。
  2. graph_get 经 BTreeMap 重排 key,PS 侧与 fixture 文本序不同——冒烟递归 key 排序归一后值级深比较,不比文本。
  3. 冒烟杀 agentd/gateway 后孤儿 engine-scene-mcp/engine-host 继承 stdout 句柄曾致 cargo test 管道假死——冒烟 finally 补连带清杀(按启动时间过滤),复跑确认无残留。

### wave.3 验收记录(2026-08-18)

- 验收门:G-F4-3(触发开门门)
- 结果:PASS
- 证据:
  - scripts/f4-w3-logic-smoke.ps1 PASS(经 gateway→agentd→engine-scene-mcp):**主链「触发开门」全通**——门实体(Script graphRef=door_opener + Trigger box 2³)+ player(Tag player + RigidBody dynamic)→ play_enter 断言 logic.start → transform_set player 入门 AABB + play_step×90 → on_trigger_enter + 门 yaw=90°(openSpeed 默认 90,rotate_tween duration 1.2s=72 帧完成);**人工改常量生效**——component_set props {openSpeed:45} 重进重测 yaw=45°;**事件规范序**——drain 真实序 input@1 < contact@3 < update@5(第 20 帧 contact Begin);**接触事件**——player dynamic 落体接触门 static body,on_contact_begin 进环并派发探针图;**热重载**——play 态 component_set Script → logic.start 重发 + 黑板重置(D-F4-B);play_exit → edit
  - cargo test --workspace 137/137 全绿(exit=0,无非零失败行):forge-logic 28(interp 11:tween 60 帧恰 90°/timer/message 帧末清空/规范序/跨实体升序/黑板链/reload 重发+重置/trigger 差集+tag 门控/delay/unsupported 续链/move_tween);engine-host 12 单测(新增 6:body 建删/下落回写/contact 进环/trigger 沿/规范序/热重载)+ 11 集成;forge-agentd 35(修复一处 F3 潜伏并发竞争,见踩坑 1)
- 交付:
  - forge-logic interp.rs:GraphInstance(props 合并/黑板/tween/delay/timer 表)+ LogicRuntime::frame(规范序:input → contact(规范序)→ trigger 沿 → timer → update → message 清空;跨实体 id 升序,同实体挂载序);13 类节点实现;未实现节点(physics.*/audio.*/spawn/destroy/look_at/lerp/for_each/gate/draw_debug_line/call_function)如实 logic.unsupported 续链,不静默不伪造
  - forge-scene:REGISTRY + Tag(tag:string,D-F4-F)+ Trigger(kind:enum:box, extents:[f32;3],D-F4-G)
  - engine-host rpc.rs:advance_frame(step → drain_contacts 每帧新 SyncBudget → BodyId→实体翻译保规范序 → active_transforms 回写 → runtime.frame);play_enter 批建 body(D-F4-I,Box half_extents=|scale|/2)+ 装图(读/解失败回滚不进 play);play_exit 批删;component_set Script 热重载;transform_set play 态 body 传送同步(remove+re-add,裁决见下);logic.inject_input 新方法;main.rs 后台物理线程门控 Running(edit/Paused 不步进,保 play_step 确定性)
  - MCP:logic_inject_input(engine-scene-mcp 转发);KNOWN_TOOLS 63→64;watchdog 断言 40→41 工具 / 5→7 组件
  - skills/logic-blueprint-gen 摘 seam:工作流改真实工具面(graph_validate → graph_create → component_set Script → play 注入断言 → debug 循环),「不交付未验证逻辑」纪律保持
  - scripts/f4-w3-logic-smoke.ps1
- 裁决记录(契约外最小必要补充,代码注释留档):
  1. transform_set play 态 body 同步(remove+re-add,双后端通用):D-F4-I 单向回写会把传送踩回旧位;传送语义=速度/接触态重置,失败如实报错。
  2. 后台物理线程门控 Running:原实现 edit 态也空跑;Paused 墙钟步进毁 play_step 确定性。既有测试仅断言 steps 存在性,无破损。
  3. logic.start 无条件记录(load 即激活语义,10 §3.1「PIE 进入/实体激活」):door_opener.rxgraph 本无 on_start 节点,冒烟以此断言。
  4. engine-host 单测经 FORGE_PROJECT_ROOT env 注入临时项目根(默认路径行为不变)。
- 踩坑:
  1. **F3 潜伏测试并发竞争曝光**:subagents_hot_reload_without_restart 写真实 data/agents/zz-test-hot.md,与 subagents_list_five_builtin_profiles「恰好 5 个」断言撞窗口(cargo test 同进程并行,F3 收官时序未踩中,F4 wave.3 全量跑曝光)——SUBAGENTS_DIR_LOCK 互斥修复,forge-agentd 三连跑 35/35。
  2. interp 初版 eval_pure 丢事件上下文(has_tag 读事件 pin 恒 false)→ EventCtx 透传修复,单测抓获。
  3. 规范序断言用「每帧注入 input + 逐帧步进至 Begin」避开落地时刻预测与 Jolt 睡眠窗口,确定性通过。

### wave.4 验收记录(2026-08-18)

- 验收门:G-F4-4(前端/LSP 门)
- 结果:PASS
- 证据:
  - **NodeGraph 面板**:client vitest 64/64(新增 14:graphStore 7——loadByPath/editConst dirty/save 校验失败不落盘/保存成功 dirty=false;nodeGraphView 7——4 节点 2 边渲染/常量内联编辑/保存流/校验失败红框/EditorView 集成选中实体→页签→graph_get);desktop 冒烟 f4-w4-nodegraph-smoke.ps1 PASS:editor → NodeGraph 页签 → 路径加载 → [data-graph-node]=4 → 截图 desktop-smoke-nodegraph-2026-08-17T17-14-28-794Z.png(154,867 B,目验:4 节点卡片 event 绿/flow 蓝头条 + exec 实线 + 数据虚线 + Exposed openSpeed=90 表)
  - **code_* LSP 三工具**:scripts/f4-w4-lsp-smoke.ps1 PASS(经 gateway→agentd→code-forge-mcp→rurixc --tooling-server 常驻 LSP 会话):code_symbol_search {query:"smooth"} 命中 3:7 span;code_references refs=3(定义行 3 + 调用行 8;上游 def_span 整项 + item_defs 名 span 双发如实透传不去重);code_structured_edit span replace 41→7 读回 + newDiagnostics=[] + 改回还原;越界 SPAN_OUT_OF_RANGE 不写盘;双路 SYMBOL_NOT_FOUND
  - cargo test:code-forge-mcp 16(含 2 真 LSP 集成)+ forge-agentd 35(KNOWN_TOOLS 64→67);workspace 148/148 全绿(基线 137 + 新增 11)
  - pnpm -r test 87/87(protocol 5 + client 64 + host 18);client typecheck/build 绿
- 交付:
  - client:graphStore.ts(GraphDoc/ValueSource 三态与 Rust serde 逐字对齐;loadByPath/loadForSelectedEntity/editConst/editExposedDefault/save 校验不过不落盘)+ NodeGraphView.tsx(SVG 20px 网格 + 节点卡片 + exec 实线/数据虚线贝塞尔 + 常量内联编辑 + Exposed 表 + 校验红框 + 空态如实标注「从零生成走 Chat agent」)+ EditorView 接线(centerTab/selectedId 联动加载)+ forgeApi.callCodeTool;desktop main.cjs nodegraph 冒烟场景;scripts/f4-w4-nodegraph-smoke.ps1
  - code-forge-mcp:lspclient.rs(rurixc --tooling-server 常驻会话;Content-Length framing 读写兼容上游 LF 行尾;读帧线程+mpsc 10s 超时;OnceLock+Mutex 单例崩溃懒重连;Drop 杀进程)+ codetool.rs(三工具 + 文本符号扫描器 + .rx 递归走查排 target/vendor);FORGE_CODE_FORGE_PROJECT env 项目根兜底
  - skills/code-rx-migration 摘 seam:code_symbol_search → code_references → code_structured_edit → rx_check → rx_test 五步真实工具面
  - scripts/f4-w4-lsp-smoke.ps1;tests/fixtures/f4/lsp/mathlib.rx(单文件双 fn,rurixc check 零诊断)
- 勘探纠偏(如实登记):
  1. **--emit=reflection 不能当符号表**:实测产物 entries 仅枚举 shader entry(vertex/fragment/compute/mesh/kernel),JSON 禁用面无 file/span(RXS-0304/0305)——code_symbol_search 改 crate 内文本扫描器(条目头匹配,0 基字符口径 span),工具描述标注「文本级非语义级」。
  2. **上游 LSP 单文档语义**:ToolingSession 每 uri 独立 analyze 无跨文件装载 → references 限同文件;fixture 走单文件双 fn(头注释 + SKILL.md 注记);跨文件 references 待上游多文档会话(登记 deferred)。
  3. 上游 LSP 写帧 LF(writeln!)非标准 CRLF——读帧兼容两种。
- 踩坑:
  1. main.cjs 经编辑工具改两次报成功但磁盘未变(IDE 脏缓冲坑复现)——Write 临时块 + PowerShell 直写插入 + Select-String 核验 + node --check 绕过;EditorView.tsx 导入行被后续编辑覆盖同法补回。**教训坐实:desktop/client 关键文件改后必须终端核验落盘**。
  2. React 受控 input:冒烟填路径须 HTMLInputElement.prototype.value 原生 setter + input 事件,填值与点「加载」分两次 executeJavaScript(间隔 400ms),否则 click 闭包读旧 state。
  3. PS 5.1 Set-Content utf8 带 BOM 被 .rx lexer 拒(RX0001);println 只收 &str(RX2001)——fixture 无 BOM 不打印数值。

### wave.5 验收记录(2026-08-18):全量回归 + close-out 终审

- 全量回归数字(均来自命令输出):
  - cargo test --workspace **148/148 全绿**(exit=0 无非零失败行;forge-logic 28 / engine-host 23 / code-forge-mcp 16 / forge-agentd 35 / assetd 21 / forge-scene 9 / 余 crate 套件)
  - pnpm -r typecheck 全绿;pnpm -r test **87/87**(protocol 5 + client 64 + host 18);pnpm -r build 全绿
  - go test ./...(gateway-go)ok
  - desktop 冒烟:home PASS(75,681 B)+ nodegraph PASS(154,867 B,4 节点)+ settings PASS(F3 面回归)
- 四门终审:
  - G-F4-1 rx 工具门:PASS(wave.1 §6)——rx_test CI 恒绿兑现(fixture #[test] 正反例经 MCP 全过/失败捕获);rx_check 结构化诊断(rurixc --error-format=json,D-F4-A);rx_fmt 幂等;KNOWN_TOOLS 55→60
  - G-F4-2 图校验门:PASS(wave.2 §6)——DoorOpener 图 validate ok;四类坏图 + 双事件入口逐类拒绝;Script graphRef 引用防护 SCRIPT_REF_NOT_FOUND;KNOWN_TOOLS 60→63
  - G-F4-3 触发开门门:PASS(wave.3 §6)——agent 图 → 校验 → 挂载 → 注入断言(yaw=90°);人工改常量生效(45°);事件规范序 input<contact<update;接触 Begin;热重载 on_start 重发;KNOWN_TOOLS 63→64
  - G-F4-4 前端/LSP 门:PASS(wave.4 §6)——NodeGraph 真实渲染 + 常量编辑 + 保存校验红框;code_* 三工具栈级(LSP 常驻会话);KNOWN_TOOLS 64→67
- 结论:**F4 四门全绿,里程碑 close-out。status: active → closed。**
- open RD 不阻收官(均有 refill 路径):RD-F4-001(rx_doc/fix|watch)/ RD-F4-002(跨文件 references)/ RD-F4-003(trigger sensor 物理腿)/ RD-F4-004(call_function 互绑——**下一 code-forge 波第一优先**)。
- 下一里程碑可选:**F5 生成接入**(gen-image/gen-model 适配层 + Assets 右键生成 + generation 设置页 + gen-asset-fill skill)或 RD-F4-004 回填波(call_function 互绑)或先消化 open RD。
