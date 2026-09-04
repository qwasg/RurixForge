# 03 · 引擎层(rurix 内核复用面与 engine-host 契约)

> 事实源 = rurix 仓库(`H:\rurix`)。本章只定义**复用面**与**新增宿主层**;
> 渲染/物理算法实现一律使用 rurix 既有代码(不变量 I-1),不在本仓重写。

## 1. 复用总览

| rurix 组件 | 路径 | 本引擎用途 | 消费方式 |
|---|---|---|---|
| `rurix-render` | `src/rurix-render` | 全部实时渲染:render graph、虚拟化几何、VSM、GI、RT、材质流送、TAA/TSR | Cargo 库依赖,`vulkan` feature 按 rurix 默认(off) |
| `rurix-physics` | `src/rurix-physics` | 全部物理:固定步世界、查询、接触事件、渲染合流桥 | Cargo 库依赖,默认 `jolt` 后端;`rapier` 为 CI/交叉验证档 |
| `rurix-geom-build` | `src/rurix-geom-build` | 素材管线离线几何构建:网格→meshlet→简化层级 DAG | assetd 库依赖 |
| `rurix-geometry` | `src/rurix-geometry` | mesh/BVH 工具(零依赖全 safe) | assetd/engine-host 库依赖 |
| `image-io` / `soft-raster` | `src/image-io` `src/soft-raster` | 图像落盘、CPU 参照 | assetd、测试 |
| `rx` 工具链 | `src/rx` | `.rx` 脚本 build/check/run/fmt/test/doc | code-forge-mcp 子进程 |
| `rurix-rt-cabi` | `src/rurix-rt-cabi` | 宿主编排 C ABI(`rxrt_*`/`rxp_*`/`rxio_*`) | 编译期 `.rx` 单源应用的运行时边界,engine-host 加载游戏代码时复用 |
| `rurix-engine` C ABI 先例 | `src/rurix-engine` | cdylib + 随附头文件单一事实源模式 | 仅作模式参考;本引擎引擎不导出 C ABI(D-004:宿主=进程而非 DLL) |

## 2. 渲染复用面(rurix-render,G5)

模块 ↔ 用途(rurix-render `lib.rs` 七报告主线):

| 模块 | 内容 | 引擎使用点 |
|---|---|---|
| `graph` | 声明式 render graph:compile/sync/transient/resources/dump | engine-host 每帧构建并执行场景帧图;`dump` 供 debug 模式 agent 读图诊断 |
| `geometry` | `GpuScene`(host 侧场景:mesh/instance/变换写口/DirtyRange/part group)、GPU 两级剔除、VisBuffer | Entity 的 MeshRenderer 组件 ↔ `GpuScene::add_instance` / `update_transform`;DirtyRange 驱动增量上传 |
| `material` | 材质 closure/table/PSO cache | Material 资产 ↔ closure;PSO cache 跨帧复用 |
| `shadow` | VSM clipmap/page_table/pool | 灯光组件阴影默认 VSM |
| `gi` | 屏幕探针 GI(probe/sh/tracer/temporal/filter) | GI 设置 = 场景级渲染配置 |
| `rt` | AS 管理/BVH/降噪/效果 | RT 效果开关 = 渲染配置;AS 由 as_manager 统一维护 |
| `streaming` | 材质/资源流送 engine/feedback/pool | 大场景流送;`StreamingBridge` 与物理移除联动 |
| `temporal` | TAA/TSR/upscale/ssim | 抗锯齿与超分配置;ssim 供 playtest 图像断言 |

引擎侧约定:

- **跨帧资源**(TAA 历史、VSM 页表、GI 探针历史)一律外部资源 import,不入 transient——沿用 rurix-render 架构纪律,engine-host 负责持有。
- **场景描述事实源 ≠ GpuScene**:`.rxscene` 是事实源;`GpuScene` 是其派生运行态,可由场景重载全量重建(I-4)。
- 渲染配置(阴影分辨率、GI 开关、TAA/TSR 档位、RT 开关)集中在 `.rxscene` 的 `renderSettings` 块,engine-scene-mcp 提供 get/set 工具。

