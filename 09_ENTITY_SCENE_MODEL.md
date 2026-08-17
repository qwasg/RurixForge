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
