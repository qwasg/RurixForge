//! D-045:复刻素材生产(`design_assets`)——干净底图 → 切图抠底 → 单体重绘 → 入库。
//!
//! 顺序固定:干净底图先做,切图的差分抠图要拿它当「背景参照」(定稿与底图逐像素差 = 前景)。
//! 全部产物先写流程目录 `elements/<id>.png`(审阅与验收看它),再经 gend 的 accept 链入库到
//! `Content/Designs/<slug>/`,provenance 一律 `gen-image`(切图也是 AI 定稿的衍生物,I-7),
//! `detail.op` 区分 cleanplate / crop / regen。

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};

use gend::matte;
use gend::video_frames::{ChromaKey, Frame};

use super::layout::{self, Element, Layout};
use super::{compare, data_url, DesignRuntime, DesignState};
use crate::llm::ToolFeedback;
use crate::ultraplan::{read_json, write_json_atomic};
use crate::AppState;

/// 干净底图蒙版外扩像素(覆盖抗锯齿边与投影)。
const PLATE_MASK_PAD: i64 = 6;
/// 差分抠图容差与羽化带。
const DIFF_TOL: u8 = 18;
const DIFF_FEATHER: u8 = 28;
/// 抠完几乎全透明 = 抠坏(元素与背景同色,或底图没抹干净)。
const MIN_OPAQUE_RATIO: f64 = 0.02;

/// The CPU image pipeline remains blocking; native inference runs on the host
/// runtime and retains the run's cancellation and Codex authentication context.
struct NativeEditor {
    state: Arc<AppState>,
    sid: String,
    run_id: String,
    project_root: std::path::PathBuf,
    runtime: tokio::runtime::Handle,
}

/// PNG 字节 → 暂存 → accept 入库,返回 (Content 相对路径, GUID)。
pub fn accept_png(project_root: &Path, png: &[u8], dest_folder: &str, name: &str, detail: Value) -> Result<(String, String), String> {
    let project = assetd::project::ForgeProject::load(project_root).map_err(|e| e.message)?;
    let seed = gend::fnv1a64(png);
    let file_ref = gend::tmpstore::save_candidate(&project, png, seed, 0, &detail).map_err(|e| e.to_string())?;
    let acc = gend::accept::accept_asset(&project, &file_ref, dest_folder, name, "gen-image", detail, None)
        .map_err(|e| e.to_string())?;
    Ok((acc.asset_path, acc.guid))
}

fn aspect_for(w: u32, h: u32) -> gend::backends::Aspect {
    use gend::backends::Aspect;
    if w * 10 > h * 12 {
        Aspect::Landscape
    } else if h * 10 > w * 12 {
        Aspect::Portrait
    } else {
        Aspect::Square
    }
}

fn edit_once(prompt: String, images: Vec<Vec<u8>>, mask: Option<Vec<u8>>, aspect: gend::backends::Aspect, background: Option<&str>, native: Option<&NativeEditor>) -> Result<(String, Vec<u8>), String> {
    if let Some(native) = native {
        let mut candidates = native.runtime.block_on(crate::codex::imagegen::generate(
            &native.state, &native.sid, &native.run_id, &native.project_root,
            crate::codex::imagegen::Request {
                prompt, negative_prompt: None, images, mask, aspect, n: 1,
                quality: None, background: background.map(str::to_string),
            },
        )).map_err(|e| e.0)?;
        let candidate = candidates.drain(..).next().ok_or("Codex 原生改图没有返回图片")?;
        return Ok((crate::codex::imagegen::BACKEND_ID.into(), candidate.png_bytes));
    }
    let cfg = gend::config::GenConfig::load();
    let keys = gend::keystore::Keystore::load();
    let backend = gend::backends::resolve_backend(None, "img2img", &cfg, &keys).map_err(|e| format!("{}: {}", e.code, e.message))?;
    let seed = gend::hash_parts(&[prompt.as_bytes(), &images[0]]);
    let req = gend::backends::EditRequest {
        prompt,
        images,
        mask,
        aspect,
        seed,
        n: 1,
        quality: None,
        background: background.map(str::to_string),
    };
    let mut out = backend.edit(&req, &cfg, &keys).map_err(|e| format!("{}: {}", e.code, e.message))?;
    let first = out.drain(..).next().ok_or("改图后端没有返回图片")?;
    Ok((backend.id().to_string(), first.png_bytes))
}

/// 单个元素的生产结果。
#[derive(Debug, Clone)]
struct Produced {
    id: String,
    op: String,
    frame: Frame,
    backend: Option<String>,
    quality: Option<matte::MatteQuality>,
    warning: Option<String>,
}

