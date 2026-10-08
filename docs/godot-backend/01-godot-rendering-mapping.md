# 01 · Godot 4.7.2 渲染接口映射（RenderingServer / RenderingDevice / gdext 0.5.5）

> 进度：已完成 §0-§9（第 3 轮之后由父会话亲自续写，2026-09-28）
> 日期 2026-09-27；Godot `4.7.2-stable`（commit `ed1daf0bf001b61586d9930840f2f1394092c079`），gdext `=0.5.5`（feature `api-4-7`）。
> 记号：`GD` = `D:\godot-src\4.7.2-stable`（只读参考源码）；`RF` = `D:\RurixForge`；引用写成 `GD/路径:行号`、`RF/路径:行号`。
> 姊妹文档：`00-spike-report.md`（冒烟结论）、`02-render-seam-design.md`（engine-host 接缝设计，另一 agent 编写，本文不触碰）。

> 后续实施与验收：见 [03-completion.md](03-completion.md)。本文保留接口研究时的结论；实际能力、修正和未通过项以最新验收记录为准。

## 0. 结论速览

- **线程（T1-T3，§1）**
  - `thread_model=Safe`，所有 RS / RD 调用只在 Godot 主线程发出。
  - 宿主是 SceneTree 的子类，每帧在 `process()` 里应用差量，由引擎自己 `sync` + `draw`，在 `frame_post_draw` 发起帧导出，结果在回调里交付。
  - forge 的 RPC、物理、推流线程只和主线程交换纯 Rust 数据，不在锁里同步等帧。
  - 节拍：vsync 关、low_processor_mode 关、max_fps = 推流帧率。
- **相机与坐标（C1，§2）**：两边都是右手系、Y 朝上、相机看 −Z。
  - fov 直接传入；正交用 `camera_set_orthogonal(2 × orthoSize)`。
  - 相机变换用 `Basis::looking_at(fwd, UP)`，复现 forge 丢掉滚转的行为。
  - Godot 以顺时针为正面，写入三角形时交换顶点顺序。
- **帧导出（X1-X3，§3）**
  - L1 = D3D12 且 LUID 相同时：RD 的 `texture_copy` 拷到导入的中间纹理 → 我方在 Godot 主队列上 `CopyTextureRegion` 到现有的共享 buffer（RGBA8，行距按 256 对齐）→ 共享 fence。presenter 和 `bind buf …` 协议都不改。
  - L2 = `RD::texture_get_data_async`，Compatibility 下用 `texture_2d_get`，结果规整为 RGBA8。Vulkan 和 GLES3 一律走 L2。
  - 视口纹理是 `R8G8B8A8_UNORM`，存的是已经做过 sRGB 编码的值。
- **运行时（R1、D1，§4）**
  - 就绪行沿用 engine-host 的格式，行首 `FORGE_HOST_LISTENING`，监督器逐行扫描前缀。
  - 运行时目录 = 官方模板 exe + project.godot + .gdextension + `.godot/extension_list.cfg` + 空的 `global_script_class_cache.cfg` + `forge_runtime.tscn` + `bin/godot_host.dll`，不带 `--path`。
  - gdext：`#[class(base=SceneTree)]` 可行，不开 `experimental-threads`，RID 用 RAII 管理。
- **3D 映射（§5）**
  - MeshRenderer → 炸开的平直着色 surface。
  - ModelRenderer / `.mat` → StandardMaterial3D 或 ORMMaterial3D，逐字段对应。
  - Light → directional / omni / spot，场景里没有灯时生成缺省灯光来逼近 rurix 写死的灯光。
  - Animator → CPU 算姿态 + skeleton_* 做 GPU 蒙皮。
  - ParticleEmitter → particles_* + ParticleProcessMaterial。
  - V6 → MultiMesh（每实例 16 个 float）。
- **环境（§6）**
  - Environment / Sky / CameraAttributes / ReflectionProbe / Decal / FogVolume / Compositor 都可以不经过节点，直接调 RS。
  - VoxelGI 必须经过节点烘焙，先用 SDFGI 代替。
  - tonemap 有 AgX。
  - Stage 5 的 schema 缺省值取 Godot 的缺省值。
  - **Stage 5 已接**：7 个组件（Environment / CameraAttributes / RenderSettings / ReflectionProbe / Decal / FogVolume / LightParams），Environment 与 CameraAttributes 经资源对象设值（`environment_set_ssil` 没有绑定给 GDExtension）；粒子样式预设改成 canvas 叠加层解析复刻，与 rurix 最大差 ≤ 3。实测结论见 §6 开头。
- **能力差异（K1，§7）**
  - Forward+ 功能最全。
  - Mobile 缺 SSAO / SSIL / SSR / SDFGI / VoxelGI / 体积雾 / TAA / 自动曝光。
  - Compatibility 没有 RD（不能走 L1），也没有贴花、DOF、FXAA / SMAA / TAA。
- **2D（S1、S2，§8）**
  - 纯 2D 场景走 Canvas，其余场景把精灵当 3D quad 画。
  - 精灵的尺寸、锚点、翻转、排序、色键和三种混合都按 forge 的着色器规则复刻，色键和混合用 6 个 canvas shader 变体实现。
  - 2D 相机 = 画布变换，缩放 S = H / (2·orthoSize)。
- **主要风险（§9）**：L1 的状态跟踪要在 Stage 3 用 debug layer 验证；双显卡下的 LUID 匹配；两个后端灯光的一致性。

## 1. 帧生命周期与线程

### 1.1 thread_model 与 RS 调用的线程约束

- 设置键 `rendering/driver/threads/thread_model`，取值是 `OS::RenderThreadMode`：`RENDER_THREAD_UNSAFE`=0（已弃用，实际按 Safe 处理）、`RENDER_THREAD_SAFE`=1（缺省）、`RENDER_SEPARATE_THREAD`=2（`GD/core/os/os.h:52-54`；编辑器提示串见 `GD/core/config/project_settings.cpp:1774-1777`）。
  - 启动时只判断"是不是 Separate"（`GD/main/main.cpp:2775-2776`）。
  - 命令行 `--render-thread safe|separate` 优先于设置（`GD/main/main.cpp:1570-1581`）。编辑器和项目管理器强制 Safe（`GD/main/main.cpp:2779-2785`）。
  - 选 Separate 会打印"实验特性，可能崩溃"的警告（`GD/main/main.cpp:3517-3520`）。
- `RenderingServerDefault(create_thread)` 在 `GD/main/main.cpp:3565` 构造。
  - Safe：`server_thread = Thread::MAIN_ID`（`GD/servers/rendering/rendering_server_default.cpp:286`、`:299`）。
  - Separate：渲染在 WorkerThreadPool 任务 `_thread_loop` 里执行（同文件 `:277-282`、`:415`）。
- 从非 server 线程调 RS 时的行为：由 `ASYNC_COND_PUSH / _AND_RET / _AND_SYNC` 决定，三者都是 `Thread::get_caller_id() != server_thread`（`GD/servers/rendering/rendering_server_default.h:117-119`），宏体在 `GD/servers/server_wrap_mt_common.h:44-73`。
  - 无返回值的调用：压进 `command_queue`，异步执行。
  - 有返回值的调用（`FUNC*R`）：`push_and_ret`，调用线程一直阻塞，直到 server 线程冲刷队列。
  - `*_create()`（`FUNCRIDSPLIT`）：当场分配 RID 返回，初始化命令入队。
  - Safe 模式下，队列只在两种时机冲刷：主线程调 `RS::sync()`（`command_queue.flush_all()`，`rendering_server_default.cpp:435-441`），或主线程下一次调 RS（`flush_if_pending`）。
  - 所以 forge 的 RPC / 物理线程如果直接调带返回值的 RS 函数，会一直阻塞到主线程下一次冲刷；主线程此时若在等这个线程，就会死锁。
- `RS::draw()` 只允许在主线程调用（`rendering_server_default.cpp:444`，`ERR_FAIL_COND_MSG(!Thread::is_main_thread(), ...)`）。
- `call_on_render_thread(Callable)`：调用者就在 server 线程时，先 `flush_if_pending` 再直接调用；否则入队（`GD/servers/rendering/rendering_server_default.h:1216-1222`）。
  - RD 层的一些操作要求在渲染线程执行（`ERR_NOT_ON_RENDER_THREAD`，`GD/servers/rendering/rendering_server.h:47-58`，仅 DEBUG 构建检查）。
  - Separate 模式下这类操作必须经由 `call_on_render_thread`；Safe 模式下主线程就是渲染线程。

> **结论 T1（线程模型）**：宿主固定用 Safe（`thread_model=1`，即缺省值）。所有 RS / RD 调用只在 Godot 主线程发出，也就是 SceneTree 子类的 `process()` 回调，以及 `frame_post_draw` / frame-drawn 回调。forge 的 RPC、物理、推流线程一律不直接调 RS，只通过消息 / 快照交接给主线程（对应 02 §2.4 的 `[gmain]`）。

### 1.2 `Main::iteration` 一帧的先后顺序

`Main::iteration()` 从 `GD/main/main.cpp:4921` 开始，按下面的顺序执行：

1. 物理步（0..N 次）：`main_loop->physics_process()`（`:5001`），随后是 2D / 3D 导航的 `physics_process`（`:5019`、`:5023`）。forge 不接这一步（确定性红线）。
2. `main_loop->process()`（`:5062`），然后 `message_queue->flush()`（`:5065`），再是导航的 `process`。
3. `RenderingServer::sync()`（`:5077`）：Safe 模式下冲刷其他线程积压的 RS 命令。
4. `RenderingServer::draw(wants_present, scaled_step)`（`:5089` 或 `:5093`）。
   - `wants_present` 要求有窗口可以绘制（`can_any_window_draw()`）或有额外输出，并且 render loop 没被关掉（`:5080-5083`）。条件不满足、又没有待处理的 RD 资源时，这一帧整帧不 draw（`:5085`）。这就是窗口最小化时停绘的原因，与 00 §2.1 一致。
   - low_processor_mode 下，只有 `RS::has_changed()` 为真才 draw（`:5087-5090`）。
5. 帧节流：`OS::add_frame_delay()`（`:5178`）；`--fixed-fps` 时跳过这一步（`:5170-5172`）。

SceneTree 子类里 `process()` 所处的位置（`GD/scene/main/scene_tree.cpp:688-739`）：

- `SceneTree::process` 先调 `MainLoop::process(p_time)`（`:700`），它通过 `GDVIRTUAL_CALL(_process, ...)` 调用扩展类的 `_process`（`GD/core/os/main_loop.cpp:67-69`），也就是 gdext 里 override 的 `process()`。
- 之后才是 `process_frame` 信号、节点处理、MessageQueue 冲刷、timers / tweens，最后是 `_call_idle_callbacks()`（`:739`）。
- 材质着色器的重建队列就挂在 idle callback 上（00 §2.9）。所以我们在 `process()` 里改的材质属性，会在同一帧 draw 之前落到 RS。

### 1.3 `frame_pre_draw` / `frame_post_draw` / frame-drawn 回调

- `RS::draw()` 先发 `frame_pre_draw` 信号（`GD/servers/rendering/rendering_server_default.cpp:446`），再执行 `_draw`。Safe 模式下 `_draw` 同步执行（`:448-452`）。
- `_draw` 的最后调用 `_run_post_draw_steps()`：Safe 模式直接调用（`:133`），Separate 模式通过 `call_deferred` 回到主线程再调（`:131`）。
  - 它先按顺序调用 `request_frame_drawn_callback()` 登记的一次性回调（登记 `:72-74`，执行 `:215-227`），再发 `frame_post_draw` 信号（`:229`）。
- 结论：Safe 模式下，`frame_post_draw` 和 frame-drawn 回调都在主线程上、同一次 `RS::draw()` 之内触发，此时本帧的 GPU 命令已经提交。这是发起帧导出（§3）的时机。
  - 此时 GPU 不一定已经执行完。RD 的回读或拷贝要靠 fence 或异步回调才能确认完成（见 §3）。

### 1.4 max_fps、vsync、low_processor_mode

- `application/run/max_fps`（`GD/main/main.cpp:2255`，缺省 0 表示不限）。
  - `OS::add_frame_delay`（`GD/core/os/os.cpp:708-741`）：max_fps > 0 且不是编辑器时，按 1e6 / max_fps 的目标节拍睡眠。
  - 开启 low_processor_mode 或窗口不可绘制时，至少睡 `low_processor_usage_mode_sleep_usec`。
  - 另有 `application/run/frame_delay_msec`，是固定的额外延迟（`main.cpp:2856`）。
- `display/window/vsync/vsync_mode`（`main.cpp:2839`），缺省 `VSYNC_ENABLED`：present 会被显示器刷新率节流。
  - 宿主窗口在屏外、不可见，节拍应该由 max_fps 决定，所以设为 0（Disabled），与 00 §3 的 project.godot 一致。
- `application/run/low_processor_mode`（`main.cpp:2263`）：开启后只有 RS 有改动才 draw（`main.cpp:5087-5090`），并额外睡眠。
  - 对编辑器视口来说，"没改动就不出帧"能省电，但会让按节拍出帧的推流停下来。
  - 宿主缺省关闭。要不要在空闲时开启，留作 Stage 3 的优化项。

> **结论 T2（节拍）**：宿主的 project.godot 固定为 `thread_model=Safe`、`vsync_mode=0`、`low_processor_mode=false`、`max_fps`=推流目标帧率（缺省 60）。

### 1.5 推荐时序（宿主是 SceneTree 子类）

每个 Godot 帧，在主线程上依次是：

1. `ForgeHost::process(delta)`（override 的 `_process`）：从交接箱取最新的 `RenderList`，与已应用状态做 diff，然后发出 RS 调用（instance / mesh / material / camera / 视口尺寸）。
2. SceneTree 自身的处理和 idle callbacks：材质着色器的变更在这里落到 RS。
3. `RS::sync()` + `RS::draw()`：由引擎自动调用，不需要我们手动调。
4. 在 `frame_post_draw`（或 `request_frame_drawn_callback` 的一次性回调）里，对本帧的视口纹理发起导出：L1 在 Godot 的队列上录制拷贝并 signal 共享 fence，L2 调 `texture_get_data_async`（§3）。
5. 导出完成的回调（下一帧或更晚）：把像素规整成紧凑 RGBA8，带上 seq 交给 FrameSink。

> **结论 T3（时序）**：一帧 = `process()` 里应用差量 → 引擎 draw → `frame_post_draw` 发起导出 → 回调交付。交付至少晚 1 帧，L2 更晚（§3）。宿主不自己调 `RS::draw()`，也不在锁里同步等帧。

## 2. 视口 / 相机 / scenario / instance API 与坐标系

### 2.1 精确签名

以下全部出自 `GD/servers/rendering/rendering_server.h`，行首数字是行号。4.7 起这些枚举放在 `RSE::` 命名空间，定义在 `rendering_server_enums.h`；gdext 里的命名见 §4。

