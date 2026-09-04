# 10 · 交互逻辑设计

> 双轨制:**节点图**(可视化事件编排,UE Blueprint 精简版)负责「什么时候做什么」,
> **`.rx` 脚本**(rurix 语言,agent 主写)负责「怎么算」。人类在节点图上审阅微调;
> 生成、重构、批量修改一律 agent 经 MCP 完成(P-3)。

## 1. 从 UE5/Unity 取用的设计决策

| 设计点 | 来源 | 本引擎决策 |
|---|---|---|
| 节点图可视化脚本(Event Graph + 执行引脚) | UE Blueprint | 采用,精简为单图 = 一个 Script 组件的事件编排面 |
| Compile & Save 显式生效 | UE Blueprint | 图保存 → 自动 lowering 校验(等价编译),错误进 Problems;不另设 Compile 按钮 |
| C# 脚本组件挂 GameObject,SerializeField 暴露属性 | Unity | `Script` 组件挂 `.rx` 模块或节点图,暴露属性在 Inspector 可编辑(dict) |
| 事件驱动(BeginPlay/Tick/碰撞事件) | UE/Unity 共有 | §3 事件模型 |
| Visual Scripting 与代码双轨 | Unity(VS + C#) | 节点图 + `.rx` 双轨,图可调用 `.rx` 函数(§4.3) |

## 2. 职责划分(何时用哪轨)

| 需求 | 用 |
|---|---|
| 「玩家进触发区 → 开门 + 播音效」类事件编排 | 节点图 |
| 「按贝塞尔曲线移动」「寻路」「数值计算」类算法 | `.rx` 脚本函数,图中经 `CallFunction` 节点引用 |
| 一次性原型逻辑、agent 生成后人工审 | 节点图(agent 生成 JSON,人审阅) |
| 性能敏感/复杂状态机 | `.rx` 脚本(节点图退化为只挂事件入口) |

## 3. 事件模型(运行时)

### 3.1 事件源

| 事件 | 来源 | 负载 |
|---|---|---|
| `on_start` | PIE 进入 / 实体激活 | entityId |
| `on_update` | 每帧(逻辑帧率 = 物理固定步 `fixedDt`) | entityId, dt |
| `on_contact_begin` / `on_contact_persist` / `on_contact_end` | `PhysicsWorld::drain_contacts`(`03 §3.3`,规范序确定性) | entityId, otherEntityId, contactPoints |
| `on_trigger_enter` / `on_trigger_exit` | trigger sensor 接触过滤 | entityId, otherEntityId |
| `on_input` | PIE 输入队列(键/鼠/手柄占位) | entityId, action, value |
| `on_message` | 图/脚本互发命名消息 | entityId, name, payload |
| `on_timer` | 图内 Timer 节点到期 | entityId, timerId |

### 3.2 调度规则(确定性,I-5 配套)

- 每逻辑帧:输入事件 → 接触事件(规范序)→ timer → update → message 队列清空。
- 同实体多图按挂载序;跨实体按实体 id 升序。**该顺序为契约**,playtest 回放依赖。

## 4. 节点图格式(`.rxgraph`,JSON)

### 4.1 图结构

```json
{
  "version": 1,
  "id": "g_01J…",
  "name": "DoorOpener",
  "exposedProps": [ { "name": "openSpeed", "kind": "F32", "default": 90.0 } ],
  "nodes": [
    { "id": "n1", "type": "event.on_trigger_enter", "pos": [40, 80] },
    { "id": "n2", "type": "flow.branch", "pos": [240, 80],
      "inputs": { "condition": { "node": "n3", "pin": "out" } } },
    { "id": "n3", "type": "entity.has_tag", "pos": [60, 200],
      "inputs": { "entity": { "node": "n1", "pin": "otherEntity" }, "tag": { "const": "player" } } },
    { "id": "n4", "type": "transform.rotate_tween", "pos": [460, 80],
      "inputs": { "target": { "const": "$self" }, "angle": { "ref": "openSpeed" }, "duration": { "const": 1.2 } } }
  ],
  "edges": [ { "from": ["n1", "exec"], "to": ["n2", "exec"] }, { "from": ["n2", "then"], "to": ["n4", "exec"] } ]
}
```

- 执行边(exec,UE 白色执行引脚对齐)与数据边分离;`inputs` 内联数据边,`edges` 列执行边。
- 值来源三态:`const` 常量 / `node+pin` 数据边 / `ref` 暴露属性。
- `$self` = 挂载实体;`$parent` 链保留。

### 4.2 节点类型注册表(首发,冻结子集)

| 族 | 节点 |
|---|---|
| `event.*` | §3.1 全部事件入口(每图每事件至多一个) |
| `flow.*` | branch、sequence、for_each、delay、timer_start/cancel、gate |
| `entity.*` | get_transform、set_transform、spawn(prefabRef)、destroy、has_tag、add_tag、find_by_tag |
| `transform.*` | move_tween、rotate_tween、look_at、lerp |
| `physics.*` | cast_ray、apply_impulse、overlap |
| `audio.*` | play、stop(占位) |
| `var.*` | get、set、add(图内黑板变量) |
| `call.*` | call_function(调 `.rx` 导出函数,§4.3)、send_message |
| `debug.*` | log、draw_debug_line(编辑态) |

注册表机制同组件注册表(`09 §3.1`):schema 单源,驱动 UI 节点面板、`rx_check` 等价校验、agent 生成约束。

### 4.3 与 `.rx` 脚本互绑

- 图 → 脚本:`call.call_function { module: "Content/Scripts/door.rx", fn: "smooth_open", args: [...] }`;
  被调函数必须 `#[export]` 且签名在函数注册表(纯值进/出,无副作用句柄)。
- 脚本 → 图:脚本可 `send_message`,图经 `event.on_message` 接收。
- lowering:`.rxgraph` → 中间 IR → 解释执行(F0–F3 解释器;F4 评估编译到 `.rx`,D-011)。

## 5. NodeGraph 面板(UI,`07 §1` G 区)

- 与 Viewport 同位页签;网格画布 + 节点 + 连线;`Space` 搜索节点;拖线连接;常量内联编辑。
- 顶部:图名、保存状态、关联实体清单、暴露属性表。
- **人在回路定位**:用户做「审阅 + 微调 + 改常量」;「从零生成」「批量改图」「重命名清理」一律 Chat → agent(`logic-blueprint-gen` skill 直写 `.rxgraph` JSON)。
- 校验:保存即全图校验(悬空输入/类型不匹配/环检测=执行边禁环,数据边禁环),错误节点红框 + Problems 汇总。

## 6. agent 生成路径(契约)

1. agent 收需求 → 选轨(§2)→ 生成 `.rxgraph` JSON 或 `.rx` 模块。
2. 图:过 `code-forge` 的 `graph_validate`(schema + 类型检查 + 节点注册表校验)。
3. 挂载:`component_set` 写 `Script { graphRef }` 或 `{ module }`。
4. 验证:playtest `test_run` 注入触发(如 spawn player tag 实体进触发区)断言结果(`05 §6`)。
5. 失败 → debug 模式循环(`04 §3`),不交付未验证逻辑(skill `logic-blueprint-gen` 强制)。

## Errata(只追加区)

- **E-10-001(2026-08-31,F-GAME-4 帧动画波 / D-031)——§4.2 节点注册表 40→45 加性扩展**:新增两族五节点(RD-F4-004 加性扩展先例):`sprite.play { entity, clip, restart?(可选,缺省 false) }`、`sprite.stop { entity }`、`sprite.set_frame { entity, index }`、`animator.set_bool { entity, param, value }`、`animator.set_trigger { entity, param }`。①`restart` 是注册表首个**可选输入 pin**(`PinSpec.required=false`,校验器不要求接线),缺省 false = **幂等语义**——同 clip 重复调用 no-op,可安全挂 on_update 每帧调用(内置 VibeGame「每帧 restart 重启致冻帧」头号坑的防御);retrigger 场景(碰撞/按键的离散时刻)才显式传 true。②执行语义:五节点**不直接写组件**,产出 `AnimCommand` 进 `LogicRuntime` 命令队列,宿主 `advance_frame` 在逻辑帧后取走(`take_anim_commands`)交动画系统按声明序消费——`Sprite.frame/clip` 唯一写者是宿主,图侧无双写者(裁决记 09 E-09-002)。③目标实体无 .rxsprite 精灵、clip 不存在、animator 参数不存在、对 FSM 模式实体手控 play(实体 clip 留空且文档带 animator 时状态机独占,见 09 E-09-002 模式判定)等误用一律如实 `anim.warn` 事件,不静默(I-5)。
