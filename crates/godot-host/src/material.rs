//! 材质与贴图(01 §5.1 / §5.2),只在 [gmain]。
//! - sprite_mesh 腿 MeshRenderer:StandardMaterial3D(Lambert、无高光、albedo = entity_tint),由缺省灯逼近 rurix 平直光照;
//! - 贴图 quad(TexQuad / LegacyQuad):ShaderMaterial(BaseMaterial3D 没有洋红色键,01 §5.2 表"洋红色键"行);
//! - 模型:`.mat` / glTF 材质 → StandardMaterial3D(occlusion 与 metallicRoughness 同图且 strength = 1 时 ORMMaterial3D)。
//!
//! 颜色约定:rurix 的 baseColor / emissive 是线性值,BaseMaterial3D 的 albedo / emission 是 sRGB(source_color),
//! 所以传 linear_to_srgb(x);RS 级 ShaderMaterial 的参数不走 source_color(Stage 3:Color 参数会被一律 srgb→linear,
//! material_storage.cpp:801),统一用 Vector4 自己换算。GLES3 场景着色器把 ALBEDO 当 sRGB(scene.glsl:2398),
//! 所以 RS 级着色器按渲染方式分两个变体(shader_set_code 不跑预处理器,不能 `#if`)。

use std::collections::HashMap;

use godot::classes::base_material_3d::{
    CullMode, DiffuseMode, EmissionOperator, Feature, Flags, ShadingMode, SpecularMode, TextureChannel, TextureFilter,
    TextureParam, Transparency,
};
use godot::classes::image::Format;
use godot::classes::{BaseMaterial3D, Image, ImageTexture, OrmMaterial3D, RenderingServer, StandardMaterial3D, Texture2D};
use godot::prelude::*;

use engine_host::{ModelBundle, TexData};

use crate::rid::Owned;

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// 线性 RGB → BaseMaterial3D 的 sRGB Color(alpha 原样)。
pub fn srgb_color(rgb: [f32; 3], a: f32) -> Color {
    Color::from_rgba(linear_to_srgb(rgb[0]), linear_to_srgb(rgb[1]), linear_to_srgb(rgb[2]), a)
}

const SRGB_FNS: &str = r#"
vec3 forge_srgb_to_linear(vec3 c) {
	return mix(pow((c + vec3(0.055)) * (1.0 / 1.055), vec3(2.4)), c * (1.0 / 12.92), lessThan(c, vec3(0.04045)));
}
vec3 forge_linear_to_srgb(vec3 c) {
	return mix(vec3(1.055) * pow(max(c, vec3(0.0)), vec3(1.0 / 2.4)) - vec3(0.055), c * 12.92, lessThan(c, vec3(0.0031308)));
}
"#;

/// rurix 全金属面的环境项 base·0.14·ao 在 Godot 中缺失,补入 EMISSION。
/// 仅补偿实际 metallic == 1 的表面(含 MR 贴图采样后的值):部分金属的直射 BRDF
/// 与 rurix 不同,把 metallic 线性外推为 emission 会扩大偏差(g5_fixes 半金属探针 5→11.5)。
/// 显式 Environment 仍使用 Godot 原生语义。贴图分支保留原 emissive × emissive 贴图。
pub enum MetalEmission {
    Color([f32; 3]),
    Texture { w: u32, h: u32, rgba: Vec<u8>, energy: f32 },
}

