# 浏览器全量用户交互测试 + 真实全项目制作 Spec

## Why

F8 已交付 Playwright 落仓八任务矩阵（G-F8-4），但覆盖的是抽样式关键链路；IDE 的「可商用」还差最后一里：**以真实用户视角在内置浏览器中点遍全部交互面**，并用这套交互**从空项目真实制作出一个完整可玩项目**（建场景→摆实体→材质→逻辑→试玩→打包），全程截图留证。这既是全量 UI 回归，也是端到端产品价值实证。

## What Changes

- 新增一轮**内置浏览器（TRAE-browseruse）驱动的全量交互测试**：壳/设置/workbench/Inspector/编辑器各区逐面点击+截图，禁用态与降级态如实核验（不充绿）。
- 新增**真实全项目制作 journey**：在浏览器 UI 中完成一个新项目从空到可打包的全流程（场景/实体/材质/逻辑图/保存/PIE/playtest/project-pack），每步截图。
- 新增 evidence 留档：`evidence/f9-journey-*`（截图 + 汇总 JSON）。
- 测试基建复用 F8 成果（host:3080 浏览器通路、canvas readback 回退腿、服务编排纪律），**不修改产品代码**；发现产品 bug 如实登记不私修。

## Impact

- Affected specs: F8 G-F8-3/G-F8-4 浏览器通路的扩展层（本 spec 不改既有门禁，只加实证）
- Affected code: 仅新增 `evidence/f9-journey-*` 与 spec 文档；projects/ 下新增 journey 项目资产（真实制作的产物，属用户数据面）
- 前置依赖: client dist 已构建、agentd(8103)/host(3080) 运行、内置浏览器可用

## ADDED Requirements

### Requirement: 全量交互巡检
系统 SHALL 在内置浏览器中对 IDE 全部一级交互面完成真实点击核验并逐面截图：侧栏（会话 CRUD/置顶/文件夹/搜索）、TitleBar（菜单/搜索胶囊，浏览器态窗口钮已隐藏）、命令面板（Ctrl+K）、toast、StatusBar、设置五页（外观/Agent/模型/技能/关于）、workbench tabs（plan/todo/diff/editor）、底部面板（Agent Logs/Output/Metrics）、Inspector 工作区树+文件预览、ChatColumn/Composer 全量控件。

#### Scenario: 逐面点击核验
- **WHEN** 巡检每个交互面
- **THEN** 该面响应符合预期（真实数据渲染或 F8 定型诚实禁用态），零白屏零未捕获异常，截图落档

#### Scenario: 禁用态核验
- **WHEN** 在浏览器环境点击 Electron 专有面（资产导入/在文件夹显示）
- **THEN** 呈现 F8 定型诚实禁用态（disabled+说明），不崩溃不伪造

### Requirement: 真实全项目制作 journey
系统 SHALL 支持用户在浏览器 UI 中完成一个完整项目制作闭环：新建场景→经聊天 agent 与编辑器创建并摆放实体→赋予材质贴图→挂载逻辑图（触发开门类）→保存场景→PIE 试玩→playtest 断言→project-pack 打包产物。

#### Scenario: 全项目闭环
- **WHEN** 用户按 journey 逐步操作（每步截图）
- **THEN** 每步后端状态可核验（entity_list/asset_refs/scene 保存文件/playtest 报告/pack 产物清单），最终打包产物目录真实存在且含引用闭包

#### Scenario: 步骤失败诚实登记
- **WHEN** journey 任一步骤失败或降级
- **THEN** 如实登记 FAIL/降级原因与截图，不跳过不伪造，汇总 JSON 标注

### Requirement: evidence 留档与汇总
系统 SHALL 将全部截图与逐面/逐步 verdict 汇总为机器可读 JSON（`evidence/f9-journey-*`），含页面错误/控制台错误计数、失败项根因。

#### Scenario: 汇总报告
- **WHEN** 巡检与 journey 完成
- **THEN** 汇总 JSON 含每面/每步独立布尔 verdict、截图路径、pageErrors/consoleErrors 计数、失败根因

## MODIFIED Requirements

无（不改既有门禁与产品行为）。

## REMOVED Requirements

无。