```text
// 相机
523 RID  camera_create()
524 void camera_set_perspective(RID p_camera, float p_fovy_degrees, float p_z_near, float p_z_far)
525 void camera_set_orthogonal(RID p_camera, float p_size, float p_z_near, float p_z_far)
526 void camera_set_frustum(RID p_camera, float p_size, Vector2 p_offset, float p_z_near, float p_z_far)
527 void camera_set_transform(RID p_camera, const Transform3D &p_transform)
528 void camera_set_cull_mask(RID p_camera, uint32_t p_layers)
529 void camera_set_environment(RID p_camera, RID p_env)
530 void camera_set_camera_attributes(RID p_camera, RID p_camera_attributes)
532 void camera_set_use_vertical_aspect(RID p_camera, bool p_enable)
// 视口
536 RID  viewport_create()
542 void viewport_set_size(RID p_viewport, int p_width, int p_height, int p_view_count = 1)
543 void viewport_set_active(RID p_viewport, bool p_active)
550 void viewport_set_scaling_3d_mode(RID p_viewport, RSE::ViewportScaling3DMode p_scaling_3d_mode)
551 void viewport_set_scaling_3d_scale(RID p_viewport, float p_scaling_3d_scale)
552 void viewport_set_fsr_sharpness(RID p_viewport, float p_fsr_sharpness)
553 void viewport_set_texture_mipmap_bias(RID p_viewport, float p_texture_mipmap_bias)
554 void viewport_set_anisotropic_filtering_level(RID p_viewport, RSE::ViewportAnisotropicFiltering p_level)
556 void viewport_set_update_mode(RID p_viewport, RSE::ViewportUpdateMode p_mode)
559 void viewport_set_clear_mode(RID p_viewport, RSE::ViewportClearMode p_clear_mode)
561 RID  viewport_get_render_target(RID p_viewport) const
562 RID  viewport_get_texture(RID p_viewport) const
565 void viewport_set_disable_3d(RID p_viewport, bool p_disable)
566 void viewport_set_disable_2d(RID p_viewport, bool p_disable)
568 void viewport_attach_camera(RID p_viewport, RID p_camera)
569 void viewport_set_scenario(RID p_viewport, RID p_scenario)
570 void viewport_attach_canvas(RID p_viewport, RID p_canvas)
571 void viewport_remove_canvas(RID p_viewport, RID p_canvas)
572 void viewport_set_canvas_transform(RID p_viewport, RID p_canvas, const Transform2D &p_offset)
573 void viewport_set_transparent_background(RID p_viewport, bool p_enabled)
574 void viewport_set_use_hdr_2d(RID p_viewport, bool p_use_hdr)
582 void viewport_set_global_canvas_transform(RID p_viewport, const Transform2D &p_transform)
583 void viewport_set_canvas_stacking(RID p_viewport, RID p_canvas, int p_layer, int p_sublayer)
587 void viewport_set_positional_shadow_atlas_size(RID p_viewport, int p_size, bool p_16_bits = true)
590 void viewport_set_msaa_3d(RID p_viewport, RSE::ViewportMSAA p_msaa)
591 void viewport_set_msaa_2d(RID p_viewport, RSE::ViewportMSAA p_msaa)
593 void viewport_set_screen_space_aa(RID p_viewport, RSE::ViewportScreenSpaceAA p_mode)
595 void viewport_set_use_taa(RID p_viewport, bool p_use_taa)
597 void viewport_set_use_debanding(RID p_viewport, bool p_use_debanding)
603 void viewport_set_use_occlusion_culling(RID p_viewport, bool p_use_occlusion_culling)
610 void viewport_set_debug_draw(RID p_viewport, RSE::ViewportDebugDraw p_draw)
// scenario
715 RID  scenario_create()
717 void scenario_set_environment(RID p_scenario, RID p_environment)
718 void scenario_set_fallback_environment(RID p_scenario, RID p_environment)
719 void scenario_set_camera_attributes(RID p_scenario, RID p_camera_attributes)
720 void scenario_set_compositor(RID p_scenario, RID p_compositor)
// instance
724 RID  instance_create2(RID p_base, RID p_scenario)          // 非纯虚，基类实现
726 RID  instance_create()
728 void instance_set_base(RID p_instance, RID p_base)
729 void instance_set_scenario(RID p_instance, RID p_scenario)
730 void instance_set_layer_mask(RID p_instance, uint32_t p_mask)
731 void instance_set_pivot_data(RID p_instance, float p_sorting_offset, bool p_use_aabb_center)
732 void instance_set_transform(RID p_instance, const Transform3D &p_transform)
734 void instance_set_blend_shape_weight(RID p_instance, int p_shape, float p_weight)
735 void instance_set_surface_override_material(RID p_instance, int p_surface, RID p_material)
736 void instance_set_visible(RID p_instance, bool p_visible)
738 void instance_teleport(RID p_instance)
740 void instance_set_custom_aabb(RID p_instance, AABB aabb)
742 void instance_attach_skeleton(RID p_instance, RID p_skeleton)
744 void instance_set_extra_visibility_margin(RID p_instance, real_t p_margin)
747 void instance_set_ignore_culling(RID p_instance, bool p_enabled)
758 void instance_geometry_set_flag(RID p_instance, RSE::InstanceFlags p_flags, bool p_enabled)
759 void instance_geometry_set_cast_shadows_setting(RID p_instance, RSE::ShadowCastingSetting p_setting)
760 void instance_geometry_set_material_override(RID p_instance, RID p_material)
762 void instance_geometry_set_visibility_range(RID p_instance, float p_min, float p_max, float p_min_margin, float p_max_margin, RSE::VisibilityRangeFadeMode p_fade_mode)
765 void instance_geometry_set_transparency(RID p_instance, float p_transparency)
```

RS 相机的缺省值（`GD/servers/rendering/renderer_scene_cull.h:93-101`）：`fov=75`、`znear=0.05`、`zfar=4000`、`size=1.0`、`vaspect=false`、`visible_layers=0xFFFFFFFF`。这些缺省值和 forge 的都不一样，宿主必须每次显式设置。

### 2.2 相关枚举（全部取值，`GD/servers/rendering/rendering_server_enums.h`）

以下数值按声明顺序从 0 起。gdext 里去掉前缀，写成 `rendering_server::ViewportUpdateMode::ALWAYS` 这种形式（§4.4）。

- `ViewportUpdateMode`（`:492-498`）：`DISABLED`=0、`ONCE`=1（画一次后自动变成 DISABLED）、`WHEN_VISIBLE`=2（缺省）、`WHEN_PARENT_VISIBLE`=3、`ALWAYS`=4。
  - 离屏视口不在屏幕树里，用 WHEN_VISIBLE 永远不会刷新，必须设成 ALWAYS（00 冒烟已验证），或者按需用 ONCE。
- `ViewportClearMode`（`:500-504`）：`ALWAYS`、`NEVER`、`ONLY_NEXT_FRAME`。
- `ViewportMSAA`（`:528-534`）：`DISABLED`、`2X`、`4X`、`8X`、`MAX`。
- `ViewportScreenSpaceAA`（`:536-541`）：`DISABLED`、`FXAA`、`SMAA`、`MAX`。
- `ViewportScaling3DMode`（`:456-465`）：`BILINEAR`=0、`FSR`=1、`FSR2`=2、`METALFX_SPATIAL`=3、`METALFX_TEMPORAL`=4、`NEAREST`=5、`MAX`=6、`OFF`=255（内部用）。
  - FSR2 和 METALFX_TEMPORAL 属于时间型缩放，其余属于空间型（`:474-481`）。
- `ViewportAnisotropicFiltering`（`:483-490`）：`DISABLED`、`2X`、`4X`、`8X`、`16X`、`MAX`。
- `ViewportEnvironmentMode`（`:506-511`）：`DISABLED`、`ENABLED`、`INHERIT`、`MAX`。
- `ViewportDebugDraw`（`:563-593`，共 29 个，0..28）：
  - `DISABLED`、`UNSHADED`、`LIGHTING`、`OVERDRAW`、`WIREFRAME`、`NORMAL_BUFFER`
  - `VOXEL_GI_ALBEDO`、`VOXEL_GI_LIGHTING`、`VOXEL_GI_EMISSION`
  - `SHADOW_ATLAS`、`DIRECTIONAL_SHADOW_ATLAS`、`SCENE_LUMINANCE`、`SSAO`、`SSIL`、`PSSM_SPLITS`、`DECAL_ATLAS`
  - `SDFGI`、`SDFGI_PROBES`、`GI_BUFFER`、`DISABLE_LOD`
  - `CLUSTER_OMNI_LIGHTS`、`CLUSTER_SPOT_LIGHTS`、`CLUSTER_DECALS`、`CLUSTER_REFLECTION_PROBES`
  - `OCCLUDERS`、`MOTION_VECTORS`、`INTERNAL_BUFFER`、`CLUSTER_AREA_LIGHTS`、`AREA_LIGHT_ATLAS`
  - 最后两个与 4.7 的面光源有关。
- `InstanceType`（`:767-783`）：`NONE`、`MESH`、`MULTIMESH`、`PARTICLES`、`PARTICLES_COLLISION`、`LIGHT`、`REFLECTION_PROBE`、`DECAL`、`VOXEL_GI`、`LIGHTMAP`、`OCCLUDER`、`VISIBLITY_NOTIFIER`（源码拼写如此）、`FOG_VOLUME`、`MAX`。
- `InstanceFlags`（`:786-792`）：`USE_BAKED_LIGHT`、`USE_DYNAMIC_GI`、`DRAW_NEXT_FRAME_IF_VISIBLE`、`IGNORE_OCCLUSION_CULLING`、`MAX`。
- `ShadowCastingSetting`（`:794-799`）：`OFF`、`ON`、`DOUBLE_SIDED`、`SHADOWS_ONLY`。
- `VisibilityRangeFadeMode`（`:801-805`）：`DISABLED`、`SELF`、`DEPENDENCIES`。

### 2.3 坐标系与换算

**forge 的约定**（`RF/crates/engine-host/src/viewport.rs`）：
- 右手系、Y 轴朝上。`look_at_rh` 的注释写的是"RH;-z 为前向"（`:121-133`）。
- 场景相机沿自身 -Z 看：`scene_camera_view_proj`（`:1607`）里 `fwd = rot·(0,0,-1)`（`:1629`）。构造 view 矩阵时用世界 up `(0,1,0)`（`:1639`），所以相机的滚转会被丢掉。
- 四元数按 `[x,y,z,w]` 存（`quat_to_mat3`，`:134-135`）。
- 投影是 Vulkan 风格，z∈[0,1]（`perspective_vk` `:95-106`、`orthographic_vk` `:107-120`）。
  - `fov` 是垂直视角，单位度。
  - `orthoSize` 是正交的半高（`:108`）。
- 两处投影 Y 对角元取负（编辑器相机 `:254`、场景相机 `:1638`），是 rurix 为回读行序做的显示约定（注释 `:243-247`），Godot 侧不用照搬。
- 编辑器相机是绕 `target` 转的轨道相机（`EditorCamera`，`:197-219`）：缺省 `fov_y_deg=50`，`eye()` 在 `:223-233`，near/far 固定为 0.05/500（`view_proj`，`:248-257`）；`ortho` 模式用 `ortho_half_h`。

**Godot 的约定**：
- 同样是右手系、Y 朝上、相机看 -Z。`camera_set_transform` 接收的是相机的世界变换，不是 view 矩阵。
- `camera_set_perspective` 的 `p_fovy_degrees`：vaspect=false（缺省）时就是垂直视角，只有 `p_flip_fov` 为真才换算（`GD/core/math/projection.cpp:252-256`）。
- `camera_set_orthogonal` 的 `p_size`：vaspect=false 时是**全高**。源码先做 `p_size *= p_aspect`，再取 `±p_size/p_aspect/2` 作为上下边界（`GD/core/math/projection.cpp:356-361`）。

> **结论 C1（相机与变换换算）**
> - 透视：`camera_set_perspective(cam, fov, near, far)`，数值和 forge 相同。
> - 正交：`camera_set_orthogonal(cam, 2 × orthoSize, near, far)`；编辑器相机用 `2 × ortho_half_h`。
> - `camera_set_use_vertical_aspect(false)` 保持缺省。
> - 场景相机的变换：`Transform3D(Basis::looking_at(fwd, Vector3::UP), translation)`。`Basis::looking_at` 在 `GD/core/math/basis.h:231`，这样可以复现 forge 丢掉滚转的行为。
> - 编辑器相机的变换：`Transform3D(Basis::looking_at(target − eye, UP), eye)`，其中 eye 按 `viewport.rs:223-233` 计算。
> - 实例：`instance_set_transform(Transform3D(Basis(Quaternion(x,y,z,w)) · Basis::from_scale(s), t))`。世界矩阵按父链累乘，与 rurix 用同一个中立函数（02 §3）。
> - 视口：`viewport_set_size(vp, w, h)`，RS 按视口尺寸算 aspect，与 forge 的 aspect = w/h 一致。
> - 行序：Godot `Image` 回读时顶行在前，不需要复制 rurix 的 y-flip。L1 路径 GPU 纹理的行序**未核实**，Stage 3 用像素测试确认。

## 3. 帧导出（L1 零拷贝 / L2 回读）

### 3.1 取纹理的链路，以及视口纹理的格式、usage、sRGB

- 链路：`RS::viewport_get_texture(vp)` 得到 RS 纹理 RID（`GD/servers/rendering/rendering_server.h:562`）→ `RS::texture_get_rd_texture(tex, p_srgb=false)` 得到 RD 纹理 RID（`:156`）→ `RD::get_driver_resource(DRIVER_RESOURCE_TEXTURE, rd_tex, 0)` 得到原生对象（§3.2）。
  - `texture_get_native_handle(tex, srgb)`（`:157`）可以一步拿到原生句柄，但它不经过 RD，拿不到 RD 层的状态跟踪，所以不采用。
- 视口 render target 的颜色纹理在 `TextureStorage::_update_render_target` 里创建（`GD/servers/rendering/renderer_rd/storage_rd/texture_storage.cpp:4263`、`:4283-4316`）。
  - 格式：非 HDR 时是 `R8G8B8A8_UNORM`，并声明可共享的 `R8G8B8A8_SRGB` 视图；开了 `use_hdr_2d` 时是 `R16G16B16A16_SFLOAT`（`render_target_get_color_format`，`:5285-5290`）。
  - usage：`SAMPLING | COLOR_ATTACHMENT | CAN_COPY_FROM | STORAGE`（`render_target_get_color_usage_bits(false)`，`:5293-5298`）。MSAA 的中间纹理只有 `COLOR_ATTACHMENT`，最终结果会 resolve 到上面这张。
  - 数值编码：非 HDR 时 tonemap 这一步写入的是**已经 sRGB 编码**的值（`tonemap.convert_to_srgb = !using_hdr`，`GD/servers/rendering/renderer_rd/renderer_scene_render_rd.cpp:752`、`:961`；着色器 `linear_to_srgb`，`.../shaders/effects/tonemap.glsl:230`、`:918`）。所以 UNORM 字节可以直接显示，和 rurix 的 sRGB RGBA8 输出同一语义。
- 00 报告里"RD 路径回读是 RGB8"的原因：`RS::texture_2d_get` 会把数据转换成 render target 的 `image_format`，而视口背景不透明时这个格式是 `FORMAT_RGB8`（透明时是 `RGBA8`，`texture_storage.cpp:4286-4290`）。如果直接用 `RD::texture_get_data(_async)` 读 RD 纹理，得到的是 4 字节 / 像素、紧凑行的原始 `R8G8B8A8` 数据（`GD/servers/rendering/rendering_device.cpp:2861-2882` 按 `tight_mip_size` 布局）。
  - 背景不透明时 alpha 通道里是什么值**未核实**。规整成 RGBA8 时一律强制 A=255。

### 3.2 `get_driver_resource` 与各驱动返回的原生对象

- 签名：`uint64_t RenderingDevice::get_driver_resource(DriverResource p_resource, RID p_rid = RID(), uint64_t p_index = 0)`（`GD/servers/rendering/rendering_device.h:1957`）。它要求在渲染线程调用（`ERR_RENDER_THREAD_GUARD_V`，`rendering_device.cpp:8714-8715`）。
  - `COMMAND_QUEUE` 固定返回 main queue（`:8723-8725`）。纹理类的 RID 必须是 RD 纹理（`:8729-8736`）。