pub fn metal_emission(b: &ModelBundle, m: &engine_host::ModelMaterial) -> MetalEmission {
    let metal_ambient = |metal: f32| if metal >= 1.0 { 0.14 } else { 0.0 };
    let tex = |i: Option<usize>| i.and_then(|i| b.textures.get(i)).filter(|t| t.width > 0 && t.height > 0 && t.rgba.len() == (t.width * t.height * 4) as usize);
    let (albedo, mr, ao, em) = (tex(m.base_color_texture), tex(m.metallic_roughness_texture), tex(m.occlusion_texture), tex(m.emissive_texture));
    let base = [m.base_color[0], m.base_color[1], m.base_color[2]];
    let Some(size) = [albedo, mr, ao, em].into_iter().flatten().next().map(|t| (t.width, t.height)) else {
        return MetalEmission::Color(std::array::from_fn(|c| m.emissive[c] + base[c] * metal_ambient(m.metallic)));
    };
    let (w, h) = size;
    let at = |t: &engine_host::ModelTexture, x: u32, y: u32| -> [u8; 4] {
        let tx = ((x as u64 * t.width as u64) / w as u64).min(t.width as u64 - 1) as u32;
        let ty = ((y as u64 * t.height as u64) / h as u64).min(t.height as u64 - 1) as u32;
        let i = ((ty * t.width + tx) * 4) as usize;
        [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]]
    };
    let lin = |v: u8| srgb_to_linear(v as f32 / 255.0);
    let mut vals = Vec::with_capacity((w * h * 3) as usize);
    let mut peak = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            let a = albedo.map(|t| at(t, x, y));
            let metal = m.metallic * mr.map_or(1.0, |t| at(t, x, y)[2] as f32 / 255.0);
            let occ = ao.map_or(1.0, |t| 1.0 + m.occlusion_strength * (at(t, x, y)[0] as f32 / 255.0 - 1.0));
            let e = em.map(|t| at(t, x, y));
            for c in 0..3 {
                let v = base[c] * a.map_or(1.0, |p| lin(p[c])) * metal_ambient(metal) * occ + m.emissive[c] * e.map_or(1.0, |p| lin(p[c]));
                peak = peak.max(v);
                vals.push(v);
            }
        }
    }
    let scale = if peak > 1.0 { 1.0 / peak } else { 1.0 };
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in vals.chunks_exact(3) {
        for v in px {
            rgba.push((linear_to_srgb(v * scale) * 255.0).round().clamp(0.0, 255.0) as u8);
        }
        rgba.push(255);
    }
    MetalEmission::Texture { w, h, rgba, energy: 1.0 / scale }
}

/// 贴图 quad。`legacy = false`:sprite_mesh 腿类 0(FS_TEX_WGSL:alpha ≤ 0 或(非混合且 < 0.02)丢弃、
/// compositing.x 开时洋红色键、输出 texel.rgb × tint,环境 tonemap 为 LINEAR);`legacy = true`:模型腿 legacy_draw
/// (MASK 0.02、恒开色键、rurix 不经 tonemap → 按 Reinhard 逆变换预补偿,环境是 Reinhard 时画面等于 texel)。
pub fn quad_shader(compatibility: bool, legacy: bool) -> String {
    let discard = if legacy {
        "if (forge_tint.a * texel.a < 0.02) { discard; }\n\tif (texel.g < 0.5 * min(texel.r, texel.b)) { discard; }"
    } else {
        "if (texel.a <= 0.0 || (forge_compositing.y < 0.5 && texel.a < 0.02)) { discard; }\n\tif (forge_compositing.x > 0.5 && texel.g < 0.5 * min(texel.r, texel.b)) { discard; }"
    };
    let out = match (legacy, compatibility) {
        (false, false) => "ALBEDO = forge_srgb_to_linear(srgb);",
        (false, true) => "ALBEDO = srgb;",
        // y = 目标的线性值;Reinhard(x) = x / (1 + x) = y → x = y / (1 − y)。
        (true, false) => "vec3 y = min(forge_srgb_to_linear(srgb), vec3(0.9999));\n\tALBEDO = y / (vec3(1.0) - y);",
        (true, true) => "vec3 y = min(forge_srgb_to_linear(srgb), vec3(0.9999));\n\tALBEDO = forge_linear_to_srgb(y / (vec3(1.0) - y));",
    };
    format!(
        r#"shader_type spatial;
render_mode unshaded, cull_disabled;
uniform sampler2D forge_tex : filter_nearest, repeat_disable;
uniform vec4 forge_compositing = vec4(1.0, 0.0, 0.0, 0.0);
uniform vec4 forge_tint = vec4(1.0);
{SRGB_FNS}
void fragment() {{
	vec4 texel = texture(forge_tex, UV);
	{discard}
	vec3 srgb = texel.rgb * forge_tint.rgb;
	{out}
}}
"#
    )
}

/// 模型腿选中高亮(rurix FS:color = mix(color, (1, 0.4, 0.06), 0.25),在 tonemap 之前):
/// 同一几何上的透明叠加层,RD 在线性 HDR 缓冲里混合,等价于 tonemap 前的 mix。
pub fn overlay_shader(compatibility: bool) -> String {
    let albedo = if compatibility { "forge_linear_to_srgb(vec3(1.0, 0.4, 0.06))" } else { "vec3(1.0, 0.4, 0.06)" };
    format!(
        r#"shader_type spatial;
render_mode unshaded, blend_mix, depth_draw_never, cull_disabled;
{SRGB_FNS}
void fragment() {{
	ALBEDO = {albedo};
	ALPHA = 0.25;
}}
"#
    )
}

