# 09 · 实体与场景模型

> 设计参考:UE5(Actor + Components + World Outliner + Details Panel)与
> Unity(GameObject + Component Inspector + Prefab 覆盖/应用)的交集;
> 落地形态为**组件化实体模型**(非数据导向 ECS,理由 D-010)。

## 1. 核心模型

### 1.1 Entity

```json
{
  "id": "e_01J…",                 // 场景内唯一,创建即定
  "name": "StreetLamp_07",
  "parentId": "e_01H…",            // null = 根
  "folder": "Lighting",            // Outliner 文件夹(纯组织,无语义,UE 对齐)
  "active": true,
  "transform": {                   // 本地 TRS;世界变换由层级组合
    "translation": [0, 0, 0],
    "rotation": [0, 0, 0, 1],      // quat xyzw(与 PhysicsTransform 单源对齐,03 §3.1)
    "scale": [1, 1, 1]
  },
  "components": [ /* §3 */ ],
  "prefab": null                   // 或 { "prefabRef": "guid", "overrides": {…} },§5
}
```

决策要点:

- Entity = id + 名称 + 层级 + Transform + 组件列表(UE Actor / Unity GameObject 语义)。
- **非 ECS**:组件是能力挂载单元而非纯数据行;渲染/物理系统内部自由使用 SoA/并行(engine-host 实现细节),但对 agent/UI/序列化暴露的是组件对象模型(D-010)。理由:agent 可读性、Inspector 直映射、UE/Unity 用户习惯、本期规模。
- 文件夹(UE Outliner 文件夹语义)与父子层级分离:文件夹纯组织;父子 = 变换继承。

### 1.2 Scene

```json
{
  "version": 1,
  "name": "Main",
  "entities": [ /* Entity 数组,父先于子 */ ],
  "renderSettings": { "shadows": {"vsm": {"clipmapLevels": 4}}, "gi": true, "taa": true, "tsr": "off", "rtEffects": [] },
  "physicsSettings": { "fixedDt": 0.016666, "backend": "jolt", "syncBudget": {…} },
  "logic": { "graphs": [ /* 节点图 ref,10 §4 */ ] },
  "environment": { "gravity": [0, -9.81, 0], "timeScale": 1.0 }
}
```

序列化 = `.rxscene`(JSON,UTF-8,2 空格缩进,key 序稳定——可 diff,P-4)。
事实源(I-4):磁盘 `.rxscene`;engine-host 内存态是其派生,`scene_load` 可全量重建。

## 2. 从 UE5/Unity 取用的设计决策

| 设计点 | 来源 | 本引擎决策 |
|---|---|---|
| Actor 挂 Components,Details 面板分节编辑 | UE | Entity + components[];Inspector 组件分节(`07 §3`) |
| GameObject 必有 Transform,组件含启用勾选 | Unity | Transform 为 Entity 固有字段(非组件);每组件 `enabled` 固有字段 |
| Outliner 文件夹 + 搜索操作符 + 可见/锁定 | UE | Hierarchy 文件夹列 + `-`/`+` 搜索语法 + eye/lock 图标 |
| 多选仅公共属性;Property Matrix 批量 | UE | 多选公共组件显示(值同显/异显 `—`);**不做** Property Matrix(批量→agent) |
| Prefab 实例覆盖、Apply/Revert、隔离编辑 | Unity | §5 全量采用(覆盖存实例侧 `.rxprefab` 差量) |
| Apply Instance Changes to Blueprint | UE | 等价于 Prefab Apply,合并进 §5.3 |
| Play 态修改不持久化,退出恢复 | Unity | PIE 语义(`03 §5.4`)+ 运行态 UI 配色区分 |
| 组件 Add Component 搜索弹窗 | UE/Unity | Inspector「+ Add Component」= 注册表清单搜索 |

## 3. 组件系统

### 3.1 组件注册表(单一事实源)

注册于 `forge-scene` crate,每个组件类型声明:

```rust
ComponentDef {
    type: "MeshRenderer",
    category: Render,
    singleton: true,               // 每实体至多一个
    fields: [
        Field { name: "mesh",      kind: AssetRef(Mesh), required: true },
        Field { name: "materials", kind: List(AssetRef(Material)), default: [] },
        Field { name: "castShadows", kind: Bool, default: true },
        Field { name: "lodBias",   kind: F32, default: 0.0, range: Some(-4.0..4.0) },
    ],
}
```

注册表驱动四件事:序列化/反序列化校验、Inspector property drawer 渲染(`07 §3`)、
`component_list_types` 工具输出(agent 发现面)、`.rxscene` schema 校验。

### 3.2 首发组件集(冻结)