fn plate_prompt() -> String {
    "Remove every UI element, button, icon, panel, label and piece of text inside the transparent mask area, \
and repaint that area as the plain underlying background so it blends seamlessly with the surrounding scenery, \
matching its colors, lighting, texture and perspective. Do not add any new objects or text. Do not change anything outside the mask."
        .to_string()
}

fn regen_prompt(e: &Element) -> String {
    let what = e.regen_prompt.clone().unwrap_or_else(|| format!("this {}", e.kind));
    format!(
        "Redraw only {what} from the reference image as a single isolated game asset: same art style, colors, shape, \
proportions and details as the reference, complete and unoccluded, centered, on a fully transparent background. \
No text, no other elements, no frame, no drop shadow outside the object."
    )
}

/// 同步生产(在 spawn_blocking 里跑;生图是阻塞 HTTP)。
fn produce(approved_png: &[u8], layout: &Layout, only: Option<&[String]>, dir: &Path, native: Option<&NativeEditor>) -> (Vec<Produced>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mockup = match matte::decode(approved_png) {
        Ok(f) => f,
        Err(e) => return (out, vec![format!("定稿解码失败: {e}")]),
    };
    let (w, h) = (mockup.width, mockup.height);
    let wanted = |e: &Element| only.is_none_or(|ids| ids.iter().any(|i| i == &e.id));
    let bg = layout.elements.iter().find(|e| e.kind == "background").expect("校验保证恰有一个背景");
    // 1. 背景。只重做部分元素时,干净底图从上次的产物读回(切图的差分抠图仍需要它)。
    let mut plate: Option<Frame> = None;
    if wanted(bg) {
        match bg.source.as_str() {
            "cleanplate" => {
                let rects: Vec<[i64; 4]> = layout
                    .elements
                    .iter()
                    .filter(|e| e.kind != "background")
                    .map(|e| e.bbox.map(i64::from))
                    .collect();
                if rects.is_empty() {
                    plate = Some(mockup.clone());
                    out.push(Produced { id: bg.id.clone(), op: "cleanplate".into(), frame: mockup.clone(), backend: None, quality: None, warning: None });
                } else {
                    let mask = matte::edit_mask(w, h, &rects, PLATE_MASK_PAD);
                    let res = matte::encode(&mask).map_err(|e| e.to_string()).and_then(|mask_png| {
                        edit_once(plate_prompt(), vec![approved_png.to_vec()], Some(mask_png), aspect_for(w, h), None, native)
                    });
                    match res.and_then(|(backend, png)| {
                        let edited = matte::decode(&png).map_err(|e| e.to_string())?;
                        let edited = matte::resize(&edited, w, h);
                        let restored = matte::restore_outside_mask(&edited, &mockup, &mask).map_err(|e| e.to_string())?;
                        Ok((backend, restored))
                    }) {
                        Ok((backend, f)) => {
                            plate = Some(f.clone());
                            out.push(Produced { id: bg.id.clone(), op: "cleanplate".into(), frame: f, backend: Some(backend), quality: None, warning: None });
                        }
                        Err(e) => errors.push(format!("{}(干净底图): {e}", bg.id)),
                    }
                }
            }
            _ => {
                plate = None;
                out.push(Produced {
                    id: bg.id.clone(),
                    op: "crop".into(),
                    frame: mockup.clone(),
                    backend: None,
                    quality: None,
                    warning: Some("背景直接用定稿整图:前景元素会在背景里重复出现".into()),
                });
            }
        }
    } else if let Ok(bytes) = std::fs::read(dir.join("elements").join(format!("{}.png", bg.id))) {
        plate = matte::decode(&bytes).ok().filter(|f| (f.width, f.height) == (w, h));
    }
    // 2. 其余元素。
    for e in layout.elements.iter().filter(|e| e.kind != "background" && wanted(e)) {
        let Some(r) = matte::clamp_rect(&mockup, e.bbox.map(i64::from)) else {
            errors.push(format!("{}: bbox 落在画布外", e.id));
            continue;
        };
        match e.source.as_str() {
            "text" => {}
            "crop" => {
                let crop = matte::crop(&mockup, r);
                let (frame, how) = match &plate {
                    Some(p) => match matte::diff_matte(&crop, &matte::crop(p, r), DIFF_TOL, DIFF_FEATHER) {
                        Ok(f) => (f, "diff"),
                        Err(err) => {
                            errors.push(format!("{}: {err}", e.id));
                            continue;
                        }
                    },
                    None => (matte::key_matte(&crop, ChromaKey::Auto), "key"),
                };
                let q = matte::quality(&frame);
                let warning = if q.opaque_ratio < MIN_OPAQUE_RATIO {
                    Some(format!("抠图几乎全透明({how}),元素可能与背景同色或底图没抹干净;建议改 regen"))
                } else if q.border_opaque_ratio > 0.6 && how == "key" {
                    Some("边缘大面积不透明:可能切进了背景或相邻元素".into())
                } else {
                    None
                };
                out.push(Produced { id: e.id.clone(), op: "crop".into(), frame, backend: None, quality: Some(q), warning });
            }
            "regen" => {
                let crop = matte::crop(&mockup, r);
                let res = matte::encode(&crop)
                    .map_err(|e| e.to_string())
                    .and_then(|png| edit_once(regen_prompt(e), vec![png], None, gend::backends::Aspect::Square, Some("transparent"), native));
                match res.and_then(|(backend, png)| {
                    let mut f = matte::decode(&png).map_err(|e| e.to_string())?;
                    // 后端没给透明底(全不透明)→ 按四角底色抠一次。
                    if f.rgba.chunks_exact(4).all(|p| p[3] == 255) {
                        f = matte::key_matte(&f, ChromaKey::Auto);
                    }
                    let (trimmed, _) = matte::trim(&f).ok_or("重绘结果全透明")?;
                    Ok((backend, matte::fit_into(&trimmed, r[2], r[3])))
                }) {
                    Ok((backend, frame)) => {
                        let q = matte::quality(&frame);
                        out.push(Produced { id: e.id.clone(), op: "regen".into(), frame, backend: Some(backend), quality: Some(q), warning: None });
                    }
                    Err(err) => errors.push(format!("{}(重绘): {err}", e.id)),
                }
            }
            _ => {}
        }
    }
    (out, errors)
}

