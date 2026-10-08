//! Real GPU sprite probes: synthetic atlas, Canvas and spatial paths, four render configurations.
mod common;
mod g4util;
mod g5util;
use common::{serial, Rpc};
use g4util::*;
use serde_json::{json, Value};
use std::path::Path;
const W: u32 = 320;
const H: u32 = 240;
const COLORS: [[u8; 4]; 4] = [[192,64,32,255], [32,192,64,255], [64,32,192,255], [192,160,32,255]];

// Tiny deterministic RGBA PNG writer using uncompressed DEFLATE, no external tool/dependency.
fn png(path: &Path, w: u32, h: u32, pixels: &[u8]) {
    fn chunk(out: &mut Vec<u8>, tag: &[u8;4], bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        let start = out.len(); out.extend_from_slice(tag); out.extend_from_slice(bytes);
        let mut crc = !0u32;
        for b in &out[start..] { crc ^= *b as u32; for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320u32 & 0u32.wrapping_sub(crc & 1)); } }
        out.extend_from_slice(&(!crc).to_be_bytes());
    }
    let mut raw = Vec::new();
    for row in pixels.chunks_exact((w*4) as usize) { raw.push(0); raw.extend_from_slice(row); }
    assert_eq!(raw.len(), ((w*4+1)*h) as usize); assert!(raw.len()<65536);
    let n = raw.len() as u16;
    let mut zip = vec![0x78,0x01,0x01]; zip.extend_from_slice(&n.to_le_bytes()); zip.extend_from_slice(&(!n).to_le_bytes()); zip.extend_from_slice(&raw);
    let (mut a, mut b) = (1u32,0u32); for v in raw { a=(a+v as u32)%65521; b=(b+a)%65521; }
    zip.extend_from_slice(&((b<<16)|a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new(); ihdr.extend_from_slice(&w.to_be_bytes()); ihdr.extend_from_slice(&h.to_be_bytes()); ihdr.extend_from_slice(&[8,6,0,0,0]);
    chunk(&mut out,b"IHDR",&ihdr); chunk(&mut out,b"IDAT",&zip); chunk(&mut out,b"IEND",&[]); std::fs::write(path,out).unwrap();
}
fn fixture(root: &Path) {
    let content = root.join("Content");
    let pixels = rgba(16,8,|x,y| if x>=8 { [200,40,200,255] } else { COLORS[((y>=4) as usize)*2+(x>=4) as usize] });
    png(&content.join("atlas.png"),16,8,&pixels);
    png(&content.join("blend.png"),8,8,&rgba(8,8,|_,_|[160,80,32,128]));
    std::fs::write(content.join("blend.png.meta"),"guid: g6-blend\ntype: texture\nimporter: texture\n").unwrap();
    std::fs::write(content.join("atlas.png.meta"),"guid: g6-atlas\ntype: texture\nimporter: texture\n").unwrap();
    std::fs::write(content.join("atlas.rxsprite"),json!({"version":1,"texture":"g6-atlas","pivot":[0.5,0.5],"frames":{"a":{"bbox":[0,0,8,8]},"b":{"bbox":[8,0,8,8],"pivot":[0.0,1.0]}}}).to_string()).unwrap();
    std::fs::write(content.join("atlas.rxsprite.meta"),"guid: g6-sprite\ntype: sprite\nimporter: sprite\n").unwrap();
}
fn props() -> Value { json!({"sprite":"g6-sprite","pixelsPerUnit":4.0,"chromaKey":"none","blendMode":"opaque","tint":[1,1,1,1]}) }
fn scene(r: &mut Rpc, canvas: bool, props: &Value) -> u64 {
    r.call("scene.new",json!({"name":"g6-sprite","mode":if canvas {"2d"} else {"3d"}}));
    r.call("viewport.setCamera",json!({"target":[0,0,0],"yaw":0,"pitch":0,"dist":6,"ortho":true,"orthoSize":3}));
    r.call("entity.create",json!({"name":"sprite","components":[{"type":"Sprite","enabled":true,"props":props}]}))["id"].as_u64().unwrap()
}
fn set(r: &mut Rpc,id:u64,p:&Value) {r.call("component.set",json!({"id":id,"type":"Sprite","props":p}));}
fn close_color(actual: [u8;3], expected: [u8;3], label:&str) {
    assert!((0..3).all(|i|actual[i].abs_diff(expected[i])<=3),"{label}: actual={actual:?} expected={expected:?}");
}
fn samples(p:&[u8])->[[u8;3];4] { [(140,100),(180,100),(140,140),(180,140)].map(|(x,y)|at(p,W,x,y)) }
#[test]
fn blend_sort_reload_and_selection_have_visible_effects() {
    let _lock=serial(); let root=temp_project("g6-sprite-state",None);
    for (method,driver) in CONFIGS {
        fixture(&root);
        let g=godot_at(method,driver,&root,&[]); let mut r=g.rpc();
        for canvas in [true,false] {
            let mut p=json!({"texture":"g6-blend","pixelsPerUnit":4,"chromaKey":"none","blendMode":"opaque"});
            let id=scene(&mut r,canvas,&p);
            let opaque=at(&r.frame(W,H).1,W,160,120);
            p["blendMode"]=json!("alpha"); set(&mut r,id,&p);
            let alpha=at(&r.frame(W,H).1,W,160,120);
            p["blendMode"]=json!("additive"); set(&mut r,id,&p);
            let additive=at(&r.frame(W,H).1,W,160,120);
            eprintln!("{method}/{driver} canvas={canvas}: opaque={opaque:?} alpha={alpha:?} additive={additive:?}");
            close_color(opaque,[160,80,32],"opaque ignores nonzero texture alpha");
            assert!(alpha[0]>23 && alpha[0]<opaque[0]-10,"alpha must blend");
            assert!(additive[0]>alpha[0] && additive[1]>alpha[1],"additive must add rather than mix");
            p=props(); p["blendMode"]=json!("alpha"); p["tint"]=json!([1,0,0,1]); p["sortingOrder"]=json!(10); set(&mut r,id,&p);
            let mut q=p.clone(); q["tint"]=json!([0,1,0,1]); q["sortingOrder"]=json!(0);
            let second=r.call("entity.create",json!({"name":"behind","components":[{"type":"Sprite","props":q}]}))["id"].as_u64().unwrap();
            let front=at(&r.frame(W,H).1,W,140,100);
            assert!(front[0]>150 && front[1]<5,"higher sortingOrder should be front: {front:?}");
            q["sortingOrder"]=json!(20); set(&mut r,second,&q);
            let swapped=at(&r.frame(W,H).1,W,140,100);
            assert!(swapped[0]<5 && swapped[1]>50,"sortingOrder update should swap overlap: {swapped:?}");
            r.call("entity.destroy",json!({"id":second})); set(&mut r,id,&props());
            let normal=r.frame(W,H).1;
            let selected=r.call("viewport.frame",json!({"width":W,"height":H,"selectedId":id}));
            let selected=base64::Engine::decode(&base64::engine::general_purpose::STANDARD,selected["pixelsB64"].as_str().unwrap()).unwrap();
            assert_ne!(samples(&normal),samples(&selected),"selected highlight must change actual pixels");
        }
        // All live instances/textures must refresh on explicit asset.reload, without restarting host.
        let id=scene(&mut r,true,&props()); r.frame(W,H);
        png(&root.join("Content/atlas.png"),16,8,&rgba(16,8,|_,_|[40,180,220,255]));
        r.call("asset.reload",json!({}));
        close_color(at(&r.frame(W,H).1,W,140,100),[40,180,220],"asset.reload updates cached image");
        r.call("entity.destroy",json!({"id":id}));
        close_color(at(&r.frame(W,H).1,W,140,100),[23,24,29],"reload then destroy leaves no sprite");
    }
}

#[test]
fn mixed_scene_depth_and_live_canvas_spatial_transitions() {
    let _lock = serial();
    let root = temp_project("g6-mixed-sprite", None);
    fixture(&root);
    let model = g5util::white(&root);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        let id = scene(&mut r, true, &props());
        let canvas = r.frame(W,H).1;
        r.call("viewport.setCamera", json!({"ortho":false}));
        let perspective = r.frame(W,H).1;
        close_color(samples(&perspective)[0], [192,64,32], "live switch to perspective quad");
        r.call("viewport.setCamera", json!({"ortho":true}));
        assert_eq!(r.frame(W,H).1, canvas, "camera reset must restore exact Canvas frame");
        let blocker = r.call("entity.create", json!({"name":"blocker","translation":[0,0,1],"scale":[2,2,2],
            "components":[{"type":"MeshRenderer","props":{"mesh":"cube","material":""}}]}))["id"].as_u64().unwrap();
        let hidden = r.frame(W,H).1;
        assert_ne!(samples(&hidden), samples(&canvas), "mixed 3D mesh must occlude the sprite");
        r.call("transform.set", json!({"id":blocker,"translation":[0,0,-2]}));
        let front = r.frame(W,H).1;
        close_color(samples(&front)[0], [192,64,32], "sprite in front of mixed 3D mesh");
        r.call("entity.destroy", json!({"id":blocker}));
        assert_eq!(r.frame(W,H).1, canvas, "removing mesh must return to Canvas without stale instances");
        // An actual model forces the model path, rather than merely the sprite/mesh path.
        // Place it out of view so a default environment change is visible on the sprite itself.
        let model_id = r.call("entity.create", json!({"name":"model","translation":[20,0,0],
            "components":[{"type":"ModelRenderer","props":{"model":model}}]}))["id"].as_u64().unwrap();
        let mixed_model = r.frame(W,H).1;
        eprintln!("{method}/{driver} model-path sprite sample={:?}", samples(&mixed_model));
        close_color(samples(&mixed_model)[0], [192,64,32], "default model environment retains sprite colors");
        let env_id = r.call("entity.create", json!({"name":"explicit-env", "components":[{"type":"Environment",
            "props":{"tonemap":"reinhard", "white":16.0}}]}))["id"].as_u64().unwrap();
        let toned = samples(&r.frame(W,H).1)[0];
        assert!(toned[0] < 180 && toned[0] > 140, "explicit Environment must retain tone mapping rather than compensation: {toned:?}");
        r.call("entity.destroy", json!({"id":env_id}));
        close_color(samples(&r.frame(W,H).1)[0], [192,64,32], "removing explicit environment restores compensation");
        r.call("entity.destroy", json!({"id":model_id}));
        assert_eq!(r.frame(W,H).1, canvas);
        // Canvas transform sign/rotation must agree with the spatial path.
        let s = std::f32::consts::FRAC_1_SQRT_2;
        r.call("transform.set", json!({"id":id,"rotation":[0,0,s,s]}));
        let rotated = r.frame(W,H).1;
        for (i, source) in [1,3,0,2].into_iter().enumerate() {
            close_color(samples(&rotated)[i], COLORS[source][..3].try_into().unwrap(), "Canvas quarter turn");
        }
    }
}

