//! 灯光与环境(01 §5.3、§6.1),只在 [gmain]。
//! rurix 不读 Light 组件,两条腿各有写死的光照(02 §3.7):
//! - sprite_mesh 腿(viewport/rurix.rs FS_WGSL):out_srgb = color_srgb × (0.28 + 0.72·ndl),L = normalize(0.45, 0.75, 0.35),无 tonemap;
//! - 模型腿(modelrender/rurix.rs FS):c = base·0.14·ao + (diffuse + spec)·nl·3 + emission,l = normalize(0.45, 0.8, 0.35),
//!   再 Reinhard c/(1+c)、pow(1/2.2)。
//!
//! 场景里没有启用的 Light 时,按 leg 生成缺省灯(方向光 + COLOR 环境光 + tonemap)逼近上面的结果;有 Light 实体时按表映射,
//! energy = k·intensity(k = 该腿缺省灯的标定能量),环境光与 tonemap 仍按 leg(Stage 5 的环境 schema 之前的约定)。
//! 标定值与误差见 01 §5.3(g4_light 测试给出数字)。

use godot::classes::rendering_server::{
    EnvironmentAmbientSource, EnvironmentReflectionSource, EnvironmentToneMapper, LightDirectionalShadowMode, LightOmniShadowMode,
    LightParam,
};
use godot::classes::RenderingServer;
use godot::prelude::*;

use engine_host::{Leg, LightItem, LightKind};

use crate::material::{linear_to_srgb, srgb_color, srgb_to_linear};
use crate::props::{fields, Fields};
use crate::rid::Owned;

/// 一条腿的缺省灯。`toward_light` = 从表面指向光源的方向(rurix 的 l;Godot 方向光沿自身 −Z 照射,−Z = −toward_light)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rig {
    pub toward_light: [f32; 3],
    /// Godot 非物理单位:漫反射 = albedo·energy·NdotL(light_storage.cpp 把 energy 乘 π,Lambert BRDF 再除 π)。
    pub energy: f32,
    /// COLOR 环境光(白色 × ambient):环境项 = albedo·ambient。
    pub ambient: f32,
    pub reinhard: bool,
}

/// sprite_mesh 腿:rurix 的 color·(0.28 + 0.72·ndl) 是在 sRGB 编码值上相乘,线性 Lambert 无法处处相等;
/// 这里让 forge 场景里最常见的轴向面(cube 的六个面)逐面相等:目标线性比 t = lin(c·s)/lin(c)(c 取调色板中值 0.7),
/// ambient = t(ndl = 0),各正轴面 energy·L'ᵢ = t(sᵢ) − ambient ⇒ L' 与 energy 由三个正轴面解出。
pub const SPRITE_MESH_RIG: Rig =
    Rig { toward_light: [0.396_07, 0.875_49, 0.276_68], energy: 0.722_87, ambient: 0.071_14, reinhard: false };

/// 模型腿:方向与 rurix 相同;energy = 3/π·(1 − F₀) 起步(rurix 漫反射 (1−F)(1−metal)·base/π·nl·3),ambient = 0.14,
/// Reinhard(white 取很大,4.7 的公式 color·(1 + color/white²)/(1 + color) → color/(1 + color))。
pub const MODEL_RIG: Rig = Rig { toward_light: [0.45, 0.8, 0.35], energy: 0.9167, ambient: 0.14, reinhard: true };

pub const REINHARD_WHITE: f32 = 1000.0;

pub fn rig(leg: Leg) -> Rig {
    match leg {
        Leg::Model => MODEL_RIG,
        _ => SPRITE_MESH_RIG,
    }
}

fn norm(v: [f32; 3]) -> Vector3 {
    Vector3::new(v[0], v[1], v[2]).normalized()
}

/// 清屏色(8 bit 目标 = rurix 写进 UNORM 的值)→ Environment 背景色(sRGB)。Reinhard 腿先做逆变换,出帧后等于目标。
pub fn bg_color(leg: Leg, c: [f32; 4]) -> Color {
    if !rig(leg).reinhard {
        return Color::from_rgba(c[0], c[1], c[2], c[3]);
    }
    let inv = |v: f32| {
        let y = srgb_to_linear(v).min(0.9999);
        linear_to_srgb(y / (1.0 - y))
    };
    Color::from_rgba(inv(c[0]), inv(c[1]), inv(c[2]), c[3])
}

