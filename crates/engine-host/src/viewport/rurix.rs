//! viewport 的 rurix 渲染腿(02 §5.2,feature `backend-rurix`):DeviceFrameSession 会话、WGSL→SPIR-V、
//! 槽位 / UBO / push constants 打包、render_scene_frame。自 viewport.rs 整段搬来,函数体逐字不变;
//! 导入与核心类型(FramePixels、render_core 再导出)经 `use super::*` 取自父模块。视口测试仍在父模块,
//! 它们要用的几项升为 pub(super)。

use super::*;

// ─────────────────────────── 向量/矩阵(手卷 f32,确定性) ───────────────────────────

/// 行主序 → 列主序字节(mat4x4<f32> UBO/push 内存布局)。
pub(crate) fn m4_col_bytes(m: M4) -> [u8; 64] {
    let mut out = [0u8; 64];
    for c in 0..4 {
        for r in 0..4 {
            let o = (c * 4 + r) * 4;
            out[o..o + 4].copy_from_slice(&m[r][c].to_le_bytes());
        }
    }
    out
}

/// 贴图精灵槽 push constants 字节数(F-GAME-4:model 64 + tint 16 + tex_size 8 +
/// flip flags 8 + uv_rect 16 = 112 ≤ 128 上限)。
pub(super) const SPRITE_PC_LEN: usize = 128;

pub(super) fn sprite_blend(e: &forge_scene::Entity) -> rex::BlendMode {
    crate::render_core::sprite::blend(e).into()
}

impl From<SpriteBlend> for rex::BlendMode {
    fn from(b: SpriteBlend) -> Self {
        match b {
            SpriteBlend::Opaque => rex::BlendMode::Opaque,
            SpriteBlend::Alpha => rex::BlendMode::Alpha,
            SpriteBlend::Additive => rex::BlendMode::Additive,
        }
    }
}

// A stable draw graph for pooled RGBA scenes. All additive effects must follow alpha layers.
const STABLE_ALPHA_SLOTS: usize = 192;
pub(super) fn stable_rgba_inventory(scene: &Scene) -> Option<Vec<&'static TexGpu>> {
    if !scene.is_2d() { return None; }
    let mut textures: Vec<&'static TexGpu> = Vec::new();
    let mut max_alpha = f64::NEG_INFINITY;
    let mut min_additive = f64::INFINITY;
    let mut plane = None;
    let mut seen_variants = std::collections::HashSet::new();
    let project_root = crate::rpc::project_root();
    for e in scene.entities.iter().filter(|e| is_renderable(e)) {
        let sprite = sprite_component(e)?;
        if sprite.props.get("material").and_then(|v|v.as_str()).is_some_and(|v|!v.is_empty()){return None;}
        if !crate::render_core::sprite::chroma_none(sprite) { return None; }
        if plane.is_some_and(|z| z != e.transform.translation[2]) { return None; }
        plane = Some(e.transform.translation[2]);
        match sprite_blend(e) {
            rex::BlendMode::Alpha => max_alpha = max_alpha.max(sprite_sorting_order(e)),
            rex::BlendMode::Additive => min_additive = min_additive.min(sprite_sorting_order(e)),
            rex::BlendMode::Opaque | rex::BlendMode::AlphaDepth => {
                // On one XY plane, opaque output with alpha 1 is equivalent in
                // the alpha PSO. Preserve the legacy shader's alpha-cutout rule.
                let alpha = sprite.props.get("tint").and_then(Value::as_array)
                    .and_then(|v| v.get(3)).and_then(Value::as_f64).unwrap_or(1.);
                if alpha != 1. { return None; }
                max_alpha = max_alpha.max(sprite_sorting_order(e));
            }
        }
        // All atlas variants must be resident before the first state transition.
        // A change of frame must not rebuild the GPU session or upload a texture.
        if let Some(variants) = sprite.props.get("spriteVariants").and_then(Value::as_array) {
            for guid in variants.iter().filter_map(Value::as_str) {
                if !seen_variants.insert(guid) { continue; }
                let doc = sprite_doc_cached(guid)?;
                let tex = load_tex_static_cached(&project_root, &doc.texture)?;
                if !textures.iter().any(|t| std::ptr::eq(*t, tex)) { textures.push(tex); }
            }
        }
        // Variant order must be independent of the currently selected frame.
        // Inserting that frame's texture first would reorder the resident set
        // when an impact changes type and needlessly rebuild the GPU session.
        let texture = resolve_sprite_render(sprite)?.tex;
        if !textures.iter().any(|t| std::ptr::eq(*t, texture)) { textures.push(texture); }
    }
    if textures.is_empty() || min_additive <= max_alpha { None } else { Some(textures) }
}

/// 解析 Sprite 组件当前帧(F-GAME-4)。中立实现与完整说明在 `render_core::sprite`;这里是 rurix 侧包装,
/// 项目根仍取 `rpc::project_root()`(搬迁前该函数在内部取的是同一个值)。
pub(crate) fn resolve_sprite_render(c: &forge_scene::Component) -> Option<SpriteRenderInfo> {
    crate::render_core::sprite::resolve_sprite_render(c, &crate::rpc::project_root())
}

// ─────────────────────────── 立方体网格与着色器 ───────────────────────────

/// ── 贴图精灵管线(F-GAME-2:2D 游戏「真实画面」腿) ──
/// 每个带材质 albedo 的实体 = 一张朝 +z 的单位四边形,逐槽绑定 albedo 纹理采样;
/// 品红底色键 discard(生成素材的 chroma-key 约定)。

/// 单位四边形(朝 +z;pos+normal+uv;2 三角)。
fn quad_mesh_bytes() -> &'static [u8] {
    static MESH: OnceLock<&'static [u8]> = OnceLock::new();
    MESH.get_or_init(|| {
        let corners: [[f32; 2]; 4] = [
            [-0.5, -0.5],
            [0.5, -0.5],
            [0.5, 0.5],
            [-0.5, 0.5],
        ];
        let uvs: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        let n = [0.0, 0.0, 1.0];
        let mut bytes = Vec::with_capacity(6 * 32);
        let mut push = |c: [f32; 2], uv: [f32; 2]| {
            for f in [c[0], c[1], 0.0, n[0], n[1], n[2], uv[0], uv[1]] {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
        };
        for i in [0usize, 1, 2, 0, 2, 3] {
            push(corners[i], uvs[i]);
        }
        Box::leak(bytes.into_boxed_slice())
    })
}

/// 贴图精灵顶点布局:pos(0) + normal(12) + uv(24),stride 32。
/// R32G32_SFLOAT = VK_FORMAT_R32G32_SFLOAT(103;上游 render_exec TexFormat 同表)。
const R32G32_SFLOAT: u32 = 103;
const VERTEX_ATTRS_TEX: [(u32, u32, u32); 3] = [
    (0, R32G32B32_SFLOAT, 0),
    (1, R32G32B32_SFLOAT, 12),
    (2, R32G32_SFLOAT, 24),
];
const QUAD_STRIDE: u32 = 32;

const VS_TEX_WGSL: &str = r#"
struct CameraUbo { view_proj: mat4x4<f32>, };
// albedo 走 storage buffer texel 数组:buffer 初始数据上传是 cube VB 同款已验证
// 路径;image sampled/storage 两路在本执行器实测读零,弃用(F-GAME-2)。
@group(0) @binding(0) var<storage, read> albedo_texels: array<u32>;
@group(0) @binding(1) var<uniform> u_cam: CameraUbo;
// F-GAME-3:flags = (flipX, flipY)——Sprite 组件 UV 镜像(0/1)。
// F-GAME-4:uv_rect = 图集子矩形(offset.xy + scale.zw,0..1 贴图空间)——帧动画的
// 每帧可变通道(push constants 112B ≤ 128B 上限);整图模式恒 [0,0,1,1]。
struct PushConsts { model: mat4x4<f32>, color: vec4<f32>, tex_size: vec2<u32>, flags: vec2<f32>, uv_rect: vec4<f32>, compositing: vec4<f32>, };
var<push_constant> pc: PushConsts;
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
    @location(1) uv: vec2<f32>,
};
@vertex
fn main(@location(0) pos: vec3<f32>, @location(1) nrm: vec3<f32>, @location(2) uv: vec2<f32>) -> VsOut {
    var o: VsOut;
    o.clip = u_cam.view_proj * (pc.model * vec4<f32>(pos, 1.0));
    o.nrm = (pc.model * vec4<f32>(nrm, 0.0)).xyz;
    // 先在帧内翻转,再映射进图集子矩形(flip 语义 = 帧内镜像,不跨帧)。
    let fu = vec2<f32>(mix(uv.x, 1.0 - uv.x, pc.flags.x), mix(uv.y, 1.0 - uv.y, pc.flags.y));
    o.uv = pc.uv_rect.xy + fu * pc.uv_rect.zw;
    return o;
}
"#;

