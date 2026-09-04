# PvZ 冒险模式复刻设计规约

## 0. 范围、事实源与可执行边界
目标版本锁定为 2009 PC GOTY 冒险模式。`docs/data/plants.json`、`zombies.json`、`levels.json`、`stages.json` 是唯一内容事实源：分别提供 49 种植物、26 种僵尸、50 关和 5 种场景的 ID、数值、解锁、出场与规则；`docs/RESEARCH.md` 记录来源及存疑项。本文引用这些字段，不复制大表；任何未在 JSON 中出现的数值必须先补事实源并评审。运行时只能使用引擎已有的 Sprite、.rxsprite/animator、Script props、Trigger、冻结节点图子集和 .rx 规则。

## 1. 坐标、网格与绘制
- 只使用 XY 平面，所有玩法实体 `z=0`。相机位于 `[0,0,10]`，恒等旋转，朝 `-Z`，正交投影；禁止 XZ、透视或依赖透视形变。
- 网格采用 `cell=1.6m` 的设计基准。棋盘左下角锚点 `O=(x0,y0)` 由关卡背景 prefab 决定；格心为 `x(c)=x0+(c+0.5)*cell`、`y(r)=y0+(r+0.5)*cell`，`c=0..8`，`r=0..rows-1`。点击换算取 floor 后做边界检查。
- 白天/黑夜为 5 行；泳池、浓雾为 6 行，行号及水路以 `stages.json.pool_rows` 为准（不得把数组索引自行改成另一套编号）。屋顶为 5 行、前 5 列斜坡；斜坡 y 偏移表由屋顶 stage 配置生成并固定为 `roof_y_offset[c]`：`c=0..4` 使用美术/关卡配置的坡面偏移，`c=5..8=0`，不得用透视相机替代。缺少经审计的偏移数值时，先用 0 并标记待校准，不擅自造数。
- 建议相机 `projection=orthographic`、`orthoSize` 取能覆盖棋盘和 HUD 的世界半高；具体值由背景尺寸和 cell 通过视口验收确定。
- sortingOrder：背景 0、草坪/水面 1、植物 2、子弹/投射物 3、僵尸 4、遮罩与交互提示按所属层加偏移、HUD 10。实体必须显式写 Category：动态角色为 `role`，静态棋盘为 `map`，带 Script/Trigger 的交互物为 `interaction`。

## 2. 实体架构与组件契约
- 每个植物类型、僵尸类型各有一个 prefab/模板实体：Sprite（或 Sprite 指向 `.rxsprite` GUID）+ Tag（类型 ID）+ Script（模块和实例 props）。数值不硬编码到实体；Script `props` 是实例覆盖载体，例如 `hp`、`cooldown_remaining`、`lane`、`target_row`、`phase`。
- LevelController：单实体、Category=interaction，挂 Script，`graphRef` 指向该关卡图，props 载入 level/stage ID、波次状态、棋盘锚点和规则开关。
- HUDController：全精灵实现卡栏、阳光数、波次进度、暂停/提示；无原生 UI。ProjectileFactory：设计上的交互实体/图节点边界；其实际生成若超出冻结节点能力，必须使用预布置对象或代码/人工创建的 Projectile prefab，不宣称存在引擎服务。
- 运动体统一加 `RigidBody{kind:"kinematic"}`；只有确实受场景重力的对象才用 dynamic。无重力的俯视/棋盘玩法场景重力为 `[0,0,0]`。

## 3. 数据到运行时通路（硬约束）
JSON 是给人和工具审阅的源，不在运行时读取文件。`.rx` 编译器限制为：导出函数必须自包含，跨函数调用/内部符号不链接；`call.call_function` 可靠支持一个动态参数，且要求标量入、标量出。因此构建脚本将 JSON 数值表生成到 `Content/Scripts/pvz_data.rx`，以数值编码植物 `1..49`、僵尸 `1..26`，每个字段生成独立 if 链查询函数。字符串/描述/数组（如出怪名单）由 LevelController 的 Script props 或预生成关卡图承载，不能假设 `.rx` 可返回数组。