#[test]
fn opaque_sorting_on_canvas_and_spatial_depth() {
    let _lock = serial();
    let root = temp_project("g6-opaque-sort", None); fixture(&root);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]); let mut r = g.rpc();
        for canvas in [true, false] {
            let mut p=props(); p["tint"]=json!([1,0,0,1]); p["sortingOrder"]=json!(10);
            scene(&mut r,canvas,&p);
            let mut q=p.clone(); q["tint"]=json!([0,1,0,1]); q["sortingOrder"]=json!(0);
            let second=r.call("entity.create",json!({"name":"overlap","components":[{"type":"Sprite","props":q}]}))["id"].as_u64().unwrap();
            let first=at(&r.frame(W,H).1,W,140,100);
            q["sortingOrder"]=json!(20); set(&mut r,second,&q);
            let swapped=at(&r.frame(W,H).1,W,140,100);
            eprintln!("{method}/{driver} canvas={canvas} opaque order first={first:?} swapped={swapped:?}");
            if canvas {
                close_color(first,[192,0,0],"opaque Canvas higher sortingOrder");
                close_color(swapped,[0,64,0],"opaque Canvas sortingOrder update");
            } else {
                assert_eq!(first, swapped, "opaque spatial sortingOrder is a reported limitation; depth controls overlap");
            }
            r.call("transform.set",json!({"id":second,"translation":[0,0,1]}));
            close_color(at(&r.frame(W,H).1,W,140,100),[0,64,0],"opaque front depth wins");
            r.call("transform.set",json!({"id":second,"translation":[0,0,-1]}));
            close_color(at(&r.frame(W,H).1,W,140,100),[192,0,0],"opaque rear depth loses");
        }
    }
}

