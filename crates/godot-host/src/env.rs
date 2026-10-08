//! Stage 5 场景级设置(01 §6、02 §9.5 Stage 5),只在 [gmain]。
//! - Environment 组件 → `Environment` 资源(不挂节点,只用 get_rid;缺省 = Godot 缺省),按表 `Object::set` Godot 属性名;
//!   没有 Environment 组件时仍用 Slot 自己的 RS environment + light::configure_environment(Stage 4,逐字节不变)。
//! - CameraAttributes → `CameraAttributesPractical` 资源,scenario_set_camera_attributes。
//! - RenderSettings → 视口级 viewport_set_*;RS 全局质量设置只跟 Main 通道,组件消失时恢复 ProjectSettings 值。

use std::sync::atomic::{AtomicBool, Ordering};

use godot::classes::rendering_server::{
    EnvironmentSdfgiRayCount, EnvironmentSsaoQuality, EnvironmentSsilQuality, ViewportMsaa, ViewportScaling3DMode,
    ViewportScreenSpaceAa,
};
use godot::classes::{
    CameraAttributesPractical, Environment, ImageTexture, Material, Object, PanoramaSkyMaterial, PhysicalSkyMaterial,
    ProceduralSkyMaterial, ProjectSettings, RenderingServer, Sky,
};
use godot::prelude::*;

use engine_host::{RenderList, TexData};

use crate::material::{linear_to_srgb, srgb_to_linear};
use crate::props::{fields, Fields};

#[derive(Clone, Copy)]
pub enum K {
    F,
    I,
    B,
    C,
    V3,
    E(&'static [&'static str]),
}

