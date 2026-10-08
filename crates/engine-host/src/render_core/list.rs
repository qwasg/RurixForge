//! 分派判据与 Pipelined 帧清单(02 §2.5 / §3.6)。rurix 仍在 `viewport::render_scene_frame` 开头自己判(不改);
//! Godot 腿调 `classify`。两处判据、先后顺序必须逐字一致,单测按夹具锁定。
//! RenderList / extract 属于 §2.5 的 Pipelined 路径(rurix 从不调用)。Stage 4 抽完整 3D 集合(01 §5):
//! sprite_mesh 腿的 MeshRenderer(真网格 / 贴图 quad)、模型腿(ModelRenderer / ModelNode、旧 MeshRenderer)、
//! Light、相机、Animator 姿态(CPU 算好的骨骼矩阵)、ParticleEmitter 事件、V6 的 records / terrain / sprites;
//! 世界矩阵一律调 `modelrt::entity_world` 等中立函数(02 §3),不重写。普通精灵保留独立 SpriteDraw，按场景分流到 Canvas 或 3D quad。

use std::sync::Arc;

use assetd::model::{ModelBundle, ModelMaterial};
use forge_scene::Scene;

use crate::render::snapshot::RenderSnapshot;
use crate::render_core::camera::{editor_view, scene_view, EditorCamera, Projection, ViewSetup};
use crate::render_core::math::{trs_model, M4};

/// 一帧走哪条渲染腿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    /// 缺省腿(viewport 精灵 / 网格)。
    SpriteMesh,
    /// 任一启用 ModelRenderer。
    Model,
    /// scene.name == "Code Sentinels V6" 且含 SentinelsV6Batch。
    SentinelsV6,
}

impl Leg {
    /// 与 render.capabilities 的 legs 取值同名(snake_case)。
    pub fn as_str(self) -> &'static str {
        match self {
            Leg::SpriteMesh => "sprite_mesh",
            Leg::Model => "model",
            Leg::SentinelsV6 => "sentinels_v6",
        }
    }
}

/// 与 render_scene_frame 同判据、同顺序(V6 先于模型腿)。
pub fn classify(scene: &Scene) -> Leg {
    if scene.name == "Code Sentinels V6" && scene.entities.iter().any(|e| e.component("SentinelsV6Batch").is_some()) {
        return Leg::SentinelsV6;
    }
    if scene.entities.iter().any(|e| e.component("ModelRenderer").is_some_and(|c| c.enabled)) || crate::shader::scene_uses_spatial_graph(scene) {
        return Leg::Model;
    }
    Leg::SpriteMesh
}

/// SpriteMesh 腿清屏色(sRGB 编码值,rurix 按 floor(c*255+0.5) 取整为 23,24,29)。
/// viewport/rurix.rs 的 CLEAR_RGBA 引用本常量,两后端同源。
pub const SPRITE_MESH_CLEAR_RGBA: [f32; 4] = [0.090, 0.094, 0.114, 1.0];

/// 模型腿清屏色(与 modelrender/rurix.rs 的 `CLEAR` 同值:线性写进 UNORM 目标,8 bit = 9,11,15)。
pub const MODEL_CLEAR_RGBA: [f32; 4] = [0.035, 0.045, 0.06, 1.0];

/// V6 腿清屏色(sentinels_v6_render/rurix.rs:238,sRGB 编码值,8 bit = 6,10,15)。
pub const V6_CLEAR_RGBA: [f32; 4] = [0.025, 0.04, 0.06, 1.0];

/// V6 腿的一帧(后端中立)。`sentinels_v6_render::staged_frame` 与 rurix `render` 开头同一段 CPU 流程
/// (STAGED → visual_time → compose → interpolate);record = [x, y, z, 0, w, h, height, 1, r, g, b, a],
/// (x, y, z) 是盒子最小角,w 沿 +X、h 沿 +Y、height 沿 +Z(竖直),颜色是 sRGB 编码值(见 02 §9.5 Stage 4 step 8)。
#[derive(Debug, Clone)]
pub struct V6Frame {
    /// 地形薄板(rurix 拼在 Batch.records 最前面);TerrainBatch 换了才换这一份 Arc。
    pub terrain: Arc<Vec<[f32; 12]>>,
    /// 物体盒子(按 x + y + 0.05z 升序)。
    pub objects: Vec<[f32; 12]>,
    /// [cx, cy, half, 16/9]:iso 屏幕中心、半高 24/zoom、剔除用宽高比。
    pub view: [f32; 4],
    pub fallbacks: usize,
    pub sprites: Vec<V6SpriteDraw>,
    /// 本帧跳过的精灵数(Stage 6 之前保留的遥测字段；当前 Godot 路径应为 0)。
    pub skipped_sprites: usize,
}

impl PartialEq for V6Frame {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.terrain, &o.terrain)
            && self.objects.len() == o.objects.len()
            && self.objects.iter().flatten().zip(o.objects.iter().flatten()).all(|(a, b)| a.to_bits() == b.to_bits())
            && self.view.map(f32::to_bits) == o.view.map(f32::to_bits)
            && self.view.map(f32::to_bits) == o.view.map(f32::to_bits)
            && self.fallbacks == o.fallbacks
            && self.sprites == o.sprites
            && self.skipped_sprites == o.skipped_sprites
    }
}

