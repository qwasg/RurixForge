# 01 · 产品定位与设计原则

## 1. 定位

RurixForge 是一套 **AI 主导(all-in-AI)的游戏制作引擎**:

- 运行时内核 = rurix 已有渲染器与物理引擎(`03` 全篇),Windows 原生 + NVIDIA 单栈起步。
- 第一操作者 = agent 集群。素材处理、代码修改、场景批量搭建、重复性调整等
  **全部可由 agent 经 MCP 工程完成**,人类不经手也可产出可玩游戏原型。
- 人类角色 = 导演与验收者:提出意图、在视口中直观确认、微调关键对象、
  批准破坏性操作、验收 agent 产物。

非目标(本期不做):

- 不做移动端/Web 发布(继承 rurix 现状:Windows + NVIDIA 优先;Vulkan 跨端后端为 rurix preview,不在本引擎承诺面)。
- 不做完整 DCC 工具(网格建模、骨骼绑定、贴图绘制),只做素材的导入/转换/组装/生成接入。
- 不做多人协同编辑(单人 + agent 集群;git 兜底协作)。
- 不重新发明 agent 运行时(照搬 agent-cowork agentd 架构与 coding 模式,`04` 全篇)。

## 2. 设计原则

- **P-1 内核复用**:渲染/物理只用 rurix 已有实现(不变量 I-1)。引擎层新需求优先组合既有 API(`GpuScene`/`PhysicsWorld`/render graph),确需新内核能力时立项回馈 rurix。
- **P-2 agent 是一等公民**:引擎每个子系统的设计评审必答「agent 如何经 MCP 完成此操作」。没有工具面的功能视为未完成(I-2)。
- **P-3 UI 只做必须**:前端仅承载「必须直观进行」的工作:场景可视确认、对象点选与微调、素材预览与挑拣、逻辑关系审阅、agent 对话与批准。其余一律 MCP + agent(I-3)。
- **P-4 文本即资产**:场景 `.rxscene`、prefab、节点图、导入设置全部 JSON/TOML 文本序列化,可 diff、可 code review、agent 可直接读写(经 MCP,不裸写)。
- **P-5 确定性可重放**:物理固定步、资产构建确定性、agent 操作记事件日志,任意会话可 replay(继承 agentd replay 能力)。
- **P-6 失败显式化**:能力缺失/参数非法/后端未编译一律结构化错误返回,禁止静默降级(I-5,对标 rurix P-01)。
- **P-7 人类有最终否决权**:一切破坏性、外部可见、批量性操作经 Proposal 确认门(I-6)。

## 3. 用户与核心工作流

目标用户:独立游戏开发者 / 小型团队技术负责人,具备游戏概念但不愿在
素材整理、样板代码、重复摆放上消耗时间。

### 3.1 工作流 WF-1「一句话起项目」

1. 用户在 Chat(composer `build` 模式)输入:「做一个第三人称迷宫探索原型,低多边形风格」。
2. agent 经 `project-mcp` 建项目骨架 → `asset-pipeline-mcp` 建目录规范 → `code-forge-mcp` 建 `.rx` 脚本骨架 → `engine-scene-mcp` 搭初始场景。
3. 用户在 Viewport 查看结果,在 Chat 继续迭代(「迷宫再密一点」「加拾取物」)。
4. 验收:Play 模式试玩(`playtest-mcp` 供 agent 自测)。

### 3.2 工作流 WF-2「素材处理」(必须直观)

1. 用户拖入 FBX/贴图(或让 agent 批量导入目录)。
2. Assets 面板预览网格/材质;用户在 Inspector 调导入设置(法线、压缩、meshlet 预算)。
3. agent 经 `asset-pipeline-mcp` 批量构建(meshlet DAG、纹理压缩),失败项进 Problems。
4. 引用关系(谁用了这张贴图)经引用图工具查询,删除前 agent 自动检查并提案。

### 3.3 工作流 WF-3「场景设置」(必须直观)

1. Hierarchy 树中点选 Entity,Inspector 显示组件;Transform gizmo 视口微调。
2. 批量操作(「把这一层所有灯的强度减半」「沿曲线摆 50 个路灯」)一律交给 Chat → agent → `engine-scene-mcp` 批量工具,UI 不提供批量编辑窗。
3. Prefab:选中 Entity → 「存为 Prefab」;实例覆盖在 Inspector 标蓝,可 Apply/Revert(对标 Unity Prefab 工作流)。

### 3.4 工作流 WF-4「交互逻辑设计」(必须直观)

1. 用户在节点图面板审阅 agent 生成的逻辑图(Blueprint-lite,`10 §2`),可拖线微调、改常量。
2. 复杂逻辑由 agent 写 `.rx` 脚本(code-forge-mcp),节点图与脚本互绑:节点图负责事件编排,脚本负责计算。
3. 运行期事件(接触 Begin/Persist/End)在 Console 面板可观测,debug 模式 agent 直接读事件流定位问题。

### 3.5 工作流 WF-5「AI 生成素材」

1. 用户在 Chat 描述所需贴图/模型(或在 Assets 面板右键「生成」)。
2. agent 经 `gen-image-mcp` / `gen-model-mcp`(预留接口,`08 §6`)调用生成后端,产物自动入管线、写 provenance。
3. 用户在 Assets 面板从候选中挑拣、打回重生成。

## 4. 红线

- **R-1** 永不把 agent 工具面做成「截图+模拟点击」前端自动化;一切操作走引擎真实 API。
- **R-2** 永不在前端实现批量编辑窗、批导窗、命令面板之外的「第二操作面」——批量 = agent 职责。
- **R-3** 永不绕过 Proposal 直接执行破坏性操作,包括「用户看着像无害」的批量覆盖。
- **R-4** 永不在引擎层引入第二个渲染/物理后端;跨端等待 rurix MB 系列上游收口。
- **R-5** 生成式 API 密钥只存后端 keystore,不进前端、不进场景/资产文件(I-7 配套)。
