---
name: rig2d-animate
description: 2D 骨骼角色动作与挂接:为 .rxrig 写动作 clip(待机/说话/挥手)与 animator 状态机,用 rig_spawn 挂进场景,用节点图 rig.set_param / rig.play / animator.set_trigger 驱动(视线跟随、说话嘴型、受击),PIE 试玩验收。当任务涉及「骨骼角色动起来 / Live2D 动作 / rig 动画 clip / 表情切换 / 视线跟随鼠标 / 把绑好的角色放进场景」时使用。
tags: [2d, rig, animation, live2d-style]
---

# rig2d-animate · 2D 骨骼角色动作与挂接规程

## 目标
在已通过 `rig2d-build` 校验的 `.rxrig` 上写出可循环的动作 clip 与(按需)animator 状态机,
挂进当前场景并由节点图驱动,PIE 试玩确认动作、自动眨眼呼吸、头发物理正常,落盘交付。

## 必须遵守
- **先查询后修改**:`mcp__asset-pipeline__rig_get { sections:["parameters","bones","clips","animator"] }` 取真实参数名、
  骨骼名与已有 clip;`mcp__engine-scene__scene_summary` 确认 mode=`2d` 与 playState=`edit`。
- clip 轨道只写已存在的目标:`param:<参数id>`(绝对值)、`bone:<骨骼id>.rotation|x|y|sx|sy`(相对静止的增量)、
  `part:<部件id>.opacity`(乘子)。**优先用参数轨道**(参数会带动 keyform、物理一起联动),骨骼轨道只用于模板外的大动作。
- 自动眨眼 / 呼吸 / 物理在运行时叠加在 clip 之上:clip 里**不要**再写眨眼与呼吸轨道,否则双重叠加。
- 改场景前 `mcp__engine-scene__scene_checkpoint`;自己进过 play 态必须 `play_exit` 再收尾。
- 模式二选一(与 .rxsprite 相同,play 开始时判定):`Rig2D.clip` 留空且 rig 带 animator → **FSM 模式**
  (状态机独占 clip 选择,`rig.play` 会被忽略并发 `rig.warn`);`clip` 写了名字 → **手动模式**。

## 分步骤执行流程
1. **取证**:`rig_get` 列出参数与范围、clip、animator;记下要用的参数名。
2. **写 clip**(`mcp__asset-pipeline__rig_patch` 的 `set_clip`,先 `dryRun`):
   - `idle`(3–4 秒,loop):`ParamAngleX` 在 ±5 内缓慢摆动、`ParamAngleZ` ±3、`ParamBodyAngleX` ±2,首尾关键帧值相同保证无缝循环,
     `interp:"smooth"`。
   - `talk`(0.6 秒,loop):`ParamMouthOpenY` 0→1→0.3→0.8→0,`interp:"linear"`。
   - 一次性动作(如 `wave`,1.2 秒,`loop:false`):`bone:arm_r.rotation` 0→-60→-40→-60→0。
   - 每个 clip 写完用 `mcp__asset-pipeline__rig_preview { clip, time }` 在 0 / ¼ / ½ / ¾ 时长各出一帧核对。
3. **状态机(按需)**:`set_animator`——`defaultState:"idle"`,参数如 `talking: bool`、`wave: trigger`,
   转换 `idle→talk when talking=true`、`talk→idle when talking=false`、`Any→wave when trigger wave`、`wave→idle hasExitTime`。
   `mcp__asset-pipeline__rig_validate` 必须 ok。
4. **挂进场景**:`mcp__engine-scene__scene_checkpoint` → `mcp__engine-scene__rig_spawn { name, rig:<GUID>, position:[x,y,0],
   sortingOrder, clip? }`(FSM 模式 clip 留空;手动模式写 `"idle"`)。编辑态想摆初始姿势可用
   `mcp__engine-scene__component_set` 写 `Rig2D.params`(PIE 开始时由宿主接管,不影响运行态)。
5. **逻辑驱动**(需要时,遵循 `logic-blueprint-gen` 的校验与挂载流程):
   - 视线跟随:`on_update` 里取指针/目标位置换算到 [-1,1] → `rig.set_param { param:"ParamEyeBallX", value, blend:0.2 }`,
     同理 `ParamAngleX`(×30)——`blend` 让参数平滑趋近,避免抖动。
   - 说话:FSM 模式 `animator.set_bool { param:"talking", value:true }`;手动模式 `rig.play { clip:"talk" }`。
   - 一次性动作:`animator.set_trigger { param:"wave" }`。
   - 图用 `mcp__code-forge__graph_validate` 校验后 `mcp__code-forge__graph_create` 落盘,Script 组件挂到该实体。
6. **试玩验收**:`mcp__engine-scene__play_enter` → 推进若干帧 → `mcp__engine-scene__viewport_frame` 截图核对:
   角色在动、约 2–6 秒内至少眨眼一次、头发随头部摆动有滞后;`mcp__engine-scene__host_events_drain` 里出现 `rig.warn`
   必须逐条处置(未知参数/clip、FSM 模式下误用 rig.play 等)。`mcp__engine-scene__play_exit`。
7. **落盘**:`mcp__engine-scene__scene_save { path }`。

## 输出约束
- 报告:rig GUID、clip 清单(名字 / 时长 / loop / 轨道目标)、animator 状态与转换、场景实体名与位置、
  驱动方式(模式 + 节点图路径)、试玩截图结论、`rig.warn` 处置结果。
- 动作卡顿、循环接缝跳变、物理过飘等问题单独列清单,不混入「完成」叙述。

## 失败回退策略
- `rig_patch` 校验失败:按错误里的 JSON 路径修正(常见:轨道 target 拼错、关键帧时间未升序、animator 引用不存在的 clip)。
- 场景修改或试玩出错:`mcp__engine-scene__scene_rollback` 回到本次 checkpoint,如实报告失败步与错误原文。
- 画面不动:先查 `Rig2D.playing` 与模式(FSM 模式下 `rig.play` 无效),再查 `host_events_drain` 的 `rig.warn`。
- 当前渲染后端不支持该效果(`render_capabilities` 标为 unsupported / limited)时如实报告,不切换后端、不冒充完成。