/// 清屏色的 8 bit 值(与 rurix nonzero 统计的背景色同式)。
pub fn clear_rgb8(c: [f32; 4]) -> [u8; 3] {
    [0, 1, 2].map(|i| (c[i] * 255.0 + 0.5).floor() as u8)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemKey {
    pub entity: u64,
    /// 同一实体的第 n 个 draw(实体 id 重复时也靠它区分);MeshRenderer 恒 0。
    pub sub: u32,
}

/// 网格身份(缓存键;内容本身在 MeshData.vertices)。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MeshRef {
    Cube,
    Asset { reference: String },
}

/// 顶点数据:交错 pos3 + normal3(f32 LE,stride 24),三角形炸开、面法线、逆时针为正面
/// (cube = render_core::assets::cube_mesh_bytes,资产 = meshres::load_mesh_static_cached)。
#[derive(Debug, Clone, PartialEq)]
pub struct MeshData {
    pub id: MeshRef,
    pub vertices: &'static [u8],
    pub vertex_count: u32,
    /// 资产解析失败回退了 cube(与 rurix 的 meshFallbacks 同义)。
    pub fallback: bool,
}

/// CPU RGBA8 贴图(`assets::load_tex_static_cached` 的 'static 缓存,sRGB 编码值)。
/// 相等 = 同 guid、同尺寸、同一块缓存(指针相等,不逐字节比)。
#[derive(Clone)]
pub struct TexData {
    pub guid: String,
    pub w: u32,
    pub h: u32,
    pub rgba: &'static [u8],
}

impl PartialEq for TexData {
    fn eq(&self, o: &Self) -> bool {
        self.guid == o.guid && self.w == o.w && self.h == o.h && std::ptr::eq(self.rgba, o.rgba)
    }
}

impl std::fmt::Debug for TexData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TexData({} {}x{})", self.guid, self.w, self.h)
    }
}

/// 模型资产:`key` = rurix Draw.key 里的 `guid:revision:source_hash`,bundle 与 `modelrt::load_revision` 共享。
#[derive(Clone)]
pub struct ModelData {
    pub key: String,
    pub bundle: Arc<ModelBundle>,
}

impl PartialEq for ModelData {
    fn eq(&self, o: &Self) -> bool {
        self.key == o.key && Arc::ptr_eq(&self.bundle, &o.bundle)
    }
}

impl std::fmt::Debug for ModelData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ModelData({})", self.key)
    }
}

/// 模型腿的一个 (node, primitive)。材质已按 `materialOverrides` 覆盖(与 rurix `collect` 同一个
/// `material_override::apply` 调用);`material_fp` = 覆盖后材质的 JSON 指纹,相等按它比。
#[derive(Clone, Debug)]
pub struct ModelPrim {
    pub model: ModelData,
    pub node: usize,
    pub prim: usize,
    pub material: Arc<ModelMaterial>,
    pub material_fp: u64,
    pub graph: Option<Arc<crate::shader::Material>>,
    /// node.skin:Some → 该 item 的 `pose` 是骨骼矩阵,`world` 是实体矩阵(蒙皮顶点不再乘节点矩阵,与 modelrt::vertices 同)。
    pub skin: Option<usize>,
    pub selected: bool,
}

