---
name: logic-blueprint-gen
description: 交互逻辑生成(节点图)。当任务涉及「做交互 / 触发开门 / 节点图 / 逻辑生成」时使用。
---

# logic-blueprint-gen · 交互逻辑生成

## 目标
把交互需求生成为 .rxgraph 节点图,校验、挂载、playtest 验证全链走通。

## 必须遵守
- 生成的图必须过 graph_validate 校验(10 §4),校验失败不挂载。
- 等价性:生成图必须与需求描述逐点对应,不多不少。
- 不交付未验证逻辑:playtest 断言不过 = 未完成,不得充绿。

## 分步骤执行流程(F4 wave.3 工具面已落地)
1. 需求 → 节点图 JSON(事件/条件/动作节点 + 连线,10 §4 schema;节点类型以注册表为准,
   仅 10 §4.2 首发冻结子集 40 节点可用)。
2. `mcp__code-forge__graph_validate` 校验;不通过 → 修图重来。
3. `mcp__code-forge__graph_create` 落盘(Content/Graphs/<name>.rxgraph)。
4. 挂载:实体 `mcp__engine-scene__component_add`/`component_set` 写 Script 组件
   { module:"", graphRef:"Content/Graphs/<name>.rxgraph", props:{…暴露属性覆盖…} }。
   需要触发区 → 同实体加 Trigger{kind:"box",extents:[x,y,z]};标签判别 → 目标实体加
   Tag{tag:"…"};物理参与 → RigidBody{kind:static|dynamic|kinematic, mass}。
5. playtest 注入断言:
   - `play_enter` → `play_pause` → `host_events_drain` 断言 logic.start(on_start);
   - 触发注入:`transform_set` 移动实体进/出 Trigger AABB,或 `logic_inject_input`
     {action,value} 注入输入;
   - `play_step` 单帧推进 → `host_events_drain` 按真实事件序断言(logic.input →
     logic.contact → logic.trigger → logic.timer → logic.update → logic.message;
     logic.log 为 debug.log 节点输出;logic.unsupported 出现即图含未实现节点,须如实报告);
   - `transform_get`/`component_get` 断言最终状态(如四元数换算 yaw);
   - play 态 `component_set` Script props = 热重载(on_start 重发 + 黑板重置),可断言
     「人工改常量生效」。
6. 报告:图 JSON 摘要 + 校验结果 + playtest 断言结果(真实事件序/真实数值)。

## 输出约束
- 图必须可解释执行;挂载后人工在图上改常量(props 覆盖)须生效。
- 未实现节点(physics.*/audio.*/spawn/destroy/look_at/lerp/for_each/gate/draw_debug_line)
  不得在图中当已实现使用;运行出现 logic.unsupported 必须写进报告。
- `call.call_function` 已实现(RD-F4-004),是图调用 .rx 代码的唯一入口:module = 项目根相对的
  .rx 路径、fn = 脚本里 `#[export(c)] pub fn` 的函数名(两者须为常量),args 为数组,返回值从
  result pin 取;graph_validate 会核对 module 文件存在、fn 已导出、签名相符。运行期构建/加载/
  调用失败发 logic.call_error(该次调用未生效,链继续),同样必须写进报告。
- 运行时只加载 Script.graphRef:只填 module、不填 graphRef 的 Script 在 play 态不会执行。

## 失败回退策略
- 校验/playtest 失败:卸载组件(component_remove Script),报告失败断言详情,不遗留半截图。
- 参考实现:scripts/f4-w3-logic-smoke.ps1(G-F4-3 触发开门全链,含规范序/热重载/接触断言)。