const FS_TEX_WGSL: &str = r#"
struct PushConsts { model: mat4x4<f32>, color: vec4<f32>, tex_size: vec2<u32>, flags: vec2<f32>, uv_rect: vec4<f32>, compositing: vec4<f32>, };
var<push_constant> pc: PushConsts;
@group(0) @binding(0) var<storage, read> albedo_texels: array<u32>;
// RGBA8 texel → vec4(小端:byte0=R 落在 u32 最低字节)。
fn texel_at(x: i32, y: i32) -> vec4<f32> {
    let idx = y * i32(pc.tex_size.x) + x;
    let p = albedo_texels[idx];
    return vec4<f32>(
        f32((p) & 0xffu) / 255.0,
        f32((p >> 8u) & 0xffu) / 255.0,
        f32((p >> 16u) & 0xffu) / 255.0,
        f32((p >> 24u) & 0xffu) / 255.0,
    );
}
@fragment
fn main(@location(0) nrm: vec3<f32>, @location(1) uv: vec2<f32>) -> @location(0) vec4<f32> {
    // F-GAME-4:采样钳制在 uv_rect 子矩形内(帧边界插值恰达上界时防越入邻帧 1px);
    // 整图模式 rect=[0,0,1,1] 与旧行为逐 texel 一致。
    let w = f32(pc.tex_size.x);
    let h = f32(pc.tex_size.y);
    let x = i32(clamp(uv.x * w, max(pc.uv_rect.x * w, 0.0), min((pc.uv_rect.x + pc.uv_rect.z) * w, w) - 1.0));
    let y = i32(clamp(uv.y * h, max(pc.uv_rect.y * h, 0.0), min((pc.uv_rect.y + pc.uv_rect.w) * h, h) - 1.0));
    let texel = texel_at(x, y);
    if (texel.a <= 0.0 || (pc.compositing.y < 0.5 && texel.a < 0.02)) { discard; }
    // 品红族色键(生成精灵 chroma-key 背景及渐变边):G 显著低于 R/B 两者即弃。
    // 素材里绿身(g 高)/蓝天(r 低)/红屋(b 低)/黄瓣(g 高)均不受误伤。
    let g_dom = 0.5 * min(texel.r, texel.b);
    if (pc.compositing.x > 0.5 && texel.g < g_dom) { discard; }
    // F-GAME-3:pc.color = Sprite.tint(旧贴图 quad 路径恒推白色,行为不变)。
    let alpha = mix(pc.color.a, texel.a * pc.color.a, pc.compositing.y);
    return vec4<f32>(texel.rgb * pc.color.rgb, alpha);
}
"#;

pub(super) fn shader_tex_bytes() -> Result<(&'static [u8], &'static [u8]), String> {
    static SHADERS: OnceLock<Result<(&'static [u8], &'static [u8]), String>> = OnceLock::new();
    SHADERS
        .get_or_init(|| Ok((compile_wgsl(VS_TEX_WGSL, "vs_tex")?, compile_wgsl(FS_TEX_WGSL, "fs_tex")?)))
        .clone()
}

const VS_WGSL: &str = r#"
struct CameraUbo { view_proj: mat4x4<f32>, };
@group(0) @binding(0) var<uniform> u_cam: CameraUbo;
struct PushConsts { model: mat4x4<f32>, color: vec4<f32>, };
var<push_constant> pc: PushConsts;
struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
};
@vertex
fn main(@location(0) pos: vec3<f32>, @location(1) nrm: vec3<f32>) -> VsOut {
    var o: VsOut;
    o.clip = u_cam.view_proj * (pc.model * vec4<f32>(pos, 1.0));
    o.nrm = (pc.model * vec4<f32>(nrm, 0.0)).xyz;
    return o;
}
"#;

const FS_WGSL: &str = r#"
struct PushConsts { model: mat4x4<f32>, color: vec4<f32>, };
var<push_constant> pc: PushConsts;
@fragment
fn main(@location(0) nrm: vec3<f32>) -> @location(0) vec4<f32> {
    let l = normalize(vec3<f32>(0.45, 0.75, 0.35));
    let ndl = clamp(dot(normalize(nrm), l), 0.0, 1.0);
    let rgb = pc.color.rgb * (0.28 + 0.72 * ndl);
    return vec4<f32>(rgb, 1.0);
}
"#;

/// 零拷贝 pack(共享体 buffer 形态):色 attachment(storage image)→ 共享 SSBO,
/// 按 D3D12 `CopyTextureRegion` 的 PLACED_FOOTPRINT 契约以 256B 对齐行距逐像素打包 RGBA8。
/// 绑定序沿 render_exec set0 固定约定:storage_buffers 在前(binding 0),
/// storage_images 次之(binding 1)——sampled_images 是 COMBINED_IMAGE_SAMPLER,
/// 与 naga 产出的分离式绑定不兼容,故读色附件走 storage image。
const PACK_WGSL: &str = r#"
struct PackPc { width: u32, height: u32, row_words: u32, pad: u32, };
var<push_constant> pc: PackPc;
@group(0) @binding(0) var<storage, read_write> dst: array<u32>;
@group(0) @binding(1) var src: texture_storage_2d<rgba8unorm, read>;
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= pc.width || gid.y >= pc.height) { return; }
    let c = textureLoad(src, vec2<i32>(i32(gid.x), i32(gid.y)));
    let r = u32(clamp(c.r, 0.0, 1.0) * 255.0 + 0.5);
    let g = u32(clamp(c.g, 0.0, 1.0) * 255.0 + 0.5);
    let b = u32(clamp(c.b, 0.0, 1.0) * 255.0 + 0.5);
    let a = u32(clamp(c.a, 0.0, 1.0) * 255.0 + 0.5);
    dst[gid.y * pc.row_words + gid.x] = r | (g << 8u) | (b << 16u) | (a << 24u);
}
"#;

/// WGSL → SPIR-V(naga 纯 Rust;lang_version 1.3 与 render_exec 一致)。
pub(crate) fn compile_wgsl(src: &str, stage: &str) -> Result<&'static [u8], String> {
    let module =
        naga::front::wgsl::parse_str(src).map_err(|e| format!("{stage} wgsl 解析失败: {e}"))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty() | naga::valid::Capabilities::PUSH_CONSTANT,
    )
    .validate(&module)
    .map_err(|e| format!("{stage} wgsl 校验失败: {e}"))?;
    let mut opts = naga::back::spv::Options::default();
    opts.lang_version = (1, 3);
    let words = naga::back::spv::write_vec(&module, &info, &opts, None)
        .map_err(|e| format!("{stage} spirv 产出失败: {e}"))?;
    let mut bytes = Vec::with_capacity(words.len() * 4);
    for w in words {
        bytes.extend_from_slice(&w.to_le_bytes());
    }
    Ok(Box::leak(bytes.into_boxed_slice()))
}

/// 光栅着色器对字节(启动期一次,OnceLock 缓存)。
pub(super) fn shader_bytes() -> Result<(&'static [u8], &'static [u8]), String> {
    static SHADERS: OnceLock<Result<(&'static [u8], &'static [u8]), String>> = OnceLock::new();
    SHADERS
        .get_or_init(|| Ok((compile_wgsl(VS_WGSL, "vs")?, compile_wgsl(FS_WGSL, "fs")?)))
        .clone()
}

/// 零拷贝 pack compute 着色器字节(仅共享档用;OnceLock 缓存)。
pub(super) fn pack_shader_bytes() -> Result<&'static [u8], String> {
    static PACK: OnceLock<Result<&'static [u8], String>> = OnceLock::new();
    PACK.get_or_init(|| compile_wgsl(PACK_WGSL, "pack")).clone()
}