impl PartialEq for ModelPrim {
    fn eq(&self, o: &Self) -> bool {
        self.model == o.model
            && self.graph == o.graph
            && (self.node, self.prim, self.material_fp, self.skin, self.selected)
                == (o.node, o.prim, o.material_fp, o.skin, o.selected)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemBody {
    GraphMesh { vertices: Arc<Vec<f32>>, graph: Arc<crate::shader::Material> },
    /// 启用 Sprite 组件，普通场景 SpriteMesh / Model 腿按一张 2D 图集帧绘制。
    Sprite(SpriteDraw),
    /// sprite_mesh 腿 MeshRenderer(材质无 albedo 贴图):color = entity_tint(选中橙已折入),与 rurix 腿同一个函数;
    /// rurix 光照 = color × (0.28 + 0.72·ndl),L = normalize(0.45, 0.75, 0.35)(viewport/rurix.rs FS_WGSL)。
    Mesh { mesh: MeshData, color: [f32; 4] },
    /// sprite_mesh 腿 MeshRenderer + 带 albedo 贴图的材质:rurix 画成朝 +z 的单位 quad(类 0,F-GAME-2),
    /// 不受光、最近邻、clamp;alpha ≤ 0(或非混合且 < 0.02)丢弃;compositing.x > 0.5 时洋红色键(FS_TEX_WGSL)。
    TexQuad { tex: TexData, compositing: [f32; 4] },
    /// 模型腿 ModelRenderer 的一个 primitive(PBR,modelrender/rurix.rs FS)。
    Model(ModelPrim),
    /// 模型腿里的 MeshRenderer(无 albedo 贴图):缺省 PBR 材质(render_core::model::default_material)。
    LegacyMesh { mesh: MeshData, selected: bool },
    /// 模型腿里 MeshRenderer + albedo 贴图:unlit quad,MASK 0.02、洋红色键、不经 tonemap(legacy_draw)。
    LegacyQuad { tex: TexData, tint: [f32; 4], selected: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderItem {
    pub key: ItemKey,
    /// 场景实体序(绘制序)。
    pub order: u32,
    /// 世界矩阵(行主序、列向量)。sprite_mesh 腿 = trs_model(本地 transform),与 rurix 同式(不走 Parent 链);
    /// 模型腿 = modelrt::entity_world(走 Parent 链)× 节点世界矩阵(非蒙皮)。
    pub world: M4,
    /// 内容指纹(fnv1a64),不含 world / pose;RenderDelta 据此区分"换内容"与"只移动 / 只换姿态"。
    pub content: u64,
    pub body: ItemBody,
    /// 蒙皮姿态:每个关节一个 jointWorld × inverseBind(模型空间,modelrt::node_worlds 在 Animator 时间处求值)。
    pub pose: Option<Arc<Vec<M4>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
pub enum SpriteBlend {
    Opaque,
    Alpha,
    Additive,
}

/// 一张普通场景精灵的解析结果。尺寸和锚点沿用 sprite_render_transform 的源帧数据；
/// 两种 Godot 精灵路径共享这一份，不依赖 RenderingServer 句柄。
#[derive(Clone, PartialEq)]
pub struct SpriteDraw {
    pub tex: TexData,
    pub graph: Option<Arc<crate::shader::Material>>,
    pub uv_rect: [f32; 4],
    pub frame_px: [f32; 2],
    pub pivot: [f32; 2],
    pub tint: [f32; 4],
    pub flip: [bool; 2],
    pub blend: SpriteBlend,
    pub chroma_key: bool,
    pub sorting_order: f64,
    pub selected: bool,
}

impl std::fmt::Debug for SpriteDraw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpriteDraw")
            .field("tex", &self.tex)
            .field("uv_rect", &self.uv_rect)
            .field("frame_px", &self.frame_px)
            .field("pivot", &self.pivot)
            .field("tint", &self.tint)
            .field("flip", &self.flip)
            .field("blend", &self.blend)
            .field("chroma_key", &self.chroma_key)
            .field("sorting_order", &self.sorting_order)
            .field("selected", &self.selected)
            .finish()
    }
}

/// V6 原生图集的一帧像素与场景投放数据。像素仅包含实际采样矩形，尺寸是无损帧尺寸。
#[derive(Debug, Clone, PartialEq)]
pub struct V6SpriteDraw {
    pub key: String,
    pub asset: String,
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub pivot: [f32; 2],
    pub span: f32,
    pub position: [f32; 3],
    pub scale: f32,
    pub rotation: f32,
    pub tint: [f32; 4],
    pub ground: bool,
    pub additive: bool,
    pub entity: u64,
    pub owner: u32,
    pub entity_kind: String,
    pub entity_pos: Option<sentinels_v6::Pos>,
}

/// Light 组件(forge-scene 注册表:kind 无枚举;01 §5.3)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightKind {
    Directional,
    Point,
    Spot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LightItem {
    pub key: ItemKey,
    pub kind: LightKind,
    /// kind 不是 directional / point / spot 时按 directional 处理(stats.unknown_light_kinds 计数)。
    pub kind_known: bool,
    /// 线性颜色。
    pub color: [f32; 3],
    pub intensity: f32,
    pub cast_shadow: bool,
    /// modelrt::entity_world(走 Parent 链);方向光 / 聚光沿自身 −Z 照射。
    pub world: M4,
    /// Stage 5:同一实体上启用的 LightParams(Godot 灯参数);None = 按 Stage 4 的节点缺省。
    pub params: Option<super::env::Props>,
}

/// 抽取统计(I8)。skipped_* 是未接入、本帧没有画出的实体数,如实上报。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExtractStats {
    pub renderables: usize,
    pub triangles: usize,
    pub mesh_fallbacks: usize,
    pub skipped_sprites: usize,
    pub skipped_textured_meshes: usize,
    pub model_prims: usize,
    pub skinned_prims: usize,
    pub lights: usize,
    pub unknown_light_kinds: usize,
    pub particles: usize,
    pub v6_records: usize,
    pub skipped_v6_sprites: usize,
    /// Stage 5:ReflectionProbe / Decal / FogVolume 实例数。
    pub volumes: usize,
}


/// Pipelined 后端(Godot)的帧清单(§2.5)。锁外由 extract 产出,经 SubmitBox 交给 [gmain];不含任何 GPU 句柄。
#[derive(Debug, Clone, PartialEq)]
pub struct RenderList {
    pub seq: u64,
    pub scene_rev: u64,
    pub width: u32,
    pub height: u32,
    /// Scene.mode == "2d"。
    pub mode_2d: bool,
    /// Scene.mode == "2d" 且相机正交、没有 3D 混合内容并可用 Canvas 表达时为 true。
    pub canvas_2d: bool,
    pub leg: Leg,
    /// 分解后的相机:PIE 且有场景相机 → 场景相机,否则编辑器相机(与 rurix 的 vp_override 同判据)。
    pub view: ViewSetup,
    pub clear_rgba: [f32; 4],
    pub selected: Option<u64>,
    /// = SnapshotParams.want_readback(viewport.frame format=none → false:不要 CPU 像素)。
    pub want_pixels: bool,
    /// = SnapshotParams.want_stats(nonzero 由消费线程在拿到像素后统计)。
    pub want_stats: bool,
    /// 按 key 升序(供 RenderDelta 归并)。
    pub items: Vec<RenderItem>,
    /// 启用的 Light 实体(按 key 升序)。为空时 Godot 按 `leg` 生成逼近 rurix 写死灯光的缺省灯(01 §5.3)。
    pub lights: Vec<LightItem>,
    /// V6 腿:records / terrain(其余腿 None)。
    pub v6: Option<Arc<V6Frame>>,
    /// `FORGE_GPU_PARTICLES` 打开时 sprite_mesh 腿的发射器事件(rurix 只在这条腿画粒子;其余情况为空)。
    pub particles: Vec<ParticleItem>,
    /// Stage 5 场景级设置(Environment / CameraAttributes / RenderSettings);全 None = Stage 4 的缺省环境。
    pub env: super::env::SceneEnv,
    /// Stage 5 实体级 ReflectionProbe / Decal / FogVolume(按 key 升序)。
    pub volumes: Vec<super::env::VolumeItem>,
    pub stats: ExtractStats,
    pub asset_generation: u64,
}

/// 一个 ParticleEmitter 事件(gpu_particles.rs 的 emitter 记录;Transform 装的是 center + [年龄, 寿命, 样式])。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleItem {
    /// 槽位 = 启用发射器的场景序(前 64 个);无效的也占槽(与 rurix 相同,影响随机图样)。
    pub slot: u32,
    pub entity: u64,
    pub center: [f32; 3],
    /// 事件年龄(秒)、寿命(秒)、样式(取整后 1..=4)。
    pub age: f32,
    pub lifetime: f32,
    pub kind: f32,
    /// rurix 的随机种子(entity.id % 1_000_000)。
    pub seed: f32,
}

fn fingerprint(mesh: &MeshRef, color: [f32; 4]) -> u64 {
    let mut b = Vec::with_capacity(64);
    match mesh {
        MeshRef::Cube => b.extend_from_slice(b"cube\0"),
        MeshRef::Asset { reference } => {
            b.extend_from_slice(b"asset\0");
            b.extend_from_slice(reference.as_bytes());
            b.push(0);
        }
    }
    for f in color {
        b.extend_from_slice(&f.to_bits().to_le_bytes());
    }
    crate::meshres::fnv1a64(&b)
}

/// 指纹构造器(fnv1a64;字段依次写入,字符串以 0 结尾)。
pub(super) struct Fp(Vec<u8>);

impl Fp {
    pub(super) fn new(tag: &str) -> Fp {
        let mut b = Vec::with_capacity(96);
        b.extend_from_slice(tag.as_bytes());
        b.push(0);
        Fp(b)
    }
    pub(super) fn s(mut self, s: &str) -> Fp {
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
        self
    }
    pub(super) fn f(mut self, v: &[f32]) -> Fp {
        for x in v {
            self.0.extend_from_slice(&x.to_bits().to_le_bytes());
        }
        self
    }
    pub(super) fn u(mut self, v: u64) -> Fp {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub(super) fn done(self) -> u64 {
        crate::meshres::fnv1a64(&self.0)
    }
}

/// MeshRenderer.material → albedo 贴图。与 rurix 两处判据相同:viewport 腿类 0(viewport/rurix.rs 的 entity_sprite)
/// 与模型腿 legacy_draw(material_albedo_guid → load_tex_static_cached,解析失败即"无贴图")。
pub(super) fn mesh_albedo_tex(e: &forge_scene::Entity, project_root: &std::path::Path) -> Option<TexData> {
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
    let albedo = crate::render_core::assets::material_albedo_guid(mat_guid, project_root)?;
    let t = crate::render_core::assets::load_tex_static_cached(project_root, &albedo)?;
    Some(TexData { guid: albedo, w: t.w, h: t.h, rgba: t.rgba })
}

pub(super) fn tex_fp(tag: &str, tex: &TexData, extra: &[f32]) -> u64 {
    Fp::new(tag).s(&tex.guid).u(tex.w as u64).u(tex.h as u64).f(extra).done()
}

pub(super) fn mesh_data(project_root: &std::path::Path, mesh_ref: String) -> MeshData {
    let cube = |fallback| {
        let v = crate::render_core::assets::cube_mesh_bytes();
        MeshData { id: MeshRef::Cube, vertices: v, vertex_count: (v.len() / 24) as u32, fallback }
    };
    if mesh_ref == "cube" {
        return cube(false);
    }
    match crate::meshres::load_mesh_static_cached(project_root, &mesh_ref) {
        Ok(m) => MeshData { id: MeshRef::Asset { reference: mesh_ref }, vertices: &m.bytes, vertex_count: m.vertex_count, fallback: false },
        Err(_) => cube(true), // 失败详情已由 meshres 缓存路径 eprintln 一次(与 rurix 同)
    }
}

/// 将启用的 Sprite 解析为后端中立绘制项。model_leg=true 时保留 Parent 链，与模型腿旧 quad 变换一致。
pub(super) fn sprite_item(
    scene: &Scene,
    e: &forge_scene::Entity,
    order: u32,
    selected: bool,
    root: &std::path::Path,
    model_leg: bool,
) -> Result<Option<RenderItem>, String> {
    let Some(c) = crate::render_core::sprite::sprite_component(e) else {
        return Ok(None);
    };
    let Some(info) = crate::render_core::sprite::resolve_sprite_render(c, root) else {
        return Ok(None);
    };
    let local = crate::render_core::sprite::sprite_render_transform(e).unwrap_or(e.transform);
    let world = if model_leg {
        let base = crate::modelrt::entity_world(scene, e)?;
        crate::render_core::math::m4_mul(
            base,
            crate::render_core::math::m4_mul(
                crate::modelrt::inverse(trs_model(&e.transform))?,
                trs_model(&local),
            ),
        )
    } else {
        trs_model(&local)
    };
    let values = c.props.get("tint").and_then(serde_json::Value::as_array);
    let tint = std::array::from_fn(|i| values
        .and_then(|a| a.get(i))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(1.0) as f32);
    let blend = match crate::render_core::sprite::blend(e) {
        crate::render_core::sprite::SpriteBlend::Opaque => SpriteBlend::Opaque,
        crate::render_core::sprite::SpriteBlend::Alpha => SpriteBlend::Alpha,
        crate::render_core::sprite::SpriteBlend::Additive => SpriteBlend::Additive,
    };
    let sprite_guid = c.props.get("sprite").and_then(serde_json::Value::as_str).unwrap_or("");
    let texture_guid = if let Some(variants) = c.props.get("spriteVariants").and_then(serde_json::Value::as_array).filter(|a| !a.is_empty()) {
        let packed = c.props.get("frame").and_then(serde_json::Value::as_f64).unwrap_or(0.0).max(0.0) as usize;
        let stride = c.props.get("variantStride").and_then(serde_json::Value::as_f64).unwrap_or(1.0).max(1.0) as usize;
        variants.get(packed / stride).and_then(serde_json::Value::as_str).unwrap_or(sprite_guid)
    } else { sprite_guid };
    let text_key = if crate::render_core::sprite::is_text(c) {
        crate::render_core::text::text_key(c, root)
    } else {
        None
    };
    let tex = TexData {
        guid: if let Some(k) = text_key {
            k
        } else if texture_guid.is_empty() {
            c.props.get("texture").and_then(serde_json::Value::as_str).unwrap_or("").to_string()
        } else { texture_guid.to_string() },
        w: info.tex.w,
        h: info.tex.h,
        rgba: info.tex.rgba,
    };
    let body = SpriteDraw {
        tex,
        graph: crate::shader::sprite(e)?,
        uv_rect: info.uv_rect,
        frame_px: info.frame_px,
        pivot: info.pivot,
        tint,
        flip: [
            crate::render_core::sprite::sprite_bool(c, "flipX"),
            crate::render_core::sprite::sprite_bool(c, "flipY"),
        ],
        blend,
        chroma_key: !crate::render_core::sprite::chroma_none(c),
        sorting_order: crate::render_core::sprite::sprite_sorting_order(e),
        selected,
    };
    let content = tex_fp("sprite", &body.tex, &[
        body.uv_rect[0], body.uv_rect[1], body.uv_rect[2], body.uv_rect[3],
        body.frame_px[0], body.frame_px[1], body.pivot[0], body.pivot[1],
        body.tint[0], body.tint[1], body.tint[2], body.tint[3],
        if body.flip[0] { 1.0 } else { 0.0 }, if body.flip[1] { 1.0 } else { 0.0 },
        if body.chroma_key { 1.0 } else { 0.0 }, if body.selected { 1.0 } else { 0.0 },
        body.sorting_order as f32,
    ]);
    Ok(Some(RenderItem {
        key: ItemKey { entity: e.id, sub: 0 },
        order,
        world,
        content: content ^ body.graph.as_ref().map_or(0,|g|crate::meshres::fnv1a64(g.key.as_bytes())),
        body: ItemBody::Sprite(body),
        pose: None,
    }))
}

/// sprite_mesh 腿:MeshRenderer → 真网格(Mesh)或贴图 quad(TexQuad),判据与顺序同 rurix `render_scene_frame`;
/// 世界矩阵 = trs_model(本地 transform)(rurix 这条腿不走 Parent 链)。
fn sprite_mesh_items(scene: &Scene, selected: Option<u64>, root: &std::path::Path, stats: &mut ExtractStats) -> Vec<RenderItem> {
    let mut items = Vec::new();
    for (order, e) in scene.entities.iter().enumerate() {
        if !crate::render_core::sprite::is_renderable(e) {
            continue;
        }
        if crate::render_core::sprite::sprite_component(e).is_some() {
            if let Some(item) = sprite_item(scene, e, order as u32, selected == Some(e.id), root, false).ok().flatten() {
                stats.triangles += 2;
                items.push(item);
                continue;
            }
            if e.component("MeshRenderer").is_none_or(|c| !c.enabled) {
                stats.skipped_sprites += 1;
                continue;
            }
        }
        let key = ItemKey { entity: e.id, sub: 0 };
        let world = trs_model(&e.transform);
        if crate::render_core::sprite::sprite_component(e).is_none() {
            if let Some(tex) = mesh_albedo_tex(e, root) {
                let compositing = crate::render_core::sprite::sprite_compositing(e);
                stats.triangles += 2;
                let content = tex_fp("texquad", &tex, &compositing);
                items.push(RenderItem { key, order: order as u32, world, content, body: ItemBody::TexQuad { tex, compositing }, pose: None });
                continue;
            }
        }
        let mesh = mesh_data(root, crate::render_core::sprite::entity_mesh_ref(e));
        stats.mesh_fallbacks += usize::from(mesh.fallback);
        stats.triangles += mesh.vertex_count as usize / 3;
        let color = crate::render_core::assets::entity_tint(e, selected == Some(e.id));
        let content = fingerprint(&mesh.id, color);
        items.push(RenderItem { key, order: order as u32, world, content, body: ItemBody::Mesh { mesh, color }, pose: None });
    }
    items
}

/// 按 key 升序;重复 id(或同一实体的多个 primitive)用 sub 区分(稳定排序保持场景序 / 遍历序)。
pub(super) fn assign_sub_keys(items: &mut [RenderItem]) {
    items.sort_by_key(|i| (i.key.entity, i.order));
    for i in 1..items.len() {
        if items[i].key.entity == items[i - 1].key.entity {
            items[i].key.sub = items[i - 1].key.sub + 1;
        }
    }
}

fn canvas_2d_compatible(scene: &Scene, view: &ViewSetup, root: &std::path::Path) -> bool {
    if scene.mode != forge_scene::SCENE_MODE_2D
        || !matches!(view.projection, Projection::Orthographic { .. })
        || (view.eye[0] - view.center[0]).abs() > 1e-4
        || (view.eye[1] - view.center[1]).abs() > 1e-4
    {
        return false;
    }
    if scene.entities.iter().any(|e| ["MeshRenderer", "ModelRenderer", "ParticleEmitter"]
        .iter().any(|kind| e.component(kind).is_some_and(|c| c.enabled)))
    {
        return false;
    }
    let mut plane_z = None;
    for e in scene.entities.iter().filter(|e| crate::render_core::sprite::sprite_component(e).is_some()) {
        if crate::render_core::sprite::resolve_sprite_render(crate::render_core::sprite::sprite_component(e).unwrap(), root).is_none() {
            return false;
        }
        let r = crate::render_core::math::quat_to_mat3(e.transform.rotation);
        if [r[0][2], r[1][2], r[2][0], r[2][1]].iter().any(|v| v.abs() > 1e-5) || (r[2][2] - 1.0).abs() > 1e-5 {
            return false;
        }
        if plane_z.is_some_and(|z: f32| z.to_bits() != e.transform.translation[2].to_bits()) {
            return false;
        }
        plane_z = Some(e.transform.translation[2]);
    }
    true
}

/// 锁外把快照抽成 RenderList(§2.5)。只读快照与资产缓存,不取 HS。
/// 相机:PIE 且有场景相机 → 场景相机,否则编辑器相机(与 rurix 的 vp_override 同判据)。
pub fn extract(s: &RenderSnapshot) -> Result<RenderList, String> {
    let scene: &Scene = &s.scene;
    let leg = classify(scene);
    let (w, h) = (s.params.width, s.params.height);
    let aspect = w as f32 / h.max(1) as f32;
    let view = s
        .vp_override
        .and_then(|_| scene_view(scene, aspect))
        .unwrap_or_else(|| editor_view(&s.camera, aspect));
    let root = &*s.project_root;
    let mut stats = ExtractStats::default();
    let mut view = view;
    let mut v6 = None;
    let (mut items, clear_rgba) = match leg {
        Leg::SpriteMesh => (sprite_mesh_items(scene, s.params.selected, root, &mut stats), SPRITE_MESH_CLEAR_RGBA),
        Leg::Model => (super::extract3d::model_items(scene, s.params.selected, root, &mut stats)?, MODEL_CLEAR_RGBA),
        Leg::SentinelsV6 => {
            let f = crate::sentinels_v6_render::staged_frame()?;
            stats.v6_records = f.terrain.len() + f.objects.len();
            stats.skipped_v6_sprites = f.skipped_sprites;
            stats.triangles = stats.v6_records * 6 + f.sprites.len() * 2;
            stats.mesh_fallbacks = f.fallbacks;
            // rurix 的 V6 腿不看编辑器 / 场景相机,相机完全来自 Batch.view;与 publish() 设给编辑器相机的参数等价
            // (target = iso 中心、yaw = pitch = 0、dist 100、正交、半高 24/zoom,sentinels_v6.rs:426-433)。
            let cam = EditorCamera {
                target: [f.view[0], f.view[1], 0.0],
                yaw_deg: 0.0,
                pitch_deg: 0.0,
                dist: 100.0,
                fov_y_deg: 50.0,
                ortho: true,
                ortho_half_h: f.view[2],
            };
            view = editor_view(&cam, aspect);
            v6 = Some(Arc::new(f));
            (Vec::new(), V6_CLEAR_RGBA)
        }
    };
    assign_sub_keys(&mut items);
    stats.renderables = if let Some(frame) = &v6 { stats.v6_records + frame.sprites.len() } else { items.len() };
    // V6 owns a separate isometric batch; an empty ordinary item list is not a Canvas scene.
    let canvas_2d = leg == Leg::SpriteMesh && canvas_2d_compatible(scene, &view, root);
    let lights = super::extract3d::lights(scene, &mut stats);
    // rurix 只在 sprite_mesh 腿画粒子(viewport/rurix.rs),模型腿与 V6 腿都不画。
    let particles = if leg == Leg::SpriteMesh { super::particles::items(scene) } else { Vec::new() };
    stats.particles = particles.len();
    let env = super::env::scene_env(scene, root);
    let volumes = super::env::volumes(scene, root, &mut stats);
    Ok(RenderList {
        seq: s.seq,
        scene_rev: s.scene_rev,
        width: w,
        height: h,
        mode_2d: scene.is_2d(),
        canvas_2d,
        leg,
        view,
        clear_rgba,
        selected: s.params.selected,
        want_pixels: s.params.want_readback,
        want_stats: s.params.want_stats,
        items,
        lights,
        v6,
        particles,
        env,
        volumes,
        stats,
        asset_generation: s.asset_generation,
    })
}

/// 便于跨线程投递。
pub fn extract_arc(s: &RenderSnapshot) -> Result<Arc<RenderList>, String> {
    extract(s).map(Arc::new)
}


#[cfg(test)]
mod tests {
    use super::*;
    use forge_scene::{Component, Entity, Transform};

