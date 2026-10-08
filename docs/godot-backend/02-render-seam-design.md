# 02 · engine-host 渲染接缝(Render Seam)盘点与设计

> 进度：已完成 §0-§9(第 3 轮完成);已按 01 结论修订 §2.4 §2.6 §4.1 §4.2 §5.3 §6.2,明细见 §9.5
> 日期 2026-09-27 起草,2026-09-28 第 3 轮完成;基线 = `main@817a229` + 用户未提交工作树(只读盘点,本文档是唯一新增文件)。
> 目标:让 engine-host 支持第二渲染后端(Godot 4.7.2 + gdext 0.5.5),**rurix 路径零行为变化**。
> 行号均针对当前工作树;`EH` = `crates/engine-host/src`。姊妹文档:`00-spike-report.md`(冒烟)、`01-godot-rendering-mapping.md`(另一 agent 编写,本文不触碰)。

> 后续实施与验收：见 [03-completion.md](03-completion.md)。本文保留接缝设计和阶段记录；“待实现”等表述须结合最新验收状态阅读。

## 0. 结论速览

**接缝形态**

- 接缝是"快照进、帧出"的消息式结构(§2.4-§2.5)。rurix 保持同步出帧(`Immediate`),调用链、线程、锁都不变,逐字节不变由 §2.6 I1 + §3.8 + §9.3 保证。Godot 是 `Pipelined`:`RenderSnapshot` → 锁外 `extract` 成 `RenderList` → `[gmain]` 算 `RenderDelta` 后调 RS;帧经 `FrameSink` 发布到 `FrameBus`。
- 新增的 `SubmitBox`(`GB`)和 `FrameBus`(`FB`)都是叶子锁,`[gmain]` 从不取 `HS`(I3、I4)。
- `RenderBackend` / `ImmediateRender` / `PipelinedRender` / `Capabilities` / `FrameSink` / `FrameOrigin` 的签名在 §4.1-§4.2。`RurixBackend` 只是把 `render_scene_frame` 和 `modelrender::render` 装进 trait(§4.4)。调用点改造清单共 19 项(§4.5)。取帧类 RPC 在 Godot 下走"短锁建请求 → 放锁等帧 → 短锁记账"。新增 `render.backendInfo` / `render.capabilities` 两个 RPC(§4.6)。
- CPU 逻辑(相机、拾取、精灵变换、裁剪、资产解码)整段搬进 `render_core`,原位置 re-export,用金值单测锁定逐位结果(§3)。
- crate 拆分:engine-host 变成 lib + bin(bin 名 `engine-host` 是测试契约);feature `backend-rurix` 缺省开启;godot-host 是 cdylib,以 `default-features = false` 依赖核心;两者共用 `start_core`(§5)。

**启动、配置与契约**

- engine-host 由 engine-scene-mcp 的 `supervisor.rs` 监督,已经按前缀扫描就绪行。它就绪后会停止读 stdout,Godot 宿主必须持续排空(§6.1-§6.2)。
- forge.toml 只有 assetd 一个解析器。新增 `[render]` 段,含校验矩阵;优先级 Cli > Env > ForgeToml > Default;缺省项目写出的 forge.toml 字节不变(§7)。
- IDE 契约全部不变(§8.1-§8.3)。bind 消息两端不一致已核实:desktop 发 6 段 `bind tex …`(RF/apps/desktop/src/main.cjs:358, :380),presenter 只收 7 段 `bind buf … <row_pitch>`(RF/crates/viewport-presenter/src/main.rs:448-466)。Stage 3 只改 desktop(§8.4)。
- `cargo check -p engine-host --tests --locked` 通过,engine-host 自身无警告(§9.1)。无 GPU 时渲染测试以 SKIP 通过(§9.2)。帧哈希基线的脚本与证据格式见 §9.3。

**与 01 的对齐结果**(01 已完成;按 01 的源码证据修订了 02,明细见 §9.5)

| 01 结论 | 结果 |
|---|---|
| T1 线程模型、T3 时序 | 一致(§2.4 的 `[gmain]` 就是 T1 的主线程回调);只修正了 GLES3 回读的发起时机 |
| T2 节拍 | 02 改为 `max_fps` = 推流帧率(缺省 60)、vsync 关、低处理器模式关(§2.6 I12、§6.2) |
| X1 L1 | §4.2 按 a2 变体重写:仅 D3D12 且 LUID 相同;共享 buffer / fence 建在 Godot 的 device 上;Godot 主队列拷贝并 Signal;presenter 与 `bind buf` 协议不变 |
| X2 L2、X3 发起与降级 | Vulkan / GLES3 走 L2;规整为 RGBA8、A=255;导出都在 `frame_post_draw` 里发起,失败只降级不停帧(§4.2) |
| R1 就绪行 | 一致(§5.3、§6.2);02 额外要求就绪后继续排空 stdout |
| D1 运行时目录 | 一致(§6.2);`forge-godot.exe` 降为占位名 |
| C1 相机 | 与 §3.1 一致(−Z 前向、正交全高 = 2 × orthoSize、丢 roll)。01 另指出 Godot 以顺时针为正面,Godot 腿写三角形时要交换绕序(Stage 4 实现项) |
| S1 2D 分流、K1 能力差异 | `RenderList.mode_2d` + `Leg` 足够 Godot 腿按 S1 分流;`render.capabilities` 以后按 K1 增加逐渲染方式的特性位 |

**仍未决 / 未核实**

1. ~~Godot 版本串和实际生效渲染方式 / 驱动的读回 API~~（验收时已解决）：`RS::get_current_rendering_driver_name()` / `get_current_rendering_method()`（GD/servers/rendering/rendering_server.h:1041-1042）、`get_video_adapter_name/vendor/type/api_version()`（:977-980）、`Engine::get_version_info()`（GD/core/config/engine.h:196），`render.backendInfo`（§4.6）直接用这些值（已补进 01 §4.3）。
2. 主窗口被最小化后的恢复 API(§6.2),01 §1.2 只确认了停绘条件。
3. ~~presenter 用哪个 adapter~~（验收时已解决）：缺省 adapter，`D3D12CreateDevice(None, …)`（RF/crates/viewport-presenter/src/main.rs:66），L1 判据 = Godot adapter LUID 等于缺省 adapter LUID（§4.2）。
4. 基线脚本用到的 `scene.load` 参数名、`--port 0`、WS 帧头字节序、8 MiB 帧上限是否也限制响应(§8、§9.3)。
5. 新建 godot-host 会改写用户未提交的 `Cargo.lock`,需先征得同意;workspace members 的写法未核实(§5.3)。
6. V6 的 `imported` 误报修不修(§9.4)。
7. 材质:02 §3.4 首版对齐 rurix 的 albedo 平均色,01 §5 提议直接映射 `.mat`,Stage 4 二选一。光源:`RenderList` 目前没有光源项(§2.5),01 §5 已给出 Light 与缺省灯光的映射,Stage 4 之前要补上。
8. 第 2 轮声明的"§2.4 两处行号校正"没有留下记录,无法还原(§9.5)。

## 1. 渲染/GPU/帧路径调用点全量盘点

### 1.0 记号(本节与 §2 共用)

线程(engine-host 现状;与 01 的 T1… 编号无关,§2.4 再做映射):

| 记号 | 线程 | 出处 |
|---|---|---|
| `[main]` | 主线程:建 HostState → 起物理线程 → `stream::spawn` → 绑 TCP → 打就绪行 → `--game` 启动 → 阻塞在 accept 循环 | EH/main.rs:36-91 |
| `[rpc]` | 每条 TCP 连接一个线程,`serve_conn` 读帧→`rpc::dispatch`→写帧 | EH/main.rs:85, :182-199 |
| `[phys]` | 物理/逻辑固定步线程,2 ms 轮询,每轮最多 `MAX_STEPS_PER_ITERATION`=2 步,**逐步重新加锁** | EH/main.rs:144-176;EH/sentinels_v6_clock.rs:13 |
| `[wsr]` | 推流渲染线程 `render_loop`(快照→锁外渲染→喂共享 buffer→广播) | EH/stream.rs:230, :419 |
| `[wsa]` | WS accept 线程 | EH/stream.rs:232-239 |
| `[wsc]` | 每个 WS 连接一个线程 `serve_ws`(发帧信箱/文本队列 + 5 ms 超时读输入) | EH/stream.rs:237, :255 |

锁与全局状态(全部是 `std::sync::Mutex`,没有 RwLock):

| 记号 | 对象 | 出处 |
|---|---|---|
| `HS` | `Arc<Mutex<HostState>>`,全宿主单锁;`rpc::lock` 毒化后取回内部值 | EH/main.rs:39;EH/rpc.rs:438-440 |
| `VR` | viewport `RENDERER: OnceLock<Mutex<RendererState>>`(rurix 精灵/网格会话) | EH/viewport.rs:940 |
| `LR` | viewport `LAST_REBUILD`(重建防抖时刻);另有原子 `ASSET_GENERATION`、`REBUILD_FLAG` | EH/viewport.rs:950, :941, :948 |
| `MR` / `MP` | modelrender `STATE`(模型会话)/ `PACKED`(打包贴图缓存) | EH/modelrender.rs:53-54 |
| `MM` | modelrt `MODELS`(ModelBundle 缓存) | EH/modelrt.rs:14 |
| `V6G` / `V6S` / `V6P` / `V6T` | V6 渲染器 `GPU` / `STAGED`(发布的世界快照)/ `PAUSED_AT` / `TERRAIN_BATCH`;另有 `RENDER_TIMINGS` | EH/sentinels_v6_render.rs:1262, :1020, :1021, :95, :13 |
| `SH` | share `SHARE: OnceLock<Mutex<Option<Producer>>>`(D3D12 共享 buffer 生产者) | EH/share.rs:88 |
| `REG` | stream `Registry.subs`;每订阅者另有 `frame` 信箱锁、`texts` 队列锁 | EH/stream.rs:89-91, :83, :85 |
| `C*` | 资产缓存:精灵文档 2 s TTL、贴图(按 `ASSET_GENERATION` 分代)、材质平均色、guid→路径 2 s 重建、网格 | EH/viewport.rs:441, :666, :1422, :1442;EH/meshres.rs:169, :190 |

实测嵌套顺序(读代码得出,未做运行时验证):`HS → REG`(`stream::primary_size`)、`HS → VR|MR|V6G → C*`、`VR → SH`(`current_import`→`share::vk_import_info`,EH/viewport.rs:975;`build_session_with`→`share::adapter_luid`,EH/viewport.rs:1353)、`HS → SH`(RPC 腿 `feed_share_frame`)。`[wsr]` 渲染时**不持** `HS`,只持 `VR|MR|V6G`;没有发现反向嵌套(持 `SH`/`VR` 再取 `HS`),所以当前没有锁序反转。

### 1.1 main.rs / frame.rs / rpc.rs(进程入口与 RPC 入口)

| file:line | 函数 | 作用 | 输入 | 输出 | 线程 | 持有的锁 |
|---|---|---|---|---|---|---|
| EH/main.rs:36 | `main` | 建状态、起 `[phys]`/`[wsr]`/`[wsa]`、绑 TCP、打就绪行、`--game` 启动 | CLI/env | 进程 | `[main]` | 短持 `HS`(:41, :72) |
| EH/main.rs:52 | `stream::spawn` 调用 | 起推流服务器;失败只 eprintln,视口回退轮询腿 | `Arc<Mutex<HostState>>` | WS 端口 | `[main]` | 无 |
| EH/main.rs:66-67 | 就绪行 | `println!("FORGE_HOST_LISTENING port={actual_port}")` + flush;stdout 只允许协议行 | 实际端口 | stdout 一行 | `[main]` | 无 |
| EH/main.rs:72-78 | `--game` 分支 | `rpc::game_boot`(scene_load + play_enter),成功再打 `FORGE_HOST_GAME_BOOTED scene=…` | `--game`/`FORGE_GAME_SCENE` | stdout 第二行 | `[main]` | `HS` |
| EH/main.rs:93, :123 | `parse_port` / `parse_game` | `--port N`/`--port=N` > `FORGE_HOST_PORT` > 17810;`--game`/`--game=` > `FORGE_GAME_SCENE` | argv/env | u16 / Option | `[main]` | 无 |
| EH/main.rs:144 | `spawn_physics_thread` | Running→`advance_frame`,Edit→`advance_preview`,Paused 不步进;每步重新加锁并 `yield_now` | `HS` | 场景写回 | `[phys]` | 每步持 `HS` |
| EH/main.rs:182 | `serve_conn` | 读帧→`rpc::dispatch`→写帧;坏 JSON 回 -32700 | TCP 帧 | TCP 帧 | `[rpc]` | 经 dispatch 持 `HS` |
| EH/frame.rs:9, :20 | `write_frame` / `read_frame_raw` | 4 字节 LE 长度前缀 + JSON;单帧上限 8 MiB(:6) | 字节流 | JSON | `[rpc]` | 无 |
| EH/rpc.rs:906 | `dispatch` | 总入口;`game.session.suggest/preview` 走锁外快照(:913-924);其余持 `HS` 调 `handle`;非只读方法成功后 `scene_rev+1`(:928-930,只读表 `READONLY_METHODS` :883) | JSON-RPC 请求 | 响应 | `[rpc]` | `HS` |
| EH/rpc.rs:969 | `handle` | 方法表(:975-1037);game 模式白名单 `GAME_ALLOWED`(:938, :971) | method/params | `HResult` | `[rpc]` | `HS` |
| EH/rpc.rs:975 | `host.ping` | 返回 `backend: st.backend`——**这是物理后端名**(jolt/rapier/none),不是渲染后端 | — | JSON | `[rpc]` | `HS` |
| EH/rpc.rs:1228 | `render_once` | soft-raster CPU 画一个三角形,`frames+1`、记 `render.frame` 事件 | — | `{frames,tris,nonZeroPixels}` | `[rpc]` | `HS` |
| EH/rpc.rs:1203 | `scene_summary` | 含 `render:{frames,lastTris,lastNonZeroPixels}` | — | JSON | `[rpc]` | `HS` |
| EH/rpc.rs:1252 | `viewport_size` | 缺省 960×540,钳到 16..=1920 × 16..=1080 | params | (w,h) | 调用方 | 调用方的 |
| EH/rpc.rs:1266 | `feed_share_frame` | 共享 buffer 喂帧:`imported`→`share::signal_frame`(只推 fence),否则 `share::write_frame`(CPU 上传);share 未开→`"no_share"` | `&FramePixels` | `(framePath, cpu_upload)` | `[rpc]` / `[wsr]` | `[rpc]` 下 `HS→SH`;`[wsr]` 下只 `SH` |
| EH/rpc.rs:1284 | `viewport_stream_info` | `ws://127.0.0.1:{port}/stream?token=…` + `proto:1` | — | JSON | `[rpc]` | `HS`(读 `stream::INFO`,OnceLock 无锁) |
| EH/rpc.rs:1296 | `viewport_frame` | 遗留 MCP 取帧腿:有推流订阅时尺寸让位 `stream::primary_size`(:1301);PIE 用 `scene_camera_view_proj`(:1310);`render_scene_frame`(:1316);喂共享(:1319);`format` = rgba8 / h264(`H264State::encode_frame` :1355)/ none | params | 帧 JSON | `[rpc]` | **`HS` 全程**(含 GPU 执行与回读)→ `REG` → `VR|MR|V6G` → `SH` |
| EH/rpc.rs:1398 | `viewport_set_camera` | EditorCamera 子集更新 + 钳制;WS `camera` 消息复用(EH/stream.rs:377-379) | params | 相机 JSON | `[rpc]` / `[wsc]` | `HS` |
| EH/rpc.rs:1439 | `viewport_pick` | 像素→射线→`pick_entity`(:1450) | x,y,w,h | `{hit,entityId,name,point}` | `[rpc]` | `HS`(+`MM`/`C*`) |
| EH/rpc.rs:1462 | `viewport_share_open` | `share::open(w,h,pid)`(:1468) | pid,w,h | 句柄 JSON | `[rpc]` | `HS→SH` |
| EH/rpc.rs:1033 | `viewport.shareClose` | `share::close()` | — | `{closed:true}` | `[rpc]` | `HS→SH` |
| EH/rpc.rs:1093 | `template_preview` | 空场景 `prefab::instantiate` → 设 Animator clip/time → `modelrender::bounds` 自动取景 → `modelrender::render`(:1097) | 模板参数 | rgba8 帧 JSON | `[rpc]` | `HS`(未读 st)→ `MR/MM` |
| EH/rpc.rs:1060 | `asset_reload` | `modelrt::invalidate` + `modelrender::invalidate` + `viewport::invalidate_assets`(:1061)→ 三类 GPU 会话作废 | guids | JSON | `[rpc]` | `HS→MM,MR,MP,VR` |
| EH/rpc.rs:1076 | `animation_control` | `modelrt::load` 校验 clip(:1079),写 Animator,edit 态登记预览 | action/clip/time | JSON | `[rpc]` | `HS→MM` |
| EH/rpc.rs:474 | `sync_camera_to_scene_mode` | 2d 场景→正交且 yaw/pitch 归零;scene_new(:1197)/scene_load(:1840)调用 | — | 相机 | `[rpc]` | `HS` |
| EH/rpc.rs:585 | `advance_frame` | 逻辑帧;V6 会话短路到 `sentinels_v6::advance`(:586);末尾 `anim.advance` + `modelrt::advance`(:666-667)——纯 CPU,写 `run_scene` | — | 场景 | `[phys]` / `[rpc]`(play.step) | `HS` |
| EH/rpc.rs:1049 | `advance_preview` | edit 态 Animator.time 推进,`scene_rev+1` | dt | 场景 | `[phys]` | `HS` |
| EH/rpc.rs:2085 | `pointer_to_world` | 归一化指针→场景相机射线(无相机退回编辑器相机)→与 z=0(2d)/y=0(3d)平面求交 | x01,y01,aspect | 世界点 | `[rpc]` / `[wsc]` | `HS` |
| EH/rpc.rs:2107 | `queue_pointer` | 尺寸缺省 `stream::primary_size`→960×540,只影响 aspect | action,x,y,size | 世界点 | `[rpc]` / `[wsc]` | `HS→REG` |
| EH/rpc.rs:1153 | `scene_new` | mode 缺省读 forge.toml `[project] mode`(:1162-1165,见 §7) | params | JSON | `[rpc]` | `HS` |

### 1.2 viewport.rs(rurix 精灵/网格主渲染腿)

| file:line | 函数 | 作用 | 输入 | 输出 | 线程 | 持有的锁 |
|---|---|---|---|---|---|---|
| EH/viewport.rs:73-189 | `m4_mul`/`m4_col_bytes`/`perspective_vk`/`orthographic_vk`/`look_at_rh`/`quat_to_mat3`/`trs_model` | 手写 f32 矩阵(行主序、列向量);`m4_col_bytes` 转列主序 UBO 字节 | 数值 | M4/字节 | 调用方 | 无 |
| EH/viewport.rs:197, :223-293 | `EditorCamera`(`eye`/`view_proj`/`ray`/`to_json`) | 环绕相机;`view_proj` 固定 near 0.05 / far 500;`perspective_vk` 先把 `m[1][1]` 取负(:100),`view_proj` 再取负一次(:254;`scene_camera_view_proj` 同样在 :1638),**两次抵消,最终是非翻转 Y + z∈[0,1]** | 相机参数 | M4 / 射线 / JSON | 调用方 | 调用方的(`HS`) |
| EH/viewport.rs:300 | `ray_unit_cube` | 射线 vs 单位立方体 OBB(点选) | 射线, Transform | t | 调用方 | 无 |
| EH/viewport.rs:339-387 | `is_renderable`/`sprite_component`/`sprite_sorting_order`/`sprite_blend`/`sprite_compositing` | 可渲染判定(MeshRenderer 或 Sprite 启用);Sprite 字段读取与混合模式 | Entity | 标量 | 调用方 | 无 |
| EH/viewport.rs:391 | `stable_rgba_inventory` | 2d 场景全员 `chromaKey=none` 且同一 z 平面 → 预先常驻所有图集变体,换帧不重建会话 | Scene | 常驻贴图表 | 渲染线程 | `C*` |
| EH/viewport.rs:437 | `sprite_doc_cached` | .rxsprite 文档缓存(逐 GUID 2 s TTL) | GUID | `Arc<SpriteDoc>` | 任意 | `C*` |
| EH/viewport.rs:476 | `resolve_sprite_render` | Sprite 当前帧:texture 直贴(居中)/ .rxsprite 图集(clip/frame/variants → uv_rect + frame_px + pivot) | Component | `SpriteRenderInfo` | 任意 | `C*` |
| EH/viewport.rs:545 | `sprite_render_transform` | 帧像素/ppu 缩放 + pivot 锚定折进 translation;**渲染与点选共用** | Entity | Transform | 任意 | `C*` |
| EH/viewport.rs:585 | `pick_entity` | 含 ModelRenderer 时先 `modelrender::pick`(:597,三角形精确求交),再对 MeshRenderer/Sprite 做 OBB | Scene, 相机, 像素 | (id, 点) | `[rpc]` | `HS`(+`MM`/`C*`) |
| EH/viewport.rs:623, :688 | `cube_mesh_bytes` / `quad_mesh_bytes` | 内置 cube(36 顶点,pos3+nrm3)/ 精灵 quad(pos3+nrm3+uv2) | — | 'static 字节 | 任意 | OnceLock |
| EH/viewport.rs:663 | `load_tex_static_cached` | 贴图 GUID → 解码 RGBA8(`Box::leak` 常驻,键含 `ASSET_GENERATION`) | 项目根, GUID | `&'static TexGpu` | 任意 | `C*` |
| EH/viewport.rs:845 | `compile_wgsl` | naga WGSL→SPIR-V(lang 1.3) | WGSL | SPIR-V | 任意 | OnceLock |
| EH/viewport.rs:887 | `slot_tier` | pass 槽数档位 24/32/64/96/128/192/256 | 实体数 | 槽数 | 渲染线程 | `VR` |
| EH/viewport.rs:907 | `CLEAR_RGBA` | 精灵/网格腿清屏色 ≈ RGBA8 [23,24,29,255];`nonzero` 统计以此为背景 | — | — | — | — |
| EH/viewport.rs:912-940 | `RendererState`/`ViewportRenderer`/`RENDERER` | Uninit / Ready / Degraded(原因缓存,不重试);会话含 `import_key`、粒子 pass、常驻贴图 | — | — | — | `VR` |
| EH/viewport.rs:942 | `invalidate_assets` | `ASSET_GENERATION+1` 并把 `VR` 置 Uninit | — | — | `[rpc]` | `HS→VR` |
| EH/viewport.rs:972 | `current_import` | share 已开且尺寸一致 → 取 NT handle/size/row_pitch(零拷贝档) | w,h | `Option<ShareImport>` | 渲染线程 | `VR→SH` |
| EH/viewport.rs:994, :1015 | `build_session` / `build_session_with` | `vk::vulkan_available`(:1028)/`probe_device_caps`(:1031)/需 synchronization2;建资源 + 每槽一个 raster pass;可选粒子 pass(:1289-1290,`FORGE_GPU_PARTICLES`);零拷贝档追加 pack compute pass;`Readback::Texture{res:2}`(:1318);`new_with_imported_d3d12_textures`(:1331)或 `new`(:1342);零拷贝档 LUID 对拍(:1348-1360),失败回退 readback 腿 | 槽位表、贴图、import | `ViewportRenderer` | 渲染线程 | `VR→SH` |
| EH/viewport.rs:1379-1511 | `entity_color`/`entity_tint`/`material_avg_color`/`content_guid_map_cached`/`resolve_material_avg_color`/`material_albedo_guid` | 实体着色:选中橙、id 调色板、材质 albedo 平均色;guid→Content 路径映射(2 s 重建) | Entity/GUID | 颜色/路径 | 渲染线程 | `C*` |
| EH/viewport.rs:1517 | `scene_camera_ray` | 首个启用 Camera 实体的射线;**使用 rotation 的 right/up(含 roll)** | Scene, nx,ny, aspect | 射线 | `[rpc]`/`[wsc]` | `HS` |
| EH/viewport.rs:1568 | `sprite_offscreen` | 四角投影,3 倍视口外或全在相机后 → 不进 renderables | Entity, vp | bool | 渲染线程 | `C*` |
| EH/viewport.rs:1607 | `scene_camera_view_proj` | 首个启用 Camera 实体的 view_proj;**`look_at_rh(eye, eye+fwd, +Y)` 丢弃 roll**,与 `scene_camera_ray` 不对称(现状,须保持) | Scene, aspect | `Option<M4>` | `[rpc]`/`[wsr]` | `HS` |
| EH/viewport.rs:1643-1664 | `FramePixels` / `pixels_b64` | 帧产物:紧凑 RGBA8 + `device_name/draws/truncated/nonzero/triangles/mesh_fallbacks/mesh_classes/imported` | — | — | — | — |
| EH/viewport.rs:1672 | `render_scene_frame` | **唯一帧源分派**:V6 场景(名 + `SentinelsV6Batch`)→ `sentinels_v6_render::render`(:1682-1684);含 ModelRenderer → `modelrender::render`(:1685-1687);否则 rurix 精灵/网格腿:取 `VR`(:1688)→ frame_vp(:1695)→ renderables/屏外裁剪 → 常驻贴图判定(:1703)→ 类槽位布局 → 布局签名 fnv(:1880)→ 重建判定(尺寸类 1500 ms 防抖 :1895)→ `FrameUpdate`(UBO + push constants,粒子发射器 :1951)→ `readback_subset`(:2094)→ `next_provenance_with_update`/`execute_with_frame_update`(:2100-2104)→ 回读长度校验 + 可选 nonzero 扫描 | Scene, 相机, selected, w,h, want_readback, want_stats, vp_override | `FramePixels` | `[rpc]` / `[wsr]` | `[rpc]`:`HS→VR→SH`;`[wsr]`:`VR→SH` |

要点:`render_scene_frame` 同时承担"选哪个渲染器"和"rurix 精灵/网格腿本体",Godot 接缝必须切在它的**调用方**一侧(§4.5),不能切在内部。

### 1.3 modelrender.rs / modelrt.rs / meshres.rs / material_override.rs / gpu_particles.rs

| file:line | 函数 | 作用 | 输入 | 输出 | 线程 | 持有的锁 |
|---|---|---|---|---|---|---|
| EH/modelrender.rs:9-13 | 常量 | 顶点 48 B(pos3@0 / nrm3@12 / uv2@24 / tangent4@32);清屏色 `[0.035,0.045,0.06,1]`;`MAX_DRAWS`=2048;顶点/贴图预算各 256 MiB | — | — | — | — |
| EH/modelrender.rs:16, :25 | `Draw` / `Renderer` | Draw = 实体 id + 稳定键 + **世界空间顶点** + 打包贴图 + push constants + blend + 距离;Renderer 持会话与泄漏图的裸指针,Drop 回收 | — | — | — | `MR` |
| EH/modelrender.rs:55 | `invalidate` | 清 `MR` 与 `MP` | — | — | `[rpc]` | `HS→MR,MP` |
| EH/modelrender.rs:63 | `bounds` | 世界顶点 AABB → (中心, 半径);template.preview 自动取景用 | Scene | (center, r) | `[rpc]` | `MM`/`C*` |
| EH/modelrender.rs:88 | `pick` | 对 CPU 世界三角形做 Möller–Trumbore 求交 | Scene, 射线 | (owner, t) | `[rpc]` | `MM`/`C*` |
| EH/modelrender.rs:177, :195 | `extra_shaders` / `shader` | 清屏/alpha 合成 compute + PBR WGSL(直接光 + 固定方向光 + Reinhard + gamma) | — | SPIR-V | 渲染线程 | OnceLock |
| EH/modelrender.rs:205-317 | `default_material`/`material_bytes`/`packed_length`/`charge` | 5 张 PBR 贴图打包进一个 u32 SSBO;预算记账 | ModelMaterial | 字节 | 渲染线程 | `MP` |
| EH/modelrender.rs:318 | `pc` | push constants:base/emissive/roughness/eye/metallic/normal_scale/occlusion/alpha 模式与 cutoff/unlit/double_sided/selected | 材质, eye | 字节 | 渲染线程 | 无 |
| EH/modelrender.rs:345 | `collect` | 逐 ModelRenderer:`modelrt::load_revision` → 祖先 Animator(clip/time/loop)→ `node_worlds` → nodeId 子树 → `entity_world`(Parent 链)→ `material_override::apply` → `modelrt::vertices`(CPU 蒙皮 + 世界变换);其余实体走 `legacy_draw`;不透明在前、BLEND 由远到近 | Scene, eye, selected | `Vec<Draw>` | 渲染线程 | `MM`/`C*` |
| EH/modelrender.rs:514 | `legacy_draw` | 模型场景里的 Sprite/MeshRenderer 转成模型 draw(复用 `viewport::resolve_sprite_render`/`sprite_render_transform`/`material_albedo_guid`/`load_tex_static_cached`,`meshres::load_mesh_cached`) | Entity | `Option<Draw>` | 渲染线程 | `C*` |
| EH/modelrender.rs:648 | `build` | `vulkan_available`(:649)/`probe_device_caps`(:652)→ 图 → `Readback::Texture{res:1}`(:837)→ `DeviceFrameSession::new`(:852);**不 import 共享 buffer** | w,h, draws, sig | Renderer | 渲染线程 | `MR` |
| EH/modelrender.rs:857 | `render` | eye:PIE 取相机实体世界位置,否则 `cam.eye()`;`collect` 为空 → `MODEL_EMPTY`;签名变→重建;**每帧上传全部世界顶点**,贴图只在重建时上传;相机 UBO = `vp` 或 `cam.view_proj`;返回 `imported:false`、`truncated:false`、`mesh_classes=draws.len()` | 同 `render_scene_frame` | `FramePixels` | `[rpc]`/`[wsr]` | `MR`(+`MM`/`C*`) |
| EH/modelrt.rs:14-63 | `MODELS`/`invalidate`/`load`/`load_revision` | ModelBundle 缓存;`assetd::project::ForgeProject::load(&root)`(读 forge.toml)+ `assetd::model::load_model[_revision]` | 引用, revision | `Arc<ModelBundle>` | 任意 | `MM` |
| EH/modelrt.rs:64-117 | `from_cols`/`point`/`vector`/`norm`/`inverse`/`normal_matrix` | 矩阵工具(高斯消元求逆) | — | — | 任意 | 无 |
| EH/modelrt.rs:118 | `entity_world` | Parent 链 TRS 连乘,检测环 | Scene, Entity | M4 | 任意 | 无 |
| EH/modelrt.rs:140 | `ancestor_component` | 沿 Parent 链找最近的启用组件(Animator) | — | Component | 任意 | 无 |
| EH/modelrt.rs:183, :231 | `sample` / `node_worlds` | 动画通道采样(slerp)→ 节点世界矩阵 | 模型, clip, time, loop | `Vec<M4>` | 任意 | 无 |
| EH/modelrt.rs:322 | `vertices` | 蒙皮调色板 + 节点/实体矩阵 → 48 B 世界顶点 | 模型, 节点, 图元, worlds, entity | 字节 | 渲染线程 | 无 |
| EH/modelrt.rs:395 | `advance` | run_scene 内 Animator.time += dt·speed | Scene, dt | 场景 | `[phys]` | `HS` |
| EH/meshres.rs:26-214 | `MeshGpu`/`mesh_vertex_bytes`/`resolve_rxmesh`/`load_mesh_fresh`/`load_mesh_cached`/`load_mesh_static_cached`/`fnv1a64` | .rxmesh(RXGB `read_dag`)→ 展平 pos3+nrm3;上限 `MAX_MESH_TRIANGLES`=2^18(:20)、`MAX_MESH_CLASSES`=8(:22);fnv1a64 也用于签名与 WS token | 项目根, 引用 | 字节 | 任意 | `C*` |
| EH/material_override.rs:29 | `apply` | 按槽位的 PBR 参数覆盖,不改导入资产 | 材质, overrides, slot | ModelMaterial | 渲染线程 | 无 |
| EH/gpu_particles.rs:16 | `enabled` | env `FORGE_GPU_PARTICLES` = on/1 才启用 | env | bool | 渲染线程 | 无 |
| EH/gpu_particles.rs:23 | `emitter_bytes` | ParticleEmitter 实体 → 64 条 × 32 B 发射器记录(中心 + [age,lifetime,kind] + id) | Scene | 字节, 活跃数 | 渲染线程 | 无 |
| EH/gpu_particles.rs:67, :176 | `ParticlePasses` / `append` | 往 rurix 图追加发射器/粒子 SSBO + compute + draw pass;只有 viewport 会话调用(EH/viewport.rs:1290) | 资源/pass 表 | 资源号 | 渲染线程 | `VR` |

要点:模型腿是"CPU 算好世界顶点、GPU 只做光栅"的形态;`collect`/`node_worlds`/`vertices`/`bounds`/`pick` 全是后端中立的 CPU 逻辑(§3)。

### 1.4 anim.rs / character.rs / prefab.rs(模板 / 预制体预览)

这三个文件**不直接碰 GPU**,但它们产出/消费渲染输入(Sprite.frame/clip、模型节点世界矩阵、预制体展开),接缝必须保证它们对两个后端给出同一份场景。