/// 拼版图:每格 160×160 棋盘底,元素等比放入,便于肉眼检查抠图。
fn contact_sheet(items: &[(String, Frame)]) -> compare::Img {
    const TILE: u32 = 160;
    let cols = (items.len() as u32).clamp(1, 6);
    let rows = (items.len() as u32).div_ceil(cols).max(1);
    let (w, h) = (cols * TILE, rows * TILE);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let c = if ((x / 10) + (y / 10)) % 2 == 0 { 200 } else { 150 };
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[c, c, c, 255]);
        }
    }
    for (n, (_, f)) in items.iter().enumerate() {
        let tile = matte::fit_into(f, TILE - 8, TILE - 8);
        let (ox, oy) = ((n as u32 % cols) * TILE + 4, (n as u32 / cols) * TILE + 4);
        for y in 0..tile.height {
            for x in 0..tile.width {
                let s = ((y * tile.width + x) * 4) as usize;
                let a = u32::from(tile.rgba[s + 3]);
                if a == 0 {
                    continue;
                }
                let d = (((oy + y) * w + ox + x) * 4) as usize;
                for k in 0..3 {
                    px[d + k] = ((u32::from(tile.rgba[s + k]) * a + u32::from(px[d + k]) * (255 - a)) / 255) as u8;
                }
            }
        }
    }
    compare::Img { w, h, px }
}

