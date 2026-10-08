# 00 · Godot 4.7.2 × gdext 冒烟实测(Stage 1)

> 日期 2026-09-27;机器 RTX 5060 Laptop(NVIDIA 610.88)+ Intel iGPU;运行时 = 官方 `Godot_v4.7.2-stable_win64_console.exe`
> (`scripts/godot-fetch.ps1` 下载,SHA-512 与 release `SHA512-SUMS.txt` 一致);扩展 = godot-rust gdext `=0.5.5`(`api-4-7`)cdylib。

## 1. 结论

| 项 | 结果 | 证据 |
|---|---|---|
| 官方运行时装载 Rust 扩展 | ✅ | stdout `Initialize godot-rust (API v4.7.stable.official, runtime v4.7.2.stable.official, safeguards strict)` |
| 自定义 MainLoop(GDExtension 类)经 `application/run/main_loop_type` 接管主循环 | ✅ | `main/main.cpp:4417-4432` `ClassDB::instantiate(main_loop_type)`;`initialize/process/finalize` 回调均到达 |
| 纯 RenderingServer 离屏视口出帧(无 SceneTree、无节点) | ✅ | 三种配置均回读出正确画面(橙色立方体 + 平行光 + 纯色背景) |
| Forward+ / Vulkan 1.4.341 | ✅ | 离屏 40 tick 出帧 39,回读 320×180 `RGB8` |
| Forward+ / D3D12 12_0 | ✅ | 同上;`get_driver_resource(LOGICAL_DEVICE/COMMAND_QUEUE)` 非空 |
| Compatibility / OpenGL 3.3 | ✅ | 回读 `RGBA8`;`get_rendering_device()` = null(无 RD,L1 零拷贝不适用) |
| 主窗口移到屏外(-32000,-32000)、无边框、不抢焦点 | ✅ 持续出帧 | `frames_drawn_delta = 39/40` |
| 主窗口最小化 | ❌ **停绘** | `frames_drawn_delta = 0`;恢复后 40/40 |
| 进程冷启动到首帧 | 约 2–5 s | 三次运行整进程寿命 3–8 s(含 120 tick) |

## 2. 必须写进实现的约束

1. **永不最小化 Godot 主窗口**:最小化即 `Main::iteration` 跳过 `RenderingServer::draw`,离屏视口一起停。
   宿主窗口保持「屏外 + 无边框 + no_focus」,视口内容只经 FrameSink 出去。
2. **`application/run/main_scene` 必须非空**:`main/main.cpp:2342` 在 setup2 阶段检查
   `main_args.is_empty() && main_scene == ""` 即报错退出;自定义 MainLoop 不是 SceneTree,
   `main/main.cpp:4438-4439` 只在 SceneTree 分支加载主场景,因此填占位路径即可(不会被加载)。
3. **错误路径会弹模态框卡死进程**:上面的报错走 `OS::alert`(MessageBox),进程挂起直到人工关闭。
   监督器必须带启动超时 + 强杀;扩展侧自检失败要自己打印结构化错误并 `quit`,不要走到引擎的 alert 路径。
4. **回读格式随渲染方式不同**:RD(Forward+/Mobile)视口纹理回读为 `RGB8`,GLES3 为 `RGBA8`;
   FrameSink 前统一规整为紧凑 RGBA8(sRGB),与 rurix `FramePixels` 同形。
5. **stdout 不再只有就绪行**:gdext 会先打印一行初始化横幅;监督器的就绪行探测必须按行扫描前缀
   `FORGE_HOST_LISTENING`,不能假设首行(Stage 2/3 按 02 号文档核对)。
6. **gdext 0.5.5 要求 rustc ≥ 1.94**(0.5.x 全系 MSRV 1.94;`api-4-7` 仅 0.5.4+ 提供)。本机默认工具链 1.93.1;
   已并装 `1.94.1-x86_64-pc-windows-msvc`(rustup,minimal)。Stage 2 在仓库根新增 `rust-toolchain.toml`
   钉 1.94.1(一个小版本升级,单 workspace、单 lockfile,godot-host 与其余 crate 同构建),不改全局 default。
7. **4.7 绑定里带「伪缺省」的参数也必须显式传**:例如
   `RenderingDevice::get_driver_resource(resource, rid, index)`(文档注明 index 被忽略但必须给)。
