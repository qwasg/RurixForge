---
name: scene-greybox
description: 白盒搭建关卡空间。当任务涉及「搭关卡 / 摆白盒 / blockout / 灰盒」时使用。
---

# scene-greybox · 关卡白盒搭建

## 目标
按设计意图用盒体/ primitives 快速搭出可行走的关卡空间骨架。

## 必须遵守
- 先查询后修改(06 §4):动手前 `mcp__engine-scene__scene_summary` + `mcp__engine-scene__entity_list` 读现状。
- 大干先快照:批量摆放前 `mcp__engine-scene__scene_checkpoint`。
- 批量操作优先原子批量工具,不逐实体循环单调。

## 分步骤执行流程
1. 读设计意图(房间数/尺寸/分区);`mcp__engine-scene__scene_summary` 确认当前场景态。
2. `mcp__engine-scene__scene_checkpoint` 建快照。
3. 规划分区:每个房间/走廊一个盒体,地面/墙体/障碍分类命名(`gb_地面_01` / `gb_墙_01`)。
4. `mcp__engine-scene__entity_batch_apply`(create 批)建全部盒体;`mcp__engine-scene__transform_batch_set` 摆位(墙体厚 0.5、高 3,门洞留 2×2.2)。
5. `mcp__engine-scene__viewport_frame` 截图自检:盒体齐全、无穿插、比例合理。
6. 有问题 → `mcp__engine-scene__scene_rollback` 回滚重摆;OK → `mcp__engine-scene__scene_save`。

## 输出约束
- 报告:实体清单(名称/id/位置)+ 自检结论 + 截图证据;问题清单单独列,不混入「完成」叙述。

## 失败回退策略
- 任一步失败:`mcp__engine-scene__scene_rollback` 回到快照,如实报告失败步与错误,不掩盖。
