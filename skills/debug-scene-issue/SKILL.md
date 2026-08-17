---
name: debug-scene-issue
description: 场景问题诊断(debug 模式)。当任务涉及「不亮 / 没阴影 / 穿模 / 跳不起来 / 场景 bug」时使用。
---

# debug-scene-issue · 场景问题诊断

## 目标
用三件套(图/事件/图转储)收集证据,假设排序,单变量验证,给出根因与修复。

## 必须遵守
- 先证据后结论:没有三件套数据不下任何根因判断。
- 单变量验证:一次只改一个变量,改完即验,不连环改。
- 修复后必回归断言:组件态 + PIE 双态(play_enter/step/exit)无错。

## 分步骤执行流程
1. 收集三件套:`mcp__engine-scene__viewport_frame`(截图)+ `mcp__engine-scene__host_events_drain`(场景域内存事件环,排空式)+ `mcp__engine-scene__scene_graph_dump`(实体+组件全量快照)。(`mcp__engine-scene__host_events` 是 supervisor 崩溃/重启日志,仅怀疑 host 稳定性时加取。)
2. 假设排序:按证据列候选根因(如「第三盏灯没阴影」→ graph_dump 查该灯 castShadow/intensity/enabled)。
3. 单变量验证:对最可能根因做只读核对(`mcp__engine-scene__component_get`)。
4. 修复:`mcp__engine-scene__component_set` 改单字段(如 castShadow=true)。
5. 回归断言:`mcp__engine-scene__component_get` 复核字段生效;`mcp__engine-scene__play_enter` → `mcp__engine-scene__play_step` → `mcp__engine-scene__play_exit` 双态无错;`mcp__engine-scene__viewport_frame` 修复后截图对比。
6. 报告:根因 + 证据链 + 修复内容 + 断言结果。

## 输出约束
- 根因必须附证据(graph_dump 字段值/截图/事件),禁止无证据断言;修不了如实说并给替代建议。

## 失败回退策略
- 修复引入新问题:`mcp__engine-scene__edit_undo` 撤销,回到假设列表下一个候选;全部假设证伪 → 报告「未定位」并附已排除清单。
