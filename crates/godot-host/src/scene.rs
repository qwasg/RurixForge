//! 离屏视口与清单应用(02 §2.5 RenderDelta、01 §2 C1、§5)。只在 [gmain](Godot 主线程)调用。
//! Stage 4:完整 3D 集合——MeshRenderer(真网格 / 贴图 quad)、模型(网格 + PBR 材质 + GPU 蒙皮)、腿内旧 MeshRenderer、
//! Light(或逼近 rurix 写死灯光的缺省灯)、相机、按 leg 的环境(tonemap / 环境光 / 背景)。

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use godot::classes::rendering_server::{CanvasItemTextureFilter, EnvironmentBg, InstanceFlags, ViewportUpdateMode};
use godot::classes::RenderingServer;
use godot::prelude::*;

use engine_host::{ItemBody, ItemKey, MeshData, MeshRef, ModelPrim, Projection, RenderDelta, RenderItem, RenderList, ViewSetup, M4};

use crate::env::{EnvState, Targets};
use crate::light::{self, LightInst};
use crate::material::Materials;
use crate::mesh;
use crate::particles::Particles;
use crate::rid::Owned;
use crate::sprite::{CanvasSprites, SpriteAssets};
use crate::v6::V6Draw;
use crate::volumes::Volumes;

pub fn v3(a: [f32; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

/// M4(行主序、列向量)→ Transform3D:基的列 = M 的前三列,原点 = 第四列。
pub fn xform(m: &M4) -> Transform3D {
    let col = |c: usize| Vector3::new(m[0][c], m[1][c], m[2][c]);
    Transform3D::new(Basis::from_cols(col(0), col(1), col(2)), col(3))
}

/// 相机世界变换:与 rurix look_at_rh 同一组 (s, u, f),Godot 相机看 -Z,所以 Z 列 = -f。
/// 丢掉滚转的行为随 view_basis 一起复现(01 C1)。
pub fn camera_xform(v: &ViewSetup) -> Transform3D {
    let (s, u, f) = engine_host::view_basis(v);
    Transform3D::new(Basis::from_cols(v3(s), v3(u), v3([-f[0], -f[1], -f[2]])), v3(v.eye))
}

/// 两个视口共用的 RS 资源:按 (网格身份, 资产代次) 缓存的平直网格、单位 quad、按 (模型, primitive, 蒙皮) 缓存的模型网格、材质。
/// 实例只持有这些资源的 RID;缓存只在 asset.reload 时清空,那时两个视口的实例已经先释放(host.rs control)。
pub struct Assets {
    pub graphs: crate::shader_graph::GraphMaterials,
    pub mats: Materials,
    pub sprites: SpriteAssets,
    /// 实际渲染方式是 Compatibility(GLES3):灯的阴影开关与着色器变体都据此选。
    pub compatibility: bool,
    flat: HashMap<(MeshRef, u64, bool), Owned>,
    quad: Option<Owned>,
    models: HashMap<(String, usize, bool), Result<Owned, String>>,
}

impl Assets {
    pub fn new(rs: &mut Gd<RenderingServer>, compatibility: bool) -> Self {
        Assets { graphs:crate::shader_graph::GraphMaterials::new(), mats: Materials::new(rs, compatibility), sprites: SpriteAssets::new(rs), compatibility, flat: HashMap::new(), quad: None, models: HashMap::new() }
    }

    fn flat(&mut self, rs: &mut Gd<RenderingServer>, m: &MeshData, generation: u64, two_sided: bool) -> Rid {
        self.flat.entry((m.id.clone(), generation, two_sided)).or_insert_with(|| mesh::flat_mesh(rs, m, two_sided)).rid()
    }

    fn quad(&mut self, rs: &mut Gd<RenderingServer>) -> Rid {
        self.quad.get_or_insert_with(|| mesh::quad_mesh(rs)).rid()
    }

    fn model_mesh(&mut self, rs: &mut Gd<RenderingServer>, p: &ModelPrim) -> Option<Rid> {
        let key = (p.model.key.clone(), p.prim, p.skin.is_some());
        let r = self.models.entry(key).or_insert_with(|| {
            let r = p
                .model
                .bundle
                .primitives
                .get(p.prim)
                .ok_or_else(|| "primitive out of range".to_string())
                .and_then(|prim| mesh::model_mesh(rs, prim, p.skin.is_some()));
            if let Err(e) = &r {
                eprintln!("godot-host: 模型 {} primitive {} 建网格失败:{e}", p.model.key, p.prim);
            }
            r
        });
        r.as_ref().ok().map(Owned::rid)
    }

    /// asset.reload:丢弃网格与材质缓存(实例在下一份清单的全量重建里换成新资源)。
    pub fn invalidate(&mut self) {
        self.graphs.clear();
        self.models.clear();
        self.flat.clear();
        self.mats.clear();
        self.sprites.clear();
    }

    pub fn free(mut self) {
        self.invalidate();
    }
}


/// 一个实体 draw:RS instance(先释放)+ 蒙皮时它自己的 skeleton。
struct Inst {
    rid: Owned,
    _graph_mesh: Option<Owned>,
    // Keep the actual GPU instance's graph alive, including when a rejected
    // replacement has already become the latest extracted RenderList.
    _graph_material: Option<Arc<engine_host::shader::Material>>,
    skeleton: Option<Owned>,
}

/// Mobile 的输出视口(有意修正 2,02 §9.5 Stage 5 step 4):内层 3D 视口开 use_hdr_2d(3D 缓冲 RGBA16F,不再经 RGB10A2
/// 量化;tonemap 输出线性值),外层 8 bit 视口用 canvas 着色器按 Godot tonemap 同一个公式做 linear→sRGB。
/// 导出 / L1 / 粒子叠加层都用外层,格式与 F+ 相同(RGBA8,sRGB 编码值)。
pub struct HdrBlit {
    item: Owned,
    _canvas: Owned,
    _material: Owned,
    _shader: Owned,
    src: Rid,
}

const BLIT_SHADER: &str = r#"shader_type canvas_item;
render_mode blend_disabled, unshaded;
vec3 forge_linear_to_srgb(vec3 color) {
	const vec3 a = vec3(0.055f);
	return mix((vec3(1.0f) + a) * pow(color.rgb, vec3(1.0f / 2.4f)) - a, 12.92f * color.rgb, lessThan(color.rgb, vec3(0.0031308f)));
}
void fragment() {
	COLOR = vec4(forge_linear_to_srgb(texture(TEXTURE, UV).rgb), 1.0);
}
"#;

impl HdrBlit {
    fn new(rs: &mut Gd<RenderingServer>, out: Rid, src_viewport: Rid) -> HdrBlit {
        let shader = Owned::new(rs.shader_create());
        rs.shader_set_code(shader.rid(), BLIT_SHADER);
        let material = Owned::new(rs.material_create());
        rs.material_set_shader(material.rid(), shader.rid());
        let canvas = Owned::new(rs.canvas_create());
        let item = Owned::new(rs.canvas_item_create());
        rs.canvas_item_set_parent(item.rid(), canvas.rid());
        rs.canvas_item_set_material(item.rid(), material.rid());
        rs.canvas_item_set_default_texture_filter(item.rid(), CanvasItemTextureFilter::NEAREST);
        rs.viewport_attach_canvas(out, canvas.rid());
        rs.viewport_set_canvas_stacking(out, canvas.rid(), 0, 0);
        HdrBlit { item, _canvas: canvas, _material: material, _shader: shader, src: rs.viewport_get_texture(src_viewport) }
    }

    fn resize(&self, rs: &mut Gd<RenderingServer>, w: u32, h: u32) {
        rs.canvas_item_clear(self.item.rid());
        rs.canvas_item_add_texture_rect(self.item.rid(), Rect2::new(Vector2::ZERO, Vector2::new(w as f32, h as f32)), self.src);
    }
}

/// 一个离屏视口(Main / Preview 各一个,02 §4.3):自己的 scenario、相机、环境、灯与实例表。
pub struct Slot {
    instances: BTreeMap<ItemKey, Inst>,
    lights: Vec<LightInst>,
    v6: Option<V6Draw>,
    canvas: Option<CanvasSprites>,
    particles: Option<Particles>,
    volumes: Volumes,
    env: EnvState,
    blit: Option<HdrBlit>,
    /// 导出用的视口(Mobile 是外层 8 bit 视口,其余就是 3D 视口)。
    pub viewport: Rid,
    /// 画 3D 的视口。
    vp3d: Rid,
    scenario: Rid,
    camera: Rid,
    environment: Rid,
    pub size: (u32, u32),
    applied: Option<Arc<RenderList>>,
    /// 当前实例用的是"缺省环境"下的模型材质(金属环境项补丁,见 material.rs);Environment 组件出现 / 消失时整体重建实例。
    rurix_ambient: bool,
    /// 实例带 USE_BAKED_LIGHT(SDFGI 参与);Environment.sdfgiEnabled 变化时整体重建实例。
    static_gi: bool,
    /// Main 通道(RenderSettings 的 RS 全局设置只跟它)。
    main: bool,
}

fn offscreen_viewport(rs: &mut Gd<RenderingServer>) -> Rid {
    let viewport = rs.viewport_create();
    rs.viewport_set_size(viewport, 64, 64);
    rs.viewport_set_transparent_background(viewport, false);
    // 离屏视口不在屏幕树里,WHEN_VISIBLE 永不刷新(01 §2.2);每份清单用 ONCE 画一次。
    rs.viewport_set_update_mode(viewport, ViewportUpdateMode::DISABLED);
    rs.viewport_set_active(viewport, true);
    viewport
}

impl Slot {
    pub fn new(rs: &mut Gd<RenderingServer>, mobile: bool, main: bool) -> Self {
        let scenario = rs.scenario_create();
        let environment = rs.environment_create();
        rs.environment_set_background(environment, EnvironmentBg::COLOR);
        rs.scenario_set_environment(scenario, environment);
        let camera = rs.camera_create();
        let vp3d = rs.viewport_create();
        rs.viewport_set_size(vp3d, 64, 64);
        rs.viewport_set_scenario(vp3d, scenario);
        rs.viewport_attach_camera(vp3d, camera);
        rs.viewport_set_transparent_background(vp3d, false);
        rs.viewport_set_update_mode(vp3d, ViewportUpdateMode::DISABLED);
        rs.viewport_set_active(vp3d, true);
        let (viewport, blit) = if mobile {
            rs.viewport_set_use_hdr_2d(vp3d, true);
            let out = offscreen_viewport(rs);
            // 子视口先画(RendererViewport::_sort_active_viewports:children first)。
            rs.viewport_set_parent_viewport(vp3d, out);
            let b = HdrBlit::new(rs, out, vp3d);
            b.resize(rs, 64, 64);
            (out, Some(b))
        } else {
            (vp3d, None)
        };
        Slot {
            instances: BTreeMap::new(),
            lights: Vec::new(),
            v6: None,
            canvas: None,
            particles: None,
            volumes: Volumes::default(),
            env: EnvState::default(),
            blit,
            viewport,
            vp3d,
            scenario,
            camera,
            environment,
            size: (64, 64),
            applied: None,
            rurix_ambient: true,
            static_gi: false,
            main,
        }
    }

    /// 应用一份清单(RenderDelta),并让视口在本帧 draw 里重画一次。返回(本帧实际绘制的实例数, 是否新建了内容)。
    pub fn apply(&mut self, rs: &mut Gd<RenderingServer>, assets: &mut Assets, list: Arc<RenderList>) -> (usize, bool) {
        let d = RenderDelta::diff(self.applied.as_deref(), &list);
        // env / volumes 只在全量帧或 Stage 5 组件变化时下发,没有 Stage 5 组件的场景 fresh 与 Stage 4 相同。
        let mut fresh = d.full || !d.added.is_empty() || !d.content.is_empty() || d.v6.is_some() || d.lights.is_some()
            || d.env.is_some() || d.volumes.is_some();
        if d.full {
            self.instances.clear();
            light::configure_environment(rs, self.environment, list.leg);
        }
        if let Some((w, h)) = d.resize {
            rs.viewport_set_size(self.vp3d, w as i32, h as i32);
            if let Some(b) = &self.blit {
                rs.viewport_set_size(self.viewport, w as i32, h as i32);
                b.resize(rs, w, h);
            }
            self.size = (w, h);
        }
        if let Some(v) = &d.view {
            rs.camera_set_transform(self.camera, camera_xform(v));
            match v.projection {
                Projection::Perspective { fov_y_deg } => rs.camera_set_perspective(self.camera, fov_y_deg, v.near, v.far),
                Projection::Orthographic { half_h } => rs.camera_set_orthogonal(self.camera, 2.0 * half_h, v.near, v.far),
            }
        }
        if let Some(c) = d.clear_rgba {
            rs.environment_set_bg_color(self.environment, light::bg_color(list.leg, c));
        }
        if let Some(ls) = &d.lights {
            self.lights.clear();
            self.lights = light::build(rs, self.scenario, list.leg, ls, assets.compatibility);
        }
        // Stage 5 场景级设置(background = clearColor 跟着清屏色走;清屏色只在 leg 变时变,那时本来就是全量帧)。
        if d.env.is_some() || (d.clear_rgba.is_some() && self.env.custom()) {
            let t = Targets { scenario: self.scenario, vp3d: self.vp3d, raw_env: self.environment };
            self.env.apply(rs, t, &list, self.main);
        }
        let ambient = list.env.environment.is_none();
        // SDFGI 只收 GI_MODE_STATIC 的几何(INSTANCE_FLAG_USE_BAKED_LIGHT);只在 Environment 开了 SDFGI 时给实例打这个标志,
        // 其余时候实例与 Stage 4 完全相同。
        let static_gi = list.env.environment.as_ref().is_some_and(|p| p.flag("sdfgiEnabled"));
        let rebuild_all = (ambient != self.rurix_ambient || static_gi != self.static_gi) && !d.full;
        self.rurix_ambient = ambient;
        self.static_gi = static_gi;
        let scenario = self.scenario;
        if d.volumes.is_some() {
            self.volumes.update(rs, scenario, &list);
        }
        if d.full && list.v6.is_none() {
            self.v6 = None;
        }
        if let Some(f) = &d.v6 {
            self.v6.get_or_insert_with(|| V6Draw::new(rs, scenario, assets.compatibility)).update(rs, scenario, f, &mut assets.sprites);
        }
        if list.canvas_2d {
            let created = self.canvas.is_none();
            self.instances.clear();
            self.lights.clear();
            self.v6 = None;
            self.particles = None;
            self.canvas.get_or_insert_with(|| CanvasSprites::new(rs, self.vp3d))
                .update(rs, &mut assets.sprites, &list);
            self.redraw(rs);
            let draws = self.canvas.as_ref().map_or(0, CanvasSprites::instances);
            self.applied = Some(list);
            return (draws, fresh || created);
        }
        self.canvas = None;
        if let Some(ps) = &d.particles {
            if ps.is_empty() {
                self.particles = None;
            } else {
                let out = self.viewport;
                if self.particles.is_none() {
                    self.particles = Some(Particles::new(rs, out));
                    fresh = true;
                }
                if let Some(p) = self.particles.as_mut() {
                    p.update(rs, ps);
                }
            }
        }
        if let Some(p) = self.particles.as_mut() {
            p.set_view(rs, &list.view, list.width, list.height);
        }
        for k in &d.removed {
            self.instances.remove(k);
        }
        if rebuild_all {
            // Environment 组件出现 / 消失:模型材质要在"缺省环境补丁版"与原版之间切换,整表重建。
            self.instances.clear();
            fresh = true;
            for it in &list.items {
                if let Some(i) = self.instance(rs, assets, it, list.asset_generation, list.leg == engine_host::Leg::Model && ambient) {
                    self.instances.insert(it.key, i);
                }
            }
        } else {
            for it in d.added.iter().chain(d.content.iter()) {
                if let Some(i) = self.instance(rs, assets, it, list.asset_generation, list.leg == engine_host::Leg::Model && ambient) {
                    self.instances.insert(it.key, i);
                }
            }
            for (k, m) in &d.moved {
                if let Some(i) = self.instances.get(k) {
                    rs.instance_set_transform(i.rid.rid(), xform(m));
                }
            }
            for (k, pose) in &d.posed {
                if let Some(sk) = self.instances.get(k).and_then(|i| i.skeleton.as_ref()) {
                    for (b, m) in pose.iter().enumerate() {
                        rs.skeleton_bone_set_transform(sk.rid(), b as i32, xform(m));
                    }
                }
            }
        }
        self.redraw(rs);
        self.applied = Some(list);
        // draws 与 rurix 同口径:只数几何 draw(粒子叠加层、体积类实例 rurix 都不计入 draws)。
        let draws = self.instances.len() + self.v6.as_ref().map_or(0, V6Draw::instances);
        (draws, fresh)
    }

    /// 让视口在下一次 RS::draw 里再画一次(每份清单一次;Mobile 的预热帧再一次)。Mobile 连同外层输出视口。
    pub fn redraw(&self, rs: &mut Gd<RenderingServer>) {
        rs.viewport_set_update_mode(self.vp3d, ViewportUpdateMode::ONCE);
        if self.blit.is_some() {
            rs.viewport_set_update_mode(self.viewport, ViewportUpdateMode::ONCE);
        }
    }

    fn instance(&self, rs: &mut Gd<RenderingServer>, assets: &mut Assets, it: &RenderItem, generation: u64, compensate_reinhard: bool) -> Option<Inst> {
        let mut graph_mesh=None;
        let graph_material = match &it.body { ItemBody::GraphMesh{graph,..}=>Some(graph.clone()), ItemBody::Sprite(draw)=>draw.graph.clone(), ItemBody::Model(p)=>p.graph.clone(), _=>None };
        let sprite_sort = match &it.body { ItemBody::Sprite(draw) => Some(draw.sorting_order as f32), _ => None };
        let (base, mat, overlay) = match &it.body {
            ItemBody::GraphMesh{vertices,graph}=>{let mesh=crate::shader_graph::mesh(rs,vertices);let rid=mesh.rid();graph_mesh=Some(mesh);let mat=assets.graphs.material(rs,graph,&engine_host::shader::Texture::white(),false,[1.;4],[0.,0.,1.,1.],[false;2])?;(rid,mat,false)},
            ItemBody::Sprite(draw) => (assets.sprites.spatial_quad(rs), assets.sprites.spatial_material(rs, draw, generation, compensate_reinhard)?, false),
            ItemBody::Mesh { mesh, color } => (assets.flat(rs, mesh, generation, true), assets.mats.color(*color), false),
            ItemBody::TexQuad { tex, compositing } => (assets.quad(rs), assets.mats.quad(rs, tex, false, *compositing)?, false),
            ItemBody::Model(p) => {
                let base = assets.model_mesh(rs, p)?;
                let mat = if let Some(graph)=&p.graph {let source=p.material.base_color_texture.and_then(|i|p.model.bundle.textures.get(i)).map(|t|engine_host::shader::Texture{width:t.width,height:t.height,rgba:Arc::new(t.rgba.clone())}).unwrap_or_else(engine_host::shader::Texture::white);assets.graphs.material(rs,graph,&source,false,[1.;4],[0.,0.,1.,1.],[false;2])?}else{assets.mats.model(&p.model.key, &p.model.bundle, &p.material, p.material_fp, self.rurix_ambient)};
                (base, mat, p.selected)
            }
            ItemBody::LegacyMesh { mesh, selected } => (assets.flat(rs, mesh, generation, false), assets.mats.legacy_mesh(), *selected),
            ItemBody::LegacyQuad { tex, tint, .. } => (assets.quad(rs), assets.mats.quad(rs, tex, true, *tint)?, false),
        };
        let inst = Owned::new(rs.instance_create2(base, self.scenario));
        rs.instance_geometry_set_material_override(inst.rid(), mat);
        if self.static_gi {
            rs.instance_geometry_set_flag(inst.rid(), InstanceFlags::USE_BAKED_LIGHT, true);
        }
        if overlay {
            rs.instance_geometry_set_material_overlay(inst.rid(), assets.mats.overlay());
        }
        rs.instance_set_transform(inst.rid(), xform(&it.world));
        if let Some(offset) = sprite_sort { rs.instance_set_pivot_data(inst.rid(), offset, false); }
        let skeleton = it.pose.as_ref().map(|pal| {
            let sk = Owned::new(rs.skeleton_create());
            rs.skeleton_allocate_data(sk.rid(), pal.len() as i32);
            for (b, m) in pal.iter().enumerate() {
                rs.skeleton_bone_set_transform(sk.rid(), b as i32, xform(m));
            }
            rs.instance_attach_skeleton(inst.rid(), sk.rid());
            // 姿态在 CPU 算、每帧可能大幅移动:给蒙皮实例一个足够大的 AABB,避免按静止 AABB 被视锥剔除。
            rs.instance_set_custom_aabb(inst.rid(), Aabb::new(Vector3::splat(-1.0e4), Vector3::splat(2.0e4)));
            sk
        });
        Some(Inst { rid: inst, skeleton, _graph_mesh:graph_mesh, _graph_material:graph_material })
    }

    /// 丢弃已应用状态(asset.reload):下一份清单全量重建(env / volumes 随全量帧重配)。
    pub fn reset(&mut self) {
        self.instances.clear();
        self.canvas = None;
        self.lights.clear();
        self.v6 = None;
        self.particles = None;
        self.volumes = Volumes::default();
        self.applied = None;
    }

    pub fn free(mut self, rs: &mut Gd<RenderingServer>) {
        self.reset();
        self.env = EnvState::default();
        self.blit = None;
        let outer = (self.viewport != self.vp3d).then_some(self.viewport);
        for r in [self.vp3d, self.camera, self.environment, self.scenario].into_iter().chain(outer) {
            rs.free_rid(r);
        }
    }
}