// ─────────────────────────── 渲染会话(DeviceFrameSession) ───────────────────────────

/// 固定 draw pass 槽数上限(实体上限;超出截断并如实报 truncated)。
pub(crate) const MAX_DRAW_SLOTS: usize = 256;

/// F6 wave.5 动态档位:pass 图槽数按可渲染实体数取档(48/96/128),
/// 避免小场景为空槽 pass 付全量重录/draw 代价(128 空槽实测 1080p ~22ms/帧大头)。
/// 档位只升不降(滞后:实体数回落不重建,避免抖动);超 128 仍截断。
pub(super) fn slot_tier(renderable: usize) -> usize {
    // 档位必须覆盖当帧实体数；此前阈值 48→32 / 96→64 会主动截掉合法精灵，
    // 导致 PvZ 关卡背景/单位消失。保持有限档位但保证 tier >= renderable。
    if renderable <= 24 {
        24
    } else if renderable <= 32 {
        32
    } else if renderable <= 64 {
        64
    } else if renderable <= 96 {
        96
    } else if renderable <= 128 {
        128
    } else if renderable <= 192 {
        192
    } else {
        MAX_DRAW_SLOTS
    }
}
/// 清屏底色(cursor+Claude 系深色;RGBA8 ≈ [23,24,29,255])。
const CLEAR_RGBA: [f32; 4] = crate::render_core::list::SPRITE_MESH_CLEAR_RGBA; // 值不变:[0.090, 0.094, 0.114, 1.0]
const R32G32B32_SFLOAT: u32 = 109;
/// 顶点属性:(location, format, offset)。
const VERTEX_ATTRS: [(u32, u32, u32); 2] = [(0, R32G32B32_SFLOAT, 0), (1, R32G32B32_SFLOAT, 12)];

enum RendererState {
    Uninit,
    Ready(ViewportRenderer),
    /// 诚实降级原因(loader 缺失/能力缺失/会话创建失败)。
    Degraded(String),
}

pub(super) struct ViewportRenderer {
    width: u32,
    height: u32,
    /// 本会话 pass 图槽数(F6 wave.5 动态档;升档触发重建)。
    slots: usize,
    /// 本会话槽位→网格类签名(网格类布局变化触发重建)。
    mesh_sig: u64,
    pub(super) session: rex::DeviceFrameSession<'static>,
    device_name: String,
    /// 本会话 import 的共享纹理键(nt_handle, alloc_size);None = 纯 readback 腿。
    /// share 重建(尺寸协商)后键值变化 → 会话重建。
    import_key: Option<(u64, u64)>,
    particles: Option<crate::gpu_particles::ParticlePasses>,
    texture_resources: Vec<(&'static TexGpu, u32)>,
    graph_resources: Vec<Option<u32>>,
    _graph: SessionGraph,
}

// The session field is dropped before this owner, including failed candidate builds.
struct SessionGraph {
    resources: *mut [rex::ResourceDesc<'static>], passes: *mut [rex::Pass<'static>],
    plans: *mut [Vec<(u32,rex::TargetState)>], barriers: *mut [&'static [(u32,rex::TargetState)]],
    readbacks: *mut [rex::Readback], _programs: Vec<std::sync::Arc<crate::shader::Program>>,
}
impl Drop for SessionGraph {fn drop(&mut self){unsafe{drop(Box::from_raw(self.resources));drop(Box::from_raw(self.passes));drop(Box::from_raw(self.barriers));drop(Box::from_raw(self.plans));drop(Box::from_raw(self.readbacks));}}}
// SAFETY:`DeviceFrameSession` 内含 *mut c_void(VkDevice 等原生句柄)被保守标 !Send。
// 本渲染器仅经 `RENDERER: Mutex` 互斥访问——任一时刻单线程持有,满足 Vulkan「外部同步」
// 线程模型(单 queue 提交者);原生句柄进程内全线程有效,不存在无同步的跨线程并发。
unsafe impl Send for ViewportRenderer {}

static RENDERER: OnceLock<Mutex<RendererState>> = OnceLock::new();

/// invalidate_assets 的 GPU 段(原函数第二句):rurix 视口会话下一帧重建。
pub(super) fn reset_renderer() {
    if let Some(r)=RENDERER.get(){*r.lock().unwrap()=RendererState::Uninit;}
}

/// 诊断:本帧是否发生了会话重建(性能定位用)。
pub(super) static REBUILD_FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 最近一次会话重建时刻(重建防抖用)。
static LAST_REBUILD: Mutex<Option<std::time::Instant>> = Mutex::new(None);
/// D-045:viewport.frame{exact:true} 期间置位——纯尺寸重建不押后,保证回读尺寸 = 请求尺寸
/// (设计稿复刻验收要逐像素对齐;代价是一次会话重建,调用方知情)。
pub(crate) static FORCE_EXACT_SIZE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn renderer_slot() -> &'static Mutex<RendererState> {
    RENDERER.get_or_init(|| Mutex::new(RendererState::Uninit))
}

/// 共享 buffer 导入面(buffer 形态零拷贝):D3D12 `D3D12_HEAP_FLAG_SHARED` 线性
/// buffer 的 NT handle + 字节数 + 256B 对齐行距。
///
/// 之所以共享体是 buffer 而非纹理:两侧对同一张纹理的行/高补齐规则不同
/// (960×540 实测 VK 需 2,457,600B、D3D12 committed 只给 2,228,224B),
/// 尺寸不匹配会让未绑定图像参与渲染直至 `VK_ERROR_DEVICE_LOST`;线性 buffer
/// 两侧字节数逐字一致,无歧义(与上游 fsr 驻留车道同形态)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShareImport {
    handle: u64,
    size: u64,
    row_pitch: u32,
}

/// 当前应 import 的共享 buffer:share 已开且尺寸与本帧一致时返回;
/// 否则 None(纯 readback 腿)。非 Windows 恒 None。
fn current_import(width: u32, height: u32) -> Option<ShareImport> {
    #[cfg(windows)]
    {
        crate::share::vk_import_info()
            .filter(|i| i.width == width && i.height == height)
            .map(|i| ShareImport {
                handle: i.handle,
                size: i.size,
                row_pitch: i.row_pitch,
            })
    }
    #[cfg(not(windows))]
    {
        let _ = (width, height);
        None
    }
}

