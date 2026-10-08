//! Stage 5 step 4(01 §6、02 §9.5 Stage 5):Environment 组件 → Godot `Environment` 资源,四种配置。
//! - 缺省等价:没有 Stage 5 组件 = 组件存在但禁用 = 全缺省的 LightParams / RenderSettings(逐字节相同);
//!   全缺省的 Environment 背景仍是腿清屏色。
//! - 每个特性在同一进程里开 / 关各取一帧,支持的配置画面按方向变化,能力表标为不支持的配置画面逐字节不变。
mod common;
mod g4util;
mod g5util;

use serde_json::{json, Value};

use common::{serial, sha256, Rpc};
use g4util::{godot_at, temp_project, CONFIGS};
use g5util::*;

/// 环境光面板:2×2 白 quad,缺省灯关掉(只剩环境光),最后一个实体挂 Environment(先禁用)。
fn panel(root: &std::path::Path) -> Vec<Value> {
    let w = white(root);
    vec![
        ent("panel", Some(&w), [0.0; 3], ID, [2.0, 2.0, 1.0], vec![]),
        no_light(),
        ent("env", None, [0.0; 3], ID, [1.0; 3], vec![json!({ "type": "Environment", "enabled": false, "props": {} })]),
    ]
}

fn center(px: &[u8]) -> [f64; 3] {
    mean_rect(px, 150, 80, 170, 100)
}

fn bg(px: &[u8]) -> [f64; 3] {
    mean_rect(px, 2, 2, 12, 12)
}

#[test]
fn defaults_are_byte_identical_to_stage4() {
    let _g = serial();
    let root = temp_project("g5-defaults", None);
    let ents = panel(&root);
    let w = white(&root);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        // 模型腿 + 缺省灯(没有 Light 实体)。
        r.call("scene.new", json!({ "name": "g5-defaults" }));
        let panel_id = r.call("entity.create", ent("p", Some(&w), [0.0; 3], ID, [2.0, 2.0, 1.0], vec![]))["id"].as_u64().unwrap();
        r.call("viewport.setCamera", ortho_cam());
        let base = r.frame(W, H).1;
        for (ctype, props) in [("Environment", json!({})), ("RenderSettings", json!({})), ("CameraAttributes", json!({}))] {
            r.call("component.add", json!({ "id": panel_id, "type": ctype, "enabled": false, "props": props }));
            assert_eq!(sha256(&r.frame(W, H).1), sha256(&base), "{method}/{driver}:禁用的 {ctype} 不改变画面");
            r.call("component.remove", json!({ "id": panel_id, "type": ctype }));
        }
        for ctype in ["RenderSettings", "CameraAttributes"] {
            r.call("component.add", json!({ "id": panel_id, "type": ctype, "props": {} }));
            let px = r.frame(W, H).1;
            eprintln!("G5 {method} default-{ctype}: identical={}", px == base);
            assert_eq!(sha256(&px), sha256(&base), "{method}/{driver}:全缺省的 {ctype} 与没有它逐字节相同");
            r.call("component.remove", json!({ "id": panel_id, "type": ctype }));
        }
        // 有 Light 实体时:挂一个全缺省的 LightParams 与不挂逐字节相同。
        let l = r.call("entity.create", ent("sun", None, [0.0; 3], rot_x(-0.8), [1.0; 3],
            vec![comp("Light", json!({ "kind": "directional", "color": [1.0, 0.9, 0.8], "intensity": 1.0, "castShadow": false }))]))["id"].as_u64().unwrap();
        let lit = r.frame(W, H).1;
        r.call("component.add", json!({ "id": l, "type": "LightParams", "props": {} }));
        assert_eq!(sha256(&r.frame(W, H).1), sha256(&lit), "{method}/{driver}:全缺省 LightParams = 不挂");
        r.call("entity.destroy", json!({ "id": l }));
        // 全缺省 Environment:tonemap 变成 Godot 缺省的 linear,但背景仍是腿清屏色。
        r.call("component.add", json!({ "id": panel_id, "type": "Environment", "props": {} }));
        let e = r.frame(W, H).1;
        let (b0, b1) = (bg(&base), bg(&e));
        eprintln!("G5 {method} default-Environment bg {b0:?} -> {b1:?}");
        assert!((0..3).all(|c| (b0[c] - b1[c]).abs() <= 1.0), "{method}/{driver}:clearColor 背景 {b0:?} vs {b1:?}");
        drop(g);
        let _ = &ents;
    }
}
struct Case {
    feature: &'static str,
    off: Value,
    on: Value,
    frames: usize,
    /// (off, on) → 期望方向上的变化量(> 阈值才算通过)。
    metric: fn(&[u8], &[u8]) -> f64,
    min: f64,
    expect: &'static str,
}