- `DriverResource` 枚举（`GD/servers/rendering/rendering_device_commons.h:945-957`）：`LOGICAL_DEVICE`、`PHYSICAL_DEVICE`、`TOPMOST_OBJECT`、`COMMAND_QUEUE`、`QUEUE_FAMILY`、`TEXTURE`、`TEXTURE_VIEW`、`TEXTURE_DATA_FORMAT`、`SAMPLER`、`UNIFORM_SET`、`BUFFER`、`COMPUTE_PIPELINE`、`RENDER_PIPELINE`。`VULKAN_*` 是它们的别名（`:959-971`）。
- D3D12 下的返回值（`GD/drivers/d3d12/rendering_device_driver_d3d12.cpp:5735-5781`）：

| DriverResource | D3D12 返回 |
|---|---|
| LOGICAL_DEVICE | `ID3D12Device*`（`device.Get()`） |
| PHYSICAL_DEVICE | `IDXGIAdapter*`（`adapter.Get()`），可以用来取 LUID |
| TOPMOST_OBJECT / QUEUE_FAMILY / SAMPLER / UNIFORM_SET | 0 |
| COMMAND_QUEUE | main queue 的 `ID3D12CommandQueue*` |
| TEXTURE | 纹理的 `ID3D12Resource*`；是别名视图时取 `main_texture` 的资源 |
| TEXTURE_VIEW | 该视图自己的 `ID3D12Resource*` |
| TEXTURE_DATA_FORMAT | `DXGI_FORMAT`（`desc.Format`） |
| BUFFER | `ID3D12Resource*` |
| COMPUTE / RENDER_PIPELINE | 驱动内部 ID |

- Vulkan 下按别名名字返回 `VkDevice`、`VkPhysicalDevice`、`VkInstance`、`VkQueue`、队列族索引、`VkImage`、`VkImageView`、`VkFormat`、`VkBuffer` 等。这是按别名推断的，没有逐行读 Vulkan 驱动的实现，**未逐行核实**。
- RD 能导入外部**纹理**：`texture_create_from_extension`（`rendering_device.h:465`；D3D12 驱动有实现，`rendering_device_driver_d3d12.cpp:1500`）。但 RD 的公开 API 里**没有**导入外部 buffer 的函数（`rendering_device.h` 里只有 `texture_create_from_extension`，没有 `buffer_create_from_extension`）。

### 3.3 D3D12 的屏障模型与帧末资源状态

- 驱动在运行时选屏障模型：`barrier_capabilities.enhanced_barriers_supported` 为真时用 Enhanced Barriers（布局 `D3D12_BARRIER_LAYOUT_*`），否则用 legacy 的 `D3D12_RESOURCE_STATE_*`。代码里到处按这个标志分支，例如 `rendering_device_driver_d3d12.cpp:921`、`:1381-1383`、`:3659`、`:4434-4436`。
- 资源转换由 RD 的 draw graph 按每个资源"最后一次 usage"自动插入。一帧结束后，视口纹理停留在它最后一次被使用时的布局或状态。offscreen 视口最后一次使用通常是作为颜色附件被写入（tonemap / canvas pass），但具体是哪个布局取决于 graph 的实现，**未核实**。
- 结论：外部代码直接对视口纹理打屏障，需要准确知道"当前状态"和"当前屏障模型"，否则会破坏 RD 的状态跟踪。L1 方案因此绕开这个问题（§3.5 的 a2 方案）。

### 3.4 现有共享对象（L1 要对接的目标，presenter 不改）

- 共享体是**线性 buffer**，不是纹理：DEFAULT 堆 + `D3D12_HEAP_FLAG_SHARED`（`RF/crates/engine-host/src/share.rs:39-45`；创建流程是 `share.rs:133` 起的 `Producer::open`）。
- 布局：RGBA8（`DXGI_FORMAT_R8G8B8A8_UNORM`），行距 = ceil(w·4/256)·256，总字节 = 行距 × 高（`share.rs:30-37`，`shared_layout`）。presenter 用同一公式校验，不一致就报错（`RF/crates/viewport-presenter/src/main.rs:134-139`）。
- 同步：共享 fence，生产者每帧 `Signal(++v)`。
  - CPU 上传档：`share.rs:258-317`，写完后等 GPU，最多 2 s。
  - 零拷贝档：`share.rs:320` 起，只推进 fence。
  - presenter 轮询 `fence.GetCompletedValue()`，值大于已呈现的值才拷一次（`main.rs:198-207`）。拷贝用 placed footprint（R8G8B8A8_UNORM + RowPitch）做 `CopyTextureRegion`，拷进 swapchain 的后台缓冲（`main.rs:241-253`）。
- 句柄交接：presenter 从 stdin 读 `bind buf <buffer 句柄> <fence 句柄> <w> <h> <rowPitch>`（`main.rs:444-457`；旧的 `bind tex|heap` 形式已经退役）。句柄由 engine-host 用 `DuplicateHandle` 复制进 presenter 进程（02 §1.6）。
- 设备：producer 和 presenter 都用 `D3D12CreateDevice(None, FL_11_0)` 创建设备，也就是**缺省 adapter**（`share.rs:102`、`main.rs:66`）。共享 heap 只能在同一个 adapter（同一 LUID）上打开，这是 L1 的硬性前提。
  - 本机是 RTX 5060 Laptop + Intel iGPU 的双显卡。缺省 adapter 和 Godot 选中的 adapter 不一定是同一个，Godot 的 D3D12 驱动会自己按评分挑设备。所以 L1 必须在运行时比较 LUID（§3.5 第 1 步），或者启动时用 `--gpu-index` 把 Godot 固定到同一块卡（§4）。

### 3.5 三种方案的比较与选定

| 方案 | 做法 | 优点 | 问题 | 结论 |
|---|---|---|---|---|
| (a) 在 Godot 队列上提交我方的 CopyTextureRegion，用共享 fence 同步 | 在 Godot 的 `ID3D12Device` 上按现有布局创建共享 buffer 和 fence；每帧在 Godot 的 `COMMAND_QUEUE` 上执行我方命令列表，把纹理拷进共享 buffer（placed footprint），再 `Signal(fence, ++v)` | presenter 零改动；拷贝在 GPU 内完成，不经 CPU | 要知道源纹理的当前状态 / 布局（§3.3）；只适用于 D3D12，且 LUID 必须相同 | **选为 L1**，用下面的 a2 变体绕开状态问题 |
| (b) 把共享资源导入 RD，用 RD 的 `texture_copy` | 用 `texture_create_from_extension` 导入共享**纹理**，帧内调 `texture_copy` | 屏障全部由 RD 管理 | presenter 消费的是 **buffer**，而 RD 没有导入外部 buffer 的 API（§3.2），只能改 presenter，违反约束 | 否决。将来如果允许改 presenter，可以重新考虑 |
| (c) L2 回读 | `RD::texture_get_data_async` 取紧凑 RGBA8 → CPU → 现有 CPU 出口（WS 帧、`share.rs` 的 CPU 上传档） | 所有驱动都能用（GLES3 改用 `RS::texture_2d_get`）；不碰 Godot 内部状态 | 要走 GPU→CPU→GPU 往返；延迟等于帧队列深度 | **选为 L2**：通用兜底，也是 GLES3 唯一的选择 |

**L1 采用 a2 变体**（不去猜视口纹理的当前状态），步骤如下：

1. 初始化（主线程，RD 已可用）：取 `LOGICAL_DEVICE`、`COMMAND_QUEUE`、`PHYSICAL_DEVICE`。用 `IDXGIAdapter::GetDesc().AdapterLuid` 和 presenter / share 使用的缺省 adapter 比较 LUID（share.rs 已有 `adapter_luid`，见 02 §1.6），不同就降级到 L2。
2. 在 Godot 的 device 上创建两样东西：
   - 一张中间纹理 `export_tex`：R8G8B8A8_UNORM，DEFAULT 堆，普通的非共享资源。用 `RD::texture_create_from_extension(TEXTURE_TYPE_2D, DATA_FORMAT_R8G8B8A8_UNORM, TEXTURE_SAMPLES_1, CAN_COPY_TO|CAN_COPY_FROM, resource_ptr, w, h, 1, 1, 1)` 导入 RD。
   - 按 `shared_layout(w, h)` 建共享 buffer 和共享 fence。创建和 `DuplicateHandle` 的流程沿用 share.rs，只是把 device 换成 Godot 的；然后给 presenter 发同样的 `bind buf …` 命令。
3. 每帧的 `frame_post_draw` 里调 `RD::texture_copy(vp_rd_tex, export_rd_tex, …)`。
   - 这条命令会录进**下一帧**的 RD 命令缓冲。本帧在 `RS::_draw` 的 `end_frame` → `swap_buffers` 时就已提交（`GD/servers/rendering/rendering_server_default.cpp:114-115`、`GD/servers/rendering/renderer_rd/renderer_compositor_rd.cpp:135-136`），post-draw 步骤排在它之后（`rendering_server_default.cpp:130-134`）。
   - 两张纹理的屏障都由 RD graph 负责。拷贝之后，`export_tex` 的最后一次 usage 是"拷贝目标"。
4. 下一次 `frame_post_draw` 时，上一步的拷贝已经随该帧提交。此时在 Godot 的 `COMMAND_QUEUE` 上执行我方命令列表：
   - 屏障：`export_tex` 从拷贝目标转到拷贝源；
   - `CopyTextureRegion`：从 `export_tex` 子资源 0 拷到共享 buffer 的 placed footprint；
   - 屏障：转回拷贝目标；
   - 最后 `Signal(shared_fence, ++v)`。
   - 同一队列按提交顺序执行，所以读到的一定是 RD 拷贝完成之后的内容。
   - 屏障 API 跟 Godot 驱动的选择保持一致（§3.3）：`CheckFeatureSupport(OPTIONS12).EnhancedBarriersSupported` 为真用 Enhanced（`LAYOUT_COPY_DEST ↔ LAYOUT_COPY_SOURCE`），否则用 legacy（`COPY_DEST ↔ COPY_SOURCE`）。
5. 我方的命令分配器 / 命令列表各做 3 个轮转，用一个我方私有的 fence 保证复用安全。尺寸变化时重建 `export_tex` 和共享 buffer，并重新发 `bind`。

以下两点**未核实**：导入的 `export_tex` 在 RD 拷贝之后具体处于哪个布局；RD 能否容忍外部改动它的布局。第 4 步把布局转回原样，就是为了让 RD 的状态跟踪继续成立。Stage 3 必须打开 D3D12 debug layer 验证。

a1 变体（直接从视口纹理拷贝，能省一帧）要等状态问题核实之后，再作为优化来做。

### 3.6 帧队列深度、延迟与 GLES3

- RD 帧队列深度：`rendering/rendering_device/vsync/frame_queue_size`，缺省 2（`GD/doc/classes/ProjectSettings.xml:3388`），运行时取 `MAX(2, 设置值)`（`GD/servers/rendering/rendering_device.cpp:8368`）。swapchain 图像数缺省 3（`ProjectSettings.xml:3393`、`rendering_device.cpp:5357`）。
- `texture_get_data_async` 的回调：在对应帧的 fence 完成、帧槽被复用时才触发。函数本体从 `rendering_device.cpp:2792` 开始，请求挂在 `frames[frame]` 上（`:2861-2901`）。
- 延迟估计（按 60 fps 推算，未实测）：
  - L1-a2 约 2 帧：RD 拷贝要等下一帧提交，再加我方这一次拷贝。
  - L1-a1 约 1 帧。
  - L2 约 1 + frame_queue_size ≈ 3 帧，另外还有 CPU 拷贝；走共享档时还要再上传一次。
- 带宽：推流尺寸上限是 1280×720（`RF/crates/engine-host/src/stream.rs:34-43`，见 02 §1.6），RGBA8 每帧不超过 3.7 MB，60 fps 时 L2 回读约 220 MB/s，可以接受。
- GLES3（Compatibility）没有 RenderingDevice（00 §1），只能走 L2，而且只能用同步的 `RS::texture_2d_get`（`GD/servers/rendering/rendering_server.h:131`），CPU 会等 GPU。建议这个模式下推流上限设为 30 fps。

> **结论 X1（L1）**：D3D12 且 LUID 相同时，走方案 (a) 的 a2 变体：先用 RD 的 `texture_copy` 拷到导入的中间纹理，再由我方在 Godot 主队列上 `CopyTextureRegion` 到现有布局的共享 buffer，最后推进共享 fence。presenter 和 `bind buf …` 协议都不用改。
>
> **结论 X2（L2）**：Forward+ / Mobile 在任何驱动下用 `RD::texture_get_data_async`，Compatibility 用 `RS::texture_2d_get`。结果规整为紧凑 RGBA8（A 强制为 255），交给现有的 CPU 出口（WS 帧、`share.rs` 的 CPU 上传档）。
> - Vulkan 一律走 L2：Godot 的 Vulkan 驱动没有启用 Win32 外部内存（`GD/drivers/vulkan/SCsub:36` 显式设了 `VMA_EXTERNAL_MEMORY_WIN32=0`），导入不了 D3D12 的共享 buffer。
>
> **结论 X3（发起与降级）**：导出一律在主线程的 `frame_post_draw` 里发起，结果通过 FrameSink 交付。导出失败只做降级（L1 → L2），不停止出帧。

## 4. 运行时工程、启动参数、gdext 0.5.5 要点

### 4.1 运行时工程需要设置的 ProjectSettings 键（均已在源码核实）

| 键 | 取值 | 出处 | 说明 |
|---|---|---|---|
| `application/config/name` | `"ForgeHost"` | `GD/core/config/project_settings.cpp:1691` | |
| `application/run/main_loop_type` | `"ForgeHost"` | 定义 `project_settings.cpp:1707`（缺省 `"SceneTree"`），读取 `GD/main/main.cpp:4365` | 填 gdext 注册的 SceneTree 子类名 |
| `application/run/main_scene` | `"res://forge_runtime.tscn"` | 定义 `project_settings.cpp:1696`，检查 `main.cpp:2342`，加载 `main.cpp:4336` 起 | 必须非空，否则 setup2 报错并走 OS::alert（00 §2.2/2.3） |
| `application/run/max_fps` | 60 | `main.cpp:2255` | 结论 T2 |
| `application/run/low_processor_mode` | false | `main.cpp:2263` | 结论 T2 |
| `display/window/size/viewport_width` / `viewport_height` | 64 / 64 | `project_settings.cpp:1719-1720`，`main.cpp:2686-2687` | 主窗口不参与出帧，取小值 |
| `display/window/size/mode` | 0（Windowed） | `project_settings.cpp:1722`，`main.cpp:2729` | 绝不能设为 1（Minimized），会停绘 |
| `display/window/size/borderless` | true | `main.cpp:2711` | |
| `display/window/size/no_focus` | true | `main.cpp:2723` | |
| `display/window/size/initial_position_type` | 0（Absolute） | `project_settings.cpp:1725`，`main.cpp:2730` | |
| `display/window/size/initial_position` | `Vector2i(-32000, -32000)` | `main.cpp:2733` | 放到屏外 |
| `display/window/vsync/vsync_mode` | 0 | `main.cpp:2839` | 结论 T2 |
| `rendering/renderer/rendering_method` | `forward_plus` / `mobile` / `gl_compatibility` | `main.cpp:2647-2653` | 来自 forge.toml `[render].method`（02 §7） |
| `rendering/rendering_device/driver.windows` | `d3d12` / `vulkan`（缺省 `vulkan`） | `main.cpp:2389` | 来自 `[render].driver` |
| `rendering/gl_compatibility/driver.windows` | `opengl3`（另有 `opengl3_angle`） | `main.cpp:2404` | |
| `rendering/driver/threads/thread_model` | 1（Safe） | `main.cpp:2776` | 结论 T1 |
| `rendering/rendering_device/vsync/frame_queue_size` | 2（缺省值） | `rendering_device.cpp:8368` | 影响 L2 延迟（§3.6） |
| `audio/driver/driver` | `"Dummy"` | `main.cpp:2802`、`:2810`；驱动名见 `GD/servers/audio/audio_driver_dummy.h:61` | 不用 Godot 音频 |

