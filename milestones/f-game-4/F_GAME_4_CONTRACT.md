---
contract: F-GAME-4
title: F-GAME-4 2D 帧动画 + 角色素材管理 + Agent 团队规划(参照 VibeGame 机制)
status: active
implementation_status: unlocked
active_scope: all-waves
version: 0.1
date: 2026-08-31
rfc_required: []
upstream_docs:
  - 14_DECISION_LOG.md (D-031)
  - 05_MCP_PROJECTS.md (§3 asset-pipeline 工具面 + Errata E-05-001)
  - 07_FRONTEND_IDE.md (§9 UI 不做清单 + Errata E-07-002)
  - 08_ASSET_PIPELINE.md (§3.2 类型闭集 + Errata E-08-002)
  - 09_ENTITY_SCENE_MODEL.md (§3 组件注册表 + Errata E-09-002)
  - 10_INTERACTION_LOGIC.md (§4.2 节点注册表 + Errata E-10-001)
  - 04_AGENT_BACKEND.md (§3 composer 模式 / §4 plan-todo 引擎 + Errata)
implementation_unlock:
  required_all:
    - F-GAME-3 落账(D-030,Sprite 组件/2D 模式/正交相机基础面在位)
    - 用户开工指令(2026-08-31 原文:「参考 VibeGame 项目,将其 agent-team 规划、角色素材管理、2d 帧动画等当前本引擎没有或缺失的部分补充到当前引擎当中,允许 fork 后阅读。积极调用子 agent」;两问拍板:Agent 团队=完整版〔并行+DAG+代码级编排+收工门〕/ 动画状态机=本期一并做)
    - 参考材料:VibeGame 克隆读源(.tmp-vibegame/,机制分析留档于实现会话)
in_scope:
  - "**wave.1 帧动画引擎面**:.rxsprite 资产类型(assetd sprite.rs:帧 bbox + pivot 级联〔帧级>文档级>缺省脚底锚〕+ clip〔duration 优先于 fps/loop/onFinish〕+ 可选 animator 状态机 + 连通域自动切帧〔背景判定与视口色键同规则〕);Sprite 组件扩 sprite/clip/frame 可选字段(texture 转可选,二选一);渲染 UV 子矩形进 push constants(96→112B)+ 帧尺寸/pivot 锚定折入模型矩阵与点选;宿主 anim.rs 动画运行时(dt while 跨帧、FSM 首匹配 trigger 消费、组件唯一写者=宿主、FSM/手动双模式按实体 clip 空否判定);forge-logic 五节点(sprite.play〔restart 可选缺省幂等〕/sprite.stop/sprite.set_frame/animator.set_bool/animator.set_trigger,40→45)经 AnimCommand 命令通道"
  - "**wave.2 角色素材管理**:asset-pipeline-mcp 四工具(sprite_create/sprite_get/sprite_set/sprite_autoslice,05 E-05-001);客户端 Sprite 编辑器 Workbench tab(bbox 拖拽编辑/pivot 十字准星/前端 Auto-Detect 同判定规则/clip 编辑/动画预览 rail;I-3 零常驻面板,F11 tab 先例);AssetsPanel sprite 类型过滤与右键入口 + Inspector 精灵摘要;game-2d-kit skill 动作表生成纪律(整表一次生成保 identity/品红底/containment/idle 反向闭合)与切帧-clip-挂接工作流"
  - "**wave.3 Agent 团队规划**:task 工具并行执行(同轮多 task 并发,上限 4,结果按原序回注);subagent profile.model 生效(按角色配模型,mock 恒 mock);TodoItem 扩 stage/deps/role/prompt/verify(旧 todos.json 兼容);plan_write 结构化 DAG;plan.rs 调度器(deps 就绪集并行派发);team 模式代码级编排(leader 产计划 → 阶段并行派发 → verify=qa 自动复测与修复轮 → verify=reviewer 终审 VERDICT 收工门,修复轮上限 3 超限如实失败);planner/reviewer 角色档案"
  - "**demo 与验收**:PZ 僵尸走路图集(2 帧步行摆动)+ PZ_Zombie_Walk.rxsprite(idle/walk clip + isMoving animator)+ anim_demo.rxscene(FSM 模式与手动 clip 模式双实体)+ anim_demo.rxgraph(on_start → animator.set_bool);活体 E2E(TCP RPC 驱动 play + viewport.frame 帧差异断言)"
out_of_scope:
  - 动画状态机图形化编辑器(animator 段以 JSON 表单编辑,数据即事实源;图形化待需求驱动)
  - 关键帧编辑时间轴(预览 rail 为只读播放控制;transform 关键帧动画非本波)
  - 骨骼/蒙皮动画(SkinnedMeshStub 维持文档占位)
  - .rxanim 独立动画资产类型(v1 单表单文档已足;拆分增加引用面,D-031 驳回项)
  - 多贴图 per-clip 纹理切换(一角色一张表纪律;会话槽位静态绑定约束下的多表支持待需求)
  - gend img2img/参考图一致性生成(生成侧纪律经 skill 提示词表达;后端能力扩展另立项)
  - VibeGame tmux 多进程编排复刻(进程内并行 + 事件面等价达成,D-031 驳回项)