fn amb(c: [f32; 4], energy: f32) -> Value {
    json!({ "ambientSource": "color", "ambientColor": c, "ambientEnergy": energy, "reflectionSource": "disabled" })
}

fn with(mut base: Value, extra: Value) -> Value {
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

fn panel_cases() -> Vec<Case> {
    let gray = amb([0.5, 0.5, 0.5, 1.0], 1.0);
    vec![
        Case { feature: "Environment.ambient", off: gray.clone(), on: amb([1.0, 0.0, 0.0, 1.0], 1.0), frames: 1,
               metric: |a, b| { let (x, y) = (center(a), center(b)); (y[0] - y[1]) - (x[0] - x[1]) }, min: 50.0, expect: "环境光变红" },
        Case { feature: "Environment.exposure", off: gray.clone(), on: with(gray.clone(), json!({ "exposure": 2.0 })), frames: 1,
               metric: |a, b| lum(center(b)) - lum(center(a)), min: 10.0, expect: "曝光 ×2 变亮" },
        Case { feature: "Environment.tonemap", off: amb([1.0; 4], 4.0), on: with(amb([1.0; 4], 4.0), json!({ "tonemap": "agx" })), frames: 1,
               metric: |a, b| lum(center(a)) - lum(center(b)), min: 5.0, expect: "AgX 压住 linear 截断的高光" },
        Case { feature: "Environment.adjustment", off: amb([1.0, 0.5, 0.25, 1.0], 1.0),
               on: with(amb([1.0, 0.5, 0.25, 1.0], 1.0), json!({ "adjustmentEnabled": true, "adjustmentSaturation": 0.0 })), frames: 1,
               metric: |a, b| { let (x, y) = (center(a), center(b)); (x[0] - x[2]) - (y[0] - y[2]).abs() }, min: 30.0, expect: "饱和度 0 变灰" },
        Case { feature: "Environment.background", off: json!({}), on: json!({ "background": "color", "backgroundColor": [0.0, 0.0, 1.0, 1.0] }), frames: 1,
               metric: |a, b| bg(b)[2] - bg(a)[2], min: 100.0, expect: "背景变蓝" },
        Case { feature: "Environment.sky", off: json!({}), on: json!({ "background": "sky" }), frames: 3,
               metric: |a, b| lum(bg(b)) - lum(bg(a)), min: 20.0, expect: "程序化天空比清屏色亮" },
        Case { feature: "Environment.fog", off: amb([1.0; 4], 1.0),
               on: with(amb([1.0; 4], 1.0), json!({ "fogEnabled": true, "fogDensity": 0.3, "fogLightColor": [0.0, 1.0, 0.0, 1.0] })), frames: 1,
               metric: |a, b| center(a)[0] - center(b)[0], min: 30.0, expect: "白面被绿雾盖住、红通道下降" },
    ]
}

fn run(r: &mut Rpc, method: &str, caps: &Value, env: u64, cases: &[Case]) {
    for c in cases {
        set(r, env, "Environment", c.off.clone());
        enable(r, env, "Environment", true);
        let off = frame_n(r, c.frames);
        set(r, env, "Environment", c.on.clone());
        let on = frame_n(r, c.frames);
        check(method, caps, c.feature, (c.metric)(&off, &on) - c.min, &off, &on, c.expect);
    }
}

#[test]
fn environment_features_follow_capabilities() {
    let _g = serial();
    let root = temp_project("g5-env", None);
    let ents = panel(&root);
    let glow_ents = {
        let e = emissive(&root, "hot", [6.0, 6.0, 6.0]);
        vec![ent("hot", Some(&e), [0.0; 3], ID, [0.4, 0.4, 1.0], vec![]), no_light(),
             ent("env", None, [0.0; 3], ID, [1.0; 3], vec![json!({ "type": "Environment", "enabled": false, "props": {} })])]
    };
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let caps = r.call("render.capabilities", json!({}));
        let ids = scene(&mut r, "g5-panel", &ents, &ortho_cam());
        run(&mut r, method, &caps, ids[2], &panel_cases());
        let ids = scene(&mut r, "g5-glow", &glow_ents, &ortho_cam());
        run(&mut r, method, &caps, ids[2], &[Case { feature: "Environment.glow", off: json!({}),
            on: json!({ "glowEnabled": true, "glowIntensity": 1.0, "glowBloom": 0.3 }), frames: 2,
            metric: |a, b| lum(mean_rect(b, 180, 86, 186, 94)) - lum(mean_rect(a, 180, 86, 186, 94)), min: 5.0, expect: "辉光溢出到自发光面外" }]);
        drop(g);
    }
}
/// 墙角:地面(y = −1,朝 +Y)+ 背墙(z = −3,朝 +Z),都是 4×4;透视相机从前上方看墙角,墙角线过画面中心。
fn corner(floor: &str, wall: &str) -> Vec<Value> {
    vec![
        ent("floor", Some(floor), [0.0, -1.0, -1.0], rot_x(-std::f32::consts::FRAC_PI_2), [4.0, 4.0, 1.0], vec![]),
        ent("wall", Some(wall), [0.0, 1.0, -3.0], ID, [4.0, 4.0, 1.0], vec![]),
        no_light(),
        ent("env", None, [0.0; 3], ID, [1.0; 3], vec![json!({ "type": "Environment", "enabled": false, "props": {} })]),
    ]
}