- 物理：决定是不用 Godot 物理（红线）。能不能把 `physics/3d/physics_engine`（`GD/servers/physics_3d/physics_server_3d.cpp:1163`）设成空实现，**未核实**：在 `servers/physics_3d` 下没找到名为 "Dummy" 的注册。Stage 3 用 `--verbose` 列出可选引擎后再定；在那之前，只要不创建任何物理体，开销可以忽略。

### 4.2 release 模板可用的命令行参数

- 可用性标注：`print_help_option(..., p_availability = CLI_OPTION_AVAILABILITY_TEMPLATE_RELEASE)`（`GD/main/main.h:51`）。也就是说，没写第三个参数的选项在 release 模板里都能用。
- 显式标成 `TEMPLATE_UNSAFE` 的选项，只在编辑器和用 `disable_path_overrides=false` 编译的模板里可用（图例 `main.cpp:547`）：`--path`、`--scene`、`--main-pack`（`:576-580`），`--script`、`--main-loop`、`--check-only`（`:695-697`）。官方模板不接受这些参数，传了直接报错（00 §2.8）。
- 模板启动时会把 CWD 改成 exe 所在目录，再从那里加载工程（`main.cpp:1043-1055`）。加载失败时的错误提示里会专门说明"在原 CWD 找到了 project.godot 也不会用"（`main.cpp:2121-2123`）。
- 宿主要用到的参数（都没有写第三个参数，所以 release 模板可用）：
  - `--rendering-method <m>`（`:621`）、`--rendering-driver <d>`（`:622`）；
  - `--gpu-index <n>`（`:623`，只对 Forward+ / Mobile 有效）；
  - `--audio-driver Dummy`（`:593`）；
  - `--render-thread safe`（`:584`）；
  - `--max-fps <n>`（`:678`）、`--disable-vsync`（`:681`）；
  - `--position <X>,<Y>`、`--resolution <W>x<H>`（`:639-640`）；
  - `--log-file <file>`（`:627`）、`--no-header`（`:558`）、`--disable-crash-handler`（`:683`）；
  - `--verbose`：列出 GPU 和驱动，排障用。
- 不要用 `--headless`（`:626`）：它会切到 headless 显示驱动（`--display-driver headless`），而这个模式下没有可用的窗口 / RD 渲染，本方案需要的离屏出帧就没法做了。这一点**未实测**，是按 headless 驱动的定义推断的。

### 4.3 gdext 0.5.5 要点

- **`#[class(base=SceneTree)]` 可行。**
  - 4.7 的 API 描述里 SceneTree 是 `is_instantiable: true`、`inherits: "MainLoop"`（`gdextension-api-0.5.1/src/4.7/extension_api.json:288599-288603`）。MainLoop 的虚函数 `_initialize` / `_physics_process` / `_process` / `_finalize` 在 `:172180-172230`。
  - godot-codegen 0.5.5 生成 I-trait 时会遍历所有基类，把基类的虚函数一并收进来（`godot-codegen-0.5.5/src/generator/virtual_traits.rs:257-290`，`make_all_virtual_methods` 遍历 `all_base_names`）。所以 `impl ISceneTree for ForgeHost` 可以 override `process`。
  - `process` 的具体签名按 `IMainLoop` 推断为 `fn process(&mut self, delta: f64) -> bool`，**未在生成代码里核实**，因为生成代码要到构建时才产生。
  - 引擎按 `main_loop_type` 用 ClassDB 实例化主循环（00 §1）。
- **RID 与资源的生命周期。**
  - RS 的 RID 不是引用计数的，必须显式调 `RS::free_rid()`。gdext 的 `Rid` 只是普通的 Copy 值，不会自动释放。宿主要用 Rust 侧的 RAII 句柄：Drop 时调 `free_rid`，而且 Drop 只发生在主线程。
  - `StandardMaterial3D`、`BoxMesh` 这类 RefCounted 资源，底层 RID 归资源自己持有：只要还持有 `Gd<T>` 就存活，最后一个 `Gd` 丢掉 RID 就释放。所以缓存表里要存 `Gd`，不能只存 rid。
- **非主线程调 RS，以及 `experimental-threads`。**
  - 按结论 T1，宿主不在其他线程调 RS。
  - gdext 缺省（不开 `experimental-threads`）时，`Callable::from_fn` 只允许在创建它的线程上调用（`godot-core-0.5.5/src/builtin/callable.rs:145-156` 的文档）；`from_sync_fn` 需要打开该 feature（`:245-246`）。实例存储也按这个 feature 切换单线程 / 多线程实现（`src/storage/single_threaded.rs`、`src/storage/multi_threaded.rs`）。
  - 结论：不开 `experimental-threads`。forge 线程和主线程之间只传纯 Rust 数据（mpsc 通道，或 latest-wins 的交接箱），不跨线程传 `Gd`、`Variant`、`Callable`。
- **PackedByteArray 的转换开销。**
  - `as_slice()` 是零拷贝借用（`src/builtin/collections/packed_array.rs:266-275`，底层是 COW 存储）；`to_vec()` 会拷贝（`:228`）；从 `&[u8]` 构造也会拷贝一次。
  - L2 回调里拿到的 PackedByteArray 直接用 `as_slice()` 做规整，写进 FrameSink 的缓冲，能省掉一次拷贝。
- **用 Callable 包装 Rust 闭包。**
  - `Callable::from_fn(name, |args: &[&Variant]| -> R)`（`callable.rs:152`），用在 `texture_get_data_async` 的回调和 `request_frame_drawn_callback` 上。
  - Safe 模式下这两种回调都在主线程触发，用 `from_fn` 就够了。
- **枚举在 gdext 里的命名。**
  - 类作用域的枚举生成成模块路径下的类型，常量去掉公共前缀，例如 `rendering_server::ViewportUpdateMode::ALWAYS`、`rendering_server::EnvironmentBg::COLOR`、`rendering_device::DriverResource::LOGICAL_DEVICE`。00 §4 的冒烟代码就是这么写的，并且编译通过了。
  - 位域（比如 `TextureUsageBits`）生成成 bitfield 类型，用 `|` 组合。**未在生成代码里核实**。
- **"伪缺省"参数必须显式传**（00 §2.7），例如 `get_driver_resource(resource, rid, 0)`。
- **读回实际生效的后端信息**（给 `render.backendInfo` 用，见 02 §4.6）：
  - `RS::get_current_rendering_driver_name()` / `RS::get_current_rendering_method()`（`GD/servers/rendering/rendering_server.h:1041-1042`）：模板可能因为驱动不支持而回退到别的驱动或渲染方式，所以要以这两个返回值为准，不能只信命令行参数；
  - `get_video_adapter_name()`、`get_video_adapter_vendor()`、`get_video_adapter_type()`、`get_video_adapter_api_version()`（`:977-980`）；
  - `RS::get_rendering_device()`（`:1032`）：返回 null 就表示没有 RD，只能走 L2；
  - Godot 版本号：`Engine::get_version_info()`（`GD/core/config/engine.h:196`）。

> **结论 R1（就绪行）**：RPC 端口监听成功后，Godot 宿主的扩展在 stdout 打印一行 `FORGE_HOST_LISTENING port={actual_port}` 并 flush。这与 rurix engine-host 现有的就绪行逐字相同（`RF/crates/engine-host/src/main.rs:66-67`）。
> - 它前面会先出现 gdext 横幅和 Godot 自己的输出，`--no-header` 只能去掉引擎头。所以监督器必须逐行扫描这个前缀，不能假设它是第一行（00 §2.5）。
> - 监督器读到就绪行之后就不再读 stdout，而 Godot 会一直写日志。所以宿主进程要么把日志导向 `--log-file`，要么由监督器持续把 stdout 读空，否则管道写满后进程会阻塞（02 §6.1-§6.2）。
>
> **结论 D1（运行时目录）**：`<runtime>/` 下放这些文件：
> - 官方 release 或 debug 模板的 exe。要拿 stdout 就用 console 版；console 版需要和主 exe 一起放，两者的命名对应规则**未核实**，Stage 3 实测；
> - `project.godot`（按 §4.1 设置）、`forge_host.gdextension`；
> - `.godot/extension_list.cfg`（内容只有一行 `res://forge_host.gdextension`）、`.godot/global_script_class_cache.cfg`（空文件）；
> - `forge_runtime.tscn`（只含一个 Node）；
> - `bin/godot_host.dll`（`crates/godot-host` 编出的 cdylib）。
>
> 不用 `.pck`，也不传 `--path`：模板会自动把 CWD 设成 exe 所在目录（`main.cpp:1043-1055`）。开发期和打包期用同一套布局。

## 5. 3D 映射矩阵

### 5.0 通则

- 每个带渲染组件的实体对应一个 RS instance。
  - 创建：`instance_create2(base, scenario)`（`GD/servers/rendering/rendering_server.h:724`）。
  - 变换：`instance_set_transform(inst, xform)`（`:732`），xform 按结论 C1 由世界矩阵换算。
  - 显隐：组件 `enabled=false` 对应 `instance_set_visible(inst, false)`（`:736`），不销毁 instance。
- 世界矩阵沿 `Parent{entity}` 链累乘（`RF/crates/forge-scene/src/lib.rs:199-202`）。rurix 用的是 `modelrt::entity_world`（`RF/crates/engine-host/src/modelrt.rs:118`），Godot 后端必须调用同一个中立函数（02 §3），不能自己重写一份。
- 资源去重：同一个 `.rxmesh` / 模型 / 纹理 / `.mat` 只建一份 RS 资源，按"引用 + revision"缓存。
  - 这些资源可以跨实体共享，由 RAII 句柄持有（§4.3）。
  - 实体删除时只 `free_rid` 它自己的 instance。
- 所有"未核实"的数值换算（光强、粒子样式），都要在 Stage 4 做 rurix / Godot 对照截图，标定后再定。（Stage 4：光强、法线 Y、AO、V6 轴向、蒙皮方案都已实测定案，见各小节；只剩粒子样式预设，放 Stage 5。）

### 5.1 MeshRenderer{mesh, material}（`RF/crates/forge-scene/src/lib.rs:243-249`）

| 字段 | forge 现状 | RS 调用 | 转换规则 |
|---|---|---|---|
| `mesh` | `.rxmesh` 是 `rurix_geom_build::ClusterDag`：`read_dag` 读入（`RF/crates/engine-host/src/meshres.rs:147-164`），`dag_full_mesh` 把叶层簇拼成 positions + indices（`:128-145`）。rurix 再把三角形炸开，每个顶点带面法线，交错为 pos3+normal3、stride 24（`:22-31`、`:53-75`） | `mesh_create()`（`rendering_server.h:194`）+ `mesh_add_surface_from_arrays(mesh, RSE::PRIMITIVE_TRIANGLES, arrays, Array(), Dictionary(), 0)`（`:213`）；实例 `instance_create2(mesh, scenario)` | `arrays` 长度为 `ARRAY_MAX`（=13，`GD/servers/rendering/rendering_server_enums.h:126-140`），其中 `[ARRAY_VERTEX]` 放炸开后的 `PackedVector3Array`，`[ARRAY_NORMAL]` 放同长度的面法线；其余槽位为 null。format 由存在的数组自动推出（VERTEX \| NORMAL，`:159-173`）。为了和 rurix 的平直着色一致，不放索引、不做平滑法线。每个网格的三角形上限与 rurix 相同（`MAX_MESH_TRIANGLES`，`meshres.rs:157`） |
| `mesh`（绕序） | 右手系，面法线 = (b−a)×(c−a)（`meshres.rs:60-63`），即逆时针为正面 | — | Godot 以**顺时针**为正面（`GD/doc/classes/ArrayMesh.xml:49`），和 forge 相反。所以写入顶点时每个三角形按 (a, c, b) 的顺序排列，法线不变。**Stage 4 实测（g4_mesh）**：真 .rxmesh 单三角正反两面、四种配置都与 rurix 同色（±3）。另查到两点：① 内置 cube 的 ±X 面在 forge 里几何绕序与存的法线**相反**（`render_core/assets.rs` cube_mesh_bytes），rurix 按法线属性着色看不出来，Godot 在 CULL_DISABLED 下会把"背面"的法线取反，两个面就黑了 → `godot-host/src/mesh.rs` 改为**按存的法线定向每个三角形**（几何法线与存的法线同向 → (a, c, b)，反向 → (a, b, c)）；② rurix 光栅管线是 CULL_MODE_NONE 且不翻背面法线（`rurix-rt render_exec.rs:8190`），开放网格从背面看与正面同色，Godot 的 CULL_DISABLED 会翻 → sprite_mesh 腿给每个三角形补一个反绕序的副本（法线不变）、材质用 CULL_BACK，等价于"不剔除、不翻法线"。三角上限在共享的 meshres 加载器里判（`MAX_MESH_TRIANGLES`），超限与 rurix 一样回退 cube；rurix 的 8 个网格类上限（`MAX_MESH_CLASSES`）是它的槽位限制，Godot 腿不继承 |
| `material` | 指向 `.mat`（§5.2 的 schema）。legacy 路径只取其中的 albedo 纹理（`RF/crates/engine-host/src/viewport.rs:1493`、`:1508` 附近），Stage 4 细化 | `instance_geometry_set_material_override(inst, mat_rid)`（`rendering_server.h:760`） | **Stage 4 实测 rurix 语义**：sprite_mesh 腿里，材质带能解码的 albedo 贴图 → 整个实体画成朝 +z 的单位贴图 quad（类 0：不受光、最近邻、clamp、alpha ≤ 0 或 < 0.02 丢弃、洋红色键），网格引用被忽略；否则画网格，颜色 = `entity_tint`（调色板 / 选中橙），材质的 baseColor 不参与。Godot 按同一判据：贴图 quad 用 ShaderMaterial（色键 BaseMaterial3D 做不了），网格用 StandardMaterial3D（Lambert、specular 关、albedo = entity_tint）+ §5.3 的缺省灯。模型腿里的 MeshRenderer 走 rurix `legacy_draw`：无贴图 → 缺省 PBR 材质（`default_material`），有贴图 → unlit quad（MASK 0.02、色键、不经 tonemap，Godot 侧按 Reinhard 逆变换预补偿） |

### 5.2 ModelRenderer / ModelNode 与 `.mat` → BaseMaterial3D

- 组件字段：`ModelRenderer{model, nodeId="", materialOverrides={}, revision=0}`、`ModelNode{model, nodeId}`（`RF/crates/forge-scene/src/lib.rs:183-198`）。
  - rurix 走专门的 UV/PBR 管线（`RF/crates/engine-host/src/modelrender.rs:1`）。顶点输入是 pos / normal / uv / tangent(vec4)（`:141`）。
  - 动画和蒙皮在 CPU 上算（`RF/crates/engine-host/src/modelrt.rs:1`）。
- 映射方式：模型里每个节点、每个 primitive 对应一个 RS mesh surface。
  - `mesh_add_surface_from_arrays` 的 arrays 至少放 `ARRAY_VERTEX` / `ARRAY_NORMAL` / `ARRAY_TANGENT`（每顶点 4 个 float，w 为副切线符号）/ `ARRAY_TEX_UV` / `ARRAY_INDEX`。
  - 有蒙皮时再加 `ARRAY_BONES` / `ARRAY_WEIGHTS`（§5.5）。
  - 绕序处理同 §5.1。`nodeId` 非空时只取该节点子树。`revision` 变化时整体重建 mesh。