/// 建固定 pass 图会话(资源:0=内置 cube VB,1=相机 UBO,2=色 attachment,3=深度,
/// 4..=各网格类 VB,随后=零拷贝共享 SSBO)。零拷贝优先:有共享纹理可 import 时先建
/// import 会话;失败(如无 external memory 扩展)如实 eprintln 并回退纯 readback 腿
/// (G-F1-10 证据面能区分两档)。
fn build_session(
    width: u32,
    height: u32,
    slots: usize,
    slot_mesh: &[Option<&'static MeshGpu>],
    slot_tex: &[Option<&'static TexGpu>],
    slot_blend: &[rex::BlendMode],
    slot_graph: &[Option<std::sync::Arc<crate::shader::Material>>],
    extra_tex: &[&'static TexGpu],
    mesh_sig: u64,
) -> Result<ViewportRenderer, String> {
    let import = current_import(width, height);
    match build_session_graph(width, height, import, slots, slot_mesh, slot_tex, slot_blend, slot_graph, extra_tex, mesh_sig) {
        Ok(r) => Ok(r),
        Err(e) if import.is_some() => {
            eprintln!("[viewport] 零拷贝 import 会话创建失败,回退 readback 上传腿: {e}");
            build_session_graph(width, height, None, slots, slot_mesh, slot_tex, slot_blend, slot_graph, extra_tex, mesh_sig)
        }
        Err(e) => Err(e),
    }
}

pub(super) fn build_session_with(width:u32,height:u32,import:Option<ShareImport>,slots:usize,slot_mesh:&[Option<&'static MeshGpu>],slot_tex:&[Option<&'static TexGpu>],slot_blend:&[rex::BlendMode],extra_tex:&[&'static TexGpu],mesh_sig:u64)->Result<ViewportRenderer,String>{
    build_session_graph(width,height,import,slots,slot_mesh,slot_tex,slot_blend,&vec![None;slots],extra_tex,mesh_sig)
}
fn build_session_graph(
    width: u32,
    height: u32,
    import: Option<ShareImport>,
    slots: usize,
    slot_mesh: &[Option<&'static MeshGpu>],
    slot_tex: &[Option<&'static TexGpu>],
    slot_blend: &[rex::BlendMode],
    slot_graph: &[Option<std::sync::Arc<crate::shader::Material>>],
    extra_tex: &[&'static TexGpu],
    mesh_sig: u64,
) -> Result<ViewportRenderer, String> {
    assert_eq!(slot_mesh.len(), slots, "槽位→网格类表长度须等于槽数");
    assert_eq!(slot_tex.len(), slots, "槽位→贴图表长度须等于槽数");
    if !vk::vulkan_available() {
        return Err("DEV_ENV_DEGRADE: vulkan loader 不可用(无 GPU/驱动)".to_owned());
    }
    let caps = rex::probe_device_caps().map_err(|e| format!("DEV_ENV_DEGRADE: 设备探测失败: {e}"))?;
    if !caps.synchronization2 {
        return Err(format!(
            "DEV_ENV_DEGRADE: 设备缺 synchronization2({})",
            caps.device_name
        ));
    }
    let (vs, fs) = shader_bytes()?;
    let (vs_tex, fs_tex) = shader_tex_bytes()?;
    let cube = cube_mesh_bytes();

    // 槽位→网格类 VB 资源下标:cube 恒 res 0;不同网格按首现序 4..;共享 SSBO 随后。
    // (类布局由调用方按当帧实体计数定案;此处纯映射,重复 Arc 共享同一 VB 资源。)
    let mut distinct: Vec<(&'static MeshGpu, u32)> = Vec::new(); // (mesh, res)
    for s in slot_mesh {
        if let Some(m) = s {
            if !distinct.iter().any(|(d, _)| std::ptr::eq(*d, *m)) {
                distinct.push((m, 4 + distinct.len() as u32));
            }
        }
    }
    // import_res 在下方资源表构建后定案(纹理资源插入其间)。

    // 资源:0=cube VB / 1=相机 UBO / 2=色 attachment / 3=深度 / 4..=网格类 VB /(零拷贝档)共享 SSBO。
    let mut resources: Vec<rex::ResourceDesc> = vec![
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: cube.len() as u64,
            usage: rex::BufferUsage {
                vertex: true,
                ..Default::default()
            },
            data: Some(cube),
            device_local: false,
        }),
        rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: 64,
            usage: rex::BufferUsage {
                uniform: true,
                ..Default::default()
            },
            data: None,
            // FrameUpdate::buffer_uploads 目标须 host-visible(上游校验期 fail-closed)。
            device_local: false,
        }),
        rex::ResourceDesc::Texture(rex::TextureDesc {
            width,
            height,
            format: rex::TexFormat::Rgba8Unorm,
            usage: rex::TextureUsage {
                color: true,
                // 零拷贝档色附件兼作 pack pass 的 storage image 源。
                storage: import.is_some(),
                ..Default::default()
            },
            data: None,
        }),
        rex::ResourceDesc::Texture(rex::TextureDesc {
            width,
            height,
            format: rex::TexFormat::Depth32Float,
            usage: rex::TextureUsage {
                depth: true,
                ..Default::default()
            },
            data: None,
        }),
    ];
    for (m, _) in &distinct {
        resources.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: m.bytes.len() as u64,
            usage: rex::BufferUsage {
                vertex: true,
                ..Default::default()
            },
            data: Some(&m.bytes),
            device_local: false,
        }));
    }
    // 贴图精灵腿:四边形 VB(随后的 tex 资源表;F-GAME-2)。
    let quad = quad_mesh_bytes();
    let quad_res = (4 + distinct.len()) as u32;
    resources.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
        size: quad.len() as u64,
        usage: rex::BufferUsage {
            vertex: true,
            ..Default::default()
        },
        data: Some(quad),
        device_local: false,
    }));
    // 不同 albedo 贴图各一资源(首现序),跨槽复用同一资源下标。
    let mut distinct_tex: Vec<&'static TexGpu> = Vec::new();
    for t in slot_tex.iter().flatten().chain(extra_tex.iter()) {
        if !distinct_tex
            .iter()
            .any(|x| std::ptr::eq(*x, *t))
        {
            distinct_tex.push(t);
        }
    }
    for t in &distinct_tex {
        resources.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: t.rgba.len() as u64,
            usage: rex::BufferUsage {
                storage: true,
                ..Default::default()
            },
            data: Some(t.rgba),
            // Immutable atlas pixels upload once through Rurix staging; keep them
            // in device memory instead of sampling large V2 atlases across PCIe.
            device_local: true,
        }));
    }
    let tex_res = |t: &'static TexGpu| -> u32 {
        (5 + distinct.len()) as u32
            + distinct_tex
                .iter()
                .position(|x| std::ptr::eq(*x, t))
                .unwrap_or(0) as u32
    };
    if let Some(im) = import {
        // 上游 imported 集强制 data=None + device_local。
        resources.push(rex::ResourceDesc::Buffer(rex::BufferDesc {
            size: im.size,
            usage: rex::BufferUsage {
                storage: true,
                ..Default::default()
            },
            data: None,
            device_local: true,
        }));
    }
    let import_res = (5 + distinct.len() + distinct_tex.len()) as u32;

    let mut graph_resources=vec![None;slots];
    for (i,graph) in slot_graph.iter().enumerate(){if let (Some(graph),Some(source))=(graph,slot_tex[i]){
        let data=graph.pack(&crate::shader::Texture{width:source.w,height:source.h,rgba:std::sync::Arc::new(source.rgba.to_vec())})?;
        graph_resources[i]=Some(resources.len()as u32);
        resources.push(rex::ResourceDesc::Buffer(rex::BufferDesc{size:data.len()as u64,usage:rex::BufferUsage{storage:true,..Default::default()},data:None,device_local:false}));
    }}
    // 消隐槽模型:远埋 + 微缩(全有限值,规避 NaN 顶点未定义光栅化)。
    let hidden_model = trs_model(&Transform {
        translation: [0.0, -1000.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1e-6, 1e-6, 1e-6],
    });
    let mut hidden_pc = Vec::with_capacity(80);
    hidden_pc.extend_from_slice(&m4_col_bytes(hidden_model));
    hidden_pc.extend_from_slice(&[0u8; 16]);
    // 贴图槽消隐 push constants(112B:model+color+tex_size+flip flags+uv_rect,F-GAME-4)。
    let mut hidden_pc_tex = Vec::with_capacity(SPRITE_PC_LEN);
    hidden_pc_tex.extend_from_slice(&m4_col_bytes(hidden_model));
    hidden_pc_tex.extend_from_slice(&[0u8; 16]);
    hidden_pc_tex.extend_from_slice(&1u32.to_le_bytes());
    hidden_pc_tex.extend_from_slice(&1u32.to_le_bytes());
    hidden_pc_tex.extend_from_slice(&0f32.to_le_bytes());
    hidden_pc_tex.extend_from_slice(&0f32.to_le_bytes());
    for f in FULL_UV_RECT {
        hidden_pc_tex.extend_from_slice(&f.to_le_bytes());
    }
    for f in [1.0f32, 0., 0., 0.] {
        hidden_pc_tex.extend_from_slice(&f.to_le_bytes());
    }

    let mut passes: Vec<rex::Pass> = Vec::with_capacity(slots);
    let mut barrier_plan: Vec<Vec<(u32, rex::TargetState)>> = Vec::with_capacity(slots);
    for (k, s) in slot_mesh.iter().enumerate() {
        let first = k == 0;
        if let Some(t) = slot_tex[k] {
            // 贴图精灵槽:四边形 + albedo 采样(品红色键在 FS 内 discard)。
            passes.push(rex::Pass::Raster(rex::RasterPass {
                blend: slot_blend[k],
                name: "forge_viewport_sprite",
                vs_spirv: vs_tex,
                fs_spirv: if let Some(g)=&slot_graph[k]{unsafe{std::slice::from_raw_parts(g.program.sprite_spirv.as_ptr(),g.program.sprite_spirv.len())}}else{fs_tex},
                vertex: rex::VertexData::Resource {
                    res: quad_res,
                    offset: 0,
                    stride: QUAD_STRIDE,
                    attrs: &VERTEX_ATTRS_TEX,
                },
                draw: rex::DrawSpec::Direct {
                    vertex_count: 6,
                    instance_count: 1,
                    first_vertex: 0,
                    first_instance: 0,
                },
                colors: vec![rex::ColorAttachmentRef {
                    res: 2,
                    clear: if first { Some(CLEAR_RGBA) } else { None },
                }],
                depth: Some(rex::DepthAttachmentRef {
                    res: 3,
                    clear: if first { Some(1.0) } else { None },
                }),
                viewport: None,
                bindings: rex::Bindings {
                    uniform: Some(rex::UniformRef {
                        res: 1,
                        offset: 0,
                        size: 64,
                    }),
                    storage_buffers: vec![graph_resources[k].unwrap_or_else(||tex_res(t))],
                    push_constants: hidden_pc_tex.clone(),
                    ..Default::default()
                },
                conservative: None,
            }));
            barrier_plan.push(vec![
                (2, rex::TargetState::ColorAttachmentWrite),
                (3, rex::TargetState::DepthAttachmentWrite),
            ]);
            continue;
        }
        let (vb_res, vertex_count) = match s {
            None => (0u32, 36u32),
            Some(m) => (
                distinct.iter().find(|(d, _)| std::ptr::eq(*d, *m)).map(|(_, r)| *r).unwrap_or(0),
                m.vertex_count,
            ),
        };
        let first = k == 0;
        passes.push(rex::Pass::Raster(rex::RasterPass {
            blend: rex::BlendMode::Opaque,
            name: "forge_viewport_entity",
            vs_spirv: vs,
            fs_spirv: fs,
            vertex: rex::VertexData::Resource {
                res: vb_res,
                offset: 0,
                stride: 24,
                attrs: &VERTEX_ATTRS,
            },
            draw: rex::DrawSpec::Direct {
                vertex_count,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
            },
            colors: vec![rex::ColorAttachmentRef {
                res: 2,
                clear: if first { Some(CLEAR_RGBA) } else { None },
            }],
            depth: Some(rex::DepthAttachmentRef {
                res: 3,
                clear: if first { Some(1.0) } else { None },
            }),
            viewport: None,
            bindings: rex::Bindings {
                uniform: Some(rex::UniformRef {
                    res: 1,
                    offset: 0,
                    size: 64,
                }),
                push_constants: hidden_pc.clone(),
                ..Default::default()
            },
            conservative: None,
        }));
        barrier_plan.push(vec![
            (2, rex::TargetState::ColorAttachmentWrite),
            (3, rex::TargetState::DepthAttachmentWrite),
        ]);
    }
    let particles = if crate::gpu_particles::enabled() {
        Some(crate::gpu_particles::append(&mut resources, &mut passes, &mut barrier_plan, 1, 2, None)?)
    } else { None };
    // 零拷贝档追加 pack pass:色附件 → 共享 SSBO(帧图内无 copy/blit pass,
    // 搬运只能经 compute;帧末上游自动追加 EXTERNAL release,D3D12 侧据本帧 fence 消费)。
    if let Some(im) = import {
        let pack = pack_shader_bytes()?;
        let mut pc = Vec::with_capacity(16);
        for v in [width, height, im.row_pitch / 4, 0u32] {
            pc.extend_from_slice(&v.to_le_bytes());
        }
        passes.push(rex::Pass::Compute(rex::ComputePass {
            name: "forge_viewport_pack",
            spirv: pack,
            entry: None,
            dispatch: rex::DispatchSpec::Direct([width.div_ceil(8), height.div_ceil(8), 1]),
            bindings: rex::Bindings {
                storage_buffers: vec![import_res],
                storage_images: vec![2],
                push_constants: pc,
                ..Default::default()
            },
        }));
        barrier_plan.push(vec![
            (2, rex::TargetState::StorageImageReadWrite),
            (import_res, rex::TargetState::StorageReadWrite),
        ]);
    }

    let readbacks: Vec<rex::Readback> = vec![rex::Readback::Texture { res: 2 }];

    // session 借用上述描述块:'static 提升;重建仅发生在改尺寸,有界。
    let resources = Box::leak(resources.into_boxed_slice());
    let passes = Box::leak(passes.into_boxed_slice());
    let barrier_plan = Box::leak(barrier_plan.into_boxed_slice());
    let plans_ptr=barrier_plan as *mut [_];
    let barriers: Vec<&[(u32, rex::TargetState)]> =
        barrier_plan.iter().map(Vec::as_slice).collect();
    let barriers = Box::leak(barriers.into_boxed_slice());
    let readbacks = Box::leak(readbacks.into_boxed_slice());

    let graph_owner=SessionGraph{resources,passes,plans:plans_ptr,barriers,readbacks,_programs:slot_graph.iter().flatten().map(|g|g.program.clone()).collect()};
    let session = match import {
        // 共享 buffer 以 D3D12_RESOURCE 反向导入(资源下标 import_res ↔ NT handle 地址值)。
        Some(im) => rex::DeviceFrameSession::new_with_imported_d3d12_textures(
            resources,
            passes,
            barriers,
            readbacks,
            2,
            &[],
            &[],
            &[(import_res, im.handle as usize)],
        )
        .map_err(|e| format!("零拷贝会话创建失败: {e}"))?,
        None => rex::DeviceFrameSession::new(resources, passes, barriers, readbacks, 2)
            .map_err(|e| format!("DEV_ENV_DEGRADE: 渲染会话创建失败: {e}"))?,
    };

    // LUID 对拍由调用方负责(上游契约):跨 adapter 不可共享显存。不匹配即 Err,
    // 由 build_session 收口回退到 readback 腿,绝不带着错 adapter 继续跑。
    #[cfg(windows)]
    if import.is_some() {
        let vk_luid = session
            .physical_device_luid()
            .ok_or_else(|| "零拷贝会话:deviceLUIDValid=false,无法与 D3D12 adapter 对拍".to_string())?;
        let d3d_luid = crate::share::adapter_luid()
            .ok_or_else(|| "零拷贝会话:D3D12 adapter LUID 不可得".to_string())?;
        if vk_luid != d3d_luid {
            return Err(format!(
                "零拷贝会话:LUID 不匹配(vulkan {vk_luid:?} vs d3d12 {d3d_luid:?})——跨 adapter 不可共享显存"
            ));
        }
    }

    if let Some(p) = &particles {
        eprintln!("[gpu-particles] Rurix Vulkan experiment enabled: compute=64 workgroups, draw=4096 instances, emitter_buffer={}, particle_buffer={}; analytic GPU trajectories, no CPU particle uploads", p.emitter_resource, p.particle_resource);
    }
    if slot_graph.iter().any(Option::is_some) { crate::shader::record_program_build("rurix"); }
    Ok(ViewportRenderer {
        width,
        height,
        slots,
        mesh_sig,
        session,
        device_name: caps.device_name,
        import_key: import.map(|im| (im.handle, im.size)),
        particles,
        texture_resources: distinct_tex.iter().map(|t| (*t, tex_res(*t))).collect(),
        graph_resources, _graph:graph_owner,
    })
}