#[test]
fn canvas_glow_and_spatial_glow_follow_reported_limits() {
    let _lock = serial();
    let root = temp_project("g6-sprite-glow", None);
    fixture(&root);
    for (method, driver) in CONFIGS {
        let g = godot_at(method, driver, &root, &[]);
        let mut r = g.rpc();
        for canvas in [true, false] {
            scene(&mut r, canvas, &props());
            let mut env = json!({"background":"color", "backgroundColor":[0,0,0,1],
                "tonemap":"linear", "glowEnabled":false, "glowBloom":1.0,
                "glowHdrThreshold":0.0, "glowIntensity":2.0, "glowStrength":1.0,
                "glowBlendMode":"additive", "glowLevel1":1.0, "glowLevel2":1.0});
            let id = r.call("entity.create", json!({"name":"env", "components":[{"type":"Environment", "props":env}]}))["id"].as_u64().unwrap();
            for _ in 0..8 { r.frame(W,H); }
            let off = r.frame(W,H).1;
            env["glowEnabled"] = json!(true);
            r.call("component.set", json!({"id":id,"type":"Environment","props":env}));
            for _ in 0..8 { r.frame(W,H); }
            let on = r.frame(W,H).1;
            let changed = off.chunks_exact(4).zip(on.chunks_exact(4)).filter(|(a,b)| a != b).count();
            eprintln!("{method}/{driver} Canvas={canvas} glow changed pixels={changed}");
            if canvas { assert_eq!(changed, 0, "Canvas does not currently enter Environment glow"); }
            else { assert!(changed > 100, "spatial sprites must actually enter Environment glow"); }
            env["glowEnabled"] = json!(false);
            r.call("component.set", json!({"id":id,"type":"Environment","props":env}));
            for _ in 0..8 { r.frame(W,H); }
            assert_eq!(r.frame(W,H).1, off, "glow reset must restore original output");
        }
        assert!(!g.log().iter().any(|line| line.contains("SHADER ERROR") || line.contains("未知属性")), "{:?}", g.log());
    }
}

