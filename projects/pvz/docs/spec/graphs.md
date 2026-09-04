# PvZ 核心图节点级实现规约(graphs.md v2 · 对象池版)

> 本规约是 docs/spec/spec.md 的下位实现契约,精确到节点编排模式。
> 所有模式均对照引擎源码验证(forge-logic interp.rs / registry.rs / callruntime.rs)。
> v2 修订:确认 entity.spawn/entity.destroy/flow.for_each/flow.gate/physics.*/audio.* 等节点
> 在解释器中未实现(仅记 logic.unsupported),全部设计改为**对象池**架构。

## 0. 引擎硬约束(已验证,必须遵守)

### 0.1 可用节点(解释器真实实现,interp.rs)
- 事件:event.on_start / on_update(dt) / on_contact_begin/persist/end(otherEntity) / on_trigger_enter(otherEntity) / on_trigger_exit / on_input(action,value) / on_message(name,payload) / on_timer(timerId)
- 流控:flow.branch(condition→then/else)、flow.sequence(seq0,seq1)、flow.delay(duration,续链异步)、flow.timer_start(timerId,duration)/timer_cancel
- 实体:entity.get_transform / entity.set_transform(部分应用,字段须字面量或 Transform 整体直通)/ entity.has_tag / entity.add_tag(改写唯一 Tag 值)/ entity.find_by_tag(首个匹配)
- 变换:transform.move_tween(target,offset 字面 Vec3,duration;同目标新 move tween 替换旧的)/ rotate_tween
- 变量:var.get/set/add(图实例级黑板)
- 调用:call.call_function(module,fn,args→result)/ call.send_message(name,payload 同帧广播)
- 调试:debug.log
- 动画:sprite.play(entity,clip,restart?)/stop/set_frame;animator.set_bool/set_trigger

### 0.2 未实现节点(调用仅记 logic.unsupported,绝无效果)
**entity.spawn、entity.destroy、flow.for_each、flow.gate、physics.cast_ray、physics.apply_impulse、physics.overlap、transform.look_at、transform.lerp、audio.play、audio.stop、debug.draw_debug_line**
→ 严禁使用。运行时没有实体增删:一切实体预置场景,「生成/销毁」= 对象池激活/回收。

### 0.3 其余约束
| 约束 | 设计对策 |
|---|---|
| call_function args 是单一 ValueSource:全 const 数组(可多参)或单 node 引用(包一元);多动态参不可达 | 多动态值打包进一个 f32(如 row*100+col) |
| .rx 导出函数:自包含、同构标量参数(≤4)、标量返回;无 floor/min/max/abs | if 表达式 + `as` 截断手写;fn main() {} 必须有 |
| move_tween offset 须字面 Vec3 | 常量步进;停止 = 停止续发(微 tween 0.1s 级,漂移可忽略) |
| set_transform 部分应用,数组须字面量;Transform 整体可 get→set 直通 | 位置复制用 get_transform→set_transform |
| Trigger 事件只发给 Trigger 持有实体的图;对方无需 Trigger;Trigger AABB=translation±extents/2,对方 AABB=translation±|scale|/2 | 碰撞处理放「感知方」侧 |
| 消息同帧广播,所有 on_message 图都收;payload 可为任意 JSON(含 Transform dict) | 接收方用自身状态过滤;payload 可携带位置 |
| var 黑板为图实例私有 | 跨实体通信只能消息或改对方 Tag |
| 运行时输入 = (action:String, value:F32),无指针坐标 | 输入契约编码(§7) |
| 图实例 props 只能来自场景文件静态初始值(spawn 不存在) | 池实例差异 = 场景中每个池实体各自的 Script props |

## 1. 对象池架构(核心模式)

### P0 池化总则
- 每类动态实体一个对象池:豌豆池(20)、阳光池(12)、僵尸池(16)。池实体全部预置在场景,初始位置在场外(y=-60),Tag=<pool>_free(如 pea_free)。
- 「激活」= set_transform 移入 + add_tag 改写为 <pool>_active;「回收」= set_transform 移回场外 + add_tag 改写回 free。
- 池实体的行为图每帧用 has_tag($self,"<pool>_active") 门控行为(free 态什么都不做)。
- 池管理集中在 LevelController:它持有 next 游标 var,round-robin 轮转;激活表 = 按池序号的静态分支(生成器产出)。

### P1 池实体移动(僵尸行走)
僵尸图 on_update:branch(has_tag($self,"zombie_active") 且 var state==walk)→ move_tween($self,[-0.034,0,0],0.1)(每帧续发微 tween,速率=0.34 单位/秒;停止续发即停,漂移 ≤0.1s)。
冻结替代:state 改 eat 后不再续发即可,无需零 tween。