    fn scene(name: &str, components: &[(&str, bool)]) -> Scene {
        let mut s = Scene::new(name);
        for (i, (ctype, enabled)) in components.iter().enumerate() {
            let mut c = Component::new(*ctype, serde_json::json!({}));
            c.enabled = *enabled;
            s.entities.push(Entity { entity_guid: None, id: i as u64 + 1, name: format!("e{i}"), transform: Transform::default(), components: vec![c] });
        }
        s
    }

    #[test]
    fn classify_matches_render_scene_frame_dispatch_order() {
        let v6 = "Code Sentinels V6";
        let cases = [
            (scene("empty", &[]), Leg::SpriteMesh),
            (scene("sprites", &[("Sprite", true), ("MeshRenderer", true)]), Leg::SpriteMesh),
            (scene("model", &[("Sprite", true), ("ModelRenderer", true)]), Leg::Model),
            (scene("model-disabled", &[("ModelRenderer", false)]), Leg::SpriteMesh),
            (scene(v6, &[("SentinelsV6Batch", true)]), Leg::SentinelsV6),
            // V6 判据不看 enabled;V6 先于模型腿。
            (scene(v6, &[("SentinelsV6Batch", false), ("ModelRenderer", true)]), Leg::SentinelsV6),
            // 名字不对 / 没有 batch:不是 V6 腿。
            (scene("Code Sentinels V5", &[("SentinelsV6Batch", true)]), Leg::SpriteMesh),
            (scene(v6, &[("ModelRenderer", true)]), Leg::Model),
            (scene(v6, &[]), Leg::SpriteMesh),
        ];
        for (s, leg) in cases {
            assert_eq!(classify(&s), leg, "scene {:?} / {:?}", s.name, s.entities.iter().map(|e| &e.components[0].ctype).collect::<Vec<_>>());
        }
    }

