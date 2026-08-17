---
name: logic-blueprint-gen
description: 交互逻辑生成(节点图)。当任务涉及「做交互 / 触发开门 / 节点图 / 逻辑生成」时使用。
---

# logic-blueprint-gen · 交互逻辑生成

> SEAM 标注:依赖 code-forge 工具面(rx_check / 节点图校验,F4 交互逻辑承接,未落地),当前不可执行。

## 目标
把交互需求生成为 .rxgraph 节点图,校验、挂载、playtest 验证全链走通。

## 必须遵守
- 生成的图必须过 schema 校验(10 §4),校验失败不挂载。
- 等价性:生成图必须与需求描述逐点对应,不多不少。

## 分步骤执行流程(F4 工具面落地后生效)
1. 需求 → 节点图 JSON(事件/条件/动作节点 + 连线,10 §4 schema)。
2. rx_check 等价校验 + 图校验器;不通过 → 修图重来。
3. 挂载 ScriptComponent(实体 component_set)。
4. playtest 注入断言验证(触发 → 预期状态变化)。
5. 报告:图 JSON 摘要 + 校验结果 + playtest 断言结果。

## 输出约束
- 图必须可解释执行;挂载后人工在图上改常量须生效(NodeGraph 面板路径)。

## 失败回退策略
- 校验/playtest 失败:卸载组件,报告失败断言详情,不遗留半截图。