### P2 受害方碰撞处理(豌豆命中僵尸)
僵尸本体 Trigger(extents≈[0.9,1.4,0.9])。豌豆实体不挂 Trigger(免双向事件)。
僵尸图 on_trigger_enter(other):has_tag(other,"pea_active")→ var.add(hp,-20)→ **回收豌豆**:set_transform(other, 场外常量)→ add_tag(other,"pea_free")→ call hp_dead(hp)→ branch → 死亡序列(P6)。

### P3 发射(植物→豌豆池)
植物不直接碰池:发消息带位置。
plant_shooter:on_update 冷却到 → entity.get_transform($self)→ call.send_message("fire_pea", payload=该 Transform dict)。
控制器 on_message "fire_pea":var.add(peaNext,1)→ call mod20(peaNext)→ 20 路分支:find_by_tag("pea_free")……不行,find_by_tag 只给首个匹配。**决议**:池实体 Tag 值带序号(pea_free_1..20 / pea_active_1..20),控制器分支按序号 find_by_tag 精确取;激活时 add_tag 改写 pea_active_3 等。分支表由生成器产出。

### P4 生产者(向日葵)
plant_producer:on_update 计时到 → get_transform($self) → send_message("spawn_sun", payload=Transform)→ 控制器阳光池激活(同 P3 序号分支,sun_free_1..12)→ 阳光实体缓落 tween 到落点(阳光图:on_update 门控 sun_active + var fallT 控制 move_tween 下落段)→ 8 秒未收自动回收(sun 图 var 计时,自回收:set_transform 场外 + add_tag sun_free_i——序号从哪来?阳光图 exposedProp poolId 场景静态写入,回收分支按 poolId 常量改写 tag)。

### P5 死亡序列(僵尸)
var.set(state,9)→ animator.set_trigger($self,"die")→ flow.delay(1.2)→ set_transform($self,场外)→ add_tag($self,"zombie_free_{poolId}")(poolId 为 exposedProp,分支常量)→ send_message("zombie_died", payload=row)。

### P6 行/格编码
row(1..6)、col(1..9):code=row*100+col。pvz_rules.rx:pack_rc/unpack_row/unpack_col(单参/双 const 参)。
植物格位:45/54 格 cell 实体静态预置,Tag=cell_r{row}c{col},位置即格心。

## 2. zombie_walker.rxgraph(挂每个池僵尸,props:zombieId、poolId、row 由激活时控制器 add_tag 写 row 码)

- on_start:var.set hp=call pvz_data.zombie_hp(zombieId prop);var.set state=0(0=池内 1=walk 2=eat 9=dead)
- on_message "zombie_activate":payload=poolId;本实例过滤——call pvz_rules.eq_f32 需两参……**过滤决议**:控制器激活时不发广播,直接对目标实体操作:set_transform(入口)+ add_tag(zombie,"zombie_active")。僵尸图 on_update 用 has_tag($self,"zombie_active") 自门控,首帧检测到 active 且 state==0 → var.set(state,1) 初始化。无需消息过滤。
- on_update(dt):
  1. branch(state==0)→ 空
  2. branch(state==1)→ move_tween 微步(P1)
  3. branch(state==2)→ 啃食计时(动画由 FSM 驱动;伤害植物侧自扣)
- on_trigger_enter(other) 顺序过滤:
  1. has_tag 前缀判 pea_active:has_tag 须精确值——豌豆 tag 带序号(pea_active_3)……**修订**:豌豆 active 态统一 tag 值 "pea_active"(不带序号;序号只在 free 态 tag:pea_free_1..20 供控制器精确回收/激活;激活后统一改写 pea_active)。僵尸侧 has_tag(other,"pea_active") 精确匹配可行!同理 zombie_active 统一、植物 plant、阳光 sun_active、mower_active、loseline。
  2. has_tag(other,"pea_active")→ P2 命中链
  3. else has_tag(other,"plant")→ var.set(state,2)→ animator.set_bool("eating",true)
  4. else has_tag(other,"mower_active")→ var.set(hp,0)→ 死亡序列(P5)
  5. else has_tag(other,"loseline")→ send_message("loseline_hit", payload=0)(行由 loseline 实例侧补充:loseline 图转发 "loseline_hit_row" payload=本行 row 常量)
- on_trigger_exit:has_tag(other,"plant")→ var.set(state,1)→ animator.set_bool("eating",false)

## 3. plant_shooter.rxgraph(props:plantId)