    fn snap_of(scene: Scene, selected: Option<u64>) -> RenderSnapshot {
        let mut st = crate::rpc::HostState::new();
        st.scene = scene;
        crate::render::snapshot(&st, crate::render::SnapshotParams { scene_camera: false,
            width: 320, height: 180, selected, want_readback: true, want_stats: true,
            requester: crate::render::FrameRequester::ViewportFrame,
        }, 9)
    }

    fn mesh_entity(id: u64, props: serde_json::Value, t: [f32; 3]) -> Entity {
        Entity { entity_guid: None, id, name: format!("m{id}"), transform: Transform { translation: t, ..Transform::default() },
                 components: vec![Component::new("MeshRenderer", props)] }
    }

    #[test]
    fn extract_meshes_with_rurix_color_and_transform_rules() {
        let mut s = Scene::new("x");
        s.entities.push(mesh_entity(5, serde_json::json!({ "mesh": "cube" }), [1.0, 2.0, 3.0]));
        s.entities.push(mesh_entity(2, serde_json::json!({}), [0.0; 3]));
        s.entities.push(Entity { entity_guid: None, id: 3, name: "sp".into(), transform: Transform::default(),
                                 components: vec![Component::new("Sprite", serde_json::json!({}))] });
        s.entities.push(mesh_entity(4, serde_json::json!({ "mesh": "__missing__.rxmesh" }), [0.0; 3]));
        let snap = snap_of(s, Some(2));
        let l = extract(&snap).unwrap();
        assert_eq!((l.seq, l.width, l.height, l.leg, l.clear_rgba), (9, 320, 180, Leg::SpriteMesh, SPRITE_MESH_CLEAR_RGBA));
        assert_eq!(l.items.iter().map(|i| i.key.entity).collect::<Vec<_>>(), [2, 4, 5], "按 key 升序");
        assert_eq!((l.stats.renderables, l.stats.skipped_sprites, l.stats.mesh_fallbacks, l.stats.triangles), (3, 1, 1, 36));
        let e5 = &snap.scene.entities[0];
        let it5 = l.items.iter().find(|i| i.key.entity == 5).unwrap();
        assert_eq!(it5.world, trs_model(&e5.transform));
        let ItemBody::Mesh { mesh, color } = &it5.body else { panic!("{:?}", it5.body) };
        assert_eq!((mesh.id.clone(), mesh.vertex_count, mesh.fallback), (MeshRef::Cube, 36, false));
        assert_eq!(*color, crate::render_core::assets::entity_tint(e5, false));
        let ItemBody::Mesh { color: sel, .. } = &l.items[0].body else { panic!() };
        assert_eq!(*sel, [1.0, 0.62, 0.18, 1.0], "选中橙");
        assert_eq!(l.view, editor_view(&snap.camera, 320.0 / 180.0), "编辑态走编辑器相机");
        assert_eq!(clear_rgb8(l.clear_rgba), [23, 24, 29]);
        assert!(l.lights.is_empty() && l.items.iter().all(|i| i.pose.is_none()));
    }

