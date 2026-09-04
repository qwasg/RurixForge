---
name: game-2d-kit
description: 2D 游戏制作规程。当任务涉及「做 2D 游戏 / 精灵 / 横版 / 俯视角 / 打砖块 / 平台跳跃 / 角色动画 / 帧动画 / 走路动画」且项目为 2D 模式时使用。
---

# game-2d-kit · 2D 游戏制作规程

## 目标
在 2D 模式项目(forge.toml [project] mode="2d")里,按引擎 2D 约定高质搭建玩法:
XY 平面、正交相机、Sprite 精灵、sortingOrder 叠放、场景级重力、.rxsprite 帧动画。

## 必须遵守
- 先确认模式:`mcp__engine-scene__scene_summary` 看 mode 字段;项目不是 2d 时如实告知用户并停下,不混用 3D 套路。
- 先查询后修改:动手前 entity_list / asset_list 读现状;批量操作前 scene_checkpoint。
- 坐标硬约定:玩法实体一律 XY 平面、z=0;相机实体在 +Z(如 [0,0,10])恒等旋转朝 -Z。

## 分步骤执行流程
1. 选型确认:scene_summary 核对 mode="2d";俯视/零重力玩法在 scene_new 时写 gravity=[0,0,0],侧视(平台/弹射)保持默认。
2. 相机:场景含一个 Camera 组件实体,projection="orthographic";orthoSize = 画面半高(世界米),
   例:竖屏 10 单位高场景 → orthoSize=5。
3. 素材:精灵图透明底/纯色底;尺寸即游戏内基准(pixelsPerUnit 缺省 100,256px = 2.56 米)。
4. 搭建:可见实体用 sprite_create 一步创建(scale=1 即原生尺寸);背景 sortingOrder 小、前景大;
   脚本驱动移动体 RigidBody kind="kinematic",受重力体才 dynamic。
5. 自检:viewport_frame 截图须为正交正视(无透视形变);play_enter 试玩验证后 play_exit;scene_save 落盘。

## 角色帧动画工序(F-GAME-4:图集 → .rxsprite → clip → 场景挂接)

### A. 生成动作表(gen_image 提示词纪律,实证经验)
- **整张表一次生成**:一个角色的多帧/多动作必须在同一次 gen_image 调用里出整张表——逐帧
  单独生成再拼会角色形象漂移(identity 漂移)。提示词写明精确网格(如 "2x2 grid, 4 frames")。
- **纯品红底**:提示词强制 "solid magenta background (#FF00FF), no gradients"(引擎色键
  discard 品红族;主体含品红/粉色时改用 #00FF00 并注明)。帧与帧之间留纯底色隔离带,
  任何肢体/特效不得跨格(containment)。
- 同表全帧同一角色、同一比例、同一朝向(侧视规范朝右);无文字/边框/网格线。
- 帧数判据:身体固有循环运动(走/跑/游/飞)→ 多帧;引擎位移中的持定姿势(跳/落/冲刺)
  → 单帧即可。idle 循环表的末帧应向首帧回摆(reverse closure),循环才无缝。
- 单帧连续动作救急:只有 1 帧动作图时,与 idle 帧组成 2 帧交替 clip(振荡即动感),
  绝不静止持帧。

### B. 切帧入库(asset-pipeline 工具)
1. `gen_accept` 入库贴图 → 记 GUID;
2. `sprite_autoslice { assetPath }` 预览连通域 bbox(噪点多时调高 minArea);
3. `sprite_create { name, texture: <GUID>, autoslice: true }` 建 .rxsprite(帧名 frame_<i>,
   行序从上到下、行内从左到右);
4. `sprite_get` 核对帧数与预期网格一致;帧数不符 = 生成图有粘连/缺帧,回 A 重生成,
   不手工硬凑。

### C. 定义 clip 与锚点(sprite_set)
- clip 形如 `{ "walk": { "frames": ["frame_0","frame_1"], "fps": 8, "loop": true } }`;
  一次性动作 `loop:false, onFinish:"hold"`;帧数将来可能变的 clip 用 `duration`(总秒数)
  替代 fps——换帧数不漂手感。
- pivot 规则:地面角色省略(缺省 [0.5,1] 脚底锚);居中 VFX/飞行体写文档级 [0.5,0.5];
  个别帧尺寸异常用帧级 pivot 微调。
- 多状态角色可加 animator 段(defaultState/parameters/states/transitions,转换表有序
  首匹配,trigger 触发即消费,hasExitTime 等 clip 播完)。

### D. 场景挂接与驱动
- 实体 Sprite 组件写 `sprite: <.rxsprite GUID>`(留空 texture);scale=1 时世界尺寸 =
  帧 bbox 像素 / pixelsPerUnit。
- 模式二选一(play 初始化时按实体 clip 是否为空判定):
  - **FSM 模式**:实体 `clip` 留空(文档须带 animator)→ 状态机从 defaultState 接管;
    图里用 `animator.set_bool` / `animator.set_trigger` 驱动;此模式下 sprite.play 会被
    如实忽略并进 anim.warn 事件。
  - **手动模式**:实体显式写 `clip: "walk"` → 纯 clip 播放;图里用
    `sprite.play { entity, clip }`(缺省幂等,可安全每帧调用)/`sprite.stop` 切换。
- PIE 试玩确认帧在动、脚底贴地不漂移;`events_drain` 里出现 anim.warn 必须逐条处置。

## 输出约束
- 报告:模式确认结论、相机参数(orthoSize)、精灵清单(实体名/贴图或 .rxsprite GUID/
  sortingOrder)、动画清单(clip 名/帧数/fps)、自检截图结论。
- 穿帮/透视/落体异常/动画冻帧等问题单独列清单,不混入「完成」叙述。

## 失败回退策略
- 任一步失败:scene_rollback 回到快照,如实报告失败步与错误,不掩盖、不伪造画面。
- 贴图 GUID 解析失败(精灵渲染成方块):回 asset_list 核对 GUID,修复后重试验证。
