# 13 · 里程碑路线图

> 指导性文档(非冻结契约)。每期验收门 = 可机验条目;性能数字必须实测(measured),对标 rurix 工程纪律。

## 依赖序总览

```
F0 地基 → F1 场景编辑闭环 → F2 素材管线 → F3 agent 集群与 skills
     ↘ F4 交互逻辑 → F5 生成接入 → F6 试玩回归与打包
```

## F0 · 地基(进程与通道)

交付:

1. monorepo 骨架(`02 §4`);forge-agentd 由 agent-cowork 移植编译通过,mock provider 恒绿。
2. engine-host 空场景启动:rurix-render 离屏渲一三角 + rurix-physics `PhysicsWorld` 固定步空跑。
3. 控制通道 `host.ping` / `scene_new` / `scene_summary`;事件通道空 ring。
4. gateway 反代 + JWT;`data/mcp.json` autoStart 拉起 engine-scene-mcp。

验收门:

- `curl /health` 全绿;`mcp__engine-scene__scene_summary` 经 agentd 工具循环调用成功。
- host 崩溃看门狗重启并上报 `host.crashed` 事件(kill -9 实测)。

## F1 · 场景编辑闭环

交付:

1. IDE 七区骨架(`07 §1`);Viewport 帧通道(共享纹理);相机与 gizmo;点选链路 `viewport_pick`。
2. Entity/组件模型 + 注册表(`09 §3`);`.rxscene` 读写;Hierarchy/Inspector 全功能。
3. engine-scene 工具面 §2 全量;Undo/Redo 栈(host 侧命令栈)。
4. PIE:`play_enter/pause/step/exit` + 运行态/编辑态双态 UI。

验收门:

- 手测脚本:建 3 实体 → 摆位 → 保存 → 重启重载 → 逐字节同态(确定性)。
- Chat 指令「创建 10 个立方体排成一列」经 agent 完成且视口可见;`scene_checkpoint` 回滚有效。

## F2 · 素材管线

交付:

1. assetd:gltf/fbx/png/jpg 导入;rurix-geom-build 构建;缓存键与 `asset_build_status`。
2. Assets 面板(`07 §4`):缩略图、拖拽实例化、右键六菜单。
3. `.meta` + GUID + 引用图;`asset_move`/`asset_delete`/`asset_fix_redirectors` 全语义。
4. 材质资产 + MeshRenderer 绑定;贴图处理(`texture_process`)。

验收门:

- 导入 gltf 示例集 → 缓存二次导入零构建(hash 命中);断链删除被阻断并给出引用清单。
- `asset-cleanup` skill 对混乱素材目录产出整理提案并经 Proposal 执行成功。

## F3 · agent 集群与 skills

交付:

1. swarm 四分片策略(`04 §5.2`)+ 冲突检测;`multitask` 模式 UI。
2. 内建 subagent 五 profile(`04 §6`);`data/agents/*.md` 热加载。
3. 首发 12 skills(`06 §3`);skill 管理设置页 tab。
4. debug 模式三件套(截图 + events_drain + graph_dump)进 `debug-scene-issue`。

验收门:

- 「给 40 个关卡块生成碰撞体」经 multitask 分片并行完成,分片报告一致、无重叠写。
- 「第三盏灯没阴影」debug 会话给出根因与修复,playtest 断言通过。

## F4 · 交互逻辑

交付:

1. `.rxgraph` schema + 节点注册表 + 校验器 + 解释执行运行时(`10`)。
2. NodeGraph 面板(查看/微调/常量编辑);`call_function` 互绑 `.rx`。
3. 接触/触发/input/timer 事件全线贯通(规范序);`logic-blueprint-gen` skill 端到端。
4. code-forge 全工具(rx_check/build/run/fmt/test + LSP 编辑)。

验收门:

- agent 生成「触发开门」图 → 校验 → 挂载 → playtest 注入断言通过;人工在图上改常量生效。
- `.rx` 脚本 `rx_test` 在 CI 恒绿。

## F5 · 生成接入

交付:

1. gen-image-mcp / gen-model-mcp 适配层 + 首个远程适配器;`gen_accept` 入管线 + provenance。
2. Assets 右键「生成」入口;候选挑选 UX;设置页 `generation` tab。
3. `gen-asset-fill` skill(缺口清单 → 生成 → 挑拣 → 入库)。

验收门:

- 无后端配置时全工具面 `GEN_BACKEND_NOT_CONFIGURED` 显式错误(I-5)。
- 配通一个后端后,「生成 4 张候选木纹 → 接受 1 张 → 材质引用 → 场景可见」全链路走通,provenance 完整。

## F6 · 试玩回归与打包

交付:

1. playtest 全工具 + 断言库 + SSIM 截图断言;`test-matrix` swarm 并发 headless host。
2. `playtest-regression` skill;Console/Metrics 面板帧统计闭环。
3. `project-pack` 最小打包(`08 §7`)+ `engine-host --game` 模式。

验收门:

- 示例游戏(迷宫原型)回归矩阵全绿;打包产物在干净机器(无工具链)可运行。
- 性能:示例场景 1080p ≥ 60fps(实测记录进 evidence/,对标 rurix measured 纪律)。

## F7 · Agent 前端重设计(后路线图里程碑,2026-08-18 立项 D-020)

交付:

1. agentd 事件基座:append-only 事件日志 + seq + SSE + design-snapshot;会话持久化 CRUD/fork/revert。
2. turn 执行事件化(`ask:execute` 五模式 + runs 控制 + todos REST)。
3. React 壳全量重设计对齐 Moonlit Agent IDE(I:\agent-debug-frontend-backend-copy-20260530):主题系统(明暗+预设+派色算法)、三栏壳、聊天时间线、Composer、workbench tabs、设置体系;游戏编辑器嵌入为壳内视图(游戏原生区零改动);技术栈不变。

验收门:见 `milestones/f7/F7_CONTRACT.md`(G-F7-1~5)。

## F8 · IDE 商用化(功能补全 + 浏览器真实任务测试,2026-08-19 立项 D-021)

交付:

1. 对话核心补全:Composer 诚实占位清零;Inspector 文件预览(agentd confined 只读端点)。
2. 模型与设置商用面:openai-compatible 通用渠道;ModelsPage 全链闭环;auth/多 provider 正式裁决留档。
3. 浏览器通路:bridge 浏览器禁用态审计;Viewport 浏览器回退腿定型;host 兼容核验。
4. 浏览器真实任务矩阵:Playwright 落仓八任务 E2E(会话/消息/编辑器/场景编辑/multitask/提案/设置/会话管理)。

验收门:见 `milestones/f8/F8_CONTRACT.md`(G-F8-1~5)。

## F11 · 资产商店 + Skill 管理(后路线图里程碑,2026-08-25 立项 D-025)

> 编号留痕:F9(全项目 journey)与 F10(context RAG)为需求驱动的轻量波,走 `.trae/specs/` 未立 milestone 契约,故本波取 F11 避让已被脚本名占用的 f9/f10 编号。

交付:

1. registry 内核:库 crate `forge-store`——协议 DTO 与校验、`file://` 与 `https://` 双源驱动、多源聚合、个人资产库(内容寻址)、安装卸载更新、打包发布;`sha256_hex` 收编进 `forge-util`。
2. skill 内核:兑现 `06 §2` 未实装的注入链路——`read_skill` 原生工具 + 系统提示技能索引 + `ask:execute` 结构化 `skills[]` 经 preamble 注入全文;技能 CRUD 与校验(删除走 Proposal)。
3. store 服务面:`store-mcp` 工具面 + agentd `store.rs` REST 长任务面;卸载与技能删除接 Proposal 门。
4. 前端两个大类:Sidebar 两个导航按钮 + Workbench 两个 tab(商店三子页 / Skill 管理);Composer 文本前缀协议下线。
5. 官方源种子(仓内 `registry/`,离线可完整验收)+ 冒烟 + 浏览器 journey 矩阵。

验收门:见 `milestones/f11/F11_CONTRACT.md`(G-F11-1~6)。