### `pvz_data.rx` 完整函数清单
所有函数均为自包含 if 链；`id` 是数值编码，未知 ID 返回 0（调用方记录数据错误并拒绝生成单位）。

**植物字段：**
- `plant_cost(plant_id: f32) -> f32`：阳光价格。
- `plant_recharge(plant_id: f32) -> f32`：卡牌冷却秒数。
- `plant_hp(plant_id: f32) -> f32`：基础生命。
- `plant_damage(plant_id: f32) -> f32`：典型单次伤害；不等同完整 DPS。
- `plant_attack_interval(plant_id: f32) -> f32`：攻击间隔。
- `plant_range_code(plant_id: f32) -> f32`：规范化范围枚举（none/self/touch/lane/3x3/whole_screen/描述性短程等由生成器固定编码）。
- `plant_production_amount(plant_id: f32) -> f32`、`plant_production_interval(plant_id: f32) -> f32`：阳光产物数量/间隔；硬币生产不伪装为阳光，另由掉落类型 props 表达。
- `plant_unlock_code(plant_id: f32) -> f32`：解锁关卡的稳定编码。
- `plant_mushroom(plant_id: f32) -> f32`、`plant_aquatic(plant_id: f32) -> f32`：布尔值以 0/1 返回。
- `plant_upgrade_code(plant_id: f32) -> f32`：升级来源编码，无升级返回 0。

**僵尸字段：**
- `zombie_hp(zombie_id: f32) -> f32`、`zombie_speed_code(zombie_id: f32) -> f32`、`zombie_dps(zombie_id: f32) -> f32`：基础耐久、速度枚举、啃食 DPS。
- `zombie_armor(zombie_id: f32) -> f32`：可数值化护甲吸收量；无护甲返回 0，部件语义由 props 标记。
- `zombie_special_code(zombie_id: f32) -> f32`：特殊行为枚举。
- `zombie_first_level_code(zombie_id: f32) -> f32`：首次出场关卡编码。

**关卡/场景字段：**
- `level_waves(level_code: f32) -> f32`、`level_flags(level_code: f32) -> f32`、`level_starting_sun(level_code: f32) -> f32`：波数、旗帜、初始阳光。
- `level_stage_code(level_code: f32) -> f32`、`level_special_code(level_code: f32) -> f32`、`level_reward_code(level_code: f32) -> f32`：场景、特殊规则、奖励编码。
- `stage_rows(stage_code: f32) -> f32`、`stage_cols(stage_code: f32) -> f32`、`stage_sky_sun_enabled(stage_code: f32) -> f32`、`stage_sky_sun_interval(stage_code: f32) -> f32`、`stage_sky_sun_amount(stage_code: f32) -> f32`、`stage_fog_columns(stage_code: f32) -> f32`、`stage_roof_slope_cols(stage_code: f32) -> f32`：场景棋盘与天降阳光/雾/屋顶字段。

生成后对该文件运行 `rx_check`；任何疑似跨函数调用都拒绝构建。

## 4. 核心系统

### 4.1 阳光经济
LevelController 用 `sun`、`sky_sun_timer`、`production_timers`、`collectible_state` 管理经济。天降阳光按 stage 的 enabled/interval/amount 生成“可点击收集”实体；夜晚关闭。向日葵/阳光菇按植物 `production` 字段产生掉落物；阳光菇白天睡眠由 stage 状态决定。点击掉落物通过 Trigger/输入命中将其标记 collected，原子增加 HUD `sun` 并销毁/回收对象；若图节点不支持 spawn/destroy，则预布置池 + enabled/位置切换，或由 `.rx`/Script 实现并在测试中注明。

