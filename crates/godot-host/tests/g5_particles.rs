//! Stage 5 step 6(01 §5.6、02 §9.5 Stage 5):ParticleEmitter 样式预设 kind 1-4 按 rurix gpu_particles 的解析式复刻。
//! FORGE_GPU_PARTICLES=1 下两后端同一场景并排:每种样式、每个年龄取 rurix 与 Godot 各一帧,Godot 再取一帧验证确定性;
//! 差值(最大 / 平均 / >2 的比例)与亮点数写进 evidence(02 §9.5 Stage 5 的表由它生成)。
mod common;
mod g4util;
mod g5util;

use serde_json::{json, Value};

use common::{serial, sha256, Rpc};
use g4util::{godot_at, stats, temp_project, CONFIGS};
use g5util::*;

const AGES: [f32; 3] = [0.15, 0.5, 0.9];
const LIFE: f32 = 1.2;

fn emitters(r: &mut Rpc, kinds: &[(f32, [f32; 3])], age: f32) {
    r.call("scene.new", json!({ "name": "g5-particles" }));
    for (i, (kind, c)) in kinds.iter().enumerate() {
        r.call("entity.create", json!({ "name": format!("p{i}"), "translation": c, "scale": [age, LIFE, kind],
            "components": [{ "type": "ParticleEmitter", "enabled": true, "props": {} }] }));
    }
    r.call("viewport.setCamera", json!({ "target": [0.0, 0.0, 0.0], "yaw": 0.0, "pitch": 0.0, "dist": 10.0, "ortho": true, "orthoSize": 3.0 }));
}

fn bright(px: &[u8]) -> usize {
    px.chunks_exact(4).filter(|p| p[0] > 60 || p[1] > 60 || p[2] > 60).count()
}

/// (样式, 中心) 的发射器表。
type Emitters = Vec<(f32, [f32; 3])>;

#[test]
fn particle_styles_are_deterministic_and_match_rurix() {
    let _g = serial();
    let root = temp_project("g5-particles", None);
    let mut cases: Vec<(String, Emitters)> = (1..=4).map(|k| (format!("kind{k}"), vec![(k as f32, [-1.5, 0.0, 0.0])])).collect();
    cases.push(("all4".into(), vec![(1.0, [-2.5, 1.0, 0.0]), (2.0, [0.5, 1.0, 0.0]), (3.0, [-2.5, -1.2, 0.0]), (4.0, [1.0, -1.2, 0.0])]));
    let rurix = RurixEnv::start(&root, &[("FORGE_GPU_PARTICLES", "1")]);
    let mut rr = rurix.rpc();
    let mut refs = Vec::new();
    for (name, ks) in &cases {
        for age in AGES {
            emitters(&mut rr, ks, age);
            refs.push((name.clone(), age, rr.frame(W, H).1));
        }
    }
    drop(rurix);
    let mut rows: Vec<Value> = Vec::new();
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[("FORGE_GPU_PARTICLES", "1")]);
        let mut r = g.rpc();
        let mut i = 0;
        for (name, ks) in &cases {
            for age in AGES {
                emitters(&mut r, ks, age);
                let (f1, a) = r.frame(W, H);
                let b = r.frame(W, H).1;
                assert_eq!(sha256(&a), sha256(&b), "{method}/{driver} {name} age {age}:有粒子的帧两次取帧应逐字节相同");
                let rp = &refs[i].2;
                i += 1;
                let (max, mean, gt2) = stats(rp, &a);
                let (nr, ng) = (bright(rp), bright(&a));
                eprintln!("G5P {method}/{driver} {name} age={age} max={max} mean={mean:.3} gt2={gt2:.4} bright rurix={nr} godot={ng} draws={}", f1["draws"]);
                rows.push(json!({ "config": format!("{method}/{driver}"), "case": name, "age": age, "max": max, "mean": mean,
                                  "gt2": gt2, "brightRurix": nr, "brightGodot": ng }));
                assert!(nr > 20 && ng > 20, "{method}/{driver} {name} age {age}:两边都应看得见粒子(rurix {nr}、godot {ng})");
                let ratio = ng as f64 / nr as f64;
                assert!((0.8..=1.25).contains(&ratio), "{method}/{driver} {name} age {age}:亮点数 godot/rurix = {ratio:.3}");
                // 实测(y 修正后):四种配置 max ≤ 3、mean ≤ 0.012;留一个数量级的余量。
                let lim = 0.1 * ks.len() as f64;
                assert!(mean <= lim, "{method}/{driver} {name} age {age}:全帧平均差 {mean:.3} > {lim}");
            }
        }
        drop(g);
    }
    let dir = common::repo_root().join("evidence").join("godot-backend").join("stage5");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("particles.json"), serde_json::to_vec_pretty(&Value::Array(rows)).unwrap()).unwrap();
}
