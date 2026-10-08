//! Stage 5 step 4 / 5(01 §6.1、02 §9.5 Stage 5):CameraAttributes(曝光倍数、远景 DOF、自动曝光)与 SDFGI。
//! 自动曝光与 SDFGI 是时间性效果,连取多帧后再比;不支持的配置画面逐字节不变且 capabilities 标出。
mod common;
mod g4util;
mod g5util;

use serde_json::{json, Value};

use common::serial;
use g4util::{godot_at, material, rgba, temp_project, texture, CONFIGS};
use g5util::*;

fn attrs_entity(env: Value) -> Vec<Value> {
    vec![ent("env", None, [0.0; 3], ID, [1.0; 3], vec![comp("Environment", env),
        json!({ "type": "CameraAttributes", "enabled": false, "props": {} })])]
}

#[test]
fn camera_attributes_follow_capabilities() {
    let _g = serial();
    let root = temp_project("g5-cam", None);
    let w = white(&root);
    // 远处棋盘(8×8 格,最近邻):DOF 远景模糊后方差下降。
    let chk = rgba(8, 8, |x, y| if (x + y) % 2 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 255] });
    let mut m = material("chk", [1.0; 4]);
    m["baseColorTexture"] = json!(0);
    let checker = quad_model(&root, "checker", m, vec![texture("chk", 8, 8, chk, true)]);
    let gray = json!({ "ambientSource": "color", "ambientColor": [0.5, 0.5, 0.5, 1.0], "reflectionSource": "disabled" });
    let dark = json!({ "ambientSource": "color", "ambientColor": [0.08, 0.08, 0.08, 1.0], "reflectionSource": "disabled" });
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let caps = r.call("render.capabilities", json!({}));
        // 曝光倍数经 exposure normalization 作用在灯的能量上(camera_attributes_storage.cpp:141),平面环境光不受影响:
        // 这里用一盏方向光照面板、环境光关掉。
        let sun = ent("sun", None, [0.0; 3], rot_x(-0.5), [1.0; 3],
            vec![comp("Light", json!({ "kind": "directional", "color": [1.0, 1.0, 1.0], "intensity": 0.5, "castShadow": false }))]);
        let mut ents = vec![ent("panel", Some(&w), [0.0; 3], ID, [2.0, 2.0, 1.0], vec![]), sun];
        ents.extend(attrs_entity(json!({ "ambientSource": "disabled", "reflectionSource": "disabled" })));
        let ids = scene(&mut r, "g5-exposure", &ents, &ortho_cam());
        toggles(&mut r, method, &caps, ids[2], &[Toggle { feature: "CameraAttributes.exposure", ctype: "CameraAttributes",
            off: None, on: Some(json!({ "exposureMultiplier": 2.0 })), frames: 1,
            metric: |a, b| lum(mean_rect(b, 150, 80, 170, 100)) - lum(mean_rect(a, 150, 80, 170, 100)), min: 10.0, expect: "曝光倍数 ×2 变亮" }]);
        let mut ents = vec![ent("far", Some(&checker), [0.0, 0.0, -30.0], ID, [12.0, 12.0, 1.0], vec![]), no_light()];
        ents.extend(attrs_entity(gray.clone()));
        let ids = scene(&mut r, "g5-dof", &ents, &persp_cam([0.0; 3], 0.0, 0.0, 6.0));
        toggles(&mut r, method, &caps, ids[2], &[Toggle { feature: "CameraAttributes.dof", ctype: "CameraAttributes",
            off: None, on: Some(json!({ "dofBlurFarEnabled": true, "dofBlurFarDistance": 8.0, "dofBlurFarTransition": 2.0, "dofBlurAmount": 0.6 })), frames: 2,
            metric: |a, b| { let (va, vb) = (variance(a, 130, 70, 190, 110), variance(b, 130, 70, 190, 110)); 100.0 * (va - vb) / va.max(1.0) }, min: 20.0,
            expect: "远处棋盘被模糊、方差下降" }]);
        let mut ents = vec![ent("panel", Some(&w), [0.0; 3], ID, [2.0, 2.0, 1.0], vec![]), no_light()];
        ents.extend(attrs_entity(dark.clone()));
        let ids = scene(&mut r, "g5-autoexp", &ents, &ortho_cam());
        toggles(&mut r, method, &caps, ids[2], &[Toggle { feature: "CameraAttributes.autoExposure", ctype: "CameraAttributes",
            off: None, on: Some(json!({ "autoExposureEnabled": true, "autoExposureSpeed": 20.0, "autoExposureScale": 0.4 })), frames: 40,
            metric: |a, b| lum(mean_rect(b, 150, 80, 170, 100)) - lum(mean_rect(a, 150, 80, 170, 100)), min: 5.0, expect: "暗场景自动曝光后变亮" }]);
        drop(g);
    }
}

/// SDFGI:阳光照亮背墙与地面,环境光关掉,只剩间接光;开 SDFGI 连取 60 帧(收敛)后画面变亮。
#[test]
fn sdfgi_follows_capabilities() {
    let _g = serial();
    let root = temp_project("g5-sdfgi", None);
    let (w, red) = (white(&root), colored(&root, "red", [1.0, 0.1, 0.1, 1.0]));
    let base = json!({ "ambientSource": "disabled", "reflectionSource": "disabled" });
    let mut on = base.clone();
    on["sdfgiEnabled"] = json!(true);
    on["sdfgiEnergy"] = json!(2.0);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let caps = r.call("render.capabilities", json!({}));
        let ents = vec![
            ent("floor", Some(&w), [0.0, -1.0, -1.0], rot_x(-std::f32::consts::FRAC_PI_2), [6.0, 6.0, 1.0], vec![]),
            ent("wall", Some(&red), [0.0, 1.0, -3.0], ID, [6.0, 4.0, 1.0], vec![]),
            ent("sun", None, [0.0; 3], rot_x(-0.6), [1.0; 3], vec![comp("Light", json!({ "kind": "directional", "color": [1.0, 1.0, 1.0], "intensity": 2.0, "castShadow": false }))]),
            ent("env", None, [0.0; 3], ID, [1.0; 3], vec![comp("Environment", base.clone())]),
        ];
        let ids = scene(&mut r, "g5-sdfgi", &ents, &persp_cam([0.0, -1.0, -3.0], 0.0, 30.0, 5.0));
        toggles(&mut r, method, &caps, ids[3], &[Toggle { feature: "Environment.sdfgi", ctype: "Environment",
            off: Some(base.clone()), on: Some(on.clone()), frames: 60,
            metric: |a, b| mean_all(b) - mean_all(a), min: 1.0, expect: "间接光让画面变亮" }]);
        drop(g);
    }
}