| file:line | 函数 | 作用 | 输入 | 输出 | 线程 | 持有的锁 |
|---|---|---|---|---|---|---|
| EH/anim.rs:44, :50 | `AnimSystem` / `clear` | play 会话内精灵帧动画状态;play.enter/exit 清空 | — | — | `[rpc]` | `HS` |
| EH/anim.rs:63, :74 | `advance` / `advance_with` | 应用 AnimCommand → FSM → 帧推进 → 回写 `Sprite.frame/clip`(**唯一写者**);`sprite_doc_cached` 解析 .rxsprite | run_scene, cmds, dt | 是否有可视变化 | `[phys]`(经 advance_frame EH/rpc.rs:666) | `HS→C*` |
| EH/character.rs:31 | `body_desc` | CharacterController/Collider → 物理 BodyDesc;模型碰撞体用 `modelrt::load/node_worlds/entity_world`(:69-71, :105-110)取世界顶点 | Scene, Entity | `Option<BodyDesc>` | `[rpc]`(play.enter / asset.reload) | `HS→MM` |
| EH/character.rs:150, :155 | `Controllers::clear` / `advance` | 角色控制器每步推进(物理查询 + 输入) | run_scene, world, body_map, input | — | `[phys]` | `HS` |
| EH/prefab.rs:163 | `instantiate` | 读 prefab 文档 → 分配 id → 挂 `PrefabInstance` 基线;末尾逐实体 `modelrt::entity_world` 校验无环(:190-192) | Scene, args | (Scene, JSON) | `[rpc]` | `HS` |
| EH/prefab.rs:198 | `refresh` | 按新 revision 重展开并保留 overrides(asset.reload / prefab.revert 用),`modelrt::ancestor_component` 取 Animator(:308) | Scene, root, revert | (Scene, JSON) | `[rpc]` | `HS` |
| EH/prefab.rs:377 | `root_id` | 找预制体根 | Scene, id | u64 | `[rpc]` | `HS` |
| EH/rpc.rs:1093 | `template_preview`(调用方) | 见 §1.1:**在空场景上渲一帧、不写 HostState**,是唯一"不走活动场景"的取帧 RPC,Godot 后端需要独立的离屏视口(§4.3 `PreviewRequest`) | — | — | `[rpc]` | `HS→MR` |

### 1.5 sentinels_v6*.rs(V6 批渲染)

V6 是 engine-host 自有的专用渲染腿:游戏状态在 `HostState.sentinels_v6`,渲染输入经全局 `STAGED` 旁路传递,`render` **忽略传入的 Scene**。

| file:line | 函数 | 作用 | 输入 | 输出 | 线程 | 持有的锁 |
|---|---|---|---|---|---|---|
| EH/sentinels_v6.rs:42 | `Session` | V6 会话:Game / Snapshot / View / 回放 / 指标 | — | — | — | `HS` |
| EH/sentinels_v6.rs:66 | `with_captured_game` | suggest/preview:短持 `HS` 克隆 Game,**锁外**规划(EH/rpc.rs:913-924 直达) | state, params | JSON | `[rpc]` | 短持 `HS` |
| EH/sentinels_v6.rs:123 | `handle` | `game.session.*`:open(:129)/close(:167,→`sentinels_v6_render::close`)/order(:174)/snapshot(:192)/applySnapshot(:204)/view(:236)/save/load/replay/replayControl/forfeit/pick(:127)/metrics(:126,含 `renderStagesByLayer`/`backendTimingsByLayer`)/pressureBenchmark(:125) | method, params | JSON | `[rpc]` | `HS`(+`V6S`) |
| EH/sentinels_v6.rs:384 | `advance` | Game::step 或回放推进;每 3 tick / 胜负变化克隆快照并 `publish` | — | — | `[phys]`(EH/rpc.rs:586 短路) | `HS` |
| EH/sentinels_v6.rs:416 | `publish` | `sentinels_v6_render::scene(&world,&view)` → `run_scene`(并写 `STAGED`);**直接改写编辑器相机**:target=`iso(center)`、yaw/pitch=0、dist=100、ortho、`ortho_half_h=24/zoom`;`scene_rev+1` | Session | run_scene, camera | `[phys]`/`[rpc]` | `HS→V6S` |
| EH/sentinels_v6.rs:437 | `iso` | 等距投影 `((x-y)/2, -(x+y)/4 + 1.5z)` | x,y,z | (x,y) | 任意 | 无 |
| EH/sentinels_v6_render.rs:14, :27 | `record_timing` / `render_metrics` | 分段计时 | — | JSON | 渲染线程 | `RENDER_TIMINGS` |
| EH/sentinels_v6_render.rs:96 | `terrain_batch` | 地形批缓存 | Snapshot | `Arc<TerrainBatch>` | 渲染线程 | `V6T` |
| EH/sentinels_v6_render.rs:209 | `compose` | Snapshot + View + visual_seconds → Batch(records + sprites + terrain) | — | Batch | 渲染线程 | `V6T` |
| EH/sentinels_v6_render.rs:1022 | `visual_time` | `tick/60 + received.elapsed()`——**墙钟插值**,暂停时冻结(`PAUSED_AT`) | Stage | 秒 | 渲染线程 | `V6P` |
| EH/sentinels_v6_render.rs:1031, :1059, :1078 | `set_paused` / `close` / `reset_scene` | 暂停冻结视觉时间;关闭/重置清 `V6G/V6S/V6T/…` | — | — | `[rpc]` | `HS→V6S,V6P,V6G` |
| EH/sentinels_v6_render.rs:1093 | `scene` | 生成占位 Scene(名 "Code Sentinels V6" + `SentinelsV6Batch{tick}` :1149)并写 `STAGED` | Snapshot, View | Scene | `[phys]`/`[rpc]` | `V6S` |
| EH/sentinels_v6_render.rs:1156 | `interpolate` | 按 previous 位置插值 | Batch, Stage | — | 渲染线程 | 无 |
| EH/sentinels_v6_render.rs:1357 | `build` | `probe_device_caps`(:1358)→ `DeviceFrameSession::new`(:1548)+ `Readback::Texture{res:2}`(:1552);槽位 1024 小 + 512 大 | w,h | Renderer | 渲染线程 | `V6G` |
| EH/sentinels_v6_render.rs:1567 | `upload_sprites` | 精灵页上传(`sentinels_v6_assets::pixels`/`sentinels_v6_pages::pixels`) | 精灵表 | FrameUpdate | 渲染线程 | `V6G` + 资产缓存 |
| EH/sentinels_v6_render.rs:1704 | `render` | 读 `STAGED` → compose → interpolate → 相机/记录上传 → 尺寸变则重建 → 执行 + 回读;返回 `draws:4`、`imported: sprite_count > 0`(:1841) | (忽略 Scene), w,h, readback, stats | `FramePixels` | `[rpc]`/`[wsr]` | `V6S→V6G` |
| EH/sentinels_v6_render.rs:1847 | `pick` | `game.session.pick`:按 View 重新 compose(:1884)后做屏幕点选 | params | JSON | `[rpc]` | `HS`(调用方)+`V6S` |
| EH/sentinels_v6_assets.rs:77, :155, :337, :351 | `available`/`describe`/`clear_caches`/`pixels` | V6 精灵清单、帧描述、像素解码 | 资产名 | 像素 | 渲染线程 | 各自缓存锁(:55, :154, :336) |
| EH/sentinels_v6_pages.rs:177, :202, :302 | `metrics`/`load_index`/`pixels` | 运行时图集页 | 根, 帧 | 像素 | 渲染线程 | `CACHE`(:176) |
| EH/sentinels_v6_backend_metrics.rs:32, :71 | `record` / `metrics` | 记录 rurix `DeviceFrameTelemetry` | 遥测 | JSON | 渲染线程 | `DATA`(:20) |
| EH/sentinels_v6_clock.rs:9, :42 | `Clock` / `domain` | 物理线程时钟策略(V6 保留墙钟债务,每轮 ≤2 步) | HostState | Domain | `[phys]` | `HS` |
| EH/sentinels_v6_pressure.rs:8 | `run` | 压力基准(纯模拟,文件内无渲染调用) | params | JSON | `[rpc]` | `HS` |

**疑似缺陷(读代码所得,未运行验证)**:EH/sentinels_v6_render.rs:1841 用 `imported: sprite_count > 0`,但 V6 会话从不 import 共享 buffer(`build` 只用 `DeviceFrameSession::new`)。`feed_share_frame`(EH/rpc.rs:1272-1273)见 `imported=true` 只推 fence、不写像素,所以 presenter 打开共享时 V6 画面不会进共享 buffer;WS 帧头 bit2(EH/stream.rs:407)也会误报"零拷贝"。§4.2 的 `FrameOrigin` 枚举把这个布尔换成显式来源,rurix 行为照旧(修复与否放 Stage 3 决定,见 §9.4)。

### 1.6 stream.rs / share.rs(推流与共享 buffer)

| file:line | 函数 | 作用 | 输入 | 输出 | 线程 | 持有的锁 |
|---|---|---|---|---|---|---|
| EH/stream.rs:34-43 | 常量 | 帧头 20 B;推流尺寸上限 1280×720;文本队列 8;空闲轮询 50 ms(无订阅)/ 15 ms(rev 未变) | — | — | — | — |
| EH/stream.rs:46-62 | `StreamInfo`/`INFO`/`REGISTRY`/`info`/`primary_size` | 端口 + token;主订阅者(最新注册者)尺寸是遗留腿的尺寸权威 | — | (w,h) | 任意 | `REG` |
| EH/stream.rs:71-91 | `SubCfg`/`Subscriber`/`Registry` | 订阅配置 {width,height,max_fps,selected};latest-wins 帧信箱;`cfg_rev` 代次 | — | — | — | `REG` |
| EH/stream.rs:186 | `parse_cfg` | 缺省 960×540@60,钳 16..=1280 × 16..=720,fps 1..=60;selected 不被 resize 覆盖 | JSON | SubCfg | `[wsc]` | `REG` |
| EH/stream.rs:204 | `gen_token` | 32 hex 随机 token(fnv 两轮,非加密) | — | String | `[main]` | 无 |
| EH/stream.rs:221 | `spawn` | 绑 127.0.0.1:0,起 `[wsr]`(:230)与 `[wsa]`(:232) | `Arc<Mutex<HostState>>` | 端口 | `[main]` | 无 |
| EH/stream.rs:255 | `serve_ws` | 握手校验 `?token=`(失败 403)→ 5 s 内首条必须 `subscribe` → 回 `hello{proto:1,width,height,maxFps}` → 循环:发帧信箱(Binary)→ 发文本 → 5 ms 超时读 | TCP | WS | `[wsc]` | `REG` + 信箱锁 |
| EH/stream.rs:348 | `handle_client_msg` | `input`→`queue_input`(:354);`pointer`→`queue_pointer`,尺寸取本订阅者(:373);`camera`→`viewport_set_camera` 成功后 `scene_rev+1`(:377-379);`select`/`resize`/`subscribe`→`REG` | 文本消息 | 状态变更 | `[wsc]` | `HS` 或 `REG` |
| EH/stream.rs:394 | `encode_frame` | `"FGF1" \| frameId u32 \| w u16 \| h u16 \| flags u32 \| draws u32` + 紧凑 RGBA8;flags bit0 play_running / bit1 truncated / bit2 imported(:407) | `FramePixels` | 字节 | `[wsr]` | 无 |
| EH/stream.rs:419 | `render_loop` | 无订阅→50 ms 空转;短持 `HS` 做快照(:441-455:**整场景 clone** + 相机 + play + rev + PIE vp);编辑/暂停态 (rev,cfg_rev) 未变则跳帧;锁外 `render_scene_frame`(:458,readback=true、stats=false);`feed_share_frame`(:470);再短持 `HS` 记 frames/cpu_uploads(:472);广播(:480);每秒 `status{playState,deviceName,draws,truncated,fps,shareError?}`;失败同因去重发 `error{message}` 并 1 s 限速;节拍 Running ≤maxFps≤60、其余 ≤30(:525) | 订阅配置 | WS 帧 | `[wsr]` | 快照时 `HS`;渲染时 `VR|MR|V6G→SH` |
| EH/share.rs:34 | `shared_layout` | 行距 = ceil(w·4/256)·256,总字节 = 行距·高 | w,h | (row_pitch, size) | 任意 | 无 |
| EH/share.rs:39, :68, :76 | `SharedBuf`/`ShareInfo`/`Producer` | 共享线性 buffer + upload buffer + 共享 fence + 事件;Producer 持 device/queue/allocator/list | — | — | — | `SH` |
| EH/share.rs:98 | `Producer::new` | `D3D12CreateDevice(None, FL_11_0)`(:102)——**缺省 adapter**,与 Vulkan 设备 LUID 不同就只能走 CPU 上传档 | — | Producer | `[rpc]` | `SH` |
| EH/share.rs:133 | `Producer::open` | DEFAULT 堆 + `HEAP_FLAG_SHARED` buffer;UPLOAD buffer 常驻映射;`CreateFence(…SHARED)`(:194);`CreateSharedHandle` ×2(:204, :208);`OpenProcess(PROCESS_DUP_HANDLE, pid)`(:211);`DuplicateHandle` ×2 进目标进程(:216, :225);本地 buffer 句柄留给 VK import | w,h,pid | (dup_buf, dup_fence) | `[rpc]` | `SH` |
| EH/share.rs:258 | `Producer::write` | 紧凑 RGBA8 按行距逐行拷进 upload → `CopyBufferRegion`(:293)→ `Signal(++v)` → 等 ≤2 s(:308) | rgba8,w,h | fence 值 | `[rpc]`/`[wsr]` | `SH` |
| EH/share.rs:320 | `Producer::signal` | 零拷贝档只推 fence | — | fence 值 | `[rpc]`/`[wsr]` | `SH` |
| EH/share.rs:350-424 | `open`/`adapter_luid`/`write_frame`/`is_open`/`vk_import_info`/`signal_frame`/`close` | 模块公开面;未打开时 write/signal 返回 `Ok(None)` | — | — | 任意 | `SH` |

要点:现有"帧出口"只有两个消费者——`FramePixels.rgba8`(WS 二进制帧 / RPC base64 / H.264 编码)和 D3D12 共享 buffer(`feed_share_frame`)。Godot 后端的 FrameSink 只要喂同样两处就能保住 IDE 契约(§4.2、§8)。

## 2. 线程与锁;一帧需要什么;RenderSnapshot

### 2.1 HostState 中与渲染相关的字段

`HostState`(EH/rpc.rs:244-299)由一把 `Mutex` 整体守护(注释原文"单 Mutex 全量守护,简洁优先",EH/rpc.rs:243)。渲染相关字段:

| 字段 | 行 | 写者(线程) | 渲染用途 |
|---|---|---|---|
| `sentinels_v6` | :245 | `[rpc]` game.session.*,`[phys]` V6 advance | V6 会话;发布到 `V6S` 后由 V6 腿渲染 |
| `scene` | :247 | `[rpc]` 编辑类方法 | 编辑态场景 |
| `run_scene` | :249 | `[phys]` advance_frame / V6 publish;`[rpc]` play.* | 运行态场景 |
| `play` | :251 | `[rpc]` play.* | 决定 `active()`(EH/rpc.rs:412-417)、PIE 相机、推流节拍 |
| `backend` | :261 | `[rpc]` play.enter / asset.reload | **物理**后端名;`host.ping`/`scene.summary` 以 `backend` 键返回——渲染后端信息不能复用这个键(§4.6) |
| `frames` / `last_tris` / `last_nonzero` / `cpu_uploads` | :269-275 | `[rpc]` viewport.frame / render.once;`[wsr]`(不写 last_nonzero) | `scene.summary.render`、`viewport.frame.cpuUploads` |
| `events` | :277 | `[rpc]` 推 `viewport.frame`/`render.frame` 事件;`[wsr]` 不推 | events.drain |
| `anim` / `preview_animations` | :281, :283 | `[phys]` | Sprite.frame/clip、Animator.time 的写者 |
| `camera` | :291 | `[rpc]` setCamera、scene.new/load 同步;`[wsc]` camera 消息;V6 publish | 编辑器相机(渲染/点选/指针) |
| `h264` | :293 | `[rpc]` viewport.frame(format=h264) | H.264 编码器(懒建,尺寸变重建) |
| `game_mode` | :295 | `[main]` --game | RPC 白名单 |
| `scene_rev` | :298 | dispatch 非只读成功(EH/rpc.rs:928-930)、`[wsc]` camera(EH/stream.rs:378-379)、advance_preview(EH/rpc.rs:1052)、V6 publish(EH/sentinels_v6.rs:434) | `[wsr]` 编辑态空闲跳帧 |

渲染器自身状态**不在** HostState 里,而是模块级全局(`VR`/`MR`/`V6G`/`V6S`/`SH`/`REG`/`C*`,见 §1.0)。

### 2.2 现状:线程与锁的交互

1. **RPC 取帧腿持 `HS` 跑完整帧**:`dispatch` 取 `HS`(EH/rpc.rs:925)后调 `viewport_frame`,GPU 执行、回读、base64/H.264 编码都在锁内(EH/rpc.rs:1296-1394)。这期间 `[phys]` 与其他 RPC 全部阻塞。stream.rs 头注释把这点列为推流腿的动机(EH/stream.rs:3-11)。
2. **推流腿快照 + 锁外渲染**:`[wsr]` 短持 `HS` 克隆活动场景、相机、play、rev,并按订阅尺寸算 PIE 相机(EH/stream.rs:441-455),放锁后渲染,再短持 `HS` 记计数(EH/stream.rs:472-477)。
3. **`[phys]` 逐步加锁**:每个固定步单独 `rpc::lock`(EH/main.rs:159),步间 `yield_now`,避免"一次追帧独占锁 0.5 s"(注释 EH/main.rs:151-155)。
4. **渲染器全局锁串行化两个取帧方**:`[rpc]` 与 `[wsr]` 共用 `VR`/`MR`/`V6G` 同一个会话;尺寸不同会触发重建拉锯,所以遗留腿在有推流订阅时让位推流尺寸(EH/rpc.rs:1297-1303),精灵/网格腿还有 1500 ms 纯尺寸重建防抖(EH/viewport.rs:1885-1900)。
5. **template.preview 用自己的尺寸渲模型会话**:模型会话签名含宽高(EH/modelrender.rs:883-884),与推流并发时会互相触发 `MR` 重建(读代码推断,未实测)。
6. **没有锁序反转**:未发现"持 `VR`/`MR`/`SH` 再取 `HS`"的路径(§1.0)。

### 2.3 一帧需要什么(rurix 现状)

`render_scene_frame(scene, cam, selected, width, height, want_readback, want_stats, vp_override)`(EH/viewport.rs:1672)的**显式输入**:

| 输入 | RPC 腿来源 | 推流腿来源 |
|---|---|---|
| `scene: &Scene` | `st.active()`(借用,锁内) | `st.active().clone()`(EH/stream.rs:455) |
| `cam: &EditorCamera` | `st.camera` 拷贝(EH/rpc.rs:1306) | 同左(快照) |
| `selected` | params.selectedId | 订阅者 `select` 消息 |
| `width/height` | params(钳制)→ 推流主订阅尺寸优先 | 主订阅者 cfg |
| `want_readback` | `format != "none"` | 恒 true |
| `want_stats` | 恒 true | 恒 false |
| `vp_override` | PIE 时 `scene_camera_view_proj(active, w/h)` | PIE 时按订阅 aspect 计算 |

**隐式输入**(接缝必须显式化,否则 Godot 后端拿不到):

- 项目根 `project_root()`:env `FORGE_PROJECT_ROOT`,否则编译期 `CARGO_MANIFEST_DIR/../../projects/demo`(EH/rpc.rs:495-505)——**运行时从 cdylib 调用时编译期路径仍然生效**,godot-host 必须显式传 env 或参数(§5、§6)。
- 资产代次 `ASSET_GENERATION` 与 `C*` 缓存(TTL 2 s 的精灵文档与 guid 映射,EH/viewport.rs:441-447, :1442-1453)。
- env `FORGE_GPU_PARTICLES`(EH/gpu_particles.rs:16-18)。
- 共享 buffer 状态(`current_import`,EH/viewport.rs:972-988)。
- V6:`STAGED` 快照 + 墙钟 `visual_time`(EH/sentinels_v6_render.rs:1022-1030),**不来自传入的 Scene**。

**输出**:`FramePixels`(EH/viewport.rs:1643-1659)。**副作用**:调用方更新 `frames/last_tris/last_nonzero/cpu_uploads`、推 `viewport.frame` 事件(仅 RPC 腿)、共享 fence +1(share 已开时)。渲染本身不改场景数据。

### 2.4 Godot 约束与线程模型提案

本节与 01 §1 的结论 T1-T3 一致(源码证据见 01 §1):T1 宿主固定 `thread_model=Safe`,RS / RD 只在 Godot 主线程(SceneTree 子类的 `process()` 与 `frame_post_draw` / frame-drawn 回调)里调用,forge 线程只经消息 / 快照交接,即下文的 `[gmain]`;T2 vsync 关、low_processor_mode 关、`max_fps` = 推流目标帧率(缺省 60);T3 一帧 = `process()` 应用差量 → 引擎 draw → `frame_post_draw` 发起导出 → 回调交付,至少晚 1 帧,宿主不自己调 `RS::draw()`,也不在锁里等帧。

约束(C1–C5):

- **C1 RS 只在 Godot 主线程调用**(任务书已定)。Godot 线程模型由 `rendering/driver/threads/thread_model` 决定,缺省 `RENDER_THREAD_SAFE`(GD/main/main.cpp:2776);RS 内部有 `is_on_render_thread` 断言宏(GD/servers/rendering/rendering_server.h:52-56, :1038-1039)。
- **C2 帧由 `Main::iteration` 按节拍产出**(GD/main/main.cpp:4921):`MainLoop::physics_process`(:5001)→ `MainLoop::process`(:5062)→ `RS::sync`(:5077)→ 仅当 `can_any_window_draw()` 为真才 `RS::draw`(:5080-5095)——这就是 00 §2.1 "最小化即停绘"的源码依据。低处理器模式下只在 `has_changed()` 时绘制(:5087-5091)。
- **C3 回读是异步的**:RD 路径 `RenderingDevice::texture_get_data_async(RID, layer, Callable)`(GD/servers/rendering/rendering_device.h:469;实现 GD/servers/rendering/rendering_device.cpp:2792)。回调在**之后某帧**的 `_begin_frame → _stall_for_frame` 里等该帧 fence 后调用(GD/servers/rendering/rendering_device.cpp:8048-8051, :8214-8290),即在调用 RD 的那个线程上、延迟约为帧队列深度(读代码推断,未实测)。GLES3 没有 RD(00 §1),只能同步 `RS::texture_2d_get`(GD/servers/rendering/rendering_server.h:131)。
- **C4 帧完成钩子**:`RS::request_frame_drawn_callback(Callable)`(GD/servers/rendering/rendering_server.h:964)在 `_draw` 末尾逐个调用,随后发 `frame_post_draw` 信号(GD/servers/rendering/rendering_server_default.cpp:72-73, :216-229)。
- **C5 engine-host 核心跑在 Godot 进程内**(已定决策 1):`[rpc]`/`[phys]`/`[ws*]` 仍是 Rust std 线程,由 `start_core`(§5)起;Godot 主线程**不能**碰 `HS`——否则慢 RPC/物理步会直接拖慢 Godot 主循环,并且形成 `[gmain]` 与持 `HS` 等帧的 RPC 之间的死锁环。

线程映射(rurix bin 与 godot-host 共用 `start_core`):

| 现状线程 | rurix bin(缺省,行为不变) | godot-host |
|---|---|---|
| `[main]` accept 循环 | 不变(`start_core` 返回后主线程进 accept) | `start_core` 另起 `[accept]` 线程;进程主线程归 Godot |
| `[rpc]` | 不变 | 不变;取帧类方法改走"短锁建请求→放锁等帧→短锁记账"(§4.5) |
| `[phys]` | 不变 | 不变。**不接** Godot `physics_process`(确定性红线,已定决策 3) |
| `[wsr]` | 快照→同步渲染→喂共享→广播(不变) | 变成**快照泵**:短锁快照 → `backend.submit()`(锁外做 §3 中立提取)→ 从 `FrameBus` 取 Godot 帧 → 广播/喂共享 |
| `[wsa]`/`[wsc]` | 不变 | 不变 |
| — | — | `[gmain]` Godot 主线程:`process()` 里取交接箱 → diff → RS 调用 → 挂回读;回读回调里规整 RGBA8 → `FrameSink` → 完成请求 |

一帧的时序(godot-host):

1. `[wsr]` 或 `[rpc]` 短持 `HS` 取 `RenderSnapshot`(§2.5),放锁;在本线程调 `extract`(§3)得 `RenderList`,投进交接箱(latest-wins),可附 `FrameRequest`(RPC 等帧用)。
2. `[gmain]` 在 `process()`(GD/main/main.cpp:5062)取最新 `RenderList`,与已应用的清单做 `RenderDelta`,只对变化项调 RS;设置视口尺寸与相机。
3. 同一次 iteration 的 `RS::draw`(:5089/:5093)把这些改动画出来。
4. `frame_post_draw`/`request_frame_drawn_callback`(C4)里对该视口纹理发起导出(01 X3):L2 在 Forward+ / Mobile 下调 `texture_get_data_async`,Compatibility 下在同一回调里同步 `texture_2d_get`(X2);L1 录制 GPU 拷贝并推进共享 fence(X1,§4.2)。
5. 回读回调(C3)里把 RGB8/RGBA8 规整为紧凑 RGBA8(00 §2.4),带上 `seq/scene_rev` 交给 `FrameSink`(§4.2),并完成所有 `min_seq ≤ seq` 且尺寸匹配的 `FrameRequest`;超过 deadline 的请求以 `RENDER_TIMEOUT` 失败。

这样接缝是"快照进、帧出"的消息式结构:没有任何线程在锁内同步等 Godot 出帧,rurix 路径完全不经过这些结构(§4.4)。

### 2.5 RenderSnapshot / RenderList / RenderDelta

三层结构分属不同线程。rurix 路径只用第一层(而且只在推流腿用,就是今天 EH/stream.rs:441-455 那份快照换了个名字),后两层只给 Pipelined 后端(Godot)用:

| 结构 | 产出者 | 线程 | 持锁 | 消费者 |
|---|---|---|---|---|
| `RenderSnapshot` | `render::snapshot(&HostState, SnapshotParams, seq)` | `[wsr]` / `[rpc]` | 短持 `HS`,与 EH/stream.rs:441-455 同一段、同样只做 clone | rurix:原样喂 `render_scene_frame`;Godot:`extract` |
| `RenderList` | `render_core::extract(&RenderSnapshot)`(§3 中立模块) | 同上,**锁外** | 只取 `C*`/`MM`(资产缓存) | `SubmitBox`(交接箱)→ `[gmain]` |
| `RenderDelta` | `RenderDelta::diff(applied, &next)` | `[gmain]` | 无(`[gmain]` 私有状态) | Godot 后端的 RS 调用 |

```rust
// crates/engine-host/src/render/snapshot.rs(Stage 2 新增)
use std::{path::PathBuf, sync::Arc};
use forge_scene::Scene;
use crate::render_core::camera::{EditorCamera, M4}; // §3.1 迁入中立模块;viewport.rs 原路径 re-export
use crate::rpc::{HostState, PlayState};

/// 谁要这一帧;决定尺寸权威、是否回读、是否做 nonzero 统计(§2.3 表)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRequester {
    /// `[wsr]`:readback=true、stats=false(EH/stream.rs:458-466)
    Stream,
    /// `viewport.frame`:readback = (format != "none")、stats=true(EH/rpc.rs:1296-1316)
    ViewportFrame,
}

#[derive(Debug, Clone, Copy)]
pub struct SnapshotParams {
    pub width: u32,
    pub height: u32,
    pub selected: Option<u64>,
    pub want_readback: bool,
    pub want_stats: bool,
    pub requester: FrameRequester,
}

/// 短锁内抄出的一帧输入:字段与 render_scene_frame(EH/viewport.rs:1672-1681)的显式输入
/// 一一对应,并把 §2.3 的隐式输入(项目根、资产代次)显式化。
#[derive(Clone)]
pub struct RenderSnapshot {
    pub seq: u64,                  // FrameBus::next_seq();rurix 路径不读
    pub scene_rev: u64,            // st.scene_rev(EH/rpc.rs:298)
    pub scene: Arc<Scene>,         // st.active().clone()(EH/stream.rs:455);Arc 只为跨线程投递
    pub camera: EditorCamera,      // st.camera(Copy,EH/viewport.rs:196-204)
    pub play: PlayState,           // st.play(EH/rpc.rs:251)
    pub vp_override: Option<M4>,   // 条件照搬各调用点:推流腿 play != Edit(EH/stream.rs:449-453),RPC 腿 EH/rpc.rs:1310
    pub params: SnapshotParams,
    pub project_root: Arc<PathBuf>,// rpc::project_root()(EH/rpc.rs:495-505)
    pub asset_generation: u64,     // viewport::ASSET_GENERATION(EH/viewport.rs:941)
}

/// 调用方已持 HS 守卫;函数内不做 IO、不取任何其他锁。
pub fn snapshot(st: &HostState, p: SnapshotParams, seq: u64) -> RenderSnapshot;
```

```rust
// crates/engine-host/src/render_core/list.rs(中立模块;不依赖 rurix-rt)
pub type M4 = [[f32; 4]; 4]; // 与 EH/viewport.rs 的 M4 同约定:行主序存储、列向量乘法

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    SpriteMesh,  // 缺省腿
    Model,       // 任一启用 ModelRenderer(EH/viewport.rs:1685)
    SentinelsV6, // scene.name == "Code Sentinels V6" 且含 SentinelsV6Batch(EH/viewport.rs:1682)
}
/// 与 render_scene_frame 的分派同判据、同顺序;两个后端必须共用这一个函数(§3.6)。
pub fn classify(scene: &Scene) -> Leg;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemKey { pub entity: u64, pub sub: u32 } // sub = 模型腿同实体第 n 个 draw;其余 0

pub struct RenderList {
    pub seq: u64,
    pub scene_rev: u64,
    pub width: u32,
    pub height: u32,
    pub mode_2d: bool,             // Scene.mode == "2d"(RF/crates/forge-scene/src/lib.rs:95-111)
    pub leg: Leg,
    pub view: ViewSetup,           // §3.1:分解后的相机参数 + 与 rurix 同算法的 view_proj
    pub clear_rgba: [f32; 4],      // SpriteMesh 腿 CLEAR_RGBA(EH/viewport.rs:907);Model 腿 CLEAR(EH/modelrender.rs:11)
    pub selected: Option<u64>,
    pub items: Vec<RenderItem>,    // 按 key 升序(供 diff 归并);绘制次序看 order
    pub particles: Vec<ParticleItem>, // 仅 gpu_particles::enabled()(EH/gpu_particles.rs:16)
    pub stats: ExtractStats,
    pub asset_generation: u64,
    pub project_root: Arc<PathBuf>,
}

pub struct RenderItem {
    pub key: ItemKey,
    pub order: u32,     // 绘制序:Model 腿 = collect 的"不透明在前、BLEND 远→近"(EH/modelrender.rs:345);
                        // SpriteMesh 腿 = rurix 槽位布局序(排序键 sprite_sorting_order EH/viewport.rs:351,§3.3)
    pub world: M4,      // 渲染态世界矩阵:Sprite 已折入 frame_px/ppu 与 pivot(EH/viewport.rs:545);
                        // Parent 链已连乘(EH/modelrt.rs:118);ModelDraw 恒为单位阵(顶点已在世界空间)
    pub content: u64,   // 内容指纹(fnv1a64,EH/meshres.rs):几何/贴图/uv/颜色/混合,不含 world
    pub body: ItemBody,
}

pub enum ItemBody {
    /// MeshRenderer(mesh 缺省 "cube",EH/viewport.rs:574-582);color = entity_color/entity_tint 结果(选中橙已折入)
    Mesh { mesh: MeshRef, color: [f32; 4], albedo: Option<TexRef> },
    /// Sprite:单位 quad × world;blend/chroma 与 sprite_blend/sprite_compositing(EH/viewport.rs:339-387)同解析
    Sprite { tex: TexRef, uv_rect: [f32; 4], tint: [f32; 4], flip: [bool; 2],
             blend: SpriteBlend, chroma_key: bool, sorting_order: f64 },
    /// 模型 draw:与 modelrender::Draw(EH/modelrender.rs:16-24)同源,顶点 48 B 布局(EH/modelrender.rs:9-10)已在世界空间
    ModelDraw { vertices: Arc<[u8]>, material: Arc<ModelMaterial>, blend: bool, distance: f32, legacy: bool },
}
pub enum MeshRef { Cube, Asset { reference: String, class: u32 } } // class 与 MAX_MESH_CLASSES=8(EH/meshres.rs:22)同分配
pub struct TexRef { pub guid: String, pub generation: u64, pub rgba: &'static TexGpu } // TexGpu 迁入中立模块(§3.4)
pub enum SpriteBlend { Opaque, Alpha, Additive } // Sprite.blendMode 三值;与 rex::BlendMode 一一映射
pub struct ParticleItem { pub entity: u64, pub center: [f32; 3], pub age: f32, pub lifetime: f32, pub kind: f32 } // EH/gpu_particles.rs:23 的 32 B 记录
pub struct ExtractStats { pub renderables: usize, pub truncated: bool, pub mesh_fallbacks: usize,
                          pub mesh_classes: usize, pub triangles: usize }

/// 锁外调用;Err 字符串前缀与现状一致(如 "MODEL_EMPTY: …",EH/modelrender.rs:878-880)。
pub fn extract(s: &RenderSnapshot) -> Result<RenderList, String>;
```

