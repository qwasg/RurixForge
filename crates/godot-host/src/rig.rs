//! F-GAME-5(D-050):Rig2D(`ItemBody::RigMesh`)的 Godot 4.7.2 映射。
//! - Canvas 路径(纯 2D 正交):`canvas_item_add_triangle_array_ex`,逐顶点 `colors` 携带 `tint * opacity`
//!   (nrm.z 通道),与 `Sprite` 共用同一个 Canvas 与 `sorting_order` 排序。
//! - Spatial 路径(3D / 混合):`ArrayMesh` 动态顶点缓冲(`ARRAY_VERTEX` + `ARRAY_COLOR` + `ARRAY_TEX_UV`);
//!   姿态变形(`RenderDelta::rig_verts`)只调 `update_spatial_mesh` 原地刷新顶点,不重建实例与材质。

use godot::classes::rendering_server::PrimitiveType;
use godot::classes::RenderingServer;
use godot::prelude::*;

use engine_host::{RigDraw, SpriteBlend, M4, RIG_VERTEX_F32};

use crate::rid::Owned;
use crate::sprite::{CanvasSprites, SpriteAssets};

const ARRAY_VERTEX: usize = 0;
const ARRAY_COLOR: usize = 3;
const ARRAY_TEX_UV: usize = 4;
const ARRAY_MAX: usize = 13;

pub fn tint_values(draw: &RigDraw) -> [f32; 4] {
    if draw.selected {
        [draw.tint[0], draw.tint[1] * 0.62, draw.tint[2] * 0.18, draw.tint[3]]
    } else {
        draw.tint
    }
}

