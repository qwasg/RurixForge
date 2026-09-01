---
name: material-tuning
description: 材质/灯光参数调整。当任务涉及「调材质 / 调灯 / 太亮 / 太暗 / 颜色不对」时使用。
---

# material-tuning · 材质灯光调参

> SEAM 标注:依赖 render_get_settings / render_set 渲染设置工具面(渲染波承接,未落地);组件级灯光/材质参数子流程(component_get/set)当前可执行,渲染全局参数流程不可执行。

## 目标
以基线→单变量→截图对比的纪律收敛材质/灯光参数,不盲调。

## 必须遵守
- 先取基线:任何调整前 `mcp__engine-scene__component_get` 记录当前值(全局渲染参数待 render_get_settings 落地)。
- 单变量:一次只改一个字段,改完截图对比。
- 资产引用改动后 `mcp__asset-pipeline__asset_refs` 验证引用边。

## 分步骤执行流程
1. 基线:`mcp__engine-scene__component_get`(Light/MeshRenderer)+ `mcp__engine-scene__viewport_frame` 截图存档。
2. 单变量改:`mcp__engine-scene__component_set` 改一个字段(如 intensity ±20%)。
3. `mcp__engine-scene__viewport_frame` 截图,与基线对比;向目标收敛则继续,恶化则回退该步。
4. 材质资产侧:`mcp__asset-pipeline__material_create` / `mcp__asset-pipeline__texture_process` 调整后,`mcp__asset-pipeline__asset_refs` 验证材质→贴图边。
5. 收敛报告:每步(字段/旧值/新值/截图对比结论)+ 最终参数。

## 输出约束
- 每步调整必须有截图对比证据;未收敛如实报「未收敛」并给当前最优值。

## 失败回退策略
- 任何一步恶化超基线:回退该字段到基线值;连续 3 步不收敛 → 停止并报告,附已试参数表。