deliverables:
  - id: D-FG4-1
    name: .rxsprite 资产链(类型/校验/切帧/引用图/索引)
    evidence: cargo test -p assetd(29)+ forge-index(17)全绿;sprite.rs 单测覆盖 pivot 级联/duration 优先/animator 校验/行带排序/色键同规则
  - id: D-FG4-2
    name: 渲染与运行时(uv_rect 112B + anim.rs + rpc 全链)
    evidence: cargo test -p engine-host 全绿(含 Vulkan 设备腿真跑 + sprite_animation_full_loop_via_play 集成);cargo test -p forge-scene(16)/forge-logic(43)
  - id: D-FG4-3
    name: 素材管理工具面(四 MCP 工具 + stdio E2E)
    evidence: cargo test -p asset-pipeline-mcp:stdio_sprite_tools_end_to_end(合成图集→导入→autoslice→create→get→set 坏文档拒→sprite→texture 引用边)
  - id: D-FG4-4
    name: 客户端 Sprite 编辑器
    evidence: client vitest 全绿(spriteStore/autoDetect/tab 冒烟)+ build 通过
  - id: D-FG4-5
    name: Agent 团队编排(并行/模型/DAG/team 收工门/角色档案)
    evidence: cargo test -p forge-agentd 全绿(并行重叠断言/调度器拓扑/收工门脚本化回路/超限如实失败)
  - id: D-FG4-6
    name: demo 与活体验收
    evidence: anim_e2e.py PASS(FSM 僵尸 walk 帧循环 0↔1 + 手动 clip 僵尸 walk + play 两帧像素差异 21712 + play.exit 编辑态零污染)
acceptance_gates:
  - id: G-FG4-1
    name: 引擎面门
    check: 全量 cargo 分套件绿;旧场景 load→save 字节同态不破(forge-scene 同态测试);texture 直贴模式渲染行为不变(uv_rect=[0,0,1,1] 恒等);点选与画面一致(sprite_render_transform 共用)
  - id: G-FG4-2
    name: 动画语义门
    check: restart 缺省幂等(重复 play 不冻帧)/duration 优先于 fps/非循环 hold|first/FSM 转换首匹配且 trigger 消费/hasExitTime 等播毕/FSM 与手动双模式并存(同一 .rxsprite);误用一律 anim.warn 不静默
  - id: G-FG4-3
    name: 素材管理门
    check: autoslice 背景判定与 FS_TEX_WGSL 色键一字不差(alpha<0.02×255 或 2g<min(r,b));sprite_set 坏文档 SPRITE_INVALID 拒;sprite_create 同名复用 GUID;编辑器保存→视口 2s 内生效(TTL 缓存)
  - id: G-FG4-4
    name: 团队编排门
    check: 同轮多 task 并发(时间重叠证据)且结果按原序回注;profile.model 解析与回落如实;调度器依赖拓扑推进、环依赖如实报错;reviewer REJECT 触发修复轮、APPROVE 方收工、3 轮超限如实 failed;mock provider 恒绿(无 LLM 依赖)
  - id: G-FG4-5
    name: 活体验收门
    check: anim_e2e.py PASS(帧号推进断言 + 帧像素差异 + exit 零污染);Sprite 编辑器全流程手测(打开→Auto-Detect→调 bbox/pivot→建 clip→预览→保存→视口生效)
---

# F-GAME-4 契约(摘要)

正文见 frontmatter;设计细节与裁决见 `14_DECISION_LOG.md` D-031 与六篇 Errata
(E-05-001 / E-07-002 / E-08-002 / E-09-002 / E-10-001 / 04 Errata)。

## 交付记录(滚动)

- 2026-08-31 wave.1/wave.2(后端)完成:assetd 29 绿(sprite 单测 6)、forge-scene 16 绿、
  forge-logic 43 绿(五节点 + 命令通道)、engine-host 全套件绿(38 单测含 anim 5 + rpc
  全链集成;Vulkan 设备腿真跑,新 WGSL 112B push constants 实证)、asset-pipeline-mcp
  stdio E2E 绿(sprite 四工具端到端)。
- 2026-08-31 demo 落地:PZ_Zombie_Walk 图集(1056×528,2 帧底对齐)+ .rxsprite
  (idle/walk + isMoving animator)+ anim_demo 场景(FSM/手动双模式)+ 活体 E2E PASS
  (walk 帧循环 0↔1、两帧像素差异 21712、exit 零污染)。
- 2026-08-31 wave.3 完成:cargo test -p forge-agentd **224 passed 0 failed**(新增 22:
  llm 并行 3 / agent 6 / plan.rs 调度与编排 12 / engine schema 1)+ cargo check
  --workspace 通过。并行策略 = 全-task 轮分段并发(上限 4,`_toolCallId` 注入解决
  并发父时间线归属,串行路径逐字不变);team 编排状态机 = TodoStore 本身(todo.updated
  推进,无内存副本);planner/reviewer 档案落 data/agents(工种 6→8);修复轮以
  agent.steered 留痕,超限如实 failed;计数测试如实更新(工具 98→102 / profile 6→8)。
- 2026-08-31 wave.2(客户端)完成:Sprite 编辑器 Workbench tab(spriteStore + 画布/
  右栏/预览三组件 + spriteAutoDetect 纯函数,判定规则与引擎色键逐字一致)+ 资产面板/
  检视器接线。client vitest **515 passed(46 files)**(新增 3 文件 26 用例,既有零破坏),
  tsc --noEmit + vite build 通过,零新 lint 错。已知取舍:animator 图形化 defer(JSON
  文本域 + 实时校验)、tab 关闭无 dirty 拦截(编辑态留 store 不丢)。
- 2026-08-31 终局回归:cargo test --workspace 全绿(全套件含 forge-agentd 224 与
  engine-host Vulkan 设备腿)+ pnpm -r test 全绿(client 515 / host 29)+ ReadLints
  六个核心改动文件零错;go 面零改动未触发。G-FG4-1~4 证据齐;G-FG4-5 活体 E2E PASS,
  编辑器交互面留用户手测(vitest 已覆盖 store/交互 reducer/预览语义)。
  参考克隆 .tmp-vibegame 按 D-031 计划收尾删除(可随时重克隆)。