光源:engine-host 两条 rurix 腿都是固定光照(模型腿 WGSL 内置固定方向光,EH/modelrender.rs:195);`Light` 组件是否被 EH 读取见 §3.7 的核实。RenderList 暂不含光源项,Godot 腿的光照映射由 01 §2 定(§0 对齐项)。

```rust
// crates/engine-host/src/render_core/delta.rs
pub struct RenderDelta {
    pub full: bool,                  // 首帧 / leg 变 / asset_generation 变 / mode_2d 变 → 全量重建
    pub to_seq: u64,
    pub resize: Option<(u32, u32)>,
    pub view: Option<ViewSetup>,
    pub clear_rgba: Option<[f32; 4]>,
    pub removed: Vec<ItemKey>,
    pub added: Vec<RenderItem>,
    pub content: Vec<RenderItem>,    // 同 key、content 变:换 mesh/贴图/材质(RS 侧换 base/material 或重建)
    pub moved: Vec<(ItemKey, M4)>,   // 同 key 同 content、只有 world 变:只调 transform 类 RS 接口
    pub reordered: bool,             // order 序列变(2D sorting / BLEND 远近序)
    pub particles: Option<Vec<ParticleItem>>,
}
impl RenderDelta {
    /// 归并遍历两份按 key 升序的 items;world 逐元素按 f32::to_bits 比较(不设容差,NaN 也稳定)。
    pub fn diff(applied: Option<&RenderList>, next: &RenderList) -> RenderDelta;
    pub fn is_empty(&self) -> bool;
}
```

交接箱与帧总线(两把新锁都是**叶子锁**:持有时不取任何其他锁;`[gmain]` 只碰这两把,满足 §2.4 C5):

```rust
// crates/engine-host/src/render/bus.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel { Main, Preview } // Preview = template.preview 专用离屏视口(§4.3)

/// 输入侧交接箱(记号 `GB`)。latest-wins:[gmain] 每次 process() 只取最新一份。
pub struct SubmitBox { inner: std::sync::Mutex<SubmitState> }
struct SubmitState { latest: Option<Arc<RenderList>>, waiters: Vec<FrameRequest> }

/// RPC 等帧:min_seq = 自己那份快照的 seq;完成条件 frame.seq >= min_seq 且尺寸一致。
pub struct FrameRequest {
    pub min_seq: u64,
    pub size: (u32, u32),
    pub deadline: std::time::Instant,
    pub reply: std::sync::mpsc::SyncSender<Result<Arc<FrameOut>, String>>, // 容量 1
}

/// 输出侧帧总线(记号 `FB`):[gmain] 的回读回调写,[wsr] 等。
pub struct FrameBus {
    seq: std::sync::atomic::AtomicU64,
    latest: std::sync::Mutex<Option<Arc<FrameOut>>>,
    cv: std::sync::Condvar,
}
impl FrameBus {
    pub fn next_seq(&self) -> u64;
    pub fn publish(&self, f: Arc<FrameOut>);
    /// 等 seq > after 的帧或超时([wsr] 用;Condvar 等待期间不持 FB)。
    pub fn wait_newer(&self, after: u64, timeout: std::time::Duration) -> Option<Arc<FrameOut>>;
}

pub struct FrameOut {
    pub seq: u64,
    pub scene_rev: u64,
    pub pixels: FramePixels,  // 同一结构(EH/viewport.rs:1643-1659);rgba8 已规整为紧凑 RGBA8、首行在上
    pub origin: FrameOrigin,  // §4.2
}
```

错误约定沿用现有"大写前缀 + 冒号"风格(`DEV_ENV_DEGRADE:` EH/viewport.rs:1667;`MODEL_EMPTY:`):新增 `RENDER_NOT_READY:`(Godot 首帧前,冷启动 2–5 s,00 §1)、`RENDER_TIMEOUT:`(超过 deadline,缺省 3000 ms)、`RENDER_UNSUPPORTED:`(Capabilities 不含该腿,§4.1)。

### 2.6 不变量

接缝落地后必须始终成立的条件(Stage 2/3 的评审清单;"验证"列给出怎么证):

| # | 不变量 | 依据 / 现状 | 验证 |
|---|---|---|---|
| I1 | **rurix 逐字节不变**:缺省后端(rurix)下,RPC 返回、WS 二进制帧、共享 buffer 内容与 `main@817a229` 一致 | RurixBackend 只把现有函数装进 trait,不经过 `RenderList`/`RenderDelta`(§4.4);推流腿的 `RenderSnapshot` 与 EH/stream.rs:440-456 的元组同字段同时机 | §9.3 帧哈希基线在改动前后各跑一次,哈希全等 |
| I2 | **渲染不写场景**:`snapshot`/`extract`/`submit`/回读回调都不写 `HostState` 与场景 | 现状即如此(§2.3);唯一写回是调用方的计数(EH/stream.rs:472-477, EH/rpc.rs viewport_frame) | 代码评审:`[gmain]` 的代码路径里不出现 `rpc::lock` |
| I3 | **`[gmain]` 不持 `HS`**,只碰叶子锁 `GB`/`FB` | §2.4 C5 | 同上;另在 debug 构建给 `rpc::lock` 加"不得在 Godot 主线程调用"断言(线程 id 记在 `start_core` 里) |
| I4 | **锁序**:`HS → (放锁) → C*/MM → GB`;`[gmain]`:`GB`、`FB` 各自独立短持;等帧(`FrameRequest.reply` / `FrameBus::wait_newer`)时不持任何锁 | §1.0 的现有锁序不变,新锁不嵌套 | 代码评审 + `rpc_integration` 并发用例(§9.2) |
| I5 | **seq 单调**:`FrameOut.seq` 单调不减;`FrameRequest` 只被 `seq >= min_seq` 且尺寸相等的帧完成;WS 帧头 `frameId` 仍由 `[wsr]` 自增(EH/stream.rs:420, :480-481),与 seq 无关 | §2.5 | 单测:乱序完成的回读不得让 `latest` 回退 |
| I6 | **尺寸精确**:出帧尺寸 = 请求尺寸,不缩放、不裁切;Godot 视口 resize 之后的第一帧才可完成新尺寸请求 | rurix 会话按尺寸重建(EH/viewport.rs:1885-1900、EH/modelrender.rs:883-884) | `f1_viewport` 类用例对 Godot 后端复跑 |
| I7 | **像素规范**:`rgba8.len() == w*h*4`,紧凑、无行填充、sRGB 8 bit、**首行 = 显示的顶行**;不透明背景 alpha = 255 | rurix 靠投影侧双重 Y 取负得到直立画面(EH/viewport.rs:245-255 注释);清屏色 alpha 255(EH/viewport.rs:907);Godot 规整见 00 §2.4 | Godot 帧与 rurix 帧同场景目视对照 + 行序单测(顶行色块) |
| I8 | **统计语义不变**:`draws/triangles/truncated/mesh_fallbacks/mesh_classes/nonzero/imported/device_name` 按 EH/viewport.rs:1643-1659 的定义填;`nonzero` 只在 `want_stats` 时扫、背景色取本腿 clear;给不出的填 0/false,并在 Capabilities 声明(§4.1 `stats`) | §1.2、§1.6 | 契约测试对比两后端字段集 |
| I9 | **确定性红线**:渲染后端不参与逻辑/物理;`[phys]` 不感知后端,不接 Godot `physics_process` | 已定决策 3;V6 的墙钟 `visual_time` 只影响画面(EH/sentinels_v6_render.rs:1022) | `play.step` 回放哈希在两后端一致 |
| I10 | **后端单例、启动期决定**:进程内只有一个活动 `RenderBackend`(`OnceLock`),由 forge.toml `[render]` + CLI 在启动时选定;运行中不切换,切换 = 重启宿主 | §6、§7 | `render.backendInfo` 返回值在进程生命周期内不变 |
| I11 | **错误前缀兼容**:现有前缀(`DEV_ENV_DEGRADE:`、`MODEL_EMPTY:` 等)在 rurix 下原样;新前缀 `RENDER_NOT_READY:`/`RENDER_TIMEOUT:`/`RENDER_UNSUPPORTED:` 只由 Godot 后端产生 | §2.5 | 契约测试 |
| I12 | **推流节拍不变**:Running ≤ maxFps ≤ 60、其余 ≤ 30(EH/stream.rs:517-525);Godot 主循环 `max_fps` = 推流目标帧率(缺省 60)、vsync 关(01 §1 结论 T2),节拍仍由 `[wsr]` 决定 | §1.6 | `stream_ws` 用例 |

## 3. 需要后端中立化的 CPU 逻辑

原则:

- **P1 搬家不改写**:函数体逐字搬进新模块 `crates/engine-host/src/render_core/`(不 `use rurix_rt`),原位置留 `pub(crate) use render_core::…` 再导出,调用点一行不改。浮点运算次序不变 → 结果逐位不变(Rust 不做浮点收缩 / fast-math,`mul_add` 只在显式调用时出现;跨模块内联不改变 IEEE 结果)。
- **P2 rurix 专用的留在原处**:push constants 打包(`SPRITE_PC_LEN` EH/viewport.rs:373、`modelrender::pc` EH/modelrender.rs:318)、UBO 布局 `m4_col_bytes`(EH/viewport.rs:84)、槽位档位 `slot_tier`(:887)、常驻贴图 `stable_rgba_inventory`(:391)、`rex::BlendMode`。
- **P3 依赖面**:`render_core` 只依赖 `forge_scene`、`assetd`、`serde_json`;godot-host 以 `default-features = false` 依赖 engine-host 时仍能编译(§5)。
- **P4 一套 CPU 结果给两个后端**:点选、指针反投影、取景、屏外裁剪永远走 CPU,**不用** Godot 的物理射线或 GPU 拾取;这些 RPC 的返回因此与渲染后端无关。

### 3.1 相机

| 现状 | 位置 | 搬到 | 注意 |
|---|---|---|---|
| `V3`/`M3`/`M4`、`v3_*`、`m4_mul` | EH/viewport.rs:35-80 | `render_core::math` | `M4` 行主序存储、列向量 |
| `perspective_vk` / `orthographic_vk` / `look_at_rh` | EH/viewport.rs:96 / :109 / :122 | `render_core::math` | 名字保留;两者都先把 `m[1][1]` 取负(:100, :114) |
| `quat_to_mat3` / `m3_transpose` / `m3_apply` / `trs_model` | EH/viewport.rs:135-189(`trs_model` :178) | `render_core::math` | quat 内部归一化 |
| `EditorCamera`(`eye`/`basis`/`view_proj`/`ray`/`to_json`) | EH/viewport.rs:196-289 | `render_core::camera` | `view_proj` 固定 near 0.05 / far 500(:249-253),再把 `m[1][1]` 取负一次(:254) |
| `scene_camera_view_proj` | EH/viewport.rs:1607 | `render_core::camera` | `look_at_rh(eye, eye+fwd, +Y)`:**丢 roll**;用实体本地 `transform`,**不走** Parent 链 |
| `scene_camera_ray` | EH/viewport.rs:1517 | `render_core::camera` | right/up 取自 rotation(**含 roll**) |
| 模型腿 eye | EH/modelrender.rs:867-877 | `render_core::camera::model_eye` | PIE 时取 `modelrt::entity_world`(**走** Parent 链),是第三处不对称 |

三处不对称都是现状,rurix 必须保持;Godot 腿只要吃同一个 `ViewSetup`,画面就与 rurix 的 view_proj 同源。

```rust
// crates/engine-host/src/render_core/camera.rs
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    Perspective { fov_y_deg: f32 }, // 竖直视场角
    Orthographic { half_h: f32 },   // 半高(世界单位),与 Camera.orthoSize / EditorCamera.ortho_half_h 同义
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewSource { Editor, SceneCamera { entity: u64 } }

/// 分解后的相机:Godot 腿用前 7 个字段调 RS;view_proj 与 rurix 同算法(逐位相等,单测锁定)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewSetup {
    pub source: ViewSource,
    pub eye: V3,
    pub center: V3,             // look_at 注视点:Editor = cam.target;SceneCamera = eye + fwd
    pub projection: Projection,
    pub near: f32,              // Editor 恒 0.05 / far 恒 500;SceneCamera 取组件 near/far(缺省 0.1 / 500)
    pub far: f32,
    pub aspect: f32,            // width / height.max(1),与 render_scene_frame 同式(EH/viewport.rs:1695)
    pub view_proj: M4,
}
pub fn editor_view(cam: &EditorCamera, aspect: f32) -> ViewSetup;   // view_proj = cam.view_proj(aspect)
pub fn scene_view(scene: &Scene, aspect: f32) -> Option<ViewSetup>; // view_proj = scene_camera_view_proj(scene, aspect)
/// 与 look_at_rh 同式:f = norm(center - eye),s = norm(f × +Y),u = s × f。
pub fn view_basis(v: &ViewSetup) -> (V3 /*right*/, V3 /*up*/, V3 /*forward*/);
```

Godot 映射(源码核实):

- 变换:`camera_set_transform(cam, Transform3D)`(GD/servers/rendering/rendering_server.h:527),basis 三列 = (right, up, −forward),origin = eye。Godot 相机沿自身 −Z 看,与 `look_at_rh` 的"−z 为前向"同约定(Godot 侧约定以 01 §2 为准)。
- 透视:`camera_set_perspective(cam, fov_y_deg, near, far)`(rendering_server.h:524),并显式 `camera_set_use_vertical_aspect(cam, false)`(:532)。`render_camera` 把 `camera->vaspect` 当 `flip_fov` 传给 `Projection::set_perspective`(GD/servers/rendering/renderer_scene_cull.cpp:2701-2720);`flip_fov=false` 时 fov 就是竖直视场角(GD/core/math/projection.cpp:252-256)。
- 正交:`camera_set_orthogonal(cam, 2.0 * half_h, near, far)`(rendering_server.h:525)。`Projection::set_orthogonal(size, aspect, …, flip_fov=false)` 先 `size *= aspect`,再以 `±size/2` 为左右、`±size/aspect/2` 为上下(GD/core/math/projection.cpp:356-362),所以 `size` 是**全高**。
- 深度范围、NDC 的 Y 朝向只在 Godot 内部起作用;出帧行序由 FrameSink 统一(§2.6 I7)。

### 3.2 拾取与包围盒

| 现状 | 位置 | 搬到 | 注意 |
|---|---|---|---|
| `viewport.pick` | EH/rpc.rs:1438-1456 | 不动(RPC 层) | **始终用编辑器相机** `st.camera`(EH/rpc.rs:1449),PIE 下也一样——现状保持 |
| `pick_entity` | EH/viewport.rs:585 | `render_core::pick` | 有 ModelRenderer 先做三角形求交,再对 MeshRenderer/Sprite 做 OBB |
| `ray_unit_cube` | EH/viewport.rs:300 | `render_core::pick` | 零缩放不可选 |
| `modelrender::pick` / `bounds` | EH/modelrender.rs:88 / :63 | `render_core::model` | 都调 `collect(scene, eye, None)`,即连 CPU 贴图打包一起算,只为取三角形 / 顶点 |
| `sentinels_v6_render::pick` | EH/sentinels_v6_render.rs:1847 | 暂不动 | V6 专用,依赖 V6 compose;Godot 后端首版不支持 V6 腿(§4.1 `legs`) |

`collect`(EH/modelrender.rs:345)**整体搬迁**,包括 `material_bytes`/`packed_length`/`charge` 这些纯 CPU 的打包与预算记账(EH/modelrender.rs:205-317),不拆函数。拆成"几何 + 打包"两段会改变预算超限时的报错时机(`pick` 对 `collect` 的 Err 返回 None,`bounds` 返回 Err),逐字节不变就无法验证。`Draw` 只追加一个字段 `material: Arc<ModelMaterial>`(`material_override::apply` 之后的值),供 Godot 腿建材质;rurix 不读它。

### 3.3 精灵变换与可见性

| 现状 | 位置 | 搬到 | 注意 |
|---|---|---|---|
| `is_renderable` / `sprite_component` / `sprite_sorting_order` / `sprite_num` / `sprite_bool` / `FULL_UV_RECT` | EH/viewport.rs:339 / :346 / :353 / :360 / :365 / :370 | `render_core::sprite` | |
| `sprite_blend` / `sprite_compositing` | EH/viewport.rs:375 / :383 | `render_core::sprite::blend` 返回 `SpriteBlend`;rurix 侧 `impl From<SpriteBlend> for rex::BlendMode`,`viewport::sprite_blend` 变成一行包装 | `compositing = [chroma≠none, blend≠opaque, 0, 0]` 照搬 |
| `SpriteRenderInfo` / `resolve_sprite_render` | EH/viewport.rs:461 / :476 | `render_core::sprite` | 函数内部调 `rpc::project_root()`;中立版改为参数(取 `RenderSnapshot.project_root`),rurix 包装仍传 `rpc::project_root()`,值相同 |
| `sprite_render_transform` | EH/viewport.rs:545 | `render_core::sprite` | 渲染与点选共用 |
| `entity_mesh_ref` | EH/viewport.rs:574 | `render_core::sprite` | 缺省 "cube" |
| `sprite_offscreen` + `OFFSCREEN_CULL_MARGIN = 3.0` | EH/viewport.rs:1568 / :1561 | `render_core::cull` | 输入是 `frame_vp`;Godot 腿传 `ViewSetup.view_proj`(同一矩阵),两后端可见集相同 |
| `modelrender::legacy_draw` | EH/modelrender.rs:514 | 随 `collect` 搬 | 读 tint(:549)、flipX/flipY(:575) |
| 槽位布局、常驻贴图、`SPRITE_PC_LEN` | EH/viewport.rs:887 / :391 / :373 | 留 rurix | 128 槽预算是 rurix 限制;Godot 腿不继承 `truncated` 语义(§4.1 `max_draws`) |

### 3.4 资产解码缓存

| 现状 | 位置 | 处理 | 注意 |
|---|---|---|---|
| `TexGpu{w, h, rgba: &'static [u8]}` / `load_tex_static_cached` | EH/viewport.rs:656-660 / :663 | 搬 `render_core::assets` | 本来就是 CPU RGBA8(名字里的 Gpu 是历史叫法);Godot 腿用它建 `Image` 再 `texture_2d_create` |
| `sprite_doc_cached` | EH/viewport.rs:437 | 同上 | 逐 GUID 2 s TTL |
| `entity_color`/`entity_tint`/`material_avg_color`/`content_guid_map_cached`/`resolve_material_avg_color`/`material_albedo_guid` | EH/viewport.rs:1379-1511 | 同上 | 精灵/网格腿"材质 = albedo 平均色"是 rurix 现状;Godot 腿首版对齐这一行为,是否改用真实贴图列为 §0 待定项 |
| `ASSET_GENERATION` / `invalidate_assets` | EH/viewport.rs:941 / :942 | 代次计数搬 `render_core::assets`;`invalidate_assets` 仍在 viewport(清 `VR`),另经 `RenderBackend::invalidate_assets` 通知 Godot 腿(§4.1) | |
| `meshres::*`、`modelrt::*` | EH/meshres.rs:26-214、EH/modelrt.rs | 原地保留 | 纯 CPU;是否已无 `rurix_rt` 依赖见 §5.2 的 grep |

### 3.5 指针反投影

| 现状 | 位置 | 处理 |
|---|---|---|
| `pointer_to_world` | EH/rpc.rs:2085 | 留在 rpc.rs;射线改从 `render_core::camera` 取(`scene_camera_ray`,无相机退 `EditorCamera::ray`);2d 场景与 z=0 求交、3d 与 y=0 求交 |
| `queue_pointer` / `logic.inject_pointer` | EH/rpc.rs:2107 | 不动;尺寸缺省 `stream::primary_size` → 960×540,只影响 aspect |
| WS `pointer` 消息 | EH/stream.rs:373 | 不动;尺寸取本订阅者 |

这条链只依赖 HostState 与相机数学,本来就与渲染后端无关;抽走相机数学后只有 import 路径变化。

### 3.6 分派判据与 V6

- `classify`(§2.5)= `render_scene_frame` 开头两处判据(EH/viewport.rs:1682、:1685)。rurix 仍在 `render_scene_frame` 里自己判(不改),Godot 腿调 `classify`;单测保证二者对全部 fixture 同结果。
- V6 的 `iso`(EH/sentinels_v6.rs:437)和 `publish` 改写编辑器相机(EH/sentinels_v6.rs:416)属游戏侧逻辑,不搬。`compose`/`interpolate`(EH/sentinels_v6_render.rs:209、:1156)是 CPU,但产出 V6 私有 Batch;将来要 Godot 画 V6 时再中立化(§4.1 `legs`)。

### 3.7 核实:Light 组件

- EH 全目录 grep `"Light"|castShadow` 零命中:rurix 两条腿都不读 Light,光照固定(模型腿 PBR WGSL 内置方向光,EH/modelrender.rs:195)。
- 注册表:`Light{kind: string(无枚举), color: [f32;3], intensity: number, castShadow: bool}`,四个字段都没有缺省值(RF/crates/forge-scene/src/lib.rs:257-266);:263 的注释写明"渲染内核不消费"。
- 结论:RenderList 暂不含光源;Godot 腿要用 Light,得先定 `kind` 的取值集合(注册表里没有枚举),属 01 §2 / Stage 3 决策(§0)。

### 3.8 逐字节不变的保证

1. 纯搬迁 PR(在 §9.3 基线之后):只移动函数 + re-export,viewport.rs / modelrender.rs 除 `use` 外只减不增。
2. 金值单测:同一 PR 的**第一个提交**(搬迁前)给 `EditorCamera::view_proj/ray`、`scene_camera_view_proj/ray`、`sprite_render_transform`、`ray_unit_cube`、`pick_entity`、`modelrender::bounds` 写 fixture,断言 `f32::to_bits` 全等;搬迁后这组测试必须仍然全绿。
3. 帧哈希:§9.3 流程在搬迁前后各跑一次,`rurix-frame-baseline.json` 全等。
4. `ViewSetup.view_proj` 与 `cam.view_proj(aspect)` / `scene_camera_view_proj` 逐位相等的单测(Godot 腿吃的就是这个矩阵)。

## 4. RenderBackend / FrameSink / Capabilities 精确签名 + RurixBackend 映射 + 调用点改造清单

设计要点:

- 两种出帧形态分开建模:`Immediate`(rurix,调用方线程同步出帧,即今天的 `render_scene_frame`)与 `Pipelined`(Godot,提交 `RenderList`,帧从 `FrameBus` 出,§2.4)。**不**强行统一成一个 `render()`,否则 rurix 要为迁就异步而改调用时机,I1 无法保证。
- 后端是进程级单例(I10)。`render::backend()` 未安装时惰性装 `RurixBackend`,直接调 `rpc::dispatch` 的测试不用改。
- 下文签名均为 Stage 2 新增代码的提案;引用的现有函数签名已按源码核对(`render_scene_frame` EH/viewport.rs:1672-1681,`FramePixels` EH/viewport.rs:1642-1659,`feed_share_frame` EH/rpc.rs:1266-1268,`dispatch` EH/rpc.rs:906)。

### 4.1 RenderBackend 与 Capabilities

```rust
// crates/engine-host/src/render/backend.rs(Stage 2 新增)
use std::sync::{Arc, OnceLock};
use forge_scene::Scene;
use crate::render::bus::{Channel, FrameBus, FrameRequest};
use crate::render_core::{camera::{EditorCamera, M4}, list::{Leg, RenderList}};
use crate::viewport::FramePixels;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind { Rurix, Godot }
/// forge.toml [render].method / driver(§7);rurix 下两者都是 None。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMethod { ForwardPlus, Mobile, GlCompatibility }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderDriver { D3d12, Vulkan, Opengl3 }
/// 配置来自哪里,优先级 Cli > Env > ForgeToml > Default(§7.3)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource { Default, ForgeToml, Env, Cli }

#[derive(Debug, Clone)]
pub struct BackendInfo {
    pub kind: BackendKind,
    pub method: Option<RenderMethod>,
    pub driver: Option<RenderDriver>,
    pub source: ConfigSource,
    pub godot_version: Option<String>, // 取值来源见 01 §4(未核实,§0 对齐项)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LegSet { pub sprite_mesh: bool, pub model: bool, pub sentinels_v6: bool }
impl LegSet { pub fn contains(&self, leg: Leg) -> bool; }

/// I8:给不出的统计填 0/false,并在这里声明 false。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatsCaps {
    pub nonzero: bool, pub triangles: bool, pub truncated: bool,
    pub mesh_fallbacks: bool, pub mesh_classes: bool,
}
/// None = 该腿不截断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaxDraws { pub sprite_mesh: Option<u32>, pub model: Option<u32>, pub sentinels_v6: Option<u32> }

#[derive(Debug, Clone)]
pub struct Capabilities {
    pub legs: LegSet,
    pub pipelined: bool,    // true = 帧经 FrameBus 异步产出
    pub preview: bool,      // template.preview 可用
    pub particles: bool,    // rurix = gpu_particles::enabled()(EH/gpu_particles.rs:16);Godot 首版 false
    pub cpu_rgba8: bool,    // L2 出口;恒 true(WS / RPC / H.264 都靠它)
    pub shared_d3d12: bool, // 能喂 D3D12 共享 buffer(CPU 上传或 GPU 直写)
    pub zero_copy: bool,    // 能不经 CPU 写共享 buffer(rurix = import 档;Godot = L1)
    pub stats: StatsCaps,
    pub max_draws: MaxDraws,
}

pub enum FramePath<'a> { Immediate(&'a dyn ImmediateRender), Pipelined(&'a dyn PipelinedRender) }

pub trait RenderBackend: Send + Sync + 'static {
    fn info(&self) -> &BackendInfo;
    fn capabilities(&self) -> &Capabilities;
    fn path(&self) -> FramePath<'_>;
    /// asset.reload 在现有三处 invalidate(EH/rpc.rs:1060-1061)之后调用;rurix 为空操作。
    fn invalidate_assets(&self);
    /// 最近一帧的 FramePixels.device_name;首帧前 None(render.backendInfo 用)。
    fn device_name(&self) -> Option<String>;
}

/// rurix:调用方线程同步出帧。
pub trait ImmediateRender: Send + Sync {
    fn render(&self, input: FrameInput<'_>) -> Result<FramePixels, String>;
    /// template.preview(EH/rpc.rs:1097 的 modelrender::render 调用)。
    fn preview(&self, input: FrameInput<'_>) -> Result<FramePixels, String>;
}

/// Godot:提交 RenderList;[gmain] 出帧后经 FrameSink(§4.2)发布到 FrameBus。
pub trait PipelinedRender: Send + Sync {
    fn next_seq(&self) -> u64;
    /// latest-wins 投进 ch 的 SubmitBox;req 进 waiters。不阻塞、不取 HS。
    fn submit(&self, ch: Channel, list: Arc<RenderList>, req: Option<FrameRequest>) -> Result<(), String>;
    fn bus(&self, ch: Channel) -> &FrameBus;
    fn control(&self, msg: ControlMsg) -> Result<(), String>;
    /// Main 通道首帧已发布;否则取帧类 RPC 返回 RENDER_NOT_READY。
    fn ready(&self) -> bool;
}

/// 非帧类控制消息;同样经 SubmitBox,[gmain] 在 process() 开头先处理控制消息再处理 RenderList。
pub enum ControlMsg {
    ShareAttach(SharedTarget), // §4.2 L1;[rpc] 在 share::open 成功后投递
    ShareDetach,               // viewport.shareClose 之后
    InvalidateAssets { generation: u64 },
    Shutdown,
}

/// 与 render_scene_frame 的八个参数一一对应(EH/viewport.rs:1672-1681);只借用、不 clone。
#[derive(Clone, Copy)]
pub struct FrameInput<'a> {
    pub scene: &'a Scene, pub cam: &'a EditorCamera, pub selected: Option<u64>,
    pub width: u32, pub height: u32, pub want_readback: bool, pub want_stats: bool,
    pub vp_override: Option<M4>,
}

static BACKEND: OnceLock<Box<dyn RenderBackend>> = OnceLock::new();
/// start_core(§5.3)在起任何线程之前调用一次;重复安装返回 Err(I10)。
pub fn install(b: Box<dyn RenderBackend>) -> Result<(), String>;
/// 未安装时惰性装 RurixBackend::new():直接调 rpc::dispatch 的测试零改动。
pub fn backend() -> &'static dyn RenderBackend;
```

两个后端的 Capabilities 取值:

| 字段 | RurixBackend | GodotBackend(首版) |
|---|---|---|
| `legs` | 三腿全开 | `sprite_mesh` + `model`;`sentinels_v6` = false(§3.6),V6 场景取帧返回 `RENDER_UNSUPPORTED:` |
| `pipelined` / `preview` | false / true | true / true(`Channel::Preview`,§4.3) |
| `particles` | `gpu_particles::enabled()` | false(映射方案见 01 §5:particles_* + ParticleProcessMaterial;首版不启用) |
| `cpu_rgba8` / `shared_d3d12` | true / true | true / true |
| `zero_copy` | true(是否真正进入 import 档仍由 `current_import` 按帧决定,EH/viewport.rs:972) | L1 可用且 LUID 一致时 true(§4.2) |
| `stats` | 全 true | `nonzero`/`triangles` true;`truncated`/`mesh_fallbacks`/`mesh_classes` 首版 false(恒填 false/0) |
| `max_draws` | sprite_mesh = `MAX_DRAW_SLOTS`(`slot_tier` 顶档,EH/viewport.rs:886-902);model = `MAX_DRAWS` 2048(EH/modelrender.rs:9-13);sentinels_v6 = 1024 小 + 512 大槽(EH/sentinels_v6_render.rs:1357) | 全 None |

注:`slot_tier` 的注释写"超 128 仍截断"(EH/viewport.rs:885),但函数体已有 192 与 `MAX_DRAW_SLOTS` 两档(:898-901),注释过时;以函数体为准。

### 4.2 FrameSink、FrameOrigin 与 L1 / L2 出口

FrameSink 是 Godot 后端 `[gmain]` 把帧交还宿主的唯一入口;rurix 同步腿不经过它(I1),调用方继续直接调 `feed_share_frame`。帧导出的 Godot 侧细节见 01 §3 结论 X1-X3:L1 只在 D3D12 且 adapter LUID 相同时启用(X1),其余一律 L2(X2);导出都在主线程的 `frame_post_draw` 里发起,失败只降级(L1 → L2)、不停帧(X3)。

```rust
// crates/engine-host/src/render/sink.rs(Stage 2 新增)
use crate::render::bus::{Channel, FrameOut};
use crate::viewport::FramePixels;

/// 帧像素的来源。与 FramePixels.imported 的关系是不变量:
/// imported == matches!(origin, SharedZeroCopy | SharedGpuCopy)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOrigin {
    CpuReadback,    // 像素在 pixels.rgba8(rurix 回读档 / Godot L2)
    SharedZeroCopy, // rurix import 档:已直渲进共享 buffer,只需推 fence
    SharedGpuCopy,  // Godot L1:[gmain] 已在 GPU 上把视口拷进共享 buffer,且拷贝已完成
    NoPixels,       // want_readback = false(viewport.frame format=none)
}
impl FrameOrigin {
    /// 逐字节保持现状:imported → SharedZeroCopy,否则 CpuReadback;
    /// V6 的 imported 误报(§1.5)也原样映射,修不修放 Stage 3(§9.4)。
    pub fn from_rurix(f: &FramePixels) -> FrameOrigin;
}

/// L1 目标(01 X1 的 a2 变体):共享 buffer 与共享 fence 由 share::open 在 **Godot 的 ID3D12Device** 上创建
/// (布局、DuplicateHandle、RPC 返回值都与今天相同),这里只把本进程内的 COM 指针交给 [gmain]。
#[derive(Debug, Clone, Copy)]
pub struct SharedTarget {
    pub buffer: usize,  // ID3D12Resource*(已 AddRef;[gmain] 处理 ShareDetach 时 Release)
    pub fence: usize,   // ID3D12Fence*(同上);L1 期间只由 [gmain] 在 Godot 主队列上 Signal(++v)
    pub width: u32,
    pub height: u32,
    pub row_pitch: u32, // shared_layout:ceil(w*4/256)*256(EH/share.rs:34)
    pub size: u64,      // row_pitch * height
}

pub trait FrameSink: Send + Sync {
    /// [gmain] 在 L2 回读回调 / L1 命令提交之后调用:只 publish 到 FrameBus 并完成 waiters,
    /// 不取 HS/SH(I3)。L1 的 GPU 拷贝与 Signal 由 [gmain] 在调用前自己录制提交。
    fn deliver(&self, ch: Channel, frame: FrameOut);
}

/// 宿主实现:两条通道各一个 FrameBus + SubmitBox。
pub struct HostFrameSink {
    pub main: Arc<FrameBus>, pub preview: Arc<FrameBus>,
    pub main_box: Arc<SubmitBox>, pub preview_box: Arc<SubmitBox>,
}

/// 共享 buffer 喂帧,由消费线程([wsr] / 等帧的 [rpc])调用,不在 [gmain]。
/// SharedGpuCopy → 直接返回 ("zero_copy", false),不再调 signal_frame(fence 已由 [gmain] 推进,
/// 再推一次会让 presenter 的帧计数跳号);其余 origin = 现有函数 feed_share_frame(&out.pixels)(EH/rpc.rs:1266),
/// 分支由 pixels.imported 决定,两后端的 framePath / cpuUploads 语义相同。
pub fn feed_share(out: &FrameOut) -> Result<(&'static str, bool), String>;

/// L2 规整(01 X2):结果为紧凑 RGBA8,A 强制 255。RD 视口纹理是 R8G8B8A8_UNORM、存 sRGB 编码值(01 §3.1);
/// 00 实测的 RGB8 来自 Image 路径,保留 Rgb8 分支兜底。src_pitch 是源行距(可能含填充)。
/// 行序:Image 回读顶行在前,不复制 rurix 的 y-flip(01 §2.3 C1),L2 传 flip_y=false;L1 行序未核实(01),Stage 3 像素测试定。
pub enum SrcFormat { Rgb8, Rgba8 }
pub fn normalize_rgba8(src: &[u8], w: u32, h: u32, fmt: SrcFormat, src_pitch: usize, flip_y: bool) -> Vec<u8>;
```