/// 模型贴图的派生版本:法线贴图烘进 normalScale(rurix 是 xy × scale,Godot 的 normal_scale 是与几何法线之间的 mix,
/// 两者只在 scale = 1 时一致),AO 烘进 occlusionStrength(rurix ao = mix(1, r, strength),BaseMaterial3D 没有这个参数)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TexVariant {
    Raw,
    NormalScaled(f32),
    AoStrength(f32),
}

fn variant_key(v: TexVariant) -> (u8, u32) {
    match v {
        TexVariant::Raw => (0, 0),
        TexVariant::NormalScaled(s) => (1, s.to_bits()),
        TexVariant::AoStrength(s) => (2, s.to_bits()),
    }
}

fn apply_variant(rgba: &mut [u8], v: TexVariant) {
    match v {
        TexVariant::Raw => {}
        TexVariant::NormalScaled(s) => {
            for p in rgba.chunks_exact_mut(4) {
                let f = |b: u8| b as f32 / 255.0 * 2.0 - 1.0;
                let (x, y, z) = (f(p[0]) * s, f(p[1]) * s, f(p[2]));
                let l = (x * x + y * y + z * z).sqrt().max(1e-6);
                let enc = |c: f32| (((c / l) * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
                (p[0], p[1], p[2]) = (enc(x), enc(y), enc(z));
            }
        }
        TexVariant::AoStrength(s) => {
            for p in rgba.chunks_exact_mut(4) {
                let r = p[0] as f32 / 255.0;
                p[0] = ((1.0 + s * (r - 1.0)) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

pub fn image_texture(w: u32, h: u32, rgba: &[u8]) -> Option<Gd<ImageTexture>> {
    if w == 0 || h == 0 || rgba.len() != (w * h * 4) as usize {
        return None;
    }
    let img = Image::create_from_data(w as i32, h as i32, false, Format::RGBA8, &PackedByteArray::from(rgba))?;
    ImageTexture::create_from_image(&img)
}


fn quad_key(tex: &TexData, v: [f32; 4]) -> (String, usize, [u32; 4]) {
    (tex.guid.clone(), tex.rgba.as_ptr() as usize, v.map(f32::to_bits))
}

/// 两个视口共用的材质 / 贴图缓存(键 = 内容身份)。字段按 drop 顺序排:材质先于贴图、贴图先于着色器。
pub struct Materials {
    color: HashMap<[u32; 4], Gd<StandardMaterial3D>>,
    /// 键 = (模型键, 材质指纹, 是否带金属环境项)。
    models: HashMap<(String, u64, bool), Gd<BaseMaterial3D>>,
    quads: HashMap<(bool, String, usize, [u32; 4]), Owned>,
    overlay: Owned,
    quad_tex: HashMap<(String, usize), Gd<ImageTexture>>,
    model_tex: HashMap<(String, usize, (u8, u32)), Gd<ImageTexture>>,
    /// 金属环境项烘焙贴图(键同 models)。
    metal_tex: HashMap<(String, u64), Gd<ImageTexture>>,
    quad_shader: Owned,
    legacy_quad_shader: Owned,
    /// 只为保活(overlay 材质引用它),最后释放。
    _overlay_shader: Owned,
}

impl Materials {
    pub fn new(rs: &mut Gd<RenderingServer>, compatibility: bool) -> Self {
        let mut shader = |code: String| {
            let s = rs.shader_create();
            rs.shader_set_code(s, code.as_str());
            Owned::new(s)
        };
        let quad_s = shader(quad_shader(compatibility, false));
        let legacy_s = shader(quad_shader(compatibility, true));
        let overlay_s = shader(overlay_shader(compatibility));
        let overlay = Owned::new(rs.material_create());
        rs.material_set_shader(overlay.rid(), overlay_s.rid());
        Materials {
            color: HashMap::new(),
            models: HashMap::new(),
            quads: HashMap::new(),
            overlay,
            quad_tex: HashMap::new(),
            model_tex: HashMap::new(),
            metal_tex: HashMap::new(),
            quad_shader: quad_s,
            legacy_quad_shader: legacy_s,
            _overlay_shader: overlay_s,
        }
    }

    /// asset.reload:丢弃按内容缓存的材质与贴图(着色器保留)。
    pub fn clear(&mut self) {
        self.color.clear();
        self.models.clear();
        self.metal_tex.clear();
        self.quads.clear();
        self.quad_tex.clear();
        self.model_tex.clear();
    }

    pub fn overlay(&self) -> Rid {
        self.overlay.rid()
    }

    /// sprite_mesh 腿 MeshRenderer:albedo = entity_tint(rurix 把它当 sRGB 编码值乘光照),Lambert、无高光;
    /// 网格是双份(mesh::flat_mesh two_sided),所以 CULL_BACK 等价于 rurix 的"不剔除、不翻法线"。
    /// 亮度由 light.rs 的 sprite_mesh 缺省灯拟合。
    pub fn color(&mut self, c: [f32; 4]) -> Rid {
        let key = c.map(f32::to_bits);
        let m = self.color.entry(key).or_insert_with(|| {
            let mut m = StandardMaterial3D::new_gd();
            m.set_albedo(Color::from_rgba(c[0], c[1], c[2], c[3]));
            m.set_diffuse_mode(DiffuseMode::LAMBERT);
            m.set_specular_mode(SpecularMode::DISABLED);
            m.set_roughness(1.0);
            m.set_metallic(0.0);
            m.set_cull_mode(CullMode::BACK);
            m
        });
        m.get_rid()
    }

    fn quad_texture(&mut self, tex: &TexData) -> Option<Rid> {
        let key = (tex.guid.clone(), tex.rgba.as_ptr() as usize);
        if !self.quad_tex.contains_key(&key) {
            let t = image_texture(tex.w, tex.h, tex.rgba)?;
            self.quad_tex.insert(key.clone(), t);
        }
        self.quad_tex.get(&key).map(|t| t.get_rid())
    }

    /// 贴图 quad 材质(legacy = 模型腿 legacy_draw 版)。贴图解码失败返回 None(调用方不建实例,draws 如实少 1)。
    pub fn quad(&mut self, rs: &mut Gd<RenderingServer>, tex: &TexData, legacy: bool, v: [f32; 4]) -> Option<Rid> {
        let (g, p, bits) = quad_key(tex, v);
        let key = (legacy, g, p, bits);
        if let Some(m) = self.quads.get(&key) {
            return Some(m.rid());
        }
        let t = self.quad_texture(tex)?;
        let mat = Owned::new(rs.material_create());
        let shader = if legacy { self.legacy_quad_shader.rid() } else { self.quad_shader.rid() };
        rs.material_set_shader(mat.rid(), shader);
        rs.material_set_param(mat.rid(), "forge_tex", &t.to_variant());
        let (name, value) = if legacy { ("forge_tint", v) } else { ("forge_compositing", v) };
        rs.material_set_param(mat.rid(), name, &Vector4::new(value[0], value[1], value[2], value[3]).to_variant());
        let rid = mat.rid();
        self.quads.insert(key, mat);
        Some(rid)
    }
}


impl Materials {
    fn model_texture(&mut self, b: &ModelBundle, key: &str, idx: Option<usize>, v: TexVariant) -> Option<Gd<Texture2D>> {
        let idx = idx?;
        let k = (key.to_string(), idx, variant_key(v));
        if !self.model_tex.contains_key(&k) {
            let t = b.textures.get(idx)?;
            let mut rgba = t.rgba.clone();
            apply_variant(&mut rgba, v);
            let tex = image_texture(t.width, t.height, &rgba)?;
            self.model_tex.insert(k.clone(), tex);
        }
        self.model_tex.get(&k).map(|t| t.clone().upcast())
    }

    /// 模型材质。`rurix_ambient` = 本帧是缺省环境(场景里没有 Environment 组件):metallic > 0 且不是 unlit 时
    /// 另建一份带金属环境项的材质(metal_emission);其余情况与 Stage 4 是同一个 BaseMaterial3D(同一个缓存项)。
    pub fn model(&mut self, bundle_key: &str, b: &ModelBundle, m: &engine_host::ModelMaterial, fp: u64, rurix_ambient: bool) -> Rid {
        let metal = rurix_ambient && m.metallic > 0.0 && !m.unlit;
        self.model_base(bundle_key, b, m, fp, metal)
    }

    fn metal_ambient(&mut self, mat: &mut Gd<BaseMaterial3D>, bundle_key: &str, b: &ModelBundle, m: &engine_host::ModelMaterial, fp: u64) {
        mat.set_feature(Feature::EMISSION, true);
        match metal_emission(b, m) {
            MetalEmission::Color(rgb) => {
                let peak = rgb.iter().copied().fold(0.0f32, f32::max);
                let (rgb, energy) = if peak > 1.0 { (rgb.map(|x| x / peak), peak) } else { (rgb, 1.0) };
                mat.set_emission(srgb_color(rgb, 1.0));
                mat.set_emission_energy_multiplier(energy);
            }
            MetalEmission::Texture { w, h, rgba, energy } => {
                mat.set_emission(Color::from_rgba(1.0, 1.0, 1.0, 1.0));
                mat.set_emission_energy_multiplier(energy);
                mat.set_emission_operator(EmissionOperator::MULTIPLY);
                let key = (bundle_key.to_string(), fp);
                if !self.metal_tex.contains_key(&key) {
                    if let Some(t) = image_texture(w, h, &rgba) {
                        self.metal_tex.insert(key.clone(), t);
                    }
                }
                if let Some(t) = self.metal_tex.get(&key) {
                    mat.set_texture(TextureParam::EMISSION, &t.clone().upcast::<Texture2D>());
                }
            }
        }
    }

    /// ModelMaterial(已按 materialOverrides 覆盖)→ BaseMaterial3D(01 §5.2 表逐行):
    /// albedo = sRGB(baseColor)·贴图(sRGB 解码,与 rurix linear(texel) 同);MR 图 G = roughness、B = metallic;
    /// 法线图烘进 normalScale;AO 只作用环境光(ao_light_affect = 0,rurix 的 ao 只乘 0.14 环境项)并烘进 strength;
    /// emission = sRGB(emissive)·贴图;alphaMode → transparency;doubleSided → cull;unlit → UNSHADED;
    /// 过滤按 glTF sampler(nearest / linear,无 mip,与 rurix sample_tex 相同),clamp 只在所有贴图两轴都 clamp 时用。
    fn model_base(&mut self, bundle_key: &str, b: &ModelBundle, m: &engine_host::ModelMaterial, fp: u64, metal: bool) -> Rid {
        let key = (bundle_key.to_string(), fp, metal);
        if let Some(mat) = self.models.get(&key) {
            return mat.get_rid();
        }
        let orm = m.occlusion_texture.is_some() && m.occlusion_texture == m.metallic_roughness_texture && m.occlusion_strength == 1.0;
        let mut mat: Gd<BaseMaterial3D> =
            if orm { OrmMaterial3D::new_gd().upcast() } else { StandardMaterial3D::new_gd().upcast() };
        mat.set_albedo(srgb_color([m.base_color[0], m.base_color[1], m.base_color[2]], m.base_color[3]));
        mat.set_metallic(m.metallic);
        mat.set_roughness(m.roughness);
        mat.set_diffuse_mode(DiffuseMode::LAMBERT);
        if let Some(t) = self.model_texture(b, bundle_key, m.base_color_texture, TexVariant::Raw) {
            mat.set_texture(TextureParam::ALBEDO, &t);
        }
        if orm {
            if let Some(t) = self.model_texture(b, bundle_key, m.metallic_roughness_texture, TexVariant::Raw) {
                mat.set_texture(TextureParam::ORM, &t);
            }
            mat.set_ao_light_affect(0.0);
        } else {
            if let Some(t) = self.model_texture(b, bundle_key, m.metallic_roughness_texture, TexVariant::Raw) {
                mat.set_texture(TextureParam::METALLIC, &t);
                mat.set_metallic_texture_channel(TextureChannel::BLUE);
                mat.set_texture(TextureParam::ROUGHNESS, &t);
                mat.set_roughness_texture_channel(TextureChannel::GREEN);
            }
            if let Some(t) = self.model_texture(b, bundle_key, m.occlusion_texture, TexVariant::AoStrength(m.occlusion_strength)) {
                mat.set_feature(Feature::AMBIENT_OCCLUSION, true);
                mat.set_texture(TextureParam::AMBIENT_OCCLUSION, &t);
                mat.set_ao_texture_channel(TextureChannel::RED);
                mat.set_ao_light_affect(0.0);
            }
        }
        if let Some(t) = self.model_texture(b, bundle_key, m.normal_texture, TexVariant::NormalScaled(m.normal_scale)) {
            mat.set_feature(Feature::NORMAL_MAPPING, true);
            mat.set_texture(TextureParam::NORMAL, &t);
            mat.set_normal_scale(1.0);
        }
        let e = m.emissive;
        let emissive_tex = self.model_texture(b, bundle_key, m.emissive_texture, TexVariant::Raw);
        if e.iter().any(|x| *x > 0.0) {
            let peak = e.iter().copied().fold(0.0f32, f32::max);
            let (rgb, energy) = if peak > 1.0 { (e.map(|x| x / peak), peak) } else { (e, 1.0) };
            mat.set_feature(Feature::EMISSION, true);
            mat.set_emission(srgb_color(rgb, 1.0));
            mat.set_emission_energy_multiplier(energy);
            if let Some(t) = &emissive_tex {
                mat.set_texture(TextureParam::EMISSION, t);
            }
        }
        match m.alpha_mode.as_str() {
            "MASK" => {
                mat.set_transparency(Transparency::ALPHA_SCISSOR);
                mat.set_alpha_scissor_threshold(m.alpha_cutoff);
            }
            "BLEND" => mat.set_transparency(Transparency::ALPHA),
            _ => mat.set_transparency(Transparency::DISABLED),
        }
        mat.set_cull_mode(if m.double_sided { CullMode::DISABLED } else { CullMode::BACK });
        if m.unlit {
            mat.set_shading_mode(ShadingMode::UNSHADED);
        }
        let used: Vec<usize> = [m.base_color_texture, m.normal_texture, m.metallic_roughness_texture, m.occlusion_texture, m.emissive_texture]
            .into_iter()
            .flatten()
            .collect();
        let first = used.first().and_then(|&i| b.textures.get(i));
        let nearest = first.is_some_and(|t| matches!(t.mag_filter.or(t.min_filter).unwrap_or(9729), 9728 | 9984 | 9986));
        mat.set_texture_filter(if nearest { TextureFilter::NEAREST } else { TextureFilter::LINEAR });
        let clamp_all = !used.is_empty() && used.iter().filter_map(|&i| b.textures.get(i)).all(|t| t.wrap_s == 33071 && t.wrap_t == 33071);
        mat.set_flag(Flags::USE_TEXTURE_REPEAT, !clamp_all);
        if metal {
            self.metal_ambient(&mut mat, bundle_key, b, m, fp);
        }
        let rid = mat.get_rid();
        self.models.insert(key, mat);
        rid
    }

    /// 模型腿里的旧 MeshRenderer:rurix legacy_draw 用 default_material()(engine_host::default_material,同一份值;
    /// metallic = 0,所以不需要金属环境项补丁)。
    pub fn legacy_mesh(&mut self) -> Rid {
        let m = engine_host::default_material();
        self.model_base("legacy-default", &EMPTY_BUNDLE, &m, 0, false)
    }
}

static EMPTY_BUNDLE: std::sync::LazyLock<ModelBundle> = std::sync::LazyLock::new(|| ModelBundle {
    version: 1,
    guid: String::new(),
    revision: 0,
    name: String::new(),
    source_id: String::new(),
    source_hash: String::new(),
    kind: String::new(),
    roots: vec![],
    primitives: vec![],
    nodes: vec![],
    materials: vec![],
    textures: vec![],
    skins: vec![],
    animations: vec![],
    idle_clip: String::new(),
    walk_clip: String::new(),
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metal_ambient_is_limited_to_full_metal_and_preserves_emission() {
        let mut m = engine_host::default_material();
        m.base_color = [0.8, 0.4, 0.2, 1.0];
        m.emissive = [0.1, 0.2, 0.3];
        for metal in [0.0, 0.25, 0.5, 0.75, 1.0] {
            m.metallic = metal;
            let MetalEmission::Color(actual) = metal_emission(&EMPTY_BUNDLE, &m) else { panic!("untextured material") };
            for c in 0..3 {
                let expected = m.emissive[c] + if metal == 1.0 { m.base_color[c] * 0.14 } else { 0.0 };
                assert!((actual[c] - expected).abs() < 1e-6);
            }
        }
    }
}