8. **官方导出模板不支持 `--path` / `--main-pack` / `--main-loop`**:release 模板以 `disable_path_overrides` 编译,
   `--path` 直接报错退出(`main/main.cpp:1769-1786`;可用性标注见 `main/main.cpp:574-581`)。
   但模板会把 CWD 设为 exe 所在目录并把它当 `res://`(`main/main.cpp:1044-1053`)——**把运行时工程
   (project.godot + .gdextension + `.godot/extension_list.cfg` + `bin/*.dll`)直接放在 exe 同目录即可运行,
   无需 `.pck`,也无需自编模板**。已实测:`windows_release_x86_64_console.exe` + D3D12 在该布局下
   离屏出帧 39/40、回读 OK。唯一噪声是 `Could not load global script cache`
   (`core/config/project_settings.cpp:1458`),生成空的 `.godot/global_script_class_cache.cfg` 即可消除(Stage 3 落实)。
   → 开发期与打包期统一采用「运行时目录 = exe + 工程文件同目录」布局;开发期用 release/debug 模板,编辑器二进制只用于排障。
9. **宿主主循环应继承 `SceneTree`,而非裸 `MainLoop`**:`BaseMaterial3D` / `ParticleProcessMaterial` /
   `CanvasItemMaterial` 的着色器重建队列只由 `SceneTree` 的 idle callback 冲刷
   (`scene/register_scene_types.cpp:786, :877, :910` 注册各自 `flush_changes`,`scene/resources/material.cpp:2086`)。
   冒烟里 albedo 能生效是因为它只是 uniform;一旦切换 transparency / normal map 等「改着色器」的特性,
   裸 MainLoop 下改动可能永不落到 RS。继承 SceneTree(不挂任何业务节点,仍全程直调 RenderingServer)
   同时保住 MessageQueue 冲刷与 idle callback;`application/run/main_scene` 指向生成的最小
   `res://forge_runtime.tscn`(单个 Node),因为 SceneTree 分支会真的加载主场景(`main/main.cpp:4438-4439`)。

## 3. 已验证可用的最小运行时工程

`project.godot`:

```ini
config_version=5

[application]
config/name="ForgeSpike"
run/main_loop_type="ForgeSpikeLoop"
run/main_scene="res://forge_runtime.placeholder"
run/max_fps=120

[display]
window/size/viewport_width=64
window/size/viewport_height=64
window/size/borderless=true
window/size/no_focus=true
window/size/initial_position_type=0
window/size/initial_position=Vector2i(-32000, -32000)
window/vsync/vsync_mode=0

[rendering]
renderer/rendering_method="forward_plus"
```

`forge_spike.gdextension`:

```ini
[configuration]
entry_symbol = "gdext_rust_init"
compatibility_minimum = 4.7
reloadable = false

[libraries]
windows.debug.x86_64 = "res://bin/forge_spike.dll"
windows.release.x86_64 = "res://bin/forge_spike.dll"
```

`.godot/extension_list.cfg`(非编辑器运行时由它决定装载哪些扩展):

```text
res://forge_spike.gdextension
```

启动:`Godot_v4.7.2-stable_win64_console.exe --path <工程目录> --rendering-method forward_plus|gl_compatibility --rendering-driver vulkan|d3d12|opengl3`。

## 4. 冒烟代码要点(完整源码在本次会话 scratch,Stage 3 重写进 `crates/godot-host`)

- `#[derive(GodotClass)] #[class(base = MainLoop)]` + `impl IMainLoop { init / initialize / process / finalize }`。
- 场景:`scenario_create` → `environment_create`(`EnvironmentBg::COLOR`)→ `scenario_set_environment`;
  `BoxMesh::new_gd().get_rid()` + `StandardMaterial3D` → `instance_create2` + `instance_geometry_set_material_override`;
  `directional_light_create` → `instance_create2` + `instance_set_transform`。
- 相机 / 视口:`camera_create` + `camera_set_perspective(60, 0.05, 100)` + `camera_set_transform`;
  `viewport_create` → `viewport_set_size` / `viewport_set_scenario` / `viewport_attach_camera` /
  `viewport_set_update_mode(ViewportUpdateMode::ALWAYS)` / `viewport_set_active(true)`。
- 回读:`viewport_get_texture` → `texture_2d_get` → `Image`(同步,Stage 3 换 `texture_get_data_async` 或 L1 零拷贝)。
- 出帧计数:`Engine::get_frames_drawn()` 只在真正 draw 时递增,可直接当「是否停绘」探针。