- `.mat` 的 schema：`{ version:1, shader:"pbr-default"|"unlit", params:{…}, textures:{…} }`（`RF/crates/assetd/src/material.rs:5`；发布时的写法见 `RF/crates/assetd/src/model/publication.rs:425-435`）。按下表映射到 `StandardMaterial3D`，或者 `ORMMaterial3D`：

| `.mat` / override 字段 | rurix 着色语义（`modelrender.rs`） | Godot `BaseMaterial3D` 属性 |
|---|---|---|
| `baseColor[4]` + `textures.albedo` | base = baseColor × linear(texel)（`:155`） | `albedo_color`，`albedo_texture`。Godot 按 sRGB 解码 albedo 纹理，与 rurix 的 `linear()` 一致 |
| `metallic` / `roughness` + `textures.metallicRoughness` | 按 glTF 约定：rough = roughness × mr.g，metal = metallic × mr.b（`:162`） | `metallic`、`roughness`、`metallic_texture` + `metallic_texture_channel=TEXTURE_CHANNEL_BLUE`、`roughness_texture` + `roughness_texture_channel=GREEN`（`GD/scene/resources/material.h:307-313`）；这两张纹理按线性采样。**Stage 4 并排实测（未解决，移交 Stage 5）**：metallic = 1 的面明显偏暗——mats.metal（base 0.9、rough 0.3）rurix 97，Godot 四配置 30-31。rurix 的环境项 base × 0.14 × ao 对金属照样加（`modelrender/rurix.rs:72`），Godot 的 PBR 金属没有漫反射环境项，COLOR 环境光下也没有反射源可取。要对齐得在 Stage 5 的环境 / 反射源里处理。**Stage 5 已修（有意修正 1）**：缺省环境下 metallic > 0 的材质把缺的 base·metallic·0.14·ao 放进 EMISSION（无贴图 = 颜色，有贴图 = CPU 烘的 sRGB 贴图），全金属（base 0.9、rough 0.3）rurix 97、Godot 98；有 Environment 组件时不补。部分金属（m 0.5）在直射光下偏亮（rurix 的漫反射还乘 (1 − F)，Godot 的 k_model 按电介质标定），列为未决项，数字见 02 §9.5 Stage 5 |
| `textures.occlusion` + `occlusionStrength` | ao = mix(1, tex.r, strength)（`:165`） | `ao_enabled`、`ao_texture` + `ao_texture_channel=RED`、`ao_light_affect`。**Stage 4 实测结论**：两者不对应——rurix 的 ao 只乘 0.14 的环境项，Godot 的 AO 恒作用于环境光、`ao_light_affect` 决定作用于直射光的比例（`scene_forward_clustered.glsl:2141`、`:2233`），所以取 `ao_light_affect = 0`；BaseMaterial3D 没有 strength，把 strength 烘进 AO 贴图（r′ = 1 + s·(r − 1)）。测试贴图左 r=0、右 r=255、strength 0.5：rurix 144 / 152，Godot 146 / 153（g4_material）。occlusion 与 metallicRoughness 同图且 strength = 1 时用 `ORMMaterial3D.orm_texture`（R=AO，G=rough，B=metal） |
| `textures.normal` + `normalScale` | 用切线空间法线贴图，xy 乘 scale（`:161`） | `normal_enabled`、`normal_texture`、`normal_scale`。**Stage 4 实测结论**：glTF 法线贴图的 Y 方向与 Godot 一致（OpenGL 约定，副切线 = cross(n, t)·w），不需要翻 G 通道——切线空间法线 normalize(0, ±0.5, 1) 两块、光从上方来，rurix 175 / 98，Godot 176 / 98（g4_material，四配置）。`normal_scale` 语义不同：rurix 是 xy × scale，Godot 是几何法线与贴图法线之间的 mix（`scene_forward_clustered.glsl:1442`，只在 1 时相等）→ 把 scale 烘进法线贴图（xy × s 后重新归一），Godot 侧 `normal_scale = 1`；Godot 忽略贴图的 z、按 xy 重建，与重新归一后的值一致 |
| `emissive[3]` + `textures.emissive` | emission = emissive × linear(tex)（`:165`） | `emission_enabled`、`emission`、`emission_texture`、`emission_energy_multiplier=1` |
| `alphaMode`：OPAQUE / MASK / BLEND，加 `alphaCutoff` | MASK 时 alpha < cutoff 丢弃，BLEND 时 alpha ≤ 0 丢弃（`:156-157`）；覆盖入口在 `RF/crates/engine-host/src/material_override.rs:29-103` | `transparency` 分别取 `TRANSPARENCY_DISABLED` / `TRANSPARENCY_ALPHA_SCISSOR`（配 `alpha_scissor_threshold`）/ `TRANSPARENCY_ALPHA`（`material.h:186-192`）。**Stage 4 并排实测**：BLEND（红、α 0.5，叠在暗背景上）rurix 111，F+ / Mobile 121，Compatibility 81——Compatibility 在 sRGB 帧缓冲里混合 |
| `doubleSided` | 背面且非双面时丢弃（`:154`） | `cull_mode` 取 `CULL_DISABLED` 或 `CULL_BACK`（`material.h:248-252`）。背面法线翻转 Godot 会自动处理 |
| `shader:"unlit"` | 输出 base + emission（`:167`） | `shading_mode=SHADING_MODE_UNSHADED`（`material.h:202-206`） |
| 过滤 / 重复 / mipmap | 着色器里自己采样，无 mip（`sample_tex`，`:151`） | `texture_filter`（`material.h:170-177`）。**Stage 4 实际取值**：按 glTF sampler 取 `NEAREST`（mag/min = 9728 / 9984 / 9986）或 `LINEAR`，不生成 mip（与 rurix 一致，原设想的 `LINEAR_WITH_MIPMAPS` 会让远处画面与 rurix 不同）；`texture_repeat` 只在所有贴图两轴都是 clamp 时关（BaseMaterial3D 是逐材质开关，没有 MIRRORED_REPEAT，按 repeat 处理） |
| 颜色空间 | baseColor / emissive 是线性值，albedo / emissive 贴图 sRGB 解码 | BaseMaterial3D 的 `albedo_color` / `emission` 是 sRGB（source_color），传 linear_to_srgb(x)；emissive 大于 1 时归一后放进 `emission_energy_multiplier`。实测 emissive (1, 0.5, 0) 在 0.2 灰上：rurix 190/163/85，Godot 191/164/84 |
| 洋红色键（模型纹理） | `flags.z` 打开时 g < 0.5·min(r, b) 的像素丢弃（`:158-159`） | BaseMaterial3D 没有等价功能。需要时换成 `ShaderMaterial` 并加 `discard`（与 §8 精灵共用同一段着色器代码） |
| `materialOverrides{slot: {…}}` | 按 slot 覆盖上面的参数（`material_override.rs:29`） | 给对应 slot 复制一份材质再覆盖参数，用 `instance_set_surface_override_material(inst, surface, mat)`（`rendering_server.h:735`） |

### 5.3 Light{kind, color[3], intensity, castShadow}（`RF/crates/forge-scene/src/lib.rs:257-266`）

- **rurix 的现状：Light 组件没有被任何渲染代码读取。**
  - 在 `crates/engine-host/src` 里全文搜 `"Light"` / `intensity`，零命中。
  - 模型管线用的是写死的方向光：光向 `l = normalize(0.45, 0.8, 0.35)`，辐照系数 3.0，环境项 = base × 0.14 × ao（`RF/crates/engine-host/src/modelrender.rs:163`、`:166`）。
  - `castShadow` 的注释也写明"渲染内核不消费"（`lib.rs:263`）。
- 所以 Godot 后端有两种情况：
  1. **场景里没有启用的 Light 实体**：生成一套"缺省灯光"，尽量逼近 rurix 的画面。
     - 一盏方向光：`directional_light_create()`（`rendering_server.h:303`）+ `instance_create2`，变换取 `Basis::looking_at(-l, UP)`，让光线沿 −l 方向射出。
     - Environment 的环境光：`ambient_source=COLOR`，颜色 (0.14, 0.14, 0.14)，见 §6。
     - tonemap 用 `LINEAR`。
  2. **场景里有 Light 实体**：按下表映射，不再生成缺省灯光。

> **Stage 4 实测与标定（g4_light、g4_mesh；数值写在 `RF/evidence/godot-backend/stage4/calibration/`）**
> - 两条腿的写死光照不同，缺省灯分腿生成（`godot-host/src/light.rs`）：
>   - **模型腿**：rurix = Reinhard c/(1+c) 再 pow(1/2.2)，c = base·0.14·ao + (漫反射 + 高光)·nl·3 + emission。Godot 用同向方向光（l = normalize(0.45, 0.8, 0.35)）、energy **k_model = 0.9167**（= 3/π·(1 − F₀)，非物理单位下 Lambert = albedo·energy·NdotL）、COLOR 环境光白 × 0.14、反射源关、**tonemap = REINHARD（white 1000）**。原定的 LINEAR 做不到：rurix 自己就做了 Reinhard，线性映射无法同时贴合不同角度和不同反照率；Godot 4.7 的 Reinhard = color·(1 + color/white²)/(1 + color)，white 取大就是 c/(1+c)。清屏色与不经 tonemap 的旧贴图 quad 按 Reinhard 逆变换预补偿。灯的 `LIGHT_PARAM_SPECULAR` 取 1.0（RS 缺省 0.5 会让高光只有一半，饱和色暗通道 10 vs rurix 23）。白 PBR 灰卡（roughness 1）：NdotL = 1 / 0.8 / 0.6 / 0.4 / 0.2 / 0 → rurix 189 / 181 / 170 / 156 / 135 / 98，Godot 190 / 182 / 172 / 157 / 136 / 98，四种配置相同，**逐卡差 ≤ 2 LSB**；线性空间最小二乘给出 0.924，输出空间误差反而变大，所以保留 0.9167。剩余差异来源：rurix 用 pow(1/2.2)、Godot 用分段 sRGB 编码，暗通道（如纯红面的 G/B）低约 6 LSB。
>   - **sprite_mesh 腿**：rurix = color_sRGB × (0.28 + 0.72·ndl)，L = normalize(0.45, 0.75, 0.35)，在 sRGB 编码值上相乘、无 tonemap。线性 Lambert 做不到处处相等，于是让最常见的轴向面逐面相等：目标线性比 t = lin(c·s)/lin(c)（c 取调色板中值 0.7），ambient = t(ndl = 0) = **0.0711**，三个正轴面解出方向 L′ = (0.3961, 0.8755, 0.2767)、energy **k_sprite = 0.7229**；Lambert、specular 关、tonemap LINEAR。cube 六个面（调色板 [0.62, 0.78, 0.52]）：F+ / Vulkan / Compat 逐面差 ≤ 2、Mobile ≤ 4。任意朝向的误差（调色板灰平面 quad）：NdotL = 1 / 0.8 / 0.6 / 0.4 / 0.2 / 0 → −16.3 / −10.3 / −4.0 / −1.0 / −4.0 / −1.3 LSB（法线接近 L 时偏暗最多，属拟合取舍）。
> - **k 的定义**：Light 实体 energy = k_leg × intensity，即 intensity 1 的灯正照白面时等于该腿缺省灯在 NdotL = 1 时的亮度。实测模型腿 directional 强度 1 正照白面 190（预期 190）、强度 2 → 213。
> - RS 创建的灯参数缺省值与节点不同（`light_storage.cpp:145-165`：RANGE 1、SPECULAR 0.5、SHADOW_MAX_DISTANCE 0 → 方向光阴影根本不画），按 Godot 节点缺省补齐（`scene/3d/light_3d.cpp:489-511`、`:614-619`）。阴影实测：F+ / Vulkan / Mobile 斜照白墙 185，遮挡区 98（只剩环境项）。**Compatibility 下 castShadow 不生效**：GLES3 把投影灯放进附加 pass、各 pass 分别 tonemap 后在 sRGB 里相加（`drivers/gles3/shaders/scene.glsl:3067-3070`），Reinhard 下亮部直接到 255，已在 `render.capabilities.coverage` 写明。
> - rurix 同一场景加 Light 前后逐字节相同（不读 Light），所以有 Light 的场景两后端只能近似。

| 字段 | RS 调用 | 转换规则 |
|---|---|---|
| `kind` | `directional_light_create()` / `omni_light_create()` / `spot_light_create()`（`rendering_server.h:303-305`） | `"directional"`、`"point"`、`"spot"` 分别对应三个函数；其他值记告警后按 directional 处理。4.7 新增了 `LIGHT_AREA`（`rendering_server_enums.h:235-240`），forge 暂时不映射 |
| `color[3]` | `light_set_color(light, Color)`（`:308`） | 按线性颜色直接传入，alpha 取 1 |
| `intensity` | `light_set_param(light, RSE::LIGHT_PARAM_ENERGY, v)`（`:309`；LightParam 的全部取值见 `rendering_server_enums.h:242-265`） | energy = k_leg × intensity（k_model = 0.9167，k_sprite = 0.7229，见上面的标定） |
| `castShadow` | `light_set_shadow(light, bool)`（`:310`） | 直接传入。方向光的阴影模式用 `light_directional_set_shadow_mode`（`:327`），缺省 PSSM4。Compatibility 下的阴影限制见 §7 |
| 方向 / 位置 | `instance_set_transform` | 方向光沿自身 −Z 照射，所以直接用实体的世界变换；omni / spot 的 `LIGHT_PARAM_RANGE` 缺省取 Godot 的缺省值 |

### 5.4 Camera

字段映射见 §2.3 的结论 C1：
- `projection` / `fov` / `near` / `far` 对应 `camera_set_perspective`（`rendering_server.h:524`）；
- 正交时 `orthoSize` 对应 `camera_set_orthogonal(2 × orthoSize)`（`:525`）；
- 取第一个启用的 Camera 实体，挂到视口上：`viewport_attach_camera`（`:568`）。
- 编辑器视口用 `EditorCamera`，PIE 时用场景相机，规则和 rurix 一样（`RF/crates/engine-host/src/viewport.rs:1605-1606` 的注释）。

### 5.5 Animator{clip, idleClip, walkClip, time, speed, playing, manualControl, loop} → skeleton_*

- **forge 现状**
  - 字段定义在 `RF/crates/forge-scene/src/lib.rs:211-223`。
  - 采样和蒙皮都在 CPU 上做，按显式的场景时间求值（`RF/crates/engine-host/src/modelrt.rs:1`、`sample` 函数 `:183`）。
  - 状态推进在 `RF/crates/engine-host/src/anim.rs:44-74`（`AnimSystem::advance`），PIE 期间由确定性逻辑驱动。
- **映射方案：姿态继续由 CPU 算，蒙皮交给 GPU**
  - 所有 Animator 字段都只作用在 forge 侧的姿态计算上：`time`、`speed`、`playing`、`loop` 和几个 clip 名决定采样时刻；`manualControl` 为真时由逻辑直接写 `time`。映射层不直接消费这些字段，只接收算好的骨骼矩阵。
  - 这样姿态的求值仍然是确定性的，和 rurix 完全一致；Godot 只负责画。