fn corner_cam() -> Value {
    persp_cam([0.0, -1.0, -3.0], 0.0, 30.0, 5.0)
}

fn below(px: &[u8]) -> [f64; 3] {
    mean_rect(px, 130, 100, 190, 125)
}

#[test]
fn screen_space_effects_follow_capabilities() {
    let _g = serial();
    let root = temp_project("g5-ss", None);
    let (w, red, mirror, lamp) = (white(&root), emissive(&root, "redlamp", [3.0, 0.0, 0.0]), metal(&root, "mirror", [0.9, 0.9, 0.9, 1.0], 0.05), emissive(&root, "lamp", [4.0, 4.0, 4.0]));
    let gray = amb([0.5, 0.5, 0.5, 1.0], 1.0);
    let dim = amb([0.3, 0.3, 0.3, 1.0], 1.0);
    let sun = ent("sun", None, [0.0; 3], rot_x(-0.9), [1.0; 3],
        vec![comp("Light", json!({ "kind": "directional", "color": [1.0, 1.0, 1.0], "intensity": 1.0, "castShadow": false }))]);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let caps = r.call("render.capabilities", json!({}));
        let ids = scene(&mut r, "g5-ssao", &corner(&w, &w), &corner_cam());
        run(&mut r, method, &caps, ids[3], &[Case { feature: "Environment.ssao", off: gray.clone(),
            on: with(gray.clone(), json!({ "ssaoEnabled": true, "ssaoIntensity": 4.0, "ssaoRadius": 1.5, "ssaoPower": 2.0 })), frames: 2,
            metric: |a, b| lum(mean_rect(a, 130, 84, 190, 96)) - lum(mean_rect(b, 130, 84, 190, 96)), min: 3.0, expect: "墙角变暗" }]);
        let ids = scene(&mut r, "g5-ssil", &corner(&w, &red), &corner_cam());
        run(&mut r, method, &caps, ids[3], &[Case { feature: "Environment.ssil", off: dim.clone(),
            on: with(dim.clone(), json!({ "ssilEnabled": true, "ssilIntensity": 4.0, "ssilRadius": 3.0 })), frames: 2,
            metric: |a, b| (below(b)[0] - below(b)[1]) - (below(a)[0] - below(a)[1]), min: 2.0, expect: "红墙把红光弹到地面" }]);
        let ids = scene(&mut r, "g5-ssr", &corner(&mirror, &lamp), &corner_cam());
        run(&mut r, method, &caps, ids[3], &[Case { feature: "Environment.ssr", off: dim.clone(),
            on: with(dim.clone(), json!({ "ssrEnabled": true, "ssrMaxSteps": 128 })), frames: 2,
            metric: |a, b| lum(below(b)) - lum(below(a)), min: 10.0, expect: "镜面地面映出发光墙" }]);
        let mut ents = panel(&root);
        ents.insert(1, sun.clone());
        let ids = scene(&mut r, "g5-vfog", &ents, &ortho_cam());
        run(&mut r, method, &caps, ids[3], &[Case { feature: "Environment.volumetricFog", off: json!({}),
            on: json!({ "volumetricFogEnabled": true, "volumetricFogDensity": 0.3, "volumetricFogTemporalReprojection": false }), frames: 3,
            metric: |a, b| (mean_all(b) - mean_all(a)).abs(), min: 2.0, expect: "体积雾改变画面亮度" }]);
        drop(g);
    }
}
