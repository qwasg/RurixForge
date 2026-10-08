//! Stage 4 step 9:同一场景连续取帧的稳定性(每个配置、载入后连取 4 帧,逐帧与第 1 帧比)。
//! Godot 4.4+ 编译专用管线时先用 ubershader 顶上,专用管线编好后换过去;Mobile 下两条路径差 1 LSB(约 1% 像素),
//! 所以 host.rs 在 Mobile、内容刚变的帧先不交付、再画一帧(预热帧)。这里锁定"载入后第 1 帧起逐字节稳定"。
mod common;
mod g4util;

use serde_json::json;

use common::{serial, sha256};
use g4util::*;

#[test]
fn consecutive_frames_after_scene_load() {
    let _g = serial();
    let demo = common::repo_root().join("projects").join("demo");
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &demo, &[]);
        let mut r = g.rpc();
        for scene in ["Content/Scenes/maze.rxscene", "Content/Scenes/pz_mvp_phase1.rxscene"] {
            r.call("scene.load", json!({ "path": scene }));
            let frames: Vec<Vec<u8>> = (0..4).map(|_| r.frame(320, 180).1).collect();
            let hashes: Vec<String> = frames.iter().map(|f| sha256(f)[..12].to_string()).collect();
            let diffs: Vec<(u8, f64)> = frames.iter().map(|f| { let (m, mean, _) = stats(&frames[0], f); (m, mean) }).collect();
            let last = &frames[3];
            let settled = frames.iter().position(|f| f == last).unwrap();
            eprintln!("{method}/{driver} {scene}: hashes={hashes:?} vs-first={diffs:?} settled-from-frame={settled}");
            // 判据:载入后第 1 帧起逐字节稳定(Mobile 靠 host.rs 的预热帧;没有预热时 Mobile 首帧差 1 LSB、约 1% 像素)。
            assert_eq!(settled, 0, "{method}/{driver} {scene}: 载入后第 1 帧起应逐字节稳定 {hashes:?} {diffs:?}");
        }
    }
}