- **RS 调用**
  - 创建：`skeleton_create()`（`rendering_server.h:292`），`skeleton_allocate_data(sk, bone_count, false)`（`:293`）。
  - 挂到实例：`instance_attach_skeleton(inst, sk)`（`:742`）。
  - 网格表面要带 `ARRAY_BONES` / `ARRAY_WEIGHTS`（`rendering_server_enums.h:136-137`）。超过 4 个权重时加 `ARRAY_FLAG_USE_8_BONE_WEIGHTS`（`:189`）。
  - 每帧：对每根骨骼调 `skeleton_bone_set_transform(sk, i, bone_matrix)`（`:295`）。`bone_matrix` = 当前姿态的全局矩阵 × 逆绑定矩阵，写成 Godot 的 Transform3D。
- **备选方案**：沿用 CPU 蒙皮，每帧用 `mesh_surface_update_vertex_region` 上传蒙皮后的顶点。实现最简单，但开销随顶点数线性增长。先 profile，Stage 4 再定用哪种。
- **Stage 4 选定：GPU 蒙皮（skeleton_*）**。profile（debug 构建，150×150 = 22500 顶点、44402 三角、8 骨骼链，`render_core::extract3d::skinning_profile` 与 g4_anim）：forge 侧只抽姿态 0.089 ms/帧、每帧上传 8 × 48 = 384 B；CPU 蒙皮（= rurix `collect`）390.6 ms/帧、每帧 6.39 MB；Godot 取帧耗时静止 49.97 ms、每帧换姿态 49.99 ms（GPU 蒙皮几乎不加钱），rurix 同一网格 402 / 582 ms。extract 输出实例矩阵（蒙皮时 = 实体矩阵）与骨骼矩阵 jointWorld × inverseBind（`modelrt::node_worlds` 在 Animator 时间处求值），与 `modelrt::vertices` 同式；权重在建网格时按和归一。编辑态三个动画时刻与 rurix（CPU 蒙皮）前景 IoU = 1.0、平均差 0.05；PIE 两个独立进程 step×10 逐帧哈希相同。

### 5.6 ParticleEmitter{} → particles_* + ParticleProcessMaterial

- **forge 现状**：还是实验特性，要设环境变量 `FORGE_GPU_PARTICLES=on|1` 才会打开（`RF/crates/engine-host/src/gpu_particles.rs:1-17`）。
  - 每个发射器 64 个粒子（`:12`）。CPU 只上传一条事件记录，轨迹在 GPU 上算，不做回读（`:3-5`）。
  - 实体 Transform 装的不是普通变换，而是中心点加 `[事件年龄, 寿命, 样式]`（`RF/crates/forge-scene/src/lib.rs:180-181`）。
- **RS 调用**
  - 创建：`particles_create()`（`rendering_server.h:435`），`particles_set_mode(p, PARTICLES_MODE_3D)`（`:436`）。
  - 基本参数：`particles_set_amount(p, 64)`（`:440`）、`particles_set_lifetime(p, 寿命)`（`:442`）、`particles_set_one_shot(p, true)`（`:443`）。
  - 材质与绘制：`particles_set_process_material(p, ppm.get_rid())`（`:451`），`particles_set_draw_passes(p, 1)` + `particles_set_draw_pass_mesh(p, 0, quad_mesh)`（`:475-476`）。QuadMesh 用 billboard 材质。
  - 实例化：`instance_create2(p, scenario)`，实例变换只取中心点（3D 模式下发射变换就是实例变换，`:480` 的注释）。
- **事件年龄的处理**：forge 给的是"事件年龄"，也就是这次爆发已经过去了多久。
  - 年龄回到 0（新事件）时，调 `particles_restart`（`:467`），再调 `particles_set_emitting(p, true)`（`:438`）。
  - 需要从中途接上时，用 `particles_set_pre_process_time`（`:444`）预跑到当前年龄。
  - 帧率固定用 `particles_set_fixed_fps`（`:452`），保证不同机器上的画面一致。
- **样式**：映射成 ParticleProcessMaterial 的预设（方向、扩散、初速度、颜色渐变）。预设表**未定**，要对着 rurix 的着色器（`gpu_particles.rs:74-92` 起）逐个样式对齐，放到 Stage 5。
- 粒子只是视觉效果，不参与模拟，不影响确定性红线。
- **Stage 4 实现（基本映射）**：rurix 只在 sprite_mesh 腿画粒子（viewport/rurix.rs），Godot 同样只在这条腿、只在 `FORGE_GPU_PARTICLES = on | 1` 时映射。事件解码在 `render_core::particles`（`gpu_particles` 整模块受 backend-rurix 门控，这里按同一判据另写一份，单测与 `emitter_bytes` 逐字节锁定）。每个槽位一个 RS particles：3D、64 个、one_shot、explosiveness 1、fixed_fps 60、XY 平面里径向爆开（spread 180、flatness 1、DISABLE_Z、初速 0.9-3.7、无重力），绘制用 XY 平面里的小 quad + 加法混合、不测深度的软圆点；年龄变小（新事件）或第一次见到 → pre_process 到当前年龄再 restart；事件失效 → 释放。四种配置下可见、失效后不画、开关关掉不画（g4_v6）。Godot 粒子按 Godot 自己的时钟模拟，只在 restart 时与 forge 的年龄对齐，所以有粒子的帧不保证两次取帧逐字节相同；rurix 的轨迹是年龄的解析函数（确定性），Stage 5 若用 particles 着色器按年龄复刻，可以同时解决样式与确定性。
- **Stage 5 实现（样式预设 + 确定性）**：不再用 Godot 的粒子模拟，改成一个 canvas 叠加层：4096 个粒子 × 6 个顶点的静态三角形数组，canvas_item 着色器按 gpu_particles COMPUTE 同一个 hash / random、同一组 kind 1-4 公式（喷流 / 抛物线 / 摆动 / 螺旋）算位置、尺寸与颜色，用本帧的 view_proj 投到像素，每帧只上传 64 个发射器的事件。非 HDR 视口的 canvas 在 sRGB 编码值上做加法混合，与 rurix 在 UNORM 目标上的混合同一个空间，而且同样画在所有网格之后、不测深度。实测（g5_particles，320×180，kind 1-4 各 3 个年龄 + 四个发射器同屏）：四种配置两次取帧逐字节相同；与 rurix 最大差 ≤ 3、全帧平均差 ≤ 0.012，亮点数相同（Compatibility ±2）。

### 5.7 SentinelsV6Batch → multimesh_*

- **forge 现状**：V6 批渲染的核心是 `Record = [f32; 12]`，内容为 `[x, y, z, 0, w, h, height, 1, r, g, b, a]`，也就是轴对齐盒的位置、尺寸和颜色（`RF/crates/engine-host/src/sentinels_v6_render.rs:33-57`）。`Batch` 里另外还有 `sprites`、`terrain`、`view`（`:34-40`）。
- **records 的映射**
  - 用一个单位立方体 mesh：`multimesh_create()`（`rendering_server.h:248`），`multimesh_allocate_data(mm, n, RSE::MULTIMESH_TRANSFORM_3D, true, false, false)`（`:255`），`multimesh_set_mesh(mm, cube)`（`:258`）。
  - 每帧用 `multimesh_set_buffer(mm, buf)`（`:275`）整块上传。每个实例 16 个 float：12 个是 Transform3D（按行主序的 3×4），4 个是 Color。布局见 `GD/doc/classes/RenderingServer.xml:2883` 起的方法说明。
  - 原设想:变换 = 平移 (x, y, z) × 缩放 (w, height, h),z 是否是高度方向待确认 → Stage 4 实测否定(z 是竖直方向,且 rurix 的等距投影普通相机拍不出来),改为下一条。
  - **Stage 4 实测结论**（`sentinels_v6_render/rurix.rs:8-27`）：(x, y, z) 是盒子**最小角**；w 沿 +X、h 沿 +Y（地面）、height 沿 **+Z（竖直）**。rurix 只画顶 / +X / +Y 三个面，面阴影 1.0 / 0.65 / 0.82 乘在 sRGB 编码值上，不受光、不剔除；投影是 2D 等距 sx = (x − y)/2、sy = −(x + y)/4 + 1.5z，深度键 k = x + y + 2z。这个映射相对右手系是镜像、两轴缩放不等，普通相机拍不出来 → **把 iso 烘进每个实例的 Transform3D**：G = A·p，A 的三行 = (0.5, −0.5, 0)、(−0.25, −0.25, 1.5)、(0.25, 0.25, 0.5)，basis = A·diag(w, h, height)，原点 = A·盒子中心，相机 = 朝 −Z 的正交相机（中心 = Batch.view 的 iso 中心、半高 24/zoom，与 `publish()` 设给编辑器相机的参数等价），材质 cull_disabled。V6 的数据不在 Scene 里（全局 STAGED），extract 经 `sentinels_v6_render::staged_frame()`（与 rurix render 同一段 CPU 流程）取 V6Frame。实测（demo 项目、无 V6 资产清单、`game.session.open seed 1`、640×360）：F+ / Vulkan 与 rurix 最大差 1、平均差 0.23；三角形 55848、fallbacks 1 与 rurix 相同。
  - 实例数变化时，先用 `multimesh_set_visible_instances`（`:287`）截断显示，容量不够时再重新 allocate。
- **sprites 和 terrain**：`sprites` 走 §8 的 Canvas / 精灵映射。`terrain` 映射成一个 mesh surface（带顶点色），细节放到 Stage 4。
  - **Stage 4**：rurix 的 terrain 就是同格式的薄板 records（每个可见格一块，高 0.02，高地 0.11），拼在 Batch.records 最前面。Godot 侧按 01 原设想做成一个带顶点色的网格（每格 3 面 × 2 三角 = 18 顶点，位置同样烘 iso，COLOR = 格子颜色，UV.x = 面阴影），TerrainBatch 的 Arc 身份不变就不重建；物体盒子走 MultiMesh（每实例 16 个 float：行主序 3×4 + 颜色）。`sprites` 仍跳过（Stage 6），数量记进 `skipped_v6_sprites`；有 V6 资产清单时，有精灵的物体在 rurix 里也不出盒子，所以那种项目在 Godot 下会缺建筑和单位，已写进 capabilities.coverage。

### 5.8 Parent / PrefabInstance

- `Parent{entity}`（`lib.rs:199-202`）不对应任何 RS 调用，只参与世界矩阵的累乘（§5.0）。
- `PrefabInstance{prefabRef, revision, baseline}`（`lib.rs:203-210`）也不对应 RS 调用。预制体在 forge 侧展开成普通实体（`RF/crates/engine-host/src/prefab.rs`），映射层看到的就是展开后的实体。`revision` 变化时，这个子树的 instance 全部重建。

## 6. 环境 / 后处理 / GI / 反射 / 贴花 / 雾（含 Stage 5 schema 提案）

> **Stage 5 实测结论（2026-09-29，g5_* 测试四配置;实现与数字见 02 §9.5「Stage 5 实施记录」）**
> - schema 定稿：Environment / CameraAttributes / RenderSettings（场景级）、ReflectionProbe / Decal / FogVolume（实体级）、LightParams（挂在 Light 上）共 7 个组件，字段全部扁平 camelCase、全部可选、缺省 = Godot 4.7.2 缺省；与下面 §6.3 原提案的差异：Light 不加字段（改成 LightParams）、嵌套对象拆成前缀字段、VoxelGI 与 Compositor 不进 schema。逐字段表见 02 §9.5 Stage 5 step 2。
> - 映射方式：Environment 用 `Environment` 资源（不挂节点、只用 `get_rid`）按属性名设值，而不是逐个调 RS——`environment_set_ssil` 没有绑定给 GDExtension；CameraAttributes 用 `CameraAttributesPractical` 资源；ReflectionProbe / Decal / FogVolume 直接调 RS（FogVolume 的材质用 `FogMaterial` 资源）。
> - 没有 Stage 5 组件的场景：仍是 §5.3 的缺省灯 + 缺省环境，与 Stage 4 逐字节相同（两项有意修正除外：缺省环境下金属补环境项、Mobile 改 HDR 2D 缓冲，见 02 §9.5）。
> - 实测能力与 §7 K1 表的出入：Compatibility 的反射探针实测不生效（开关前后逐字节相同，原因未查清）；Compatibility 的 SSAO 是另一套实现（s4ao，后处理里按深度估算），效果与 F+ 不同，capabilities 标 `limited`；Mobile 改用 HDR 2D 之后没有 debanding（Godot 只在非 HDR 目标上做），Glow 也不再受 RGB10A2 的 0-2 范围限制。其余各项与 K1 一致：F+ 全部生效；Mobile 的 SSAO / SSIL / SSR / SDFGI / 体积雾 / FogVolume / 自动曝光 / TAA 被忽略且画面逐字节不变；Compatibility 的 SSIL / SSR / SDFGI / 体积雾 / FogVolume / DOF / 自动曝光 / 贴花 / TAA / FXAA / debanding 同样。
> - 时间性效果（SDFGI 收敛、自动曝光、体积雾时间重投影、TAA、updateMode=always 的反射探针）要连续多帧；单次取帧看到的是当前收敛程度。实测 SDFGI（F+ D3D12 / Vulkan）：第 62、122 帧相邻两帧仍差 1 LSB，第 242 帧起逐字节相同。
> - Godot 4.7.2 缺陷：Environment 背景为 color / clearColor 且 F+ 开了雾或体积雾、Mobile 开了雾时，背景改走"只画雾的天空"着色器，颜色被转两次线性而偏暗（(9,11,15) → (1,1,1)），backgroundEnergy 也被多乘一次；Compatibility 不受影响。源码位置与可选补偿见 02 §9.5 Stage 5 step 7，本阶段未修。

### 6.1 API（`GD/servers/rendering/rendering_server.h`）

- **Environment**：`environment_create()`（`:645`）；挂到场景用 `scenario_set_environment`（`:717`），挂到相机用 `camera_set_environment`（`:529`），相机上的优先。
  - 背景：`environment_set_background(env, RSE::EnvironmentBG)`（`:647`）、`environment_set_bg_color`（`:651`）、`environment_set_bg_energy(env, multiplier, exposure_value)`（`:652`）。
  - 天空：`environment_set_sky`（`:648`）、`environment_set_sky_orientation`（`:650`）。
  - 环境光与反射来源：`environment_set_ambient_light(env, color, ambient_source=BG, energy=1.0, sky_contribution=0.0, reflection_source=BG)`（`:654`）。
  - 调色：`environment_set_tonemap(env, tonemapper, exposure, white)`（`:661`）、`environment_set_adjustment(env, enable, brightness, contrast, saturation, use_1d, color_correction)`（`:663`）。3D LUT 就是这里的 `color_correction` 纹理。
  - 辉光：`environment_set_glow(env, enable, levels, intensity, strength, mix, bloom_threshold, blend_mode, hdr_bleed_threshold, hdr_bleed_scale, hdr_luminance_cap, glow_map_strength, glow_map)`（`:657`）。
  - 屏幕空间效果：`environment_set_ssr`（`:665`）、`environment_set_ssao`（`:671`）、`environment_set_ssil`（`:675`）。
  - 全局光照：`environment_set_sdfgi(env, enable, cascades, min_cell_size, y_scale, use_occlusion, bounce_feedback, read_sky, energy, normal_bias, probe_bias)`（`:679`）。
  - 雾：`environment_set_fog(env, enable, light_color, light_energy, sun_scatter, density, height, height_density, aerial_perspective, sky_affect, mode=EXPONENTIAL)`（`:687`）、`environment_set_volumetric_fog(...)`（`:690`）。
  - 下面这些是**全局**设置，不属于单个 environment：`environment_set_ssao_quality`（`:673`）、`environment_set_ssil_quality`（`:677`）、`environment_set_ssr_roughness_quality`（`:669`）、`environment_set_sdfgi_ray_count`（`:681`）、`environment_set_volumetric_fog_volume_size`（`:691`）、`environment_glow_set_use_bicubic_upscale`（`:659`）。
