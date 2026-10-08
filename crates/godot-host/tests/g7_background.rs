//! Godot 4.7.2 fog-only sky must preserve a zero-density color background.
//! Covers both RD renderers and the unaffected Compatibility path, including
//! background energy and camera exposure so compensation cannot double-apply them.
mod common;
mod g4util;
mod g5util;

use common::serial;
use g4util::{godot_at, temp_project, CONFIGS};
use g5util::*;
use serde_json::json;

#[test]
fn zero_density_fog_preserves_background_color_and_energy() {
    let _guard = serial();
    let root = temp_project("g7-fog-background", None);
    let model = white(&root);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut rpc = g.rpc();
        let ids = scene(&mut rpc, "fog-background", &[
            ent("card", Some(&model), [0.0; 3], ID, [0.5, 0.5, 1.0], vec![
                comp("Environment", json!({})),
                comp("CameraAttributes", json!({})),
            ]),
        ], &ortho_cam());
        for background in ["clearColor", "color"] {
          for (tonemap, exposure) in [("linear", 1.0), ("reinhard", 1.0), ("linear", 2.0)] {
            for (energy, camera_exposure) in [(1.0, 1.0), (0.5, 1.0), (2.0, 0.5), (1.0, 2.0), (0.0, 1.0)] {
                set(&mut rpc, ids[0], "CameraAttributes", json!({"exposureMultiplier": camera_exposure}));
                let base = json!({
                    "background": background, "backgroundColor": [0.12, 0.24, 0.4, 1.0],
                    "backgroundEnergy": energy, "tonemap": tonemap, "exposure": exposure, "white": 4.0,
                    "fogDensity": 0.0, "fogSkyAffect": 0.0,
                    "volumetricFogDensity": 0.0, "volumetricFogSkyAffect": 0.0,
                    "volumetricFogTemporalReprojection": false,
                });
                set(&mut rpc, ids[0], "Environment", base.clone());
                let off = frame_n(&mut rpc, 3);
                for flag in ["fogEnabled", "volumetricFogEnabled"] {
                    let mut on = base.clone();
                    on[flag] = json!(true);
                    set(&mut rpc, ids[0], "Environment", on);
                    let on = frame_n(&mut rpc, 4);
                    let a = mean_rect(&off, 3, 3, 40, 40);
                    let b = mean_rect(&on, 3, 3, 40, 40);
                    eprintln!("{method}/{driver} {background} {tonemap}/{exposure} {flag} energy={energy} exposure={camera_exposure}: {a:?} -> {b:?}");
                    assert!((0..3).all(|c| (a[c] - b[c]).abs() <= 2.0),
                        "neutral fog changed background: {method}/{driver} {background} {flag} energy={energy} exposure={camera_exposure}: {a:?} -> {b:?}");
                }
            }
          }
        }
    }
}
