//! V6 腿(01 §5.7),只在 [gmain]。records → MultiMesh,terrain → 带顶点色的网格。
//! rurix(sentinels_v6_render/rurix.rs:8-27):每个盒子只画顶 / +X / +Y 三个面,面阴影 1.0 / 0.65 / 0.82 乘在 sRGB 编码值上,
//! 不受光、不剔除;投影是 2D 等距:sx = (x − y)/2,sy = −(x + y)/4 + 1.5z,深度键 k = x + y + 2z(越大越近)。
//! 这个映射相对右手系是镜像、两轴缩放不等,普通相机拍不出来,于是把 iso 烘进每个实例的变换:
//! Godot 坐标 G = A·p,A 的三行 = (0.5, −0.5, 0)、(−0.25, −0.25, 1.5)、(S, S, 2S),S = 0.25;
//! 相机是朝 −Z 的正交相机(list.rs 按 Batch.view 构造),于是屏幕坐标 = (sx, sy),深度顺序 = k 的顺序。

use std::collections::{HashMap, HashSet};

use godot::classes::rendering_server::{MultimeshTransformFormat, PrimitiveType};
use godot::classes::RenderingServer;
use godot::prelude::*;

use engine_host::{SpriteBlend, V6Frame, V6SpriteDraw};

use crate::rid::Owned;
use crate::sprite::SpriteAssets;

const S: f32 = 0.25;
const SHADES: [f32; 3] = [1.0, 0.65, 0.82];

fn a(p: [f32; 3]) -> Vector3 {
    Vector3::new(0.5 * (p[0] - p[1]), -0.25 * (p[0] + p[1]) + 1.5 * p[2], S * (p[0] + p[1] + 2.0 * p[2]))
}

pub fn shader(compatibility: bool) -> String {
    let out = if compatibility { "ALBEDO = srgb;" } else { "ALBEDO = forge_srgb_to_linear(srgb);" };
    format!(
        r#"shader_type spatial;
render_mode unshaded, cull_disabled;
vec3 forge_srgb_to_linear(vec3 c) {{
	return mix(pow((c + vec3(0.055)) * (1.0 / 1.055), vec3(2.4)), c * (1.0 / 12.92), lessThan(c, vec3(0.04045)));
}}
void fragment() {{
	vec3 srgb = clamp(COLOR.rgb * UV.x, vec3(0.0), vec3(1.0));
	{out}
}}
"#
    )
}