    #[test]
    fn model_leg_errors_match_rurix_collect_and_v6_is_refused() {
        // 与 render_core::model::collect 同一错误文本(rurix 模型腿取帧同样失败)。
        let err = extract(&snap_of(scene("m", &[("ModelRenderer", true)]), None)).unwrap_err();
        assert_eq!(err, "ModelRenderer.model missing");
    }

    /// V6 腿:经 sentinels_v6_render::staged_frame 取 records / terrain,相机由 Batch.view 构造(与 publish() 同参数)。
    #[test]
    fn v6_leg_extracts_records_terrain_and_iso_camera() {
        let snap = ::sentinels_v6::Game::new(1, false).state;
        let view = crate::sentinels_v6::View::default();
        let v6_scene = crate::sentinels_v6_render::scene(&snap, &view);
        let a = extract(&snap_of(v6_scene, None)).unwrap();
        assert_eq!((a.leg, a.clear_rgba, a.items.len()), (Leg::SentinelsV6, V6_CLEAR_RGBA, 0));
        assert!(!a.canvas_2d, "V6 batches must not be cleared by the ordinary Canvas path");
        // STAGED / TERRAIN_BATCH 是进程级全局状态,别的 V6 测试会并发改写,所以这里只断言单次抽取的自洽性;
        // "同一局面两次取帧相同"由 godot-host 的 g4_v6 在独立进程里验证。
        let fa = a.v6.as_ref().unwrap();
        assert!(fa.objects.len() + fa.terrain.len() > 100, "初始局面应有足够多的盒子");
        assert_eq!(a.stats.v6_records, fa.objects.len() + fa.terrain.len());
        assert_eq!(a.stats.triangles, a.stats.v6_records * 6);
        assert_eq!(a.view.projection, crate::render_core::camera::Projection::Orthographic { half_h: fa.view[2] });
        assert_eq!((a.view.center[0], a.view.center[1]), (fa.view[0], fa.view[1]));
    }

    #[test]
    fn duplicate_entity_ids_get_distinct_sub_keys_and_content_ignores_world() {
        let mut s = Scene::new("d");
        s.entities.push(mesh_entity(7, serde_json::json!({}), [0.0; 3]));
        s.entities.push(mesh_entity(7, serde_json::json!({}), [5.0, 0.0, 0.0]));
        let l = extract(&snap_of(s, None)).unwrap();
        assert_eq!(l.items.iter().map(|i| (i.key.entity, i.key.sub)).collect::<Vec<_>>(), [(7, 0), (7, 1)]);
        assert_eq!(l.items[0].content, l.items[1].content);
        assert_ne!(l.items[0].world, l.items[1].world);
    }
}