## 3. 物理复用面(rurix-physics,G6.2)

### 3.1 世界与句柄

- `PhysicsWorld::new(WorldDesc)` 创建;后端 `BackendKind::Jolt`(生产默认,feature `jolt`)/ `BackendKind::Rapier`(feature `rapier`,CI 与交叉验证)。**引擎发布构建 = jolt;无后端构建档在 CI 维持恒绿**(`PhysicsWorld::new` → `Err(BackendNotCompiled)`,I-5)。
- `BodyId` / `ShapeId` 不透明句柄(index 32b + generation 32b);Entity 的物理组件只持有这些句柄,永不触原生指针。
- `PhysicsTransform { translation, rotation /* xyzw quat */ }` 为物理→渲染唯一桥接输入;引擎侧 Transform 组件序列化用 TRS,桥接处转换(3×4 组合用 bridge 的 `compose_transform_3x4`)。

### 3.2 步进与同步

- 固定步 `step(dt_fixed)`:accumulator 在 engine-host 帧循环;变步长调用 = `Err(FixedStepMismatch)`。
- `SyncBudget` 每帧重置:物理→渲染变换同步、接触事件、查询三轴预算;饱和 = 确定性截断 + 计数(`budget_saturation()`),帧统计上报事件通道。
- 渲染合流:`PhysicsBridge`(物理 → `GpuScene` 单向变换同步)+ `StreamingBridge`(流送批插移除,`RemovalReceipt` 先卸后放)。Entity 销毁涉及流送资源时必须走 StreamingBridge 顺序。

### 3.3 查询与事件(agent 高频使用)

- `cast_ray(QueryRay)` / `cast_shape` / `overlap`:step 外并发;cast 结果 `(t, BodyId)` 规范序 = 确定性。engine-scene-mcp 暴露为场景查询工具(拾取、视线检测、范围收集)。
- `drain_contacts()`:`ContactEvent`(Begin/Persist/End)有界 ring + 规范序归一化;引擎逻辑事件总线的一等来源(`10 §3`)。
- `add_bodies_batch` / `remove_bodies_batch`:批量摆放 agent 操作的物理对应面。

### 3.4 组件映射

| 引擎组件 | 物理 API |
|---|---|
| `RigidBodyComponent` | `BodyDesc`(kind: Static/Dynamic/Kinematic,mass props)→ `add_bodies_batch` |
| `ColliderComponent` | `ShapeDesc`(box/sphere/capsule/convex/trimesh)→ 挂到 body |
| `CharacterControllerComponent` | Kinematic body + 每帧 `cast_shape` 扫掠(脚本驱动) |
| `TriggerVolumeComponent` | sensor body + `drain_contacts` 过滤 |

## 4. 几何构建复用面(rurix-geom-build)

- 能力:网格输入 → meshlet 化 → 分组简化层级 DAG → 序列化(`serialize.rs`);CPU 参照剔除器(`cull_ref.rs`)供测试对拍。
- assetd 在导入构建阶段调用,产物 = `.rxmesh`(meshlet DAG + 材质槽绑定)+ 统计(顶点/三角形/meshlet 数、LOD 层级数)。
- 构建必须 host 纯 safe 确定性:同一输入哈希 → 同一产物哈希(缓存键基础,`08 §4.3`)。
- 非虚拟化路径:小件/动态物体可构建为普通 GPU 网格(`geometry/gpu_layout.rs` 布局),由资产 `.meta` 的 `geometryPath: virtualized|standard` 决定。

## 5. engine-host 进程契约(新增宿主层)

engine-host 是本引擎唯一新增的内核侧进程 = rurix-render + rurix-physics + forge-scene 的宿主。

### 5.1 生命周期

- 由 engine-scene-mcp 的 `autoStart` 拉起;`--project <dir>` 指定项目,`--scene <path>` 可选预载场景。
- 健康检查:控制通道 `host.ping`;看门狗(engine-scene-mcp 内)检测无响应 → 标记 `HostCrashed` 事件 → 自动重启并恢复到最近 checkpoint 场景(`12 §4`)。
- GPU 设备丢失/Vulkan 错误 → 结构化错误上报事件通道,不静默重建(I-5);用户可经 Chat 让 agent 诊断(graph dump + 日志)。