/// 一个盒子的三个面(各 2 个三角形)在"最小角 (x,y,z)、尺寸 (w,h,ht)"下的 18 个顶点 + 面阴影。
fn box_faces(o: [f32; 3], s: [f32; 3], mut f: impl FnMut([f32; 3], f32)) {
    let t = [[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    for (face, shade) in SHADES.iter().enumerate() {
        for q in t {
            let p = match face {
                0 => [q[0] * s[0], q[1] * s[1], s[2]],
                1 => [s[0], q[0] * s[1], q[1] * s[2]],
                _ => [q[0] * s[0], s[1], q[1] * s[2]],
            };
            f([o[0] + p[0], o[1] + p[1], o[2] + p[2]], *shade);
        }
    }
}

fn surface(rs: &mut Gd<RenderingServer>, pos: &[Vector3], col: &[Color], uv: &[Vector2]) -> Owned {
    let mut arrays = VarArray::new();
    for i in 0..13 {
        match i {
            0 => arrays.push(&PackedVector3Array::from(pos).to_variant()),
            3 if !col.is_empty() => arrays.push(&PackedColorArray::from(col).to_variant()),
            4 => arrays.push(&PackedVector2Array::from(uv).to_variant()),
            _ => arrays.push(&Variant::nil()),
        }
    }
    let mesh = rs.mesh_create();
    rs.mesh_add_surface_from_arrays(mesh, PrimitiveType::TRIANGLES, &arrays);
    Owned::new(mesh)
}

fn quad_sprite_mesh(rs: &mut Gd<RenderingServer>, sprite: &V6SpriteDraw, ground: bool) -> Owned {
    let mut pos = Vec::new();
    let mut uv = Vec::new();
    let corners = [[0.0f32, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    if ground {
        for i in [0usize, 2, 1, 0, 3, 2] {
            let q = corners[i];
            pos.push(Vector3::new(q[0] - 0.5, q[1] - 0.5, 0.0));
            uv.push(Vector2::new(q[0], q[1]));
        }
        return surface(rs, &pos, &[], &uv);
    }
    let aspect = sprite.height as f32 / sprite.width.max(1) as f32;
    let (sin, cos) = sprite.rotation.sin_cos();
    let point = |q: [f32; 2]| {
        let dx = q[0] - sprite.pivot[0];
        let dy = (sprite.pivot[1] - q[1]) * aspect;
        let rx = dx * cos - dy * sin;
        let ry = dx * sin + dy * cos;
        (Vector3::new(rx, ry, (-ry).max(0.0)), ry)
    };
    let clip = |poly: Vec<[f32; 2]>, below: bool| {
        let mut result = Vec::new();
        if poly.is_empty() { return result; }
        let inside = |ry: f32| if below { ry <= 1e-7 } else { ry >= -1e-7 };
        let mut a = *poly.last().unwrap();
        let (_, mut da) = point(a);
        for b in poly {
            let (_, db) = point(b);
            if inside(da) != inside(db) {
                let t = da / (da - db);
                result.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            }
            if inside(db) { result.push(b); }
            a = b;
            da = db;
        }
        result
    };
    let triangles = [[corners[0], corners[1], corners[2]], [corners[0], corners[2], corners[3]]];
    for triangle in triangles {
        for below in [true, false] {
            let polygon = clip(triangle.to_vec(), below);
            if polygon.len() < 3 { continue; }
            for k in 1..polygon.len() - 1 {
                for q in [polygon[0], polygon[k], polygon[k + 1]] {
                    let (p, _) = point(q);
                    pos.push(p);
                    uv.push(Vector2::new(q[0], q[1]));
                }
            }
        }
    }
    surface(rs, &pos, &[], &uv)
}

fn sprite_instance_transform(sprite: &V6SpriteDraw) -> Transform3D {
    let [x, y, z] = sprite.position;
    if sprite.ground {
        let scale = sprite.scale;
        let origin = a([x, y, z]);
        return Transform3D::new(
            Basis::from_cols(
                Vector3::new(0.5 * scale, -0.25 * scale, S * scale),
                Vector3::new(-0.5 * scale, -0.25 * scale, S * scale),
                Vector3::new(0.0, 0.0, 2.0 * S * 0.001),
            ),
            origin,
        );
    }
    let screen = a([x, y, z]);
    let scale = (sprite.span * sprite.scale).max(1e-4);
    Transform3D::new(
        Basis::from_scale(Vector3::new(scale, scale, scale)),
        Vector3::new(screen.x, screen.y, screen.z),
    )
}

fn sprite_mesh_key(sprite: &V6SpriteDraw) -> [u32; 6] {
    [
        (sprite.span * sprite.scale).to_bits(),
        (sprite.height as f32 / sprite.width.max(1) as f32).to_bits(),
        sprite.pivot[0].to_bits(), sprite.pivot[1].to_bits(), sprite.rotation.to_bits(),
        u32::from(sprite.ground),
    ]
}

/// V6 的 RS 对象:单位盒子网格 + MultiMesh 实例 + 地形网格实例 + V6 原生图集精灵 + 材质。字段按 drop 顺序排。
pub struct V6Draw {
    _mm_inst: Owned,
    sprite_instances: Vec<Owned>,
    sprite_meshes: HashMap<[u32; 6], Owned>,
    terrain_inst: Option<Owned>,
    terrain_mesh: Option<Owned>,
    terrain_key: usize,
    mm: Owned,
    count: usize,
    _cube: Owned,
    material: Owned,
    _shader: Owned,
}

impl V6Draw {
    pub fn new(rs: &mut Gd<RenderingServer>, scenario: Rid, compatibility: bool) -> V6Draw {
        let shader = Owned::new(rs.shader_create());
        rs.shader_set_code(shader.rid(), shader_code(compatibility).as_str());
        let material = Owned::new(rs.material_create());
        rs.material_set_shader(material.rid(), shader.rid());
        // 单位盒子:中心在原点、边长 1(实例变换 = A·diag(w, h, ht),原点 = A·盒子中心)。
        let (mut pos, mut uv) = (Vec::new(), Vec::new());
        box_faces([-0.5; 3], [1.0; 3], |p, shade| {
            pos.push(Vector3::new(p[0], p[1], p[2]));
            uv.push(Vector2::new(shade, 0.0));
        });
        let cube = surface(rs, &pos, &[], &uv);
        let mm = Owned::new(rs.multimesh_create());
        rs.multimesh_set_mesh(mm.rid(), cube.rid());
        let mm_inst = Owned::new(rs.instance_create2(mm.rid(), scenario));
        rs.instance_geometry_set_material_override(mm_inst.rid(), material.rid());
        V6Draw { _mm_inst: mm_inst, sprite_instances: Vec::new(), sprite_meshes: HashMap::new(), terrain_inst: None, terrain_mesh: None, terrain_key: 0, mm, count: usize::MAX, _cube: cube, material, _shader: shader }
    }

    /// 本帧的 RS 实例数(MultiMesh + 地形网格 + V6 原生精灵)。
    pub fn instances(&self) -> usize {
        1 + usize::from(self.terrain_inst.is_some()) + self.sprite_instances.len()
    }

    pub fn update(&mut self, rs: &mut Gd<RenderingServer>, scenario: Rid, f: &V6Frame, sprites: &mut SpriteAssets) {
        let n = f.objects.len();
        if n != self.count {
            rs.multimesh_allocate_data_ex(self.mm.rid(), n.max(1) as i32, MultimeshTransformFormat::TRANSFORM_3D)
                .color_format(true)
                .done();
            self.count = n;
        }
        let mut buf = Vec::with_capacity(n.max(1) * 16);
        for r in &f.objects {
            let (w, h, ht) = (r[4], r[5], r[6]);
            let c = a([r[0] + w / 2.0, r[1] + h / 2.0, r[2] + ht / 2.0]);
            // 行主序 3×4 = [A·diag(w, h, ht) | A·中心],后接颜色(sRGB 编码值,着色器里乘面阴影再换线性)。
            buf.extend_from_slice(&[0.5 * w, -0.5 * h, 0.0, c.x, -0.25 * w, -0.25 * h, 1.5 * ht, c.y, S * w, S * h, 2.0 * S * ht, c.z]);
            buf.extend_from_slice(&[r[8], r[9], r[10], r[11]]);
        }
        if n == 0 {
            buf.extend_from_slice(&[0.0; 16]);
        }
        rs.multimesh_set_buffer(self.mm.rid(), &PackedFloat32Array::from(buf.as_slice()));
        rs.multimesh_set_visible_instances(self.mm.rid(), n as i32);
        let key = std::sync::Arc::as_ptr(&f.terrain) as usize;
        if key != self.terrain_key {
            self.terrain_key = key;
            self.terrain_inst = None;
            self.terrain_mesh = None;
            if !f.terrain.is_empty() {
                let (mut pos, mut col, mut uv) = (Vec::new(), Vec::new(), Vec::new());
                for r in f.terrain.iter() {
                    let color = Color::from_rgba(r[8].max(0.0), r[9].max(0.0), r[10].max(0.0), r[11]);
                    box_faces([r[0], r[1], r[2]], [r[4], r[5], r[6]], |p, shade| {
                        pos.push(a(p));
                        col.push(color);
                        uv.push(Vector2::new(shade, 0.0));
                    });
                }
                let mesh = surface(rs, &pos, &col, &uv);
                let inst = Owned::new(rs.instance_create2(mesh.rid(), scenario));
                rs.instance_geometry_set_material_override(inst.rid(), self.material.rid());
                self.terrain_mesh = Some(mesh);
                self.terrain_inst = Some(inst);
            }
        }
        self.sprite_instances.clear();
        let mut wanted = HashSet::new();
        for sprite in &f.sprites {
            let key = sprite_mesh_key(sprite);
            wanted.insert(key);
            let mesh = if let Some(mesh) = self.sprite_meshes.get(&key) { mesh.rid() } else {
                let mesh = quad_sprite_mesh(rs, sprite, sprite.ground);
                let rid = mesh.rid();
                self.sprite_meshes.insert(key, mesh);
                rid
            };
            let blend = if sprite.additive { SpriteBlend::Additive } else { SpriteBlend::Alpha };
            let Some((_, mat)) = sprites.v6_texture_material(
                rs,
                &sprite.key,
                sprite.width,
                sprite.height,
                sprite.pixels.as_ref(),
                sprite.tint,
                blend,
            ) else {
                // A missing/invalid atlas texture must not render with the cube shader.
                // That fallback produced visible untextured triangles in place of a sprite.
                continue;
            };
            let inst = Owned::new(rs.instance_create2(mesh, scenario));
            rs.instance_geometry_set_material_override(inst.rid(), mat);
            rs.instance_set_transform(inst.rid(), sprite_instance_transform(sprite));
            self.sprite_instances.push(inst);
        }
        self.sprite_meshes.retain(|key, _| wanted.contains(key));
    }
}

fn shader_code(compatibility: bool) -> String {
    shader(compatibility)
}
