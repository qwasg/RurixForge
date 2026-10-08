//! Stage 5 step 5(01 §6.1、02 §9.5 Stage 5):ReflectionProbe / Decal / FogVolume,四种配置。
//! 组件开 / 关(enabled)各取帧:支持的配置画面按方向变化,能力表标为不支持的配置逐字节不变。
mod common;
mod g4util;
mod g5util;

use serde_json::json;

use common::serial;
use g4util::{godot_at, temp_project, CONFIGS};
use g5util::*;

#[test]
fn volumes_follow_capabilities() {
    let _g = serial();
    let root = temp_project("g5-vol", None);
    let (w, mirror) = (white(&root), metal(&root, "mirror", [0.95, 0.95, 0.95, 1.0], 0.05));
    let red_lamp = emissive(&root, "redlamp", [3.0, 0.0, 0.0]);
    let red = write_texture(&root, "decal_red", 4, 4, |_, _| [255, 0, 0, 255]);
    let gray = json!({ "ambientSource": "color", "ambientColor": [0.5, 0.5, 0.5, 1.0], "reflectionSource": "disabled" });
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let caps = r.call("render.capabilities", json!({}));
        // 探针:正交相机前的镜面 quad;相机背后(z = +15)一块朝 −Z 的红色自发光板,只能经探针的立方体贴图反射出来。
        let ents = vec![
            ent("mirror", Some(&mirror), [0.0; 3], ID, [2.0, 2.0, 1.0], vec![]),
            ent("lamp", Some(&red_lamp), [0.0, 0.0, 15.0], rot_y(std::f32::consts::PI), [30.0, 30.0, 1.0], vec![]),
            no_light(),
            ent("env", None, [0.0; 3], ID, [1.0; 3], vec![comp("Environment", gray.clone()),
                json!({ "type": "ReflectionProbe", "enabled": false, "props": { "size": [40.0, 40.0, 40.0], "updateMode": "always" } })]),
        ];
        let ids = scene(&mut r, "g5-probe", &ents, &ortho_cam());
        toggles(&mut r, method, &caps, ids[3], &[Toggle { feature: "ReflectionProbe", ctype: "ReflectionProbe", off: None,
            on: Some(json!({ "size": [40.0, 40.0, 40.0], "updateMode": "always" })), frames: 8,
            metric: |a, b| { let (x, y) = (mean_rect(a, 150, 80, 170, 100), mean_rect(b, 150, 80, 170, 100)); (y[0] - y[1]) - (x[0] - x[1]) },
            min: 20.0, expect: "镜面映出探针捕获的红板" }]);
        // 贴花:朝 +Z 的白面板,贴花局部 −Y 转到世界 −Z(绕 X 转 +90°),红色 albedo。
        let ents = vec![
            ent("panel", Some(&w), [0.0; 3], ID, [2.0, 2.0, 1.0], vec![]),
            no_light(),
            ent("env", None, [0.0; 3], ID, [1.0; 3], vec![comp("Environment", gray.clone())]),
            ent("decal", None, [0.0, 0.0, 0.0], rot_x(std::f32::consts::FRAC_PI_2), [1.0; 3],
                vec![json!({ "type": "Decal", "enabled": false, "props": { "size": [1.0, 1.0, 1.0], "textureAlbedo": red } })]),
        ];
        let ids = scene(&mut r, "g5-decal", &ents, &ortho_cam());
        toggles(&mut r, method, &caps, ids[3], &[Toggle { feature: "Decal", ctype: "Decal", off: None,
            on: Some(json!({ "size": [1.0, 1.0, 1.0], "textureAlbedo": red })), frames: 2,
            metric: |a, b| { let (x, y) = (mean_rect(a, 155, 85, 165, 95), mean_rect(b, 155, 85, 165, 95)); (y[0] - y[1]) - (x[0] - x[1]) },
            min: 30.0, expect: "面板中心被贴成红色" }]);
        // 雾体积:体积雾开着但全局密度 0,中心一个自发光红的盒子雾。
        let fog = json!({ "ambientSource": "color", "ambientColor": [0.3, 0.3, 0.3, 1.0], "volumetricFogEnabled": true,
                          "volumetricFogDensity": 0.0, "volumetricFogTemporalReprojection": false });
        let ents = vec![
            ent("panel", Some(&w), [0.0, 0.0, -2.0], ID, [4.0, 4.0, 1.0], vec![]),
            no_light(),
            ent("env", None, [0.0; 3], ID, [1.0; 3], vec![comp("Environment", fog)]),
            ent("fogvol", None, [0.0, 0.0, 0.0], ID, [1.0; 3],
                vec![json!({ "type": "FogVolume", "enabled": false, "props": { "size": [1.5, 1.5, 1.5], "density": 2.0, "emission": [1.0, 0.0, 0.0, 1.0] } })]),
        ];
        let ids = scene(&mut r, "g5-fogvol", &ents, &persp_cam([0.0; 3], 0.0, 0.0, 6.0));
        toggles(&mut r, method, &caps, ids[3], &[Toggle { feature: "FogVolume", ctype: "FogVolume", off: None,
            on: Some(json!({ "size": [1.5, 1.5, 1.5], "density": 2.0, "emission": [1.0, 0.0, 0.0, 1.0] })), frames: 4,
            metric: |a, b| mean_rect(b, 150, 80, 170, 100)[0] - mean_rect(a, 150, 80, 170, 100)[0], min: 5.0, expect: "雾体积区域发红" }]);
        drop(g);
    }
}
