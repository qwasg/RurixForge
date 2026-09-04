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

fn v3_add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
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

/// 正交投影(F-GAME-3 2D 支持;RH,Vulkan NDC z∈[0,1],y-flip 约定同 perspective_vk)。
/// half_h = 半高(世界单位),半宽 = half_h × aspect;z=-near→0、z=-far→1。
fn orthographic_vk(half_h: f32, aspect: f32, near: f32, far: f32) -> M4 {
    let hh = half_h.max(1e-4);
    let hw = (hh * aspect).max(1e-4);
    let mut m = [[0.0f32; 4]; 4];
    m[0][0] = 1.0 / hw;
    m[1][1] = -1.0 / hh;
    m[2][2] = 1.0 / (near - far);
    m[2][3] = near / (near - far);
    m[3][3] = 1.0;
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
/// F-GAME-3:ortho=true 切换正交投影(2D 模式;ortho_half_h 为半高世界单位,
/// dist 仍决定眼位/近远裁剪,客户端 2D 手势把 yaw/pitch 归零得正对 XY 平面视图)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditorCamera {
    pub target: V3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub dist: f32,
    pub fov_y_deg: f32,
    pub ortho: bool,
    pub ortho_half_h: f32,
}

impl Default for EditorCamera {
    fn default() -> Self {
        EditorCamera {
            target: [0.0, 0.5, 0.0],
            yaw_deg: 35.0,
            pitch_deg: 28.0,
            dist: 9.0,
            fov_y_deg: 50.0,
            ortho: false,
            ortho_half_h: 5.0,
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
    /// proj Y 对角元取负:Vulkan readback/共享纹理行序底朝上,显示面(浏览器 canvas/
    /// 视频)按顶行先行呈现——投影侧统一垂直翻转后显示直立,且与 viewport_pick 的
    /// 屏幕→射线映射(yny 向上为正)同向;right 轴不动,不引入水平镜像(F-GAME-2)。
    pub fn view_proj(&self, aspect: f32) -> M4 {
        let mut proj = if self.ortho {
            orthographic_vk(self.ortho_half_h, aspect.max(1e-6), 0.05, 500.0)
        } else {
            perspective_vk(self.fov_y_deg.to_radians(), aspect.max(1e-6), 0.05, 500.0)
        };
        proj[1][1] = -proj[1][1];
        let view = look_at_rh(self.eye(), self.target, [0.0, 1.0, 0.0]);
        m4_mul(proj, view)
    }

    /// 屏幕归一化坐标 (nx,ny ∈ [-1,1],y 向上为正) → 世界射线 (origin, dir 归一)。
    /// 正交分支(F-GAME-3):平行射线——原点 = 眼平面偏移点,方向 = 相机前向。
    pub fn ray(&self, nx: f32, ny: f32, aspect: f32) -> (V3, V3) {
        let (r, u, f) = self.basis();
        if self.ortho {
            let hh = self.ortho_half_h.max(1e-4);
            let hw = hh * aspect.max(1e-6);
            let eye = self.eye();
            let origin = [
                eye[0] + r[0] * nx * hw + u[0] * ny * hh,
                eye[1] + r[1] * nx * hw + u[1] * ny * hh,
                eye[2] + r[2] * nx * hw + u[2] * ny * hh,
            ];
            return (origin, f);
        }
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
            "ortho": self.ortho,
            "orthoSize": self.ortho_half_h,
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

/// 实体是否参与视口渲染/点选(enabled MeshRenderer 或 enabled Sprite)。
fn is_renderable(e: &forge_scene::Entity) -> bool {
    e.components
        .iter()
        .any(|c| (c.ctype == "MeshRenderer" || c.ctype == "Sprite") && c.enabled)
}

/// 启用态 Sprite 组件(F-GAME-3 2D 精灵)。
fn sprite_component(e: &forge_scene::Entity) -> Option<&forge_scene::Component> {
    e.components
        .iter()
        .find(|c| c.ctype == "Sprite" && c.enabled)
}

/// Sprite 排序键(sortingOrder;缺省/非 Sprite = 0.0,与旧贴图 quad 行为一致)。
fn sprite_sorting_order(e: &forge_scene::Entity) -> f64 {
    sprite_component(e)
        .and_then(|c| c.props.get("sortingOrder").and_then(Value::as_f64))
        .unwrap_or(0.0)
}

/// Sprite 数值字段读取(带缺省,与 forge-scene 注册表缺省一致)。
fn sprite_num(c: &forge_scene::Component, key: &str, default: f64) -> f64 {
    c.props.get(key).and_then(Value::as_f64).unwrap_or(default)
}

/// Sprite 布尔字段读取(缺省 false)。
fn sprite_bool(c: &forge_scene::Component, key: &str) -> bool {
    c.props.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// 整图 uv_rect(offset 0 + 全尺寸;texture 直贴模式 / 消隐槽用)。
const FULL_UV_RECT: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
/// 贴图精灵槽 push constants 字节数(F-GAME-4:model 64 + tint 16 + tex_size 8 +
/// flip flags 8 + uv_rect 16 = 112 ≤ 128 上限)。
const SPRITE_PC_LEN: usize = 112;

/// .rxsprite 文档缓存(逐 GUID 2s TTL:精灵编辑器/agent 随时改写 bbox/clip,
/// 不能像贴图那样泄漏进程级;解析失败缓存 None 同 TTL,避免坏文档逐帧刷盘)。
pub fn sprite_doc_cached(sprite_guid: &str) -> Option<std::sync::Arc<assetd::sprite::SpriteDoc>> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    type Entry = (std::time::Instant, Option<Arc<assetd::sprite::SpriteDoc>>);
    static CACHE: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((at, doc)) = cache.lock().unwrap().get(sprite_guid) {
        if at.elapsed() < std::time::Duration::from_secs(2) {
            return doc.clone();
        }
    }
    let loaded = (|| {
        let guid_map = content_guid_map_cached();
        let path = guid_map.get(sprite_guid)?;
        assetd::sprite::load_rxsprite(path).ok().map(Arc::new)
    })();
    cache
        .lock()
        .unwrap()
        .insert(sprite_guid.to_string(), (std::time::Instant::now(), loaded.clone()));
    loaded
}

/// Sprite 实体的渲染解析结果(texture 直贴 / .rxsprite 图集两模式统一出口)。
pub struct SpriteRenderInfo {
    pub tex: &'static TexGpu,
    /// 图集子矩形(offset.xy + scale.zw,0..1 贴图空间;整图 = FULL_UV_RECT)。
    pub uv_rect: [f32; 4],
    /// 当前帧像素宽高(世界尺寸 = scale × 帧像素/ppu)。
    pub frame_px: [f32; 2],
    /// 图像空间锚点(0..1,y 向下;texture 直贴恒 [0.5,0.5] 居中 = F-GAME-3 行为不变)。
    pub pivot: [f32; 2],
}

/// 解析 Sprite 组件当前帧(F-GAME-4):
/// - `sprite`(.rxsprite GUID)非空 → 图集模式:clip 非空取 clip 第 frame 帧
///   (钳制到末帧);clip 空则 frame 为 frames 键序下标;frames 为空 → 整图 + 文档 pivot;
/// - 否则 `texture` 非空 → 整图直贴(居中锚,行为与 F-GAME-3 一致);
/// - 两者皆空/解析失败 → None(实体回退 cube 占位腿,与既有坏引用行为同形)。
fn resolve_sprite_render(c: &forge_scene::Component) -> Option<SpriteRenderInfo> {
    let project_root = crate::rpc::project_root();
    let sprite_guid = c.props.get("sprite").and_then(Value::as_str).unwrap_or("");
    if !sprite_guid.is_empty() {
        let doc = sprite_doc_cached(sprite_guid)?;
        let tex = load_tex_static_cached(&project_root, &doc.texture)?;
        let clip_name = c.props.get("clip").and_then(Value::as_str).unwrap_or("");
        let frame_idx = sprite_num(c, "frame", 0.0).max(0.0) as usize;
        let frame = if !clip_name.is_empty() {
            doc.clips.get(clip_name).and_then(|clip| {
                let idx = frame_idx.min(clip.frames.len().saturating_sub(1));
                let name = clip.frames.get(idx)?;
                doc.frames.get(name).map(|f| (name.clone(), f.clone()))
            })
        } else {
            let keys: Vec<&String> = doc.frames.keys().collect();
            keys.get(frame_idx.min(keys.len().saturating_sub(1)))
                .map(|k| ((*k).clone(), doc.frames[*k].clone()))
        };
        return Some(match frame {
            Some((name, f)) => {
                let (tw, th) = (tex.w.max(1) as f32, tex.h.max(1) as f32);
                let bbox = f.bbox;
                SpriteRenderInfo {
                    tex,
                    uv_rect: [
                        bbox[0] as f32 / tw,
                        bbox[1] as f32 / th,
                        bbox[2] as f32 / tw,
                        bbox[3] as f32 / th,
                    ],
                    frame_px: [bbox[2] as f32, bbox[3] as f32],
                    pivot: doc.resolve_pivot(&name),
                }
            }
            // frames 为空(新建未切帧):整图 + 文档 pivot(诚实可见,编辑器可继续切)。
            None => SpriteRenderInfo {
                tex,
                uv_rect: FULL_UV_RECT,
                frame_px: [tex.w as f32, tex.h as f32],
                pivot: doc.pivot,
            },
        });
    }
    let tex_guid = c.props.get("texture").and_then(Value::as_str).unwrap_or("");
    if tex_guid.is_empty() {
        return None;
    }
    let tex = load_tex_static_cached(&project_root, tex_guid)?;
    Some(SpriteRenderInfo {
        tex,
        uv_rect: FULL_UV_RECT,
        frame_px: [tex.w as f32, tex.h as f32],
        pivot: [0.5, 0.5],
    })
}

/// Sprite 实体渲染态等效变换(F-GAME-4:帧尺寸缩放 + pivot 锚定平移),
/// 渲染模型矩阵与点选 OBB 共用,保证画面与点选一致。返回 None = 非 Sprite 实体。
/// 锚定:图像空间 pivot(y 向下)→ 单位 quad 局部偏移 (0.5-px, py-0.5),
/// 经旋转与有效缩放折入 translation,使锚点恰落在实体 translation 上。
fn sprite_render_transform(e: &forge_scene::Entity) -> Option<Transform> {
    let c = sprite_component(e)?;
    let ppu = sprite_num(c, "pixelsPerUnit", 100.0).max(1.0) as f32;
    let info = resolve_sprite_render(c);
    let (fw, fh, pivot) = match &info {
        Some(i) => (i.frame_px[0], i.frame_px[1], i.pivot),
        // 贴图未解析:回退 1×1 居中(与旧 quad 腿同形)。
        None => (ppu, ppu, [0.5, 0.5]),
    };
    let s = e.transform.scale;
    let scale = [s[0] * fw / ppu, s[1] * fh / ppu, s[2].max(1e-3)];
    let offset_local = [0.5 - pivot[0], pivot[1] - 0.5, 0.0];
    let rot = quat_to_mat3(e.transform.rotation);
    let world_off = m3_apply(
        rot,
        [
            offset_local[0] * scale[0],
            offset_local[1] * scale[1],
            0.0,
        ],
    );
    Some(Transform {
        translation: v3_add(e.transform.translation, world_off),
        rotation: e.transform.rotation,
        scale,
    })
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
        // F-GAME-3/4:Sprite 实体按渲染态变换(帧尺寸 + pivot 锚定)参与点选,与画面一致。
        let tr = sprite_render_transform(e).unwrap_or(e.transform);
        if let Some(t) = ray_unit_cube(origin, dir, &tr) {
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

/// ── 贴图精灵管线(F-GAME-2:2D 游戏「真实画面」腿) ──
/// 每个带材质 albedo 的实体 = 一张朝 +z 的单位四边形,逐槽绑定 albedo 纹理采样;
/// 品红底色键 discard(生成素材的 chroma-key 约定)。

/// 已解码贴图(进程级缓存,'static 泄漏与会话同生命周期纪律)。
pub struct TexGpu {
    pub w: u32,
    pub h: u32,
    pub rgba: &'static [u8],
}

/// 贴图 GUID → 解码缓存。
pub fn load_tex_static_cached(project: &std::path::Path, tex_guid: &str) -> Option<&'static TexGpu> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static TEX_CACHE: OnceLock<Mutex<HashMap<String, Option<&'static TexGpu>>>> = OnceLock::new();
    let cache = TEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(t) = cache.lock().unwrap().get(tex_guid) {
        return *t;
    }
    let loaded = (|| {
        let guid_map = content_guid_map_cached();
        let tex_path = guid_map.get(tex_guid)?;
        let (w, h, rgba) = assetd::texture::decode_rgba(tex_path).ok()?;
        let rgba: &'static [u8] = Box::leak(rgba.into_boxed_slice());
        Some(TexGpu { w, h, rgba })
    })();
    let leaked: Option<&'static TexGpu> = loaded.map(|t| &*Box::leak(Box::new(t)));
    cache
        .lock()
        .unwrap()
        .insert(tex_guid.to_string(), leaked);
    leaked
}

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
struct PushConsts { model: mat4x4<f32>, color: vec4<f32>, tex_size: vec2<u32>, flags: vec2<f32>, uv_rect: vec4<f32>, };
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
struct PushConsts { model: mat4x4<f32>, color: vec4<f32>, tex_size: vec2<u32>, flags: vec2<f32>, uv_rect: vec4<f32>, };
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
    if (texel.a < 0.02) { discard; }
    // 品红族色键(生成精灵 chroma-key 背景及渐变边):G 显著低于 R/B 两者即弃。
    // 素材里绿身(g 高)/蓝天(r 低)/红屋(b 低)/黄瓣(g 高)均不受误伤。
    let g_dom = 0.5 * min(texel.r, texel.b);
    if (texel.g < g_dom) { discard; }
    // F-GAME-3:pc.color = Sprite.tint(旧贴图 quad 路径恒推白色,行为不变)。
    return vec4<f32>(texel.rgb * pc.color.rgb, pc.color.a);
}
"#;

fn shader_tex_bytes() -> Result<(&'static [u8], &'static [u8]), String> {
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

/// 诊断:本帧是否发生了会话重建(性能定位用)。
static REBUILD_FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 最近一次会话重建时刻(重建防抖用)。
static LAST_REBUILD: Mutex<Option<std::time::Instant>> = Mutex::new(None);

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
    slot_tex: &[Option<&'static TexGpu>],
    mesh_sig: u64,
) -> Result<ViewportRenderer, String> {
    let import = current_import(width, height);
    match build_session_with(width, height, import, slots, slot_mesh, slot_tex, mesh_sig) {
        Ok(r) => Ok(r),
        Err(e) if import.is_some() => {
            eprintln!("[viewport] 零拷贝 import 会话创建失败,回退 readback 上传腿: {e}");
            build_session_with(width, height, None, slots, slot_mesh, slot_tex, mesh_sig)
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
    slot_tex: &[Option<&'static TexGpu>],
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
    for t in slot_tex.iter().flatten() {
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
            // host-visible 直写上传(cube VB 同款;512² RGBA ≈1MB/张,shader 读 PCIe 带宽足够)。
            device_local: false,
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

    let mut passes: Vec<rex::Pass> = Vec::with_capacity(slots);
    let mut barrier_plan: Vec<Vec<(u32, rex::TargetState)>> = Vec::with_capacity(slots);
    for (k, s) in slot_mesh.iter().enumerate() {
        let first = k == 0;
        if let Some(t) = slot_tex[k] {
            // 贴图精灵槽:四边形 + albedo 采样(品红色键在 FS 内 discard)。
            passes.push(rex::Pass::Raster(rex::RasterPass {
                name: "forge_viewport_sprite",
                vs_spirv: vs_tex,
                fs_spirv: fs_tex,
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
                    storage_buffers: vec![tex_res(t)],
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
            (import_res, rex::TargetState::StorageReadWrite),
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

/// 实体 MeshRenderer.material(GUID)→ albedo 贴图平均色。
/// 视口渲染管线尚无纹理采样(逐实体纯色 push constant),用贴图平均色着色是
/// 「真实生成素材可见」的保真替身(F-GAME-2);解码结果按材质 GUID 进程级缓存。
fn entity_tint(e: &forge_scene::Entity, selected: bool) -> [f32; 4] {
    if selected {
        return [1.0, 0.62, 0.18, 1.0];
    }
    let mat_guid = e
        .components
        .iter()
        .find(|c| c.ctype == "MeshRenderer" && c.enabled)
        .and_then(|c| c.props.get("material"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if mat_guid.is_empty() {
        return entity_color(e.id, false);
    }
    material_avg_color(&mat_guid).unwrap_or_else(|| entity_color(e.id, false))
}

/// 材质 GUID → albedo 平均色(进程级缓存;Content 资产量级 <百,首帧扫盘一次可接受)。
fn material_avg_color(mat_guid: &str) -> Option<[f32; 4]> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, Option<[f32; 4]>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(c) = cache.lock().unwrap().get(mat_guid) {
        return *c;
    }
    let color = resolve_material_avg_color(mat_guid);
    cache
        .lock()
        .unwrap()
        .insert(mat_guid.to_string(), color);
    color
}

/// guid → Content 内源文件路径映射(进程级缓存:render_scene_frame 每帧逐实体解析
/// 材质,无缓存时每帧数百次 .meta 文件 IO,实测单帧 4-6s 全卡在这——F-GAME-2)。
/// 未命中且距上次构建 >2s 时重建一次(容纳 agent 运行中新导入的资产)。
fn content_guid_map_cached() -> std::collections::HashMap<String, std::path::PathBuf> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Map = HashMap<String, std::path::PathBuf>;
    static CACHE: OnceLock<Mutex<(std::time::Instant, Map)>> = OnceLock::new();
    let cell = CACHE.get_or_init(|| {
        Mutex::new((
            std::time::Instant::now() - std::time::Duration::from_secs(3600),
            HashMap::new(),
        ))
    });
    let mut guard = cell.lock().unwrap();
    let (built_at, map) = &mut *guard;
    if map.is_empty() || built_at.elapsed() > std::time::Duration::from_secs(2) {
        let project = crate::rpc::project_root();
        *map = content_guid_map(&project.join("Content"));
        *built_at = std::time::Instant::now();
    }
    map.clone()
}

/// guid → Content 内源文件路径映射(扫全部 .meta;失配容忍——无 meta 的文件跳过)。
fn content_guid_map(content: &std::path::Path) -> std::collections::HashMap<String, std::path::PathBuf> {
    let mut map = std::collections::HashMap::new();
    let mut stack = vec![content.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if p.extension().and_then(|x| x.to_str()) == Some("meta") {
                if let Ok(doc) = assetd::meta::MetaDoc::load(&p) {
                    // 源文件 = sidecar 去掉 .meta 后缀
                    let src = p.with_extension("");
                    if src.is_file() {
                        map.insert(doc.guid, src);
                    }
                }
            }
        }
    }
    map
}

/// 解析材质:mat guid → .rxmat 文件 → textures.albedo guid → 贴图文件 → 平均色。
fn resolve_material_avg_color(mat_guid: &str) -> Option<[f32; 4]> {
    let guid_map = content_guid_map_cached();
    let mat_path = guid_map.get(mat_guid)?;
    let mat_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(mat_path).ok()?).ok()?;
    let albedo_guid = mat_json
        .get("textures")?
        .get("albedo")?
        .as_str()?
        .to_string();
    let tex_path = guid_map.get(&albedo_guid)?;
    assetd::texture::decode_average_rgba(tex_path).ok()
}

/// 材质 GUID → albedo 贴图 GUID(贴图精灵槽分类用)。
fn material_albedo_guid(mat_guid: &str, _project: &std::path::Path) -> Option<String> {
    let guid_map = content_guid_map_cached();
    let mat_path = guid_map.get(mat_guid)?;
    let mat_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(mat_path).ok()?).ok()?;
    mat_json
        .get("textures")?
        .get("albedo")?
        .as_str()
        .map(str::to_string)
}

/// 场景相机实体(首个启用 Camera 组件)在归一化设备坐标 (nx, ny ∈ [-1,1],y 向上)处的
/// 世界射线 (origin, dir)。与 [`scene_camera_view_proj`] 同一套相机参数解析,供 play 态
/// 指针输入反投影(logic.inject_pointer):正交 = 平行射线从相机平面出发;透视 = 自相机眼
/// 发散。无相机实体返回 None(调用方退回编辑器相机)。
pub fn scene_camera_ray(scene: &Scene, nx: f32, ny: f32, aspect: f32) -> Option<(V3, V3)> {
    let cam = scene.entities.iter().find(|e| {
        e.components
            .iter()
            .any(|c| c.ctype == "Camera" && c.enabled)
    })?;
    let props = cam
        .components
        .iter()
        .find(|c| c.ctype == "Camera")
        .map(|c| &c.props)?;
    let rot = quat_to_mat3(cam.transform.rotation);
    let right = v3_norm(m3_apply(rot, [1.0, 0.0, 0.0]));
    let up = v3_norm(m3_apply(rot, [0.0, 1.0, 0.0]));
    let fwd = v3_norm(m3_apply(rot, [0.0, 0.0, -1.0]));
    let eye = cam.transform.translation;
    let projection = props
        .get("projection")
        .and_then(|v| v.as_str())
        .unwrap_or("perspective");
    if projection == "orthographic" {
        let hh = props.get("orthoSize").and_then(|v| v.as_f64()).unwrap_or(5.0) as f32;
        let hh = hh.max(1e-4);
        let hw = hh * aspect.max(1e-6);
        let origin = [
            eye[0] + right[0] * nx * hw + up[0] * ny * hh,
            eye[1] + right[1] * nx * hw + up[1] * ny * hh,
            eye[2] + right[2] * nx * hw + up[2] * ny * hh,
        ];
        return Some((origin, fwd));
    }
    let fov_deg = props.get("fov").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;
    let t = (fov_deg.to_radians() * 0.5).tan();
    let dir = v3_norm([
        right[0] * nx * t * aspect + up[0] * ny * t + fwd[0],
        right[1] * nx * t * aspect + up[1] * ny * t + fwd[1],
        right[2] * nx * t * aspect + up[2] * ny * t + fwd[2],
    ]);
    Some((eye, dir))
}

/// 屏外裁剪的边距倍数:只裁「停车位」级别的远离(四角全在 3 倍视口范围之外),
/// 贴边进出的实体(右侧刷出的僵尸、飞出屏的豌豆、开走的小推车)不裁——
/// 可见集每变一次 pass 会话就要按新的贴图槽签名重建,逐帧进出会造成重建抖动。
const OFFSCREEN_CULL_MARGIN: f32 = 3.0;

/// Sprite 实体是否停在远屏外(保守四角裁剪):渲染态 quad 四角经 view_proj 投到裁剪空间,
/// 四角同侧越界(全在 x>3w / x<-3w / y>3w / y<-3w)或全在相机后方即视为屏外。
/// 用途:2D 游戏对象池把闲置实体停在屏外(如 y=-60),此前仍逐个占 draw 槽,
/// 128 槽预算被池子吃掉;屏外精灵不进 renderables 后,槽位只留给真正可见的实体。
/// 非 Sprite 实体(3D 网格/cube)不裁,行为不变。
fn sprite_offscreen(e: &forge_scene::Entity, vp: &M4) -> bool {
    let Some(tr) = sprite_render_transform(e) else {
        return false;
    };
    let model = trs_model(&tr);
    let mut outside = [true; 5]; // +x, -x, +y, -y, behind
    for (lx, ly) in [(-0.5f32, -0.5f32), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
        let local = [lx, ly, 0.0, 1.0];
        let mut world = [0.0f32; 4];
        for r in 0..4 {
            world[r] = (0..4).map(|k| model[r][k] * local[k]).sum();
        }
        let mut clip = [0.0f32; 4];
        for r in 0..4 {
            clip[r] = (0..4).map(|k| vp[r][k] * world[k]).sum();
        }
        let w = clip[3];
        if w > 1e-6 {
            outside[4] = false;
        }
        let bound = w.abs().max(1e-6) * OFFSCREEN_CULL_MARGIN;
        if clip[0] <= bound {
            outside[0] = false;
        }
        if clip[0] >= -bound {
            outside[1] = false;
        }
        if clip[1] <= bound {
            outside[2] = false;
        }
        if clip[1] >= -bound {
            outside[3] = false;
        }
    }
    outside.iter().any(|o| *o)
}

/// 场景内置相机实体(首个启用 Camera 组件)的 view_proj。
/// PIE 期间视口以此驱动(F-GAME-2:游戏画面 = 游戏相机画面);无相机实体返回 None。
pub fn scene_camera_view_proj(scene: &Scene, aspect: f32) -> Option<M4> {
    let cam = scene.entities.iter().find(|e| {
        e.components
            .iter()
            .any(|c| c.ctype == "Camera" && c.enabled)
    })?;
    let props = cam
        .components
        .iter()
        .find(|c| c.ctype == "Camera")
        .map(|c| &c.props)?;
    let fov_deg = props.get("fov").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;
    let near = props.get("near").and_then(|v| v.as_f64()).unwrap_or(0.1) as f32;
    let far = props.get("far").and_then(|v| v.as_f64()).unwrap_or(500.0) as f32;
    // F-GAME-3:projection=orthographic 走正交(2D 游戏相机),orthoSize=半高(世界单位)。
    let projection = props
        .get("projection")
        .and_then(|v| v.as_str())
        .unwrap_or("perspective");
    let ortho_size = props.get("orthoSize").and_then(|v| v.as_f64()).unwrap_or(5.0) as f32;
    let t = &cam.transform;
    let rot = quat_to_mat3(t.rotation);
    let fwd = v3_norm(m3_apply(rot, [0.0, 0.0, -1.0]));
    let eye = t.translation;
    let center = v3_add(eye, fwd);
    // proj Y 对角元取负:与 EditorCamera::view_proj 同一显示朝向约定(见其注释)。
    let mut proj = if projection == "orthographic" {
        orthographic_vk(ortho_size, aspect.max(1e-6), near, far)
    } else {
        perspective_vk(fov_deg.to_radians(), aspect.max(1e-6), near, far)
    };
    proj[1][1] = -proj[1][1];
    Some(m4_mul(proj, look_at_rh(eye, center, [0.0, 1.0, 0.0])))
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
    let want_slots = slot_tier(renderable_n);
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
    let mut qnext = 0usize;
    for &i in &quad_order {
        if qnext < class_slots[0] {
            slot_tex[qnext] = entity_tex[i];
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
        && LAST_REBUILD
            .lock()
            .unwrap()
            .map(|t| t.elapsed() < std::time::Duration::from_millis(1500))
            .unwrap_or(false);
    match &*guard {
        RendererState::Uninit => {
            REBUILD_FLAG.store(true, std::sync::atomic::Ordering::Relaxed);
            *guard = match build_session(width, height, want_slots, &slot_mesh, &slot_tex, mesh_sig)
            {
                Ok(r) => RendererState::Ready(r),
                Err(e) => RendererState::Degraded(e),
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
            *guard = match build_session(width, height, want_slots, &slot_mesh, &slot_tex, mesh_sig)
            {
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

    let vp_bytes = m4_col_bytes(frame_vp);

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
        let slot_idx = class_start[c] + drawn_class[c];
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
            update.push_constant_overrides.push((slot_idx as u32, pc));
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
        for j in drawn_class[c]..*k {
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

    #[test]
    fn tex_shader_bytes_wellformed() {
        let (vs, fs) = shader_tex_bytes().expect("精灵着色器编译应成功");
        assert_eq!(&vs[..4], &[0x03, 0x02, 0x23, 0x07]);
        assert_eq!(&fs[..4], &[0x03, 0x02, 0x23, 0x07]);
    }

    #[test]
    fn ortho_view_proj_maps_extents_to_ndc_edges() {
        // F-GAME-3:正交相机(yaw=0,pitch=0,眼在 target 正 +z)下半高=orthoSize,
        // target 正上方 half_h 处应落在 NDC y=+1(显示面顶行),右侧 half_w 落在 x=+1。
        let mut c = cam();
        c.ortho = true;
        c.ortho_half_h = 5.0;
        c.yaw_deg = 0.0;
        c.pitch_deg = 0.0;
        c.target = [0.0, 0.0, 0.0];
        c.dist = 10.0;
        let aspect = 16.0 / 9.0;
        let vp = c.view_proj(aspect);
        let xform = |p: V3| -> V3 {
            let v = [
                vp[0][0] * p[0] + vp[0][1] * p[1] + vp[0][2] * p[2] + vp[0][3],
                vp[1][0] * p[0] + vp[1][1] * p[1] + vp[1][2] * p[2] + vp[1][3],
                vp[2][0] * p[0] + vp[2][1] * p[1] + vp[2][2] * p[2] + vp[2][3],
            ];
            let w = vp[3][0] * p[0] + vp[3][1] * p[1] + vp[3][2] * p[2] + vp[3][3];
            [v[0] / w, v[1] / w, v[2] / w]
        };
        let top = xform([0.0, 5.0, 0.0]);
        assert!((top[1] - 1.0).abs() < 1e-4, "半高点应达 NDC 顶:{top:?}");
        let right = xform([5.0 * aspect, 0.0, 0.0]);
        assert!((right[0] - 1.0).abs() < 1e-4, "半宽点应达 NDC 右:{right:?}");
        let center = xform([0.0, 0.0, 0.0]);
        assert!(center[0].abs() < 1e-5 && center[1].abs() < 1e-5, "target 应居中:{center:?}");
        // 正交无透视形变:同 y 不同 z 的两点 NDC xy 相同。
        let a = xform([1.0, 2.0, -3.0]);
        let b = xform([1.0, 2.0, -8.0]);
        assert!((a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5, "正交下深度不改 xy:{a:?} vs {b:?}");
    }

    #[test]
    fn ortho_ray_is_parallel() {
        // F-GAME-3:正交射线互相平行(dir 恒为前向),原点随屏幕位置平移。
        let mut c = cam();
        c.ortho = true;
        c.ortho_half_h = 5.0;
        c.yaw_deg = 0.0;
        c.pitch_deg = 0.0;
        c.target = [0.0, 0.0, 0.0];
        c.dist = 10.0;
        let aspect = 16.0 / 9.0;
        let (o0, d0) = c.ray(0.0, 0.0, aspect);
        let (o1, d1) = c.ray(1.0, 1.0, aspect);
        for i in 0..3 {
            assert!((d0[i] - d1[i]).abs() < 1e-6, "正交射线方向须一致");
            assert!((d0[i] - [0.0, 0.0, -1.0][i]).abs() < 1e-6, "yaw0/pitch0 前向须为 -z");
        }
        // 右上角射线的原点应偏移 (+half_w, +half_h) 于眼位 xy。
        assert!((o1[0] - (o0[0] + 5.0 * aspect)).abs() < 1e-4, "x 偏移 = 半宽");
        assert!((o1[1] - (o0[1] + 5.0)).abs() < 1e-4, "y 偏移 = 半高");
    }

    #[test]
    fn scene_camera_orthographic_branch() {
        // F-GAME-3:Camera 组件 projection=orthographic → PIE 正交;缺省仍透视。
        let mk_scene = |proj: Option<&str>| {
            let mut s = Scene::new("t");
            let props = match proj {
                Some(p) => serde_json::json!({"projection": p, "orthoSize": 4.0, "fov": 60.0, "near": 0.1, "far": 100.0}),
                None => serde_json::json!({"fov": 60.0, "near": 0.1, "far": 100.0}),
            };
            s.entities.push(forge_scene::Entity {
                id: 1,
                name: "cam".into(),
                transform: Transform {
                    translation: [0.0, 0.0, 10.0],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: [1.0; 3],
                },
                components: vec![forge_scene::Component::new("Camera", props)],
            });
            s
        };
        let ortho = scene_camera_view_proj(&mk_scene(Some("orthographic")), 16.0 / 9.0).expect("有相机");
        // 正交矩阵 m[3][2]=0(无透视除法项),m[1][1] 经 y-flip 后为 +1/half_h。
        assert!(ortho[3][2].abs() < 1e-7, "正交无 w 透视项:{ortho:?}");
        assert!((ortho[1][1] - 0.25).abs() < 1e-5, "orthoSize=4 → 1/half_h=0.25:{}", ortho[1][1]);
        let persp = scene_camera_view_proj(&mk_scene(None), 16.0 / 9.0).expect("有相机");
        assert!((persp[3][2] + 1.0).abs() < 1e-5, "缺省须为透视(m[3][2]=-1):{persp:?}");
        assert!(scene_camera_view_proj(&Scene::new("空"), 1.0).is_none(), "无相机实体 → None");
    }

    fn ortho_cam_scene(ortho_size: f32) -> Scene {
        let mut s = Scene::with_mode("t", "2d");
        s.entities.push(forge_scene::Entity {
            id: 1,
            name: "cam".into(),
            transform: Transform {
                translation: [0.0, 0.0, 10.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            components: vec![forge_scene::Component::new(
                "Camera",
                serde_json::json!({"projection": "orthographic", "orthoSize": ortho_size, "fov": 60.0, "near": 0.1, "far": 100.0}),
            )],
        });
        s
    }

    /// 指针反投影腿:正交场景相机下 NDC 角点 → 半宽/半高偏移的平行射线(朝 -z),
    /// 屏幕上方(ny=+1)对应世界 +y——与画面(HUD 在上)一致。
    #[test]
    fn scene_camera_ray_orthographic_maps_ndc_to_world_plane() {
        let s = ortho_cam_scene(6.2);
        let aspect = 16.0 / 9.0;
        let (o, d) = scene_camera_ray(&s, 0.0, 0.0, aspect).expect("有相机");
        assert!((o[0]).abs() < 1e-5 && (o[1]).abs() < 1e-5 && (o[2] - 10.0).abs() < 1e-5);
        assert!((d[2] + 1.0).abs() < 1e-5, "正交射线沿 -z:{d:?}");
        let (o1, _) = scene_camera_ray(&s, 1.0, 1.0, aspect).expect("有相机");
        assert!((o1[0] - 6.2 * aspect).abs() < 1e-3, "nx=1 → x=半宽:{}", o1[0]);
        assert!((o1[1] - 6.2).abs() < 1e-4, "ny=1 → y=半高(屏幕上=世界上):{}", o1[1]);
        assert!(scene_camera_ray(&Scene::new("空"), 0.0, 0.0, 1.0).is_none(), "无相机 → None");
    }

    /// 屏外裁剪:停在 y=-60 的池子精灵四角全在裁剪空间下方 → 屏外;屏内精灵不裁;
    /// 非 Sprite 实体恒不裁(3D 网格行为不变)。
    #[test]
    fn sprite_offscreen_culls_parked_pool_entities_only() {
        let s = ortho_cam_scene(6.2);
        let vp = scene_camera_view_proj(&s, 16.0 / 9.0).expect("有相机");
        let sprite = |id: u64, y: f32| forge_scene::Entity {
            id,
            name: format!("s{id}"),
            transform: Transform {
                translation: [0.0, y, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            components: vec![forge_scene::Component::new(
                "Sprite",
                serde_json::json!({"texture": "", "sprite": "", "pixelsPerUnit": 100.0}),
            )],
        };
        assert!(sprite_offscreen(&sprite(1, -60.0), &vp), "y=-60 的池子精灵须判屏外");
        assert!(!sprite_offscreen(&sprite(2, 0.0), &vp), "屏中精灵不裁");
        assert!(!sprite_offscreen(&sprite(3, 6.0), &vp), "贴边(半高 6.2 内)精灵不裁");
        assert!(!sprite_offscreen(&sprite(5, -12.0), &vp), "刚出屏(3 倍边距内)不裁,免会话重建抖动");
        let cube = forge_scene::Entity {
            id: 4,
            name: "cube".into(),
            transform: Transform {
                translation: [0.0, -60.0, 0.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0; 3],
            },
            components: vec![forge_scene::Component::new("MeshRenderer", serde_json::json!({}))],
        };
        assert!(!sprite_offscreen(&cube, &vp), "非 Sprite 实体不裁");
    }
}
