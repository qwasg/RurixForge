//! V6 原生图集精灵：合成资产清单与 atlas 的四配置像素测试。
//!
//! GPU 用例显式 ignored；使用 --include-ignored --test-threads=1 串行验收。
mod common;
mod g4util;
mod g5util;

use std::path::Path;

use serde_json::json;

use common::{serial, Rpc};
use g4util::{godot_at, temp_project, CONFIGS};
use g5util::png;

const W: u32 = 640;
const H: u32 = 360;

fn write_synthetic_v6_assets(root: &Path) {
    let dir = root.join("Content").join("Animations").join("v6").join("terrain");
    std::fs::create_dir_all(&dir).unwrap();
    // The selected one-pixel frame is the red center texel; green/blue atlas
    // neighbors make a whole-sheet stretch or wrong crop distinguishable.
    std::fs::write(
        dir.join("synthetic.png"),
        png(3, 1, &[16, 230, 24, 255, 220, 24, 32, 255, 20, 48, 238, 255]),
    ).unwrap();
    std::fs::write(
        dir.join("synthetic.json"),
        serde_json::to_vec(&json!({
            "boxes": [[1, 0, 1, 1], [1, 0, 1, 1], [1, 0, 1, 1], [1, 0, 1, 1], [1, 0, 1, 1], [1, 0, 1, 1], [1, 0, 1, 1]],
            "keys": ["grass", "rock", "water", "road", "highland", "ore", "coal"],
            "pivot": [0.5, 0.5],
            "nativePlaneSpan": 1.0,
            "blend": "additive"
        }))
        .unwrap(),
    ).unwrap();
    let textures = ["grass", "rock", "water", "road", "highland", "ore", "coal"]
        .into_iter()
        .map(|name| (name.to_owned(), json!({ "materialTint": [1.0, 1.0, 1.0, 1.0] })))
        .collect::<serde_json::Map<_, _>>();
    let manifest = json!({
        "terrain": {
            "nativeAtlas": "Content/Animations/v6/terrain/synthetic.png",
            "nativeMetadata": "Content/Animations/v6/terrain/synthetic.json",
            "textures": textures
        },
        "models": {}, "characters": {}, "effects": {}
    });
    let ui = root.join("Content").join("UI").join("v6");
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::write(ui.join("resource-manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
}

/// A model entry is enough to make the initial `core` buildings emitted by
/// `game.session.open` take the native V6 model path. No custom snapshot or
/// game-rule mutation is needed, so this exercises the real compose path.
fn write_non_ground_assets(root: &Path, pivot: [f32; 2], blend: &str) {
    let dir = root.join("Content").join("Animations").join("v6").join("buildings");
    std::fs::create_dir_all(&dir).unwrap();
    // The middle texel is the selected frame. Neighbours are deliberately
    // unrelated colors so a whole-atlas or wrong-rectangle sample is visible.
    std::fs::write(
        dir.join("synthetic-core.png"),
        png(3, 1, &[12, 230, 30, 255, 255, 0, 255, 128, 24, 40, 230, 255]),
    ).unwrap();
    let metadata = json!({
        "boxes": [[1, 0, 1, 1]],
        "pivot": pivot,
        "nativePlaneSpan": 1.0,
        "blend": blend,
        "defaultDirection": "n",
        "clips": {
            "idle": {
                "n": { "start": 0, "endExclusive": 1, "fps": 1, "loop": true }
            }
        }
    });
    std::fs::write(dir.join("synthetic-core.json"), serde_json::to_vec(&metadata).unwrap()).unwrap();
    let manifest = json!({
        "terrain": {},
        "models": {
            "command-core": {
                "nativeAtlas": "Content/Animations/v6/buildings/synthetic-core.png",
                "nativeMetadata": "Content/Animations/v6/buildings/synthetic-core.json"
            }
        },
        "characters": {}, "effects": {}
    });
    let ui = root.join("Content").join("UI").join("v6");
    std::fs::create_dir_all(&ui).unwrap();
    std::fs::write(ui.join("resource-manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
}

fn open_v6(r: &mut Rpc) {
    r.call("game.session.open", json!({ "seed": 1, "opponent": "human", "theme": "river" }));
}

fn red_pixels(px: &[u8]) -> usize {
    px.chunks_exact(4)
        .filter(|p| p[0] > 100 && p[0] > p[1].saturating_add(40) && p[0] > p[2].saturating_add(30))
        .count()
}

fn magenta_bounds(px: &[u8]) -> Option<(u32, u32, u32, u32, usize)> {
    let mut bounds: Option<(u32, u32, u32, u32, usize)> = None;
    for (i, p) in px.chunks_exact(4).enumerate() {
        if p[0] < 100 || p[2] < 100 || p[1] > 100 {
            continue;
        }
        let x = (i as u32) % W;
        let y = (i as u32) / W;
        bounds = Some(match bounds {
            Some((min_x, min_y, max_x, max_y, count)) =>
                (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y), count + 1),
            None => (x, y, x, y, 1),
        });
    }
    bounds
}

fn pixel_diff(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(p, q)| (0..3).any(|i| p[i].abs_diff(q[i]) > 2))
        .count()
}

/// Four renderer/driver combinations use the same generated atlas. This is
/// intentionally ignored so CPU checks never start a GPU process.
#[test]
#[ignore = "GPU integration; parent session runs configurations serially"]
fn v6_native_atlas_sprite_draws_real_pixels_in_all_four_configs() {
    let _guard = serial();
    let root = temp_project("g6-v6-sprite", None);
    for (method, driver) in CONFIGS {
        write_synthetic_v6_assets(&root);
        let godot = godot_at(method, driver, &root, &[]);
        let mut rpc = godot.rpc();
        open_v6(&mut rpc);
        let (info, frame) = rpc.frame(W, H);
        let red = red_pixels(&frame);
        let mut colors = std::collections::BTreeMap::<[u8; 3], usize>::new();
        for p in frame.chunks_exact(4) { *colors.entry([p[0], p[1], p[2]]).or_default() += 1; }
        let mut colors: Vec<_> = colors.into_iter().collect();
        colors.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        eprintln!("{method}/{driver}: draws={} triangles={} synthetic V6 atlas red pixels={red}; colors={:?}; host={:?}", info["draws"], info["triangles"], &colors[..colors.len().min(12)], godot.log());
        assert!(red > 100, "{method}/{driver}: native atlas sprite did not produce real red pixels ({red})");
        assert!(info["draws"].as_u64().unwrap() > 100, "V6 sprite instances must survive Canvas routing");
        // Keep the selected center red, but radically alter both adjacent atlas texels.
        // Pixels in the red terrain mask must stay identical: sampling the whole atlas
        // instead of its selected frame would change a large part of that mask.
        let atlas = root.join("Content/Animations/v6/terrain/synthetic.png");
        std::fs::write(&atlas, png(3, 1, &[240, 240, 240, 255, 220, 24, 32, 255, 0, 0, 0, 255])).unwrap();
        rpc.call("asset.reload", json!({}));
        let neighbors = rpc.frame(W, H).1;
        let altered = frame.chunks_exact(4).zip(neighbors.chunks_exact(4))
            .filter(|(a,b)| red_pixels(a) == 1 && (0..3).any(|i| a[i].abs_diff(b[i]) > 3)).count();
        assert!(altered < red / 20, "{method}/{driver}: adjacent texels contaminated {altered}/{red} red pixels");
        // Changing the selected texel must update those same terrain pixels without
        // restarting the process; this also rules out a solid-color geometry fallback.
        std::fs::write(&atlas, png(3, 1, &[240, 240, 240, 255, 24, 32, 220, 255, 0, 0, 0, 255])).unwrap();
        rpc.call("asset.reload", json!({}));
        let reloaded = rpc.frame(W, H).1;
        let blue_replacements = frame.chunks_exact(4).zip(reloaded.chunks_exact(4))
            .filter(|(a,b)| red_pixels(a) == 1 && b[2] > 100 && b[2] > b[0].saturating_add(40)).count();
        eprintln!("{method}/{driver}: neighbor contamination={altered}/{red}, reload blue replacements={blue_replacements}");
        assert!(blue_replacements > red / 2, "{method}/{driver}: selected atlas texel reload must replace real terrain pixels");
        assert!(!godot.log().iter().any(|line| line.contains("SHADER ERROR") || line.contains("Shader compilation failed")));
        drop(godot);
    }
}

/// The initial V6 snapshot contains two `core` buildings. Supplying only the
/// `command-core` model entry therefore exercises a non-ground building sprite
/// through `game.session.open`, rather than a hand-built draw record.
#[test]
#[ignore = "GPU integration; parent session runs configurations serially"]
fn v6_non_ground_model_atlas_pivot_blend_and_reopen_in_all_four_configs() {
    let _guard = serial();
    let root = temp_project("g6-v6-non-ground", None);
    for (method, driver) in CONFIGS {
        write_non_ground_assets(&root, [0.5, 0.5], "alpha");
        let godot = godot_at(method, driver, &root, &[]);
        let mut rpc = godot.rpc();
        open_v6(&mut rpc);
        let (info, alpha) = rpc.frame(W, H);
        let first = magenta_bounds(&alpha).expect("non-ground command-core atlas must produce magenta pixels");
        assert!(first.4 > 8, "{method}/{driver}: too few native non-ground pixels: {first:?}, frame={info}");
        eprintln!("{method}/{driver}: initial non-ground bounds={first:?}, draws={}", info["draws"]);
        // This fixture supplies only the two core-building atlas sprites, not
        // thousands of terrain sprites. The base geometry has at most two draws.
        assert!(info["draws"].as_u64().unwrap() > 2, "{method}/{driver}: non-ground V6 sprite must be included in draws");

        // Pivot is part of the native metadata and changes the generated quad,
        // not merely the material. The two frames must move measurably.
        std::thread::sleep(std::time::Duration::from_millis(2_100));
        write_non_ground_assets(&root, [0.5, 0.9], "alpha");
        rpc.call("asset.reload", json!({}));
        let pivoted = rpc.frame(W, H).1;
        let second = magenta_bounds(&pivoted).expect("pivoted native sprite must remain visible");
        let shift = (second.1 as i32 - first.1 as i32).abs() + (second.3 as i32 - first.3 as i32).abs();
        assert!(shift >= 1, "{method}/{driver}: pivot change was not measurable: {first:?} -> {second:?}");

        // The same real atlas texel is alpha-blended first, then additive. A
        // material/ordering change must alter actual readback pixels.
        std::thread::sleep(std::time::Duration::from_millis(2_100));
        write_non_ground_assets(&root, [0.5, 0.9], "additive");
        rpc.call("asset.reload", json!({}));
        let additive = rpc.frame(W, H).1;
        let changed = pixel_diff(&pivoted, &additive);
        assert!(changed > 0, "{method}/{driver}: alpha/additive switch did not change pixels");

        // Closing and reopening must discard the old staged/cache state while
        // keeping the real non-ground asset path active.
        rpc.call("game.session.close", json!({}));
        open_v6(&mut rpc);
        let reopened = rpc.frame(W, H).1;
        let after_reopen = magenta_bounds(&reopened).expect("reopened session lost non-ground atlas sprite");
        assert!(after_reopen.4 > 8, "{method}/{driver}: non-ground sprite missing after close/reopen");
        eprintln!("{method}/{driver}: non-ground pixels {} -> {} -> {}, pivot shift={shift}, blend diff={changed}", first.4, second.4, after_reopen.4);
        assert!(!godot.log().iter().any(|line| line.contains("SHADER ERROR") || line.contains("Shader compilation failed")));
        drop(godot);
    }
}

#[test]
fn synthetic_atlas_fixture_has_a_single_known_opaque_texel() {
    let root = temp_project("g6-v6-sprite-cpu", None);
    write_synthetic_v6_assets(&root);
    let bytes = std::fs::read(root.join("Content/Animations/v6/terrain/synthetic.png")).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert!(bytes.windows(4).any(|w| w == [220, 24, 32, 255]));
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("Content/UI/v6/resource-manifest.json")).unwrap(),
    ).unwrap();
    assert_eq!(manifest["terrain"]["nativeAtlas"], "Content/Animations/v6/terrain/synthetic.png");
    assert_eq!(manifest["terrain"]["textures"]["grass"]["materialTint"], json!([1.0, 1.0, 1.0, 1.0]));
}