/// 渲一帧:场景 → rgba8。无设备 → `DEV_ENV_DEGRADE:` 前缀 Err(诚实档)。
/// `vp_override`:PIE 时由场景相机实体给 view_proj(游戏画面 = 游戏相机);None = 编辑器相机。
/// `want_stats`:是否做非背景像素全帧扫描(遗留 MCP 腿诊断面);推流腿 30-60fps
/// 下逐帧扫 2M 像素纯属浪费,传 false 跳过(nonzero 恒 0,不伪造)。
#[allow(clippy::too_many_arguments)]
pub fn render_scene_frame(
    scene: &Scene,
    cam: &EditorCamera,
    selected: Option<u64>,
    width: u32,
    height: u32,
    want_readback: bool,
    want_stats: bool,
    vp_override: Option<M4>,
) -> Result<FramePixels, String> {
    if scene.name == "Code Sentinels V6" &&scene.entities.iter().any(|e|e.component("SentinelsV6Batch").is_some()) {
        return crate::sentinels_v6_render::render(scene, width, height, want_readback, want_stats);
    }
    if scene.entities.iter().any(|e| e.component("ModelRenderer").is_some_and(|c| c.enabled)) || crate::shader::scene_uses_spatial_graph(scene) {
        return crate::modelrender::render(scene,cam,selected,width,height,want_readback,want_stats,vp_override);
    }
    let slot = renderer_slot();
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());

    // ── 网格类解析(资产→视口接线 + 贴图精灵腿 F-GAME-2) ──
    // 类 0 = 贴图精灵 quad(实体材质有 albedo 贴图),类 1 = 内置 cube/回退,
    // 类 2.. = 不同 mesh 引用(首现序)。
    // 屏外 Sprite 不进 renderables(对象池闲置实体不占 128 槽预算;见 sprite_offscreen)。
    let frame_vp = vp_override.unwrap_or_else(|| cam.view_proj(width as f32 / height.max(1) as f32));
    let renderables: Vec<(&forge_scene::Entity, String)> = scene
        .entities
        .iter()
        .filter(|e| is_renderable(e) && !sprite_offscreen(e, &frame_vp))
        .map(|e| (e, entity_mesh_ref(e)))
        .collect();
    let renderable_n = renderables.len();
    let resident_tex = stable_rgba_inventory(scene).filter(|_| {
        let additive = renderables.iter().filter(|(e,_)| sprite_blend(e) == rex::BlendMode::Additive).count();
        additive <= MAX_DRAW_SLOTS - STABLE_ALPHA_SLOTS
            && renderable_n - additive <= STABLE_ALPHA_SLOTS
    });
    let stable_rgba = resident_tex.is_some();
    let extra_tex = resident_tex.as_deref().unwrap_or(&[]);
    let want_slots = if stable_rgba { MAX_DRAW_SLOTS } else { slot_tier(renderable_n) };
    let want_import = current_import(width, height);

    // 实体 → 精灵解析(有则走精灵 quad 槽)。
    // F-GAME-4:Sprite 组件经 resolve_sprite_render 统一出口(texture 直贴 /
    // .rxsprite 图集帧);否则沿旧路 MeshRenderer.material → albedo(整图)。
    let project_root = crate::rpc::project_root();
    let entity_sprite: Vec<Option<SpriteRenderInfo>> = renderables
        .iter()
        .map(|(e, _)| {
            if let Some(sp) = sprite_component(e) {
                return resolve_sprite_render(sp);
            }
            let mat_guid = e
                .components
                .iter()
                .find(|c| c.ctype == "MeshRenderer" && c.enabled)
                .and_then(|c| c.props.get("material"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if mat_guid.is_empty() {
                return None;
            }
            let albedo = material_albedo_guid(mat_guid, &project_root)?;
            let tex = load_tex_static_cached(&project_root, &albedo)?;
            Some(SpriteRenderInfo {
                tex,
                uv_rect: FULL_UV_RECT,
                frame_px: [tex.w as f32, tex.h as f32],
                pivot: [0.5, 0.5],
            })
        })
        .collect();
    let entity_tex: Vec<Option<&'static TexGpu>> =
        entity_sprite.iter().map(|s| s.as_ref().map(|i| i.tex)).collect();

    let mut class_refs: Vec<String> = Vec::new();
    let mut class_mesh: Vec<&'static MeshGpu> = Vec::new();
    let mut entity_class: Vec<usize> = Vec::with_capacity(renderable_n);
    let mut mesh_fallbacks = 0usize;
    for (i, (_, mesh_ref)) in renderables.iter().enumerate() {
        if entity_tex[i].is_some() {
            entity_class.push(0);
            continue;
        }
        if mesh_ref == "cube" {
            entity_class.push(1);
            continue;
        }
        let ci = match class_refs.iter().position(|c| c == mesh_ref) {
            Some(i) => i + 2,
            None => {
                if class_refs.len() >= meshres::MAX_MESH_CLASSES {
                    mesh_fallbacks += 1;
                    entity_class.push(1);
                    continue;
                }
                match meshres::load_mesh_static_cached(&crate::rpc::project_root(), mesh_ref) {
                    Ok(m) => {
                        class_refs.push(mesh_ref.clone());
                        class_mesh.push(m);
                        class_refs.len() + 1
                    }
                    Err(_) => {
                        // 失败详情已由 meshres 缓存路径 eprintln 一次;此处回退 cube。
                        mesh_fallbacks += 1;
                        entity_class.push(1);
                        continue;
                    }
                }
            }
        };
        entity_class.push(ci);
    }

    // ── 类槽位布局:quad 精灵类先保(其实体必绘),cube 类次之,非内置类按需,余量归 cube ──
    let n_classes = class_refs.len() + 2;
    let mut class_count = vec![0usize; n_classes];
    for &c in &entity_class {
        class_count[c] += 1;
    }
    let quad_n = class_count[0];
    let cube_n = class_count[1];
    let mut class_slots = vec![0usize; n_classes];
    let mut used_extra = 0usize;
    let avail = want_slots
        .saturating_sub(quad_n.min(want_slots))
        .saturating_sub(cube_n.min(want_slots));
    for ci in 2..n_classes {
        let take = class_count[ci].min(avail.saturating_sub(used_extra));
        class_slots[ci] = take;
        used_extra += take;
    }
    // 精灵数超当档预算(如 39 精灵落入 32 档)时钳到预算,超出截断(与超 128 同语义,
    // 如实 truncated);否则 slot_tex/slot_mesh(长 want_slots)越界。
    class_slots[0] = quad_n.min(want_slots);
    // 精灵/cube 超槽位预算时 used_extra+quad_n 可超 want_slots;饱和减法防下溢崩溃。
    class_slots[1] = want_slots.saturating_sub(used_extra).saturating_sub(quad_n.min(want_slots));

    // F-GAME-3:精灵类(类 0)按 (sortingOrder, 场景序) 排序——小者先绘、大者压上;
    // 槽位贴图绑定与下方绘制序共用此次序(稳定排序,等键保持场景序)。
    let mut quad_order: Vec<usize> = (0..renderable_n).filter(|&i| entity_class[i] == 0).collect();
    quad_order.sort_by(|&a, &b| {
        sprite_sorting_order(renderables[a].0)
            .partial_cmp(&sprite_sorting_order(renderables[b].0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 槽位→几何/贴图表(构建期定案,帧内实体按类序占槽):
    // quad 类占 [0..class_slots[0]),cube 类紧随,各 mesh 类再随后。
    let mut slot_mesh: Vec<Option<&'static MeshGpu>> = vec![None; want_slots];
    let mut slot_tex: Vec<Option<&'static TexGpu>> = vec![None; want_slots];
    let mut slot_blend = vec![rex::BlendMode::Opaque; want_slots];
    let mut slot_graph=vec![None;want_slots];
    let mut qnext = 0usize;
    for &i in &quad_order {
        if qnext < class_slots[0] {
            slot_tex[qnext] = entity_tex[i];
            slot_graph[qnext]=crate::shader::sprite(renderables[i].0)?;
            slot_blend[qnext] = sprite_blend(renderables[i].0);
            qnext += 1;
        }
    }
    let mut next = class_slots[0] + class_slots[1];
    for (ci, m) in class_mesh.iter().enumerate() {
        for _ in 0..class_slots[ci + 2] {
            slot_mesh[next] = Some(m);
            next += 1;
        }
    }

    if stable_rgba {
        class_slots[0] = want_slots;
        class_slots[1] = 0;
        for slot in 0..want_slots {
            slot_mesh[slot] = None;
            slot_tex[slot] = Some(extra_tex[0]);
            slot_blend[slot] = if slot < STABLE_ALPHA_SLOTS { rex::BlendMode::Alpha } else { rex::BlendMode::Additive };
        }
    }
    // 几何/贴图布局签名(任一变化 → 会话重建;与改尺寸同路径)。
    let mut sig_src = Vec::new();
    sig_src.extend_from_slice(&width.to_le_bytes());
    sig_src.extend_from_slice(&height.to_le_bytes());
    sig_src.extend_from_slice(&(want_slots as u64).to_le_bytes());
    sig_src.extend_from_slice(&want_import.map(|im| im.handle).unwrap_or(0).to_le_bytes());
    sig_src.extend_from_slice(&want_import.map(|im| im.size).unwrap_or(0).to_le_bytes());
    sig_src.extend_from_slice(&(class_slots[0] as u32).to_le_bytes());
    for (r, ks) in class_refs.iter().zip(class_slots[2..].iter()) {
        sig_src.extend_from_slice(r.as_bytes());
        sig_src.push(0xff);
        sig_src.extend_from_slice(&(*ks as u32).to_le_bytes());
    }
    for m in &class_mesh {
        sig_src.extend_from_slice(&m.bytes.len().to_le_bytes());
    }
    // 槽位→贴图身份(场景换材质/换实体 → 重建使槽位绑定跟手)。
    for t in &slot_tex {
        match t {
            Some(t) => {
                sig_src.push(0x01);
                sig_src.extend_from_slice(&t.w.to_le_bytes());
                sig_src.extend_from_slice(&t.h.to_le_bytes());
                sig_src.extend_from_slice(&(t.rgba.len() as u64).to_le_bytes());
            }
            None => sig_src.push(0x00),
        }
    }
    sig_src.extend(slot_blend.iter().map(|mode| *mode as u8));
    // D-045:文字贴图同尺寸改字时 w/h/len 不变,签名须带上贴图身份,否则画面不刷新。
    for ((e, _), info) in renderables.iter().zip(entity_sprite.iter()) {
        if sprite_component(e).is_some_and(crate::render_core::sprite::is_text) {
            if let Some(i) = info {
                sig_src.extend_from_slice(&(i.tex as *const TexGpu as usize).to_le_bytes());
            }
        }
    }
    for texture in extra_tex {
        sig_src.extend_from_slice(&(*texture as *const TexGpu as usize).to_le_bytes());
    }
    for (index, graph) in slot_graph.iter().enumerate() {
        if let Some(graph) = graph {
            sig_src.extend_from_slice(&index.to_le_bytes());
            sig_src.extend_from_slice(graph.program.compiled.program_hash.as_bytes());
            let len = graph.packed_len(slot_tex[index].map_or(4, |t| t.rgba.len()))?;
            sig_src.extend_from_slice(&len.to_le_bytes());
        }
    }
    let mesh_sig = meshres::fnv1a64(&sig_src);

    // 懒初始化 / 改尺寸或共享纹理 import 键变化或网格类布局变化时重建
    // (降级态一经判定即缓存,不重试)。F6 wave.5:实体数超当前 pass 档 → 升档重建。
    REBUILD_FLAG.store(false, std::sync::atomic::Ordering::Relaxed);
    // 重建防抖(F-GAME-2 性能):重建 = 全量管线 + 纹理/网格初传,实测 ≈1-5s;多个取帧方
    // (浏览器面板/诊断探针)尺寸不一致时会逐帧交替重建 → 卡死。1500ms 内的纯尺寸类
    // 重建押后:复用现有会话按其自身尺寸出帧(客户端按回读宽高自适应呈现);几何/
    // 贴图类变更(sig/档位/import)不押后——场景切换最多延迟一帧。
    // 「纯尺寸」必须同时满足 import 键与槽位档未变:share 重开(新尺寸共享 buffer)
    // 若被押后,旧会话仍绑旧 import,回读字节数与请求尺寸不符直接报错(f1_zerocopy 实测)。
    let size_only_change = matches!(&*guard, RendererState::Ready(r)
        if (r.width != width || r.height != height)
            && r.import_key == want_import.map(|im| (im.handle, im.size))
            && r.slots >= want_slots);
    let throttled = size_only_change
        && !FORCE_EXACT_SIZE.load(std::sync::atomic::Ordering::Relaxed)
        && LAST_REBUILD
            .lock()
            .unwrap()
            .map(|t| t.elapsed() < std::time::Duration::from_millis(1500))
            .unwrap_or(false);
    match &*guard {
        RendererState::Uninit => {
            REBUILD_FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
            *guard = match build_session(width, height, want_slots, &slot_mesh, &slot_tex, &slot_blend, &slot_graph, extra_tex, mesh_sig)
            {
                Ok(r) => RendererState::Ready(r),
                Err(e) => {for graph in slot_graph.iter().flatten(){crate::shader::report_backend(&graph.program.compiled.hash,"rurix",Err(e.clone()));}return Err(e);},
            };
        }
        RendererState::Ready(r) if !throttled && {
            let reason = if r.width != width { Some("size") }
                else if r.height != height { Some("size_h") }
                else if r.import_key != want_import.map(|im| (im.handle, im.size)) { Some("import_key") }
                else if r.slots < want_slots { Some("slots") }
                else if r.mesh_sig != mesh_sig { Some("sig") }
                else { None };
            match reason {
                Some(w) => {
                    REBUILD_FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
                    *LAST_REBUILD.lock().unwrap() = Some(std::time::Instant::now());
                    eprintln!("[viewport] 会话重建原因: {w} (会话 {}x{} → 请求 {width}x{height}, sig {}≠{})",
                        r.width, r.height, r.mesh_sig, mesh_sig);
                    true
                }
                None => false,
            }
        } =>
        {
            *guard = match build_session(width, height, want_slots, &slot_mesh, &slot_tex, &slot_blend, &slot_graph, extra_tex, mesh_sig)
            {
                Ok(r) => RendererState::Ready(r),
                Err(e) => {for graph in slot_graph.iter().flatten(){crate::shader::report_backend(&graph.program.compiled.hash,"rurix",Err(e.clone()));}return Err(e);},
            };
        }
        _ => {}
    }
    let r = match &mut *guard {
        RendererState::Ready(r) => r,
        RendererState::Degraded(e) => return Err(e.clone()),
        RendererState::Uninit => unreachable!("上分支已初始化"),
    };
    let slots = r.slots;

    let vp_bytes = m4_col_bytes(frame_vp);

    let mut update = rex::FrameUpdate {
        buffer_uploads: vec![(rex::StableResourceId(2), 0, vp_bytes.to_vec())],
        ..Default::default()
    };
    for (i,graph)in slot_graph.iter().enumerate(){if let(Some(graph),Some(source),Some(res))=(graph,slot_tex[i],r.graph_resources.get(i).copied().flatten()){let data=graph.pack(&crate::shader::Texture{width:source.w,height:source.h,rgba:std::sync::Arc::new(source.rgba.to_vec())})?;update.buffer_uploads.push((rex::StableResourceId(res as u64+1),0,data));}}
    if let Some(particles) = &r.particles {
        let (bytes, _) = crate::gpu_particles::emitter_bytes(scene);
        update.buffer_uploads.push((rex::StableResourceId(particles.emitter_resource as u64 + 1), 0, bytes));
    }
    // 实体按类序稳定占槽(cube 类先,类内保持场景序);实体落位 = 其类槽区间内
    // 顺次下一空槽(pass 的 VB 绑定按槽位类定案,实体绝不可跨类占槽)。
    let mut class_start = vec![0usize; class_slots.len()];
    {
        let mut acc = 0usize;
        for (c, k) in class_slots.iter().enumerate() {
            class_start[c] = acc;
            acc += *k;
        }
    }
    let mut order: Vec<usize> = (0..renderable_n).collect();
    // 类序优先;精灵类内按 (sortingOrder, 场景序)(与槽位贴图绑定同序,F-GAME-3)。
    order.sort_by(|&a, &b| {
        let (ca, cb) = (entity_class[a], entity_class[b]);
        if ca != cb {
            return ca.cmp(&cb);
        }
        if ca == 0 {
            return sprite_sorting_order(renderables[a].0)
                .partial_cmp(&sprite_sorting_order(renderables[b].0))
                .unwrap_or(std::cmp::Ordering::Equal);
        }
        a.cmp(&b)
    });
    let mut drawn_class = vec![0usize; class_refs.len() + 2];
    let mut used_slots = vec![false; slots];
    let mut alpha_drawn = 0usize;
    let mut additive_drawn = 0usize;
    let mut draws = 0usize;
    let mut triangles = 0usize;
    for &i in &order {
        let c = entity_class[i];
        if drawn_class[c] >= class_slots[c] {
            continue; // 该类槽满(同类后续实体跳过;异类仍有各自槽位)
        }
        if draws >= slots {
            break; // 全槽满:剩余实体本帧不绘(truncated 如实上报)
        }
        let e = renderables[i].0;
        let model = trs_model(&e.transform);
        let slot_idx = if stable_rgba {
            if sprite_blend(e) == rex::BlendMode::Additive {
                if additive_drawn >= slots - STABLE_ALPHA_SLOTS { continue; }
                let slot = STABLE_ALPHA_SLOTS + additive_drawn; additive_drawn += 1; slot
            } else {
                if alpha_drawn >= STABLE_ALPHA_SLOTS { continue; }
                let slot = alpha_drawn; alpha_drawn += 1; slot
            }
        } else { class_start[c] + drawn_class[c] };
        used_slots[slot_idx] = true;
        if c == 0 {
            // 贴图精灵槽(112B:model + tint + 贴图尺寸 + flip 标志 + uv_rect,F-GAME-4)。
            let info = entity_sprite[i].as_ref().expect("quad 类实体必有精灵解析");
            let t = info.tex;
            // F-GAME-3/4:Sprite 实体模型 = 渲染态变换(帧尺寸 × pivot 锚定),
            // tint/flip 来自组件 props;旧 MeshRenderer+material 路径恒白 tint、
            // 无 flip、整图 uv_rect、模型不变。
            let (model, tint, flip) = match sprite_component(e) {
                Some(sp) => {
                    let m = trs_model(&sprite_render_transform(e).unwrap_or(e.transform));
                    let tint_arr = sp.props.get("tint").and_then(Value::as_array);
                    let tint = [0usize, 1, 2, 3].map(|k| {
                        tint_arr
                            .and_then(|a| a.get(k))
                            .and_then(Value::as_f64)
                            .unwrap_or(1.0) as f32
                    });
                    let flip = [sprite_bool(sp, "flipX"), sprite_bool(sp, "flipY")];
                    (m, tint, flip)
                }
                None => (model, [1.0; 4], [false, false]),
            };
            let mut pc = Vec::with_capacity(SPRITE_PC_LEN);
            pc.extend_from_slice(&m4_col_bytes(model));
            for f in tint {
                pc.extend_from_slice(&f.to_le_bytes());
            }
            pc.extend_from_slice(&t.w.to_le_bytes());
            pc.extend_from_slice(&t.h.to_le_bytes());
            pc.extend_from_slice(&(if flip[0] { 1.0f32 } else { 0.0f32 }).to_le_bytes());
            pc.extend_from_slice(&(if flip[1] { 1.0f32 } else { 0.0f32 }).to_le_bytes());
            for f in info.uv_rect {
                pc.extend_from_slice(&f.to_le_bytes());
            }
            for f in sprite_compositing(e) { pc.extend_from_slice(&f.to_le_bytes()); }
            if stable_rgba {
                let resource = r.texture_resources.iter().find(|(tex,_)| std::ptr::eq(*tex,t))
                    .map(|(_,res)| *res).ok_or_else(|| "resident Sprite texture missing".to_string())?;
                update.binding_overrides.push((slot_idx as u32,rex::Bindings {
                    uniform:Some(rex::UniformRef{res:1,offset:0,size:64}),storage_buffers:vec![resource],
                    push_constants:pc,..Default::default()
                }));
            } else { update.push_constant_overrides.push((slot_idx as u32, pc)); }
            triangles += 2;
        } else {
            let color = entity_tint(e, selected == Some(e.id));
            let mut pc = Vec::with_capacity(80);
            pc.extend_from_slice(&m4_col_bytes(model));
            for f in color {
                pc.extend_from_slice(&f.to_le_bytes());
            }
            update.push_constant_overrides.push((slot_idx as u32, pc));
            triangles += if c == 1 {
                12
            } else {
                class_mesh[c - 2].triangle_count as usize
            };
        }
        drawn_class[c] += 1;
        draws += 1;
    }
    // 类内未占槽恒推回消隐模型(上帧可能有更多实体;80/96B×128 开销可忽略)。
    let hidden_model = trs_model(&Transform {
        translation: [0.0, -1000.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1e-6, 1e-6, 1e-6],
    });
    for (c, k) in class_slots.iter().enumerate() {
        for j in 0..*k {
            if used_slots[class_start[c]+j] { continue; }
            // 贴图槽 112B、普通槽 80B——两种 push constants 尺寸并存,按槽对应类发。
            let pc_len = if c == 0 { SPRITE_PC_LEN } else { 80usize };
            let mut pc = Vec::with_capacity(pc_len);
            pc.extend_from_slice(&m4_col_bytes(hidden_model));
            pc.extend_from_slice(&[0u8; 16]);
            if pc_len == SPRITE_PC_LEN {
                pc.extend_from_slice(&1u32.to_le_bytes());
                pc.extend_from_slice(&1u32.to_le_bytes());
                pc.extend_from_slice(&0f32.to_le_bytes());
                pc.extend_from_slice(&0f32.to_le_bytes());
                for f in FULL_UV_RECT {
                    pc.extend_from_slice(&f.to_le_bytes());
                }
                for f in [1.0f32, 0., 0., 0.] { pc.extend_from_slice(&f.to_le_bytes()); }
            }
            update.push_constant_overrides.push(((class_start[c] + j) as u32, pc));
        }
    }
    // F6 wave.5 瓶颈分解:format=none 性能测量档不回读(跳过 submit→wait 同步
    // 阻塞 + 8MB 拷贝 + 2M 像素统计),纯渲染+提交产能与帧通道端到端成本可拆分留档。
    update.readback_subset = if want_readback { Some(vec![0]) } else { None };

    // F-GAME-2 性能定位:分段计时(准备/提交执行/回读)。
    let prep_t0 = std::time::Instant::now();
    let provenance = r
        .session
        .next_provenance_with_update(&update)
        .map_err(|e| format!("帧 provenance 推导失败: {e}"))?;
    let out = r
        .session
        .execute_with_frame_update(&provenance, &update)
        .map_err(|e| format!("帧执行失败: {e}"))?;
    for graph in slot_graph.iter().flatten(){crate::shader::report_backend(&graph.program.compiled.hash,"rurix",Ok(()));}
    let exec_ms = prep_t0.elapsed().as_millis();
    let (rgba8, nonzero) = if want_readback {
        let rgba8 = out
            .readbacks
            .into_iter()
            .next()
            .ok_or_else(|| "readback 缺失".to_owned())?;
        let expect = (width * height * 4) as usize;
        if rgba8.len() != expect {
            return Err(format!("回读字节数不符:{} ≠ {expect}", rgba8.len()));
        }
        let nonzero = if want_stats {
            let bg = [
                (CLEAR_RGBA[0] * 255.0 + 0.5).floor() as u8,
                (CLEAR_RGBA[1] * 255.0 + 0.5).floor() as u8,
                (CLEAR_RGBA[2] * 255.0 + 0.5).floor() as u8,
            ];
            rgba8
                .chunks_exact(4)
                .filter(|p| p[0] != bg[0] || p[1] != bg[1] || p[2] != bg[2])
                .count()
        } else {
            0
        };
        (rgba8, nonzero)
    } else {
        (Vec::new(), 0)
    };
    // 分段计时日志采样:推流 30-60fps 下逐帧打会刷爆 engine-host-err.log;
    // 重建帧必打(性能定位关键证据),常规帧每 60 帧一条。
    static FRAME_LOG_TICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let log_tick = FRAME_LOG_TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let rebuilt = REBUILD_FLAG.load(std::sync::atomic::Ordering::Relaxed);
    if rebuilt || log_tick % 60 == 0 {
        eprintln!(
            "[viewport] 帧分段 exec={exec_ms}ms readback+scan={}ms slots={} quad_slots={} draws={draws} rebuilt_this_frame={rebuilt}",
            prep_t0.elapsed().as_millis() - exec_ms,
            slots,
            class_slots[0],
        );
    }
    Ok(FramePixels {
        width,
        height,
        rgba8,
        device_name: r.device_name.clone(),
        draws,
        truncated: renderable_n > draws,
        nonzero,
        triangles,
        mesh_fallbacks,
        mesh_classes: class_refs.len(),
        imported: r.import_key.is_some(),
    })
}