| type | category | 关键字段 | 引擎映射(`03`) |
|---|---|---|---|
| `MeshRenderer` | Render | mesh(ref)、materials[]、castShadows、lodBias | `GpuScene::add_instance` |
| `SkinnedMeshStub` | Render | (占位,F5 后) | — |
| `PointLight` / `SpotLight` / `DirectionalLight` | Render | color、intensity、range/angle、shadows | render graph 灯光表 + VSM |
| `Camera` | Render | fov、near、far、active | PIE 主相机 |
| `RigidBody` | Physics | kind(static/dynamic/kinematic)、mass、friction、restitution、ccd | `BodyDesc` → `add_bodies_batch` |
| `Collider` | Physics | shape(box/sphere/capsule/convex/trimesh)、size 参数、isTrigger、offset | `ShapeDesc`;trigger → sensor |
| `Script` | Logic | module(`.rx` 文件 ref)、graphRef(节点图,二选一)、enabled、暴露属性 dict | `10` 全篇 |
| `AudioSource` | Audio | clip(ref)、volume、loop、spatial(占位实现) | 音频占位(F4) |
| `ParticleStub` | Render | (占位) | — |
| `Volume` | Scene | shape、priority、效果参数(雾/曝光) | renderSettings 局部覆盖 |

### 3.3 组件生命周期

- 编辑态:组件增删改 → 立即反映到 host 预览(`GpuScene` 增量,DirtyRange 驱动)。
- 运行态(PIE):`play_enter` 时按组件映射建物理体/渲染实例;`Script` 组件的 `on_start/on_update/on_contact` 由逻辑运行时驱动(`10 §3`)。
- `enabled=false`:渲染组件隐藏、物理组件移出世界(保留句柄映射)、脚本停驱动。

### 3.4 property drawer 类型(kind)

`Int / F32(range 可选)/ Bool / String / Enum([…]) / Vec2 / Vec3 / Color / AssetRef(类型过滤) / EntityRef / List(T) / Dict(String→T)`。
Inspector 按 kind 渲染;agent 经 `component_set` 用同 schema 校验。

## 4. 层级与变换

- 世界变换 = 祖先本地 TRS 链组合(3×4 组合用 `compose_transform_3x4`,`03 §3.1`)。
- `entity_reparent` 默认 `keepWorldTransform=true`:重算本地 TRS 保持世界位姿(Unity 对齐)。
- 循环挂接检测 = `SCENE_HIERARCHY_CYCLE` 错误。

## 5. Prefab

### 5.1 格式

`.rxprefab`(JSON)= Entity 子树模板(根 Entity + 递归子代),结构与 `.rxscene` 的 entities 项相同,另含 `prefabVersion`。

### 5.2 实例与覆盖

- 场景内 prefab 实例:Entity 带 `prefab: { prefabRef: guid, overrides: {} }`。
- 覆盖模型(Unity 对齐):实例存**差量**(`overrides` = JSON Pointer → 值),未覆盖字段随 prefab 模板更新;覆盖字段在 Inspector 标蓝。
- 嵌套 prefab 支持(prefab 内实例化另一 prefab),覆盖链逐层合并。

### 5.3 操作

| 操作 | 语义 | 工具 |
|---|---|---|
| Create | 选中实体子树 → `.rxprefab` | `entity_create` + `project.fs_write`(skill `prefab-workflow` 编排) |
| Instantiate | 拖入场景 → 实例实体 | `entity_create { prefabRef }` |
| Apply | 实例覆盖回写模板(全部/单字段),其他实例同步 | `prefab_apply`(engine-scene,write,幂等) |
| Revert | 清实例覆盖(全部/单字段) | `prefab_revert` |
| Open(隔离编辑) | 以模板为场景临时打开编辑,保存即 Apply | UI 模式;底层 = 临时场景 + Apply |

## 6. 场景校验

`scene_load` / 保存前校验(I-5 显式报错):

1. 引用完整性:AssetRef GUID 存在且类型匹配;EntityRef 存在。
2. 组件合法:注册表校验(singleton、required、range、enum)。
3. 层级:无环、父先于子。
4. 版本:`version` 字段;旧版本经迁移器链升版(迁移器注册于 forge-scene,`migrate_v{n}_to_v{n+1}`)。

校验失败 = 结构化错误列表(file + jsonPointer + code + message),进 Problems 面板;agent 可读同一份。

## Errata(只追加区)