- **Sky**：`sky_create()`（`:624`）、`sky_set_radiance_size`（`:625`）、`sky_set_mode`（`:626`）、`sky_set_material`（`:627`）。材质用 `ProceduralSkyMaterial`、`PhysicalSkyMaterial` 或 `PanoramaSkyMaterial` 资源的 RID。
- **CameraAttributes**：`camera_attributes_create()`（`:703`）、`camera_attributes_set_dof_blur`（`:709`）、`camera_attributes_set_exposure(ca, multiplier, exposure_normalization)`（`:710`）、`camera_attributes_set_auto_exposure(ca, enable, min_sensitivity, max_sensitivity, speed, scale)`（`:711`）。挂到场景用 `scenario_set_camera_attributes`（`:719`），挂到相机用 `camera_set_camera_attributes`（`:530`）。
- **ReflectionProbe**：`reflection_probe_create()`（`:350`）、`_set_update_mode`（`:352`）、`_set_intensity`（`:353`）、`_set_ambient_mode` / `_set_ambient_color`（`:356-357`）、`_set_size`（`:360`）、`_set_origin_offset`（`:361`）、`_set_enable_shadows`（`:364`）。创建后用 `instance_create2` 放进 scenario。
- **Decal**：`decal_create()`（`:372`）、`decal_set_size`（`:373`）、`decal_set_texture(decal, RSE::DecalTexture, tex)`（`:374`）、`decal_set_emission_energy`（`:375`）、`decal_set_modulate`（`:377`）、`decal_set_fade(above, below)`（`:380`）、`decal_set_normal_fade`（`:381`）。
- **FogVolume**：`fog_volume_create()`（`:504`）、`fog_volume_set_shape`（`:506`）、`fog_volume_set_size`（`:507`）、`fog_volume_set_material`（`:508`，材质用 `FogMaterial` 资源）。
- **VoxelGI**：`voxel_gi_create()`（`:387`）、`voxel_gi_allocate_data(vgi, to_cell_xform, aabb, octree_size, octree_cells, data_cells, distance_field, level_counts)`（`:389`）、`voxel_gi_set_energy`（`:401`）；`voxel_gi_set_quality`（`:408`）是全局设置。
  - **不经过节点，没法在运行时烘焙。** `allocate_data` 只接收已经烘焙好的 octree 数据。烘焙本身是 `VoxelGI::bake(Node *p_from_node, …)`（`GD/scene/3d/voxel_gi.cpp:434`），它用 `Voxelizer`（`:442`），并且要从节点树里用 `_find_meshes` 收集 `MeshInstance3D`（`:333`、`:389`）。
  - 可选做法：(1) 离线或按需烘焙：临时建一棵 VoxelGI 加 MeshInstance3D 的代理节点树，烘焙完只保留 `VoxelGIData`，之后仍然由 RS 直接调用。(2) 改用 SDFGI：实时计算、不用烘焙，但只有 Forward+ 支持（§7）。建议先做 (2)，(1) 留到 Stage 5 再评估。
- **Compositor**：`compositor_create()`（`:639`）、`compositor_set_compositor_effects`（`:641`）、`compositor_effect_create()`（`:632`）、`compositor_effect_set_callback(effect, callback_type, Callable)`（`:634`）。挂到场景用 `scenario_set_compositor`（`:720`）。它只在 RD 渲染方式下可用，是 forge 自定义后处理的扩展口。

### 6.2 相关枚举（`GD/servers/rendering/rendering_server_enums.h`，前缀已去掉）

- `EnvironmentBG`（`:640-648`）：`CLEAR_COLOR`、`COLOR`、`SKY`、`CANVAS`、`KEEP`、`CAMERA_FEED`、`MAX`。
- `EnvironmentAmbientSource`（`:650-655`）：`BG`、`DISABLED`、`COLOR`、`SKY`。
- `EnvironmentReflectionSource`（`:657-661`）：`BG`、`DISABLED`、`SKY`。
- `EnvironmentToneMapper`（`:671-677`）：`LINEAR`、`REINHARD`、`FILMIC`、`ACES`、**`AGX`**。AgX 在 4.7 里确实存在，tonemap 着色器里有对应实现（`GD/servers/rendering/renderer_rd/shaders/effects/tonemap.glsl:176` 起）。
- `EnvironmentGlowBlendMode`（`:663-669`）：`ADDITIVE`、`SCREEN`、`SOFTLIGHT`、`REPLACE`、`MIX`。
- `EnvironmentSSAOQuality`（`:686-692`）和 `EnvironmentSSILQuality`（`:694-700`）：都是 `VERY_LOW`、`LOW`、`MEDIUM`、`HIGH`、`ULTRA`。
- `EnvironmentSSRRoughnessQuality`（`:679-684`）：`DISABLED`、`LOW`、`MEDIUM`、`HIGH`。
- SDFGI：
  - 级联数不是枚举，是 `environment_set_sdfgi` 的 `int p_cascades` 参数，Environment 上的缺省值是 4（`GD/doc/classes/Environment.xml`）。
  - `EnvironmentSDFGIYScale`（`:702-706`）：`50_PERCENT`、`75_PERCENT`、`100_PERCENT`。
  - `EnvironmentSDFGIRayCount`（`:708-717`）：`4`、`8`、`16`、`32`、`64`、`96`、`128`、`MAX`。
- `EnvironmentFogMode`（`:738-741`）：`EXPONENTIAL`、`DEPTH`。
- `SkyMode`（`:611-616`）：`AUTOMATIC`、`QUALITY`、`INCREMENTAL`、`REALTIME`。
- `ReflectionProbeUpdateMode`（`:311-314`）：`ONCE`、`ALWAYS`。
- `ReflectionProbeAmbientMode`（`:316-320`）：`DISABLED`、`ENVIRONMENT`、`COLOR`。
- `DecalTexture`（`:324-330`）：`ALBEDO`、`NORMAL`、`ORM`、`EMISSION`、`MAX`。
- `FogVolumeShape`（`:426-433`）：`ELLIPSOID`、`CONE`、`CYLINDER`、`BOX`、`WORLD`、`MAX`。
- `VoxelGIQuality`（`:343-346`）：`LOW`、`HIGH`。

### 6.3 Stage 5 的 forge schema 提案

> 下面是 Stage 4 之前写的**原提案**，Stage 5 的定稿见 02 §9.5 Stage 5 step 2 的表（差异：Light 扩展字段 → LightParams 组件；`glow{…}` / `ssao{…}` 这类嵌套对象 → `glowEnabled`、`ssaoRadius` 这样的前缀字段；VoxelGI 不进 schema；"待抄录"的缺省值已全部按 `GD/doc/classes/*.xml` 抄齐）。

原则：
- 只有 `[render].backend="godot"` 时才生效；rurix 碰到这些字段一律忽略（不报错，编辑器里标"当前后端不支持"）。
- 缺省值一律取 Godot 的缺省值。下面标了出处的是已经从 `GD/doc/classes/*.xml` 抄出来的；标"待抄录"的，Stage 5 实现时再从对应 XML 逐项抄。
- 字段名用 camelCase，与现有组件一致。

```text
Environment（组件；取第一个启用的，规则同 Camera）             // Environment.xml
  background        enum clearColor|color|sky|canvas            = clearColor   (background_mode=0)
  backgroundColor   [f32;4]                                     = [0,0,0,1]
  backgroundEnergy  number                                      = 1.0
  sky               { type: procedural|physical|panorama, texture: guid } = { procedural, "" }
  ambientSource     enum bg|disabled|color|sky                  = bg           (ambient_light_source=0)
  ambientColor      [f32;4]                                     = [0,0,0,1]
  ambientEnergy     number                                      = 1.0
  ambientSkyContribution number                                 = 1.0
  reflectionSource  enum bg|disabled|sky                        = bg
  tonemap           enum linear|reinhard|filmic|aces|agx        = linear       (tonemap_mode=0)
  exposure / white  number                                      = 1.0 / 1.0
  glow   { enabled=false, intensity=0.3, strength=1.0, bloom=0.0, blendMode=screen, hdrThreshold=1.0 }
  ssao   { enabled=false, radius=1.0, intensity=2.0 }           // 其余参数待抄录
  ssil   { enabled=false, radius=5.0 }
  ssr    { enabled=false, maxSteps=64 }
  sdfgi  { enabled=false, cascades=4, minCellSize=0.2, yScale="75%" }    (sdfgi_y_scale=1)
  fog    { enabled=false, mode=exponential, density=0.01, lightColor=[0.518,0.553,0.608] }
  volumetricFog { enabled=false, density=0.05 }
  adjustment { enabled=false, brightness, contrast, saturation, colorCorrection: guid }  // 数值待抄录
ReflectionProbe（组件；实体变换 = 探针中心）                   // ReflectionProbe.xml
  size=[20,20,20]  intensity=1.0  updateMode=once  boxProjection=false  interior=false
  maxDistance=0.0  cullMask=1048575
Decal                                                           // Decal.xml
  size=[2,2,2]  albedo/normal/orm/emission: guid=""  emissionEnergy=1.0  modulate=[1,1,1,1]
  albedoMix=1.0  upperFade=0.3  lowerFade=0.3  cullMask=1048575
FogVolume                                                       // FogVolume.xml
  shape=box (shape=3)  size=[2,2,2]  material{density, albedo, emission, heightFalloff, edgeFade}（待抄录，FogMaterial.xml）
VoxelGI                                                         // VoxelGI.xml
  size=[20,20,20]  subdiv=128 (subdiv=1)  energy（待抄录）  bakeMode=offline（§6.1 方案 (1)）
Light 扩展字段（只在 godot 后端生效）
  range, attenuation, spotAngle, spotAttenuation, indirectEnergy, volumetricFogEnergy, specular,
  size, shadowBias, shadowNormalBias, shadowBlur, shadowOpacity, directionalShadowMode, omniShadowMode
  缺省值待从 Light3D / OmniLight3D / SpotLight3D / DirectionalLight3D.xml 抄录
renderSettings（场景级，写在场景根上）
  msaa3d / msaa2d, screenSpaceAA (disabled|fxaa|smaa), taa, scaling3dMode, scaling3dScale,
  fsrSharpness, debanding, occlusionCulling, anisotropy, sdfgiRayCount, ssaoQuality, ssilQuality
  缺省值待从 Viewport.xml / ProjectSettings.xml 抄录；这些设置按渲染方式降级（§7）
```

- **映射**：Environment 映射到 §6.1 的 environment_* / sky_* 调用；场景没有 Environment 实体时，用 §5.3 的"缺省灯光 + 环境光"组合。
- **校验**：字段不被当前渲染方式支持时（比如 Compatibility 下的 SDFGI），忽略该字段并通过 `render.capabilities` 上报（02 §4）。

## 7. 渲染方式能力差异表（Forward+ / Mobile / Compatibility）

出处缩写：`E` = `GD/doc/classes/Environment.xml`，`PS` = `GD/doc/classes/ProjectSettings.xml`，`VP` = `GD/doc/classes/Viewport.xml`。"未见限制说明"表示对应文档条目里没有关于渲染方式的限制说明，按支持计，Stage 4 实测复核。

| 特性 | Forward+ | Mobile | Compatibility | 出处 |
|---|---|---|---|---|
| **RenderingDevice（决定能不能走 L1）** | ✅ | ✅ | ❌ | 00 §1：`get_rendering_device()` 返回 null |
| 每物体灯数 | ✅ 集群式，无每物体上限 | ⚠️ 每个 mesh 资源最多 8 盏 omni + 8 盏 spot | ⚠️ `max_lights_per_object` 缺省 8，另有 `max_renderable_lights` | `OmniLight3D.xml:8`；`PS:3205-3206`、`PS:3213` |
| 阴影 | ✅ | ✅ | ✅（PCSS 不支持） | PCSS：方向光只有 F+ 支持（`Light3D.xml:60`），点光 / 聚光 F+ 和 Mobile 支持（`Light3D.xml:100`） |
| 灯光投影纹理 | ✅ | ✅ | ❌ | `Light3D.xml:95` |
| MSAA 3D | ✅ | ✅ | ✅ | 未见限制说明 |
| MSAA 2D | ✅ | ✅ | ❌ | `PS:2861` |
| FXAA / SMAA | ✅ | ✅ | ❌ | `PS:2871` |
| TAA | ✅ | ❌ | ❌ | `PS:2886` |
| FSR1 / FSR2 | ✅ | ⚠️ 不生效，回退 | ❌ | `PS:3410` |
| SSAO | ✅ | ❌ | ✅ | `E:266` |
| SSIL | ✅ | ❌ | ❌ | `E:288` |
| SSR | ✅ | ❌ | ❌ | `E:307` |
| SDFGI | ✅ | ❌ | ❌ | `E:219` |
| VoxelGI | ✅ | ❌ | ❌ | `VoxelGI.xml:8` |
| 反射探针 | ✅ | ⚠️ 每个 mesh 最多 8 个 | ⚠️ 每个 mesh 最多 2 个 | `ReflectionProbe.xml:10` |
| 体积雾 | ✅ | ❌ | ❌ | `E:364` |
| 普通（深度 / 高度）雾 | ✅ | ✅ | ✅ | 未见限制说明 |
| Glow | ✅ | ⚠️ 动态范围只到 2.0，效果不同 | ✅ | `E:136`、`E:148`、`E:151` |
| DOF | ✅ | ✅ | ❌ | `CameraAttributesPractical.xml:9` |
| 自动曝光 | ✅ | ❌ | ❌ | `CameraAttributes.xml:17` |
| 3D LUT（color correction） | ✅ | ✅ | ✅ | 未见限制说明 |
| Debanding | ✅ | ✅ | ❌ | `VP:465` |
| 贴花 | ✅ | ⚠️ 每个 mesh 最多 8 个 | ❌ | `Decal.xml:11-12` |
| GPU 粒子 | ✅ | ✅ | ✅（手动 emit 不支持） | `particles_emit` 只有 F+ / Mobile 支持（`GPUParticles3D.xml:38`） |
| 粒子碰撞 / 湍流 | ✅ | ✅ | ⚠️ **未核实** | 湍流开销很大（`ParticleProcessMaterial.xml:399`） |
| MultiMesh | ✅ | ✅ | ✅ | 未见限制说明 |
| 骨骼 / BlendShape | ✅ | ✅ | ✅ | 未见限制说明 |
| 遮挡剔除 | ✅ | ✅ | ✅ | 未见限制说明（CPU 光栅） |
| 2D 灯光与阴影 | ✅ | ✅ | ✅ | 未见限制说明 |
| HDR 2D | ✅ | ✅ | ⚠️ **未核实** | `VP` 的 `use_hdr_2d` 条目没有渲染方式说明 |
| Canvas SDF | ✅ | ✅ | ⚠️ **未核实** | 没找到限制说明 |
| mipmap bias | ✅ | ✅ | ⚠️ 固定为 0 | `VP:459` |

> **结论 K1（能力差异）**：Forward+ 是全功能档。Mobile 没有 SSAO / SSIL / SSR / SDFGI / VoxelGI / 体积雾 / TAA / 自动曝光，而且有每 mesh 的数量上限。Compatibility 没有 RD，所以不能走 L1；它也没有 MSAA 2D / FXAA / SMAA / TAA / 贴花 / DOF / debanding。`render.capabilities` 按这张表加上运行时探测的结果上报（02 §4），编辑器据此把不可用的字段置灰。
>
> **Stage 5 实测复核（g5_env / g5_camera_gi / g5_volumes / g5_settings）**：上表 Stage 5 相关各行与实测一致，只有三处出入——Compatibility 的反射探针实测不生效（表里写"⚠️ 每 mesh 最多 2 个"）；Compatibility 的 SSAO 是另一套实现（s4ao），`render.capabilities.coverage.limited` 标出；Mobile 改用 HDR 2D 缓冲之后没有 debanding、Glow 不再受 0-2 范围限制。不支持的特性在 `coverage.unsupported` 里逐项给出特性键与原因，对应配置下开关前后画面逐字节不变（测试断言）。

