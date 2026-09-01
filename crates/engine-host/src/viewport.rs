//! Viewport 模块(F1 wave.2,RD-F1-001 回填):编辑器相机 + rurix-rt render_exec(Vulkan)
//! 场景实渲染 + Readback 回读 + 射线点选。
//!
//! 架构:
//! - 实体(MeshRenderer enabled)以相机 UBO(viewProj)+ 逐实体 push constants
//!   (model 64B + color 16B = 80B ≤ 128)经 GPU 光栅化(Depth32Float,LESS_OR_EQUAL),
//!   `Readback::Texture` 回读 RGBA8。该帧既供 canvas 回退腿,也供 D3D12 共享纹理生产者作帧源。
//! - 网格(2026-08-28 资产→视口断链接线):`MeshRenderer.mesh` 引用经 [`crate::meshres`]
//!   解析到项目 .rxmesh 构建产物 → 展平顶点缓冲(pos3+normal3);解析失败/超上限诚实回退
//!   内置 cube 并计入 `meshFallbacks`。pass 槽位按网格类静态绑定 VB(类槽位按当帧实体
//!   计数分配,cube 类兜底占余量),实体帧内按类序稳定占槽;网格类布局变化触发会话重建
//!   (与改尺寸同路径,编辑期人手尺度,代价有界)。
//! - 固定 pass 图:[`MAX_DRAW_SLOTS`] 个 draw pass 常驻;未占槽以「远埋微缩」模型矩阵消隐
//!   (有限值,避免 NaN 顶点未定义光栅化)。每帧仅经 `FrameUpdate`(buffer_uploads 相机 +
//!   push_constant_overrides 实体)驱动,provenance 可机验。
//! - 着色器:WGSL 源码经 naga 纯 Rust 编译为 SPIR-V(缓存钉版 =25.0.1)。
//! - 诚实三态:vulkan loader/能力缺失 → `DEV_ENV_DEGRADE:` 前缀错误,绝不伪造帧。
//!
//! 会话描述块(resources/passes/barriers/readbacks)借给 `DeviceFrameSession<'static>`,
//! 经 `Box::leak` 提升;重建发生在视口改尺寸、实体数超档升档(F6 wave.5,只升不降)或
//! 网格类布局变化,代价有界。

use std::sync::{Mutex, OnceLock};

use base64::Engine as _;
use forge_scene::{Scene, Transform};
use rurix_rt::render_exec as rex;
use rurix_rt::vk;
use serde_json::Value;

use crate::meshres::{self, MeshGpu};

// ─────────────────────────── 向量/矩阵(手卷 f32,确定性) ───────────────────────────

type V3 = [f32; 3];
type M3 = [[f32; 3]; 3];
/// 行主序 4x4;列向量约定 `v' = M · v`。
type M4 = [[f32; 4]; 4];

