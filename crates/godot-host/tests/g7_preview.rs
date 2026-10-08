//! Main and template-preview channels have independent cameras, sizes and scene resources.
mod common;
mod g4util;
mod g5util;

use base64::Engine;
use common::serial;
use g4util::{godot_at, temp_project, CONFIGS};
use g5util::*;
use serde_json::json;

#[test]
fn template_preview_does_not_replace_or_resize_main_frame() {
    let _guard = serial();
    let root = temp_project("g7-preview", None);
    let model = white(&root);
    std::fs::create_dir_all(root.join("Content/Templates")).unwrap();
    std::fs::write(root.join("Content/Templates/panel.rxprefab"), serde_json::to_vec(&json!({
        "version": 1, "revision": 1, "name": "panel", "model": model,
        "entities": [{"id": 1, "name": "panel",
            "transform": {"translation": [0,0,0], "rotation": [0,0,0,1], "scale": [1,1,1]},
            "components": [{"type": "ModelRenderer", "enabled": true, "props": {"model": model}}]
        }]
    })).unwrap()).unwrap();
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        scene(&mut r, "main", &[ent("main-panel", Some(&model), [-0.5, 0.0, 0.0], ID, [1.0;3], vec![])], &ortho_cam());
        let before = frame_n(&mut r, 2);
        for (width, height) in [(96, 64), (128, 96), (96, 64)] {
            let frame = r.call("template.preview", json!({
                "prefabRef": "Content/Templates/panel.rxprefab", "width": width, "height": height, "yaw": 0.0
            }));
            assert_eq!(frame["width"], width);
            assert_eq!(frame["height"], height);
            assert!(frame["nonZeroPixels"].as_u64().unwrap() > 0, "empty preview: {method}/{driver}");
            let pixels = base64::engine::general_purpose::STANDARD.decode(frame["pixelsB64"].as_str().unwrap()).unwrap();
            assert_eq!(pixels.len(), (width * height * 4) as usize);
            assert_eq!(r.frame(W,H).1, before, "preview changed main channel: {method}/{driver}");
        }
    }
}
