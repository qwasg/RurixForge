---
name: perf-budget-check
description: 性能预算核查。当任务涉及「帧率低 / 卡顿 / 性能 / 预算核查」时使用。
---

# perf-budget-check · 性能预算核查

> SEAM 标注:依赖帧统计事件流与 render_get_settings 基线工具(F6 Metrics 面板/渲染波承接,未落地);mesh_inspect 热点分析子流程当前可执行。性能数字必须实测(measured),禁止估算充数。

## 目标
对照预算核查帧统计,定位热点,产出优化提案(不改代码,只提案)。

## 必须遵守
- 数字必须来自命令/事件输出(measured),无证据的阈值一律 estimated 占位并如实标注。
- 优化提案只列不改:性能修改走正式波次,本 skill 只诊断。

## 分步骤执行流程
1. 帧统计收集:Metrics 帧统计事件(F6 落地);当前 seam,如实标注不可用。
2. 热点分析:`mcp__asset-pipeline__mesh_inspect` 读网格统计(v/t/meshlets/lods/bounds),找超重资产。
3. 渲染设置基线:render_get_settings(渲染波 seam)。
4. 优化提案:按热点排序(资产减面/LOD/贴图尺寸/灯光数),每条附预期收益(estimated 标注)。
5. 报告:实测数字 + 热点清单 + 提案。

## 输出约束
- 报告内 measured 与 estimated 必须分开标注,禁止估算混充实测。

## 失败回退策略
- 数据面不可用(帧统计 seam)→ 报告哪些指标不可得,只就已得指标(mesh_inspect)给结论。