#[test]
fn atlas_crop_flip_tint_and_frame_pivot_render_on_canvas_and_spatial() {
    let _lock=serial(); let root=temp_project("g6-sprite",None); fixture(&root);
    for (method,driver) in CONFIGS {
        let g=godot_at(method,driver,&root,&[]); let mut r=g.rpc();
        for (canvas, ortho) in [(true,true),(false,true),(false,false)] {
            let mut p=props(); let id=scene(&mut r,canvas,&p);
            r.call("viewport.setCamera",json!({"ortho":ortho}));
            let (info,base)=r.frame(W,H); eprintln!("{method}/{driver} canvas={canvas} probes={:?} draws={}",samples(&base),info["draws"]);
            assert_eq!(info["draws"],1);
            for i in 0..4 { close_color(samples(&base)[i],COLORS[i][..3].try_into().unwrap(),&format!("{method}/{driver} canvas={canvas} quadrant={i}")); }
            for (fx,fy) in [(true,false),(false,true),(true,true)] {
                p["flipX"]=json!(fx); p["flipY"]=json!(fy); set(&mut r,id,&p);
                let (_,pixels)=r.frame(W,H);
                for i in 0..4 { let source=i ^ usize::from(fx) ^ (usize::from(fy)*2); close_color(samples(&pixels)[i],COLORS[source][..3].try_into().unwrap(),"flip within crop"); }
            }
            p["flipX"]=json!(false); p["flipY"]=json!(false); p["tint"]=json!([0.5,0.75,1,1]); set(&mut r,id,&p);
            let (_,tinted)=r.frame(W,H); close_color(samples(&tinted)[0],[96,48,32],"tint in encoded color space");
            p["tint"]=json!([1,1,1,1]); p["frame"]=json!(1); set(&mut r,id,&p);
            let (_,pivot)=r.frame(W,H); close_color(at(&pivot,W,180,100),[200,40,200],"new frame / bottom-left pivot");
            close_color(at(&pivot,W,140,140),[23,24,29],"old centered frame removed");
            p["chromaKey"]=json!("magenta"); set(&mut r,id,&p);
            close_color(at(&r.frame(W,H).1,W,180,100),[23,24,29],"chroma key removes magenta frame");
            p["chromaKey"]=json!("none"); set(&mut r,id,&p);
            close_color(at(&r.frame(W,H).1,W,180,100),[200,40,200],"chroma key toggle restores frame");
            r.call("entity.destroy",json!({"id":id})); let (empty,px)=r.frame(W,H);
            assert_eq!(empty["draws"],0); close_color(at(&px,W,180,100),[23,24,29],"destroy releases instance");
        }
        assert!(!g.log().iter().any(|line| line.contains("Shader compilation failed") || line.contains("SHADER ERROR")),"{:?}",g.log());
    }
}