/// Environment:forge 字段 → Godot 属性(background / sky / LUT 另行处理)。枚举顺序与 Godot 枚举一致。
pub const ENV: &[(&str, &str, K)] = &[
    ("backgroundEnergy", "background_energy_multiplier", K::F),
    ("skyRotation", "sky_rotation", K::V3),
    ("ambientSource", "ambient_light_source", K::E(&["bg", "disabled", "color", "sky"])),
    ("ambientColor", "ambient_light_color", K::C),
    ("ambientEnergy", "ambient_light_energy", K::F),
    ("ambientSkyContribution", "ambient_light_sky_contribution", K::F),
    ("reflectionSource", "reflected_light_source", K::E(&["bg", "disabled", "sky"])),
    ("tonemap", "tonemap_mode", K::E(&["linear", "reinhard", "filmic", "aces", "agx"])),
    ("exposure", "tonemap_exposure", K::F),
    ("white", "tonemap_white", K::F),
    ("agxContrast", "tonemap_agx_contrast", K::F),
    ("agxWhite", "tonemap_agx_white", K::F),
    ("glowEnabled", "glow_enabled", K::B),
    ("glowLevel1", "glow_levels/1", K::F),
    ("glowLevel2", "glow_levels/2", K::F),
    ("glowLevel3", "glow_levels/3", K::F),
    ("glowLevel4", "glow_levels/4", K::F),
    ("glowLevel5", "glow_levels/5", K::F),
    ("glowLevel6", "glow_levels/6", K::F),
    ("glowLevel7", "glow_levels/7", K::F),
    ("glowNormalized", "glow_normalized", K::B),
    ("glowIntensity", "glow_intensity", K::F),
    ("glowStrength", "glow_strength", K::F),
    ("glowMix", "glow_mix", K::F),
    ("glowBloom", "glow_bloom", K::F),
    ("glowBlendMode", "glow_blend_mode", K::E(&["additive", "screen", "softlight", "replace", "mix"])),
    ("glowHdrThreshold", "glow_hdr_threshold", K::F),
    ("glowHdrScale", "glow_hdr_scale", K::F),
    ("glowHdrLuminanceCap", "glow_hdr_luminance_cap", K::F),
    ("ssaoEnabled", "ssao_enabled", K::B),
    ("ssaoRadius", "ssao_radius", K::F),
    ("ssaoIntensity", "ssao_intensity", K::F),
    ("ssaoPower", "ssao_power", K::F),
    ("ssaoDetail", "ssao_detail", K::F),
    ("ssaoHorizon", "ssao_horizon", K::F),
    ("ssaoSharpness", "ssao_sharpness", K::F),
    ("ssaoLightAffect", "ssao_light_affect", K::F),
    ("ssaoAoChannelAffect", "ssao_ao_channel_affect", K::F),
    ("ssilEnabled", "ssil_enabled", K::B),
    ("ssilRadius", "ssil_radius", K::F),
    ("ssilIntensity", "ssil_intensity", K::F),
    ("ssilSharpness", "ssil_sharpness", K::F),
    ("ssilNormalRejection", "ssil_normal_rejection", K::F),
    ("ssrEnabled", "ssr_enabled", K::B),
    ("ssrMaxSteps", "ssr_max_steps", K::I),
    ("ssrFadeIn", "ssr_fade_in", K::F),
    ("ssrFadeOut", "ssr_fade_out", K::F),
    ("ssrDepthTolerance", "ssr_depth_tolerance", K::F),
    ("sdfgiEnabled", "sdfgi_enabled", K::B),
    ("sdfgiCascades", "sdfgi_cascades", K::I),
    ("sdfgiMinCellSize", "sdfgi_min_cell_size", K::F),
    ("sdfgiYScale", "sdfgi_y_scale", K::E(&["50%", "75%", "100%"])),
    ("sdfgiUseOcclusion", "sdfgi_use_occlusion", K::B),
    ("sdfgiBounceFeedback", "sdfgi_bounce_feedback", K::F),
    ("sdfgiReadSkyLight", "sdfgi_read_sky_light", K::B),
    ("sdfgiEnergy", "sdfgi_energy", K::F),
    ("sdfgiNormalBias", "sdfgi_normal_bias", K::F),
    ("sdfgiProbeBias", "sdfgi_probe_bias", K::F),
    ("fogEnabled", "fog_enabled", K::B),
    ("fogMode", "fog_mode", K::E(&["exponential", "depth"])),
    ("fogLightColor", "fog_light_color", K::C),
    ("fogLightEnergy", "fog_light_energy", K::F),
    ("fogSunScatter", "fog_sun_scatter", K::F),
    ("fogDensity", "fog_density", K::F),
    ("fogAerialPerspective", "fog_aerial_perspective", K::F),
    ("fogSkyAffect", "fog_sky_affect", K::F),
    ("fogHeight", "fog_height", K::F),
    ("fogHeightDensity", "fog_height_density", K::F),
    ("fogDepthCurve", "fog_depth_curve", K::F),
    ("fogDepthBegin", "fog_depth_begin", K::F),
    ("fogDepthEnd", "fog_depth_end", K::F),
    ("volumetricFogEnabled", "volumetric_fog_enabled", K::B),
    ("volumetricFogDensity", "volumetric_fog_density", K::F),
    ("volumetricFogAlbedo", "volumetric_fog_albedo", K::C),
    ("volumetricFogEmission", "volumetric_fog_emission", K::C),
    ("volumetricFogEmissionEnergy", "volumetric_fog_emission_energy", K::F),
    ("volumetricFogAnisotropy", "volumetric_fog_anisotropy", K::F),
    ("volumetricFogLength", "volumetric_fog_length", K::F),
    ("volumetricFogDetailSpread", "volumetric_fog_detail_spread", K::F),
    ("volumetricFogGiInject", "volumetric_fog_gi_inject", K::F),
    ("volumetricFogAmbientInject", "volumetric_fog_ambient_inject", K::F),
    ("volumetricFogSkyAffect", "volumetric_fog_sky_affect", K::F),
    ("volumetricFogTemporalReprojection", "volumetric_fog_temporal_reprojection_enabled", K::B),
    ("volumetricFogTemporalReprojectionAmount", "volumetric_fog_temporal_reprojection_amount", K::F),
    ("adjustmentEnabled", "adjustment_enabled", K::B),
    ("adjustmentBrightness", "adjustment_brightness", K::F),
    ("adjustmentContrast", "adjustment_contrast", K::F),
    ("adjustmentSaturation", "adjustment_saturation", K::F),
];

