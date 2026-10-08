//! Stage 5(01 §6.3、02 §9.5 Stage 5):LightParams(挂在 Light 实体上)与 RenderSettings(视口级 / RS 全局)。
mod common;
mod g4util;
mod g5util;

use serde_json::json;

use common::serial;
use g4util::{godot_at, temp_project, CONFIGS};
use g5util::*;

/// 被灯照亮的像素数(缺省环境光 0.14 让面板本身约 99,阈值取 150)。
fn lit(px: &[u8]) -> f64 {
    px.chunks_exact(4).filter(|p| lum([p[0] as f64, p[1] as f64, p[2] as f64]) > 150.0).count() as f64
}

/// 离点光 1 个单位处(面板上 x = +1)的亮度:range 0.5 照不到,range 5 照得到。
fn side(px: &[u8]) -> f64 {
    lum(mean_rect(px, 216, 86, 224, 94))
}

#[test]
fn light_params_follow_godot_semantics() {
    let _g = serial();
    let root = temp_project("g5-lightparams", None);
    let w = white(&root);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let caps = r.call("render.capabilities", json!({}));
        let ents = vec![
            ent("panel", Some(&w), [0.0; 3], ID, [3.0, 3.0, 1.0], vec![]),
            ent("point", None, [0.0, 0.0, 1.0], ID, [1.0; 3], vec![
                comp("Light", json!({ "kind": "point", "color": [1.0, 1.0, 1.0], "intensity": 3.0, "castShadow": false })),
                comp("LightParams", json!({ "range": 0.5 }))]),
        ];
        let ids = scene(&mut r, "g5-range", &ents, &ortho_cam());
        let rows = toggles(&mut r, method, &caps, ids[1], &[
            Toggle { feature: "LightParams.range", ctype: "LightParams", off: Some(json!({ "range": 0.5 })), on: Some(json!({ "range": 5.0 })), frames: 1,
                     metric: |a, b| side(b) - side(a), min: 20.0, expect: "range 变大、远处被照亮" },
            Toggle { feature: "LightParams.negative", ctype: "LightParams", off: Some(json!({ "range": 5.0 })), on: Some(json!({ "range": 5.0, "negative": true })), frames: 1,
                     metric: |a, b| lum(mean_rect(a, 150, 80, 170, 100)) - lum(mean_rect(b, 150, 80, 170, 100)), min: 20.0, expect: "负光变暗" },
        ]);
        eprintln!("G5 {method} lightparams {rows:?}");
        let ents = vec![
            ent("panel", Some(&w), [0.0; 3], ID, [3.0, 3.0, 1.0], vec![]),
            ent("spot", None, [0.0, 0.0, 2.0], ID, [1.0; 3], vec![
                comp("Light", json!({ "kind": "spot", "color": [1.0, 1.0, 1.0], "intensity": 4.0, "castShadow": false })),
                comp("LightParams", json!({ "range": 10.0, "spotAngle": 5.0 }))]),
        ];
        let ids = scene(&mut r, "g5-spot", &ents, &ortho_cam());
        toggles(&mut r, method, &caps, ids[1], &[Toggle { feature: "LightParams.spotAngle", ctype: "LightParams",
            off: Some(json!({ "range": 10.0, "spotAngle": 5.0 })), on: Some(json!({ "range": 10.0, "spotAngle": 30.0 })), frames: 1,
            metric: |a, b| lit(b) - lit(a), min: 500.0, expect: "聚光角变大、光斑变大" }]);
        drop(g);
    }
}

/// RenderSettings:斜放的白 quad(高对比边缘),缺省灯;off = 全缺省(与没有组件逐字节相同,见 g5_env)。
#[test]
fn render_settings_follow_capabilities() {
    let _g = serial();
    let root = temp_project("g5-rs", None);
    let w = white(&root);
    let q = [0.0, 0.0, (0.35f32).sin(), (0.35f32).cos()];
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let caps = r.call("render.capabilities", json!({}));
        let ents = vec![ent("q", Some(&w), [0.0; 3], q, [1.6, 1.6, 1.0], vec![json!({ "type": "RenderSettings", "enabled": false, "props": {} })])];
        let ids = scene(&mut r, "g5-rs", &ents, &ortho_cam());
        let edge = |a: &[u8], b: &[u8]| partial(b, 10.0, 200.0) - partial(a, 10.0, 200.0);
        let rows = toggles(&mut r, method, &caps, ids[0], &[
            Toggle { feature: "RenderSettings.msaa3d", ctype: "RenderSettings", off: None, on: Some(json!({ "msaa3d": "4x" })), frames: 1, metric: edge, min: 50.0, expect: "MSAA 让边缘出现半覆盖像素" },
            Toggle { feature: "RenderSettings.screenSpaceAA", ctype: "RenderSettings", off: None, on: Some(json!({ "screenSpaceAA": "fxaa" })), frames: 1, metric: edge, min: 50.0, expect: "FXAA 柔化边缘" },
            Toggle { feature: "RenderSettings.taa", ctype: "RenderSettings", off: None, on: Some(json!({ "taa": true })), frames: 8, metric: edge, min: 50.0, expect: "TAA 多帧后边缘柔化" },
            Toggle { feature: "RenderSettings.scaling3dScale", ctype: "RenderSettings", off: None, on: Some(json!({ "scaling3dScale": 0.5 })), frames: 1, metric: edge, min: 50.0, expect: "半分辨率 3D 放大后边缘变软" },
            Toggle { feature: "RenderSettings.debanding", ctype: "RenderSettings", off: None, on: Some(json!({ "debanding": true })), frames: 1, metric: changed, min: 100.0, expect: "debanding 的抖动改变像素" },
        ]);
        eprintln!("G5 {method} rendersettings {rows:?}");
        drop(g);
    }
}
