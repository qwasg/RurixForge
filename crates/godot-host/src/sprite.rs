//! 普通 Sprite 的 Godot 4.7.2 映射：CanvasItem（纯 2D）与不受光 3D quad（混合/透视）。
//! Canvas shader 只用 4.7 CanvasItem API 明确列出的 COLOR/UV/TEXTURE，不依赖 MODULATE。
use std::collections::{BTreeMap, BTreeSet, HashMap};

use godot::classes::rendering_server::{CanvasItemTextureFilter, PrimitiveType};
use godot::classes::{ImageTexture, RenderingServer};
use godot::prelude::*;
use engine_host::{ItemBody, ItemKey, Projection, RenderItem, RenderList, SpriteBlend, SpriteDraw, TexData, M4};

use crate::material::image_texture;
use crate::rid::Owned;

fn blend_slot(blend: SpriteBlend) -> usize {
    match blend { SpriteBlend::Opaque => 0, SpriteBlend::Alpha => 1, SpriteBlend::Additive => 2 }
}

fn canvas_shader(blend: SpriteBlend, chroma: bool) -> String {
    let render_mode = match blend {
        SpriteBlend::Opaque => "blend_disabled",
        SpriteBlend::Alpha => "blend_mix",
        SpriteBlend::Additive => "blend_add",
    };
    let cutout = if blend == SpriteBlend::Opaque { "if (texel.a < 0.02) { discard; }" } else { "" };
    let chroma_test = if chroma { "if (texel.g < 0.5 * min(texel.r, texel.b)) { discard; }" } else { "" };
    let alpha = if blend == SpriteBlend::Opaque { "vertex_tint.a" } else { "texel.a * vertex_tint.a" };
    format!(r#"shader_type canvas_item;
render_mode {render_mode};
varying vec4 forge_vertex_tint;
void vertex() {{
    forge_vertex_tint = COLOR;
}}
void fragment() {{
    vec4 vertex_tint = forge_vertex_tint;
    vec4 texel = texture(TEXTURE, UV);
    if (texel.a <= 0.0) {{ discard; }}
    {cutout}
    {chroma_test}
    COLOR = vec4(texel.rgb * vertex_tint.rgb, {alpha});
}}
"#)
}

fn spatial_shader(blend: SpriteBlend, chroma: bool) -> String {
    let mode = match blend {
        SpriteBlend::Opaque => "depth_draw_opaque",
        SpriteBlend::Alpha => "blend_mix",
        SpriteBlend::Additive => "blend_add",
    };
    let cutoff = if blend == SpriteBlend::Opaque { "if (texel.a < 0.02) { discard; }" } else { "" };
    let chroma_test = if chroma { "if (srgb.g < 0.5 * min(srgb.r, srgb.b)) { discard; }" } else { "" };
    let alpha = if blend == SpriteBlend::Opaque { "" } else { "ALPHA = texel.a * forge_tint.a;" };
    format!(r#"shader_type spatial;
render_mode unshaded, cull_disabled, {mode};
uniform sampler2D forge_tex : filter_nearest, repeat_disable;
uniform vec4 forge_tint = vec4(1.0);
uniform vec4 forge_uv_rect = vec4(0.0, 0.0, 1.0, 1.0);
uniform vec2 forge_flip = vec2(0.0);
uniform bool forge_reinhard_compensation = false;
vec3 forge_linear_to_srgb(vec3 c) {{
    return mix(1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055, c * 12.92, lessThan(c, vec3(0.0031308)));
}}
vec3 forge_srgb_to_linear(vec3 c) {{
    return mix(pow((c + vec3(0.055)) / 1.055, vec3(2.4)), c / 12.92, lessThan(c, vec3(0.04045)));
}}
void fragment() {{
    vec2 frame_uv = mix(UV, vec2(1.0) - UV, forge_flip);
    vec4 texel = texture(forge_tex, forge_uv_rect.xy + frame_uv * forge_uv_rect.zw);
    if (texel.a <= 0.0) {{ discard; }}
    {cutoff}
    vec3 srgb = texel.rgb;
    {chroma_test}
    vec3 linear_color = forge_srgb_to_linear(srgb * forge_tint.rgb);
    if (forge_reinhard_compensation) {{
        vec3 y = min(linear_color, vec3(0.9999));
        linear_color = y / (vec3(1.0) - y);
        ALBEDO = OUTPUT_IS_SRGB ? forge_linear_to_srgb(linear_color) : linear_color;
    }} else {{
        ALBEDO = OUTPUT_IS_SRGB ? srgb * forge_tint.rgb : linear_color;
    }}
    {alpha}
}}
"#)
}

fn tint_values(draw: &SpriteDraw) -> [f32; 4] {
    if draw.selected {
        [draw.tint[0], draw.tint[1] * 0.62, draw.tint[2] * 0.18, draw.tint[3]]
    } else {
        draw.tint
    }
}

fn rgba(draw: &SpriteDraw) -> Color {
    let tint = tint_values(draw);
    Color::from_rgba(tint[0], tint[1], tint[2], tint[3])
}

fn material_index(blend: SpriteBlend, chroma: bool) -> usize {
    blend_slot(blend) * 2 + usize::from(chroma)
}

fn tex_key(tex: &TexData, generation: u64) -> (String, usize, u64) {
    (tex.guid.clone(), tex.rgba.as_ptr() as usize, generation)
}

/// Sprite 的 shader、贴图与材质缓存。reload 时清掉按代次生成的动态资源；固定 shader 延续到宿主关闭。
pub struct SpriteAssets {
    pub graphs: crate::shader_graph::GraphMaterials,
    canvas_materials: Vec<Owned>,
    spatial_materials: HashMap<(String, usize, u64, [u32; 11], usize), Owned>,
    textures: HashMap<(String, usize, u64), Gd<ImageTexture>>,
    #[allow(dead_code)]
    canvas_shaders: Vec<Owned>,
    spatial_shaders: Vec<Owned>,
    spatial_quad: Option<Owned>,
}

impl SpriteAssets {
    pub fn new(rs: &mut Gd<RenderingServer>) -> Self {
        let mut canvas_shaders = Vec::with_capacity(6);
        let mut canvas_materials = Vec::with_capacity(6);
        let mut spatial_shaders = Vec::with_capacity(6);
        for blend in [SpriteBlend::Opaque, SpriteBlend::Alpha, SpriteBlend::Additive] {
            for chroma in [false, true] {
                let canvas_code = canvas_shader(blend, chroma);
                let canvas_shader_rid = Owned::new(rs.shader_create());
                rs.shader_set_code(canvas_shader_rid.rid(), &canvas_code);
                let canvas_material = Owned::new(rs.material_create());
                rs.material_set_shader(canvas_material.rid(), canvas_shader_rid.rid());
                canvas_shaders.push(canvas_shader_rid);
                canvas_materials.push(canvas_material);

                let spatial_code = spatial_shader(blend, chroma);
                let spatial_shader_rid = Owned::new(rs.shader_create());
                rs.shader_set_code(spatial_shader_rid.rid(), &spatial_code);
                spatial_shaders.push(spatial_shader_rid);
            }
        }
        Self {
            graphs: crate::shader_graph::GraphMaterials::new(),
            canvas_materials,
            spatial_materials: HashMap::new(),
            textures: HashMap::new(),
            canvas_shaders,
            spatial_shaders,
            spatial_quad: None,
        }
    }

    pub fn clear(&mut self) {
        self.graphs.clear();
        self.spatial_materials.clear();
        self.textures.clear();
        self.spatial_quad = None;
    }

    fn texture(&mut self, tex: &TexData, generation: u64) -> Option<Rid> {
        let key = tex_key(tex, generation);
        if !self.textures.contains_key(&key) {
            self.textures.insert(key.clone(), image_texture(tex.w, tex.h, tex.rgba)?);
        }
        self.textures.get(&key).map(|t| t.get_rid())
    }

    pub fn canvas_material(&mut self, rs:&mut Gd<RenderingServer>, draw: &SpriteDraw) -> Option<Rid> {
        if let Some(graph)=&draw.graph{return self.graphs.material(rs,graph,&engine_host::shader::Texture{width:draw.tex.w,height:draw.tex.h,rgba:std::sync::Arc::new(draw.tex.rgba.to_vec())},true,tint_values(draw),draw.uv_rect,draw.flip);}
        Some(self.canvas_materials[material_index(draw.blend, draw.chroma_key)].rid())
    }

    pub fn canvas_texture(&mut self, draw: &SpriteDraw, generation: u64) -> Option<Rid> {
        self.texture(&draw.tex, generation)
    }

    pub fn spatial_quad(&mut self, rs: &mut Gd<RenderingServer>) -> Rid {
        self.spatial_quad.get_or_insert_with(|| {
            let p = [Vector3::new(-0.5, -0.5, 0.0), Vector3::new(0.5, -0.5, 0.0), Vector3::new(0.5, 0.5, 0.0), Vector3::new(-0.5, 0.5, 0.0)];
            let uv = [Vector2::new(0.0, 1.0), Vector2::new(1.0, 1.0), Vector2::new(1.0, 0.0), Vector2::new(0.0, 0.0)];
            let mut pos = Vec::with_capacity(6);
            let mut tex_uv = Vec::with_capacity(6);
            for i in [0usize, 2, 1, 0, 3, 2] { pos.push(p[i]); tex_uv.push(uv[i]); }
            let mut arrays = VarArray::new();
            for slot in 0..13 {
                match slot {
                    0 => arrays.push(&PackedVector3Array::from(pos.as_slice()).to_variant()),
                    4 => arrays.push(&PackedVector2Array::from(tex_uv.as_slice()).to_variant()),
                    _ => arrays.push(&Variant::nil()),
                }
            }
            let mesh = rs.mesh_create();
            rs.mesh_add_surface_from_arrays(mesh, PrimitiveType::TRIANGLES, &arrays);
            Owned::new(mesh)
        }).rid()
    }

    fn ensure_spatial_material(&mut self, rs: &mut Gd<RenderingServer>, identity: (String, usize, u64), tex_rid: Rid, tint: [f32; 4], blend: SpriteBlend, chroma: bool, uv: [f32; 4], flip: [bool; 2], compensate_reinhard: bool) -> Rid {
        let size = tint.map(f32::to_bits);
        let rect = uv.map(f32::to_bits);
        let key = (identity.0, identity.1, identity.2, [size[0], size[1], size[2], size[3], rect[0], rect[1], rect[2], rect[3], u32::from(flip[0]), u32::from(flip[1]), u32::from(compensate_reinhard)], material_index(blend, chroma));
        if !self.spatial_materials.contains_key(&key) {
            let shader = self.spatial_shaders[material_index(blend, chroma)].rid();
            let mat = Owned::new(rs.material_create());
            rs.material_set_shader(mat.rid(), shader);
            rs.material_set_param(mat.rid(), "forge_tex", &tex_rid.to_variant());
            rs.material_set_param(mat.rid(), "forge_tint", &Vector4::new(tint[0], tint[1], tint[2], tint[3]).to_variant());
            rs.material_set_param(mat.rid(), "forge_uv_rect", &Vector4::new(uv[0], uv[1], uv[2], uv[3]).to_variant());
            rs.material_set_param(mat.rid(), "forge_flip", &Vector2::new(u8::from(flip[0]) as f32, u8::from(flip[1]) as f32).to_variant());
            rs.material_set_param(mat.rid(), "forge_reinhard_compensation", &compensate_reinhard.to_variant());
            self.spatial_materials.insert(key.clone(), mat);
        }
        self.spatial_materials[&key].rid()
    }

    pub fn spatial_material(&mut self, rs: &mut Gd<RenderingServer>, draw: &SpriteDraw, generation: u64, compensate_reinhard: bool) -> Option<Rid> {
        if let Some(graph)=&draw.graph{return self.graphs.material(rs,graph,&engine_host::shader::Texture{width:draw.tex.w,height:draw.tex.h,rgba:std::sync::Arc::new(draw.tex.rgba.to_vec())},false,tint_values(draw),draw.uv_rect,draw.flip);}
        let texture = self.texture(&draw.tex, generation)?;
        let tint = tint_values(draw);
        Some(self.ensure_spatial_material(rs, tex_key(&draw.tex, generation), texture, tint, draw.blend, draw.chroma_key, draw.uv_rect, draw.flip, compensate_reinhard))
    }

    /// V6 图集帧使用原生裁切像素，贴图与材质按帧 key 缓存，帧变化时旧资源仍由代次清理释放。
    pub fn v6_texture_material(&mut self, rs: &mut Gd<RenderingServer>, key: &str, width: u32, height: u32,
        pixels: &[u8], tint: [f32; 4], blend: SpriteBlend) -> Option<(Rid, Rid)> {
        let identity = (format!("v6:{key}"), 0, 0);
        let texture_key = identity.clone();
        if !self.textures.contains_key(&texture_key) {
            self.textures.insert(texture_key.clone(), image_texture(width, height, pixels)?);
        }
        let texture = self.textures.get(&texture_key)?.get_rid();
        let material = self.ensure_spatial_material(rs, identity, texture, tint, blend, false, [0.0, 0.0, 1.0, 1.0], [false, false], false);
        Some((texture, material))
    }
}

struct CanvasSprite {
    item: Owned,
    draw: SpriteDraw,
}

/// 纯正交 2D 场景的 Canvas 绘制会话。保留 item RID，差量帧更新命令/变换，不逐帧重建资源。
pub struct CanvasSprites {
    items: BTreeMap<ItemKey, CanvasSprite>,
    canvas: Owned,
    viewport: Rid,
}

impl CanvasSprites {
    pub fn new(rs: &mut Gd<RenderingServer>, viewport: Rid) -> Self {
        let canvas = Owned::new(rs.canvas_create());
        rs.viewport_attach_canvas(viewport, canvas.rid());
        rs.viewport_set_canvas_stacking(viewport, canvas.rid(), 0, 0);
        Self { items: BTreeMap::new(), canvas, viewport }
    }

    fn item_transform(world: &M4) -> Transform2D {
        let x = Vector2::new(world[0][0], -world[1][0]);
        // Canvas quad local +Y points down, whereas the world quad local +Y points up.
        let y = Vector2::new(-world[0][1], world[1][1]);
        let origin = Vector2::new(world[0][3], -world[1][3]);
        let sx = x.length();
        let angle = x.y.atan2(x.x);
        let sy = if sx > 1e-8 { (x.x * y.y - x.y * y.x) / sx } else { y.length() };
        Transform2D::from_angle_scale_skew_origin(angle as f32, Vector2::new(sx, sy), 0.0, origin)
    }

    pub fn update(&mut self, rs: &mut Gd<RenderingServer>, assets: &mut SpriteAssets, list: &RenderList) {
        let live: BTreeSet<ItemKey> = list.items.iter().filter_map(|item| matches!(&item.body, ItemBody::Sprite(_)).then_some(item.key)).collect();
        self.items.retain(|key, _| live.contains(key));
        let mut order: Vec<&RenderItem> = list.items.iter().filter(|item| matches!(&item.body, ItemBody::Sprite(_))).collect();
        order.sort_by(|a, b| {
            let get = |item: &RenderItem| match &item.body { ItemBody::Sprite(d) => d.sorting_order, _ => 0.0 };
            get(a).total_cmp(&get(b)).then_with(|| a.key.entity.cmp(&b.key.entity)).then_with(|| a.order.cmp(&b.order))
        });
        for (draw_index, item) in order.into_iter().enumerate() {
            let ItemBody::Sprite(draw) = &item.body else { continue };
            if !self.items.contains_key(&item.key) {
                let rid = Owned::new(rs.canvas_item_create());
                rs.canvas_item_set_parent(rid.rid(), self.canvas.rid());
                rs.canvas_item_set_default_texture_filter(rid.rid(), CanvasItemTextureFilter::NEAREST);
                self.items.insert(item.key, CanvasSprite { item: rid, draw: draw.clone() });
            }
            let Some(texture) = assets.canvas_texture(draw, list.asset_generation) else {
                rs.canvas_item_set_visible(self.items[&item.key].item.rid(), false);
                continue;
            };
            let entry = self.items.get_mut(&item.key).unwrap();
            rs.canvas_item_set_visible(entry.item.rid(), true);
            rs.canvas_item_set_draw_index(entry.item.rid(), draw_index as i32);
            let Some(material)=assets.canvas_material(rs,draw)else{continue;};
            rs.canvas_item_set_material(entry.item.rid(),material);
            rs.canvas_item_set_modulate(entry.item.rid(), rgba(draw));
            rs.canvas_item_set_transform(entry.item.rid(), Self::item_transform(&item.world));
            rs.canvas_item_clear(entry.item.rid());
            // RS treats a negative size as a UV-flip flag and takes its absolute size;
            // it does not move the rectangle origin as a geometric negative extent would.
            let rect_origin = Vector2::new(-0.5, -0.5);
            let rect_size = Vector2::new(if draw.flip[0] { -1.0 } else { 1.0 }, if draw.flip[1] { -1.0 } else { 1.0 });
            let src = Rect2::new(
                Vector2::new(draw.uv_rect[0] * draw.tex.w as f32, draw.uv_rect[1] * draw.tex.h as f32),
                Vector2::new(draw.uv_rect[2] * draw.tex.w as f32, draw.uv_rect[3] * draw.tex.h as f32),
            );
            rs.canvas_item_add_texture_rect_region_ex(
                entry.item.rid(), Rect2::new(rect_origin, rect_size), texture, src,
            ).clip_uv(true).done();
            entry.draw = draw.clone();
        }
        let Projection::Orthographic { half_h } = list.view.projection else { return };
        let scale = list.height as f32 / (2.0 * half_h.max(1e-5));
        let origin = Vector2::new(list.width as f32 * 0.5 - list.view.center[0] * scale,
            list.height as f32 * 0.5 + list.view.center[1] * scale);
        let canvas_transform = Transform2D::from_angle_scale_skew_origin(0.0, Vector2::new(scale, scale), 0.0, origin);
        rs.viewport_set_canvas_transform(self.viewport, self.canvas.rid(), canvas_transform);
    }

    pub fn instances(&self) -> usize { self.items.len() }
}
