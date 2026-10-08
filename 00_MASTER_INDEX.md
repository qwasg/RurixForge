# 00 · 主索引 — RurixForge AI 游戏引擎设计文档集

> 本文件是文档集的唯一入口与事实源锚点。所有文档条款采用 `文档号 §节号` 引用(如 `05 §3.2`),
> 决策采用 `D-xxx` 编号(见 `14_DECISION_LOG.md`),冻结契约变更必须登记决策日志。

- 项目代号:**RurixForge**(暂定;「AI 主导的游戏引擎」)
- 文档集版本:v0.1(初稿定版)
- 目标读者:实现本引擎的 agent 团队与人类工程师。文档写作约定 = agent 可机读:
  条款编号化、表格优先、接口面全量枚举、无装饰性图表、无营销性语言。

## 1. 产品一句话

以 rurix 渲染器/物理引擎为运行时内核,以 agent 集群(coding 模式 + swarm)为第一公民操作者,
以 MCP 工程为全部重复性工作的执行面,人类只经极简前端做必须人工确认与直观调整的
**AI-first 游戏制作引擎**。

## 2. 文档地图

| 文档 | 标题 | 内容域 | 冻结级别 |
|---|---|---|---|
| `00_MASTER_INDEX.md` | 主索引 | 文档地图、术语表、全局不变量 | 冻结 |
| `01_PRODUCT_VISION.md` | 产品定位与设计原则 | 定位、七条设计原则、用户与核心工作流、红线 | 冻结 |
| `02_SYSTEM_ARCHITECTURE.md` | 系统总体架构 | 进程拓扑、组件职责矩阵、通信协议、仓库与目录布局 | 冻结 |
| `03_ENGINE_LAYER.md` | 引擎层(rurix 内核) | rurix-render / rurix-physics / rurix-geom-build 复用面、engine-host 进程契约、帧流与输入通道 | 冻结 |
| `04_AGENT_BACKEND.md` | Agent 后端(forge-agentd) | crate 划分、coding profile、composer 模式、plan/todo、swarm 集群、subagent、memory、checkpoint、providers | 冻结 |
| `05_MCP_PROJECTS.md` | MCP 工程集 | 7 个 MCP server 的工具面全量契约(命名、参数、错误码、幂等性) | 冻结 |
| `06_SKILLS_LIBRARY.md` | Skill 体系 | SKILL.md 格式、发现/加载机制、游戏域核心 skill 清单 | 冻结 |
| `07_FRONTEND_IDE.md` | 前端 IDE | 面板布局、Viewport/Hierarchy/Inspector/Assets/Console/Chat、节点图、设置页、快捷键 | 冻结 |
| `08_ASSET_PIPELINE.md` | 素材处理管线 | 导入/构建/缓存/引用管理、`.meta` 与 GUID、图像/3D 生成 API 预留接口 | 冻结 |
| `09_ENTITY_SCENE_MODEL.md` | 实体与场景模型 | Entity-Component 模型、组件注册表、prefab、场景序列化 `.rxscene` | 冻结 |
| `10_INTERACTION_LOGIC.md` | 交互逻辑设计 | 节点图(Blueprint-lite)+ `.rx` 脚本双轨、agent 生成路径、事件模型 | 冻结 |
| `11_API_CONTRACTS.md` | API 与数据契约 | HTTP/SSE 路由、事件信封、DTO、错误码表 | 冻结 |
| `12_SECURITY_PERMISSIONS.md` | 安全与权限 | 权限模式、proposal 确认、checkpoint/回滚、资产安全 | 冻结 |
| `13_ROADMAP.md` | 里程碑路线图 | F0–F6 分期、验收门、依赖序 | 指导性 |
| `14_DECISION_LOG.md` | 决策日志 | D-001 起全部架构决策与驳回项 | 滚动追加 |

## 3. 调研来源(设计依据)