/// CameraAttributes → CameraAttributesPractical 属性。
pub const CAM: &[(&str, &str, K)] = &[
    ("exposureMultiplier", "exposure_multiplier", K::F),
    ("exposureSensitivity", "exposure_sensitivity", K::F),
    ("autoExposureEnabled", "auto_exposure_enabled", K::B),
    ("autoExposureScale", "auto_exposure_scale", K::F),
    ("autoExposureSpeed", "auto_exposure_speed", K::F),
    ("autoExposureMinSensitivity", "auto_exposure_min_sensitivity", K::F),
    ("autoExposureMaxSensitivity", "auto_exposure_max_sensitivity", K::F),
    ("dofBlurFarEnabled", "dof_blur_far_enabled", K::B),
    ("dofBlurFarDistance", "dof_blur_far_distance", K::F),
    ("dofBlurFarTransition", "dof_blur_far_transition", K::F),
    ("dofBlurNearEnabled", "dof_blur_near_enabled", K::B),
    ("dofBlurNearDistance", "dof_blur_near_distance", K::F),
    ("dofBlurNearTransition", "dof_blur_near_transition", K::F),
    ("dofBlurAmount", "dof_blur_amount", K::F),
];

pub fn value(f: &Fields, k: &str, kind: K) -> Variant {
    match kind {
        K::F => f.num(k).to_variant(),
        K::I => (f.num(k).round() as i64).to_variant(),
        K::B => f.flag(k).to_variant(),
        K::C => {
            let c = f.rgba(k);
            Color::from_rgba(c[0], c[1], c[2], c[3]).to_variant()
        }
        K::V3 => {
            let v = f.vec3(k);
            Vector3::new(v[0], v[1], v[2]).to_variant()
        }
        K::E(values) => f.index(k, values, 0).to_variant(),
    }
}

/// 按表设属性。第一次用某张表时顺带检查属性名(拼错的属性 `Object::set` 会静默忽略),不认识的打一行 stderr。
pub fn set_all(obj: &mut Gd<Object>, f: &Fields, table: &[(&str, &str, K)], checked: &AtomicBool) {
    if !checked.swap(true, Ordering::SeqCst) {
        for (_, prop, _) in table {
            if obj.get(*prop).is_nil() {
                eprintln!("godot-host: 未知属性 {}.{prop}", obj.get_class());
            }
        }
    }
    for (k, prop, kind) in table {
        obj.set(*prop, &value(f, k, *kind));
    }
}

