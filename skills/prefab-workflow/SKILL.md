---
name: prefab-workflow
description: 预制体创建/覆盖/回写。当任务涉及「做 prefab / 预制体 / Apply / Revert」时使用。
---

# prefab-workflow · 预制体工作流

> SEAM 标注:依赖 prefab 工具面(prefab_create / prefab_instantiate / prefab_apply / prefab_revert,引擎预制体波承接,未落地),当前不可执行。

## 目标
把选中实体集沉淀为可复用预制体,管理实例覆盖与回写。

## 必须遵守
- 先查询后修改:操作前 `mcp__engine-scene__entity_get` / `mcp__asset-pipeline__asset_list` 确认选中集与既有 prefab。
- 覆盖检查先行:Apply 前必须列出本实例相对 prefab 的覆盖差异清单。

## 分步骤执行流程(工具面落地后生效)
1. 选中集 → prefab_create(落 Content/Prefabs/,自动 .meta + GUID)。
2. prefab_instantiate 生成实例;实例覆盖(组件改值)逐条记录。
3. Apply:覆盖差异回写 prefab 资产,全部实例同步;Revert:丢弃覆盖回 prefab 默认值。
4. 验证:`mcp__asset-pipeline__asset_refs` 查 prefab 引用;实例场景保存后重载一致。
5. 报告:prefab 路径/GUID/实例数/覆盖差异清单。

## 输出约束
- 覆盖差异必须逐条列出(实体/组件/字段/旧值/新值),不笼统报「已同步」。

## 失败回退策略
- Apply 冲突或校验失败:放弃回写,保持 prefab 原值,报告冲突清单。