### 5.2 控制通道(JSON-RPC 2.0)

方法族(engine-scene-mcp 工具的一一底层):

| 方法族 | 示例方法 | 说明 |
|---|---|---|
| `scene.*` | `load` `save` `new` `diff` | 场景加载/保存/新建/与磁盘 diff |
| `entity.*` | `create` `destroy` `reparent` `rename` `get` `list` `batchApply` | Entity CRUD 与批量操作 |
| `component.*` | `add` `remove` `set` `get` `listTypes` | 组件增删改查;`listTypes` 返回组件注册表(供 agent 发现可用组件) |
| `transform.*` | `set` `get` `batchSet` | TRS 读写 |
| `camera.*` | `setEditorCamera` `getEditorCamera` | 视口相机 |
| `play.*` | `enter` `pause` `resume` `step` `exit` `state` | PIE 控制;`step` = 单帧推进(Unity 对齐) |
| `physics.*` | `castRay` `castShape` `overlap` `bodyGet` `impulse` | §3.3 映射 |
| `render.*` | `getSettings` `setSettings` `graphDump` | 渲染配置与诊断 |
| `viewport.*` | `screenshot` `pickEntity` `setGizmoState` | 截图、屏幕坐标拾取(内部用 cast_ray)、gizmo 状态同步 |
| `sim.*` | `runHeadless` `injectInput` `getState` | 无头仿真(playtest-mcp 底层) |

错误:JSON-RPC error object `{ code, message, data }`;`code` 段划分见 `11 §5`。

### 5.3 帧通道

- 首选:同机 D3D12 共享纹理(rurix 已有 CUDA–D3D12 interop / D3D12 运行时先例),IDE 在原生组件(Electron `sharedTexture` 或原生子窗口句柄嵌入)中呈现。
- 回退:host 侧 RAW/H.264 编码 → loopback 流 → IDE `<canvas>` 解码绘制;仅用于开发排障,不作为默认体验。
- 帧协商:IDE 连接时声明期望分辨率/格式/帧率上限;host 按 `SyncBudget` 语义独立限流,帧通道积压 = 丢帧不阻塞仿真。
- 输入转发:IDE 采集 Viewport 区域键鼠 → `input.inject` 控制消息 → host 编辑器相机或 PIE 输入队列(Play 状态下输入归游戏)。

### 5.4 PIE(Play-In-Editor)

- `play.enter`:host 克隆当前编辑态场景为运行态(`PhysicsWorld` 重建,实体映射重建);编辑态冻结但可检视。
- 运行中 Hierarchy/Inspector 显示运行态(对标 Unity:运行态修改不持久化,退出即恢复编辑态;UI 上以配色区分)。
- `play.exit`:销毁运行态,编辑态原样恢复;运行态中的 dump(用户主动 `snapshot`)可另存为新场景,不进默认流。

## 6. 引擎层红线

- engine-host 以外任何进程不得 `use rurix_render` / `rurix_physics`(assetd 仅 geom-build/geometry/image-io)。
- 不引入 rurix 之外的渲染/物理依赖(红线 R-4)。
- 物理确定性:同一 `.rxscene` + 同一输入序列 → 同一状态序列;playtest 回放依赖此性质,破坏即 P0 缺陷。

## Errata(只追加区)