### 4.2 种子卡栏
每卡实体有 `plant_id`、selected、cooldown_remaining、cost；HUDController 维护唯一选中卡。点击卡先检查解锁、冷却和 `sun >= plant_cost`，成功才进入种植态并显示遮罩；取消/右键回到空闲。铲子是独立模式，点击植物先销毁/禁用植物并清空格占用，不退款、不绕过 Trigger 校验。遮罩是精灵，不使用原生 UI。

### 4.3 种植/铲除
输入事件 → 屏幕坐标转视口世界坐标（正交相机）→ 依据网格公式取行列 → 校验边界、占用、泳池是否有睡莲、屋顶是否有花盆、墓碑/弹坑等阻挡 → 扣阳光、设置卡冷却、创建植物实体。`entity.spawn` 不是已确认的冻结图节点：若图校验器不接受它，不留假图，改用预布置实体池的 enabled/transform/component_set；否则由 Script 实现同等动作并记录限制。任何创建/销毁图节点不可当作已实现能力宣称。

### 4.4 波次导演
读取 level 的 `waves`、`flags`、`zombie_ids` 和 `special_rules`。普通波按关卡清单过滤出怪类型，导演以 props 权重表递增，权重和刷新间隔作为可调实例覆盖；不得创造 JSON 没有的僵尸 ID。旗帜波在由 `flags` 推导的边界出现旗帜僵尸/大波提示。HUD 进度条使用 `current_wave / total_waves`；数据事实明确的波数遵循 JSON（通常 flags×10，0 旗关 5，BOSS 关按事实源）。

### 4.5 战斗
植物攻击以定时 Script 更新；子弹是 Sprite、sortingOrder=3，直线运动使用 transform 更新，屋顶投手使用 tween 近似抛射（不能引入未提供的曲线物理）。僵尸和子弹用 Trigger AABB/行列筛选命中；碰撞后按 `hp`、`armor_remaining` 扣减，归零进入 die 并禁用。啃食状态保存 `target_plant_id`、`eat_timer`，按 zombie dps 更新植物 HP。寒冰射手写入 `slow_remaining`；寒冰菇写入全屏 `freeze_remaining`；计时到 0 恢复速度。挡门、矿工、撑杆、投手等差异作为类型 Script props 与状态机分支。

### 4.6 小推车与判负
每行预置一辆推车/泳池清洁车实体，带 Trigger 与 `lane`。僵尸越过判负线触发对应行推车，推车沿 X 清除该行敌人后变为 spent；再次越线立即进入 defeat。判负线是 map/interaction 的 Trigger，不依赖越界渲染。

### 4.7 关卡流与进度
流程状态为 `level_select → card_select → battle → reward → unlock_next`，由 LevelController/HUD 的 Script props 和图节点消息驱动；奖励 ID 取 level 数据。运行时图节点没有写文件能力，不能声称 `save.json` 可由运行时写入。第一阶段以内存 `unlocked_level`/关卡实体解锁态表达；编辑器工具或外部宿主在完成关卡后通过受控 `write_file` 生成 `Content/save.json`，启动时由宿主/props 注入，缺文件回退到 1-1。该限制必须保留在 QA 报告。

## 5. 场景机制
- 黑夜：按 stage 的墓碑规则预置/生成墓碑点；无天降阳光；蘑菇正常工作，白天睡觉。
- 泳池：stage 的 `pool_rows` 为水路；水路植物必须先有睡莲，水路使用泳池清洁车。
- 浓雾：右侧 `fog_columns`=4 列用遮罩精灵盖住，灯笼草所在有效范围清除局部遮罩，三叶草触发后清除全场遮罩计时；遮罩排序高于单位但低于 HUD。
- 屋顶：`roof_slope_cols`=5，前五列平射受坡面阻挡，投手走 tween 抛射；所有植物格先占花盆，植物不直接种裸屋顶。

