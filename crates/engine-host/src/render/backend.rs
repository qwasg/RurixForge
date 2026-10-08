//! 渲染后端接缝(02 §4.1):后端身份、能力、两种出帧形态(Immediate / Pipelined)与进程级单例。
//! rurix 走 Immediate(调用方线程同步出帧);Pipelined 只有接口,实现归 Stage 3(Godot 后端)。

use std::sync::{Arc, OnceLock};

use forge_scene::Scene;

use crate::render::bus::{Channel, FrameBus, FrameRequest};
use crate::render::sink::SharedTarget;
use crate::render_core::camera::EditorCamera;
use crate::render_core::list::{Leg, RenderList};
use crate::render_core::math::M4;
use crate::viewport::FramePixels;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Rurix,
    #[allow(dead_code)] // Stage 3:GodotBackend
    Godot,
}

/// forge.toml [render].method / driver(§7);rurix 下两者都是 None。
#[allow(dead_code)] // Stage 3:只有 Godot 后端取值
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMethod {
    ForwardPlus,
    Mobile,
    GlCompatibility,
}

#[allow(dead_code)] // Stage 3:只有 Godot 后端取值
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderDriver {
    D3d12,
    Vulkan,
    Opengl3,
}

/// 配置来自哪里,优先级 Cli > Env > ForgeToml > Default(§7.3)。Stage 2 恒 Default(§7 的解析未落地)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Default,
    #[allow(dead_code)] // §7 [render] 段落地后才有
    ForgeToml,
    #[allow(dead_code)]
    Env,
    #[allow(dead_code)]
    Cli,
}

#[derive(Debug, Clone)]
pub struct BackendInfo {
    pub kind: BackendKind,
    pub method: Option<RenderMethod>,
    pub driver: Option<RenderDriver>,
    pub source: ConfigSource,
    /// 取值来源见 01 §4。
    pub godot_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LegSet {
    pub sprite_mesh: bool,
    pub model: bool,
    pub sentinels_v6: bool,
}

impl LegSet {
    pub fn contains(&self, leg: Leg) -> bool {
        match leg {
            Leg::SpriteMesh => self.sprite_mesh,
            Leg::Model => self.model,
            Leg::SentinelsV6 => self.sentinels_v6,
        }
    }
}

/// I8:给不出的统计填 0/false,并在这里声明 false。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatsCaps {
    pub nonzero: bool,
    pub triangles: bool,
    pub truncated: bool,
    pub mesh_fallbacks: bool,
    pub mesh_classes: bool,
}

/// None = 该腿不截断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaxDraws {
    pub sprite_mesh: Option<u32>,
    pub model: Option<u32>,
    pub sentinels_v6: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Capabilities {
    pub legs: LegSet,
    /// true = 帧经 FrameBus 异步产出。
    pub pipelined: bool,
    /// template.preview 可用。
    pub preview: bool,
    /// rurix = gpu_particles::enabled();Godot 首版 false。
    pub particles: bool,
    /// L2 出口;恒 true(WS / RPC / H.264 都靠它)。
    pub cpu_rgba8: bool,
    /// 能喂 D3D12 共享 buffer(CPU 上传或 GPU 直写)。
    pub shared_d3d12: bool,
    /// 能不经 CPU 写共享 buffer(rurix = import 档;Godot = L1)。
    pub zero_copy: bool,
    pub stats: StatsCaps,
    pub max_draws: MaxDraws,
}

/// render.capabilities 的 coverage 键(RenderBackend::coverage):本后端当前版本在已声明的腿里跳过的内容。
/// 不放进 Capabilities 结构体:它的字段集是 Stage 2 的公开契约(tests/f3_start_core.rs 用结构体字面量构造)。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Coverage {
    pub skipped: Vec<&'static str>,
}

/// D3D12 debug layer 统计(`--gpu-validation` 时 [gmain] 读 ID3D12InfoQueue 的存量消息)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DebugLayerStats {
    pub errors: u64,
    pub warnings: u64,
    pub corruption: u64,
}

/// 帧通道现状(render.backendInfo 的 frameChannels 键;只有 Pipelined 后端给出,rurix = None,不出现该键)。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FrameChannels {
    /// L2 读回路径:"rd_async"(Forward+ / Mobile)| "texture_2d_get"(Compatibility)。
    pub l2: &'static str,
    /// L1 能否启用:D3D12 + 有 RD + Godot adapter 的 LUID 与缺省 adapter 相同(01 X1)。
    pub l1_available: bool,
    /// L1 当前是否在工作(已 ShareAttach、尺寸一致、未降级)。
    pub l1_active: bool,
    /// L1 不可用 / 降级的原因。
    pub l1_reason: Option<String>,
    pub adapter: Option<String>,
    /// 已交付帧计数:经 L1 写进共享 buffer 的帧 / 带 CPU 像素(L2)的帧。
    pub l1_frames: u64,
    pub l2_frames: u64,
    /// 延迟(帧):最近一次交付时,距这一帧被画出过了几次 RS::draw。L1 的设计值 1(a2 变体),L2 ≈ 1 + frame_queue_size(01 §3.6)。
    pub l1_lag: Option<u64>,
    pub l2_lag: Option<u64>,
    /// None = 没开 debug layer。
    pub debug_layer: Option<DebugLayerStats>,
}