- on_start:var.set remain = call pvz_data.plant_attack_interval(plantId)
- on_update(dt):call pvz_rules.neg(dt)→ var.add(remain,结果)→ call pvz_rules.nonpositive(remain)→ branch:
  true → var.set(remain, 重查 interval)→ get_transform($self)→ send_message("fire_pea", transform)
  (v1 盲射简化:不检测本行僵尸;列入已知取舍)

## 4. plant_producer.rxgraph(props:plantId)

- on_start:var.set remain = call pvz_data.plant_production_interval(plantId)
- on_update:同 §3 计时 → 到点 → get_transform($self)→ send_message("spawn_sun", transform)→ animator.set_trigger($self,"produce")

## 5. 豌豆/阳光池实体图

- pea_fly.rxgraph(挂每个池豌豆):on_update → branch(has_tag($self,"pea_active"))→ move_tween($self,[+0.13,0,0],0.1)(≈1.3 单位/秒……豌豆应快:8 单位/秒 → offset [+0.8,0,0] duration 0.1);飞出右界回收:pea 图 var 累计飞行时间 var.add(flyT,dt)→ branch(flyT>3.5)→ 自回收(set_transform 场外 + add_tag pea_free_{poolId} 分支 + var.set(flyT,0))。命中回收由僵尸侧 P2 负责(同样改写 tag 为 pea_free_{i}——僵尸不知道 i!**修订**:豌豆命中回收统一 tag "pea_free"(无序号),控制器激活时找 pea_free 首个匹配 + 立即改写 pea_active——find_by_tag 首个匹配即空闲池位,无需序号!阳光池同理。P3 的 20 路分支表简化为单分支:find_by_tag("pea_free")→ 判空(has_tag 结果实体无效时 set_transform 静默无效)→ 激活。)
- sun_fall.rxgraph(挂每个池阳光):on_update → branch(has_tag($self,"sun_active"))→ 下落 tween 段 + 8s 自回收(同上,tag sun_free 无序号)+ 被收集:on_message "collect_sun" 由控制器发?——点击收集:输入 collect_sun value=阳光序号……简化:收集也走池位扫描——控制器收 collect_sun 输入后 find_by_tag("sun_active") 逐个?v1 简化:collect_sun 输入 → 控制器广播消息 → 所有 active 阳光自回收 + 每个回发 "sun_gained" → 控制器计数(一次收全屏,列入取舍;原版逐一点收)。

## 6. level_controller.rxgraph(props:levelCode 缺省 101)

状态 var:phase(0 准备/1 战斗/2 胜/9 负)、sun、waveIndex、totalWaves、zombiesAlive、skyT、peaNext
- on_start:
  1. var.set sun = call pvz_data.level_starting_sun(levelCode)
  2. var.set totalWaves = call pvz_data.level_waves(levelCode)
  3. var.set zombieKinds = call pvz_data.level_zombie_count(levelCode)
  4. var.set rows = call pvz_data.stage_rows(call pvz_data.level_stage_code(levelCode))——嵌套调用不可(单参动态)……**决议**:stage_code 是 levelCode/100 的整数部分——pvz_rules 加 level_stage(levelCode) 单参(整除);stage_rows(stage) 再单参;分两步两个 call 节点,中间结果经 var 传递。可行!
  5. flow.timer_start("sky_sun", 5.0);flow.timer_start("wave", 12.0)
  6. send_message("hud_set_sun", sun)
- on_timer timerId=="sky_sun":branch(call stage_sky_sun_enabled(stage))→ 激活阳光池位(find_by_tag "sun_free" → set_transform 到 8 预设落点之一(var.add skyIdx 轮换,call mod8)→ 落点坐标:8 个分支常量)→ add_tag sun_active → flow.timer_start("sky_sun", call stage_sky_sun_interval(stage))
- on_timer timerId=="wave":var.add(waveIndex,1)→ 本波只出 1+waveIndex/3 只(call wave_size 单参)→ 每只:种类 = call level_zombie_at(levelCode, call pick_kind(pack(waveIndex,i)))——双动态打包 pack 为 waveIndex*100+i 单参 → 激活僵尸:find_by_tag("zombie_free")→ set_transform 行入口(行轮换 var.add spawnRow;行 y 坐标 6 分支常量;x=+12)→ add_tag zombie_active → var.add(zombiesAlive,1)→ 只数循环:无 for_each——用 timer 链:flow.timer_start("wave_member", 0.4) 逐只,计数 var。→ 下一波:branch(waveIndex<totalWaves)→ flow.timer_start("wave", call wave_interval(waveIndex*100+totalWaves 打包))
- on_message:
  - "zombie_died":var.add(zombiesAlive,-1)→ branch(zombiesAlive<=0 且 waveIndex>=totalWaves 且 phase==1)→ 胜利序列(var.set phase=2;send_message "level_win" payload=call level_reward_code(levelCode))
  - "loseline_hit_row"(payload=row):branch 链按 row 1..6 → 各行:find_by_tag("mower_r{row}")→ has_tag(结果,"mower_idle")→ then:add_tag mower_active + move_tween 碾压([+22,0,0],1.8);else:判负序列(phase=9,send_message "game_over")
  - "fire_pea"(payload=Transform):find_by_tag("pea_free")→ set_transform(pea,payload)→ add_tag(pea,"pea_active")
  - "spawn_sun"(payload=Transform):find_by_tag("sun_free")→ set_transform(sun,payload)→ add_tag(sun,"sun_active")
  - "sun_gained":var.add(sun,25)→ send_message("hud_set_sun", sun)