/// 按 leg 配环境:背景 COLOR、环境光 COLOR(白 × ambient,天空贡献 0、反射源关)、tonemap。
pub fn configure_environment(rs: &mut Gd<RenderingServer>, env: Rid, leg: Leg) {
    let r = rig(leg);
    rs.environment_set_ambient_light_ex(env, Color::from_rgba(1.0, 1.0, 1.0, 1.0))
        .ambient(EnvironmentAmbientSource::COLOR)
        .energy(r.ambient)
        .sky_contribution(0.0)
        .reflection_source(EnvironmentReflectionSource::DISABLED)
        .done();
    if r.reinhard {
        rs.environment_set_tonemap(env, EnvironmentToneMapper::REINHARD, 1.0, REINHARD_WHITE);
    } else {
        rs.environment_set_tonemap(env, EnvironmentToneMapper::LINEAR, 1.0, 1.0);
    }
}

/// 一盏灯:light RID + scenario 里的 instance(instance 先释放)。
pub struct LightInst {
    _inst: Owned,
    _light: Owned,
}

/// M4(行主序、列向量)→ Transform3D(与 scene::xform 同式)。
fn xform(m: &engine_host::M4) -> Transform3D {
    let col = |c: usize| Vector3::new(m[0][c], m[1][c], m[2][c]);
    Transform3D::new(Basis::from_cols(col(0), col(1), col(2)), col(3))
}

fn place(rs: &mut Gd<RenderingServer>, scenario: Rid, light: Owned, t: Transform3D) -> LightInst {
    let inst = rs.instance_create2(light.rid(), scenario);
    rs.instance_set_transform(inst, t);
    LightInst { _inst: Owned::new(inst), _light: light }
}

/// RS 层的灯参数缺省值与节点不同(light_storage.cpp:145-165:RANGE 1、SPECULAR 0.5、SHADOW_MAX_DISTANCE 0 → 方向光阴影
/// 根本不画)。这里按 Godot 节点的缺省值补齐(scene/3d/light_3d.cpp:489-511 Light3D、:614-619 DirectionalLight3D、
/// SpotLight3D 的 bias 0.03);SPECULAR 统一 1.0(DirectionalLight3D 的缺省,也与 rurix 满额高光一致)。
fn node_defaults(rs: &mut Gd<RenderingServer>, l: Rid, kind: LightKind) {
    let set = |rs: &mut Gd<RenderingServer>, p: LightParam, v: f32| rs.light_set_param(l, p, v);
    set(rs, LightParam::SPECULAR, 1.0);
    set(rs, LightParam::RANGE, 5.0);
    set(rs, LightParam::ATTENUATION, 1.0);
    set(rs, LightParam::SPOT_ANGLE, 45.0);
    set(rs, LightParam::SPOT_ATTENUATION, 1.0);
    set(rs, LightParam::SHADOW_SPLIT_1_OFFSET, 0.1);
    set(rs, LightParam::SHADOW_SPLIT_2_OFFSET, 0.2);
    set(rs, LightParam::SHADOW_SPLIT_3_OFFSET, 0.5);
    set(rs, LightParam::SHADOW_FADE_START, 0.8);
    set(rs, LightParam::SHADOW_PANCAKE_SIZE, 20.0);
    set(rs, LightParam::SHADOW_OPACITY, 1.0);
    set(rs, LightParam::SHADOW_BLUR, 1.0);
    set(rs, LightParam::SHADOW_BIAS, 0.1);
    set(rs, LightParam::SHADOW_NORMAL_BIAS, 1.0);
    match kind {
        LightKind::Directional => {
            set(rs, LightParam::SHADOW_MAX_DISTANCE, 100.0);
            set(rs, LightParam::SHADOW_NORMAL_BIAS, 2.0);
        }
        LightKind::Spot => set(rs, LightParam::SHADOW_BIAS, 0.03),
        LightKind::Point => {}
    }
}

