# 14 · 决策日志

> 滚动追加。每条 = 编号 / 日期 / 状态 / 决策 / 理由 / 驳回项。冻结契约变更必须先在此登记。

| 编号 | 状态 | 决策 | 理由 | 驳回项 |
|---|---|---|---|---|
| D-001 | 定案 | 前端 = Electron + React + TS + Tailwind;不采用 GPUI 原生壳 | 设置页模式(cindy)与 IDE 模块(agent-cowork)均为 web 技术栈心智,React 面板生态(节点图/树/属性表)成熟;Electron 原生共享纹理方案可满足视口;agentd 是 HTTP/SSE,前端栈可替换成本低 | GPUI 原生(设置页模式不可移植,节点图等复杂面板自研成本高,人力集中内核) |
| D-002 | 定案 | API 前缀 `/api/forge/*`,端口 gateway:8102 / agentd:8103 | 与 agent-cowork(8002/8003)可同机共存,移植对照清晰 | 复用原端口(冲突) |
| D-003 | 定案 | rurix 依赖锚定 release tag(起步 `v1.0.1-dist` 系),Cargo path/git 依赖 | 稳定面冻结(rurix RD-008 stable 快照);跟随上游节奏升级,不追 main | vendored fork(漂移风险,I-1 精神相悖) |
| D-004 | 定案 | 引擎宿主 = 独立进程(engine-host),不做 rurix-engine 式 DLL 嵌入 | GPU 崩溃隔离、看门狗重启、多 headless 实例(swarm test-matrix)都要求进程边界;rurix-engine DLL 先例仅作 C ABI 模式参考 | IDE 进程内嵌渲染(单点崩溃拖垮编辑器与 agent 会话) |
| D-005 | 定案 | 配置变量前缀 `FORGE_AGENT_*` | 与 agent-cowork 并存不串扰 | 沿用 `AGENT_DEBUG_*` |
| D-006 | 定案 | 图像/3D 生成走 MCP 适配层,不进 agent-providers | providers 是 LLM 文本通道;生成有候选管理/落盘/入管线语义,MCP 工具面更贴合,后端可插拔 | provider 扩展(语义错配) |
| D-007 | 定案 | 不新增 `gamedev` agentKind;沿用 `coding` 默认 | agent-cowork 已验证 profile 应极少;域差异经 prompts/skills/MCP 工具面表达,避免复制 profile 机器 | 新增 profile(维护面翻倍) |
| D-008 | 定案 | subagent `tools` 支持 MCP 前缀通配(`mcp__engine-scene__*`) | 域级授权符合 profile 语义;在 task 工具过滤层实现 | 逐工具枚举(维护脆弱) |
| D-009 | 定案 | 生成 3D 资产后处理用 rurix-geom-build 简化 DAG;不引第三方 retopo 库 | 内核单一(I-1);生成资产与人工资产同构建链同权管理 | 引 OpenMesh/InstantMeshes 类依赖(第二几何栈) |
| D-010 | 定案 | 实体模型 = 组件化对象模型(UE Actor/Unity GameObject 语义),非数据导向 ECS | agent 可读性、Inspector 直映射、目标用户习惯;引擎内部仍可用 SoA/并行实现 | 纯 ECS 暴露到序列化/UI(认知与工具链成本高,本期规模不需要) |
| D-011 | 待定 | 节点图 F0–F3 解释执行;F4 评估编译到 `.rx` | 解释器先求语义正确与可调试;性能数据不足前不做编译器 | 直接编译(过早优化,调试面差) |
| D-012 | 定案 | 场景/prefab/节点图 = JSON 文本;`.meta` = YAML | 可 diff/review,agent 直读直写(经 MCP);YAML 仅用于人手维护的 sidecar | 二进制序列化(P-4 相悖) |
| D-013 | 定案 | 设置页逐字对齐 cindy 三件套(tab 单一事实源 + Section 组件 + main 端 JSON store) | 用户指定参考;模式已在 cindy 生产验证 | 自研设置框架 |
| D-014 | 定案 | 批量操作一律无 UI,经 agent + MCP bulk 工具;UI 多选仅显示公共属性 | 用户明确要求「繁杂重复操作不交前端」;UE Property Matrix 式批量窗明确不采用 | 批量编辑窗(红线 R-2) |
| D-015 | 定案 | 前端应用架构改栈:借鉴 DeepSeek Harness(dsh) webui 架构自研——pnpm monorepo + Cordis 式插件化 host/client 分层 + profile/bundle 组合 + append-only 会话事件日志;保留 Electron 壳加载本地 host;UI 布局与风格(cursor+Claude 系)不变,React 18 + Tailwind 保留;不 fork dsh 代码;删除 rust/ GPUI 实验 workspace。**修订 D-001 的应用结构部分**(UI 层技术选型不变,单体 electron-forge 包改为 host/client 分层) | 用户指令(2026-08-16 /goal);dsh「一切皆插件 + 分层组合 + 事件日志单一事实源」与本项目 04 会话模型、12 checkpoint/replay 同构;Electron 壳保留原生帧通道(共享纹理)可能;dsh 处于 developer preview,fork 有破坏性变更风险;GPUI 路线 D-001 已否决,rust/ 为无引用遗留 | fork dsh web-app(绑定 dev preview);纯浏览器应用(失共享纹理视口通道);保留 GPUI workspace(维护面) |