## 8. 2D Canvas 映射

### 8.1 forge 精灵的现有语义（映射必须对齐的基准）

- forge 的 2D **不是**独立的画布：精灵是 3D 世界里的单位 quad，由正交或透视相机来看（F-GAME-3）。
- 渲染变换：`sprite_render_transform`（`RF/crates/engine-host/src/viewport.rs:540-565`）。
  - 世界尺寸 = `transform.scale × 帧像素 / pixelsPerUnit`（`:547`、`:555`）。
  - 锚点：图像空间的 pivot（y 向下）折算成 quad 局部偏移 `(0.5 − px, py − 0.5)`（`:556`），再经旋转和缩放并入平移，保证锚点正好落在实体的 translation 上。
  - texture 直贴模式的 pivot 固定为 (0.5, 0.5)，即居中（`:552`、`:537`）。`.rxsprite` 模式的 pivot 和帧矩形取自图集文档（`SpriteRenderInfo`，`:461-537`）。
  - 点选（OBB）用的也是这个变换，所以画面和点选一致。
- 着色器规则（`viewport.rs:752-781`）：
  - 采样：按整数 texel 取值，也就是最近邻；坐标钳在帧的 `uv_rect` 以内，防止越界采到相邻帧（`:767-772`）。
  - 丢弃：`a ≤ 0` 丢弃；opaque 模式下 `a < 0.02` 也丢弃（`:773`）。
  - 洋红色键：`g < 0.5 × min(r, b)` 时丢弃（`:776-777`）。
  - 输出：`rgb × tint.rgb`；alpha 在 opaque 模式下取 `tint.a`，其余模式取 `texel.a × tint.a`（`:779-780`）。
  - 混合方式来自 `blendMode`（`sprite_blend`，`:375-381`）；排序键是 `sortingOrder`（`:353-357`）。
- 相机：正交相机的 orthoSize 是半高（§2.3）。

> **结论 S1（2D 走哪条路）**：按已定决策，2D 走 Canvas。但 Canvas 和 3D 场景之间没有深度交错，而 forge 允许精灵和 3D 网格混在同一个场景里，还允许用透视相机看精灵。所以按场景分流：
> - **纯 2D 场景**（相机是正交的，并且场景里没有 MeshRenderer / ModelRenderer / ParticleEmitter）：走 Canvas，映射见 §8.2-§8.4。
> - **其他场景**：精灵作为 3D quad 放进 scenario。材质用 unshaded、最近邻过滤，透明方式按 blendMode 选，`render_priority` 取 sortingOrder 排名。色键逻辑复用 §8.3 的着色器片段。
> - 分流判断在 forge 侧的中立层完成（02 §3），两条路共用同一套尺寸和锚点计算。

### 8.2 Sprite 字段 → canvas_item_*（`GD/servers/rendering/rendering_server.h`）

- **创建**：每个精灵实体建一个 `canvas_item_create()`（`:799`），用 `canvas_item_set_parent(item, layer_canvas)` 挂到层（`:800`）。
- **绘制**：用 `canvas_item_clear`（`:849`）清掉旧内容，再用 `canvas_item_add_texture_rect_region(item, rect, tex, src_rect, modulate, transpose=false, clip_uv=true)`（`:828`）重录一条绘制命令。字段变化时才重录；只改变换时调 `canvas_item_set_transform`（`:810`）就够了。

| 字段 | 映射 |
|---|---|
| `texture` / `sprite` + `clip` + `frame` | `tex` = 贴图的 `ImageTexture.get_rid()`。`src_rect` = 帧的 `uv_rect × 贴图像素尺寸`。`.rxsprite` 的帧解析复用 `resolve_sprite_render`，属于中立逻辑（02 §3） |
| `pixelsPerUnit` + `transform.scale` | 画布本地单位统一用"世界单位"：矩形尺寸 w = scale.x × fw / ppu，h = scale.y × fh / ppu，与 `viewport.rs:555` 一致；像素缩放交给 2D 相机（§8.4） |
| pivot（`.rxsprite`） / 居中（texture） | `rect = Rect2(−px·w, −py·h, w, h)`。画布本地 Y 向下，和图像空间 pivot 的方向相同，锚点正好落在 item 原点 |
| `transform.translation` / 绕 Z 的旋转 | `canvas_item_set_transform(item, Transform2D(−θ, (x, −y)))`。Y 翻转后旋转方向也跟着反号 |
| `flipX` / `flipY` | 把 `rect.size.x` / `rect.size.y` 取负来镜像。Sprite2D 就是这么做的，**未在源码核实**；也可以改成交换 src_rect 的左右 / 上下 |
| `tint[4]` | 传给 `add_texture_rect_region` 的 `p_modulate`。opaque 模式的 alpha 规则由 §8.3 的着色器实现 |
| `sortingOrder` | 同一层内按 (sortingOrder, 实体 id) 排序，用 `canvas_item_set_draw_index(item, 名次)`（`:850`）；`z_index`（`:843`）只接受整数，所以不直接用它 |
| `chromaKey` / `blendMode` | 见 §8.3，用 `canvas_item_set_material(item, mat)`（`:852`） |
| 过滤方式 | `canvas_item_set_default_texture_filter(item, CANVAS_ITEM_TEXTURE_FILTER_NEAREST)`（`:802`），与 forge 的最近邻采样一致 |
| `enabled` | `canvas_item_set_visible`（`:805`） |
| `spriteVariants` / `variantStride` | 在中立层先选好具体的贴图或帧，再按上面的规则绘制。V6 专用，Stage 6 细化 |

### 8.3 色键与三种混合：用 canvas shader，不用 CanvasItemMaterial

`CanvasItemMaterial` 只能设混合模式，做不到色键丢弃，也关不掉混合，所以改用 `ShaderMaterial`。按"混合方式 × 色键开 / 关"组合出 6 个变体，只缓存这 6 份，由所有精灵共享：

```glsl
shader_type canvas_item;
render_mode blend_mix;          // opaque: blend_disabled ; alpha: blend_mix ; additive: blend_add
uniform bool chroma_key = true;
uniform bool opaque_mode = false;
varying vec4 forge_vertex_tint;
void vertex() { forge_vertex_tint = COLOR; }
void fragment() {
    vec4 t = texture(TEXTURE, UV);
    if (t.a <= 0.0 || (opaque_mode && t.a < 0.02)) discard;
    if (chroma_key && t.g < 0.5 * min(t.r, t.b)) discard;
    COLOR = vec4(t.rgb * forge_vertex_tint.rgb, opaque_mode ? forge_vertex_tint.a : t.a * forge_vertex_tint.a);
}
```

- Stage 6 已用固定 Godot 4.7.2 在四配置实际编译并验证 `blend_disabled`、`blend_mix`、`blend_add`，三种模式均产生预期不同的像素。
- 实现不依赖 `MODULATE`：vertex 阶段保存 `COLOR` 到 varying，fragment 阶段与采样颜色相乘，避免片元默认 `COLOR` 已含纹理造成重复乘法。真实实现见 `crates/godot-host/src/sprite.rs`。

### 8.4 2D 相机、CanvasLayer 与视差

- **2D 相机**：一个画布对应一个变换，用 `viewport_set_canvas_transform(vp, canvas, T)`（`:572`）设置。
  - T = 平移 (W/2, H/2) · 缩放 (S, S) · 平移 (−cam_x, +cam_y)。其中 S = H / (2 · orthoSize) 表示每个世界单位多少像素；cam 是正交相机位置在 XY 平面上的投影。
  - 这样画布本地的"世界单位、Y 翻转"坐标就映射成了屏幕像素，而且和 forge 正交相机的视野严格一致：半高 = orthoSize（§2.3）。
- **CanvasLayer**：每层一个 `canvas_create()`（`:779`），用 `viewport_attach_canvas`（`:570`）挂到视口，用 `viewport_set_canvas_stacking(vp, canvas, layer, sublayer)`（`:583`）决定层之间的叠放顺序。
  - forge 目前没有层这个字段，所有精灵都在同一层，靠 sortingOrder 排序。
  - 将来如果加 `layer` 字段，就一层一个 canvas。
- **视差**：每层单独设画布变换，相机平移量乘以该层的 scroll 系数。需要无缝循环的背景用 `canvas_set_item_mirroring`（`:780`）或 `canvas_set_item_repeat`（`:781`）。
- 全局画布变换 `viewport_set_global_canvas_transform`（`:582`）保持单位变换，不使用。

### 8.5 其他 2D 特性

- **Light2D**：`canvas_light_create()`（`:874`）、`canvas_light_attach_to_canvas`（`:878`）、`_set_transform`（`:880`）、`_set_color`（`:881`）、`_set_energy`（`:883`）、`_set_texture`（`:892`）、`_set_texture_scale`（`:891`）、`_set_mode`（`:876`）、`_set_blend_mode`（`:895`）、`_set_shadow_enabled`（`:897`）、`_set_z_range` / `_set_layer_range`（`:884-885`）。灯光纹理用同一套 Y 翻转坐标。
- **LightOccluder2D**：`canvas_occluder_polygon_create()` 加 `canvas_occluder_polygon_set_shape(poly, points, closed)`（`:922-923`），`canvas_light_occluder_create()`（`:908`），再用 `_attach_to_canvas`（`:909`）、`_set_polygon`（`:911`）、`_set_transform`（`:913`）。
- **2D 粒子**：`particles_create()` 后调 `particles_set_mode(p, PARTICLES_MODE_2D)`（`:436`），用 `canvas_item_add_particles(item, p, texture)`（`:837`）挂到一个 canvas item 上。2D 模式下发射变换要手动设：`particles_set_emission_transform`（`:480`，注释写明"只用于 2D"）。
- **HDR 2D**：现实现仅 Mobile 开启 `viewport_set_use_hdr_2d(vp, true)`，用于保留 3D 中间输出精度后显式转换为 sRGB；这不是跨配置可调的 Forge 组件能力，也不等于 2D Glow 已接入。
- **2D Glow**：`g6_sprite_render::canvas_glow_and_spatial_glow_follow_reported_limits` 用黑色背景排除背景自身发光，四配置 Canvas 精灵开启 Glow 后均为 0 个变化像素；同夹具走 3D quad 后均有明确的 Glow 差分且关闭后精确恢复。当前 Canvas 在 Environment 后合成，`Sprite.canvasEnvironment` 如实上报受限。`ENV_BG_CANVAS` 及 HDR 2D 组合属于后续实现方向，本轮不引入新组件。

> **结论 S2（Stage 6 实测修订）**：精灵尺寸、锚点、翻转、色键及 Canvas/透明 quad 排序已用合成图集验证；不透明颜色容差为每通道 3。Forward+ 的 3D quad 和 Mobile 的 Canvas/3D quad 在线性 HDR 空间混合，其他已测路径在编码空间混合，不能承诺各路径或 Rurix 逐像素相同。混合模型场景只在无显式 Environment 时逆补偿缺省 Reinhard；显式 Environment 保留用户 tone map。证据和其余边界见 [03 收尾记录](03-completion.md)。

## 9. 风险与未决项

| # | 风险 / 未决项 | 影响 | 处置 |
|---|---|---|---|
| 1 | L1-a2 里导入纹理在 RD 拷贝之后的布局，以及外部屏障和 RD 状态跟踪能不能共存（§3.5 第 4 步），都**未核实** | 错了会出现 D3D12 校验错误或画面撕裂 | Stage 3 打开 D3D12 debug layer 和 GPU-based validation 验证；不通过就只用 L2 |
| 2 | 双显卡笔记本上，缺省 adapter（presenter / share.rs 用的）可能不是 Godot 选中的那块 | L1 不可用 | 运行时比较 LUID，不同就降级到 L2；或者启动时用 `--gpu-index` 把 Godot 固定到缺省 adapter，这需要监督器先枚举一遍 adapter（02 §6） |
| 3 | Vulkan 下无法导入 D3D12 共享 buffer（`GD/drivers/vulkan/SCsub:36`） | Vulkan 只能走 L2 | 结论 X2；Windows 下 `[render].driver` 缺省用 d3d12（02 §7） |
| 4 | GLES3 只能用同步的 `texture_2d_get` | 帧率下降 | 推流上限设 30 fps；能力表里标出来（§7） |
| 5 | rurix 没有读 Light 组件，灯光是写死的（`modelrender.rs:163`、`:166`） | 两个后端的画面只能近似一致 | 缺省灯光按 §5.3 逼近；k 系数 Stage 4 用灰卡标定；不追求逐像素一致，基线只对 rurix 自己做（02 §9） |
| 6 | 细节**未核实**：法线贴图的 Y 方向、粒子样式预设、V6 record 的轴向、`ao_light_affect` 对应关系 | 3D 画面有偏差 | Stage 4 / 5 逐项做对照测试。**Stage 4 已结**：法线 Y 与 Godot 一致；AO → ao_light_affect 0 + strength 烘进贴图；V6 z 竖直、最小角、iso 烘进实例变换；光强 k 见 §5.3。**剩粒子样式预设**（Stage 5）。**Stage 5 已结**：kind 1-4 按 gpu_particles 解析式复刻（canvas 叠加层），与 rurix 最大差 ≤ 3、逐帧确定 |
| 7 | 精灵"Canvas / 3D quad"分流（结论 S1） | 场景类型变化时要重建整个 2D 映射 | 分流判断放在中立层；判断条件由 02 §3 定义 |
| 8 | gdext 的 `ISceneTree::process` 签名和 bitfield 类型名只是推断，生成代码要到构建时才有 | Stage 3 可能出编译错误 | Stage 3 第一次编译后按生成代码修正，不影响设计 |
| 9 | console 模板和主 exe 的命名对应规则**未核实**（结论 D1） | 打包布局可能要调整 | Stage 3 实测 |
| 10 | 启动出错时引擎会弹 `OS::alert` 模态框，进程卡住（00 §2.3） | 监督器会一直等下去 | 启动超时后强杀，扩展在自检失败时自己打印错误并退出（02 §6） |
| 11 | `--headless` 不可用于出帧（§4.2，按推断） | 不能省掉窗口 | 用屏外窗口方案（00 §2.1） |
| 12 | Compatibility 下 HDR 2D / Canvas SDF / 粒子碰撞的支持情况**未核实**（§7） | 能力上报可能不准 | Stage 4 / 6 用运行时探测补齐 |
| 13 | 能否把 3D 物理引擎设成空实现**未核实**（§4.1） | 空转有少量开销 | Stage 3 用 `--verbose` 确认 |

**与 00 报告的关系**（00 不改，这里只记录）：
- 00 §3 末尾的启动示例带 `--path`，那只适用于编辑器二进制。release 模板要用结论 D1 的目录布局，而且不能传 `--path`（§4.2）。这和 00 §2.8 一致，只是 §3 的示例容易误导。
- 00 §4 的冒烟代码用的是 `base = MainLoop`，正式实现按 00 §2.9 和本文 §4.3 改为 `base = SceneTree`。这两处本来就一致，不算冲突。
- 00 §1 说"RD 路径回读是 RGB8"，本文 §3.1 找到了原因：`RS::texture_2d_get` 会按 render target 的 `image_format` 转换。直接用 `RD::texture_get_data(_async)` 读得到的是 RGBA8。这是对 00 的补充，不是冲突。