- **E-03-001(2026-08-28,双仓对账波 / D-028)——§1/§2/§5 as-built 勘误**:①§1 表 `rurix-render` 行「全部实时渲染…Cargo 库依赖」**不实**——截至本日零 crate 依赖 `rurix-render`;视口实渲染建在 `rurix-rt`(features=["vulkan"])的 `render_exec` 库面(`DeviceFrameSession` 固定 pass 图 + 相机 UBO + 逐实体 push constants + Readback)上,着色器为 engine-host 内嵌 WGSL 源经 naga 纯 Rust 编译 SPIR-V(不经 rurixc)。②§1 表 `rurix-geometry` 行「assetd/engine-host 库依赖」不实——两侧均未依赖。③§1 表 `rurix-rt-cabi` 行「engine-host 加载游戏代码时复用」不实——图解释执行走 forge-logic 自有 callruntime(`rurixc --emit=dll` 子进程 + libloading,缓存键含 rurixc.exe 字节 SHA-256);`#[export(c)]` 导出面为文本级扫描(rurixc `--emit=reflection` 对宿主 fn 产空,RXS-0304 实测留痕)。④§5 首段「engine-host = rurix-render + rurix-physics + forge-scene 的宿主」→ as-built = rurix-rt(vulkan)+ rurix-physics + soft-raster + forge-scene + forge-logic + forge-util + assetd + rurix-geom-build(2026-08-28 起含后两者:资产→视口断链接线)。⑤§2 渲染复用面补一行:2026-08-28 起 `MeshRenderer.mesh` 引用(.meta GUID/路径/文件名)经 assetd 同款 cache_key → `.forge/cache/rxmesh/<key>.rxmesh` → 叶层簇重建 → GPU 出帧(资产→视口断链接线,engine-host meshres)。**I-1 不变量维持不变**:内核单一、不自研第二渲染器;升级到 `rurix-render`/`rurix-renderer-sdk` 稳定面留待其覆盖本仓场景后按需立项(D-028),届时本章正文与 §1 表一并修订,本勘误不回写。

- **E-03-003(2026-09-03,PvZ 可玩化波 / D-037)——指针输入反投影与屏外精灵裁剪**:①`logic.inject_pointer {x,y,action?,width?,height?}` RPC + 视口 WS `pointer` 消息:归一化坐标经场景相机射线(`viewport::scene_camera_ray`,正交=平行射线/透视=自眼发散,与 `scene_camera_view_proj` 同参数解析)与游戏平面求交(2d:z=0;3d:y=0),同帧按序入队 `<action>_x/_y/_z` + `<action>`(`rpc::queue_pointer`);aspect 取调用方画面尺寸(WS 取订阅者流尺寸)。②`render_scene_frame` 对 **Sprite 实体**做保守四角裁剪(`sprite_offscreen`):四角全在 **3 倍**视口范围外(或全在相机后)才不进 renderables——对象池停车位(y=-60/-70/-80)不再占 128 draw 槽;边距取 3 倍是因可见集每变一次 pass 会话就按新贴图槽签名重建,贴边进出(右侧刷出/飞出屏)若逐帧裁剪会造成会话重建抖动。非 Sprite 实体不裁,`draws/truncated` 语义不变(只算真正提交的槽)。
- **E-03-002(2026-08-31,F-GAME-3 2D 支持波 / D-030)——正交投影与场景重力**:①`EditorCamera` 增 `ortho`/`ortho_half_h`,新增 `orthographic_vk()`(与 perspective_vk 同 NDC/y-flip 约定);`viewport.setCamera` 收 `ortho`/`orthoSize`;`viewport_pick` 正交分支为平行射线。②PIE 场景相机(`scene_camera_view_proj`)读 Camera 组件 `projection`/`orthoSize`,2D 游戏 PIE 画面为正交正视,无相机实体的 2D 场景经 scene_load/scene_new 联动编辑器相机切正交。③场景级重力:`.rxscene` 顶层 `gravity` 在 play.enter 与当前物理世界比对,不同则按场景重力重建世界(edit 态 body_map 恒空,重建安全;rurix-physics `BodyDesc` 无轴锁/重力缩放面,2D 动态体靠「全员 z=0 的 3D box 物理自然留 XY 平面」约定,轴锁列为未来上游扩展项,不改 pinned 上游仓)。④精灵渲染:贴图 quad 腿扩展消费 `Sprite` 组件实体(无需 MeshRenderer),按 (sortingOrder, 场景序) 稳定排序绘制,tint/flip 经 push constants 下发(88B→96B)。