pub async fn tool(
    state: &Arc<AppState>,
    sid: &str,
    rid: &str,
    rt: &DesignRuntime,
    flow: &DesignState,
    args: &Value,
) -> (bool, ToolFeedback) {
    let fail = |t: String| (false, ToolFeedback { text: t, images: Vec::new() });
    let Some(raw) = read_json(&rt.dir_abs.join("layout.json")) else {
        return fail("还没有元素清单(先 design_layout)".into());
    };
    let layout: Layout = match serde_json::from_value(raw) {
        Ok(l) => l,
        Err(e) => return fail(format!("layout.json 损坏: {e}")),
    };
    let only: Option<Vec<String>> = args["elementIds"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
    if let Some(ids) = &only {
        if let Some(bad) = ids.iter().find(|i| !layout.elements.iter().any(|e| &e.id == *i)) {
            return fail(format!("elementIds 含清单里没有的元素: {bad}"));
        }
    }
    let approved = match std::fs::read(rt.approved_path()) {
        Ok(b) => b,
        Err(e) => return fail(format!("读取定稿失败: {e}")),
    };
    let (layout_c, dir) = (layout.clone(), rt.dir_abs.clone());
    let only_c = only.clone();
    let native = state.sessions.get(sid).filter(|s| s.is_codex()).map(|_| NativeEditor {
        state: state.clone(), sid: sid.into(), run_id: rid.into(),
        project_root: rt.project_root.clone(), runtime: tokio::runtime::Handle::current(),
    });
    let joined = tokio::task::spawn_blocking(move || produce(&approved, &layout_c, only_c.as_deref(), &dir, native.as_ref())).await;
    let (produced, mut errors) = match joined {
        Ok(v) => v,
        Err(e) => return fail(format!("素材生产任务异常: {e}")),
    };
    let scale = layout::capture_scale(layout.canvas);
    let elements_dir = rt.dir_abs.join("elements");
    let _ = std::fs::create_dir_all(&elements_dir);
    let mut manifest = read_json(&rt.dir_abs.join("assets.json"))
        .filter(|_| only.is_some())
        .and_then(|v| v["elements"].as_object().cloned())
        .unwrap_or_default();
    let mut sheet = Vec::new();
    let mut lines = Vec::new();
    for p in &produced {
        let e = layout.elements.iter().find(|e| e.id == p.id).expect("产物来自清单");
        // 原尺寸存流程目录(审阅用);入库版本按截帧比例缩放,保证渲染时贴图像素 1:1。
        let path = elements_dir.join(format!("{}.png", p.id));
        let original = match matte::encode(&p.frame) {
            Ok(b) => b,
            Err(err) => {
                errors.push(format!("{}: {err}", p.id));
                continue;
            }
        };
        let _ = std::fs::write(&path, &original);
        let sb = layout::scale_bbox(e.bbox, scale);
        let scaled = matte::resize(&p.frame, sb[2], sb[3]);
        let png = match matte::encode(&scaled) {
            Ok(b) => b,
            Err(err) => {
                errors.push(format!("{}: {err}", p.id));
                continue;
            }
        };
        let detail = json!({
            "op": p.op, "flowId": rt.flow_id, "elementId": p.id, "bbox": e.bbox,
            "sourceRefs": [format!("{}/approved.png", rt.dir_rel)],
            "backendId": p.backend, "round": rt.replication_round,
        });
        match accept_png(&rt.project_root, &png, &format!("Designs/{}", rt.slug), &p.id, detail) {
            Ok((asset_path, guid)) => {
                lines.push(format!(
                    "{} [{}] → {} ({}){}",
                    p.id,
                    p.op,
                    asset_path,
                    p.quality.map(|q| format!("不透明 {:.0}%", q.opaque_ratio * 100.0)).unwrap_or_else(|| "—".into()),
                    p.warning.as_deref().map(|w| format!(" ⚠ {w}")).unwrap_or_default()
                ));
                manifest.insert(
                    p.id.clone(),
                    json!({
                        "guid": guid, "assetPath": asset_path, "op": p.op, "bbox": e.bbox, "scaledBbox": sb,
                        "file": format!("{}/elements/{}.png", rt.dir_rel, p.id),
                        "quality": p.quality.map(|q| json!({"opaqueRatio": compare::round4(q.opaque_ratio), "borderOpaqueRatio": compare::round4(q.border_opaque_ratio)})),
                        "warning": p.warning,
                    }),
                );
                if e.kind != "background" {
                    sheet.push((p.id.clone(), p.frame.clone()));
                }
            }
            Err(err) => errors.push(format!("{}(入库): {err}", p.id)),
        }
    }
    let missing: Vec<String> = layout
        .elements
        .iter()
        .filter(|e| e.source != "text" && !manifest.contains_key(&e.id))
        .map(|e| e.id.clone())
        .collect();
    let (cw, ch) = (
        (f64::from(layout.canvas.width) * scale).round() as u32,
        (f64::from(layout.canvas.height) * scale).round() as u32,
    );
    let doc = json!({
        "scale": scale, "captureWidth": cw, "captureHeight": ch,
        "elements": manifest, "errors": errors, "missing": missing, "round": rt.replication_round,
    });
    let _ = write_json_atomic(&rt.dir_abs.join("assets.json"), &doc);
    let complete = missing.is_empty();
    let fid = rt.flow_id.clone();
    let _ = state.sessions.update_design(sid, |slot| match slot.as_mut() {
        Some(d) if d.id == fid => {
            d.assets_ready = complete;
            true
        }
        _ => false,
    });
    let _ = flow;
    super::emit(
        state,
        sid,
        "design.assets.ready",
        json!({"runId": rid, "id": rt.flow_id, "round": rt.replication_round, "produced": produced.len(), "errors": errors, "missing": missing}),
    );
    let images = if sheet.is_empty() { Vec::new() } else { data_url(&contact_sheet(&sheet)).into_iter().collect() };
    let text = format!(
        "素材生产:成功 {} 个{}。\n{}{}{}",
        lines.len(),
        if scale < 1.0 { format!("(截帧缩放 {scale:.3},入库素材已按比例缩小)") } else { String::new() },
        lines.join("\n"),
        if errors.is_empty() { String::new() } else { format!("\n失败:\n{}", errors.join("\n")) },
        if missing.is_empty() {
            "\n全部非文字元素已有素材,可以 design_build。".to_string()
        } else {
            format!("\n仍缺素材:{}(修正后用 elementIds 重做)", missing.join(", "))
        }
    );
    (errors.is_empty() || complete, ToolFeedback { text, images })
}