fn v3_sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn v3_scale(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn v3_dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn v3_cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn v3_norm(a: V3) -> V3 {
    let len = v3_dot(a, a).sqrt();
    if len < 1e-12 {
        [0.0, 0.0, 0.0]
    } else {
        v3_scale(a, 1.0 / len)
    }
}

fn m4_mul(a: M4, b: M4) -> M4 {
    let mut out = [[0.0f32; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

/// 行主序 → 列主序字节(mat4x4<f32> UBO/push 内存布局)。
fn m4_col_bytes(m: M4) -> [u8; 64] {
    let mut out = [0u8; 64];
    for c in 0..4 {
        for r in 0..4 {
            let o = (c * 4 + r) * 4;
            out[o..o + 4].copy_from_slice(&m[r][c].to_le_bytes());
        }
    }
    out
}

/// 透视投影(RH,Vulkan NDC z∈[0,1];m[1][1] 取负做 y-flip 适配 attachment 行序)。
fn perspective_vk(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> M4 {
    let t = 1.0 / (fov_y_rad * 0.5).tan();
    let mut m = [[0.0f32; 4]; 4];
    m[0][0] = t / aspect;
    m[1][1] = -t;
    m[2][2] = far / (near - far);
    m[2][3] = far * near / (near - far);
    m[3][2] = -1.0;
    m
}

/// 观察矩阵(RH;-z 为前向)。
fn look_at_rh(eye: V3, center: V3, up: V3) -> M4 {
    let f = v3_norm(v3_sub(center, eye));
    let s = v3_norm(v3_cross(f, up));
    let u = v3_cross(s, f);
    [
        [s[0], s[1], s[2], -v3_dot(s, eye)],
        [u[0], u[1], u[2], -v3_dot(u, eye)],
        [-f[0], -f[1], -f[2], v3_dot(f, eye)],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// 四元数 [x,y,z,w] → 旋转矩阵(内部归一化,抗数值漂移)。
fn quat_to_mat3(q: [f32; 4]) -> M3 {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    let (x, y, z, w) = if n < 1e-12 {
        (0.0, 0.0, 0.0, 1.0)
    } else {
        (q[0] / n, q[1] / n, q[2] / n, q[3] / n)
    };
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

fn m3_transpose(m: M3) -> M3 {
    [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ]
}

fn m3_apply(m: M3, v: V3) -> V3 {
    [
        v3_dot(m[0], v),
        v3_dot(m[1], v),
        v3_dot(m[2], v),
    ]
}

/// TRS 模型矩阵(T·R·S;非均匀缩放按列施加)。
fn trs_model(t: &Transform) -> M4 {
    let r = quat_to_mat3(t.rotation);
    let mut m = [[0.0f32; 4]; 4];
    for rr in 0..3 {
        for cc in 0..3 {
            m[rr][cc] = r[rr][cc] * t.scale[cc];
        }
        m[rr][3] = t.translation[rr];
    }
    m[3][3] = 1.0;
    m
}

// ─────────────────────────── 编辑器相机 ───────────────────────────

/// 环绕式编辑器相机(07 §2:Alt+左键环绕 / 滚轮缩放 / F 聚焦 target)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorCamera {
    pub target: V3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub dist: f32,
    pub fov_y_deg: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        EditorCamera {
            target: [0.0, 0.5, 0.0],
            yaw_deg: 35.0,
            pitch_deg: 28.0,
            dist: 9.0,
            fov_y_deg: 50.0,
        }
    }
}

impl EditorCamera {
    /// 眼位(绕 target 球坐标;yaw 绕 Y,pitch 抬升)。
    pub fn eye(&self) -> V3 {
        let yaw = self.yaw_deg.to_radians();
        let pitch = self.pitch_deg.to_radians();
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = yaw.sin_cos();
        [
            self.target[0] + self.dist * cp * sy,
            self.target[1] + self.dist * sp,
            self.target[2] + self.dist * cp * cy,
        ]
    }

    /// 相机基 (right, up, forward)。
    fn basis(&self) -> (V3, V3, V3) {
        let eye = self.eye();
        let f = v3_norm(v3_sub(self.target, eye));
        let r = v3_norm(v3_cross(f, [0.0, 1.0, 0.0]));
        let u = v3_cross(r, f);
        (r, u, f)
    }

    /// viewProj(列向量约定;aspect = w/h)。
    pub fn view_proj(&self, aspect: f32) -> M4 {
        let proj = perspective_vk(self.fov_y_deg.to_radians(), aspect.max(1e-6), 0.05, 500.0);
        let view = look_at_rh(self.eye(), self.target, [0.0, 1.0, 0.0]);
        m4_mul(proj, view)
    }

    /// 屏幕归一化坐标 (nx,ny ∈ [-1,1],y 向上为正) → 世界射线 (origin, dir 归一)。
    pub fn ray(&self, nx: f32, ny: f32, aspect: f32) -> (V3, V3) {
        let (r, u, f) = self.basis();
        let t = (self.fov_y_deg.to_radians() * 0.5).tan();
        let dir = v3_norm([
            r[0] * nx * t * aspect + u[0] * ny * t + f[0],
            r[1] * nx * t * aspect + u[1] * ny * t + f[1],
            r[2] * nx * t * aspect + u[2] * ny * t + f[2],
        ]);
        (self.eye(), dir)
    }

    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "target": self.target,
            "yaw": self.yaw_deg,
            "pitch": self.pitch_deg,
            "dist": self.dist,
            "fovY": self.fov_y_deg,
        })
    }
}

// ─────────────────────────── 射线 vs 单位立方体 OBB ───────────────────────────

/// 射线(世界系,dir 归一)与实体单位立方体 OBB 求交;命中返回参数 t(≥0,与世界射线同参)。
/// 局部化不除 d 的长度,保持参数一致(仿射变换保参数)。
pub fn ray_unit_cube(origin: V3, dir: V3, tr: &Transform) -> Option<f32> {
    let r = quat_to_mat3(tr.rotation);
    let rt = m3_transpose(r);
    let rel = v3_sub(origin, tr.translation);
    let o_l = m3_apply(rt, rel);
    let d_l = m3_apply(rt, dir);
    let s = tr.scale;
    let mut tmin = f32::NEG_INFINITY;
    let mut tmax = f32::INFINITY;
    for i in 0..3 {
        if s[i].abs() < 1e-9 {
            return None; // 零缩放实体不可见亦不可选
        }
        let o = o_l[i] / s[i];
        let d = d_l[i] / s[i];
        if d.abs() < 1e-12 {
            if o.abs() > 0.5 {
                return None;
            }
            continue;
        }
        let mut t1 = (-0.5 - o) / d;
        let mut t2 = (0.5 - o) / d;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        tmin = tmin.max(t1);
        tmax = tmax.min(t2);
        if tmin > tmax {
            return None;
        }
    }
    if tmax < 0.0 {
        return None;
    }
    Some(if tmin >= 0.0 { tmin } else { tmax })
}

/// 实体是否参与视口渲染/点选(含 enabled MeshRenderer)。
fn is_renderable(e: &forge_scene::Entity) -> bool {
    e.components
        .iter()
        .any(|c| c.ctype == "MeshRenderer" && c.enabled)
}

/// 实体网格引用(MeshRenderer.props.mesh;缺省/空 = 内置 cube)。
fn entity_mesh_ref(e: &forge_scene::Entity) -> String {
    e.components
        .iter()
        .find(|c| c.ctype == "MeshRenderer" && c.enabled)
        .and_then(|c| c.props.get("mesh").and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .unwrap_or("cube")
        .to_string()
}

/// 点选:像素坐标(左上原点) → 最近命中实体 (id, 命中点)。
pub fn pick_entity(
    scene: &Scene,
    cam: &EditorCamera,
    px: f32,
    py: f32,
    width: u32,
    height: u32,
) -> Option<(u64, V3)> {
    let aspect = width as f32 / height.max(1) as f32;
    let nx = 2.0 * px / width.max(1) as f32 - 1.0;
    let ny = 1.0 - 2.0 * py / height.max(1) as f32;
    let (origin, dir) = cam.ray(nx, ny, aspect);
    let mut best: Option<(u64, f32)> = None;
    for e in &scene.entities {
        if !is_renderable(e) {
            continue;
        }
        if let Some(t) = ray_unit_cube(origin, dir, &e.transform) {
            if best.is_none_or(|(_, bt)| t < bt) {
                best = Some((e.id, t));
            }
        }
    }
    best.map(|(id, t)| {
        let p = [
            origin[0] + dir[0] * t,
            origin[1] + dir[1] * t,
            origin[2] + dir[2] * t,
        ];
        (id, p)
    })
}

// ─────────────────────────── 立方体网格与着色器 ───────────────────────────

/// 单位立方体 36 顶点(每面 2 三角形,面法线;交错 pos3+normal3,stride 24)。
fn cube_mesh_bytes() -> &'static [u8] {
    static MESH: OnceLock<&'static [u8]> = OnceLock::new();
    MESH.get_or_init(|| {
        // (法线, 该面四角(逆时针));每角 = 基准 ± 两轴半长。
        let faces: [(V3, [V3; 4]); 6] = [
            ([1.0, 0.0, 0.0], [[0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5], [0.5, -0.5, -0.5]]),
            ([-1.0, 0.0, 0.0], [[-0.5, -0.5, -0.5], [-0.5, 0.5, -0.5], [-0.5, 0.5, 0.5], [-0.5, -0.5, 0.5]]),
            ([0.0, 1.0, 0.0], [[-0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5]]),
            ([0.0, -1.0, 0.0], [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]]),
            ([0.0, 0.0, 1.0], [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]]),
            ([0.0, 0.0, -1.0], [[0.5, -0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5, 0.5, -0.5], [0.5, 0.5, -0.5]]),
        ];
        let mut bytes = Vec::with_capacity(36 * 24);
        let mut push_vert = |p: V3, n: V3| {
            for f in [p[0], p[1], p[2], n[0], n[1], n[2]] {
                bytes.extend_from_slice(&f.to_le_bytes());
            }
        };
        for (n, c) in faces {
            // 两面三角形 (0,1,2) / (0,2,3),法线侧朝外(逆时针,正面剔除约定沿 render_exec 默认)。
            for i in [0usize, 1, 2, 0, 2, 3] {
                push_vert(c[i], n);
            }
        }
        Box::leak(bytes.into_boxed_slice())
    })
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
fn compile_wgsl(src: &str, stage: &str) -> Result<&'static [u8], String> {
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
fn shader_bytes() -> Result<(&'static [u8], &'static [u8]), String> {
    static SHADERS: OnceLock<Result<(&'static [u8], &'static [u8]), String>> = OnceLock::new();
    SHADERS
        .get_or_init(|| Ok((compile_wgsl(VS_WGSL, "vs")?, compile_wgsl(FS_WGSL, "fs")?)))
        .clone()
}

/// 零拷贝 pack compute 着色器字节(仅共享档用;OnceLock 缓存)。
fn pack_shader_bytes() -> Result<&'static [u8], String> {
    static PACK: OnceLock<Result<&'static [u8], String>> = OnceLock::new();
    PACK.get_or_init(|| compile_wgsl(PACK_WGSL, "pack")).clone()
}

// ─────────────────────────── 渲染会话(DeviceFrameSession) ───────────────────────────

/// 固定 draw pass 槽数上限(实体上限;超出截断并如实报 truncated)。
const MAX_DRAW_SLOTS: usize = 128;

/// F6 wave.5 动态档位:pass 图槽数按可渲染实体数取档(48/96/128),
/// 避免小场景为空槽 pass 付全量重录/draw 代价(128 空槽实测 1080p ~22ms/帧大头)。
/// 档位只升不降(滞后:实体数回落不重建,避免抖动);超 128 仍截断。
fn slot_tier(renderable: usize) -> usize {
    if renderable <= 48 {
        48
    } else if renderable <= 96 {
        96
    } else {
        MAX_DRAW_SLOTS
    }
}
/// 清屏底色(cursor+Claude 系深色;RGBA8 ≈ [23,24,29,255])。
const CLEAR_RGBA: [f32; 4] = [0.090, 0.094, 0.114, 1.0];
const R32G32B32_SFLOAT: u32 = 109;
/// 顶点属性:(location, format, offset)。
const VERTEX_ATTRS: [(u32, u32, u32); 2] = [(0, R32G32B32_SFLOAT, 0), (1, R32G32B32_SFLOAT, 12)];

enum RendererState {
    Uninit,
    Ready(ViewportRenderer),
    /// 诚实降级原因(loader 缺失/能力缺失/会话创建失败)。
    Degraded(String),
}

struct ViewportRenderer {
    width: u32,
    height: u32,
    /// 本会话 pass 图槽数(F6 wave.5 动态档;升档触发重建)。
    slots: usize,
    /// 本会话槽位→网格类签名(网格类布局变化触发重建)。
    mesh_sig: u64,
    session: rex::DeviceFrameSession<'static>,
    device_name: String,
    /// 本会话 import 的共享纹理键(nt_handle, alloc_size);None = 纯 readback 腿。
    /// share 重建(尺寸协商)后键值变化 → 会话重建。
    import_key: Option<(u64, u64)>,
}

// SAFETY:`DeviceFrameSession` 内含 *mut c_void(VkDevice 等原生句柄)被保守标 !Send。
// 本渲染器仅经 `RENDERER: Mutex` 互斥访问——任一时刻单线程持有,满足 Vulkan「外部同步」
// 线程模型(单 queue 提交者);原生句柄进程内全线程有效,不存在无同步的跨线程并发。
unsafe impl Send for ViewportRenderer {}

static RENDERER: OnceLock<Mutex<RendererState>> = OnceLock::new();

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
struct ShareImport {
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
    mesh_sig: u64,
) -> Result<ViewportRenderer, String> {
    let import = current_import(width, height);
    match build_session_with(width, height, import, slots, slot_mesh, mesh_sig) {
        Ok(r) => Ok(r),
        Err(e) if import.is_some() => {
            eprintln!("[viewport] 零拷贝 import 会话创建失败,回退 readback 上传腿: {e}");
            build_session_with(width, height, None, slots, slot_mesh, mesh_sig)
        }
        Err(e) => Err(e),
    }
}

fn build_session_with(
    width: u32,
    height: u32,
    import: Option<ShareImport>,
    slots: usize,
    slot_mesh: &[Option<&'static MeshGpu>],
    mesh_sig: u64,
) -> Result<ViewportRenderer, String> {
    assert_eq!(slot_mesh.len(), slots, "槽位→网格类表长度须等于槽数");
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
    let import_res = 4 + distinct.len() as u32;

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

    // 消隐槽模型:远埋 + 微缩(全有限值,规避 NaN 顶点未定义光栅化)。
    let hidden_model = trs_model(&Transform {
        translation: [0.0, -1000.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1e-6, 1e-6, 1e-6],
    });
    let mut hidden_pc = Vec::with_capacity(80);
    hidden_pc.extend_from_slice(&m4_col_bytes(hidden_model));
    hidden_pc.extend_from_slice(&[0u8; 16]);

    let mut passes: Vec<rex::Pass> = Vec::with_capacity(slots);
    let mut barrier_plan: Vec<Vec<(u32, rex::TargetState)>> = Vec::with_capacity(slots);
    for (k, s) in slot_mesh.iter().enumerate() {
        let (vb_res, vertex_count) = match s {
            None => (0u32, 36u32),
            Some(m) => (
                distinct.iter().find(|(d, _)| std::ptr::eq(*d, *m)).map(|(_, r)| *r).unwrap_or(0),
                m.vertex_count,
            ),
        };
        let first = k == 0;
        passes.push(rex::Pass::Raster(rex::RasterPass {
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
            (4, rex::TargetState::StorageReadWrite),
        ]);
    }

    let readbacks: Vec<rex::Readback> = vec![rex::Readback::Texture { res: 2 }];

    // session 借用上述描述块:'static 提升;重建仅发生在改尺寸,有界。
    let resources = Box::leak(resources.into_boxed_slice());
    let passes = Box::leak(passes.into_boxed_slice());
    let barrier_plan = Box::leak(barrier_plan.into_boxed_slice());
    let barriers: Vec<&[(u32, rex::TargetState)]> =
        barrier_plan.iter().map(Vec::as_slice).collect();
    let barriers = Box::leak(barriers.into_boxed_slice());
    let readbacks = Box::leak(readbacks.into_boxed_slice());

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

    Ok(ViewportRenderer {
        width,
        height,
        slots,
        mesh_sig,
        session,
        device_name: caps.device_name,
        import_key: import.map(|im| (im.handle, im.size)),
    })
}

/// 实体确定性配色(id 哈希 → 调色板;选中高亮橙)。
fn entity_color(id: u64, selected: bool) -> [f32; 4] {
    if selected {
        return [1.0, 0.62, 0.18, 1.0];
    }
    const PALETTE: [[f32; 3]; 8] = [
        [0.45, 0.62, 0.85],
        [0.62, 0.78, 0.52],
        [0.85, 0.62, 0.45],
        [0.72, 0.55, 0.78],
        [0.50, 0.75, 0.72],
        [0.82, 0.70, 0.45],
        [0.60, 0.60, 0.66],
        [0.75, 0.52, 0.58],
    ];
    let c = PALETTE[(id as usize) % PALETTE.len()];
    [c[0], c[1], c[2], 1.0]
}

/// 一帧产物(rgba8 紧凑字节 + 诊断面)。
pub struct FramePixels {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    pub device_name: String,
    pub draws: usize,
    pub truncated: bool,
    pub nonzero: usize,
    /// 本帧实际绘制的三角形总数(cube=12/实体;真实网格 = 其 triangle_count)。
    pub triangles: usize,
    /// 因网格解析失败/超上限而回退 cube 的实体数(诚实诊断面)。
    pub mesh_fallbacks: usize,
    /// 本帧使用的不同网格类数(不含内置 cube)。
    pub mesh_classes: usize,
    /// 本帧会话是否直渲进 D3D12 共享纹理(F1 wave.3 零拷贝档证据面)。
    pub imported: bool,
}

impl FramePixels {
    pub fn pixels_b64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(&self.rgba8)
    }
}

/// 渲一帧:场景 → rgba8。无设备 → `DEV_ENV_DEGRADE:` 前缀 Err(诚实档)。
pub fn render_scene_frame(
    scene: &Scene,
    cam: &EditorCamera,
    selected: Option<u64>,
    width: u32,
    height: u32,
    want_readback: bool,
) -> Result<FramePixels, String> {
    let slot = renderer_slot();
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());

    // ── 网格类解析(资产→视口接线) ──
    // 可渲染实体 + 网格引用;类 0 = 内置 cube/回退,类 1.. = 不同 mesh 引用(首现序)。
    let renderables: Vec<(&forge_scene::Entity, String)> = scene
        .entities
        .iter()
        .filter(|e| is_renderable(e))
        .map(|e| (e, entity_mesh_ref(e)))
        .collect();
    let renderable_n = renderables.len();
    let want_slots = slot_tier(renderable_n);
    let want_import = current_import(width, height);

    let mut class_refs: Vec<String> = Vec::new();
    let mut class_mesh: Vec<&'static MeshGpu> = Vec::new();
    let mut entity_class: Vec<usize> = Vec::with_capacity(renderable_n);
    let mut mesh_fallbacks = 0usize;
    for (_, mesh_ref) in &renderables {
        if mesh_ref == "cube" {
            entity_class.push(0);
            continue;
        }
        let ci = match class_refs.iter().position(|c| c == mesh_ref) {
            Some(i) => i + 1,
            None => {
                if class_refs.len() >= meshres::MAX_MESH_CLASSES {
                    mesh_fallbacks += 1;
                    entity_class.push(0);
                    continue;
                }
                match meshres::load_mesh_static_cached(&crate::rpc::project_root(), mesh_ref) {
                    Ok(m) => {
                        class_refs.push(mesh_ref.clone());
                        class_mesh.push(m);
                        class_refs.len()
                    }
                    Err(_) => {
                        // 失败详情已由 meshres 缓存路径 eprintln 一次;此处回退 cube。
                        mesh_fallbacks += 1;
                        entity_class.push(0);
                        continue;
                    }
                }
            }
        };
        entity_class.push(ci);
    }

    // ── 类槽位布局:cube 类先保(其实体必绘),非 cube 类按需分配,余量归 cube ──
    let mut class_count = vec![0usize; class_refs.len() + 1];
    for &c in &entity_class {
        class_count[c] += 1;
    }
    let cube_n = class_count[0];
    let mut class_slots = vec![0usize; class_refs.len() + 1];
    let mut used_noncube = 0usize;
    let avail = want_slots.saturating_sub(cube_n.min(want_slots));
    for ci in 1..=class_refs.len() {
        let take = class_count[ci].min(avail - used_noncube);
        class_slots[ci] = take;
        used_noncube += take;
    }
    class_slots[0] = want_slots - used_noncube;

    // 槽位→网格类表(构建期定案,帧内实体按类序占槽):
    // cube 类占 [0..class_slots[0]),各 mesh 类紧随。
    let mut slot_mesh: Vec<Option<&'static MeshGpu>> = vec![None; want_slots];
    let mut next = class_slots[0];
    for (ci, m) in class_mesh.iter().enumerate() {
        for _ in 0..class_slots[ci + 1] {
            slot_mesh[next] = Some(m);
            next += 1;
        }
    }

    // 网格类布局签名(布局变化 → 会话重建;与改尺寸同路径)。
    let mut sig_src = Vec::new();
    sig_src.extend_from_slice(&width.to_le_bytes());
    sig_src.extend_from_slice(&height.to_le_bytes());
    sig_src.extend_from_slice(&(want_slots as u64).to_le_bytes());
    sig_src.extend_from_slice(&want_import.map(|im| im.handle).unwrap_or(0).to_le_bytes());
    sig_src.extend_from_slice(&want_import.map(|im| im.size).unwrap_or(0).to_le_bytes());
    for (r, ks) in class_refs.iter().zip(class_slots[1..].iter()) {
        sig_src.extend_from_slice(r.as_bytes());
        sig_src.push(0xff);
        sig_src.extend_from_slice(&(*ks as u32).to_le_bytes());
    }
    for m in &class_mesh {
        sig_src.extend_from_slice(&m.bytes.len().to_le_bytes());
    }
    let mesh_sig = meshres::fnv1a64(&sig_src);

    // 懒初始化 / 改尺寸或共享纹理 import 键变化或网格类布局变化时重建
    // (降级态一经判定即缓存,不重试)。F6 wave.5:实体数超当前 pass 档 → 升档重建。
    match &*guard {
        RendererState::Uninit => {
            *guard = match build_session(width, height, want_slots, &slot_mesh, mesh_sig) {
                Ok(r) => RendererState::Ready(r),
                Err(e) => RendererState::Degraded(e),
            };
        }
        RendererState::Ready(r)
            if r.width != width
                || r.height != height
                || r.import_key != want_import.map(|im| (im.handle, im.size))
                || r.slots < want_slots
                || r.mesh_sig != mesh_sig =>
        {
            *guard = match build_session(width, height, want_slots, &slot_mesh, mesh_sig) {
                Ok(r) => RendererState::Ready(r),
                Err(e) => RendererState::Degraded(e),
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

    let aspect = width as f32 / height as f32;
    let vp_bytes = m4_col_bytes(cam.view_proj(aspect));

    let mut update = rex::FrameUpdate {
        buffer_uploads: vec![(rex::StableResourceId(2), 0, vp_bytes.to_vec())],
        ..Default::default()
    };
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
    order.sort_by_key(|&i| entity_class[i]);
    let mut drawn_class = vec![0usize; class_refs.len() + 1];
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
        let color = entity_color(e.id, selected == Some(e.id));
        let mut pc = Vec::with_capacity(80);
        pc.extend_from_slice(&m4_col_bytes(model));
        for f in color {
            pc.extend_from_slice(&f.to_le_bytes());
        }
        let slot_idx = class_start[c] + drawn_class[c];
        update.push_constant_overrides.push((slot_idx as u32, pc));
        triangles += if c == 0 {
            12
        } else {
            class_mesh[c - 1].triangle_count as usize
        };
        drawn_class[c] += 1;
        draws += 1;
    }
    // 类内未占槽恒推回消隐模型(上帧可能有更多实体;80B×128 开销可忽略)。
    let hidden_model = trs_model(&Transform {
        translation: [0.0, -1000.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1e-6, 1e-6, 1e-6],
    });
    for (c, k) in class_slots.iter().enumerate() {
        for j in drawn_class[c]..*k {
            let mut pc = Vec::with_capacity(80);
            pc.extend_from_slice(&m4_col_bytes(hidden_model));
            pc.extend_from_slice(&[0u8; 16]);
            update.push_constant_overrides.push(((class_start[c] + j) as u32, pc));
        }
    }
    // F6 wave.5 瓶颈分解:format=none 性能测量档不回读(跳过 submit→wait 同步
    // 阻塞 + 8MB 拷贝 + 2M 像素统计),纯渲染+提交产能与帧通道端到端成本可拆分留档。
    update.readback_subset = if want_readback { Some(vec![0]) } else { None };

    let provenance = r
        .session
        .next_provenance_with_update(&update)
        .map_err(|e| format!("帧 provenance 推导失败: {e}"))?;
    let out = r
        .session
        .execute_with_frame_update(&provenance, &update)
        .map_err(|e| format!("帧执行失败: {e}"))?;
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
        let bg = [
            (CLEAR_RGBA[0] * 255.0 + 0.5).floor() as u8,
            (CLEAR_RGBA[1] * 255.0 + 0.5).floor() as u8,
            (CLEAR_RGBA[2] * 255.0 + 0.5).floor() as u8,
        ];
        let nonzero = rgba8
            .chunks_exact(4)
            .filter(|p| p[0] != bg[0] || p[1] != bg[1] || p[2] != bg[2])
            .count();
        (rgba8, nonzero)
    } else {
        (Vec::new(), 0)
    };
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

// ─────────────────────────── 测试(host 腿恒跑;device 腿见 tests/f1_viewport.rs) ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> EditorCamera {
        EditorCamera::default()
    }

    #[test]
    fn camera_center_ray_points_at_target() {
        let c = cam();
        let (o, d) = c.ray(0.0, 0.0, 16.0 / 9.0);
        let to_target = v3_norm(v3_sub(c.target, o));
        for i in 0..3 {
            assert!((d[i] - to_target[i]).abs() < 1e-5, "中心射线应指向 target");
        }
    }

    #[test]
    fn obb_hit_miss_and_nearest_order() {
        let tr = Transform {
            translation: [0.0, 0.0, -5.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0, 1.0, 1.0],
        };
        // 正面命中:t ≈ 4.5(前面 z=-4.5)。
        let t = ray_unit_cube([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], &tr).expect("应命中");
        assert!((t - 4.5).abs() < 1e-4, "命中参数错:{t}");
        // 偏离未命中。
        assert!(ray_unit_cube([3.0, 0.0, 0.0], [0.0, 0.0, -1.0], &tr).is_none());
        // 背后命中拒绝(tmax<0)。
        assert!(ray_unit_cube([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], &tr).is_none());
        // 缩放×2 后前面 z=-4。
        let tr2 = Transform { scale: [2.0, 2.0, 2.0], ..tr };
        let t2 = ray_unit_cube([0.0, 0.0, 0.0], [0.0, 0.0, -1.0], &tr2).expect("应命中");
        assert!((t2 - 4.0).abs() < 1e-4, "缩放后命中参数错:{t2}");
    }

    #[test]
    fn pick_returns_nearest_entity() {
        let mut scene = Scene::new("t");
        let mk = |id: u64, x: f32, z: f32| forge_scene::Entity {
            id,
            name: format!("e{id}"),
            transform: Transform {
                translation: [x, 0.0, z],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
            components: vec![forge_scene::Component::new(
                "MeshRenderer",
                serde_json::json!({"mesh": "cube", "material": "m"}),
            )],
        };
        // 近者(z=-4)应遮挡远者(z=-8);x=0 位于画面中心附近。
        scene.entities.push(mk(7, 0.0, -8.0));
        scene.entities.push(mk(3, 0.0, -4.0));
        let mut c = cam();
        c.target = [0.0, 0.0, -4.0];
        c.dist = 4.0;
        c.pitch_deg = 0.0;
        c.yaw_deg = 0.0; // 眼在 target 后 +z 向(z=0),看向 -z
        let hit = pick_entity(&scene, &c, 320.0, 180.0, 640, 360).expect("应命中");
        assert_eq!(hit.0, 3, "应取近者");
    }

    #[test]
    fn mesh_and_shader_bytes_wellformed() {
        assert_eq!(cube_mesh_bytes().len(), 36 * 24);
        let (vs, fs) = shader_bytes().expect("着色器编译应成功");
        // SPIR-V magic 0x07230203 小端。
        assert_eq!(&vs[..4], &[0x03, 0x02, 0x23, 0x07]);
        assert_eq!(&fs[..4], &[0x03, 0x02, 0x23, 0x07]);
    }
}
