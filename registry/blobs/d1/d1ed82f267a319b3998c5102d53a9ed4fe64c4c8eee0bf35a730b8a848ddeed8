---
name: scene-audit
description: 场景健康度审计。当任务涉及「审计场景 / 检查场景问题 / 场景体检 / 交付前自检 / scene audit」时使用。
version: 1.0.0
license: CC0-1.0
tags: [场景, 审计, 交付自检]
allowed-tools: [mcp__engine-scene__scene_summary, mcp__engine-scene__scene_index, mcp__engine-scene__entity_list, mcp__engine-scene__entity_get, mcp__engine-scene__scene_graph_dump, mcp__asset-pipeline__asset_list, mcp__asset-pipeline__asset_refs, mcp__asset-pipeline__asset_build_status]
---

# 场景健康度审计

交付前对当前场景做一次只读体检，产出问题清单与优先级，不做任何修改。

## 目标

用一次遍历回答四个问题：场景里有没有孤儿实体？有没有引用了不存在资产的组件？有没有资产处于 stale/failed 构建态？实体分类（role/map/interaction）是否完整。

## 必须遵守

- **全程只读**。本 skill 不调用任何写工具；发现问题只报告，修复交回主循环由用户决定。
- **不猜测**。每条结论必须指向具体实体名或资产路径，禁止「可能存在若干问题」这类无锚点表述。
- **不越域**。代码诊断（`.rx` 编译错误）、性能预算、玩法回归分别属于 `code-rx-migration`、`perf-budget-check`、`playtest-regression`，本 skill 不顺手做。

## 执行流程

1. `mcp__engine-scene__scene_summary` 取基线：实体总数、组件计数、dirty 标记、playState。若场景未加载则如实报告 `SCENE_NOT_LOADED` 并终止。
2. `mcp__engine-scene__scene_index` 取三类分组（role / map / interaction），记录未分类实体数。
3. `mcp__engine-scene__entity_list` 全量列举，对每个带 `MeshRenderer` 的实体读取其 `mesh` 与 `material` GUID。
4. `mcp__asset-pipeline__asset_list` 取资产全集，建 GUID 集合；步骤 3 收集的 GUID 逐个比对，不在集合内的记为**断链引用**。
5. `mcp__asset-pipeline__asset_build_status` 取构建态，筛出 `stale` 与 `failed` 两类。
6. `mcp__engine-scene__scene_graph_dump` 检查层级，筛出无父无子且无任何组件的**孤儿实体**。
7. 汇总为三级清单：`blocker`（断链引用、failed 构建）/ `warn`（stale 构建、未分类实体）/ `info`（孤儿实体、空组件槽）。

## 输出约束

- 固定四段：**基线数字** → **blocker 清单** → **warn 清单** → **info 清单**。
- 每条问题一行，格式 `[级别] <实体名或资产路径> — <一句话事实>`，不带修复建议（建议单独列在末尾，与事实分开）。
- 零问题时明确写「本次审计未发现问题」，并附上实际检查的实体数与资产数作为佐证，不留空泛结论。
- 数字一律来自工具返回，禁止估算。

## 失败回退策略

- 任一只读工具调用失败：如实记录该步骤失败与错误码，跳过该维度继续其余检查，最终报告中标注「本次审计缺失 X 维度」，不伪装成全绿。
- 场景为空（实体数 0）：直接报告空场景，不进入后续步骤。
- 资产列表为空但场景有 `MeshRenderer`：这本身是 blocker，如实报告而非判为工具故障。
