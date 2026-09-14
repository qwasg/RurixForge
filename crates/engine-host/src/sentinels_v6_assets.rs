//! Native frame lookup and bounded per-frame texture cache inputs. Atlas files
//! are never stretched as whole sheets or substituted with generated portraits.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};
#[derive(Clone, Serialize, Deserialize)]
pub struct Sprite {
    pub asset: String,
    pub action: String,
    pub direction: usize,
    pub seconds: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub scale: f64,
    pub tint: [f32; 4],
    #[serde(default)]
    pub rotation: f32,
    #[serde(default)]
    pub ground: bool,
    #[serde(default)]
    pub entity: u64,
    #[serde(default)]
    pub owner: u32,
    #[serde(default)]
    pub entity_kind: String,
    #[serde(default)]
    pub entity_pos: Option<sentinels_v6::Pos>,
    #[serde(default)]
    pub transient: bool,
}
#[derive(Clone)]
pub struct Frame {
    pub index:usize,
    pub metadata:String,
    pub key: String,
    pub atlas: String,
    pub bbox: [u32; 4],
    pub pivot: [f32; 2],
    pub span: f32,
    pub duration: f64,
    pub effect: bool,
    pub additive: bool,
    pub tile_size: u32,
    pub sample_size: [u32; 2],
}
struct Manifest {
    at: Instant,
    value: Arc<Value>,
}
static MANIFEST: OnceLock<Mutex<Manifest>> = OnceLock::new();
fn manifest() -> Arc<Value> {
    let mut m = MANIFEST
        .get_or_init(|| {
            Mutex::new(Manifest {
                at: Instant::now() - Duration::from_secs(10),
                value: Arc::new(Value::Null),
            })
        })
        .lock()
        .unwrap();
    if m.at.elapsed() > Duration::from_secs(2) {
        let path = crate::rpc::project_root().join("Content/UI/v6/resource-manifest.json");
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(value) = serde_json::from_slice(&bytes) {
                m.value = Arc::new(value);
            }
        }
        m.at = Instant::now();
    }
    m.value.clone()
}
pub fn available(asset: &str) -> bool {
    let m = manifest();
    if let Some(fx) = asset.strip_prefix("fx:") {
        return m["effects"][fx]["ready"] == true;
    }
    (asset.starts_with("terrain:") && m["terrain"]["nativeAtlas"].is_string())
        || m["models"][asset]["nativeAtlas"].is_string()
        || m["characters"][asset]["ready"] == true
        || m["effects"][asset]["ready"] == true
}
fn paths(asset: &str) -> Option<(String, String)> {
    resolve_paths(&manifest(), asset)
}
fn effect_asset(m: &Value, asset: &str) -> bool {
    asset.starts_with("fx:")
        || !m["models"][asset].is_object()
            && !m["characters"][asset].is_object()
            && m["effects"][asset].is_object()
}
fn resolve_paths(m: &Value, asset: &str) -> Option<(String, String)> {
    if let Some(fx) = asset.strip_prefix("fx:") {
        let value = &m["effects"][fx];
        if value["ready"] != true {
            return None;
        }
        return Some((
            value["nativeAtlas"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Content/Animations/v6/effects/{fx}.png")),
            value["nativeMetadata"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Content/Animations/v6/effects/{fx}.json")),
        ));
    }
    if asset.starts_with("terrain:") {
        return Some((
            m["terrain"]["nativeAtlas"].as_str()?.into(),
            m["terrain"]["nativeMetadata"].as_str()?.into(),
        ));
    }
    for section in ["models", "characters", "effects"] {
        let value = &m[section][asset];
        if value.is_null() {
            continue;
        }
        if section != "models" && value["ready"] != true {
            return None;
        }
        if let (Some(atlas), Some(meta)) = (
            value["nativeAtlas"].as_str(),
            value["nativeMetadata"].as_str(),
        ) {
            return Some((atlas.into(), meta.into()));
        }
        if section == "effects" {
            return Some((
                format!("Content/Animations/v6/effects/{asset}.png"),
                format!("Content/Animations/v6/effects/{asset}.json"),
            ));
        }
        if section == "characters" {
            return Some((
                format!("Content/Animations/v6/characters/{asset}.png"),
                format!("Content/Animations/v6/characters/{asset}.json"),
            ));
        }
    }
    None
}
struct CachedMetadata {
    at: Instant,
    stamp: SystemTime,
    doc: Arc<Value>,
}
type MetaCache = BTreeMap<String, CachedMetadata>;
static METADATA: OnceLock<Mutex<MetaCache>> = OnceLock::new();
pub fn describe(sprite: &Sprite) -> Result<Option<Frame>, String> {
    let Some((atlas, metadata)) = paths(&sprite.asset) else {
        return Ok(None);
    };
    let root = crate::rpc::project_root();
    let path = root.join(&metadata);
    let (stamp, doc) = {
        let mut cache = METADATA
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .unwrap();
        if cache
            .get(&metadata)
            .is_none_or(|v| v.at.elapsed() > Duration::from_secs(2))
        {
            let stamp = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .map_err(|e| format!("V6 {} metadata unavailable: {e}", sprite.asset))?;
            if cache.get(&metadata).is_none_or(|v| v.stamp != stamp) {
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                let value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                cache.insert(
                    metadata.clone(),
                    CachedMetadata {
                        at: Instant::now(),
                        stamp,
                        doc: Arc::new(value),
                    },
                );
            } else {
                cache.get_mut(&metadata).unwrap().at = Instant::now();
            }
        }
        let item = &cache[&metadata];
        (item.stamp, item.doc.clone())
    };
    let boxes = doc["boxes"]
        .as_array()
        .or_else(|| doc["frames"].as_array())
        .ok_or_else(|| format!("{} has no frame rectangles", sprite.asset))?;
    let dirs = ["s", "sw", "w", "nw", "n", "ne", "e", "se"];
    let clips = &doc["clips"];
    let desired = &clips[&sprite.action];
    let direction = if sprite.direction < 8 {
        dirs[sprite.direction]
    } else {
        doc["defaultDirection"].as_str().unwrap_or("n")
    };
    let terrain_frame = sprite.asset.strip_prefix("terrain:").and_then(|name| {
        doc["keys"]
            .as_array()
            .and_then(|keys| keys.iter().position(|v| v.as_str() == Some(name)))
    });
    let terrain_clip = serde_json::json!({"start":terrain_frame.unwrap_or(0),"endExclusive":terrain_frame.unwrap_or(0)+1,"fps":1,"loop":true});
    let clip = if terrain_frame.is_some() {
        &terrain_clip
    } else if desired[direction].is_object() {
        &desired[direction]
    } else if desired["start"].is_number() {
        desired
    } else if clips["idle"][direction].is_object()
        && sprite.asset != "gemini"
        && sprite.asset != "claude"
        && sprite.asset != "kimi"
        && sprite.asset != "minimax"
        && sprite.asset != "glm"
        && sprite.asset != "deepseek"
        && sprite.asset != "gpt"
    {
        &clips["idle"][direction]
    } else if clips["oneshot"].is_object() {
        &clips["oneshot"]
    } else {
        return Err(format!(
            "{} missing actual {}/{} frames",
            sprite.asset, sprite.action, direction
        ));
    };
    let start = clip["start"].as_u64().unwrap_or(0) as usize;
    let end = clip["endExclusive"].as_u64().unwrap_or(boxes.len() as u64) as usize;
    if end <= start || end > boxes.len() {
        return Err(format!("{} clip range invalid", sprite.asset));
    }
    let age = (sprite.seconds.max(0.) * clip["fps"].as_f64().unwrap_or(16.)).floor() as usize;
    let frame = start
        + if clip["loop"] == true {
            age % (end - start)
        } else {
            age.min(end - start - 1)
        };
    let b = boxes[frame].as_array().ok_or("invalid atlas frame")?;
    if b.len() != 4 {
        return Err("invalid frame size".into());
    }
    let bbox = std::array::from_fn(|i| b[i].as_u64().unwrap_or(0) as u32);
    if bbox[2] == 0 || bbox[3] == 0 || bbox[2] > 4096 || bbox[3] > 4096 {
        return Err(format!("{} invalid frame extent", sprite.asset));
    }
    let (pivot, span) = clip_geometry(clip, &doc)?;
    let preserve = doc["preservePixelDensity"] == true;
    if preserve && (bbox[2] > 512 || bbox[3] > 512) {
        return Err(format!(
            "{} needs a frame larger than the lossless512 cache",
            sprite.asset
        ));
    }
    let sample_size = if preserve {
        [bbox[2], bbox[3]]
    } else {
        [256, 256]
    };
    let tile_size = if sample_size[0] > 256 || sample_size[1] > 256 {
        512
    } else {
        256
    };
    Ok(Some(Frame {
        index:frame,
        metadata,
        key: format!("{}:{}:{:?}", atlas, frame, stamp),
        atlas,
        bbox,
        pivot,
        span,
        duration: (end - start) as f64 / clip["fps"].as_f64().unwrap_or(16.).max(0.01),
        effect: effect_asset(&manifest(), &sprite.asset),
        additive: doc["blend"] == "additive",
        tile_size,
        sample_size,
    }))
}
pub fn animation_duration(asset: &str, action: &str, direction: usize) -> Option<f64> {
    describe(&Sprite {
        asset: asset.into(),
        action: action.into(),
        direction,
        seconds: 0.,
        x: 0.,
        y: 0.,
        z: 0.,
        scale: 1.,
        tint: [1.; 4],
        rotation: 0.,
        ground: false,
        entity: 0,
        owner: 0,
        entity_kind: String::new(),
        entity_pos: None,
        transient: false,
    })
    .ok()
    .flatten()
    .map(|f| f.duration)
}
/// Action clips may use a larger source canvas for a fall without moving the
/// world-space foot anchor or shrinking the preceding standing animation.
fn clip_geometry(clip: &Value, doc: &Value) -> Result<([f32; 2], f32), String> {
    let value = if clip["pivot"].is_array() {
        &clip["pivot"]
    } else {
        &doc["pivot"]
    };
    let pivot = [
        value[0].as_f64().unwrap_or(0.5),
        value[1].as_f64().unwrap_or(0.88),
    ];
    let span = clip["nativePlaneSpan"]
        .as_f64()
        .or_else(|| doc["nativePlaneSpan"].as_f64())
        .unwrap_or(2.5456);
    if pivot
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        || !span.is_finite()
        || !(0.01..=100.).contains(&span)
    {
        return Err("invalid atlas clip pivot or world span".into());
    }
    Ok(([pivot[0] as f32, pivot[1] as f32], span as f32))
}
type SourceCache = BTreeMap<String, (SystemTime, u32, u32, Arc<Vec<u8>>)>;
static SOURCES: OnceLock<Mutex<SourceCache>> = OnceLock::new();
pub fn clear_caches() {
    crate::sentinels_v6_pages::clear();
    if let Some(cache) = SOURCES.get() {
        cache.lock().unwrap().clear();
    }
    if let Some(cache) = METADATA.get() {
        cache.lock().unwrap().clear();
    }
    if let Some(cache) = MANIFEST.get() {
        let mut value = cache.lock().unwrap();
        value.value = Arc::new(Value::Null);
        value.at = Instant::now() - Duration::from_secs(10);
    }
}
pub fn pixels(frame: &Frame) -> Result<Vec<u8>, String> {
    if let Some(page)=crate::sentinels_v6_pages::pixels(&crate::rpc::project_root(),frame)?{return Ok(page);}
    let path = crate::rpc::project_root().join(&frame.atlas);
    let stamp = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map_err(|e| e.to_string())?;
    let (w, h, data) = {
        let mut sources = SOURCES
            .get_or_init(|| Mutex::new(BTreeMap::new()))
            .lock()
            .unwrap();
        if sources.get(&frame.atlas).map(|(s, _, _, _)| *s) != Some(stamp) {
            let (w, h, rgba) = assetd::texture::decode_rgba(&path).map_err(|e| e.to_string())?;
            sources.insert(frame.atlas.clone(), (stamp, w, h, Arc::new(rgba)));
        }
        let (_, w, h, data) = &sources[&frame.atlas];
        (*w, *h, data.clone())
    };
    raster_crop(frame, w, h, &data)
}
fn raster_crop(frame: &Frame, w: u32, h: u32, data: &[u8]) -> Result<Vec<u8>, String> {
    let [x, y, bw, bh] = frame.bbox;
    if x.checked_add(bw).is_none_or(|n| n > w)
        || y.checked_add(bh).is_none_or(|n| n > h)
        || data.len() != w as usize * h as usize * 4
    {
        return Err("frame exceeds decoded image".into());
    }
    let side = frame.tile_size as usize;
    let sw = frame.sample_size[0] as usize;
    let sh = frame.sample_size[1] as usize;
    if !matches!(side, 256 | 512) || sw == 0 || sh == 0 || sw > side || sh > side {
        return Err("frame sample dimensions exceed its page".into());
    }
    let mut out = vec![0; side * side * 4];
    for row in 0..sh {
        for col in 0..sw {
            let offset = ((y as usize + row * bh as usize / sh) * w as usize
                + x as usize
                + col * bw as usize / sw)
                * 4;
            out[(row * side + col) * 4..(row * side + col + 1) * 4]
                .copy_from_slice(&data[offset..offset + 4]);
        }
    }
    Ok(out)
}
pub fn direction(dx: f64, dy: f64) -> usize {
    (((dy.atan2(dx) - std::f64::consts::FRAC_PI_4) / std::f64::consts::FRAC_PI_4).round() as i32)
        .rem_euclid(8) as usize
}
pub fn terrain_tint(key: &str) -> [f32; 4] {
    let m = manifest();
    let value = &m["terrain"]["textures"][key]["materialTint"];
    std::array::from_fn(|i| value[i].as_f64().unwrap_or(1.) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn derived_pages_match_original_raster_crop_for_all_characters_and_effects(){
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../projects/code-sentinels");
        let index=crate::sentinels_v6_pages::load_index(&root).unwrap().expect("actual derived index required for this asset oracle");
        let start=Instant::now();let mut character_frames=0usize;let mut effect_frames=0usize;let mut rows=Vec::new();
        for (atlas,entry) in &index.atlases{
            let source_start=Instant::now();
            let doc:Value=serde_json::from_slice(&std::fs::read(root.join(&entry.metadata)).unwrap()).unwrap();
            let boxes=doc["boxes"].as_array().or_else(||doc["frames"].as_array()).unwrap();
            let (width,height,rgba)=assetd::texture::decode_rgba(&root.join(atlas)).unwrap();
            assert_eq!((width,height),(entry.width,entry.height));assert_eq!(boxes.len(),entry.frames.len());
            for (i,box_value) in boxes.iter().enumerate(){
                let bbox=std::array::from_fn(|k|box_value[k].as_u64().unwrap() as u32);
                let sample_size=if doc["preservePixelDensity"]==true{[bbox[2],bbox[3]]}else{[256,256]};
                let frame=Frame{index:i,metadata:entry.metadata.clone(),key:String::new(),atlas:atlas.clone(),bbox,pivot:[0.5,0.88],span:1.,duration:1.,effect:false,additive:false,tile_size:if sample_size.iter().any(|n|*n>256){512}else{256},sample_size};
                let expected=raster_crop(&frame,width,height,&rgba).unwrap();
                let actual=crate::sentinels_v6_pages::pixels(&root,&frame).unwrap().unwrap();
                assert_eq!(actual,expected,"complete native RGBA including transparent RGB and padding: {atlas} frame{i}");
                let cached=crate::sentinels_v6_pages::pixels(&root,&frame).unwrap().unwrap();
                assert_eq!(cached,expected,"cached native RGBA must also remain exact: {atlas} frame{i}");
            }
            if atlas.contains("/characters/"){character_frames+=boxes.len();}else{effect_frames+=boxes.len();}
            let row=serde_json::json!({"atlas":atlas,"frames":boxes.len(),"allRgbaBytesEqual":true,"seconds":source_start.elapsed().as_secs_f64()});eprintln!("{row}");rows.push(row);
        }
        assert_eq!(character_frames,3584);assert_eq!(effect_frames,704);assert_eq!(index.atlases.len(),23);
        let report=serde_json::json!({"kind":"native-raster-page-oracle","actualRustExecution":true,"passed":true,"characters":character_frames,"effects":effect_frames,"totalFrames":character_frames+effect_frames,"seconds":start.elapsed().as_secs_f64(),"scope":"Existing Rust raster_crop on original source atlas and metadata versus actual production derived-page consumer. Entire RGBA pages equal, including transparent RGB and512 padding. CPU correctness, not GPU/performance acceptance.","atlases":rows});
        if let Ok(path)=std::env::var("FORGE_V6_PAGE_ORACLE_REPORT"){std::fs::write(path,serde_json::to_vec_pretty(&report).unwrap()).unwrap();}
        eprintln!("{report}");
    }
    #[test]
    fn dense_character_crop_copies_every_original_pixel_without_resizing() {
        let (width, height) = (330usize, 318usize);
        let mut source = vec![0u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let p = (y * width + x) * 4;
                source[p..p + 4].copy_from_slice(&[
                    (x % 256) as u8,
                    (y % 256) as u8,
                    ((x + y) % 251) as u8,
                    255,
                ]);
            }
        }
        let f = Frame {
            index:0,
            metadata:String::new(),
            key: String::new(),
            atlas: String::new(),
            bbox: [3, 5, 320, 300],
            pivot: [0.5, 0.5],
            span: 4.,
            duration: 1.,
            effect: false,
            additive: false,
            tile_size: 512,
            sample_size: [320, 300],
        };
        let result = raster_crop(&f, width as u32, height as u32, &source).unwrap();
        for y in 0..300 {
            for x in 0..320 {
                let actual = (y * 512 + x) * 4;
                let original = ((y + 5) * width + x + 3) * 4;
                assert_eq!(&result[actual..actual + 4], &source[original..original + 4]);
            }
        }
        assert!(result[(300 * 512) * 4..].iter().all(|b| *b == 0));
    }
    #[test]
    fn explicit_effect_namespace_does_not_collide_with_a_weapon_alias() {
        let m = serde_json::json!({"models":{"orbital-strike":{"nativeAtlas":"units/orbital-lance.png","nativeMetadata":"units/orbital-lance.json"}},"effects":{"orbital-strike":{"ready":true,"nativeAtlas":"effects/orbital-strike.png","nativeMetadata":"effects/orbital-strike.json"}}});
        assert_eq!(
            resolve_paths(&m, "orbital-strike").unwrap().1,
            "units/orbital-lance.json"
        );
        assert_eq!(
            resolve_paths(&m, "fx:orbital-strike").unwrap().1,
            "effects/orbital-strike.json"
        );
        assert!(!effect_asset(&m, "orbital-strike"));
        assert!(effect_asset(&m, "fx:orbital-strike"));
    }
    #[test]
    fn action_geometry_overrides_preserve_other_actions() {
        let doc = serde_json::json!({"pivot":[0.5,0.88],"nativePlaneSpan":3.0});
        let fall = serde_json::json!({"pivot":[0.5,0.5],"nativePlaneSpan":4.0});
        assert_eq!(clip_geometry(&fall, &doc).unwrap(), ([0.5, 0.5], 4.0));
        assert_eq!(
            clip_geometry(&Value::Null, &doc).unwrap(),
            ([0.5, 0.88], 3.0)
        );
        assert!(clip_geometry(&serde_json::json!({"nativePlaneSpan":-1}), &doc).is_err());
    }
    #[test]
    fn directions_match_isometric_screen_labels() {
        for (index, (x, y)) in [
            (1., 1.),
            (0., 1.),
            (-1., 1.),
            (-1., 0.),
            (-1., -1.),
            (0., -1.),
            (1., -1.),
            (1., 0.),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(direction(x, y), index);
        }
    }
}