§2.5 的 `SubmitState` 需要补一个控制队列:`control: VecDeque<ControlMsg>`。`SubmitBox` 的方法:`put(list, req)`、`control(msg)`、`take() -> (Vec<ControlMsg>, Option<Arc<RenderList>>)`、`complete(&Arc<FrameOut>)`(完成 `seq >= min_seq` 且尺寸相等的 waiter)、`expire(now)`(超过 deadline 的以 `RENDER_TIMEOUT:` 失败)。

**L2(CPU 紧凑 RGBA8,任何驱动都可用,包括 GLES3)**

1. `[gmain]` 按 §2.4 第 4 步发起回读:RD 走 `texture_get_data_async`,GLES3 同步 `texture_2d_get`(C3)。
2. 回读回调里 `normalize_rgba8` → `FrameOut { origin: CpuReadback, pixels.imported: false, .. }` → `sink.deliver`。
3. 消费线程 `feed_share(&out)` → 与 rurix 回读档同一分支 `share::write_frame`,返回 `("readback_upload", true)`,`cpu_uploads` 照常 +1。

**L1(GPU 写共享 buffer;仅 D3D12,且 Godot 与共享对象在同一 adapter;见 01 §3 结论 X1)**

1. 启动时 `[gmain]` 用 `RenderingDevice::get_driver_resource(LOGICAL_DEVICE / COMMAND_QUEUE, …, 0)` 取 Godot 的 `ID3D12Device` 和主队列(01 §3.2;00 §1 表 D3D12 行),读出 adapter LUID,把 AddRef 过的 device 指针交给 share 模块(新增 `share::set_device(device)`;`ID3D12Device` 可以跨线程使用)。
2. `[rpc]` `viewport.shareOpen` 仍调 `share::open(w, h, pid)`(EH/rpc.rs:1468,签名和 RPC 返回值都不变)。装了 Godot device 且其 LUID 等于缺省 adapter 的 LUID 时,`Producer::new` 用这个 device 代替 `D3D12CreateDevice(None, …)`(EH/share.rs:102);buffer / fence 布局、`DuplicateHandle`、presenter 的 `bind buf …` 全部不变(X1)。LUID 不同时照今天的做法在缺省 adapter 上建,只走 L2 的 CPU 上传档,对应 rurix 零拷贝档 LUID 对拍失败即回退(EH/viewport.rs:1348-1360)。presenter 用 `D3D12CreateDevice(None, …)`，也就是缺省 adapter（RF/crates/viewport-presenter/src/main.rs:66；01 §3.4）。所以判据就是"Godot 的 adapter LUID == 缺省 adapter 的 LUID"。双显卡下的匹配风险见 01 §9 第 2 条。
3. 走了 Godot device 时,`[rpc]` 投递 `ShareAttach(SharedTarget)`。L1 期间共享 fence 只由 `[gmain]` 推进,`Producer` 不再 Signal;否则一帧会推两次,而 presenter 把 fence 值当帧计数(presenter main.rs:9-10)。
4. 每帧在 `frame_post_draw` 里(X3):RD `texture_copy` 把视口纹理拷进中间纹理 `export_tex`(R8G8B8A8_UNORM,用 `texture_create_from_extension` 导入 RD);再在 Godot 主队列上执行我方命令列表:`CopyTextureRegion`(placed footprint,按 `row_pitch`)进共享 buffer,然后 `Signal(shared_fence, ++v)`。命令分配器 / 列表 3 个轮转,另用一个私有 fence 保证复用安全(01 §3.5 a2)。提交后 `deliver(FrameOut { origin: SharedGpuCopy, pixels.imported: true, .. })`。**注意时序**：RD 的 `texture_copy` 会录进下一帧的命令缓冲，本帧在 `swap_buffers` 时已经提交（01 §3.5 第 3 步），所以同一个回调里提交的我方 `CopyTextureRegion`，读到的是**上一帧** RD 拷贝的结果。也就是说，L1 导出的帧比渲染晚 1 帧，`FrameOut.seq` 要填上一帧的 seq（与 01 §3.5 第 3-4 步、§3.6 的延迟估计一致）。
5. 只有尺寸与 SharedTarget 一致的帧走 L1(对应 `current_import` 的尺寸判定,EH/viewport.rs:972)。WS / RPC 同时要像素时,同一帧再发一次 L2 回读。尺寸变化由 desktop 重新 shareOpen + bind 驱动,`[gmain]` 收到新的 ShareAttach 后重建 `export_tex`。
6. L1 任一步失败都只降级为 L2,不停帧(X3);资源状态跟踪要在 Stage 3 用 D3D12 debug layer 验证(01 §0)。Vulkan 与 GLES3 一律走 L2(X2),此时 `zero_copy = false`。

### 4.3 template.preview 与 Preview 通道

现状(EH/rpc.rs:1093-1099):在空场景上 `prefab::instantiate` → 改写 Animator 的 clip/time → `modelrender::bounds` 自动取景(`target = center`、`dist = (radius*2.8).max(0.1)`、可选 `yaw`)→ `viewport_size(args)` → `modelrender::render(&scene, &cam, None, w, h, true, true, None)` → 返回 `{width,height,format:"rgba8",pixelsB64,deviceName,draws,triangles,nonZeroPixels,meshFallbacks:0,truncated:false}`。函数不读 `st`;`template.preview` 不在 `READONLY_METHODS`(EH/rpc.rs:883-902),所以成功后 `scene_rev+1`(:928-930),推流腿会因此多渲一帧。这一点两后端都保持。

```rust
// crates/engine-host/src/render/preview.rs(Stage 2 新增)
/// template_preview 中 render 调用之前的部分逐字搬出(EH/rpc.rs:1094-1097,含 :1097 的 viewport_size),步骤与顺序不变。
pub struct PreviewJob { pub scene: Scene, pub cam: EditorCamera, pub width: u32, pub height: u32 }
/// 错误类型与 HResult 的 Err 相同(现有错误元组,本轮未抄录其整数类型)。
pub fn preview_job(args: &Value) -> Result<PreviewJob, HErr>;
impl PreviewJob {
    /// 与 EH/rpc.rs:1097 的八个实参一致:selected=None、readback=true、stats=true、vp=None。
    pub fn input(&self) -> FrameInput<'_>;
}
```

- rurix:`template_preview` = `preview_job(args)?` → `imm.preview(job.input())` → 原 JSON。仍在 `HS` 锁内执行,与今天相同。
- Godot:`dispatch` 在锁外分流(§4.5 第 8 项)。`preview_job` 锁外构造 → 用 `job.scene`/`job.cam` 构造无 HostState 的 `RenderSnapshot` → `extract` → `submit(Channel::Preview, list, Some(req))` → 等 `req.reply` → 成功后短持 `HS` 做 `scene_rev+1`。
- Preview 通道**不能 latest-wins**:每次请求是不同的场景,被覆盖的请求会被下一份清单的帧"完成",拿到错误的模板画面。所以 Preview 通道:`[rpc]` 侧用一把叶子锁 `PV` 串行化,同一时刻最多一个请求在途;`FrameRequest` 只接受 `seq == min_seq` 的帧(Main 通道仍是 `>=`)。
- `[gmain]` 为 Preview 单独建一个离屏视口和 scenario,尺寸与 Main 独立,所以不会出现 §2.2 第 5 点那种和推流互相触发重建的拉锯。

### 4.4 RurixBackend:1:1 包住现有实现

```rust
// crates/engine-host/src/render/rurix.rs(feature "backend-rurix",§5.2)
pub struct RurixBackend {
    info: BackendInfo,              // {Rurix, method: None, driver: None, source, godot_version: None}
    caps: Capabilities,             // §4.1 表左列;particles 在 new() 里读一次 gpu_particles::enabled()
    first_device: OnceLock<String>, // 首帧 device_name;热路径只多一次原子读,不加锁
}
impl RurixBackend { pub fn new() -> Self; }

impl ImmediateRender for RurixBackend {
    fn render(&self, i: FrameInput<'_>) -> Result<FramePixels, String> {
        let r = crate::viewport::render_scene_frame(
            i.scene, i.cam, i.selected, i.width, i.height, i.want_readback, i.want_stats, i.vp_override);
        if let Ok(f) = &r { if self.first_device.get().is_none() { let _ = self.first_device.set(f.device_name.clone()); } }
        r
    }
    fn preview(&self, i: FrameInput<'_>) -> Result<FramePixels, String> {
        crate::modelrender::render(
            i.scene, i.cam, i.selected, i.width, i.height, i.want_readback, i.want_stats, i.vp_override)
    }
}

impl RenderBackend for RurixBackend {
    fn info(&self) -> &BackendInfo { &self.info }
    fn capabilities(&self) -> &Capabilities { &self.caps }
    fn path(&self) -> FramePath<'_> { FramePath::Immediate(self) }
    fn invalidate_assets(&self) {} // asset.reload 已直接调三处 invalidate(EH/rpc.rs:1060-1061),顺序不动
    fn device_name(&self) -> Option<String> { self.first_device.get().cloned() }
}
```

逐字节不变的理由:

1. 调用链不变:`render` 用同样八个实参、在同一线程、持同样的锁调 `render_scene_frame`。RPC 腿仍在 `HS` 内,推流腿仍在锁外。
2. 包装层不碰像素,`FramePixels` 原样返回;`dyn` 分派不改变任何浮点运算。
3. `feed_share_frame`、`encode_frame`、计数写回、事件推送都不改。
4. rurix 永远不构造 `RenderList`/`RenderDelta`;推流腿的 `RenderSnapshot` 就是 EH/stream.rs:441-455 那个元组,同字段、同时机(I1)。
5. 唯一新增的运行时动作是首帧写一次 `OnceLock`,不影响输出。

### 4.5 调用点改造清单

"rurix"列写缺省后端下的改动,目标是只改调用形式、不改行为;"Godot"列写 Pipelined 后端下的行为。

| # | 调用点 | rurix | Godot |
|---|---|---|---|
| 1 | EH/main.rs:36-91 `main` | 拆成 `start_core(CoreConfig)`(§5.3)+ 主线程 accept 循环;顺序不变:建状态 → `[phys]` → `stream::spawn` → 绑 TCP → 就绪行 → `--game` | godot-host 在 GDExtension 初始化后调 `start_core`,accept 循环放到 `[accept]` 线程(§2.4 线程映射) |
| 2 | EH/stream.rs:441-455 快照 | 元组换成 `render::snapshot(&st, params, seq)`,字段和跳帧判定 `(rev, cfg_rev)` 不变 | 同左 |
| 3 | EH/stream.rs:458-466 `render_scene_frame` | `match backend().path()` 的 Immediate 分支:`r.render(snap.input())`,实参与原来八个一一相同 | Pipelined 分支:`extract(&snap)` → `submit(Main, list, None)` → `bus(Main).wait_newer(last_seq, 1 s)`;只广播尺寸等于本订阅 cfg 的帧,其余跳过 |
| 4 | EH/stream.rs:470 `feed_share_frame(&f)` | 不改 | `feed_share(&out)`,内部就是同一个函数(§4.2) |
| 5 | EH/stream.rs:472-477 计数写回 | 不改 | 不改 |
| 6 | EH/stream.rs:480 `encode_frame(frame_id, &f, playing)` | 不改 | 传 `&out.pixels`;帧头 bit2 仍读 `imported`(L1 为 true) |
| 7 | `render_loop` 的 `status` / `error` 消息 | 不改 | `deviceName` 取 `out.pixels.device_name`;`RENDER_NOT_READY:` 与其他错误一样按同因去重、1 s 限速 |
| 8 | EH/rpc.rs:906 `dispatch` | 不改(新分支的条件在 rurix 下恒假) | 在 `game.session.*` 分流(:913-924)之后、`lock(state)`(:925)之前加:`backend` 为 Pipelined 且 method ∈ {`viewport.frame`, `template.preview`} → `pipelined_frame_rpc(state, method, &params)`。`GAME_ALLOWED` 检查照搬 `handle` 的做法(:971) |
| 9 | EH/rpc.rs:1296-1394 `viewport_frame` | 只把 :1316 的 `render_scene_frame(…)` 换成 Immediate 分支调用(一行);Pipelined 走不到这里,防御性返回 `RENDER_UNSUPPORTED:` | 由第 8 项接管,见下面的三段式 |
| 10 | EH/rpc.rs:1093-1099 `template_preview` | 拆成 `preview_job` + `imm.preview(job.input())`(§4.3) | 第 8 项接管;Preview 通道,`PV` 串行化,成功后短持 `HS` 做 `scene_rev+1` |
| 11 | EH/rpc.rs:1060-1061 `asset_reload` | 末尾追加 `render::backend().invalidate_assets()`(空操作) | `control(InvalidateAssets{generation})`;`[gmain]` 丢弃旧代次建的 RS 贴图/网格 |
| 12 | EH/rpc.rs:1462-1468 `viewport_share_open` | 不改 | `share::open` 成功后 `control(ShareAttach(SharedTarget))` |
| 13 | EH/rpc.rs:1033 `viewport.shareClose` | 不改 | 先 `control(ShareDetach)` 并等 `[gmain]` 确认(上限 500 ms),再 `share::close()`,避免 `[gmain]` 往已关闭的句柄拷贝 |
| 14 | EH/rpc.rs:1439 `viewport_pick`、:2085 `pointer_to_world`、:2107 `queue_pointer`、EH/stream.rs:373 | 不改(P4,CPU 结果与后端无关) | 不改 |
| 15 | EH/rpc.rs:975 `host.ping`、:1203 `scene_summary` | 不改;`backend` 键仍是物理后端 | 不改;渲染后端只经 §4.6 暴露 |
| 16 | EH/rpc.rs:1228 `render_once` | 不改(CPU soft-raster,与渲染后端无关) | 不改 |
| 17 | EH/rpc.rs:969-1037 方法表 | 加 `render.backendInfo`、`render.capabilities`(§4.6),两者都加进 `READONLY_METHODS`(:883)和 `GAME_ALLOWED`(:938) | 同左 |
| 18 | `game.session.*`(EH/sentinels_v6.rs:123)、`sentinels_v6_render::close` | 不改 | 不改;V6 场景取帧返回 `RENDER_UNSUPPORTED: sentinels_v6`(`legs`) |
| 19 | EH/viewport.rs:942 `invalidate_assets` 等资产缓存 | 不改 | 经 §3.4 中立模块读同一份缓存 |

`pipelined_frame_rpc` 处理 `viewport.frame` 的三段式,对应 §2.4 的"短锁建请求 → 放锁等帧 → 短锁记账":

1. **短锁建请求**:`lock(state)` → `viewport_size(params)?` → 与 :1297-1303 相同的 `stream::primary_size` 让位(锁序仍是 `HS → REG`)→ 与 :1307-1313 相同的 `vp_override` → `snapshot(&st, SnapshotParams{requester: ViewportFrame, want_readback: format != "none", want_stats: true, ..}, p.next_seq())` → 放锁。
2. **放锁等帧**:`extract(&snap)?` → `submit(Main, list, Some(FrameRequest{min_seq: snap.seq, size: (w, h), deadline: now + 3 s, reply}))` → `reply.recv_timeout(3 s)`。超时返回 `RENDER_TIMEOUT:`;`!ready()` 返回 `RENDER_NOT_READY:`;都用 -32000,与 :1316 的 `DEV_ENV_DEGRADE:` 走同一个错误出口。然后 `feed_share(&out)`,此时只持 `SH`,是今天 `HS → SH` 的子集。
3. **短锁记账**:`lock(state)` → 与 :1320-1330 相同的 `frames`/`last_tris`/`last_nonzero`/`cpu_uploads` 写回和 `viewport.frame` 事件 → `format = h264` 时在锁内调 `st.h264` 编码(与今天一样在锁内,只是时长变短)→ 组装与现有完全同键的 JSON。`viewport.frame` 在 `READONLY_METHODS` 里,不 bump `scene_rev`。

语义差异(需在 §8 契约里注明):第 1 段与第 3 段之间 `HS` 是放开的,返回的像素对应第 1 段的快照,计数在第 3 段写入。rurix 下这两者是同一把锁内的同一时刻。

### 4.6 `render.backendInfo` / `render.capabilities`

传输沿用现有 4 字节 LE 长度前缀 + JSON(EH/frame.rs:9, :20)。两个方法都不接受参数(`params` 缺省或 `{}`,其余字段忽略),都是只读:加进 `READONLY_METHODS`(不 bump `scene_rev`)和 `GAME_ALLOWED`(game 模式可调)。键名沿用现有 camelCase(`pixelsB64`/`deviceName`/`nonZeroPixels`),枚举值沿用 forge.toml 的 snake_case(§7)。顶层用 `renderBackend` 而不是 `backend`,避免与 `host.ping`/`scene.summary` 里表示**物理**后端的 `backend` 混淆(§2.1)。

请求:

```json
{"jsonrpc": "2.0", "id": 7, "method": "render.backendInfo", "params": {}}
```

`render.backendInfo` 返回(下例为 rurix;注释里给 Godot 的取值):

```json
{
  "renderBackend": "rurix",   // "rurix" | "godot"
  "method": null,             // godot: "forward_plus" | "mobile" | "gl_compatibility"
  "driver": null,             // godot: "d3d12" | "vulkan" | "opengl3"
  "source": "default",        // "default" | "forge.toml" | "env" | "cli"(§7.3)
  "ready": true,              // rurix 恒 true;godot = PipelinedRender::ready()
  "deviceName": null,         // 首帧前 null;之后 = 首帧 FramePixels.device_name
  "versions": {
    "engineHost": "0.x.y",    // env!("CARGO_PKG_VERSION")
    "godot": null,            // godot: 运行时版本串(来源待 01 §4)
    "gdext": null             // godot: "0.5.5"
  }
}
```

`render.capabilities` 返回(rurix):

```json
{
  "renderBackend": "rurix",
  "pipelined": false,
  "legs": ["sprite_mesh", "model", "sentinels_v6"],
  "preview": true,
  "particles": false,
  "frameExits": { "cpuRgba8": true, "sharedD3d12": true, "zeroCopy": true },
  "stats": { "nonzero": true, "triangles": true, "truncated": true, "meshFallbacks": true, "meshClasses": true },
  "maxDraws": { "spriteMesh": 256, "model": 2048, "sentinelsV6": 1536 },
  "maxSize": { "rpc": [1920, 1080], "stream": [1280, 720] }
}
```

- `particles` 按 `FORGE_GPU_PARTICLES` 在启动时的值填(EH/gpu_particles.rs:16)。
- `maxDraws.spriteMesh` = `MAX_DRAW_SLOTS`(§1.2 记为 256);`sentinelsV6` = 1024 小槽 + 512 大槽之和。
- `maxSize.rpc` 来自 `viewport_size` 的钳制(EH/rpc.rs:1252),`maxSize.stream` 来自 `parse_cfg`(EH/stream.rs:186)。
- Godot 首版:`pipelined: true`,`legs: ["sprite_mesh", "model"]`,`particles: false`,`frameExits.zeroCopy` 按 §4.2 L1 的结果,`stats` 里 `truncated`/`meshFallbacks`/`meshClasses` 为 false,`maxDraws` 三项都是 `null`。

错误:两个方法都没有参数错误;进程内 `install` 失败(§5.3)的话宿主根本不会打出就绪行,所以不存在"查询时后端未初始化"的状态。

这两个方法是**新增**契约;§8 列出的现有契约一条都不改。

## 5. crate 拆分:lib.rs + main.rs;Cargo features;start_core

### 5.1 现状

- `RF/crates/engine-host/Cargo.toml` 没有 `[lib]`、`[[bin]]` 和 `[features]`,唯一 target 是 `src/main.rs` 的缺省 bin `engine-host`。依赖:forge-util / forge-scene / forge-logic / rurix-physics / soft-raster / `rurix-rt`(features `["vulkan"]`,注释写明运行时动态装载 vulkan-1.dll、编译期零原生依赖)/ rurix-geom-build / assetd / `naga =25.0.1`(WGSL→SPIR-V)/ `openh264 0.6` / base64 / serde / serde_json / `tungstenite 0.24` / sentinels-v6(`projects/code-sentinels/native-v6`)/ sha2;`cfg(windows)` 下还有 `windows`(D3D12 共享纹理生产者)。
- 7 个集成测试全部拉起二进制:`env!("CARGO_BIN_EXE_engine-host")` + `Command::new`,再用 `strip_prefix("FORGE_HOST_LISTENING port=")` 取端口(RF/crates/engine-host/tests/rpc_integration.rs:25-38,其余 6 个文件在 :26-44 同一写法)。所以 **bin 名 `engine-host` 和就绪行格式是测试契约**,拆分后都不能变。
- 按文件 grep `rurix_rt|rurix_[a-z]+::|\bvk::|windows::Win32|naga::|openh264|tungstenite` 的命中数:rpc.rs 10、viewport.rs 9、stream.rs 8、share.rs 5、modelrender.rs 3、character.rs 2、gpu_particles.rs 1、sentinels_v6_backend_metrics.rs 1、sentinels_v6_render.rs 1。**meshres.rs、modelrt.rs 零命中**,这印证了 §3.4 的判断:两者是纯 CPU,可以原地留在核心里。rpc.rs 与 character.rs 的命中里包括 rurix-physics,它属于核心,不受渲染 feature 影响;逐条归类放到 Stage 2 拆分 PR 里做(本轮未逐行看)。

### 5.2 拆分与 features

```toml
# crates/engine-host/Cargo.toml(提案;只列新增/改动的键)
[lib]
name = "engine_host"
path = "src/lib.rs"

[[bin]]
name = "engine-host"                  # 测试用 CARGO_BIN_EXE_engine-host,名字不能变
path = "src/main.rs"
required-features = ["backend-rurix"]

[features]
default = ["backend-rurix"]
backend-rurix = ["dep:rurix-rt", "dep:naga"]

[dependencies]
rurix-rt = { workspace = true, features = ["vulkan"], optional = true }
naga = { version = "=25.0.1", default-features = false, features = ["wgsl-in", "spv-out"], optional = true }
# 其余依赖不变:openh264(h264 两后端都要)、tungstenite(WS)、windows(共享 buffer 两后端都要)、
# sentinels-v6(游戏逻辑)、soft-raster(render.once)、rurix-physics(物理红线)都留在核心。
```

模块归属。拆法是"目录模块":`viewport.rs` 变成 `viewport/mod.rs`(中立部分的 re-export,始终编译)加 `viewport/rurix.rs`(`#[cfg(feature = "backend-rurix")]`)。这样 rpc.rs 里 `crate::viewport::EditorCamera` 这类路径在两种配置下都有效,不用改 import:

| 模块 | 归属 | 说明 |
|---|---|---|
| `rpc`、`frame`、`stream`、`share`、`anim`、`character`、`prefab`、`meshres`、`modelrt`、`material_override`、`sentinels_v6`、`sentinels_v6_clock`、`sentinels_v6_pressure` | 核心(始终编译) | `share` 内部已按 `cfg(windows)` 分支 |
| `render`(backend / snapshot / bus / sink / preview)、`render_core`(§3) | 核心 | 新增;不 `use rurix_rt` |
| `viewport/mod.rs`、`modelrender/mod.rs`、`sentinels_v6_render/mod.rs` | 核心 | 只放 §3 中立函数的 re-export,以及 `FramePixels`、`STAGED`/`scene`/`compose`/`pick`/`visual_time` 这些 CPU 部分 |
| `viewport/rurix.rs`、`modelrender/rurix.rs`、`sentinels_v6_render/rurix.rs`、`gpu_particles`、`sentinels_v6_backend_metrics`、`render/rurix.rs`(RurixBackend) | `backend-rurix` | 会话、pass、WGSL、`DeviceFrameSession`、遥测 |
| `sentinels_v6_assets`、`sentinels_v6_pages` | `backend-rurix` | 只给 GPU 上传供像素(§1.5);若 V6 的 CPU 部分也引用它们,就留在核心(Stage 2 核实) |

rpc.rs 里要加 cfg 的地方只有直接调 GPU 函数的几处:`viewport_frame` 与 `template_preview` 改走 Immediate 分支(§4.5 第 9、10 项,无 cfg);`asset_reload` 的 `modelrender::invalidate()` 与 `viewport::invalidate_assets()`(EH/rpc.rs:1060-1061)包进 `#[cfg(feature = "backend-rurix")]` 块,顺序不动;另外核心无条件地做 `ASSET_GENERATION` 的 +1(已迁到 `render_core::assets`,§3.4)。V6 的 `close`/`reset_scene`(EH/sentinels_v6_render.rs:1059, :1078)拆成"清 CPU 状态"(核心)+"清 `V6G`"(rurix 部分),rurix 下两段按原顺序连续执行。

缺省 features 下的构建产物与今天完全一样(同一 bin、同一依赖集);`default-features = false` 只供 godot-host 用,不会产出 `engine-host.exe`。

### 5.3 `start_core`:rurix bin 与 godot-host 共用的入口

```rust
// crates/engine-host/src/lib.rs(提案)
pub struct CoreConfig {
    pub port: u16,                     // parse_port 的结果(EH/main.rs:93):--port > FORGE_HOST_PORT > 17810
    pub game_scene: Option<String>,    // parse_game(EH/main.rs:123):--game > FORGE_GAME_SCENE
    pub project_root: Option<PathBuf>, // Some 时覆盖 rpc::project_root() 的 env/编译期回退(§2.3)
    pub backend: Box<dyn render::RenderBackend>,
    pub accept: AcceptMode,
}
pub enum AcceptMode {
    Inline, // rurix bin:start_core 在调用线程上进 accept 循环,永不返回(与今天一致)
    Thread, // godot-host:另起 [accept] 线程,start_core 返回 CoreHandle,进程主线程还给 Godot
}
pub struct CoreHandle {
    pub port: u16,                           // 实际绑定端口(0 = 系统分配时有用)
    pub state: Arc<Mutex<rpc::HostState>>,
    pub stream_port: Option<u16>,            // stream::spawn 失败时为 None(今天只 eprintln,EH/main.rs:52)
}
impl CoreConfig { pub fn from_args_env(backend: Box<dyn render::RenderBackend>) -> Result<Self, String>; }
/// 固定顺序(与 EH/main.rs:36-91 一致):render::install → 建 HostState → [phys](EH/main.rs:144)
/// → stream::spawn → 绑 TCP → 就绪行 → --game 引导 → accept。
pub fn start_core(cfg: CoreConfig) -> Result<CoreHandle, String>;
```

- **rurix bin**:`main()` = `start_core(CoreConfig::from_args_env(Box::new(RurixBackend::new()))?)`,`AcceptMode::Inline`。参数解析、就绪行文本、`FORGE_HOST_GAME_BOOTED` 第二行、stdout 只打协议行的规则都照搬 EH/main.rs:36-91。
- **就绪行**:两种宿主打的都是同一行 `FORGE_HOST_LISTENING port={actual_port}` 并 flush(EH/main.rs:66-67)。godot-host 进程里 gdext 横幅会先出现在 stdout(00 §2),所以**监督器必须逐行扫描前缀**(§6.2);rurix bin 的就绪行仍是首行,现有测试的读法(§5.1)不受影响。
- **project_root**:`rpc::project_root()` 回退到编译期 `CARGO_MANIFEST_DIR/../../projects/demo`(EH/rpc.rs:495-505),在 cdylib 里这个路径没有意义。godot-host 必须传 `Some(root)` 或确保 env `FORGE_PROJECT_ROOT` 存在;实现上用一个 `OnceLock<PathBuf>` 覆盖值,`project_root()` 先查它。不用 `std::env::set_var`,因为多线程下改环境变量不安全。
- **物理线程**:`[phys]` 与今天完全相同,不接 Godot 的 `physics_process`(I9)。
- **何时调用**:godot-host 的主循环是 `#[class(base=SceneTree)]`(01 §4.3:可 override MainLoop 的 `_initialize` / `_process`)。02 选择在 `initialize` 里先 `install(GodotBackend)` 再调 `start_core`,RPC 端口监听成功后打印就绪行(01 §4 结论 R1)。不开 gdext 的 `experimental-threads`,forge 线程与 `[gmain]` 之间只传纯 Rust 数据(01 §4.3),与 §2.5 的 `SubmitBox` / `FrameBus` 一致;`process` 的确切签名 01 标为未核实。

godot-host crate(Stage 3 新建):

```toml
# crates/godot-host/Cargo.toml(提案)
[package]
name = "godot-host"

[lib]
crate-type = ["cdylib"]

[dependencies]
engine-host = { path = "../engine-host", default-features = false }
godot = { version = "=0.5.5", features = ["api-4-7"] }  # 已定决策 1
```

- `GodotBackend` 在 godot-host 里实现 `RenderBackend` + `PipelinedRender`,内含 `[gmain]` 的 RS 调用、`RenderDelta` 应用、回读和 `HostFrameSink`。engine-host 核心不依赖 `godot` crate。
- 新 crate 会改写 `Cargo.lock`;`Cargo.lock` 属于用户未提交改动,Stage 3 动手前要先和用户确认(§0)。根 `Cargo.toml` 的 workspace members 是否用通配、要不要显式加 `crates/godot-host`,本轮未核实。

## 6. 启动与监督(现状 + Godot 宿主提案)

### 6.1 现状

**进程树**:desktop(Electron)只拉起 Node 宿主 `node <hostEntry>`(`@forge/host`,cwd = 仓库根,`env: process.env`,RF/apps/desktop/src/main.cjs:66-70),再以 30 s 为上限(`HEALTH_TIMEOUT_MS`,:12)轮询 HTTP 健康检查(:104-124);另外按需拉起 viewport-presenter(§8.3)。desktop **不直接**拉起 engine-host:main.cjs 与 `packages/` 下的 TS/JS 里都没有 `FORGE_HOST_LISTENING` 或 `engine-host` 字面量(本轮 grep)。

**engine-host 的监督器**是 `RF/crates/mcp/engine-scene-mcp/src/supervisor.rs`(模块文档 RF/crates/mcp/engine-scene-mcp/src/main.rs:5-6:启动即 autoStart,看门狗每 500 ms `host.ping`):

| 项 | 现状 | 出处 |
|---|---|---|
| 二进制 | env `FORGE_ENGINE_HOST_BIN` > `<workspace>/target/debug/engine-host.exe` | supervisor.rs:39-45 |
| 端口 | 先 bind `127.0.0.1:0` 取空闲端口再 drop,传 `--port <p>`(存在短暂空窗,注释说同机独占可接受) | supervisor.rs:181-186 附近 |
| 参数 / env | 只传 `--port`;env 原样继承监督器进程(没有显式 `env()`),所以 `FORGE_PROJECT_ROOT`/`FORGE_GPU_PARTICLES` 取决于谁拉起 engine-scene-mcp(本轮未追到其 MCP 配置) | supervisor.rs:187-190 附近 |
| stdio | stdin null;stdout 管道;stderr 追加写 `<workspace>/engine-host-err.log` | supervisor.rs:189-199 |
| 就绪行 | 读行线程 + channel,10 s 截止(`START_TIMEOUT`);循环里 `line.starts_with("FORGE_HOST_LISTENING")` 才算就绪,其余行跳过继续等;EOF → "就绪前退出";超时 → `child.kill()` | supervisor.rs:205-250 |
| 连接 | 就绪后 `TcpStream::connect`,读写超时 `CALL_TIMEOUT`(值未抄录) | supervisor.rs:252-256 附近 |
| 看门狗 | 500 ms `host.ping`;失败沿边记 `host.crashed`,重启成功后调一次 `scene.new {name:"restored"}` 并记 `host.restarted`,事件写 `<workspace>/data/host-events.jsonl` | supervisor.rs:111-120、:47-50 |
| 关停 | `teardown`:丢连接,`child.kill()` + `wait()`;stdin 关闭后由 main 调 `shutdown` | supervisor.rs:163-173 附近 |

**读代码发现的隐患(未运行验证)**:就绪后 `start_host` 返回,`rx` 随之 drop;读行线程下一次 `tx.send` 失败就退出(supervisor.rs:215-217),stdout 管道的读端随 `BufReader` 关闭。engine-host 今天在就绪行之后基本不再写 stdout(只有 `--game` 时的第二行,而监督器不传 `--game`),所以没出问题。Godot 进程会持续往 stdout 写日志,读端关闭后这些写入会失败;若由 Rust 侧 `println!` 触发还会 panic("failed printing to stdout")。§6.2 因此要求就绪后继续排空 stdout。

**其他启动方**:

- 集成测试直接 `Command::new(CARGO_BIN_EXE_engine-host)` 并读首行(§5.1)。
- 打包:`RF/crates/forge-agentd/src/pack.rs` 的 `build_pack(scene_abs, project_root, out_dir, engine_bin, port)`(:179-185)。它把场景闭包、`.forge/cache/rxdll/*` 和 `engine_bin` 拷进输出目录(`bin/engine-host.exe`,:225),再写 `pack-run.ps1`(`run_script`,:169-176):`$env:FORGE_PROJECT_ROOT = $root`,然后 `& "$root\bin\engine-host.exe" --port {port} --game "{scene}"`(:172)。`engine_bin` 由路由侧定位到 `target\debug\engine-host.exe`(RF/crates/forge-agentd/src/main.rs:680)。错误前缀:`PACK_SCENE_NOT_FOUND` / `PACK_ENGINE_MISSING` / `PACK_OUTDIR_CONFLICT` / `PACK_SCENE_OUTSIDE_CONTENT`。冒烟脚本:`RF/scripts/f6-w4-pack-smoke.ps1`。
- `RF/scripts/` 下与本题相关的还有 `f1-w2-viewport-smoke.ps1`、`f1-w2-desktop-presenter-smoke.ps1`、`f1-w4-h264-smoke.ps1`、`godot-fetch.ps1`(按名字判断是取 Godot 二进制,内容本轮未读)。`RF/apps/desktop/scripts/` 在本轮 glob 中没有命中任何文件。
- **env 名冲突**:`FORGE_HOST_PORT` 同时是 Node 宿主自己的 HTTP 端口(缺省 3080,RF/packages/host/src/index.ts:8-12)和 engine-host 的端口 env(EH/main.rs:93 的优先级链)。监督器用 `--port` 覆盖,所以今天没冲突;godot-host 若改用 env 传端口,必须显式设置,不能靠继承。

### 6.2 Godot 宿主提案

运行时目录与就绪行见 01 §4 结论 D1、R1,下面逐项与之一致;project.godot 的键名见 01 §4.1,启动开关见 01 §4.2。

**运行时目录**(01 D1;不用 `.pck`、不传 `--path`,模板会把 CWD 设成 exe 所在目录,01 引 main.cpp:1043-1055):

```text
<runtime>\                                 # = res://
  <模板 exe> + <对应的 console exe>          # 官方 release/debug 模板;要拿 stdout 用 console 版,且须与主 exe 同放(D1,命名对应规则未核实)
  project.godot                            # 生成,关键项见下
  forge_runtime.tscn                       # 只含一个 Node;main_scene 指向它(00 §2 第 2 条)
  forge_host.gdextension                   # entry_symbol + windows.x86_64 = "res://bin/godot_host.dll"
  .godot\extension_list.cfg                # 一行:res://forge_host.gdextension
  .godot\global_script_class_cache.cfg     # 空文件
  bin\godot_host.dll                       # crates/godot-host(cdylib)
```

下文的 `forge-godot.exe` 只是占位名,指上面的 console 版 exe;能否改名取决于 D1 里未核实的命名规则,Stage 3 实测。

project.godot 关键项:`run/main_scene`(指向生成的 tscn)、`window/size/borderless=true`、`no_focus=true`、`initial_position=Vector2i(-32000, -32000)`(00 §3,RF/docs/godot-backend/00-spike-report.md:54-99)。节拍按 01 结论 T2 固定为 `thread_model=Safe`、`vsync_mode=0`、`low_processor_mode=false`、`max_fps` = 推流目标帧率(缺省 60;00 冒烟用的 120 不再沿用,I12)。渲染键按 01 §4.1:`rendering/renderer/rendering_method`(`forward_plus`/`mobile`/`gl_compatibility`)、`rendering/rendering_device/driver.windows`(`d3d12`/`vulkan`;**Godot 缺省是 `vulkan`**,所以 forge.toml 缺省的 `d3d12` 必须显式写入)、`rendering/gl_compatibility/driver.windows`(`opengl3`),取值来自 forge.toml `[render]`(§7)。

**启动参数**:

- Godot 开关:以 project.godot 里生成的值为准,命令行 `--rendering-method` / `--rendering-driver` 只作覆盖;各开关在 release 模板上的可用性见 01 §4.2,其中明确不能用 `--headless`(会切到 headless 显示驱动,没有可出帧的窗口 / RD)。
- engine-host 核心参数一律走 env:`FORGE_HOST_PORT`(必须显式设置,见 §6.1 的冲突)、`FORGE_PROJECT_ROOT`、`FORGE_GAME_SCENE`(仅 `--game`),以及 `FORGE_RENDER_BACKEND`/`FORGE_RENDER_METHOD`/`FORGE_RENDER_DRIVER`(§7.3)。原因:Godot 进程的 argv 里混着引擎自己的开关,`parse_port`/`parse_game` 扫整个 argv 容易误伤。如果要走命令行,godot-host 应只取 `--` 之后的用户参数,取法以 01 §4 为准。

**监督器改造**(engine-scene-mcp `supervisor.rs`,Stage 3):

1. 二进制:按 §7 解析出的 backend 选 `engine-host.exe` 或 `<runtime>\forge-godot.exe`;新增 env `FORGE_GODOT_RUNTIME_DIR` 作覆盖,与 `FORGE_ENGINE_HOST_BIN` 同风格。
2. 启动超时:rurix 保持 10 s。godot 缺省 30 s:冷启动 2–5 s(00 §1),首次运行还有着色器编译(未实测)。超时 `child.kill()`;Godot 本身不起子进程,`TerminateProcess` 够用,若 01 §4 发现有子进程再改用 Job Object。
3. 就绪行:沿用现有前缀扫描(supervisor.rs:236-240 已按 `starts_with` 跳过非就绪行,gdext 横幅不会被误判)。**新增**:就绪后读行线程不退出,把 stdout 余下内容写进 `<workspace>/engine-host-out.log`,消除 §6.1 的管道隐患。
4. 进程就绪与帧就绪分开:就绪行只表示 TCP 已绑定。首帧之前,取帧类 RPC 返回 `RENDER_NOT_READY:`;监督器和 IDE 用 `render.backendInfo.ready` 轮询(§4.6)。
5. 看门狗:沿用 500 ms `host.ping`。godot 重启是秒级,连续失败时退避(1 s、2 s、4 s…上限 30 s),避免驱动故障时反复拉起。

**窗口**(00 §2 第 1 条):屏外、无边框、no_focus、永不最小化。`Main::iteration` 只在 `can_any_window_draw()` 为真时调 `RS::draw`(§2.4 C2),窗口一被最小化(比如"显示桌面")就停帧。所以 `[gmain]` 每次 `process()` 检查主窗口模式,发现最小化立即恢复,并以 1 s 限速发 `status` 告警。具体 API 以 01 §1 为准。