fn texture(textures: &[(&'static str, TexData)], field: &str) -> Option<Gd<ImageTexture>> {
    let (_, t) = textures.iter().find(|(f, _)| *f == field)?;
    crate::material::image_texture(t.w, t.h, t.rgba)
}

/// Godot 4.7 的 Reinhard:x·(1 + x/w²)/(1 + x)(x ≥ 0 单调),二分求逆。
fn invert_reinhard(y: f32, white: f32) -> f32 {
    let w2 = (white * white).max(1e-6);
    let f = |x: f32| x * (1.0 + x / w2) / (1.0 + x);
    let (mut lo, mut hi) = (0.0f32, 1.0e4f32);
    for _ in 0..64 {
        let mid = 0.5 * (lo + hi);
        if f(mid) < y {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    0.5 * (lo + hi)
}

/// background = clearColor:让出帧的背景等于当前腿的清屏色(8 bit 目标值)。linear / reinhard 按曝光与 tonemap 求逆,
/// 其余 tonemap(filmic / aces / agx)原样传入,背景会被该 tonemap 改变(02 §9.5 Stage 5 已写明)。
fn clear_bg(clear: [f32; 4], f: &Fields) -> Color {
    let tm = f.text("tonemap");
    let exposure = f.num("exposure").max(1e-6);
    let white = f.num("white");
    let inv = |v: f32| -> f32 {
        let y = srgb_to_linear(v).min(0.9999);
        let x = match tm {
            "linear" => y,
            "reinhard" => invert_reinhard(y, white),
            _ => return v,
        };
        linear_to_srgb(x / exposure)
    };
    Color::from_rgba(inv(clear[0]), inv(clear[1]), inv(clear[2]), clear[3])
}
fn sky(f: &Fields, textures: &[(&'static str, TexData)], keep: &mut Vec<Gd<ImageTexture>>) -> Gd<Sky> {
    let energy = f.num("skyEnergy").to_variant();
    let mat: Gd<Material> = match f.text("skyType") {
        "physical" => {
            let mut m = PhysicalSkyMaterial::new_gd();
            m.set("energy_multiplier", &energy);
            m.upcast()
        }
        "panorama" => {
            let mut m = PanoramaSkyMaterial::new_gd();
            m.set("energy_multiplier", &energy);
            if let Some(t) = texture(textures, "skyTexture") {
                m.set("panorama", &t.to_variant());
                keep.push(t);
            }
            m.upcast()
        }
        _ => {
            let mut m = ProceduralSkyMaterial::new_gd();
            m.set("energy_multiplier", &energy);
            m.upcast()
        }
    };
    let mut s = Sky::new_gd();
    s.set("sky_material", &mat.to_variant());
    s
}

static ENV_CHECKED: AtomicBool = AtomicBool::new(false);
static CAM_CHECKED: AtomicBool = AtomicBool::new(false);

/// Undo the 4.7.2 RD fog-only sky's extra sRGB decode and background-energy
/// multiplication. With S = sRGB->linear, the normal clear path is S(c*e),
/// while fog draws S(S(c*e))*e. Its inverse is L(L(S(c*e)/e))/e.
/// Restrict this workaround to RD color backgrounds; sky materials are unchanged.
/// GLES has its own approximation and compensation below. Zero energy stays black.
fn fog_background(color: Color, energy: f32) -> Color {
    if energy <= 0.0 {
        return Color::from_rgba(0.0, 0.0, 0.0, color.a);
    }
    let channel = |c: f32| linear_to_srgb(linear_to_srgb(srgb_to_linear(c * energy) / energy)) / energy;
    Color::from_rgba(channel(color.r), channel(color.g), channel(color.b), color.a)
}

/// GLES uses a cubic sRGB approximation in sky.glsl, applies energy twice,
/// and tone-maps the fog sky whereas its plain clear goes straight to the framebuffer.
/// Invert those operations for the two tone maps whose inverses we implement.
fn gles_fog_background(color: Color, energy: f32, f: &Fields) -> Color {
    let tm = f.text("tonemap");
    if !matches!(tm, "linear" | "reinhard") { return color; }
    if energy <= 0.0 { return Color::from_rgba(0.0, 0.0, 0.0, color.a); }
    let exposure = f.num("exposure");
    if exposure <= 0.0 { return color; }
    let channel = |c: f32| {
        let target = (c * energy).max(0.0);
        if target == 0.0 { return 0.0; }
        let y = ((target + 0.055) / 1.055).powf(2.4);
        let linear = if tm == "reinhard" { invert_reinhard(y, f.num("white")) } else { y } / exposure;
        let polynomial = |x: f32| x * (x * (x * 0.305306011 + 0.682171111) + 0.012522878);
        let (mut lo, mut hi) = (0.0, linear.max(1.0) * 4.0);
        for _ in 0..48 {
            let mid = (lo + hi) * 0.5;
            if polynomial(mid) < linear { lo = mid; } else { hi = mid; }
        }
        (lo + hi) * 0.5 / (energy * energy)
    };
    Color::from_rgba(channel(color.r), channel(color.g), channel(color.b), color.a)
}

fn environment(f: &Fields, clear: [f32; 4], textures: &[(&'static str, TexData)], keep: &mut Vec<Gd<ImageTexture>>,
    method: &str, camera_exposure: f32) -> Gd<Environment> {
    let env = Environment::new_gd();
    let mut o = env.clone().upcast::<Object>();
    set_all(&mut o, f, ENV, &ENV_CHECKED);
    // background_mode:BG_COLOR = 1、BG_SKY = 2(clearColor 也走 BG_COLOR,颜色 = 腿清屏色的逆变换)。
    let bg = f.text("background");
    match bg {
        "sky" => o.set("background_mode", &2i64.to_variant()),
        "color" => {
            o.set("background_mode", &1i64.to_variant());
            o.set("background_color", &value(f, "backgroundColor", K::C));
        }
        _ => {
            o.set("background_mode", &1i64.to_variant());
            o.set("background_color", &clear_bg(clear, f).to_variant());
        }
    }
    if bg != "sky"
        && (f.flag("fogEnabled") || (method == "forward_plus" && f.flag("volumetricFogEnabled")))
    {
        let color = o.get("background_color").to::<Color>();
        let energy = f.num("backgroundEnergy") * camera_exposure;
        let corrected = if method == "gl_compatibility" {
            gles_fog_background(color, energy, f)
        } else {
            fog_background(color, energy)
        };
        o.set("background_color", &corrected.to_variant());
    }
    if bg == "sky" || f.text("ambientSource") == "sky" || f.text("reflectionSource") == "sky" {
        o.set("sky", &sky(f, textures, keep).to_variant());
    }
    // Texture2D 按 1D LUT 用(Environment::set_adjustment_color_correction);3D LUT 本版不接。
    if let Some(t) = texture(textures, "adjustmentColorCorrection") {
        o.set("adjustment_color_correction", &t.to_variant());
        keep.push(t);
    }
    env
}

fn camera_attributes(f: &Fields) -> Gd<CameraAttributesPractical> {
    let ca = CameraAttributesPractical::new_gd();
    set_all(&mut ca.clone().upcast::<Object>(), f, CAM, &CAM_CHECKED);
    ca
}

/// RenderSettings 的视口级部分;None = 恢复 RS 视口的缺省值(与 Stage 4 从不设置时相同)。
fn viewport_settings(rs: &mut Gd<RenderingServer>, vp: Rid, f: Option<&Fields>) {
    let pick = |k: &str, values: &[&str]| f.map_or(0, |f| f.index(k, values, 0));
    rs.viewport_set_msaa_3d(vp, match pick("msaa3d", &["disabled", "2x", "4x", "8x"]) {
        1 => ViewportMsaa::MSAA_2X,
        2 => ViewportMsaa::MSAA_4X,
        3 => ViewportMsaa::MSAA_8X,
        _ => ViewportMsaa::DISABLED,
    });
    rs.viewport_set_screen_space_aa(vp, match pick("screenSpaceAA", &["disabled", "fxaa", "smaa"]) {
        1 => ViewportScreenSpaceAa::FXAA,
        2 => ViewportScreenSpaceAa::SMAA,
        _ => ViewportScreenSpaceAa::DISABLED,
    });
    rs.viewport_set_use_taa(vp, f.is_some_and(|f| f.flag("taa")));
    rs.viewport_set_use_debanding(vp, f.is_some_and(|f| f.flag("debanding")));
    rs.viewport_set_scaling_3d_mode(vp, match pick("scaling3dMode", &["bilinear", "fsr", "fsr2"]) {
        1 => ViewportScaling3DMode::FSR,
        2 => ViewportScaling3DMode::FSR2,
        _ => ViewportScaling3DMode::BILINEAR,
    });
    rs.viewport_set_scaling_3d_scale(vp, f.map_or(1.0, |f| f.num("scaling3dScale")));
    rs.viewport_set_fsr_sharpness(vp, f.map_or(0.2, |f| f.num("fsrSharpness")));
}

const QUALITIES: [&str; 5] = ["veryLow", "low", "medium", "high", "ultra"];

/// RenderSettings 的 RS 全局部分(只跟 Main 通道);None = ProjectSettings 的值(引擎启动时就是这些)。
fn global_settings(rs: &mut Gd<RenderingServer>, f: Option<&Fields>) {
    let ps = ProjectSettings::singleton();
    let get = |k: &str| ps.get_setting(k);
    let b = |k: &str| get(k).try_to::<bool>().unwrap_or(true);
    let x = |k: &str| get(k).try_to::<f64>().unwrap_or(0.0) as f32;
    let i = |k: &str| get(k).try_to::<i64>().unwrap_or(0);
    let q = |k: &str, proj: &str| f.map_or(i(proj), |f| f.index(k, &QUALITIES, 2)) as i32;
    let ssao = q("ssaoQuality", "rendering/environment/ssao/quality");
    let ssil = q("ssilQuality", "rendering/environment/ssil/quality");
    let p = "rendering/environment/ssao/";
    rs.environment_set_ssao_quality(EnvironmentSsaoQuality::from_ord(ssao), b(&format!("{p}half_size")),
        x(&format!("{p}adaptive_target")), i(&format!("{p}blur_passes")) as i32, x(&format!("{p}fadeout_from")), x(&format!("{p}fadeout_to")));
    let p = "rendering/environment/ssil/";
    rs.environment_set_ssil_quality(EnvironmentSsilQuality::from_ord(ssil), b(&format!("{p}half_size")),
        x(&format!("{p}adaptive_target")), i(&format!("{p}blur_passes")) as i32, x(&format!("{p}fadeout_from")), x(&format!("{p}fadeout_to")));
    let rays = f.map_or(i("rendering/global_illumination/sdfgi/probe_ray_count"), |f| {
        f.index("sdfgiRayCount", &["4", "8", "16", "32", "64", "96", "128"], 1)
    });
    rs.environment_set_sdfgi_ray_count(EnvironmentSdfgiRayCount::from_ord(rays as i32));
    let (size, depth) = match f {
        Some(f) => (f.num("volumetricFogVolumeSize") as i32, f.num("volumetricFogVolumeDepth") as i32),
        None => (i("rendering/environment/volumetric_fog/volume_size") as i32, i("rendering/environment/volumetric_fog/volume_depth") as i32),
    };
    rs.environment_set_volumetric_fog_volume_size(size, depth);
}
/// 一个 Slot 的 RS 目标。
#[derive(Clone, Copy)]
pub struct Targets {
    pub scenario: Rid,
    /// 画 3D 的视口(Mobile 下是内层 HDR 视口)。
    pub vp3d: Rid,
    /// Slot 自己的 RS environment(Stage 4 的缺省环境)。
    pub raw_env: Rid,
}

/// 一个 Slot 的 Stage 5 场景级状态。字段 drop 时释放各自的资源(scenario 已先改指向或已释放)。
#[derive(Default)]
pub struct EnvState {
    custom: Option<Gd<Environment>>,
    keep: Vec<Gd<ImageTexture>>,
    camera_attributes: Option<Gd<CameraAttributesPractical>>,
    viewport_applied: bool,
    globals_applied: bool,
}

impl EnvState {
    /// 场景里有 Environment 组件(用它的资源);false = Stage 4 的缺省环境。
    pub fn custom(&self) -> bool {
        self.custom.is_some()
    }

    /// RenderDelta.env 下发时(全量帧恒下发)按 `list.env` 整体重配。没有任何 Stage 5 组件、之前也没有时一个 RS 调用都不发。
    pub fn apply(&mut self, rs: &mut Gd<RenderingServer>, t: Targets, list: &RenderList, main: bool) {
        match list.env.environment.as_ref() {
            Some(p) => {
                let f = fields!(p);
                let mut keep = Vec::new();
                // Runtime project settings leave physical light units disabled;
                // CameraAttributesPractical then normalizes exposure by its multiplier only.
                let exposure = list.env.camera_attributes.as_ref().map_or(1.0, |p| p.num("exposureMultiplier"));
                let method = rs.get_current_rendering_method().to_string();
                let env = environment(&f, list.clear_rgba, &list.env.textures, &mut keep, &method, exposure);
                rs.scenario_set_environment(t.scenario, env.get_rid());
                self.custom = Some(env);
                self.keep = keep;
            }
            None => {
                if let Some(old) = self.custom.take() {
                    rs.scenario_set_environment(t.scenario, t.raw_env);
                    drop(old);
                    self.keep.clear();
                }
            }
        }
        match list.env.camera_attributes.as_ref() {
            Some(p) => {
                let ca = camera_attributes(&fields!(p));
                rs.scenario_set_camera_attributes(t.scenario, ca.get_rid());
                self.camera_attributes = Some(ca);
            }
            None => {
                if let Some(old) = self.camera_attributes.take() {
                    rs.scenario_set_camera_attributes(t.scenario, Rid::Invalid);
                    drop(old);
                }
            }
        }
        match list.env.render_settings.as_ref() {
            Some(p) => {
                let f = fields!(p);
                viewport_settings(rs, t.vp3d, Some(&f));
                self.viewport_applied = true;
                if main {
                    global_settings(rs, Some(&f));
                    self.globals_applied = true;
                }
            }
            None => {
                if std::mem::take(&mut self.viewport_applied) {
                    viewport_settings(rs, t.vp3d, None);
                }
                if main && std::mem::take(&mut self.globals_applied) {
                    global_settings(rs, None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rd_fog_compensation_inverts_double_decode_and_energy() {
        for energy in [0.25, 0.5, 1.0, 2.0, 4.0] {
            for c in [0.0, 0.01, 0.035, 0.12, 0.4, 0.9] {
                let result = fog_background(Color::from_rgba(c, c, c, 0.5), energy);
                let actual = srgb_to_linear(srgb_to_linear(result.r * energy)) * energy;
                let expected = srgb_to_linear(c * energy);
                assert!((actual - expected).abs() <= 1e-5 * expected.max(1.0), "c={c} energy={energy}");
                assert_eq!(result.a, 0.5);
            }
        }
        let black = fog_background(Color::from_rgba(0.4, 0.2, 0.1, 1.0), 0.0);
        assert_eq!((black.r, black.g, black.b), (0.0, 0.0, 0.0));
    }
}