/// 出帧形态。两种形态分开建模,不强行统一成一个 render():rurix 不为迁就异步而改调用时机(I1)。
#[derive(Clone, Copy)]
pub enum FramePath<'a> {
    Immediate(&'a dyn ImmediateRender),
    #[allow(dead_code)] // Stage 3:GodotBackend 返回
    Pipelined(&'a dyn PipelinedRender),
}

pub trait RenderBackend: Send + Sync + 'static {
    fn info(&self) -> &BackendInfo;
    fn capabilities(&self) -> &Capabilities;
    fn path(&self) -> FramePath<'_>;
    /// asset.reload 在现有三处 invalidate(rpc.rs asset_reload 首行)之后调用;rurix 为空操作。
    fn invalidate_assets(&self);
    /// 最近一帧的 FramePixels.device_name;首帧前 None(render.backendInfo 用)。
    fn device_name(&self) -> Option<String>;
    /// render.backendInfo 的 frameChannels(L1 / L2 现状);缺省 None,rurix 不覆盖。
    fn frame_channels(&self) -> Option<FrameChannels> {
        None
    }
    /// render.capabilities 的 coverage(腿内跳过的内容);缺省 None,rurix 不覆盖,JSON 不出现该键。
    fn coverage(&self) -> Option<Coverage> {
        None
    }
    /// Stage 5:本渲染方式按 01 §7 K1 忽略的 schema 特性 (特性键如 "Environment.ssao", 原因),
    /// 进 coverage.unsupported(编辑器据此置灰);缺省空。放在 Coverage 之外:Coverage 的字段集是公开契约
    /// (tests/f3_pipelined.rs 用结构体字面量构造)。
    fn unsupported_features(&self) -> Vec<(&'static str, &'static str)> {
        Vec::new()
    }
    /// Stage 5:能用、但实现 / 效果与 Forward+ 不同的特性,进 coverage.limited;缺省空。
    fn limited_features(&self) -> Vec<(&'static str, &'static str)> {
        Vec::new()
    }
}

/// rurix:调用方线程同步出帧。
pub trait ImmediateRender: Send + Sync {
    fn render(&self, input: FrameInput<'_>) -> Result<FramePixels, String>;
    /// template.preview(rpc.rs template_preview 里的 modelrender::render 调用)。
    fn preview(&self, input: FrameInput<'_>) -> Result<FramePixels, String>;
}

/// Godot:提交 RenderList;[gmain] 出帧后经 FrameSink(§4.2)发布到 FrameBus。Stage 2 只有接口。
#[allow(dead_code)] // Stage 3 实现;rurix 从不走这里
pub trait PipelinedRender: Send + Sync {
    fn next_seq(&self) -> u64;
    /// latest-wins 投进 ch 的 SubmitBox;req 进 waiters。不阻塞、不取 HS。
    fn submit(&self, ch: Channel, list: Arc<RenderList>, req: Option<FrameRequest>) -> Result<(), String>;
    fn bus(&self, ch: Channel) -> &FrameBus;
    fn control(&self, msg: ControlMsg) -> Result<(), String>;
    /// Main 通道首帧已发布;否则取帧类 RPC 返回 RENDER_NOT_READY。
    fn ready(&self) -> bool;
    /// §4.5 第 12/13 项:投递 ShareDetach 并等 [gmain] 确认已停写旧共享 buffer(上限 timeout)。返回是否确认。
    /// 缺省实现只投递、不等回执;有 [gmain] 的后端覆盖它。
    fn detach_share(&self, timeout: std::time::Duration) -> bool {
        let _ = timeout;
        self.control(ControlMsg::ShareDetach).is_ok()
    }
}

/// 非帧类控制消息;同样经 SubmitBox,[gmain] 在 process() 开头先处理控制消息再处理 RenderList。
/// (形状是 Stage 2 的公开契约:tests/f3_start_core.rs 对它做穷尽匹配;需要回执时用 PipelinedRender::detach_share。)
pub enum ControlMsg {
    /// §4.2 L1;[rpc] 在 share::open 成功后投递。
    ShareAttach(SharedTarget),
    /// viewport.shareClose / 重开共享 buffer 之前:[gmain] 停止写旧 buffer 并释放 COM 引用(§4.5 第 12/13 项)。
    ShareDetach,
    InvalidateAssets { generation: u64 },
    Shutdown,
}

/// 与 render_scene_frame 的八个参数一一对应;只借用、不 clone。
#[derive(Clone, Copy)]
pub struct FrameInput<'a> {
    pub scene: &'a Scene,
    pub cam: &'a EditorCamera,
    pub selected: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub want_readback: bool,
    pub want_stats: bool,
    pub vp_override: Option<M4>,
}

static BACKEND: OnceLock<Box<dyn RenderBackend>> = OnceLock::new();

/// start_core(§5.3)在起任何线程之前调用一次;重复安装返回 Err(I10)。
pub fn install(b: Box<dyn RenderBackend>) -> Result<(), String> {
    BACKEND.set(b).map_err(|_| "render backend already installed".to_string())
}

/// 进程内唯一的渲染后端(I10)。未安装时惰性装 RurixBackend:直接调 rpc::dispatch 的测试零改动。
#[cfg(feature = "backend-rurix")]
pub fn backend() -> &'static dyn RenderBackend {
    BACKEND.get_or_init(|| Box::new(crate::render::rurix::RurixBackend::new())).as_ref()
}

/// 无 backend-rurix(godot-host 以 default-features = false 依赖核心)时没有可回退的后端:
/// 必须先经 start_core 安装;否则明确 panic,而不是静默给出一个不存在的渲染腿。
#[cfg(not(feature = "backend-rurix"))]
pub fn backend() -> &'static dyn RenderBackend {
    match BACKEND.get() {
        Some(b) => b.as_ref(),
        None => panic!("RENDER_BACKEND_NOT_INSTALLED: 编译时未启用 backend-rurix,须先经 start_core(CoreConfig.backend)安装渲染后端"),
    }
}