- on_input:
  - action=="plant_at":value=slot*1000+row*100+col → 解包(call unpack_slot/unpack_row/unpack_col 各单参)→ 校验:call plant_cost(slot→plantId 映射……卡槽→植物映射在 HUD;v1 简化:value 直接带 plantId:plantId*10000+row*100+col)→ 校验阳光 call can_afford(sun,cost 经 var 传递)→ 占用校验 v1 跳过(已知取舍)→ 扣费 var.add(sun,-cost)→ 激活植物:植物也池化!植物池 = 每关可选 8 种 × 每种预置 4 个实例?——**植物池决议**:场景预置「植物槽位池」:45/54 格每格一个隐藏植物实体?内存浪费但简单——不,植物种类不定。**v1 决议**:每关场景预置本关可种植物各 3 个实例(1-1 只 2 种 × 3),Tag plant_free_<plantId>_<i>;激活 = find_by_tag + set_transform 到格心(格心坐标:row/col 解包后 call grid_col_to_x(col)/grid_row_to_y(row) 单参(原点写死 .rx)→ 得到 x/y 两个 f32……set_transform 须字面 Vec3!**突破**:格心查找表——cell 实体!find_by_tag("cell_r3c5") 须常量 tag 名 → 45/54 分支:branch(code==305)→ find_by_tag("cell_r3c5")→ get_transform → set_transform(plant, 该 transform)。分支表生成器产出。)

## 7. 输入契约(action/value)

| action | value | 语义 |
|---|---|---|
| select_card | 槽位 1..8 | 选卡(HUD 记当前卡) |
| plant_at | plantId*10000+row*100+col | 种植(v1 直带 plantId) |
| shovel_at | row*100+col | 铲除(回收该格植物:格心分支表 → find 该格 active 植物……v1 简化:发消息,植物自杀式回收——植物图 on_message "shovel_at" payload=cellCode,植物用自己的 cell 码 prop 比对……prop 场景静态可写!植物激活时控制器 add_tag 写 cell 码?tag 唯一已被 plant 占用……**v1 铲子简化**:只支持铲除最近种的(控制器记 lastPlanted 实体名?名字也动不了……)。**最终**:铲子 v1 不做实体回收,只清「占用标记」语义——列入已知取舍,后续版本用第二 Tag 组件?一实体一 Tag 限制……用 Category?不。铲子 v1 = 不做,进缺口清单。) |
| collect_sun | 1 | 收集全屏阳光(v1 简化) |
| start_level | levelCode | 选关 |

## 8. hud.rxgraph

- 阳光:var sun;on_message "hud_set_sun" → var.set → 显示(11 档 scale 分支进度条 + debug.log 数值)
- 卡槽:on_input select_card → var.set selected → 冷却:var cool1..8,on_update 递减(neg 包装),种植成功消息 "plant_done" payload=slot → var.set cool[slot]=call plant_recharge(plantId)
- 进度:on_message "wave_progress" payload=waveIndex*100+totalWaves → 11 档分支

## 9. 生成纪律

- 格心分支表(45/54)、池激活分支、进度档位一律 tools/*.py 生成图 JSON。
- 每图 graph_validate 必过;play_enter 冒烟 + viewport_frame 截图 + host_events 取证。
- pvz_data.rx 由 tools/gen_pvz_data.py 生成,禁止手改;pvz_rules.rx 追加函数须自包含单参并保持 fn main() 在末尾。

## 10. 已知取舍(v1)

1. 射手盲射(不检测本行僵尸)——车道感知 v2 补(sensor 实体方案见 v1 旧稿,因消息过滤成本搁置)。
2. collect_sun 一次收全屏。
3. 铲子不做实体回收。
4. 种植占用校验跳过(玩家自律)。
5. 僵尸行走速率帧率无关但转向/变速粗糙(微 tween 粒度 0.1s)。