**打包**(`build_pack`,Stage 3/6):backend = godot 时把 `<runtime>\` 整个拷进输出目录的 `runtime\`;`pack-run.ps1` 先设 `FORGE_PROJECT_ROOT=$root`、`FORGE_HOST_PORT`、`FORGE_GAME_SCENE`,再启动 `runtime\forge-godot.exe`;新增错误前缀 `PACK_GODOT_RUNTIME_MISSING`。rurix 的打包流程逐字节不变。

## 7. forge.toml:解析器全量 + `[render]` 提案

### 7.1 解析位置与现有 schema

**唯一的解析器**是 `assetd::project::ForgeProject::load`(RF/crates/assetd/src/project.rs:50-103),用 `rurix_pkg::toml::parse`(:57)。其余位置要么调它,要么只判断文件是否存在,仓库里没有第二个 forge.toml 解析器(本轮 grep `"forge.toml"|join("forge.toml")|toml::from_str|ForgeProject::load`)。

现有 schema(缺省值照抄 :64-102 与 `with_defaults` :105-116):

| 表.键 | 类型 | 缺省 | 校验 |
|---|---|---|---|
| `[project].name` | string | `"untitled"` | 无 |
| `[project].engine-version` | string | `"0.1.0"` | 无 |
| `[project].rurix-ref` | string | `"v1.0.1-dist"` | 无 |
| `[project].entry-scene` | string | `"Content/Scenes/Main.rxscene"` | 无 |
| `[project].mode` | `"2d"` \| `"3d"` | `"3d"` | 其他值 → `PARSE_ERR`(`GameMode::parse`,:25-31) |
| `[dirs].content` | string | `"Content"` | 无 |
| `[dirs].scripts` | string | `"Content/Scripts"` | 无 |

其他规则:文件不存在 → 全缺省(:53-55);有文件但缺 `[project]` → `PARSE_ERR "forge.toml 缺 [project]"`(:58-61);**未知表和未知键一律忽略**(只 `get` 已知键),所以加 `[render]` 不会让现有解析器报错。写回:`to_toml`(:120-135)只输出 `[project]` + `[dirs]`,`save_manifest`(:137-140)整文件覆盖。它唯一的非测试调用方是项目初始化 `RF/crates/forge-agentd/src/project.rs:163`,而初始化遇到已有 forge.toml 会拒绝(`PROJECT_ALREADY_INITIALIZED`,同文件 :136-141),所以今天没有路径会抹掉用户手写的 `[render]`。

调用方(file:line):

| 位置 | 用途 |
|---|---|
| RF/crates/engine-host/src/rpc.rs:1162-1165 | `scene.new` 的 mode 缺省跟随 `[project].mode` |
| RF/crates/engine-host/src/rpc.rs:1821 | 场景加载路径取项目(失败回退 `with_defaults`) |
| RF/crates/engine-host/src/meshres.rs:84、modelrt.rs:53、prefab.rs:21 | 资产解析取 Content 根 |
| RF/crates/mcp/store-mcp/src/mcp.rs:185, :736, :764 | 失败 eprintln 后用缺省 |
| RF/crates/mcp/context-mcp/src/mcp.rs:325 | 同上 |
| RF/crates/mcp/asset-pipeline-mcp/src/main.rs:31;gen-image-mcp/src/main.rs:30;gen-model-mcp/src/main.rs:30 | 同上 |
| RF/crates/mcp/engine-scene-mcp/src/mcp.rs:36-41 | 只在 `scene_new` 工具描述里提到 mode,不解析 |
| RF/crates/forge-agentd/src/main.rs:341, :1378;memory.rs:541-545;blender.rs:974, :1104;store.rs:85;llm.rs:1304 | 取项目名、mode、Content 根 |
| RF/crates/forge-agentd/src/scope.rs:25, :80-86;project.rs:141 | 只判断存在 / 经 scope 取 mode(失败 → 3d) |
| RF/crates/forge-agentd/src/agent.rs:213-238 | 把 mode 写进 agent 提示词 |
| RF/crates/forge-index/src/extract.rs:33, :574;RF/crates/assetd/examples/model_import.rs:12 | 索引 / 示例 |
| RF/packages/client/src/components/shell/WorkspacePicker.tsx:20, :173-180, :388-390 | 经 `project/init` 写 mode;已有 forge.toml 时保留原 mode |
| RF/packages/client/src/components/shell/StatusBar.tsx:177、ChatColumn.tsx:348 | 只显示 / 注释 mode |

说明:`RF/crates/forge-agentd/src/blender.rs:671, :708` 的 `toml::from_str` 解析的是项目的 MCP 配置,不是 forge.toml;brief 候选里的 `assetd/src/lib.rs` 只在模块文档(:3)提到 forge.toml;`forge-index` 的 `store::save_manifest` 是索引清单,与 forge.toml 无关。

### 7.2 `[render]` 段提案

```toml
[render]
backend = "rurix"          # "rurix" | "godot";缺省 "rurix"
method  = "forward_plus"   # 仅 godot 生效:"forward_plus" | "mobile" | "gl_compatibility";缺省 "forward_plus"
driver  = "d3d12"          # 仅 godot 生效:"d3d12" | "vulkan" | "opengl3";缺省随 method
```

缺省与校验(全部在 `ForgeProject::load` 里做,错误码沿用 `PARSE_ERR`,消息前缀 `forge.toml [render]`):

| 规则 | 行为 |
|---|---|
| 无 `[render]` 表、或表里没有 `backend` | `backend = "rurix"`,与今天完全一致 |
| `backend` 不是两值之一 | `PARSE_ERR`(与 `mode` 的非法值同处理) |
| `backend = "rurix"` 却写了 `method`/`driver` | 接受并忽略,stderr 警告一行;`render.backendInfo` 的 `method`/`driver` 返回 null。这样改一行 `backend` 就能来回切换 |
| `backend = "godot"` 缺 `method` | `forward_plus` |
| `backend = "godot"` 缺 `driver` | `forward_plus`/`mobile` → `d3d12`;`gl_compatibility` → `opengl3` |
| 组合 | 只允许 {`forward_plus`, `mobile`} × {`d3d12`, `vulkan`} 与 `gl_compatibility` × `opengl3`,其余 `PARSE_ERR`(RD 驱动与 GLES3 不能混用,00 §1) |
| `[render]` 里的未知键 | 忽略并警告,与现有表的宽松策略一致 |
| `mode`(2d/3d)与 backend | 相互独立;godot 下 2d 走 Canvas(已定决策 2),不论 method |

```rust
// crates/assetd/src/project.rs(追加;ForgeProject 新增字段 pub render: RenderConfig)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderBackendKind { #[default] Rurix, Godot }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMethod { ForwardPlus, Mobile, GlCompatibility }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderDriver { D3d12, Vulkan, Opengl3 }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderConfig {
    pub backend: RenderBackendKind,
    pub method: Option<RenderMethod>, // rurix 恒 None;godot 恒 Some(已补缺省)
    pub driver: Option<RenderDriver>,
}
impl RenderConfig {
    pub fn parse(table: Option<&rurix_pkg::toml::Table>) -> Result<Self>; // Table 的确切类型名按 rurix_pkg 实际 API(未核实)
    pub fn as_strs(&self) -> (&'static str, Option<&'static str>, Option<&'static str>);
}
```

engine-host 的 `render::backend` 里的同名枚举(§4.1)与这里一一对应,由 `From` 转换;assetd 不依赖 engine-host。

`to_toml` 只在 `render != RenderConfig::default()` 时追加 `[render]` 段,所以缺省项目写出的 forge.toml 字节与今天相同;将来有了"设置回写",`[render]` 也不会丢。

### 7.3 优先级与传递

优先级:**Cli > Env > ForgeToml > Default**(即 §4.1 的 `ConfigSource`)。

- **Env**:`FORGE_RENDER_BACKEND` / `FORGE_RENDER_METHOD` / `FORGE_RENDER_DRIVER`,取值与 toml 相同,校验规则相同。
- **Cli**:只在 godot 进程里有意义,指 Godot 自己的 `--rendering-method`/`--rendering-driver`(§6.2)。godot-host 启动后应从 Godot 读回**实际生效**的方式与驱动,填进 `BackendInfo`;读回 API 以 01 §4 为准(未核实)。

传递链:

1. **监督器**(engine-scene-mcp,每次 `start_host`,包括看门狗重启):读项目根(env `FORGE_PROJECT_ROOT`)→ `ForgeProject::load` → 叠加 env → 选二进制(§6.2)→ 给子进程**显式**设置 `FORGE_RENDER_*` 与 `FORGE_PROJECT_ROOT`。解析失败时不启动,把 `PARSE_ERR` 原文放进 `last_start_error`,工具调用照常结构化回报(supervisor.rs:126-133 的现有通道)。
2. **rurix bin**:永远用 rurix。若读到 `backend = "godot"`(说明有人绕过监督器直接启动),只在 stderr 警告,不失败,保证集成测试与旧版 `pack-run.ps1` 不受影响(I1)。
3. **godot-host**:读 `FORGE_RENDER_*`,与 Godot 实际生效值比对;不一致时以 Godot 实际值为准,`source = "cli"`,并在 stderr 警告。
4. **打包**(`build_pack`):读同一份 `RenderConfig`。rurix → 流程不变;godot → 按 §6.2 拷运行时,并把 method/driver 写进生成的 project.godot。配置非法 → 新前缀 `PACK_RENDER_CONFIG_INVALID`。
5. **运行中修改**(I10):`[render]` 改动要重启宿主才生效。IDE 可比较 `render.backendInfo` 与 forge.toml,不一致时提示重启(Stage 7 UI;WorkspacePicker 目前只写 mode)。

## 8. 必须保持不变的 IDE 契约

两后端都必须原样满足本节;Godot 后端已知的语义差异单独列在 §8.5。传输层:TCP,4 字节 LE 长度前缀 + JSON(EH/frame.rs:9, :20),单帧上限 8 MiB(:6)。这个上限是否也作用于**响应**(1080p rgba8 的 `pixelsB64` 约 11 MB,EH/rpc.rs:1333-1334 的注释)本轮未核实,Stage 2 基线顺带确认。

### 8.1 渲染相关 JSON-RPC 方法

| 方法 | params | result | 出处 / 备注 |
|---|---|---|---|
| `viewport.frame` | `width`/`height`(缺省 960×540,钳到 16..=1920 × 16..=1080)、`format` = `"rgba8"`(缺省)\|`"h264"`\|`"none"`、`selectedId` | 公共键:`width,height,format,deviceName,draws,truncated,triangles,meshFallbacks,meshClasses,frames,nonZeroPixels,framePath,cpuUploads`;rgba8 加 `pixelsB64`;h264 加 `nalB64,keyframe`(Annex B);none 不加像素 | EH/rpc.rs:1296-1394。有推流订阅时尺寸让位主订阅者,返回的 `width/height` 是实渲尺寸(:1297-1303)。`framePath` ∈ {`no_share`,`zero_copy`,`readback_upload`}(EH/rpc.rs:1266-1280)。只读方法;推 `viewport.frame` 事件 `{frames,draws,nonZeroPixels,framePath}`。错误 -32000,消息前缀如 `DEV_ENV_DEGRADE:`、`MODEL_EMPTY:` |
| `viewport.streamInfo` | 无 | `{wsUrl: "ws://127.0.0.1:{port}/stream?token={token}", proto: 1}` | EH/rpc.rs:1284-1292;服务器未起 → 域错误,客户端回退轮询腿 |
| `viewport.setCamera` / `viewport.getCamera` | target/yaw/pitch/dist/fovY/ortho/orthoSize 子集 | 相机全量 JSON(`EditorCamera::to_json`) | EH/rpc.rs:1398 起;WS `camera` 消息复用同一解析与钳制 |
| `viewport.pick` | `x,y,width,height` | `{hit,entityId,name,point}` | EH/rpc.rs:1439-1456;始终用编辑器相机;CPU 求交(P4),与后端无关 |
| `viewport.shareOpen` | `pid`(必填,缺 → -32602)、`width`/`height` | `{texHandle,fenceHandle,width,height,format:"rgba8",handleKind:"buffer",rowPitch,bufferSize}` | EH/rpc.rs:1462-1480;句柄已 `DuplicateHandle` 进 `pid` 进程(EH/share.rs:211-225) |
| `viewport.shareClose` | 无 | `{closed: true}` | EH/rpc.rs:1033 |
| `template.preview` | 预制体参数 + `clip`/`time`/`yaw`/`width`/`height` | `{width,height,format:"rgba8",pixelsB64,deviceName,draws,triangles,nonZeroPixels,meshFallbacks:0,truncated:false}` | EH/rpc.rs:1093-1099;不在只读表,成功后 `scene_rev+1` |
| `render.once` | 无 | `{frames,tris,nonZeroPixels}` | EH/rpc.rs:1228;CPU soft-raster,与渲染后端无关 |
| `scene.summary` | 无 | 含 `render:{frames,lastTris,lastNonZeroPixels}` | EH/rpc.rs:1203 |
| `host.ping` | 无 | 含 `backend` = **物理**后端名 | EH/rpc.rs:975;渲染后端不进这个键(§4.6) |
| `asset.reload` | `guids` | JSON | EH/rpc.rs:1060;作废三类 GPU 会话,Godot 另收 `InvalidateAssets`(§4.5 第 11 项) |
| `game.session.metrics` / `game.session.pick` | V6 参数 | 含 `renderStagesByLayer`/`backendTimingsByLayer` | EH/sentinels_v6.rs:126-127;rurix 专有计时;Godot 首版不支持 V6 腿 |

新增(不影响上表):`render.backendInfo`、`render.capabilities`(§4.6)。

### 8.2 WS 推流协议(proto 1)

- 端点 `ws://127.0.0.1:{port}/stream?token={32 hex}`;token 不符 → HTTP 403(EH/stream.rs:255)。
- 握手后 5 s 内首条必须是 `subscribe`,字段 `{width,height,maxFps,selected?}`,缺省 960×540@60,钳到 16..=1280 × 16..=720、fps 1..=60(`parse_cfg` EH/stream.rs:186)。服务端回 `hello{proto:1,width,height,maxFps}`。
- 二进制帧:20 B 帧头 `"FGF1" | frameId u32 | w u16 | h u16 | flags u32 | draws u32`,后接紧凑 RGBA8;flags bit0 = play_running,bit1 = truncated,bit2 = imported(`encode_frame` EH/stream.rs:394-407)。字节序本轮未核实。
- 文本帧:每秒一条 `status{playState,deviceName,draws,truncated,fps,shareError?}`;失败时发 `error{message}`,同因去重、1 s 限速(EH/stream.rs:419 起)。
- 客户端消息:`input`、`pointer`、`camera`、`select`、`resize`、`subscribe`(`handle_client_msg` EH/stream.rs:348)。
- 节拍:Running ≤ maxFps ≤ 60,其余 ≤ 30(EH/stream.rs:517-525)。
- H.264 不走 WS:只由 `viewport.frame format=h264` 返回(`nalB64`/`keyframe`,EH/rpc.rs:1351-1372),编码器在 `HostState.h264`。

### 8.3 D3D12 共享 buffer 握手

1. desktop 拉起 `viewport-presenter.exe --hwnd <parent_hwnd_dec> x y w h`(RF/apps/desktop/src/main.cjs:315),等 presenter 就绪,10 s 超时则回退 canvas 腿(main.cjs:341-351)。
2. desktop 经 Node 宿主调 MCP 工具 `mcp__engine-scene__viewport_share_open {pid: presenter.pid, width, height}`(main.cjs:353-357)→ engine-scene-mcp → engine-host `viewport.shareOpen`(§8.1)。engine-host 用缺省 adapter 建共享线性 buffer 和共享 fence,把两个句柄 `DuplicateHandle` 进 presenter 进程(EH/share.rs:98-225)。
3. desktop 往 presenter 的 stdin 写 `bind …`(main.cjs:358),presenter `OpenSharedHandle` 两个句柄,并校验 `row_pitch` 等于按 w×h 算出的值(RF/crates/viewport-presenter/src/main.rs:119-138)。
4. 每帧:engine-host 按行距写 buffer(CPU 上传:`CopyBufferRegion` → `Signal(++v)`,EH/share.rs:258-308)或只推 fence(零拷贝档,EH/share.rs:320)。presenter 等 fence 值超过 `seen_fence`,按 `rowPitch` 的 PLACED_FOOTPRINT 拷进自己的纹理再呈现;fence 值就是帧计数(presenter main.rs:9-10)。
5. 尺寸变化:`share_close` → `share_open` → 重新 `bind`(main.cjs:372-382);`move x y w h` 只挪窗口(presenter main.rs:459-464);收尾写 `close`(main.cjs:409)。

### 8.4 已核实:bind 消息格式两端不一致

| 端 | 位置 | 实际格式 |
|---|---|---|
| 发送(desktop) | RF/apps/desktop/src/main.cjs:358(首次)、:380(改尺寸) | `` `bind ${share.handleKind === 'heap' ? 'heap' : 'tex'} ${share.texHandle} ${share.fenceHandle} ${w} ${h}` `` → **6 段**,第 2 段是 `tex`(engine-host 返回 `handleKind:"buffer"`,不等于 `'heap'`,所以落到 `tex`),**没有 rowPitch** |
| 接收(presenter) | RF/crates/viewport-presenter/src/main.rs:448-466(`parse_cmd`),:452-458 | 只接受 **7 段** `bind buf <tex> <fence> <w> <h> <row_pitch>`;:444-446 的注释写明旧的 `bind [tex\|heap]` 五/六段形态已随纹理共享退役,"收到即返回 None,由调用方如实报错,不做静默兼容" |

后果(读代码推断,未运行验证):presenter 从不 bind,desktop 的 presenter 腿不出画面;可见冒烟里轮询 `stat` 等 `presented >= 3` 的证据写不出来(main.cjs:364-367)。另外 presenter 的 `usage()` 文本(main.rs:495)还写着 `--selftest --tex <h> --fence <h> …`,与 :515 的实际解析(`--selftest --w <n> --h <n>`)不一致,属同一次退役遗留。

修复建议(Stage 3,只改 desktop,不改 presenter):

```js
// main.cjs:358 与 :380 两处
if (share.handleKind !== 'buffer') throw new Error(`unexpected handleKind ${share.handleKind}`);
presenterWrite(`bind buf ${share.texHandle} ${share.fenceHandle} ${share.width} ${share.height} ${share.rowPitch}`);
presenter.reqW = w; presenter.reqH = h;                  // :372 的改尺寸判定改用请求尺寸
presenter.texW = share.width; presenter.texH = share.height;
```

必须用 `share.width/height`:engine-host 会把尺寸钳到 1920×1080(EH/rpc.rs:1252),视口面板更大时,请求尺寸与共享 buffer 尺寸不同,presenter 的行距校验会失败。改完用 `RF/scripts/f1-w2-desktop-presenter-smoke.ps1` 回归,断言 `presented >= 3`。

### 8.5 Godot 后端对契约的已知差异(全部要写进 Stage 3 的契约测试)

- `viewport.frame`:像素对应"短锁建请求"那一刻的快照,计数在"短锁记账"时写入(§4.5);新增错误前缀 `RENDER_NOT_READY:`/`RENDER_TIMEOUT:`/`RENDER_UNSUPPORTED:`。
- V6 场景取帧返回 `RENDER_UNSUPPORTED:`;`truncated`/`meshFallbacks`/`meshClasses` 恒为 false/0(由 `render.capabilities.stats` 声明)。
- 除此之外,§8.1-§8.3 的键名、类型、取值集合、WS 帧字节格式、共享 buffer 布局(线性、行距 256 B 对齐、RGBA8、fence 每帧 +1)全部不变,presenter 零改动。

## 9. 基线:编译状态、渲染相关测试、rurix 帧哈希基线流程

### 9.1 编译状态(本轮执行,仅一次)

- 命令:在 `D:\RurixForge` 下执行 `cargo check -p engine-host --tests --locked`,默认工具链 `rustc 1.93.1 (01f6ddf75 2026-02-11)`,没有加 rust-toolchain.toml。用 `Start-Process` 把 stdout / stderr 分别重定向到 `$KIROCREW_SCRATCH\cargo-check-engine-host.{stdout,stderr}.log` 后再 grep。
- 结果:**exit 0**,`Finished dev profile [unoptimized + debuginfo] target(s) in 16.18s`。墙钟 1086 s,远大于 cargo 自报的 16 s,推测是在等构建目录锁(日志没有逐行看,未核实)。
- 警告:只有依赖有汇总行:`forge-logic` 1 条、`rurix-rt` 12 条、`assetd` 8 条(以 `warning` 开头的行共 24 行);**engine-host 自身没有警告汇总行**。
- `--locked` 下成功,说明 `Cargo.lock` 不需要改写,用户未提交的 `Cargo.lock` 没被动过。

### 9.2 渲染相关测试清单

7 个集成测试都拉起真实的 `engine-host.exe`,走长度前缀 JSON-RPC(§5.1)。"GPU"列里的"无则 SKIP"是指:没有 Vulkan 设备时 `viewport.frame` 返回 `DEV_ENV_DEGRADE`,测试打印 SKIP 后**以通过退出**。所以在无 GPU 的机器上全绿不能说明渲染正确,基线必须在有 GPU 的机器上跑,并检查输出里有没有 SKIP。

| 文件 | 测什么 | GPU |
|---|---|---|
| RF/crates/engine-host/tests/f1_viewport.rs | `viewport.frame` / `pick` / `setCamera`(G-F1-6/7/8,:1-6) | 需要;无则 SKIP |
| RF/crates/engine-host/tests/f1_zerocopy.rs | `shareOpen` 目标设为自进程,断言 `zero_copy` 档;设备缺 `VK_KHR_external_memory_win32` 时引擎退回 `readback_upload`,本测试**如实 FAIL**(:1-6) | 需要 Vulkan 设备 + 扩展;无设备则 SKIP |
| RF/crates/engine-host/tests/f1_h264.rs | `format=h264` 返回 Annex B(起始码 / SPS / PPS / IDR),以及 rgba8 档的 0 字节回归(:1-3, :107, :124-125) | 需要;无则 SKIP |
| RF/crates/engine-host/tests/f2_mesh_viewport.rs | 临时项目里落 `.rxmesh` → `FORGE_PROJECT_ROOT` 指过去 → `scene.new` + `entity.create(mesh=<GUID>)` → `viewport.frame` 断言真实网格出帧(:1-6) | 需要;无则 SKIP |
| RF/crates/engine-host/tests/stream_ws.rs | WS:坏 token 握手 403(:105);`subscribe` → `hello` → 二进制帧(有 GPU)或 `error` 文本(降级),两者必居其一;WS `input` → 逻辑帧(:1-5) | 可选:两条分支都算通过 |
| RF/crates/engine-host/tests/rpc_integration.rs | 长度前缀帧上的完整 RPC 链(:1-2, :112) | 不涉及(本轮只看了文件头) |
| RF/crates/engine-host/tests/f1_editing.rs | 跨进程保存 / 重载逐字节一致、undo/redo、checkpoint/rollback、play FSM、batchApply 原子性(:1-2, :132-246) | 不需要;是确定性红线(I9)的回归 |

其余相关:`src/viewport.rs` 的数学内联单测、`src/meshres.rs` 的解析内联单测(测试文件头注明"恒跑",f1_viewport.rs:5-6、f2_mesh_viewport.rs:6);`RF/crates/mcp/engine-scene-mcp/tests/watchdog_integration.rs`(需要先构建 engine-host.exe,:50-53)覆盖 §6 的看门狗;冒烟脚本 `RF/scripts/f1-w2-viewport-smoke.ps1`、`f1-w2-desktop-presenter-smoke.ps1`、`f1-w4-h264-smoke.ps1`、`f6-w4-pack-smoke.ps1`(内容本轮未读)。

### 9.3 rurix 帧哈希基线流程(只设计,Stage 2 第一步执行,早于任何代码改动)

流程:

1. **构建**:`cargo build -p engine-host --locked`,记下 `target\debug\engine-host.exe` 的 SHA-256、`git rev-parse HEAD` 和 `git status --porcelain` 的行数(工作树有用户改动,基线绑定的是"这一份二进制",不是提交号)。
2. **场景**:`tests\maze`、`projects\demo`、`projects\pvz`。每个目录单独起一个 engine-host,`FORGE_PROJECT_ROOT` 指向它,显式删掉 `FORGE_GPU_PARTICLES`;场景 = forge.toml 的 `entry-scene`(缺省 `Content/Scenes/Main.rxscene`,§7.1)加上 `Content/**/*.rxscene`。
3. **RPC**:`scene.load`(参数名未核实,`scene_load` 在 EH/rpc.rs:1840 附近,执行前先确认)→ 相机两档:`asLoaded`(加载后的编辑器相机,含 2d 自动正交,EH/rpc.rs:474)和 `fixed`(`viewport.setCamera`;3d 用 `{target:[0,0,0], yaw:35, pitch:-25, dist:12, fovY:60, ortho:false}`,2d 用 `{target:[0,0,0], yaw:0, pitch:0, dist:10, ortho:true, orthoSize:5}`,按 `viewport.getCamera` 的 ortho 判断)→ 每个尺寸连取两次 `viewport.frame {width, height, format:"rgba8"}`。
4. **尺寸**:960×540(缺省)、1280×720(推流上限)、320×180。不上 1080p:rgba8 的 base64 约 11 MB,可能超过 8 MiB 的帧上限(§8 开头,未核实)。全程不连 WS、不开共享,保证 `primary_size` 为空、尺寸不被改写(EH/rpc.rs:1301)。
5. **哈希**:对 `pixelsB64` 解码后的紧凑 RGBA8 做 SHA-256。同一进程两次取帧哈希相等才记 `stable: true`;编辑态没有 PIE,`[phys]` 只推进预览动画,缺省为空,所以画面应当静止。
6. **证据**:`D:\RurixForge\evidence\godot-backend\rurix-frame-baseline.json`。整个脚本跑两遍(两个进程),两遍哈希全等才算基线成立;之后任何改动都用 `-Compare` 复跑,有一条不等就 exit 1(§3.8、I1)。

```powershell
# D:\RurixForge\scripts\rurix-frame-baseline.ps1(Stage 2 创建;本轮不落盘)
#requires -Version 7   # PS 5.1 的 ConvertFrom-Json 有 2 MB 上限,960×540 的 base64 就超了
param([string]$Exe = "$PSScriptRoot\..\target\debug\engine-host.exe",
      [string]$Out = "$PSScriptRoot\..\evidence\godot-backend\rurix-frame-baseline.json",
      [string]$Compare = '', [string[]]$Projects = @('tests\maze', 'projects\demo', 'projects\pvz'))
$ErrorActionPreference = 'Stop'; $repo = (Resolve-Path "$PSScriptRoot\..").Path
$Sizes = @(@(960, 540), @(1280, 720), @(320, 180))
function Read-Exact($s, [byte[]]$b) { $o = 0; while ($o -lt $b.Length) { $k = $s.Read($b, $o, $b.Length - $o); if ($k -le 0) { throw 'EOF' }; $o += $k } }
function Invoke-Rpc($s, [string]$m, $p) {
  $body = [Text.Encoding]::UTF8.GetBytes((@{ jsonrpc = '2.0'; id = 1; method = $m; params = $p } | ConvertTo-Json -Depth 20 -Compress))
  $s.Write([BitConverter]::GetBytes([uint32]$body.Length), 0, 4); $s.Write($body, 0, $body.Length)   # 4 字节 LE(EH/frame.rs:9)
  $h = [byte[]]::new(4); Read-Exact $s $h; $buf = [byte[]]::new([BitConverter]::ToUInt32($h, 0)); Read-Exact $s $buf
  $r = [Text.Encoding]::UTF8.GetString($buf) | ConvertFrom-Json -AsHashtable
  if ($r.error) { throw "${m}: $($r.error.message)" }; $r.result
}
function Start-Host([string]$root) {
  $psi = [Diagnostics.ProcessStartInfo]::new($Exe, '--port 0'); $psi.UseShellExecute = $false; $psi.RedirectStandardOutput = $true
  $psi.Environment['FORGE_PROJECT_ROOT'] = $root; [void]$psi.Environment.Remove('FORGE_GPU_PARTICLES')
  $p = [Diagnostics.Process]::Start($psi)
  while ($null -ne ($l = $p.StandardOutput.ReadLine())) {             # 逐行扫前缀(与 supervisor.rs:237 同规则)
    if ($l.StartsWith('FORGE_HOST_LISTENING port=')) { return @{ proc = $p; port = [int]$l.Substring(26) } } }
  throw "host exited before ready: $root"
}
$entries = foreach ($proj in $Projects) {
  $h = Start-Host (Join-Path $repo $proj)
  try {
    $s = [Net.Sockets.TcpClient]::new('127.0.0.1', $h.port).GetStream()
    foreach ($scene in (Get-BaselineScenes (Join-Path $repo $proj))) {  # entry-scene + Content/**/*.rxscene(略)
      $null = Invoke-Rpc $s 'scene.load' @{ path = $scene }            # 参数名待核实
      foreach ($cam in 'asLoaded', 'fixed') {
        if ($cam -eq 'fixed') { $null = Invoke-Rpc $s 'viewport.setCamera' (Get-FixedCamera (Invoke-Rpc $s 'viewport.getCamera' @{})) }
        foreach ($wh in $Sizes) {
          $q = @{ width = $wh[0]; height = $wh[1]; format = 'rgba8' }
          $a = Invoke-Rpc $s 'viewport.frame' $q; $b = Invoke-Rpc $s 'viewport.frame' $q
          $ha = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Convert]::FromBase64String($a.pixelsB64)))
          $hb = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Convert]::FromBase64String($b.pixelsB64)))
          [ordered]@{ project = $proj; scene = $scene; camera = $cam; width = $a.width; height = $a.height; sha256 = $ha
                      stable = ($ha -eq $hb); draws = $a.draws; triangles = $a.triangles; truncated = $a.truncated
                      meshFallbacks = $a.meshFallbacks; meshClasses = $a.meshClasses; nonZeroPixels = $a.nonZeroPixels
                      deviceName = $a.deviceName; framePath = $a.framePath } } } }
  } finally { $h.proc.Kill() }
}
$doc = [ordered]@{ schema = 1; createdAt = (Get-Date).ToString('o'); exeSha256 = (Get-FileHash $Exe -Algorithm SHA256).Hash
                   gitHead = (git -C $repo rev-parse HEAD); dirtyFiles = @(git -C $repo status --porcelain).Count; entries = @($entries) }
if ($Compare) { $old = Get-Content $Compare -Raw | ConvertFrom-Json -AsHashtable
  $bad = @(for ($i = 0; $i -lt $doc.entries.Count; $i++) { if ($doc.entries[$i].sha256 -ne $old.entries[$i].sha256) { $doc.entries[$i] } })
  if ($bad.Count -or $doc.entries.Count -ne $old.entries.Count) { $bad | ConvertTo-Json -Depth 5; exit 1 }; 'baseline match'; exit 0 }
New-Item -ItemType Directory -Force (Split-Path $Out) | Out-Null; $doc | ConvertTo-Json -Depth 6 | Set-Content $Out -Encoding utf8
```

`Get-BaselineScenes` 与 `Get-FixedCamera` 这两个辅助函数按第 2、3 步实现(略)。`--port 0` 取随机端口的写法沿用 rpc_integration.rs:1 的说明,执行前以 EH/main.rs:93 `parse_port` 是否接受 0 为准。

### 9.4 留给 Stage 3 的决定

- **V6 的 `imported` 误报**(§1.5):EH/sentinels_v6_render.rs:1841 写 `imported: sprite_count > 0`,V6 会话却从不 import 共享 buffer。建议在基线之后单独提一个 PR 改成 `imported: false`,并单独审批:它会改变 WS 帧头 bit2,也会让共享 buffer 开始收到 V6 画面(CPU 上传)。像素哈希不受影响,所以 §9.3 的基线发现不了这个变化,需要额外检查帧头和 `framePath`。
- **bind 消息修复**(§8.4):只改 desktop,用 `f1-w2-desktop-presenter-smoke.ps1` 回归。

### 9.5 修订记录

**第 2 轮遗留**:第 2 轮的进度行写着"§2.4 两处行号已校正,见 §9.5",但该轮在写 §9.5 之前因 HTTP 5xx 中断,没有留下明细。本文件未纳入 git,没有历史可查,第 3 轮无法还原是哪两处;§2.4 其余 GD 引用本轮没有重新核对。

**第 3 轮按 01 结论改动**(01 写完之后,以 01 的源码证据为准):

| 位置 | 原内容 | 改为 | 依据 |
|---|---|---|---|
| §2.4 开头 | "01 §1 目前仍是「待填」占位,本节按 00 报告与源码自行设计" | 对齐 T1-T3 的说明 | 01 §1 结论 T1-T3 |
| §2.4 时序第 4 步 | GLES3 在"下一次 `process()`"同步 `texture_2d_get` | 在同一个 `frame_post_draw` 回调里同步取;L1 录制 GPU 拷贝 | 01 X2、X3 |
| §2.6 I12 | Godot `run/max_fps` ≥ 60(00 用 120) | `max_fps` = 推流目标帧率(缺省 60)、vsync 关 | 01 T2 |
| §4.1 表 `particles` | 映射"待 01 §2" | 指向 01 §5 | 01 §5 |
| §4.2 开头 | "01 §3 目前是「待填」占位" | 概述 X1-X3 | 01 §3 |
| §4.2 L1 | 适用 D3D12 / Vulkan;Godot 按 NT 句柄打开 Producer 建的 buffer;消费线程 CPU 侧推 fence;RD 能否导入外部 buffer 标"未核实" | 只适用 D3D12 且 LUID 相同;`share::open` 改在 Godot 的 device 上建 buffer / fence;Godot 主队列 `CopyTextureRegion` + `Signal`;L1 期间 `Producer` 不再 Signal;Vulkan 走 L2 | 01 X1;RD 没有导入外部 buffer 的 API(01 §3.2,`rendering_device.h:465` 只有 `texture_create_from_extension`) |
| §4.2 `SharedTarget` / `feed_share` / `normalize_rgba8` | NT 句柄 + LUID 字段;`feed_share` 恒等于 `feed_share_frame`;flip_y 待定 | COM 指针;`SharedGpuCopy` 不再推 fence;A 强制 255;L2 不翻转 | 01 X1、X2、C1 |
| §5.3 "何时调用" | 回调位置"以 01 §1 / §4 为准" | SceneTree 子类的 `initialize`;R1;不开 `experimental-threads` | 01 §4.3、R1 |
| §6.2 | "01 §4 目前是「待填」占位";exe 改名 `forge-godot.exe`;`max_fps=120`;project.godot 键名未核实 | 按 D1(console exe 与主 exe 同放,命名未核实,`forge-godot.exe` 降为占位名);T2 节拍;键名按 01 §4.1(`driver.windows` 缺省 `vulkan`,要显式写 `d3d12`);禁止 `--headless` | 01 D1、T2、§4.1、§4.2 |

**与 00 的表述差异(不算冲突)**:00 §3 的冒烟工程用 `max_fps=120`,02 按 01 T2 改用推流帧率;00 §2 第 4 条记 RD 回读为 `RGB8`,01 §3.1 记视口纹理为 `R8G8B8A8_UNORM`,两者对应不同的读取路径,02 按 01 X2 规整为 RGBA8,并保留 `Rgb8` 分支兜底。

**本轮自查修正**:§4.2 的 00 引用(§2 第 4 条 → §1 表 D3D12 行、§2 第 7 条);§4.3 的 EH/rpc.rs:1094-1096 → :1094-1097;§6.1 的 supervisor.rs:213-216 → :215-217。本轮用 grep 复核了 §1.6 / §2.5 引用的 stream.rs 行号(:441、:455、:458、:470、:472、:480),与第 1 轮一致。

**验收时父会话的修订**(2026-09-28):

| 位置 | 改动 | 依据 |
|---|---|---|
| §4.2 L1 第 2 步 | presenter 用哪个 adapter:从"本轮未读"改为"缺省 adapter" | RF/crates/viewport-presenter/src/main.rs:66 |
| §4.2 L1 第 4 步 | 补充时序说明:我方拷贝读到的是上一帧 RD 拷贝的结果,L1 晚 1 帧,`seq` 要填上一帧的 | 01 §3.5 第 3-4 步、§3.6 |
| §0 未决项 1、3 | 改为已解决,并补上读回 API 的出处 | rendering_server.h:977-980、:1041-1042;engine.h:196 |
| §9.5 表 | 旧原文引文里的占位标记改写成"「待填」占位",让机械检查不再误报 | — |


**Stage 2 实施记录**（2026-09-28，父会话维护。行号是写入时的行号，后续步骤会让它们漂移）

step 1-2（§9.3 基线、§9.1 工具链）：

| 位置 | 02 原设计 | 实际 | 依据 |
|---|---|---|---|
| §9.3 场景 | `RF/tests/maze` 当作项目 | 它不是项目目录，只有 PIE 输入矩阵 `matrix.json` / `matrix_red.json`。迷宫场景是 `projects/demo/Content/Scenes/maze.rxscene`，已包含在 demo 的 9 个场景里 | 目录实查 |
| §9.3 脚本环境 | PowerShell 7 | 本机只有 Windows PowerShell 5.1：JSON 用 System.Web.Extensions 的 `JavaScriptSerializer`（调大 MaxJsonLength），SHA-256 用 .NET Framework API | `RF/scripts/rurix-frame-baseline.ps1` |
| §9.3 未核实项 | `--port 0`、`scene.load` 参数名、帧上限 | `--port 0` 可用（开工时 EH/main.rs:93 接受 0，:66 打印实际端口）；`scene.load` 的参数是 `path`（EH/rpc.rs:1828-1834），相对路径按 workspace / 项目根两种方式解析（:1759-1790）；单帧上限 8 MiB（EH/frame.rs:5-6）；`viewport.frame` 宽 16..=1920、高 16..=1080 | 源码 |
| §9.3 尺寸切换 | 未提及 | 换尺寸后 1500 ms 内的纯尺寸重建会被押后，这期间回读字节数与新尺寸不符，直接报错（开工时 EH/viewport.rs:1884-1903、:2113-2115）。这是 rurix 现有行为，不改；脚本把尺寸放在外层循环，换尺寸前等 1.7 s，`viewport.frame` 最多重试 3 次并把重试次数记进 json（基线实际 0 次） | 实测 + 源码 |
| §9.3 规模 | — | demo 9 + pvz 57 个场景 × {加载时相机, 固定相机} × {960×540, 1280×720, 320×180} = 396 帧；两个独立进程全等，共 294 个不同哈希；`deviceName` = `Intel(R) Graphics` | `RF/evidence/godot-backend/rurix-frame-baseline.json` |
| §9.3 设备漂移 | — | 14:33 起 Vulkan loader 的 0 号设备从 Intel 核显变成 RTX 5060。rurix 取第 0 块卡，所以在 NVIDIA 上有光照的 36 帧浮点末位与基线不同，与代码无关。脚本 `-Compare` 改为按基线的 `deviceName` 查注册表 ICD，给子进程设 `VK_DRIVER_FILES`；设备不符时 exit 3，记为环境问题 | 同上脚本。Windows 上的显卡顺序本身会变，这也影响 Stage 3 的 LUID 判定（01 X1） |
| §9.1 工具链 | 钉 1.94.1 | `RF/rust-toolchain.toml` 钉 1.94.1，带 rustfmt / clippy。1.94.1 首次构建 `rurix-physics-sys`（Jolt）需要 cmake，PATH 上没有，改用 VS BuildTools 自带的 cmake 3.31.6（`$env:CMAKE`）。1.94.1 下 396/396 与 1.93.1 相同；Cargo.lock 未变 | 基线 json 的 `toolchain-1.94.1` 记录 |


step 3（§3 render_core）的偏差，已接受：
- `modelrender::pc` 与 `cube_mesh_bytes` 搬进了 render_core（P2 原写留在原处）。`collect` / `legacy_draw` 整体搬迁后会调用它们，留在原处会让 render_core 反向依赖 rurix 模块，违反 P3。
- `Draw.material` 暂不加。现在没有读者，加了会让 rurix 每帧多一次克隆，还多一条 dead_code 警告；留给实现 `extract` 时再加。
- `resolve_sprite_render` 把 project_root 改成参数只做了一半：原样搬来的 `content_guid_map_cached` 等函数仍在内部读 `rpc::project_root()`。godot-host 走 §5.3 的 `OnceLock` 覆盖，不受影响。
- 新 API（`ViewSetup`、`Leg`、`classify`）暂时没有非测试调用方，先用 `#[cfg_attr(not(test), allow(dead_code))]`。
- `ray_unit_cube` 在非测试代码里已无调用方，viewport.rs 只保留一个 `#[cfg(test)]` 的 re-export。
- 金值测试的精灵夹具改用私有副本（与 test-v5-* 逐字节相同，只换了 GUID）。原因是 `load_tex_static_cached` 的首次加载不是原子的，和现有 sprite_variants 测试同时首次加载同一张贴图时会偶发失败。

step 4+5（§4 接缝与两个 RPC）的偏差，已接受：
1. 只有空壳、测试里也构造不出来的 Godot 类型，用无条件的 `#[allow(dead_code)] // Stage 3`：`FramePath::Pipelined`、`PipelinedRender`、`ControlMsg`、`Channel`、`FrameRequest`、`FrameBus`、`FrameOut`、`SharedTarget`、`FrameSink`、`RenderList`。有测试覆盖的新 API 用 `cfg_attr(not(test), …)`。
2. 纯 Godot 的实现一律没建：`SubmitBox`、`HostFrameSink`、`feed_share`、`normalize_rgba8`、`RenderDelta`。`FrameBus` / `RenderList` 是不透明占位，`render_core::list::extract` 恒返回 `RENDER_UNSUPPORTED:`。
3. `pipelined_frame_rpc` 放在 rpc.rs，因为它要用私有的 `lock`、`GAME_ALLOWED`、`HResult`。
4. `Capabilities.particles` 在首次调用 `backend()` 时读 `FORGE_GPU_PARTICLES`。推流线程启动时就会调，实际等于启动时读。
5. 推流腿的 `seq`：Immediate 下为 0，只有 Pipelined 才调 `next_seq()`；`FramePath` 加了 `Clone + Copy`。
6. `shared_d3d12` / `zero_copy` 取 `cfg!(windows)`。
7. capabilities 的取值直接引用各腿的常量（`MAX_DRAW_SLOTS`、`MAX_DRAWS`、`FRAME_SLOTS`、`MAX_W` / `MAX_H` 改成 `pub(crate)`）；`maxSize.rpc` 是常量 [1920, 1080]，由单测对照 `viewport_size` 锁定。
8. `snapshot()` 保留两个调用点各自的 aspect 算式：推流用 `h.max(1)`，RPC 用 `h`。h ≥ 16，两式逐位相同。
9. gdext 版本用常量 `"0.5.5"`，因为 `BackendInfo` 没有这个字段；`M4` 直接从 `render_core::math` 引入。

另有两点：`deviceName` 只由 `render()`（`viewport.frame` / 推流）记录，进程里只调过 `template.preview` 时它仍是 null；非视口契约快照（template.preview 13 组 + 两种错误、host.ping、scene.load / summary，demo 与 pvz 各一套，模板在 scratch 副本里临时导入）改前改后 31/31 逐字相同。

每步的验证数字：单元测试 84/3/6（改前）→ 93/3/6（step 3）→ 102/3/6（step 4+5），3 个失败始终是改前就有的 `sentinels_v6::planning_contracts`；集成测试二进制 7 → 8（新增 f3_render_backend）；clippy 的 bin test 36、f2 1、非测试 35 始终不变；每一步的帧基线都是 396/396。step 6 与 step 7 的记录见下。

step 6（§5 crate 拆分）：`[lib] engine_host`（src/lib.rs）+ `[[bin]] engine-host`（src/main.rs，`required-features = ["backend-rurix"]`）；`[features] default = ["backend-rurix"]`、`backend-rurix = ["dep:rurix-rt", "dep:naga"]`，两个依赖改为 optional。Cargo.lock 未变（9C37B492…）。main.rs 只剩 `fn main()`：`CoreConfig::from_args_env(RurixBackend)` → `start_core(AcceptMode::Inline)`，出错时 `eprintln!("{e}")` 再 `exit(e.exit_code())`。`--no-default-features --lib` 编译 0 error 0 warning，依赖树里没有 rurix-rt / naga。

step 6 的偏差，已接受：

| 位置 | 02 原设计 | 实际 | 理由 |
|---|---|---|---|
| §5.3 错误类型 | `Result<_, String>` | `CoreError { kind, message }`；`CoreErrorKind` 多了 `BackendInstalled`、`AcceptThread` | 保留退出码 2（参数错）/ 1（其余）的区分；`message` 就是原 stderr 那一行，逐字相同 |
| §5.3 项目根覆盖 | `CoreConfig.project_root` | `rpc::PROJECT_ROOT_OVERRIDE`（`OnceLock`），在 install 之后、`HostState::new` 之前设；`project_root()` 先看它 | 不用 `env::set_var`（多线程下不安全）；bin 传 None，不设，行为不变 |
| §5.2 公共导出 | 接缝类型 | 另导出 `FrameOut`、`FrameOrigin`、`Leg`；`FrameSink` 暂不导出；`HostState` 字段保持 pub 并加 `impl Default`；`M4` 改 pub；`FrameBus` derive Default；`RurixBackend` 加 Default | 外部 crate 实现 `PipelinedRender` 必需；`f3_start_core` 以外部 crate 身份实现了三个 trait，编译通过 |
| §5.2 拆分写法 | 未定 | viewport、modelrender、sentinels_v6_render 各拆成 `<mod>.rs` + `<mod>/rurix.rs`；子模块 `use super::*`；测试留在父模块，21 处可见性升为 `pub(super)`；三处 GPU 语句挪进 `rurix::reset_renderer` / `reset_state` / `close_gpu`，父函数原位 cfg 调用 | 逐字节搬迁核对：原始行在父 + 子里全部再现，只差那 21 行可见性；父会话抽查 `invalidate_assets`，先代次 +1、再清会话的顺序不变 |
| §5.2 no-default 死代码 | 未定 | 24 处逐项 `cfg_attr(not(feature = "backend-rurix"), allow(dead_code | unused_imports))`，没有整模块压制；未 install 时 `backend()` panic `RENDER_BACKEND_NOT_INSTALLED: …`，缺省 features 下仍惰性装 RurixBackend | 核心里只给 rurix 用的函数在 no-default 下必然无人调用 |
| §5.3 `--game` 失败 | — | 原 main 持 HostState 锁直接 `exit(1)`；现在 `start_core` 返回 Err，锁先释放，再由 main `exit(1)`，其间物理线程可能多推进一步 | 不可观测：stdout / stderr / 退出码与原来相同（CLI 对照 3 例） |

step 6 留给 Stage 3 的限制：`cargo check --no-default-features --tests` 不能编译（单测和 `f3_start_core` 用了 rurix 的项，共 6 个 error；带 required-features 的 bin 在 no-default 下不构建）；`AcceptMode::Thread` 下 `--game` 或绑定失败时，已起的 phys / stream 线程没有关停接口；HostState 的 pub 字段面偏大，可以考虑做成不透明；`FrameSink` 要在 Stage 3 导出。


step 7（全量验证，父会话在 step 6 最终构建上独立重跑，exe sha256 17147308…）：

| 项目 | 结果 |
|---|---|
| `cargo test -p engine-host` | lib 单元 102 过 / 3 败 / 6 忽略（3 个仍是改前就有的 `sentinels_v6::planning_contracts`）；bin 单元 0；9 个集成二进制全绿：f1_editing 5、f1_h264 1、f1_viewport 1、f1_zerocopy 2、f2_mesh_viewport 1、f3_render_backend 2、f3_start_core 2、rpc_integration 1、stream_ws 3；doc-tests 0 |
| `cargo clippy -p engine-host` | lib 33 条。step 4+5 的 bin 是 35 条：拆成 lib 后 `EditorCamera::to_json` 的 wrong_self_convention（avoid-breaking-exported-api）和 `debug_state` 的 dead_code 不再报；新出的 `new_without_default`（HostState）已就地修掉 |
| `cargo clippy -p engine-host --tests` | lib test 35（其中 33 与非测试重复）、f2_mesh_viewport 1、两个 f3 各 0 |
| `cargo check -p engine-host --tests` / `cargo build -p engine-host` / `cargo check --no-default-features --lib` | 全部通过；engine-host 0 条 rustc 警告（step 4+5 的 1 条 `debug_state` 随上面一起消失） |
| 帧基线 | `step6-parent-verify`：396/396 match，设备自动钉到 Intel(R) Graphics |
| 非视口契约探针 | 31/31 与改前逐字相同 |
| CLI 对照（`C:\Users\wcj20\.kiro\crew\workspace\godot-backend\tools\step6\cli-contract-parent.ps1`，仓库外） | 11/11 相同：参数错 6 例 exit 2、env 警告与错误的先后、`--game` 失败 3 例 exit 1（就绪行先打印）、端口占用 exit 1。参照 exe 是 step 4+5 的构建；step 3-5 只给 main.rs 加了两行 `mod`（对照 backup ref b3b12e7），所以等价于对照原始 main |
| desktop 冒烟 | ① 离屏 smoke（`FORGE_SMOKE_SCENARIO=editor`，`RF/apps/desktop/scripts/smoke.mjs`）在 forge-agentd 未运行时如实降级为"agentd 不可达"；② 启动 forge-agentd（`FORGE_AGENTD_DATA_DIR` / `FORGE_GEN_DATA_DIR` 指向 scratch，不碰 `RF/data`）后重跑，编辑器视口出 maze 帧，层级列出 Player 和 32 面墙；③ 经 agentd `/api/forge/mcp/call`（客户端点选与 PIE 走的同一条链）：点选 5 次 4 中 1 空，PIE enter → play_running → 出帧（36 draws）→ exit → edit |

冒烟证据在 `RF/evidence/godot-backend/desktop-smoke/`（截图、smoke.log、smoke-child.log、mcp-pick-pie.txt、real-ui.txt）。19:40 又用 Computer Use 在可见的 desktop 窗口里补做了真实操作：鼠标单击视口里的墙，Inspector 切到 `Entity #2 Wall_0_1`（与 MCP 链点选结果相同）；点工具栏 Play，状态变成 `play_running`，再点 Stop 回到 `edit`。这次 Vulkan 0 号卡是 RTX 5060，视口走 WS 直连流。冒烟结束后按 PID 停掉 agentd / engine-scene-mcp / engine-host；`RF/data` 里只有 agentd 新建的 `skills-config.json`（`crates/forge-agentd/src/skills.rs:289` 不受 `FORGE_AGENTD_DATA_DIR` 约束），已删除；`apps/desktop/evidence` 里的受控文件哈希不变。注意 `RF/.gitignore:17` 忽略所有 `evidence/` 目录，基线 json 和冒烟证据都不在 git 状态里，提交时要 `git add -f`。


**Stage 3 实施记录**(2026-09-28 20:00 - 09-29,父会话维护。行号是写入时的行号。本阶段额外改了 `RF/crates/viewport-presenter/src/main.rs`,超出原定范围,用户 09-29 10:45 批准,见 step 7)

step 1(依赖变更,用户 2026-09-28 20:58 在确认卡片上回复"同意,按预演写入依赖变更"):
- 根 `Cargo.toml` 的 members 加 `crates/godot-host`;新建 `crates/godot-host/Cargo.toml`(cdylib,`godot = "=0.5.5"` 开 `api-4-7`,`engine-host` 用 `default-features = false`,其余只用工作区已有的包);`crates/mcp/engine-scene-mcp/Cargo.toml` 加 `assetd` 路径依赖(监督器要读 `[render]`,assetd 是唯一解析器)。
- Cargo.lock 253 → 266 包:新增 13 个(godot 家族 7 个 0.5.5、gdextension-api 0.5.1、glam 0.32.1、nanoserde / nanoserde-derive 0.2.1、venial 0.6.1、godot-host 自身),删除 0 个;全部来自本机 cargo 缓存,不需要联网。写入后 `cargo metadata --offline` 生成的锁 sha256 3502F2D7… 与 scratch 预演逐字节相同;godot-host 的依赖树里没有 rurix-rt / naga。
- 改动前的原件(含 145 个本阶段要碰的文件,已跟踪 / 未跟踪都算)在 `snapshots\pre-stage3\`,两次独立哈希核对 0 差异。

step 2(engine-host 侧):
- 新增 / 实现:`render/bus.rs`(`FrameBus`:`seq: AtomicU64` + `Mutex` + `Condvar`,`next_seq` / `publish` / `wait_newer`;`SubmitBox`;`FrameRequest::new`)、`render/sink.rs`(`FrameSink` 导出、`HostFrameSink`、`feed_share`、`normalize_rgba8`)、`render_core/list.rs`(`RenderList` 与 `extract`:只抽相机、清屏色、MeshRenderer 的变换与网格引用;Sprite 等跳过并计数,`Capabilities` 如实标出)、`render_core/delta.rs`(`RenderDelta`)、`rpc.rs` 的 `pipelined_frame_rpc` 三段式(短锁建请求 → 放锁等帧 → 短锁记账)、`template.preview` 的 Preview 通道、`shareOpen` / `shareClose` 的 detach / attach、`stream.rs` 的 `StreamFrame` 与 Pipelined 分支、`share.rs` 的共享 `FENCE_VALUE` 计数器与 `install_device` / `gpu_target`、`lib.rs` 的导出与 Thread 模式关停(`CORE_STOP`、`CoreHandle::shutdown`、`thread_mode_fail`)。
- 偏差,已接受:

| 位置 | 02 原设计 | 实际 | 理由 |
|---|---|---|---|
| §4.1 `ControlMsg` / `Capabilities` | 按需加字段 | 两个类型的形状不动;`coverage`、`frame_channels`、`detach_share` 做成 trait 缺省方法 | `tests/f3_start_core.rs` 对 `ControlMsg` 穷尽匹配、用字面量构造 `Capabilities`,改形状就得改现有测试文件 |
| step 6 遗留 `--no-default-features --tests` | 待修 | viewport / modelrender / sentinels_v6_render 的 rurix 测试模块、金值测试、render/mod.rs 的 3 个测试加 `cfg(feature = "backend-rurix")`;`write_sprite_variant_fixture` 移到模块级;`f3_start_core` 用 `[[test]] required-features`,测试文件本身没改 | `cargo check -p engine-host --no-default-features --tests` 0 error 0 warning |
| §4.5 帧编号 | 未细化 | `FrameChannels` 带 `l1Lag` / `l2Lag`(帧数),Pipelined 分支按 `seq` 记账 | L1 比渲染晚 1 帧(01 §3.6),要能从 `render.backendInfo` 观测 |

- step 2 验收(verify-acceptance `s3-step2`):lib 单元 113 过 / 3 败 / 6 忽略(3 个仍是改前就有的 `planning_contracts`,+11 为新单测);集成测试二进制 9 → 10(新增 `tests/f3_pipelined.rs`:假 Pipelined 后端 + 假 gmain 跑通三段式、推流和共享 buffer);clippy 条数与参照相同;帧基线 396/396、契约 31/31、CLI 11/11;Cargo.lock 未变。


step 3-4(`crates/godot-host` 与运行时 / 监督):
- godot-host:`lib.rs`(gdext 入口)、`config.rs`(读 `FORGE_HOST_PORT` / `FORGE_PROJECT_ROOT` / `FORGE_RENDER_*` / `FORGE_RENDER_SOURCE`)、`host.rs`(`#[class(base=SceneTree)]`:`initialize` 建离屏 RS 视口,再 `start_core(AcceptMode::Thread)`;`process()` 应用差量,`frame_post_draw` 导出;所有 RS / RD 调用只在主线程)、`scene.rs`(最小场景映射)、`export.rs`(L2)、`l1.rs`(L1)、`backend.rs`(`GodotBackend`:`RenderBackend` + `PipelinedRender`,`Capabilities` 按方式 / 驱动填)。Godot 实际生效的方式 / 驱动与请求不同时以实际值为准,`source = "cli"`(§7.3)。
- 运行时目录:`scripts/godot-runtime.ps1` 生成 `target/godot-runtime`(官方 release 模板改名 `forge-godot.exe` / `forge-godot_console.exe`,console 版按 "_console.exe" → ".exe" 找主 exe,并用 KILL_ON_JOB_CLOSE 的 Job 管住子进程,GD/platform/windows/console_wrapper_windows.cpp:77-131;`project.godot` 显式写 `driver.windows = d3d12`,物理 / 导航 / 音频全 Dummy;`runtime-manifest.json` 记模板与 dll 的 sha256)。注意:这个目录不会随 `cargo build` 自动刷新,dll 更新后要重跑脚本(集成测试用自己的 `target/godot-runtime-test`,会先检查 dll 不比源码旧)。
- 监督器:`resolve_render`(Env > forge.toml > Default;`[render]` 非法才拒启)、`start_godot`(console 版、env 传核心参数、就绪超时 30 s、就绪后把 stdout 排空到 `engine-host-out.log`、见到 `FORGE_GODOT_GPU_HINT` 带 `--gpu-index` 重启一次、Godot 连续失败按 1/2/4…30 s 退避),rurix 仍走原来的 `start_rurix`,逐字不变。assetd 加 `RenderConfig`(§7.2 规则)和 `ForgeProject.render`。
- 端到端时注意:agentd 给 engine-scene-mcp 注入的 `FORGE_PROJECT_ROOT` 是"默认工作区的项目根"(forge-agentd/src/mcp.rs `spawn_env` + scope.rs `project_root_of`),外面设的同名 env 会被盖掉;默认工作区根认 env `FORGE_AGENTD_WORKSPACE_ROOT`。

实测与 01 / 02 设计不同之处:

| 位置 | 设计 | 实际 | 依据 |
|---|---|---|---|
| 01 §5 纯色材质 | 着色器里 `#if CURRENT_RENDERER` 分支 | `RS::shader_set_code` 不跑预处理器,`#if` 直接报 Tokenizer 错;变体在 Rust 里按实际渲染方式选 | 实测 |
| 01 §5 颜色参数 | 传 `Color` | RD 的 MaterialStorage 对显式设置的 Color 参数一律 srgb→linear,不看 hint(material_storage.cpp:801)→ 改传 `Vector4`;GLES3 场景着色器自己把 ALBEDO 当 sRGB 转线性(scene.glsl:2398)→ Compatibility 变体直接给 sRGB 值 | 源码 + 实测 |
| 清屏色 | 23,24,29 | Mobile 量化成 22,25,28;Pipelined 的 nonZero 判定改为 ±1 容差 | 实测 |
| 01 X1 LUID 判定 | 进程内比较 Godot device 与 presenter adapter 的 LUID | Godot exe 导出 `NvOptimusEnablement`(os_windows.cpp:101),它进程里 DXGI 缺省卡是 RTX 5060,普通进程(presenter / 测试)是 Intel。godot-host 改用 rundll32 调本 dll 的 `ForgeProbeDefaultAdapter` 取"普通进程的缺省卡"(失败退 MINIMUM_POWER);共享对象一律建在消费端的卡上;卡不同就打印 `FORGE_GODOT_GPU_HINT index=i`,由监督器带 `--gpu-index` 重启 | 实测 |
| 02 §4.2 L1 中间纹理 | 初始状态 RENDER_TARGET | D3D12 debug layer 报 id=1350(legacy 创建在 RENDER_TARGET 的纹理不能与 enhanced barrier 互操作)→ enhanced 模式下改为 COMMON | debug layer |
| §6.2 就绪超时 | 10 s | Godot 宿主 30 s(实测冷启动 2.8-6.7 s);rurix 仍 10 s | 实测 |
| §6.2 pack | Stage 3 未含 | `pack.rs` 未改,留给 Stage 7 | 本阶段范围 |


step 5-6(帧通道)结果:
- L2:Forward+ / Mobile 用 `RD::texture_get_data_async`,Compatibility 用 `texture_2d_get`,统一成紧凑 RGBA8、A = 255,接到 `viewport.frame` 和 WS 流;导出失败只降级不停帧。四种配置(Forward+/D3D12、Forward+/Vulkan、Mobile/D3D12、Compatibility/OpenGL3)都出帧,同一场景连续两次取帧哈希相同。maze 320×180:36 draws、nonZero 34102,与 rurix 同帧逐像素最大差 1 LSB(F+ D3D12 与 F+ Vulkan 逐字节相同)。3 立方体场景 640×360(`scripts/f3-side-by-side.ps1`,拼图在 `RF/evidence/godot-backend/side-by-side-20260929-094252/`):5 路 draws 3、nonZero 7386,与 rurix 最大差 F+ / Mobile 1、Compatibility 2,差值 > 2 的像素为 0。
- L1(只在 D3D12 且与消费端同卡时启用):Godot 主队列上 `CopyTextureRegion` 进共享 buffer 再 `Signal`,L1 期间 `Producer` 不再 Signal;L1 帧与同帧 L2 逐字节相同;`l1Lag` = 1 帧、`l2Lag` = 1 帧。D3D12 debug layer 关卡:修掉 id=1350 之后,一轮完整 L1 取帧 0 error / 0 corruption / 1 warning(id=1356,"只含 Barrier 的命令列表",性能提示,不是我方的命令列表)→ 通过。卡不同 / Vulkan / OpenGL 时自动走 L2,原因写进 `render.backendInfo.frameChannels`。

step 7(desktop):
- `RF/apps/desktop/src/main.cjs`:bind 改成 presenter 接受的 7 段 `bind buf <buffer> <fence> <w> <h> <rowPitch>`(`presenterBind`,:256-268;首次和改尺寸两处都走它,尺寸取 `share.width/height`)。另外共享 buffer 的尺寸改成与推流订阅同式(:330-337,按 `ViewportCanvas.tsx` 的 `physW/physH`:CSS px 按宽封顶 1280 等比缩,再钳 16..1280 × 16..720):原来按设备像素建共享 buffer,dpr = 1.25 时流 538×552 ≠ 共享 673×690,engine-host 只把尺寸相同的流帧写进共享 buffer,presenter 一帧也收不到。
- `RF/scripts/f1-w2-desktop-presenter-smoke.ps1` 现在不能原样回归:要 gateway + JWT;没有后端开关;可见窗口被用户前台的最大化窗口整窗盖住时,Chromium 的原生遮挡计算会停帧(`capturePage` 报 UnknownVizError、视口 bounds 不上报、presenter 不嵌入)。于是按它的判据写了 `RF/scripts/f3-desktop-presenter-smoke.ps1`:`-Backend rurix|godot`,agentd 直连 8103,数据目录在 scratch,godot 用 `projects/demo` 副本 + `[render] backend = "godot"`(经 `FORGE_AGENTD_WORKSPACE_ROOT` 让 agentd 用它),Electron 带 `--disable-features=CalculateNativeWinOcclusion`,用 `PrintWindow(PW_RENDERFULLCONTENT)` 取 DWM 合成结果(`-OnScreen` 时置顶后截真屏幕,两者结果一致),每条判据单独给 PASS / FAIL。
- 结果(rurix 与 godot 完全相同;godot 由监督器按副本的 `[render]` 拉起 `forge-godot_console.exe` + `forge-godot.exe --gpu-index 1`,Godot 与 presenter 同在 Intel 卡上,走 L1):7 段 bind 被 presenter 接受、`framePath = zero_copy`、`presented ≥ 3`;把 presenter 子窗口提到最上面后,它显示的就是引擎帧;点选中心命中 cube-a;PIE edit → play_running → edit;Electron 退出码 0。**修 presenter 之前有两条 FAIL**,都是 viewport-presenter 的既有问题,rurix 下同样存在(修法和结果见本条最后):
  1. 用户看不到 presenter:Electron 窗口的直接子窗口 z 序(上 → 下)是 `Chrome_RenderWidgetHostHWND`、`Intermediate D3D Window`(GPU 进程)、`ForgeViewportPresenter`。presenter 在最底下,被 Chromium 的输出窗口盖住,用户看到的一直是 web 画布(WS 流)。presenter 的 `move` 用 `SWP_NOZORDER`(RF/crates/viewport-presenter/src/main.rs:406),从不把自己提上来。
  2. 不拉伸:swapchain 是 `DXGI_SCALING_NONE`(同文件 :176),共享 buffer 比子窗口小时(dpr ≠ 1)只占左上角,其余是黑的。
  - **已修(用户 2026-09-29 10:45 批准:"帮我顺手修掉吧")**,`RF/crates/viewport-presenter/src/main.rs` +44/−6:swapchain 改 `DXGI_SCALING_STRETCH`;子窗口加 `WS_DISABLED`(禁用的子窗口不收鼠标、由父窗接手,Chromium 自己的 `Intermediate D3D Window` 也是 style 0x58000000 这个做法——原来的注释以为 `WS_EX_TRANSPARENT` 能让命中测试穿透,跨进程其实不行,只是 presenter 一直压在底下才没暴露);子窗口创建时不带 `WS_VISIBLE`,首帧呈现后才 `SW_SHOWNA` 显示并提到 `HWND_TOP`,swapchain 因改尺寸重建时先隐藏、等新首帧(推流按需出帧,空闲时可能很久没帧,不这样会在视口上盖一块黑);`move` 对子窗口带 `HWND_TOP`;呈现循环约每 256 ms 查一次 `GW_HWNDPREV`,被 Chromium 压下去就提回来。`--selftest` 的独立窗口行为不变。
  - 修后回归(`f3-desktop-presenter-smoke.ps1`,判据加了第 8 项"视口鼠标输入不落在 presenter 上"):rurix、godot 各 8/8 PASS;`-Backend godot -OnScreen` 真屏幕也 8/8——视口中心像素 = 引擎帧(87,109,73),子窗口 z 序第 0 位,真实命中测试落在 `Chrome_RenderWidgetHostHWND`(Electron 主进程),不是 presenter。证据:`RF/evidence/godot-backend/desktop-presenter-godot-20260929-105416/`。`cargo clippy -p viewport-presenter` 0 条;`cargo test -p viewport-presenter` 0 个测试(该 crate 没有单测)。
  - 代价(已接受):presenter 显示时,视口里的网页叠加层(右上角帧统计、左下 edit 标签、2D 网格、提示文字)被它盖住。要恢复这些叠字,得把它们挪出视口区域或改由 presenter 自己画,留给 Stage 7。
- 8 月的 f1-w2 PASS 没能发现第 1 条:它比的是屏幕中心像素,而 presenter 下面的 web 画布显示的是同一帧;现在 2D 模式的 web 网格恰好压在中心,才暴露出来。


step 8(全量验证,2026-09-29 父会话在最终构建上跑;engine-host.exe sha256 09B42A6A…,godot_host.dll sha256 E9DFFA43…):

| 项目 | 参照值(Stage 2 结束) | Stage 3 结束 |
|---|---|---|
| `cargo test -p engine-host` | lib 102 / 3 / 6;9 个集成二进制全绿 | lib 113 / 3 / 6(3 个仍是改前就有的 `planning_contracts`,+11 为新单测);10 个集成二进制全绿(+`f3_pipelined`) |
| `cargo clippy -p engine-host` / `--tests` | lib 33;lib test 35(33 重复)、f2 1、f3_* 0 | 相同(新增的 f3_pipelined 0) |
| `cargo build -p engine-host` | 0 条 rustc 警告 | 0 条 |
| `--no-default-features --lib` / `--tests` | lib 能编译;tests 6 个 error | 两者都能编译,0 error 0 warning |
| 帧基线 / 契约 / CLI | 396/396、31/31、11/11 | 396/396(mismatch 0、unstable 0)、31/31、11/11 |
| Cargo.lock / 根 Cargo.toml | 9C37B492… / 82504788… | 3502F2D7… / 944E8514…(用户同意的依赖变更) |
| `cargo test -p godot-host` | — | 6/6:就绪行 + backendInfo + capabilities;四配置出帧、两帧哈希相同、与 rurix 最大差 1;WS 出帧;L1 = L2 逐字节 + debug layer 0 error;pick 与 rurix 相同;PIE。`cargo clippy -p godot-host --tests` 0 条 |
| `cargo test -p engine-scene-mcp` / `-p assetd` | — | 单测 3/3、godot_supervisor 1/1;watchdog_integration 1 败(改前就有:组件注册表 17 ≠ 16,与本阶段无关);assetd 单测 14(含 `RenderConfig` 2 个)+ 集成全绿 |
| 桌面冒烟 | — | 修 presenter 前 rurix / godot 各 7 项中 5 PASS、2 FAIL;修后 8 项(新增输入判据)rurix / godot / godot 真屏幕全 PASS |

新增的验证工具:`RF/scripts/f3-side-by-side.ps1`(rurix + godot 四配置同场景出帧、PNG、并排拼图、两帧哈希)、`RF/scripts/f3-desktop-presenter-smoke.ps1`(见 step 7)、`RF/scripts/godot-runtime.ps1`(运行时目录)。写脚本时踩到的 PowerShell 5.1 坑:含中文的 .ps1 必须存成带 BOM 的 UTF-8,否则按 ANSI 解析会报语法错;`$h` 与参数 `$H` 不分大小写,会互相遮蔽;`$ErrorActionPreference = 'Stop'` 下 `& taskkill … 2>&1` 会把 stderr 变成终止错误;跨进程父子窗口的等待循环必须泵消息,否则会死锁(昨晚一个旧版脚本因此卡住 2 h 40 min,已删除)。

Stage 3 留下的未决项:
1. ~~desktop presenter 的 z 序与拉伸~~:已按用户批准修掉(step 7)。遗留:presenter 显示时视口里的网页叠字被盖住,Stage 7 处理。
2. `target/godot-runtime` 不会随 `cargo build` 自动刷新;监督器不检查 dll 是否过期(测试会检查)。可以在 Stage 7 让监督器比对 `runtime-manifest.json` 里的 sha256。
3. 场景映射只覆盖相机、清屏色、MeshRenderer 的变换与网格、纯色材质;其余组件在 `Capabilities` 里如实标为未支持(Stage 4 / 6)。
4. 双显卡:Godot 进程默认选独显(NvOptimusEnablement),L1 需要一次 `--gpu-index` 重启(冷启动多 2-3 s)。


**Stage 4 实施记录**(2026-09-29 11:10 起,父会话维护。行号是写入时的行号)

step 0-1:HEAD 817a229、`git status --porcelain` 342 条(提示词背景写 342、step 0 写 341,以实测 342 为起点)、Cargo.lock / 根 Cargo.toml / engine-host.exe / godot_host.dll / viewport-presenter.exe 的 sha256 与 Stage 3 结束时相同。改前快照 `C:\Users\wcj20\.kiro\crew\workspace\godot-backend\snapshots\pre-stage4\`(49 个文件,`manifest.tsv` 记路径 / 字节数 / sha256 / git 状态,复制后逐一核对 0 差异;脚本 `tools\stage4\snapshot.ps1`)。desktop 冒烟起点:rurix / godot 各 8/8 PASS。

step 2(engine-host,`render_core` + `render` 之外只动了 lib.rs 导出、`render_core::model::default_material` 的可见性和 `sentinels_v6_render.rs` 新增的一个中立入口):
- `render_core/list.rs`:`ItemBody` 扩成 5 种(`Mesh`、`TexQuad`、`Model(ModelPrim)`、`LegacyMesh`、`LegacyQuad`);`RenderItem.pose`(骨骼矩阵);`RenderList.lights / v6 / particles`;`TexData` / `ModelData` / `ModelPrim` / `LightItem` / `LightKind` / `V6Frame` / `ParticleItem`;`MODEL_CLEAR_RGBA`、`V6_CLEAR_RGBA`;sprite_mesh 腿的贴图 quad(rurix 类 0 同判据)。
- 新 `render_core/extract3d.rs`:模型腿(与 `collect` / `legacy_draw` 同遍历、同错误文本、同一批中立函数)与 Light;新 `render_core/particles.rs`;`render_core/delta.rs` 加 `posed` / `lights` / `v6` / `particles`。
- 单测:render_core 18 → 20 过 + 1 个 ignore 的 profile;extract 出的模型矩阵作用到顶点上与 rurix `collect` 的世界空间顶点逐位相同(非蒙皮)/ 误差 ≤ 1e-5(蒙皮)。

step 2 的偏差,已接受:

| 位置 | 02 原设计 | 实际 | 理由 |
|---|---|---|---|
| §3.2 `Draw.material` | Draw 追加 `material` 字段给 Godot 腿 | 不加;extract 自己按 `collect` 的遍历建 `ModelPrim`(覆盖后的材质 + 指纹) | 不碰 rurix 的 Draw,也就不给 rurix 每帧多一次克隆 |
| §3.2 模型顶点 | extract 给世界空间顶点(与 rurix 同) | 给模型空间网格 + 实例矩阵 + 骨骼矩阵,Godot GPU 蒙皮 | step 7 的 profile:每帧 384 B vs 6.39 MB |
| §3.6 V6 | "将来要 Godot 画 V6 时再中立化" | `sentinels_v6_render.rs` 新增 `pub(crate) fn staged_frame()`:与 rurix `render` 开头同一段 CPU 流程(STAGED → visual_time → compose → interpolate),产出 `V6Frame`;rurix 的 `render` 没改 | V6 数据在全局 STAGED 里、不在 Scene 里,compose / interpolate 是模块私有 |
| 粒子解码 | 复用 `gpu_particles` | `gpu_particles` 整模块受 backend-rurix 门控,godot-host 拿不到;`render_core::particles` 按 gpu_particles.rs:16-58 同一判据另写一份,单测与 `emitter_bytes` 逐字节锁定 | 不动 rurix 模块 |
| §4.1 导出 | — | lib.rs 另导出新类型、`particles_enabled`、`default_material`,并再导出 `assetd::model::{ModelBundle, ModelMaterial, ModelPrimitive, ModelTexture}` | godot-host 建网格 / 材质要用;经 engine-host 再导出,godot-host 不加依赖,Cargo.lock 不变 |
| 预算 | — | rurix 的 MODEL_BUDGET(2048 draw、256 MiB)不用于 Godot 腿 | `Capabilities.max_draws` 本来就是 None |
| 步骤顺序 | step 2 抽齐全部 3D 内容 | V6 / 粒子的抽取与它们的映射一起在 step 8 落地 | 等子agent A 的只读分析(轴向、STAGED、门控)出来再定数据结构,避免返工 |


step 3-8(godot-host;新模块 `rid.rs`(RAII 句柄,drop 时 free_rid,带存活计数)、`mesh.rs`、`material.rs`、`light.rs`、`v6.rs`、`particles.rs`,`scene.rs` 重写;新测试 `tests/g4_mesh.rs`、`g4_material.rs`、`g4_light.rs`、`g4_camera.rs`、`g4_anim.rs`、`g4_v6.rs` 与公共模块 `tests/g4util/mod.rs`,Stage 3 的测试文件没改):

| step | 测试 | 结果 |
|---|---|---|
| 3 真网格 | g4_mesh 4 个 | 真 .rxmesh 单三角(经 asset-pipeline-mcp 导入)正反面四配置 ±3;cube 六面 F+ / Vk / Compat ≤ 2、Mobile ≤ 4;缺失引用回退 cube,draws / meshFallbacks / triangles 与 rurix 相同;asset.reload ×4 后存活 RID 句柄恒为 5 |
| 4 材质 | g4_material 3 个 | 棋盘 / 法线 ±Y / AO / emissive / unlit / MASK / materialOverrides 与 rurix 逐探针 ≤ 8;BLEND 与 doubleSided 符合 rurix;sprite_mesh 腿贴图 quad(ab_level1)平均差 0.02(F+)-0.24(Compat) |
| 5 灯光 | g4_light 3 个 | 灰卡与 Light 映射,数值见 01 §5.3 |
| 6 相机 | g4_camera 1 个 | 8 种相机 × 4 配置前景 IoU 1.00000、xor 0;两后端都丢场景相机滚转;near / far 裁剪相同 |
| 7 动画 | g4_anim 3 个 | 见 01 §5.5;PIE 两进程逐帧哈希相同 |
| 8 V6 / 粒子 | g4_v6 2 个 | 见 01 §5.6 / §5.7 |

与 01 的偏差(01 §5 已同步改写):

| 位置 | 01 原设计 | 实际 | 依据 |
|---|---|---|---|
| §5.3 缺省灯 tonemap | LINEAR | 模型腿 REINHARD(white 1000),sprite_mesh 腿 LINEAR | rurix 模型腿自己做 Reinhard;LINEAR 无法同时贴合不同角度与反照率 |
| §5.3 sprite_mesh 缺省灯 | 方向同 rurix | 方向 / energy / ambient 按三个正轴面逐面拟合 | rurix 在 sRGB 编码值上乘光照,线性 Lambert 不可能处处相等,轴向面(cube)最常见 |
| §5.3 灯参数 | 只设 energy / color / shadow | 按 Godot 节点缺省补齐(RANGE 5、SPECULAR 1.0、SHADOW_MAX_DISTANCE 100 等) | RS 缺省 SHADOW_MAX_DISTANCE 0 → 方向光阴影不画;SPECULAR 0.5 → 高光只有一半 |
| §5.3 阴影 | 直接传 castShadow | Compatibility 下关掉 | GLES3 附加 pass 各自 tonemap 后相加,Reinhard 下过曝 |
| §5.1 材质 | `.mat` → StandardMaterial3D | sprite_mesh 腿按 rurix 语义:有贴图 → 贴图 quad(ShaderMaterial,色键),无贴图 → entity_tint 纯色 | rurix 这条腿从不读 `.mat` 的 PBR 参数 |
| §5.1 绕序 | 统一 (a, c, b) | 按存的法线逐三角形定向;sprite_mesh 腿双份 + CULL_BACK | cube ±X 面几何绕序与法线相反;rurix CULL_NONE 不翻法线 |
| §5.2 过滤 | LINEAR_WITH_MIPMAPS | 按 sampler NEAREST / LINEAR,无 mip | 与 rurix sample_tex 一致 |
| §5.2 AO / 法线 scale | 直接映射 | 烘进贴图 | 语义不同,见 01 §5.2 |
| §5.5 选中高亮 | — | 模型腿用 `instance_geometry_set_material_overlay` 叠一层 25% 橙色(RD 在线性 HDR 里混合 = tonemap 前的 mix) | rurix FS 的 `mix(color, (1, 0.4, 0.06), 0.25)` |
| §5.7 V6 | 平移 × 缩放 + 标准相机 | iso 烘进实例变换 + 朝 −Z 正交相机;terrain = 顶点色网格 | rurix 的等距投影是镜像且两轴缩放不等 |
| §5.6 粒子 | particles_* + 样式预设 | 一种径向爆开的基本映射,样式留 Stage 5 | 按阶段划分 |



step 9 验证(收尾,2026-09-29 19:22 起):

- 环境变化:
  - 约 18:08 `D:\RurixForge\target` 整个目录被删了,原因不明;源码、evidence、snapshots 都还在。19:22 冷构建 `cargo build --workspace --locked`(1 分 28 秒),然后重跑 `scripts\godot-runtime.ps1`。
  - 重编后 engine-host.exe 是 4869342D…,godot_host.dll 是 65A2C711…。源码没变,哈希变化来自重新链接;rurix 的行为以帧基线对照为准。
  - 本机 WMI 从中午起一直卡死,`Get-CimInstance` 和 `Get-NetTCPConnection` 都会无限等待。`scripts\f4-run-nowmi.ps1` 在调用方作用域里定义了两个同名函数替身,被包装的脚本一行不改:
    - `Get-CimInstance Win32_Process`:用 Toolhelp32 快照实现。
    - `Get-NetTCPConnection -State Listen`:用 GetExtendedTcpTable 实现,覆盖 IPv4 和 IPv6。本机实测 68 个监听,与 netstat 数目相同。
  - `scripts\f4-scene-matrix.ps1` 修了一处 bug:`scene.load` 的 path 直接取 `Join-Path` 的输出,那是 PSObject 包装,JavaScriptSerializer 反射它时报循环引用,改成 `[string](...)`。此前的矩阵从没跑到这一步。
- godot-host 全量测试(最终代码、冷构建):
  - g4 17/17 通过:anim 3、camera 1、light 3、material 3、mesh 4、stability 1、v6 2。
  - g3 5/6。`ready_line_backend_info_and_capabilities` 仍断言 legs == ["sprite_mesh"],Stage 4 开了三条腿,所以按设计失败。约束是"现有测试文件不改",这里保留原样。
- 66 场景 × 4 配置:证据在 `evidence\godot-backend\stage4\scene-matrix-20260929-193551\`,320×180,用时 75 s。
  - 264 帧全部出帧,每帧两次取帧的 sha256 都相同,exit 0。
  - 8 个 3D 场景在四种配置下 draws 都与 rurix 相同:demo 的 Main、ab_level1-3、journey、maze、pz_level1、pz_mvp_phase1。
  - 另外 58 个是空帧,正好是只含 Sprite 的 2D 场景:pvz 的 57 个加 demo 的 anim_demo。其中没有一个含 3D 组件。`render.capabilities.coverage.skipped` 已写明 Sprite 属于 Stage 6。
  - rurix 参照:66/66 出帧,0 空帧。

- 多场景并排:脚本 `scripts\f4-side-by-side.ps1` 是新写的,不用 WMI。证据在 `evidence\godot-backend\stage4\side-by-side-20260929-194857\`,640×360;每个场景一张 3×2 拼图,再加 ×4 的差值图。
  - 场景用的是 `crates\godot-host\tests\fixtures\stage4\scenes\*.json` 夹具。每个 (配置, 场景) 起一个新宿主。
  - 五路全部出帧,两次取帧哈希相同。
  - 平均差按全帧 RGB 平均绝对差算,口径与 g4util::stats 相同。

| 场景 | 内容 | 目标(平均差 ≤) | F+ D3D12 | F+ Vulkan | Mobile | Compat |
|---|---|---|---|---|---|---|
| maze | 缺省灯,36 个内置 cube | 1.0 | 0.365(最大 114) | 0.365 | 0.840 | 0.352(最大 123) |
| graycard_model | 缺省灯,模型腿灰卡 | 1.0 | 0.320(最大 2) | 0.320 | **2.047**(最大 3) | 0.320 |
| graycard_mesh | 缺省灯,sprite_mesh 腿,cube 任意朝向 | 2.0 | 1.237(最大 17) | 1.237 | 1.994 | 1.263 |
| materials | 模型 / 材质 | — | 1.467(最大 70) | 1.467 | 3.452 | 1.640 |
| anim | GPU 蒙皮 | — | 0.071(最大 2) | 0.071 | 2.173 | 0.072 |
| lights | 方向光 + 点光 + 聚光(rurix 不读 Light) | — | 16.5 | 16.5 | 17.5 | 16.6 |
| pz_mvp | 含 2 盏方向光(同上) | — | 0.691 | 0.691 | 1.685 | 0.693 |

- 缺省灯目标:12 格里 11 格达标。
  - 没达标的一格是 Mobile 的 graycard_model。背景清屏色 (9,11,15) 在 Mobile 的 RGB10A2 3D 缓冲里量化成 (6,13,13),每个背景像素差 2-3,全帧平均差因此抬到 2.047。
  - 只算前景(rurix 帧里与清屏色不同的像素)时,平均差是 1.232,与另外三种配置完全相同。
  - 目标是在测量前定的,没有事后放宽。
  - Mobile 另外几个场景的全帧平均差偏高(materials 3.45、anim 2.17),也是同一个原因。
- 新发现的差异。g4 测试没有覆盖到这几项,已同步到 01 §5.2:
  - metallic = 1 的面偏暗:mats.metal(base 0.9、rough 0.3)rurix 是 97,godot 四种配置都是 30-31。rurix 的环境项 base × 0.14 × ao 对金属照样加(`modelrender\rurix.rs:72`);Godot 的 PBR 金属没有漫反射环境项,在 COLOR 环境光下也没有反射源可取。这属于环境 / 反射,移交 Stage 5。
  - Compatibility 下的 BLEND(mats.blend,红色、α 0.5):rurix 111,Compat 81,F+ 和 Mobile 121。这是已知问题"Compatibility 在 sRGB 帧缓冲里混合"。
  - maze 有 2-3 个孤立像素差到 114-123,差值图里只看得到两三个白点;其余像素的差都 ≤ 2。
  - lights 和 pz_mvp 两边的几何覆盖完全相同(nonZero 相等),差值全部来自 Light 的着色:rurix 不读 Light(01 §5.3),godot 按 Light 真画。这是设计如此,两个场景只作对照。
- desktop:`tools\stage4\run-smokes.ps1 -NoWmi` 经 f4-run-nowmi.ps1 跑,rurix 8/8 PASS,godot 8/8 PASS。结果在 `C:\Users\wcj20\.kiro\crew\workspace\godot-backend\stage4\smoke-final\`。

- rurix 验收 s4-final:verify-acceptance 与 CLI 对照,20:03 跑完。

| 项 | Stage 4 参照 | s4-final |
|---|---|---|
| engine-host lib 测试 | 118 过 / 3 败 / 7 忽略 | 118 / 3 / 7。3 个失败仍是改前就有的 `sentinels_v6::planning_contracts` |
| 集成测试二进制 | 10 个全绿 | 10 个全绿 |
| clippy:lib / --tests / f2_mesh_viewport | 33 / 35(33 条重复)/ 1 | 相同 |
| engine-host 的 build 警告;no-default | 0;能编译 | 0;能编译 |
| 帧基线 / 契约 / CLI | 396/396 / 31/31 / 11/11 | 396/396(mismatches 0、unstable 0)/ 31/31(diff 0)/ 11/11 |
| Cargo.lock / 根 Cargo.toml | 3502F2D7… / 944E8514… | 相同 |
| engine-host.exe | 211AF84A… | A66B7049… |
| godot_host.dll | — | 65A2C711… |

engine-host.exe 的哈希与 Stage 4 不同,原因有两层:
- A66B7049… 是 `cargo build -p engine-host` 的产物,Stage 4 的 211AF84A… 也是这样构建的,差别来自 target 被删后的冷构建。
- 按 workspace 构建会得到另一个哈希 4869342D…,因为各包的 feature 合并方式不同。矩阵和第一轮并排用的就是这一版。之后换 A66B7049… 重跑了一次并排(`side-by-side-20260929-200432`),35 帧的哈希与上一轮 194857 逐帧相同。


**Stage 5 实施记录**(2026-09-29 20:40 起,父会话维护。行号是写入时的行号)

step 0-1:
- HEAD 817a229,`git status --porcelain` 344 条。Cargo.lock、根 Cargo.toml、engine-host.exe(A66B7049…)、godot_host.dll(65A2C711…)与 Stage 4 收尾相同。
- 改前快照:`C:\Users\wcj20\.kiro\crew\workspace\godot-backend\snapshots\pre-stage5\`,76 个文件,`manifest.tsv` 记路径 / 字节数 / sha256 / git 状态,复制后逐一核对 0 差异(脚本 `tools\stage5\snapshot.ps1`)。
- `cargo test -p godot-host` 起点:g4 17/17;g3 5/6,失败的仍是 legs 断言,与 Stage 4 相同。
- 桌面冒烟起点没能跑。20:50 和 20:57 有人在本仓库起了开发栈:`pnpm --filter @forge/host dev` 占 3080,Git Bash 里的 `forge-agentd.exe` 占 8103,另有 engine-scene-mcp → engine-host;21:03 起还在改 `packages\client` 的聊天界面。冒烟脚本的端口检查直接 FAIL。这些进程不是本会话起的,没有停。起点沿用 Stage 4 收尾的结果(同一批二进制,两后端各 8/8)。
- 这批进程直接从 `target\debug` 运行 engine-host.exe,映像被占用时 cargo 无法重新链接。需要重编时先跑 `tools\stage5\unlock-exe.ps1`,把在用文件的全部硬链接改名成 `*.inuse-<时间>`;运行中的进程继续用旧映像,不受影响。
- 本会话 Stage 4 遗留的 `stage4\tmp-b\engine-host-s4b.exe`(12:17 起)已按 PID 停掉。

step 2 schema 改动清单(改 forge-scene 之前写入;即 01 §6.3 的定稿)。原则:
- 只往 REGISTRY 加组件,字段全部可选,缺省 = Godot 4.7.2 缺省(从 `GD/doc/classes/*.xml` 实抄,见下表)。rurix 不读这些组件。
- 已有组件一个字段都不加。normalize_props 会给每个经 RPC / MCP 新建的组件补全量缺省,给 Light 加字段,rurix 的 entity.get / scene.save 输出就会变。所以 01 §6.3 的"Light 扩展字段"改成独立组件 LightParams,挂在同一个 Light 实体上。
- 字段一律扁平的 camelCase。FieldSpec 只校验顶层字段,嵌套对象只能声明成 dict,内部既不校验也不补缺省。所以 01 §6.3 里 `glow{…}`、`ssao{…}` 这类写法都拆成 `glowEnabled`、`ssaoRadius` 这样带前缀的字段。
- 新组件的颜色字段与 Godot 一样是 sRGB 编码值;Light.color 仍按 Stage 4 是线性值。
- 场景级组件(Environment、CameraAttributes、RenderSettings)可以挂在任意实体上,取场景实体序里第一个启用的(规则同 Camera),不用实体变换。实体级组件(ReflectionProbe、Decal、FogVolume)的位置取 `modelrt::entity_world`,走 Parent 链。
- VoxelGI 和 Compositor 不进 schema。VoxelGI 只能经节点烘焙(01 §6.1);Compositor 是 RD 的扩展接口,forge 里没有对应语义。


| 组件 | 字段 = 缺省 | 映射(godot-host) |
|---|---|---|
| Environment · 背景 / 天空 / 环境光 / tonemap | background ∈ clearColor\|color\|sky = clearColor(clearColor = 当前腿的清屏色,即没有 Environment 时的背景);backgroundColor [0,0,0,1];backgroundEnergy 1;skyType ∈ procedural\|physical\|panorama = procedural;skyTexture ""(panorama 贴图 guid);skyEnergy 1;skyRotation [0,0,0];ambientSource ∈ bg\|disabled\|color\|sky = bg;ambientColor [0,0,0,1];ambientEnergy 1;ambientSkyContribution 1;reflectionSource ∈ bg\|disabled\|sky = bg;tonemap ∈ linear\|reinhard\|filmic\|aces\|agx = linear;exposure 1;white 1;agxContrast 1.25;agxWhite 16.29 | `Environment` 资源(按 Godot 属性名 `Object::set`),`scenario_set_environment` |
| Environment · glow | glowEnabled false;glowLevel1…7 = 0 / 0.8 / 0.4 / 0.1 / 0 / 0 / 0;glowNormalized false;glowIntensity 0.3;glowStrength 1;glowMix 0.05;glowBloom 0;glowBlendMode ∈ additive\|screen\|softlight\|replace\|mix = screen;glowHdrThreshold 1;glowHdrScale 2;glowHdrLuminanceCap 12 | 同上 |
| Environment · SSAO / SSIL / SSR | ssaoEnabled false、ssaoRadius 1、ssaoIntensity 2、ssaoPower 1.5、ssaoDetail 0.5、ssaoHorizon 0.06、ssaoSharpness 0.98、ssaoLightAffect 0、ssaoAoChannelAffect 0;ssilEnabled false、ssilRadius 5、ssilIntensity 1、ssilSharpness 0.98、ssilNormalRejection 1;ssrEnabled false、ssrMaxSteps 64、ssrFadeIn 0.15、ssrFadeOut 2、ssrDepthTolerance 0.5 | 同上(RS 的 `environment_set_ssil` 没绑定给 GDExtension,只能经资源设) |
| Environment · SDFGI | sdfgiEnabled false、sdfgiCascades 4、sdfgiMinCellSize 0.2、sdfgiYScale ∈ 50%\|75%\|100% = 75%、sdfgiUseOcclusion false、sdfgiBounceFeedback 0.5、sdfgiReadSkyLight true、sdfgiEnergy 1、sdfgiNormalBias 1.1、sdfgiProbeBias 1.1 | 同上 |
| Environment · 雾 | fogEnabled false、fogMode ∈ exponential\|depth = exponential、fogLightColor [0.518,0.553,0.608,1]、fogLightEnergy 1、fogSunScatter 0、fogDensity 0.01、fogAerialPerspective 0、fogSkyAffect 1、fogHeight 0、fogHeightDensity 0、fogDepthCurve 1、fogDepthBegin 10、fogDepthEnd 100 | 同上 |
| Environment · 体积雾 | volumetricFogEnabled false、…Density 0.05、…Albedo [1,1,1,1]、…Emission [0,0,0,1]、…EmissionEnergy 1、…Anisotropy 0.2、…Length 64、…DetailSpread 2、…GiInject 1、…AmbientInject 0、…SkyAffect 1、…TemporalReprojection true、…TemporalReprojectionAmount 0.9(字段名都以 volumetricFog 开头) | 同上 |
| Environment · adjustment | adjustmentEnabled false、adjustmentBrightness 1、adjustmentContrast 1、adjustmentSaturation 1、adjustmentColorCorrection ""(LUT 贴图 guid;高 1 像素 = 1D LUT) | 同上 |
| CameraAttributes | exposureMultiplier 1、exposureSensitivity 100、autoExposureEnabled false、autoExposureScale 0.4、autoExposureSpeed 0.5、autoExposureMinSensitivity 0、autoExposureMaxSensitivity 800、dofBlurFarEnabled false、dofBlurFarDistance 10、dofBlurFarTransition 5、dofBlurNearEnabled false、dofBlurNearDistance 2、dofBlurNearTransition 1、dofBlurAmount 0.1 | `CameraAttributesPractical` 资源,`scenario_set_camera_attributes` |
| RenderSettings | 视口级:msaa3d ∈ disabled\|2x\|4x\|8x = disabled、screenSpaceAA ∈ disabled\|fxaa\|smaa = disabled、taa false、debanding false、scaling3dMode ∈ bilinear\|fsr\|fsr2 = bilinear、scaling3dScale 1、fsrSharpness 0.2。RS 全局:ssaoQuality / ssilQuality ∈ veryLow\|low\|medium\|high\|ultra = medium、sdfgiRayCount ∈ 4\|8\|16\|32\|64\|96\|128 = 8、volumetricFogVolumeSize 64、volumetricFogVolumeDepth 64(缺省 = ProjectSettings 缺省) | `viewport_set_*`;全局项只跟 Main 通道,组件消失时恢复 ProjectSettings 值 |
| ReflectionProbe | size [20,20,20]、originOffset [0,0,0]、intensity 1、blendDistance 1、maxDistance 0、updateMode ∈ once\|always = once、boxProjection false、interior false、enableShadows false、ambientMode ∈ disabled\|environment\|color = environment、ambientColor [0,0,0,1]、ambientColorEnergy 1、cullMask 1048575、reflectionMask 1048575、meshLodThreshold 1 | `reflection_probe_*` + instance |
| Decal | size [2,2,2]、textureAlbedo / textureNormal / textureOrm / textureEmission ""(贴图 guid)、emissionEnergy 1、modulate [1,1,1,1]、albedoMix 1、normalFade 0、upperFade 0.3、lowerFade 0.3、distanceFadeEnabled false、distanceFadeBegin 40、distanceFadeLength 10、cullMask 1048575 | `decal_*` + instance(沿实体 −Y 投射) |
| FogVolume | shape ∈ ellipsoid\|cone\|cylinder\|box\|world = box、size [2,2,2]、density 1、albedo [1,1,1,1]、emission [0,0,0,1]、heightFalloff 0、edgeFade 0.1 | `fog_volume_*` + `FogMaterial` 资源 |
| LightParams(挂在 Light 实体上) | range 5、attenuation 1、spotAngle 45、spotAttenuation 1、specular 1、indirectEnergy 1、volumetricFogEnergy 1、size 0、negative false、shadowBias −1、shadowNormalBias −1、shadowBlur 1、shadowOpacity 1、shadowMaxDistance 100、directionalShadowMode ∈ orthogonal\|parallel2Splits\|parallel4Splits = parallel4Splits、omniShadowMode ∈ dualParaboloid\|cube = dualParaboloid | `light_set_param` 等。−1 = 该灯种的 Godot 节点缺省(bias 0.1、spot 0.03;normal bias 方向光 2.0、omni / spot 1.0)。specular 缺省取 1.0,与 Stage 4 相同(OmniLight3D / SpotLight3D 的节点缺省是 0.5);omniShadowMode 缺省取 dualParaboloid,也是 Stage 4 的实际值(Stage 4 没设,用的是 RS 缺省,`light_storage.h:77`;OmniLight3D 节点缺省是 cube)。所以"挂一个全缺省的 LightParams"与不挂完全相同 |

计数与测试:
- 注册表 17 → 24(新增 Environment、CameraAttributes、RenderSettings、ReflectionProbe、Decal、FogVolume、LightParams)。
- forge-scene 自己的 `registry_lists_legacy_and_model_types_with_fields` 断言 `len == 17`。它是注册表的变更检测,随注册表一起改成 24 并补上 7 个名字。这超出了"只加组件"的字面范围,记为偏差:不改的话,一个现在通过的测试会变红。
- engine-scene-mcp 的 `watchdog_integration`(tests/watchdog_integration.rs:188)断言 16。Stage 5 之前就是 17 ≠ 16 失败,之后变成 24 ≠ 16。按提示词不改,只在这里记录。
- 帧基线、契约、CLI 三项改前改后都必须不变(契约探针不调 component.listTypes)。

step 2 验证(s5-step2,21:43,注册表改完之后):
- engine-host lib 118 过 / 3 败 / 7 忽略,3 个失败仍是 `sentinels_v6::planning_contracts`;10 个集成测试二进制全绿。
- clippy:lib 33;`--tests` 时 lib test 35(33 条重复)、f2_mesh_viewport 1、各 f3_* 0。engine-host build 警告 0;`--no-default-features --lib` 能编译。
- 帧基线 396/396(mismatches 0、unstable 0),契约 31/31(diff 0),CLI 11/11。`cargo test -p forge-scene` 17/17。
- engine-host.exe 变成 06EE9EA9…,原因是 forge-scene 变了;rurix 的行为以上面三项对照为准。
- 验证之后把 LightParams.omniShadowMode 的缺省从 cube 改成 dualParaboloid(理由见上表),只影响新组件的缺省值。


step 3 extract(`render_core/env.rs` 新增,list.rs / delta.rs / extract3d.rs 小改;lib.rs 没动):
- 中立数据:`RenderList.env: SceneEnv`(environment / camera_attributes / render_settings 三个 `Option<Props>`,加 Environment 引用的贴图)、`RenderList.volumes: Vec<VolumeItem>`(ReflectionProbe / Decal / FogVolume:key、kind、world、props、Decal 贴图、content 指纹)、`LightItem.params: Option<Props>`(LightParams)、`ExtractStats.volumes`。
- `Props` = 该组件 `normalize_props` 之后的全量字段;类型不对的字段按注册表缺省换回(逐字段复用 `forge_scene::validate_props`),所以取值永远合法。场景里没有 Stage 5 组件时 `SceneEnv` 全是 None,Godot 走 Stage 4 的缺省环境。
- RenderDelta 加 `env`、`volumes`:全量帧恒带,之后只在变化时下发;体积类整体下发,Godot 侧按 key + content 自己分"重建 / 只移动 / 删除"。
- 这些类型没有经 lib.rs 导出类型名(lib.rs 不在改动范围),只经 RenderList / RenderDelta / LightItem 的公开字段可达;godot-host 用 `props::Fields` 适配器按字段名取值,枚举字段比较字符串。
- 单测:render_core 20 → 24 过(+ 1 个 ignore 的 profile)。新增 3 个 env 测试(7 个组件全部可选且缺省合法、Light / Camera 没加字段;类型修复;场景级取第一个启用的、体积类走 Parent 链、content 不含 world),1 个 delta 测试。
- 验证 s5-step3(21:53):lib 122 / 3 / 7(+4 个新测试),10 个集成二进制全绿;clippy 33 / 35(33 重复)/ 1 / 0;build 警告 0;no-default 能编译;帧基线 396/396(0 / 0),契约 31/31,CLI 11/11。engine-host.exe = E3CF7168…。


step 4 环境 / 后处理(godot-host 新增 `env.rs`、`props.rs`,改 scene.rs、material.rs、light.rs、backend.rs;engine-host `render/`:RenderBackend 加两个缺省方法 `unsupported_features` / `limited_features`,由 render/mod.rs 并进 `coverage.unsupported` / `limited`;Coverage 结构体不加字段,因为 `tests\f3_pipelined.rs` 用结构体字面量构造它):
- 没有 Environment 组件时,仍用 Slot 自己的 RS environment 加 `light::configure_environment`,与 Stage 4 一个 RS 调用都不差。
- 有 Environment 组件时建一个 `Environment` 资源(不挂节点,只用 `get_rid`),按 `env.rs` 的表把 94 个 forge 字段 `Object::set` 成 Godot 属性,再把 scenario 指向它;组件消失时指回原来的 environment。
  - 不直接调 RS 的原因:`environment_set_ssil` 没有绑定给 GDExtension(RS 的 `_bind_methods` 里没有它),资源的 setter 在引擎内部调;而且资源构造时就把 Godot 缺省推给 RS,缺省值天然一致。
  - 每张表第一次用时检查属性名(`Object::get` 为 nil 就打一行 stderr)。g5 测试的日志里没有出现这类行。
  - background = clearColor 时,背景色按 tonemap(linear / reinhard,含曝光与 white)求逆,出帧等于腿清屏色;filmic / aces / agx 不求逆,背景会被 tonemap 改变。
  - 天空:skyType → ProceduralSkyMaterial / PhysicalSkyMaterial / PanoramaSkyMaterial,只在背景、环境光或反射用到 sky 时才建。LUT:Texture2D 按 1D LUT 用(Godot 语义),3D LUT 本版不接。
- CameraAttributes → `CameraAttributesPractical` 资源 + `scenario_set_camera_attributes`。曝光倍数经 exposure normalization 只作用在灯的能量上(`camera_attributes_storage.cpp:141`),平面环境光不受影响。
- RenderSettings → 3D 视口的 `viewport_set_*`;RS 全局质量设置只跟 Main 通道,组件消失时恢复 ProjectSettings 的值。
- LightParams → `light_set_param` 等;全缺省时与 Stage 4 的 node_defaults 完全相同(g5 测试逐字节验证)。
- SDFGI 只收 GI_MODE_STATIC 的几何。Environment 开了 SDFGI 时给实例打 `INSTANCE_FLAG_USE_BAKED_LIGHT`,开关变化时整表重建实例;没开时实例与 Stage 4 相同。
- 有意修正 1(金属)。GDExtension 拿不到 BaseMaterial3D 生成的着色器(`get_shader_rid` 没有绑定),所以不改着色器,改材质参数:
  - 缺省环境下,metallic > 0 且不是 unlit 的材质,EMISSION 补上 base·metallic·0.14·ao(同一个 pass、tonemap 之前,三种渲染方式都成立)。
  - 没有贴图时补一个颜色。有 albedo / MR / AO / emissive 贴图时,在 CPU 上逐 texel 烘一张 sRGB 贴图(emission_operator = MULTIPLY),原有的 emissive 按 rurix 的"emissive × 贴图"并进去。
  - 材质缓存键带上这个开关。Environment 组件出现 / 消失时整表重建实例(有组件 = 纯 Godot 语义,不补)。
- 有意修正 2(Mobile)。3D 视口开 `use_hdr_2d`:render_scene_buffers_rd 的 force_hdr 让 3D 缓冲变成 RGBA16F,tonemap 输出线性值。外面再套一个 8 bit 输出视口,canvas 着色器用 Godot tonemap 同一个 `linear_to_srgb` 编码(nearest 采样;子视口先画,见 `RendererViewport::_sort_active_viewports`)。
  - 导出、L1、粒子叠加层都用外层视口,格式仍是 RGBA8 sRGB 编码值,下游不用改。
  - 代价:Mobile 的 debanding 不再生效(Godot 只在非 HDR 目标上做),已标进 capabilities。
- `render.capabilities.coverage` 新增两个键:`unsupported`(本渲染方式忽略的特性:特性键 + 原因,编辑器据此置灰)、`limited`(能用、但实现与 Forward+ 不同)。逐项见下面的映射覆盖表。


step 5 反射探针 / 贴花 / 雾体积(godot-host 新增 `volumes.rs`):
- RenderDelta.volumes 下发时按 key + content 增量处理:content 变了重建,只有 world 变了只调 `instance_set_transform`,消失的释放。
- ReflectionProbe → `reflection_probe_*` + instance。Decal → `decal_*`,贴图取场景里的贴图资产 guid(ImageTexture 保活),沿实体局部 −Y 投射。FogVolume → `fog_volume_*` + `FogMaterial` 资源。
- VoxelGI 不做:只能经节点烘焙(01 §6.1);GI 用 SDFGI。

step 6 粒子样式预设(godot-host `particles.rs` 重写):
- 不用 Godot 的粒子模拟,改成一个 canvas 叠加层。
  - 静态三角形数组:4096 个粒子 × 6 个顶点,顶点 = (全局序号, 角序号),顺序同 rurix 的实例序。
  - canvas_item 着色器按 gpu_particles COMPUTE 同一个 hash / random、同一组 kind 1-4 公式算位置、尺寸、颜色,再用本帧的 view_proj 投到像素坐标。
  - 每帧只上传 `forge_em[128]`(中心、年龄、寿命、样式、种子、有效)和相机矩阵,所以有粒子的帧两次取帧逐字节相同。
- 混合空间与 rurix 相同。rurix 把粒子加法混合在 UNORM 目标的 sRGB 编码值上、画在所有网格之后、不测深度;非 HDR 视口的 2D canvas 同样在 sRGB 编码值上混合(canvas.glsl 只在 HDR 2D 时转线性),而且画在 3D 之后。UV 按 1/w 手工做透视校正。
- 第一版 y 方向反了:rurix 出帧(首行在上)里 NDC +y 在画面上方,canvas 像素坐标 y 向下。修正后四配置与 rurix 最大差 ≤ 3、全帧平均差 ≤ 0.012,亮点数相同(Compatibility ±2),逐项见 g5_particles 与 `evidence\godot-backend\stage5\particles.json`。
- draws 改成与 rurix 同口径:粒子叠加层和体积类实例都不计入(rurix 的粒子也不计 draws)。

与 01 / 02 / 提示词的偏差(已接受或待用户确认):

| 位置 | 原设计 | 实际 | 理由 |
|---|---|---|---|
| lib.rs 导出 | 新类型经 lib.rs 导出 | 不导出;经 RenderList / RenderDelta / LightItem 的字段可达,godot-host 用 `props::Fields` 取值 | lib.rs 不在 Stage 5 的改动范围 |
| forge-scene 测试 | 只加组件 | 注册表计数测试 17 → 24 | 不改的话,一个现在通过的测试会变红 |
| 01 §6.3 Light 扩展字段 | 加在 Light 上 | 独立组件 LightParams | normalize_props 会改变 rurix 的 entity.get / scene.save |
| 01 §6.3 嵌套对象 | `glow{…}` 等 | 扁平的前缀字段 | FieldSpec 只校验顶层字段 |
| 01 §6.3 VoxelGI | 组件 | 不进 schema | 只能经节点烘焙 |
| 金属修正 | fragment 末尾补一项 | 材质 EMISSION 参数 + CPU 烘贴图 | `get_shader_rid` 没有绑定给 GDExtension |
| 粒子 | particles 着色器,或 MultiMesh + INSTANCE_CUSTOM | canvas 叠加层 + canvas_item 着色器 | 与 rurix 在同一个混合空间(sRGB 编码值),逐帧确定 |
| Mobile 背景 | 清屏走 2D / 8 bit,或 HDR 缓冲 | HDR 2D 内层视口 + 8 bit 转换层 | 导出 / L1 格式不变;代价是 Mobile 没有 debanding |

g5 第一轮 7 个失败与处理:
- 曝光倍数:测试场景只有环境光,曝光 normalization 只作用于灯 → 改用方向光照明。
- LightParams:0.14 的缺省环境光让整块面板都高于亮度阈值 → 改成离灯 1 个单位处的亮度,聚光的阈值改 150。
- 粒子:y 方向反了(见上)。
- Compatibility SSAO:GLES3 是另一套实现(s4ao,后处理里按深度估算),墙角几乎不变 → 标 `limited`,只要求画面有变化。
- Compatibility 反射探针:开关前后逐字节相同 → 标 `unsupported`,原因未查清(K1 文档只写"每 mesh 最多 2 个")。
- Mobile debanding:HDR 2D 下不生效 → 标 `unsupported`。
- 金属:全金属达标;半金属(metallic 0.5、base (0.8, 0.4, 0.2)、rough 0.6,F+ D3D12)rurix 121.7 / 97.0 / 75.0,修正后 133.2 / 102.0 / 76.0(差 11.5),修正前 124.1 / 94.1 / 70.0(差 5.0)。原因:rurix 的漫反射还乘 (1 − F),F0 随 metallic 变大,而 Godot 的 k_model 按电介质(F0 = 0.04)标定;修正前的环境项缺口恰好抵掉了一部分漫反射偏亮。测试改为只记录这一项,列为未决项。

step 7 验证(s5-final 与 `tools\stage5\final-pipeline.ps1`,2026-09-29 22:47-23:09;会话重启后 23:25-00:20 复核补测):

| rurix 参照值 | Stage 4 收尾 | Stage 5 收尾 | 结论 |
|---|---|---|---|
| `cargo test -p engine-host` lib | 118 / 3 / 7 | 122 / 3 / 7(+4 个 render_core 新测试) | 3 个失败仍是改前就有的 planning_contracts |
| 集成测试二进制 | 10 个全绿 | 10 个全绿 | 相同 |
| clippy lib / `--tests` | 33 / 35(33 重复)+ f2 1 + f3_* 0 | 相同 | 相同 |
| build 警告 / `--no-default-features --lib` | 0 / 能编译 | 0 / 能编译 | 相同 |
| 帧基线 / 契约 / CLI | 396/396 / 31/31 / 11/11 | 396/396(0 / 0)/ 31/31(diff 0)/ 11/11(diffs 0) | 相同 |
| `cargo test -p forge-scene` | 17/17 | 17/17(计数测试已改 24) | 相同 |
| Cargo.lock / 根 Cargo.toml | 3502F2D7… / 944E8514… | 不变 | 依赖没变 |
| engine-host.exe(`-p engine-host`)/ godot_host.dll | A66B7049… / 65A2C711… | 81355F03… / C10A51A9… | 源码变了;rurix 行为以上面三项对照为准 |

- godot:`cargo test -p godot-host`(s5-gh-final):g3 5/6(仍是 legs 断言)、g4 17/17、g5 11/11(6 个测试二进制:camera_gi 2、env 3、fixes 2、particles 1、settings 2、volumes 1)。
- `cargo clippy -p godot-host --tests`:godot-host 自己 0 条。依赖 engine-host 按 godot-host 的特性组合编译时报 14 条,都在 Stage 5 没改的文件里(modelrt、prefab、rpc、stream、render_core/model.rs、render/preview.rs、sentinels_v6_*)。流水线里这一步记成 exit=1:`powershell -Command "cmd /c … > log 2>&1"` 让 PowerShell 5.1 把 cargo 的 stderr 包成了错误记录;改成直接 cmd 重定向重跑,exit 0。
- 66 场景 × 4 配置(`evidence\godot-backend\stage5\scene-matrix-20260929-230306`,对照 Stage 4 的 matrix.json,`compare-stage4.json`):264 帧全部出帧、两帧哈希相同、0 错误。
  - F+ D3D12、F+ Vulkan、Compatibility 各 66/66 与 Stage 4 逐字节相同。项目场景里没有 metallic > 0 的材质,所以有意修正 1 不影响矩阵。
  - Mobile 66/66 变化,全部来自有意修正 2。修正后 Mobile 有 63 个场景与 F+ D3D12 逐字节相同(Stage 4 是 0 个);journey、maze、pz_mvp_phase1 三个仍不同。
- 桌面冒烟(`run-smokes.ps1 -NoWmi`,23:08-23:09):rurix 8/8、godot 8/8 PASS。另一方的开发栈此前已停,3080 / 8103 空闲。


两项有意修正影响到的场景与哈希(其余场景四配置与 Stage 4 逐字节相同):
- 修正 1(缺省环境下金属补环境项):66 个项目场景都不受影响。Stage 4 夹具里只影响 materials(`side-by-side-stage4-20260929-230431` 对 Stage 4 的 `side-by-side-20260929-194857`):
  - 哈希:F+ D3D12 9D5C5329E12A → 2F9E9D8DAC41,F+ Vulkan 9EA67F18B3D9 → 2F9E9D8DAC41(两个驱动现在相同),Mobile 4538C70B2463 → 2C848E2F3BC5,Compatibility 223D698F7E13 → E33EF94C064A。
  - mats.metal 探针:rurix 97;Stage 4 四配置 30-31(差 66-67);Stage 5 四配置 98(差 1),目标 ≤ 8 达成。F+ / Compatibility 其余探针都不变。
  - 半金属(metallic 0.5)反而变差,见 g5 第一轮与未决项。
- 修正 2(Mobile 改 HDR 2D 缓冲):矩阵里 Mobile 的 66 帧全变;Stage 4 夹具里 Mobile 的 7 个场景全变。全帧平均差(Stage 4 → Stage 5):

| 场景 | maze | graycard_model | graycard_mesh | materials | anim | lights | pz_mvp |
|---|---|---|---|---|---|---|---|
| Mobile 平均差 | 0.84 → 0.351 | **2.047 → 0.320** | 1.994 → 1.237 | 3.452 → 0.311 | 2.173 → 0.069 | 17.526 → 16.447 | 1.685 → 0.691 |

  - graycard_model 目标 ≤ 1.0 达成,Mobile 哈希 B34FAAE1D390 → 4120BDD99987,与另外三种配置相同。
  - 探针(与 rurix 的最大通道差):变大的只有 graycard_mesh NdotL=1.0 与 materials mats.unlit,各 +1(4 → 5、1 → 2);变小的有 graycard_mesh NdotL=0.4 / 0.2 与 materials 的 checker.red / blue / white、mask.a0(各降 1-3);其余不变。Mobile 的 lights 最大差 61 → 69,等于 F+ 的 69。

并排(Stage 5 夹具,`scripts\f5-make-fixtures.py` → `crates\godot-host\tests\fixtures\stage5\`,脚本 `scripts\f5-side-by-side.ps1 -Fixtures stage5`):
- 最终证据 `side-by-side-stage5-20260929-233951`:6 场景 × 5 路全部 PASS(五路出帧、两帧相同)。rurix 忽略 Stage 5 组件,所以与 rurix 的差只作对照,不设目标。
- 流水线里那次(`side-by-side-stage5-20260929-230617`)gi 失败:F+ D3D12 / Vulkan 两帧哈希不同。复核实验(F+ 两种驱动):SDFGI 预热 60 帧后相邻两帧差 1 LSB(平均 0.0067),120 帧后仍差 1 LSB(平均 0.0002),240 帧起逐字节相同,之后第 480 帧也不变。所以 gi 夹具的 warmFrames 改成 240(f5-make-fixtures.py 与脚本场景表的说明同步改),判据不放宽。
- 粒子(particles_05 / particles_09 与 g5_particles 的 `evidence\godot-backend\stage5\particles.json`,kind 1-4 各三个年龄 + 四种合在一起):

| 配置 | kind 1-4 最大差 | 全帧平均差(最大) | 亮点数与 rurix |
|---|---|---|---|
| F+ D3D12 / F+ Vulkan / Mobile | ≤ 2 | ≤ 0.0006(四种合在一起 ≤ 0.0013) | 全部相同 |
| Compatibility | ≤ 3(只有 kind1 一处) | ≤ 0.004(合在一起 ≤ 0.012) | 差 ≤ 2 |

映射覆盖表(01 §6 逐项;"生效" = g5 开关前后画面按预期方向变化,"不支持" = `coverage.unsupported` 列出且开关前后逐字节相同,"受限" = `coverage.limited`;F+ 一栏 D3D12 / Vulkan 结果相同):

| 01 §6 项 | forge 字段 | 状态 | 测试 | F+ | Mobile | Compatibility |
|---|---|---|---|---|---|---|
| 背景 clearColor / color | Environment.background* | 完成;开雾时有 Godot 缺陷(见下) | g5_env environment_features、defaults_are_byte_identical_to_stage4、g5_fixes background | 生效 | 生效 | 生效 |
| 天空 | skyType / skyTexture / skyEnergy / skyRotation | 完成 | g5_env(sky) | 生效 | 生效 | 生效 |
| 环境光 / 反射来源 | ambient* / reflectionSource | 完成 | g5_env(ambient) | 生效 | 生效 | 生效 |
| tonemap(含 AgX)/ 曝光 / white | tonemap / exposure / white / agx* | 完成 | g5_env(tonemap、exposure) | 生效 | 生效 | 生效 |
| adjustment / LUT | adjustment* | 部分:1D LUT 接了,3D LUT 没接 | g5_env(adjustment) | 生效 | 生效 | 生效 |
| glow | glow* | 完成 | g5_env(glow) | 生效 | 生效 | 生效 |
| SSAO | ssao* | 完成 | g5_env screen_space_effects | 生效 | 不支持 | 受限(s4ao) |
| SSIL / SSR | ssil* / ssr* | 完成 | 同上 | 生效 | 不支持 | 不支持 |
| 普通雾 | fog* | 完成 | g5_env(fog) | 生效 | 生效 | 生效 |
| 体积雾 | volumetricFog* | 完成 | g5_env screen_space_effects | 生效 | 不支持 | 不支持 |
| SDFGI | sdfgi* | 完成;约 240 帧才收敛到逐字节稳定 | g5_camera_gi sdfgi | 生效 | 不支持 | 不支持 |
| VoxelGI | — | 未做:只能经节点烘焙,不进 schema | — | — | — | — |
| 曝光倍数 / 感光度 | CameraAttributes.exposure* | 完成 | g5_camera_gi camera_attributes | 生效 | 生效 | 生效 |
| DOF | dofBlur* | 完成 | 同上 | 生效 | 生效 | 不支持 |
| 自动曝光 | autoExposure* | 完成 | 同上 | 生效 | 不支持 | 不支持 |
| 反射探针 | ReflectionProbe | 完成 | g5_volumes | 生效 | 生效 | 不支持(实测不生效,原因未查清) |
| 贴花 | Decal | 完成 | g5_volumes | 生效 | 生效 | 不支持 |
| 雾体积 | FogVolume | 完成 | g5_volumes | 生效 | 不支持 | 不支持 |
| Compositor | — | 未做:forge 没有对应语义,不进 schema | — | — | — | — |
| MSAA 3D | RenderSettings.msaa3d | 完成 | g5_settings render_settings | 生效 | 生效 | 生效 |
| FXAA / SMAA | screenSpaceAA | 完成 | 同上 | 生效 | 生效 | 不支持 |
| TAA | taa | 完成 | 同上 | 生效 | 不支持 | 不支持 |
| 3D 缩放 | scaling3dScale | 完成 | 同上 | 生效 | 生效 | 生效 |
| FSR / FSR2 | scaling3dMode / fsrSharpness | 部分:已映射,没有单独测试 | — | 按 K1 应生效 | 不支持 | 不支持 |
| debanding | debanding | 完成 | 同上 | 生效 | 不支持(HDR 2D) | 不支持 |
| RS 全局质量 | ssaoQuality / ssilQuality / sdfgiRayCount / volumetricFogVolume* | 部分:已映射,只跟 Main 通道,没有单独测试 | — | — | — | — |
| 灯参数 | LightParams | 完成;全缺省时与 Stage 4 逐字节相同 | g5_settings light_params、g5_env defaults | 生效 | 生效 | 生效 |
| 粒子样式 kind 1-4 | ParticleEmitter | 完成(canvas 叠加层) | g5_particles | 最大差 2 | 最大差 2 | 最大差 3 |


收尾复核新发现:开雾时 color / clearColor 背景偏暗(Godot 4.7.2 的缺陷,本阶段未修):
- 现象:volumes 场景开了体积雾(密度 0),F+ 背景是 (1,1,1),Mobile / Compatibility 是 (9,11,15),与腿清屏色相同。
- 实验(F+ D3D12,volumes 的 6 个变体):关掉体积雾就恢复 (9,11,15);volumetricFogSkyAffect = 0、换透视相机、去掉 FogVolume 都仍是 (1,1,1)。所以不是雾本身在混合,而是背景换了一条绘制路径。
- 源码:背景为 CLEAR_COLOR / COLOR 且开了雾(F+ 还包括体积雾缓冲存在)时,F+ / Mobile 改用"只画雾的天空"着色器(`render_forward_clustered.cpp:2034-2046`、`render_forward_mobile.cpp:1093-1105`),先把颜色 `srgb_to_linear()` 再设成材质参数;材质存储对用户给的 Color 值再转一次线性(`material_storage.cpp:801` → `variant_converters.h:208-211`),内置的雾天空着色器 `uniform vec4 clear_color;`(`sky.cpp:884`)没有 `color_conversion_disabled` 提示。(9,11,15) 连续转两次正好编码成 (1,1,1)。另外这条路径会把 backgroundEnergy 与曝光倍数再乘一遍(`render_forward_clustered.cpp:2077` 的 sky_brightness_multiplier)。Compatibility 只转一次(`rasterizer_scene_gles3.cpp:2599`),不受影响。
- 影响面:只在有 Environment 组件、background ∈ {clearColor, color}、并且 F+ 开了 fog 或 volumetricFog、Mobile 开了 fog 时出现。不在"没有 Stage 5 组件"的路径上,矩阵与 Stage 4 对照不受影响;g5 没有断言这种组合下的背景。雾天空系数(fogSkyAffect / volumetricFogSkyAffect)缺省是 1,背景多半被雾色盖住,所以 fog 夹具四配置结果一致。
- 可选补偿(在 godot-host env.rs 里做,不动 Godot):上述条件成立时,把背景色 c 换成 c' = L(L(S(c·e)/e))/e(S = sRGB→线性,L = 线性→sRGB,e = backgroundEnergy × 曝光倍数;e = 1 时 c' = L(c)),再补一个 g5 测试。是否改成与 rurix 的清屏语义一致,还是保留 Godot 原生的表现,留给用户决定。

与 01 / 02 / 提示词的偏差(补充,step 7):

| 位置 | 原设计 | 实际 | 理由 |
|---|---|---|---|
| gi 并排夹具 | 预热 60 帧 | 预热 240 帧 | SDFGI 实测 240 帧起逐字节稳定;两帧相同的判据不放宽 |
| 能力上报 | Coverage 加 unsupported / limited 字段 | RenderBackend 缺省方法,由 render/mod.rs 并进 coverage | `tests\f3_pipelined.rs` 用结构体字面量构造 Coverage,加字段就编不过 |
| Mobile 与 F+ | — | 修正 2 之后 66 个矩阵场景里 63 个与 F+ D3D12 逐字节相同 | 附带收益,记录备查 |

Stage 5 未决项（历史快照；Stage 6 更新见下节）:
1. 半金属(metallic 0.5)与 rurix 的差从 5.0 变成 11.5(见 g5 第一轮);要改 k_model 的菲涅尔标定,或只对 metallic = 1 补环境项。
2. Compatibility 的反射探针实测不生效,原因未查清;已标 unsupported。
3. 开雾时背景偏暗(上面一节),修不修待用户决定。
4. SDFGI、自动曝光、TAA、体积雾时间重投影都是时间性效果:单帧取帧看到的是当时的收敛程度,做逐帧哈希对照时要先预热(SDFGI 约 240 帧)。
5. 沿用 Stage 4 的:`g3_host.rs` 的 legs 断言(改它要先征得用户同意)、Compatibility 半透明在 sRGB 帧缓冲里混合(mats.blend 81 对 rurix 111)、Compatibility 下 castShadow 关、V6 在 Compatibility 暗部差到 8、maze 的 2-3 个孤立像素(最大差 114-123)、engine-scene-mcp 的 watchdog_integration(24 ≠ 16)。

### 9.6 Stage 6 / 7 通用后端收尾

本节覆盖上一阶段的历史未决项；详细执行结果、失败保留和证据目录见 [03-completion.md](03-completion.md)。固定 Godot 4.7.2 / gdext 0.5.5，不新增 EditorPlugin，不迁移 Code Sentinels。

- 普通 Sprite 已由中立 RenderList 携带真实图集内容；符合条件的纯正交 2D 走 Canvas，透视、混合网格/模型及非共面精灵走 3D quad。V6 单独分流，不能由空普通项目列表误判为 Canvas。
- 四配置实际验证图集裁切、最近邻、翻转、tint、色键、pivot、三种混合、透明排序、选中显示、热重载、删除清理、深度及动态分流。缺省模型环境的 Reinhard 仅对普通精灵做限定补偿；显式 Environment 沿用用户设置。
- Canvas 与透明 3D quad 支持 sortingOrder；不透明 3D quad 采用深度排序，同深度 sortingOrder 不生效，能力中明确提示。HDR 线性混合与编码空间混合不能承诺跨后端像素一致。
- 雾背景的颜色/能量重复转换已在后端内修正；Compatibility 只对已验证 tone map 与正曝光补偿。半金属不再使用全金属补偿，BRDF 差异保留为 limited。
- FSR / FSR2、SSAO quality 已补四配置像素验证及恢复验证；其他质量参数仍以各自已有证据为准。Canvas 不参与 Environment Glow，3D quad Glow 四配置有效；Mobile HDR 2D 缓冲不是新增的 2D Glow 能力。
- 运行时生成脚本与监督器检查必需文件、SHA-256、路径与开发 DLL 过期；Godot 包携带独立 runtime、项目资产闭包和必要缓存，Rurix 打包入口保留。便携测试清除继承环境、删除临时源项目并从无关工作目录启动。
- 桌面按真实能力开启共享呈现或回读，提示移出原生子窗口范围，2D 网格使用网页叠加而不污染引擎帧。配置不一致只提示重启；真实 Godot 强杀重启已验证，恢复为空场景而非编辑状态快照。
- `g3_host` 能力断言及监督器旧工具数量按当前契约核实后更新。完整核心测试仍有三个游戏规划测试失败，未修改游戏逻辑或删掉断言，不能声称整仓通过。
- 最终矩阵、原后端帧基线、便携与桌面四配置证据集中在 03；历史 Stage 4 / 5 结果保留，不将后续实现倒写成当时已完成。