/// 生成这一帧的灯。`lights` 为空 → 该腿缺省方向光(不投影);否则逐盏映射(01 §5.3 表)。
/// `compatibility`:GLES3 把投影灯放进附加 pass,每个 pass 各自 tonemap 再在 sRGB 里相加(scene.glsl:3067-3070),
/// Reinhard 下亮部直接过曝(实测方向光正照白面 185 → 255);所以 Compatibility 下 castShadow 不生效,
/// capabilities.coverage 里如实写明。
pub fn build(rs: &mut Gd<RenderingServer>, scenario: Rid, leg: Leg, lights: &[LightItem], compatibility: bool) -> Vec<LightInst> {
    let r = rig(leg);
    if lights.is_empty() {
        let l = Owned::new(rs.directional_light_create());
        node_defaults(rs, l.rid(), LightKind::Directional);
        rs.light_set_color(l.rid(), Color::from_rgba(1.0, 1.0, 1.0, 1.0));
        rs.light_set_param(l.rid(), LightParam::ENERGY, r.energy);
        rs.light_set_shadow(l.rid(), false);
        let d = norm(r.toward_light);
        // 生成的绑定只收 target(up 缺省 Vector3::UP、use_model_front 缺省 false,GD/core/math/basis.h:231)。
        let t = Transform3D::new(Basis::looking_at(-d), Vector3::ZERO);
        return vec![place(rs, scenario, l, t)];
    }
    let mut out = Vec::with_capacity(lights.len());
    for li in lights {
        let l = Owned::new(match li.kind {
            LightKind::Directional => rs.directional_light_create(),
            LightKind::Point => rs.omni_light_create(),
            LightKind::Spot => rs.spot_light_create(),
        });
        node_defaults(rs, l.rid(), li.kind);
        // Light.color 是线性值;RS 的灯光颜色按 sRGB 解释(与 Light3D.light_color 同),所以先编码。
        rs.light_set_color(l.rid(), srgb_color(li.color, 1.0));
        rs.light_set_param(l.rid(), LightParam::ENERGY, r.energy * li.intensity);
        rs.light_set_shadow(l.rid(), li.cast_shadow && !compatibility);
        if li.kind == LightKind::Directional {
            rs.light_directional_set_shadow_mode(l.rid(), LightDirectionalShadowMode::PARALLEL_4_SPLITS);
        }
        if let Some(p) = &li.params {
            light_params(rs, l.rid(), li.kind, &fields!(p));
        }
        out.push(place(rs, scenario, l, xform(&li.world)));
    }
    out
}

/// Stage 5 LightParams(挂在 Light 实体上;缺省 = Stage 4 的灯参数,见 02 §9.5 Stage 5 的 schema 表)。
/// shadowBias / shadowNormalBias < 0 = 保留该灯种的节点缺省(node_defaults 已设)。
fn light_params(rs: &mut Gd<RenderingServer>, l: Rid, kind: LightKind, f: &Fields) {
    let set = |rs: &mut Gd<RenderingServer>, p: LightParam, v: f32| rs.light_set_param(l, p, v);
    set(rs, LightParam::RANGE, f.num("range"));
    set(rs, LightParam::ATTENUATION, f.num("attenuation"));
    set(rs, LightParam::SPOT_ANGLE, f.num("spotAngle"));
    set(rs, LightParam::SPOT_ATTENUATION, f.num("spotAttenuation"));
    set(rs, LightParam::SPECULAR, f.num("specular"));
    set(rs, LightParam::INDIRECT_ENERGY, f.num("indirectEnergy"));
    set(rs, LightParam::VOLUMETRIC_FOG_ENERGY, f.num("volumetricFogEnergy"));
    set(rs, LightParam::SIZE, f.num("size"));
    set(rs, LightParam::SHADOW_BLUR, f.num("shadowBlur"));
    set(rs, LightParam::SHADOW_OPACITY, f.num("shadowOpacity"));
    if f.num("shadowBias") >= 0.0 {
        set(rs, LightParam::SHADOW_BIAS, f.num("shadowBias"));
    }
    if f.num("shadowNormalBias") >= 0.0 {
        set(rs, LightParam::SHADOW_NORMAL_BIAS, f.num("shadowNormalBias"));
    }
    rs.light_set_negative(l, f.flag("negative"));
    match kind {
        LightKind::Directional => {
            set(rs, LightParam::SHADOW_MAX_DISTANCE, f.num("shadowMaxDistance"));
            rs.light_directional_set_shadow_mode(l, match f.text("directionalShadowMode") {
                "orthogonal" => LightDirectionalShadowMode::ORTHOGONAL,
                "parallel2Splits" => LightDirectionalShadowMode::PARALLEL_2_SPLITS,
                _ => LightDirectionalShadowMode::PARALLEL_4_SPLITS,
            });
        }
        LightKind::Point => rs.light_omni_set_shadow_mode(l, if f.text("omniShadowMode") == "cube" {
            LightOmniShadowMode::CUBE
        } else {
            LightOmniShadowMode::DUAL_PARABOLOID
        }),
        LightKind::Spot => {}
    }
}
