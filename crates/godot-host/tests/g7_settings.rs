//! Additional acceptance for mapped scaling modes: unsupported renderers must
//! keep bilinear output, Forward+ must actually use FSR rather than only report it.
mod common;
mod g4util;
mod g5util;

use common::serial;
use g4util::{godot_at, temp_project, CONFIGS};
use g5util::*;
use serde_json::json;

#[test]
fn scaling_modes_follow_reported_capabilities_and_reset() {
    let _guard = serial();
    let root = temp_project("g7-scaling", None);
    let model = white(&root);
    let q = [0.0, 0.0, (0.35f32).sin(), (0.35f32).cos()];
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let ids = scene(&mut r, "scaling", &[
            ent("card", Some(&model), [0.0; 3], q, [1.6, 1.6, 1.0], vec![
                comp("RenderSettings", json!({"scaling3dScale": 0.5})),
            ]),
        ], &ortho_cam());
        let caps = r.call("render.capabilities", json!({}));
        let unsupported = unsupported(&caps).iter().any(|f| f == "RenderSettings.scaling3dMode");
        let bilinear = frame_n(&mut r, 4);
        for mode in ["fsr", "fsr2"] {
            set(&mut r, ids[0], "RenderSettings", json!({"scaling3dScale": 0.5, "scaling3dMode": mode}));
            let pixels = frame_n(&mut r, 32);
            if unsupported {
                assert_eq!(pixels, bilinear, "{method}/{driver}: unsupported {mode} must remain bilinear");
            } else {
                assert_ne!(pixels, bilinear, "{method}/{driver}: {mode} must change scaled edge reconstruction");
            }
            set(&mut r, ids[0], "RenderSettings", json!({"scaling3dScale": 0.5}));
            assert_eq!(frame_n(&mut r, 4), bilinear, "{method}/{driver}: removing {mode} must restore bilinear");
        }
        assert!(!g.log().iter().any(|line| line.contains("SHADER ERROR") || line.contains("未知属性")), "{:?}", g.log());
    }
}

#[test]
fn ssao_quality_changes_sampling_only_on_supported_renderers() {
    let _guard = serial();
    let root = temp_project("g7-quality", None);
    let model = white(&root);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let ids = scene(&mut r, "quality", &[
            ent("floor", Some(&model), [0.0,-1.0,-1.0], rot_x(-std::f32::consts::FRAC_PI_2), [4.0,4.0,1.0], vec![]),
            ent("wall", Some(&model), [0.0,1.0,-3.0], ID, [4.0,4.0,1.0], vec![]),
            no_light(),
            ent("env", None, [0.0;3], ID, [1.0;3], vec![
                comp("Environment", json!({"ambientSource":"color", "ambientColor":[0.5,0.5,0.5,1.0], "ambientEnergy":1.0,
                    "ssaoEnabled":true, "ssaoIntensity":4.0, "ssaoRadius":1.5, "ssaoPower":2.0})),
                comp("RenderSettings", json!({"ssaoQuality":"veryLow"})),
            ]),
        ], &persp_cam([0.0,-1.0,-3.0], 0.0, 30.0, 5.0));
        let low = frame_n(&mut r, 8);
        set(&mut r, ids[3], "RenderSettings", json!({"ssaoQuality":"ultra"}));
        let high = frame_n(&mut r, 8);
        let count = changed(&low, &high);
        eprintln!("{method}/{driver} SSAO quality changed pixels={count}");
        if method == "forward_plus" {
            assert!(count > 100.0, "quality must affect actual SSAO sampling: {method}/{driver}");
        } else if method == "mobile" {
            assert_eq!(low, high, "unsupported SSAO must stay unchanged");
        } else {
            assert!(count > 0.0, "GLES SSAO quality must change actual sampling");
        }
        set(&mut r, ids[3], "RenderSettings", json!({"ssaoQuality":"veryLow"}));
        assert_eq!(frame_n(&mut r, 8), low, "quality reset must restore the low-quality frame");
        assert!(!g.log().iter().any(|line| line.contains("SHADER ERROR") || line.contains("未知属性")), "{:?}", g.log());
    }
}