| 域 | 来源仓库/产品 | 取用内容 | 详见 |
|---|---|---|---|
| 渲染器 | `H:\rurix` `src/rurix-render`(G5) | 声明式 render graph、虚拟化几何(meshlet/VisBuffer)、VSM、探针 GI、RT 效果、材质流送、TAA/TSR | `03 §2` |
| 物理引擎 | `H:\rurix` `src/rurix-physics`(G6.2) | `PhysicsWorld` 固定步、Jolt 默认/Rapier 次后端、不透明 BodyId/ShapeId、查询面、接触事件、SyncBudget、渲染合流桥 | `03 §3` |
| 几何构建 | `H:\rurix` `src/rurix-geom-build` | 网格→meshlet 化→分组简化层级 DAG,离线确定性构建 | `03 §4`、`08 §4` |
| Agent 后端 | `D:\agent-cowork` `backend-rs`(agentd) | 七 crate 划分、三类 agentKind、composer 五模式、plan/todo 引擎、swarm 协调器、subagent profile、记忆、checkpoint、MCP 客户端 | `04` 全篇 |
| IDE 模块 | `D:\agent-cowork` `rust/apps/agent-ide` | chat/composer/workbench/inspector/settings 面板划分、底部面板(Problems/Output/Terminal/Logs/Metrics) | `07 §5` |
| 设置页 | `D:\cindy` `apps/desktop` | tab 单一事实源(tabLabels 模式)、URL 深链、每域一个 Section 组件、main 端 per-domain JSON settings-store、legacy 别名重定向 | `07 §7` |
| 素材/实体模块 | Unreal Engine 5 | Content Browser、World Outliner、Details Panel、Blueprint、Property Matrix、redirector 修复 | `08 §2`、`09 §2`、`10 §2` |
| 素材/实体模块 | Unity | Hierarchy、Inspector(component section + property drawer)、Project window、Prefab(覆盖/应用/隔离编辑)、Play/Pause/Step | `08 §2`、`09 §2`、`10 §2` |

## 4. 术语表

| 术语 | 定义 |
|---|---|
| `engine-host` | 独立 Rust 进程,以库形式链接 rurix-render + rurix-physics,承载场景运行时与渲染,经控制/帧流/事件三通道对外服务(`03 §5`) |
| `assetd` | 素材管线服务进程,封装导入器、rurix-geom-build、纹理处理、缓存与引用图(`08 §1`) |
| `forge-agentd` | Agent 内核进程(Rust,移植自 agent-cowork agentd),会话/plan/swarm/工具/MCP 宿主(`04 §1`) |
| `forge-gateway` | Go 边缘网关,CORS/JWT/反向代理/WS→SSE 桥(`02 §2`) |
| `MCP 工程` | 一组领域化 MCP server,agent 的一切重复性操作经其工具面执行(`05` 全篇) |
| `Entity` | 场景对象,id + 名称 + Transform + 组件列表(对标 UE Actor / Unity GameObject,`09 §1`) |
| `Component` | 挂接在 Entity 上的能力单元(渲染/物理/灯光/脚本…),注册表驱动序列化与 Inspector(`09 §3`) |
| `Prefab` | 可复用 Entity 模板 `.rxprefab`,支持实例覆盖与回写(`09 §5`) |
| `PIE` | Play-In-Editor,编辑器内运行当前场景(`03 §5.4`) |
| `composer 模式` | 会话执行模式:build / plan / debug / ask / multitask / team / ultraplan / design(`04 §3`、E-04-008) |
| `swarm` | agent 集群:协调器 + 节点注册 + 分片派工(`04 §5`) |
| `skill` | `skills/<name>/SKILL.md` 形式的可复用操作规程,agent 按需读取(`06` 全篇) |
| `Proposal` | agent 发起的需人类确认的变更单(破坏性/外部可见操作),`12 §3` |
| `Checkpoint` | 可回滚的工作区/场景快照,`12 §4` |
| `.rxscene` / `.rxprefab` / `.meta` | 场景/预制体/资产元数据序列化格式,`09 §6`、`08 §3` |

## 5. 全局不变量(任何实现不得违反)

- **I-1 渲染/物理内核唯一**:只使用 rurix 仓库 `rurix-render` 与 `rurix-physics` 的已有实现,不自研第二渲染器/物理引擎;新渲染/物理能力一律先回馈 rurix 上游或在其公开 API 上组合。
- **I-2 agent-first**:一切可程序化描述的操作必须存在 MCP 工具面;前端 UI 不得成为任何批量/重复操作的唯一入口。
- **I-3 UI 极简**:前端常驻面板集合固定为 `07 §1` 的七区;新增面板须决策日志批准。
- **I-4 单一事实源**:场景事实源 = `.rxscene` 文件;资产事实源 = 源文件 + `.meta`;agent 记忆事实源 = forge-agentd store;配置事实源 = 设置 store JSON。任何派生态(引擎内场景、缓存)必须可重建。
- **I-5 不静默回退**:引擎/后端能力缺失时返回结构化错误(对标 rurix P-01),不降级为隐式近似行为。
- **I-6 人类确认门**:删除/覆盖/批量重写/外部发送/版本控制写操作必须经 Proposal 确认(`12 §3`)。
- **I-7 生成素材溯源**:AI 生成的图像/模型落地时必须写 provenance 元数据(`08 §6.4`)。