## 6. 特殊关卡
特殊规则 ID 以 `levels.json.special_rules` 为准，规则适配器仅实现以下八类：`bowling`（1-5 坚果保龄球，滚动物理用预设直线/Trigger）、`whack`（2-5 锤击输入命中僵尸）、`conveyor`（x-10 传送带卡牌队列）、`big_trouble`（3-5 小僵尸，使用事实源类型/缩放 props）、`vasebreaker`（4-5 破罐后揭示内容）、`storm`（4-10 暴风雨黑屏，用全屏 Sprite 遮罩）、`bobsled`（5-5 雪橇区与冰道）、`boss`（5-10 僵王：血条、召唤、冰/火球、蹦极偷菜、低头时可攻击窗口）。未在 JSON 标出的规则不得自动套用。

## 7. 动画清单与策略
所有植物 ID 逐项建立 `PZ_<Name>.rxsprite`，至少包含 `idle`；有攻击字段/攻击行为的植物加 `attack`；有 production 的加 `produce`；由 `special` 明确动作的加同名特殊 clip（如 `explode`、`eat`、`sleep`、`launch`、`dig` 等）。49 种的具体动作集合以 JSON `production`、`damage/attack_interval`、`special` 机械生成并在 sprite_get 中核对，不手工补不存在的行为。

所有僵尸 ID 逐项至少 `walk/eat/die`；由 JSON `special` 明确动作的追加 `vault`、`armor_break`、`dance/summon`、`dive`、`ride`、`jump`、`steal`、`throw`、`spawn_imp`、`boss_cast` 等对应 clip。循环 walk/attack/produce 建议 4 帧、6–10 fps；idle 2–4 帧、4–8 fps；一次性特殊动作 2–6 帧、duration 优先于 fps，`die` loop=false/onFinish=hold。整张动作表一次生成，纯品红隔离底；autoslice 后帧数不符即重生成。地面单位默认 pivot `[0.5,1]`，VFX/投射物 `[0.5,0.5]`；FSM 模式 clip 留空，手动模式显式 clip，禁止混用。

## 8. 素材命名与图集
贴图路径 `Content/Textures/PZ_<Type>_<Name>.png`；动作表使用纯品红 `#FF00FF`、无渐变、帧间隔离、主体不越格，推荐 2x2（4 帧）或 2x4（8 帧），同一角色同向同尺度。图集文档命名 `Content/Sprites/PZ_<Name>.rxsprite`。种子卡图标单独一张 7x7 网格图集，卡图标不与角色动作表混用。导入后依次 asset_import → sprite_autoslice → sprite_create(autoslice=true) → sprite_get → sprite_set；记录贴图 GUID，Sprite 只引用 GUID。

## 9. 已知限制与取舍
1. 无音频管线：跳过 BGM/音效，保留逻辑事件名供未来接入。
2. 无原生 UI：HUD 全部 Sprite，文字需预渲染字库/数字精灵。
3. 图节点冻结子集不保证 `physics.*`、audio、spawn、destroy、look_at、lerp、for_each、gate、draw_debug_line、call_function；未实现节点不得写入已验证图，采用预置池、Script 自包含规则或宿主编排并标注。
4. 运行时无文件写入：`Content/save.json` 由编辑器/宿主工具回写，非游戏内写盘。
5. 屋顶抛射为 transform tween 近似；不足以模拟完整物理。
6. `damage`、冷却、护甲和僵王口径按事实源，疑点须通过实测/研究更新，不自行宣称原版精确帧行为。

## 10. 工程验收门槛
先 `scene_summary` 确认 mode=2d，再查询实体/资产；批量编辑前 `scene_checkpoint`。每张图先 `graph_validate` 再 `graph_create`；每次改动执行 `rx_check`。场景自检需正交 `viewport_frame`，PIE `play_enter/play_pause/play_step/play_exit`，并用 `host_events_drain` 处理 `logic.unsupported` 与 `anim.warn`。失败按技能规程 rollback，不遗留半成品。
