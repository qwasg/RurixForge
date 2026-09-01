---
name: scene-dressing
description: 场景装饰摆放(植被/道具散布)。当任务涉及「散布 / 摆装饰 / 植被 / 道具摆放」时使用。
---

# scene-dressing · 场景装饰散布

> SEAM 标注:依赖 physics_overlap / cast_ray 采样工具面(引擎物理查询波承接,未落地),当前不可执行;落地前可用「固定网格布局」退化模式人工执行,但不得声称走了采样流程。

## 目标
按密度/规则在指定区域批量摆放装饰资产,落地贴合地面。

## 必须遵守
- 先查询后修改(06 §4):`mcp__engine-scene__scene_summary` + `mcp__asset-pipeline__asset_list` 确认区域与可用资产。
- 大干先快照:`mcp__engine-scene__scene_checkpoint`;>50 实体升 Proposal(06 §4)。

## 分步骤执行流程(采样工具落地后生效)
1. 采样区域:physics_overlap 圈定范围,cast_ray 逐点求落地高度。
2. 按密度/规则生成点位清单(避让既有实体)。
3. `mcp__engine-scene__entity_batch_apply` 批量实例化(prefab/网格资产 GUID)。
4. `mcp__engine-scene__viewport_frame` 截图抽检:密度均匀、无悬浮/穿插。
5. 报告:点位数/资产清单/抽检结论。

## 输出约束
- 采样数据(点数/落地命中率)如实报告;悬浮/穿插点位清单单独列。

## 失败回退策略
- 采样失败或抽检不达标:`mcp__engine-scene__scene_rollback` 回滚,报告失败原因;不部分提交。