- **E-09-003(2026-10-06,D-045)——§3 组件注册表增 `Text`(as-built)**:字段 `text, font(字体 GUID), size(px), color[4], align left|center|right, verticalAlign top|middle|bottom, boxSize[2](px,0=按内容), wrap, lineHeight, letterSpacing, outlineColor[4], outlineWidth, shadowColor[4], shadowOffset[2], pixelsPerUnit, sortingOrder`,全部带缺省;字段类型新增 `[f32;2]`。渲染:宿主 `render_core::text` 用 fontdue 在 CPU 排版光栅化成贴图,包成 `SpriteRenderInfo` 走精灵腿(alpha 混合、恒不色键、锚点 = 文字框中心),rurix 与 Godot 两后端、点选与屏外裁剪同路径生效;贴图按内容 + 字体文件身份 + 资产代次缓存,rurix 会话签名带上文字贴图身份(改字即刷新,代价一次会话重建)。字体缺失或解析失败不画,并发宿主事件 `TEXT_FONT_MISSING{font, reason}`(I-5,不偷换系统字体;rurix 的 `viewport.frame`、Godot 的流水线取帧、推流循环三条取帧路径都会取走登记的问题,同一 (字体, 原因) 在被取走前只登记一次)。模型腿(ModelRenderer 场景)暂不绘制 Text。

- **E-09-002(2026-08-31,F-GAME-4 帧动画波 / D-031)——`Sprite` 组件字段扩展与动画运行时**:①`Sprite` 新增可选字段 `sprite(.rxsprite 图集 GUID,缺省 "")`、`clip(当前/起始动画 clip 名,缺省 "")`、`frame(clip 内当前帧序号,缺省 0)`;`texture` 转可选(缺省 ""),与 `sprite` 二选一非空(Script module/graphRef 先例,注册表不做跨字段校验)。**兼容红线**:texture 直贴模式渲染行为不变(整图 + 居中锚);脚底锚(pivot 级联)仅 .rxsprite 模式生效。②渲染:图集帧经 **UV 子矩形进 push constants**(96B→112B:model+tint+tex_size+flip+uv_rect,≤128B 上限)实现零会话重建的逐帧切换——贴图槽位构建期静态绑定(签名只含 w/h/len),「逐帧换贴图 GUID」路线不可行(同尺寸换图画面不刷、异尺寸 1-5s 全量重建),为 D-031 驳回项;世界尺寸 = 帧 bbox 像素/ppu × scale,pivot 锚定折入模型矩阵平移(点选 OBB 同步用渲染态变换,画面与点选一致)。③运行时:宿主侧动画系统(engine-host `anim.rs`)在逻辑帧后推进——dt 驱动 while 跨帧不丢帧、`duration` 优先、非循环 hold/first、animator 状态机(转换表有序首匹配、trigger 触发即消费、`hasExitTime` 等 clip 播毕);**`Sprite.frame/clip` 的唯一写者是宿主**,图节点只发 `AnimCommand`(避免双写者);PIE 语义:仅 run_scene,play.exit 清空,编辑态零介入。模式判定(play 初始化时定):实体 `clip` 为空且文档带 animator → **FSM 模式**(状态机独占 clip 选择,手控 sprite.play/stop/set_frame 如实 `anim.warn` 忽略,I-5);实体显式写 `clip` → **手动模式**(纯 clip 播放,FSM 不介入)——同一 .rxsprite 可被不同实体分别以两种模式使用(活体 E2E 实测:anim_demo 双僵尸)。
- **E-09-001(2026-08-31,F-GAME-3 2D 支持波 / D-030)——§3.2 组件集扩展与场景级字段**:①`Camera` 组件实装字段改为 `projection(perspective|orthographic,缺省 perspective)`、`orthoSize(正交半高,缺省 5.0)`、`fov/near/far`(均转可选带缺省 60/0.1/500);§3.2 表行「fov、near、far、active」中的 `active` 仍未实装(沿用首发注记)。②新增 `Sprite` 组件(2D 精灵):`texture(贴图 GUID,必填)`、`tint [f32;4]`、`flipX/flipY`、`pixelsPerUnit(缺省 100,世界尺寸=贴图像素/ppu × scale)`、`sortingOrder(叠放次序,小者先绘)`;渲染走视口贴图 quad 腿扩展(96B push constants:model+tint+tex_size+flip),点选按有效缩放参与。③`Scene` 顶层新增 `mode("2d"|"3d",缺省 3d)` 与 `gravity([f32;3],缺省 [0,-9.81,0])` 两字段,缺省值跳过序列化——旧 3D 场景 load→save 逐字节同态不变;`scene_new` 的 mode 缺省跟随项目 forge.toml `[project] mode`。④注册表 `FieldSpec` 新增 `default`(可选字段缺省字面量),`normalize_props` 在 RPC 边界补齐缺省,落库 props 恒全量;`RigidBody.mass` 同步转可选(缺省 1.0)。⑤2D 坐标约定:XY 平面、z=0、相机 +Z 朝 -Z、重力 -Y;俯视/零重力玩法经场景 `gravity=[0,0,0]` 表达(play.enter 按场景重力重建物理世界,rurix-physics 无 set_gravity 面)。
