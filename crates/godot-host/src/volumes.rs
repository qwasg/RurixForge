//! Stage 5 实体级体积类组件(01 §6.1、02 §9.5 Stage 5),只在 [gmain]:ReflectionProbe / Decal / FogVolume。
//! 每个 VolumeItem 一个 RS 对象 + scenario 里的 instance(变换 = 实体世界矩阵);RenderDelta.volumes 下发时按 key 对比:
//! content 变了重建、只有 world 变了只改变换、消失的释放。

use std::collections::BTreeMap;

use godot::classes::rendering_server::{DecalTexture, FogVolumeShape, ReflectionProbeAmbientMode, ReflectionProbeUpdateMode};
use godot::classes::{FogMaterial, ImageTexture, Object, RenderingServer};
use godot::prelude::*;

use engine_host::{ItemKey, RenderList, TexData, M4};

use crate::props::{fields, Fields};
use crate::rid::Owned;
use crate::scene::{v3, xform};

fn color(c: [f32; 4]) -> Color {
    Color::from_rgba(c[0], c[1], c[2], c[3])
}

/// 一个实例:instance 先放,再放 RS 对象,最后放它引用的资源(贴图 / FogMaterial)。
struct Vol {
    inst: Owned,
    _base: Owned,
    _textures: Vec<Gd<ImageTexture>>,
    _material: Option<Gd<FogMaterial>>,
    content: u64,
    world: M4,
}

#[derive(Default)]
pub struct Volumes {
    items: BTreeMap<ItemKey, Vol>,
}

fn probe(rs: &mut Gd<RenderingServer>, f: &Fields) -> Owned {
    let p = Owned::new(rs.reflection_probe_create());
    let r = p.rid();
    rs.reflection_probe_set_update_mode(r, if f.text("updateMode") == "always" { ReflectionProbeUpdateMode::ALWAYS } else { ReflectionProbeUpdateMode::ONCE });
    rs.reflection_probe_set_intensity(r, f.num("intensity"));
    rs.reflection_probe_set_blend_distance(r, f.num("blendDistance"));
    rs.reflection_probe_set_max_distance(r, f.num("maxDistance"));
    rs.reflection_probe_set_size(r, v3(f.vec3("size")));
    rs.reflection_probe_set_origin_offset(r, v3(f.vec3("originOffset")));
    rs.reflection_probe_set_as_interior(r, f.flag("interior"));
    rs.reflection_probe_set_enable_box_projection(r, f.flag("boxProjection"));
    rs.reflection_probe_set_enable_shadows(r, f.flag("enableShadows"));
    rs.reflection_probe_set_ambient_mode(r, match f.text("ambientMode") {
        "disabled" => ReflectionProbeAmbientMode::DISABLED,
        "color" => ReflectionProbeAmbientMode::COLOR,
        _ => ReflectionProbeAmbientMode::ENVIRONMENT,
    });
    rs.reflection_probe_set_ambient_color(r, color(f.rgba("ambientColor")));
    rs.reflection_probe_set_ambient_energy(r, f.num("ambientColorEnergy"));
    rs.reflection_probe_set_cull_mask(r, f.num("cullMask") as u32);
    rs.reflection_probe_set_reflection_mask(r, f.num("reflectionMask") as u32);
    rs.reflection_probe_set_mesh_lod_threshold(r, f.num("meshLodThreshold"));
    p
}

fn decal(rs: &mut Gd<RenderingServer>, f: &Fields, textures: &[(&'static str, TexData)], keep: &mut Vec<Gd<ImageTexture>>) -> Owned {
    let d = Owned::new(rs.decal_create());
    let r = d.rid();
    rs.decal_set_size(r, v3(f.vec3("size")));
    for (field, slot) in [
        ("textureAlbedo", DecalTexture::ALBEDO),
        ("textureNormal", DecalTexture::NORMAL),
        ("textureOrm", DecalTexture::ORM),
        ("textureEmission", DecalTexture::EMISSION),
    ] {
        if let Some((_, t)) = textures.iter().find(|(k, _)| *k == field) {
            if let Some(tex) = crate::material::image_texture(t.w, t.h, t.rgba) {
                rs.decal_set_texture(r, slot, tex.get_rid());
                keep.push(tex);
            }
        }
    }
    rs.decal_set_emission_energy(r, f.num("emissionEnergy"));
    rs.decal_set_modulate(r, color(f.rgba("modulate")));
    rs.decal_set_albedo_mix(r, f.num("albedoMix"));
    rs.decal_set_normal_fade(r, f.num("normalFade"));
    rs.decal_set_fade(r, f.num("upperFade"), f.num("lowerFade"));
    rs.decal_set_distance_fade(r, f.flag("distanceFadeEnabled"), f.num("distanceFadeBegin"), f.num("distanceFadeLength"));
    rs.decal_set_cull_mask(r, f.num("cullMask") as u32);
    d
}

fn fog_volume(rs: &mut Gd<RenderingServer>, f: &Fields) -> (Owned, Gd<FogMaterial>) {
    let v = Owned::new(rs.fog_volume_create());
    rs.fog_volume_set_shape(v.rid(), match f.text("shape") {
        "ellipsoid" => FogVolumeShape::ELLIPSOID,
        "cone" => FogVolumeShape::CONE,
        "cylinder" => FogVolumeShape::CYLINDER,
        "world" => FogVolumeShape::WORLD,
        _ => FogVolumeShape::BOX,
    });
    rs.fog_volume_set_size(v.rid(), v3(f.vec3("size")));
    let m = FogMaterial::new_gd();
    let mut o = m.clone().upcast::<Object>();
    o.set("density", &f.num("density").to_variant());
    let [r, g, b, a] = f.rgba("albedo");
    o.set("albedo", &Color::from_rgba(r, g, b, a).to_variant());
    let [r, g, b, a] = f.rgba("emission");
    o.set("emission", &Color::from_rgba(r, g, b, a).to_variant());
    o.set("height_falloff", &f.num("heightFalloff").to_variant());
    o.set("edge_fade", &f.num("edgeFade").to_variant());
    rs.fog_volume_set_material(v.rid(), m.get_rid());
    (v, m)
}

impl Volumes {
    pub fn update(&mut self, rs: &mut Gd<RenderingServer>, scenario: Rid, list: &RenderList) {
        let live: std::collections::BTreeSet<ItemKey> = list.volumes.iter().map(|v| v.key).collect();
        self.items.retain(|k, _| live.contains(k));
        for v in &list.volumes {
            if let Some(old) = self.items.get_mut(&v.key) {
                if old.content == v.content {
                    if old.world != v.world {
                        rs.instance_set_transform(old.inst.rid(), xform(&v.world));
                        old.world = v.world;
                    }
                    continue;
                }
            }
            self.items.remove(&v.key);
            let f = fields!(&v.props);
            let mut textures = Vec::new();
            let (base, material) = match v.kind {
                "ReflectionProbe" => (probe(rs, &f), None),
                "Decal" => (decal(rs, &f, &v.textures, &mut textures), None),
                _ => {
                    let (b, m) = fog_volume(rs, &f);
                    (b, Some(m))
                }
            };
            let inst = Owned::new(rs.instance_create2(base.rid(), scenario));
            rs.instance_set_transform(inst.rid(), xform(&v.world));
            self.items.insert(v.key, Vol { inst, _base: base, _textures: textures, _material: material, content: v.content, world: v.world });
        }
    }
}