pub fn spatial_shader(blend: SpriteBlend, chroma: bool) -> String {
    let mode = match blend {
        SpriteBlend::Opaque => "depth_draw_opaque",
        SpriteBlend::Alpha => "blend_mix",
        SpriteBlend::Additive => "blend_add",
    };
    let cutoff = if blend == SpriteBlend::Opaque {
        "if (texel.a * COLOR.a < 0.02) { discard; }"
    } else {
        ""
    };
    let chroma_test = if chroma {
        "if (srgb.g < 0.5 * min(srgb.r, srgb.b)) { discard; }"
    } else {
        ""
    };
    let alpha = if blend == SpriteBlend::Opaque {
        ""
    } else {
        "float a = texel.a * forge_tint.a * COLOR.a;\n    if (a <= 0.0) { discard; }\n    ALPHA = a;"
    };
    format!(
        r#"shader_type spatial;
render_mode unshaded, cull_disabled, {mode};
uniform sampler2D forge_tex : filter_nearest, repeat_disable;
uniform vec4 forge_tint = vec4(1.0);
uniform bool forge_reinhard_compensation = false;
vec3 forge_linear_to_srgb(vec3 c) {{
    return mix(1.055 * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - 0.055, c * 12.92, lessThan(c, vec3(0.0031308)));
}}
vec3 forge_srgb_to_linear(vec3 c) {{
    return mix(pow((c + vec3(0.055)) / 1.055, vec3(2.4)), c / 12.92, lessThan(c, vec3(0.04045)));
}}
void fragment() {{
    vec4 texel = texture(forge_tex, UV);
    if (texel.a <= 0.0 || COLOR.a <= 0.0) {{ discard; }}
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
"#
    )
}

/// Canvas 路径:将 `RigDraw` 写入指定 `canvas_item`。
/// `RigDraw.verts` 的本地坐标 Y 向上,配合 `CanvasSprites::item_transform` 需将本地 Y 取反。
pub fn draw_canvas_rig(
    rs: &mut Gd<RenderingServer>,
    assets: &mut SpriteAssets,
    item_rid: Rid,
    world: &M4,
    draw: &RigDraw,
    generation: u64,
) {
    let Some(texture) = assets.rig_texture(&draw.tex, generation) else {
        rs.canvas_item_set_visible(item_rid, false);
        return;
    };
    let material = assets.rig_canvas_material(draw.blend);
    rs.canvas_item_set_visible(item_rid, true);
    rs.canvas_item_set_material(item_rid, material);
    rs.canvas_item_set_modulate(item_rid, Color::WHITE);
    rs.canvas_item_set_transform(item_rid, CanvasSprites::item_transform(world));
    rs.canvas_item_clear(item_rid);

    let tri_verts = (draw.vertex_count as usize).min(draw.verts.len() / RIG_VERTEX_F32) / 3 * 3;
    if tri_verts < 3 {
        return;
    }
    let tint = tint_values(draw);
    let mut indices = Vec::with_capacity(tri_verts);
    let mut points = Vec::with_capacity(tri_verts);
    let mut colors = Vec::with_capacity(tri_verts);
    let mut uvs = Vec::with_capacity(tri_verts);
    for i in 0..tri_verts {
        let b = i * RIG_VERTEX_F32;
        let opacity = draw.verts[b + 5].clamp(0.0, 1.0);
        indices.push(i as i32);
        points.push(Vector2::new(draw.verts[b], -draw.verts[b + 1]));
        colors.push(Color::from_rgba(tint[0], tint[1], tint[2], tint[3] * opacity));
        uvs.push(Vector2::new(draw.verts[b + 6], draw.verts[b + 7]));
    }
    rs.canvas_item_add_triangle_array_ex(
        item_rid,
        &PackedInt32Array::from(indices.as_slice()),
        &PackedVector2Array::from(points.as_slice()),
        &PackedColorArray::from(colors.as_slice()),
    )
    .uvs(&PackedVector2Array::from(uvs.as_slice()))
    .texture(texture)
    .done();
}

/// Spatial 路径:创建承载 `RigDraw` 顶点的动态 `ArrayMesh`。
pub fn create_spatial_mesh(rs: &mut Gd<RenderingServer>, verts: &[f32]) -> Owned {
    let mesh = Owned::new(rs.mesh_create());
    update_spatial_mesh(rs, mesh.rid(), verts);
    mesh
}

/// Spatial 路径:原地更新 `ArrayMesh` 顶点(`RenderDelta::rig_verts` 差量通道调用)。
pub fn update_spatial_mesh(rs: &mut Gd<RenderingServer>, mesh: Rid, verts: &[f32]) {
    rs.mesh_clear(mesh);
    let tri_count = (verts.len() / RIG_VERTEX_F32) / 3;
    if tri_count == 0 {
        return;
    }
    let v_len = tri_count * 3;
    let mut pos = Vec::with_capacity(v_len);
    let mut col = Vec::with_capacity(v_len);
    let mut uv = Vec::with_capacity(v_len);
    for t in 0..tri_count {
        // Forge 逆时针正面 → Godot 顺时针正面([0, 2, 1])。
        for k in [0usize, 2, 1] {
            let b = (t * 3 + k) * RIG_VERTEX_F32;
            let opacity = verts[b + 5].clamp(0.0, 1.0);
            pos.push(Vector3::new(verts[b], verts[b + 1], verts[b + 2]));
            col.push(Color::from_rgba(1.0, 1.0, 1.0, opacity));
            uv.push(Vector2::new(verts[b + 6], verts[b + 7]));
        }
    }
    let mut arrays = VarArray::new();
    for slot in 0..ARRAY_MAX {
        match slot {
            ARRAY_VERTEX => arrays.push(&PackedVector3Array::from(pos.as_slice()).to_variant()),
            ARRAY_COLOR => arrays.push(&PackedColorArray::from(col.as_slice()).to_variant()),
            ARRAY_TEX_UV => arrays.push(&PackedVector2Array::from(uv.as_slice()).to_variant()),
            _ => arrays.push(&Variant::nil()),
        }
    }
    rs.mesh_add_surface_from_arrays(mesh, PrimitiveType::TRIANGLES, &arrays);
}
