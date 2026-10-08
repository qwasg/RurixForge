//! godot-host:Forge 引擎宿主的 Godot 4.7.2 渲染后端(GDExtension,cdylib)。
//!
//! 已定决策 1:官方 Godot 4.7.2 运行时装载本扩展,engine-host 核心(JSON-RPC、WS 推流、物理、逻辑)经
//! `engine_host::start_core(AcceptMode::Thread)` 跑在 Godot 进程里,IDE / MCP 看到的协议与 rurix 宿主相同。
//!
//! 线程(01 T1-T3、02 §2.4):Godot 主线程 = `[gmain]`,只有它调 RenderingServer / RenderingDevice;
//! forge 线程只经 `SubmitBox` / `FrameBus`(纯 Rust 数据)与它交接,从不跨线程传 `Gd` / `Variant` / `Callable`。
//! 物理、逻辑、PIE 只走 rurix-physics + forge-logic(确定性红线),Godot 的物理 / 音频 / 导航一律不用。
//!
//! 模块:
//! - `host`:主循环类 `ForgeHost`(`#[class(base=SceneTree)]`,01 §4.3),`initialize` / `process` / `frame_post_draw`;
//! - `backend`:`GodotBackend`(`RenderBackend` + `PipelinedRender`),forge 线程侧;
//! - `scene`:离屏视口与 RenderDelta 的应用(相机、环境、灯、MeshRenderer、模型、蒙皮);
//! - `mesh` / `material` / `light`:RS 网格、材质与贴图、灯光与环境(01 §5);`rid`:RID 的 RAII 句柄;
//! - `export`:帧导出与交付(L2 回读;L1 的 GPU 拷贝在 `l1`);
//! - `l1`:D3D12 零拷贝(01 X1 的 a2 变体),仅 Windows;
//! - `config`:env 配置(02 §6.2:核心参数一律走 env)。

use godot::prelude::*;

mod backend;
mod config;
mod env;
mod export;
mod host;
#[cfg(windows)]
mod l1;
mod light;
mod material;
mod mesh;
mod particles;
mod props;
mod rid;
mod scene;
mod sprite;
mod shader_graph;
mod v6;
mod volumes;

struct ForgeHostExtension;

// 入口符号 gdext_rust_init(forge_host.gdextension 的 entry_symbol)。
#[gdextension]
unsafe impl ExtensionLibrary for ForgeHostExtension {}
