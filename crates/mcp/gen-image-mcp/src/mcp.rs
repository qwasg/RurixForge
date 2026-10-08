//! MCP stdio 服务:initialize / tools/list / tools/call(复刻 asset-pipeline-mcp 骨架)。
//! 七工具(05 §7 逐字参数):gen_backends_list / gen_image / gen_edit(D-045) / gen_texture_set /
//! gen_accept / gen_variations / gen_video_frames。
//! 工具错误 = isError:true + {error: <GEN_* code>, message}。
//!
//! 视频本身的生成走 REST(/api/forge/gen/video,适配器 300s 预算,MCP 接不住);
//! 落到 MCP 这一侧的是「已有 mp4 → 图集」这段,它是 agent 做角色动画的必经工序:
//! gen_video_frames → gen_accept(origin=gen-video)→ sprite_create/sprite_set。

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use assetd::project::ForgeProject;
use gend::accept::{accept_asset, gen_accept};
use gend::backends::{self, GenBackend, GenRequest, DEFAULT_SIZE, MAX_BATCH};
use gend::config::GenConfig;
use gend::keystore::Keystore;
use gend::mock;
use gend::timeutil::utc_now_iso8601;
use gend::tmpstore;
use gend::video_frames;
use gend::{fnv1a64, GenError, GEN_BACKEND_ERROR, GEN_BAD_PARAMS};
use serde_json::{json, Value};

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "gen_backends_list",
                "description": "列生成后端(id/kind/configured/capabilities);configured 为真实判定(条目缺失=false),密钥值永不返回(R-5)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "gen_image",
                "description": "文生图:产 n 个候选落 .forge/tmp/gen/;无已配置后端 → GEN_BACKEND_NOT_CONFIGURED(I-5);styleRefAssetPath v1 接受但不消费(RD-F5-002);传 assetPath 把该资产 .meta 文字简介+标签直接绑进提示词(响应 promptFinal = 实际发送全文,descBinding 如实标注)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string" },
                        "negativePrompt": { "type": "string" },
                        "size": { "type": "integer", "enum": [256, 512, 1024], "description": "边长,缺省 512" },
                        "styleRefAssetPath": { "type": "string", "description": "v1 不消费(如实标注)" },
                        "assetPath": { "type": "string", "description": "可选:目标资产路径(相对 Content/)——.meta 文字简介+标签直接并入提示词;资产无简介则 descBound=false 按原 prompt 生成" },
                        "seed": { "type": "integer", "description": "缺省 = hash(最终提示词)" },
                        "n": { "type": "integer", "minimum": 1, "maximum": 4 },
                        "aspect": { "type": "string", "enum": ["square", "landscape", "portrait"], "description": "画幅:square 按 size 出正方形(缺省);landscape=1536x1024;portrait=1024x1536" },
                        "quality": { "type": "string", "enum": ["low", "medium", "high", "auto"], "description": "画质档(后端支持时生效)" },
                        "background": { "type": "string", "enum": ["transparent", "opaque", "auto"], "description": "背景(transparent 出透明底,后端支持时生效)" },
                        "backend": { "type": "string", "description": "后端 id,缺省 = 首个已配置后端" }
                    },
                    "required": ["prompt"]
                }
            },
            {
                "name": "gen_edit",
                "description": "改图(img2img,D-045):以项目内图片为参考,按指令重绘;可选蒙版(透明像素 = 允许重绘区)。候选落 .forge/tmp/gen/,sidecar 记 sourceRefs;后端不支持改图 → GEN_UNSUPPORTED(不拿文生图冒充)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "改图指令(建议写成「保持构图与风格,只改 X」)" },
                        "imageRefs": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 4, "description": "参考图(项目相对路径,首张为主图)" },
                        "maskRef": { "type": "string", "description": "可选蒙版 PNG(项目相对路径,与主图同尺寸)" },
                        "aspect": { "type": "string", "enum": ["square", "landscape", "portrait"] },
                        "quality": { "type": "string", "enum": ["low", "medium", "high", "auto"] },
                        "background": { "type": "string", "enum": ["transparent", "opaque", "auto"] },
                        "n": { "type": "integer", "minimum": 1, "maximum": 4 },
                        "seed": { "type": "integer" },
                        "backend": { "type": "string" }
                    },
                    "required": ["prompt", "imageRefs"]
                }
            },
            {
                "name": "gen_texture_set",
                "description": "材质纹理组:逐 map 生成并自动入管线(Content/Textures/,provenance detail.map=槽位名);传 assetPath 绑该资产文字简介+标签进提示词(同 gen_image)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string" },
                        "materialKind": { "type": "string", "enum": ["pbr", "unlit"] },
                        "maps": { "type": "array", "items": { "type": "string", "enum": ["albedo", "normal", "roughness", "ao"] } },
                        "size": { "type": "integer", "enum": [256, 512, 1024] },
                        "assetPath": { "type": "string", "description": "可选:目标资产路径(相对 Content/)——.meta 文字简介+标签直接并入提示词" },
                        "seamless": { "type": "boolean", "description": "缺省 true;mock v1 不保证真无缝(如实标注)" }
                    },
                    "required": ["prompt", "materialKind", "maps"]
                }
            },
            {
                "name": "gen_accept",
                "description": "候选正式入管线:.forge/tmp/gen/ 产物 → Content/<destFolder>/<name>.png + .meta provenance(origin 缺省 gen-image;视频截帧出的图集传 origin=\"gen-video\")",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "imageFileRef": { "type": "string", "description": "项目相对路径(.forge/tmp/gen/ 内)" },
                        "destFolder": { "type": "string", "description": "相对 Content/,如 \"Textures\"" },
                        "name": { "type": "string", "description": "文件名片段(不含扩展名)" },
                        "origin": { "type": "string", "enum": ["gen-image", "gen-video"], "description": "provenance 来源标注,缺省 gen-image" }
                    },
                    "required": ["imageFileRef", "destFolder", "name"]
                }
            },
            {
                "name": "gen_video_frames",
                "description": "视频截帧成精灵图集:mp4(.forge/tmp/gen/)→ ffmpeg 均匀抽帧 + 抠背景 + 拼单张图集,回 atlasFileRef 与逐帧 bbox(直接喂 sprite_create 的 frames)。本机无 ffmpeg → GEN_TOOL_MISSING(带安装指引,不伪造帧)。视频本身由 REST /api/forge/gen/video 生成",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "videoFileRef": { "type": "string", "description": "项目相对路径(.forge/tmp/gen/ 内的 .mp4)" },
                        "fps": { "type": "number", "description": "截帧率,缺省 8(1..=30)" },
                        "maxFrames": { "type": "integer", "description": "帧数上限,缺省 32(2..=256)" },
                        "chromaKey": { "type": "string", "enum": ["auto", "magenta", "black", "none"], "description": "背景抠除:auto 四角采样(缺省)/ magenta 同视口色键 / black 黑底发光特效保留亮度与软透明 / none 不抠" },
                        "crop": { "type": "string", "enum": ["union", "tight", "none"], "description": "union 全帧包围盒并集(缺省,帧等大脚底不抖)/ tight 逐帧紧致 / none 不裁" },
                        "padding": { "type": "integer", "description": "格间透明留白像素,缺省 2" },
                        "trimStartSec": { "type": "number" },
                        "trimEndSec": { "type": "number" }
                    },
                    "required": ["videoFileRef"]
                }
            },
            {
                "name": "gen_variations",
                "description": "变体:源图(.forge/tmp/gen/ 或项目内文件)解码 + strength 扰动 seed 派生重生成;远程后端 v1 不支持(img2img seam,如实 GEN_BACKEND_ERROR)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sourceImageRef": { "type": "string" },
                        "prompt": { "type": "string" },
                        "strength": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                        "n": { "type": "integer", "minimum": 1, "maximum": 4 }
                    },
                    "required": ["sourceImageRef", "strength", "n"]
                }
            }
        ]
    })
}

fn ok(id: Value, result: Value) -> Value { json!({ "jsonrpc": "2.0", "id": id, "result": result }) }
fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_wrap(v: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    let mut out = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error { out["isError"] = json!(true); }
    out
}

fn lock<'a>(m: &'a Mutex<ForgeProject>) -> MutexGuard<'a, ForgeProject> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, GenError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, format!("缺参数或为空: {key}")))
}

fn arg_size(args: &Value) -> Result<u32, GenError> {
    let size = match args.get("size") {
        None | Some(Value::Null) => DEFAULT_SIZE,
        Some(v) => v
            .as_u64()
            .and_then(|u| u32::try_from(u).ok())
            .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "size 须为整数"))?,
    };
    mock::validate_size(size)?;
    Ok(size)
}

fn arg_n(args: &Value, default: u32) -> Result<u32, GenError> {
    let n = match args.get("n") {
        None | Some(Value::Null) => default,
        Some(v) => v
            .as_u64()
            .and_then(|u| u32::try_from(u).ok())
            .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "n 须为整数"))?,
    };
    if n == 0 || n > MAX_BATCH {
        return Err(GenError::new(GEN_BAD_PARAMS, format!("n 须 1..={MAX_BATCH},实: {n}")));
    }
    Ok(n)
}

/// gen_video_frames 参数 → FrameOptions。枚举值未知即报错,不静默回落缺省
/// ——agent 拼错 chromaKey 时,得到一张没抠底的图集比得到一条错误更难排查。
fn frame_options(args: &Value) -> Result<video_frames::FrameOptions, GenError> {
    let mut opts = video_frames::FrameOptions::default();
    if let Some(v) = args.get("fps") {
        opts.fps = v
            .as_f64()
            .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "fps 须为数字"))? as f32;
    }
    if let Some(v) = args.get("maxFrames") {
        opts.max_frames = v
            .as_u64()
            .and_then(|u| usize::try_from(u).ok())
            .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "maxFrames 须为整数"))?;
    }
    if let Some(v) = args.get("padding") {
        opts.padding = v
            .as_u64()
            .and_then(|u| u32::try_from(u).ok())
            .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "padding 须为整数"))?;
    }
    opts.trim_start_sec = args.get("trimStartSec").and_then(Value::as_f64).map(|v| v as f32);
    opts.trim_end_sec = args.get("trimEndSec").and_then(Value::as_f64).map(|v| v as f32);
    if let Some(k) = args.get("chromaKey").and_then(Value::as_str) {
        opts.chroma_key = video_frames::ChromaKey::parse(k).ok_or_else(|| {
            GenError::new(GEN_BAD_PARAMS, format!("chromaKey 须为 auto|magenta|black|none,实: {k}"))
        })?;
    }
    if let Some(c) = args.get("crop").and_then(Value::as_str) {
        opts.crop = video_frames::CropMode::parse(c).ok_or_else(|| {
            GenError::new(GEN_BAD_PARAMS, format!("crop 须为 union|tight|none,实: {c}"))
        })?;
    }
    Ok(opts)
}

/// bbox 列表 → sprite_create 的 frames 映射。命名规则借 assetd 自动切帧那一份
/// (frame_<i>),免得同一个精灵里两种来源的帧各叫一套名字。
fn video_frames_map(boxes: &[[u32; 4]]) -> Value {
    Value::Object(assetd::sprite::frames_from_boxes("frame", boxes))
}

/// 后端解析(D-045 上移至 gend,与 agentd 共用同一判据)。
fn resolve_backend(
    backend: Option<&str>,
    kind: &str,
    cfg: &GenConfig,
    keys: &Keystore,
) -> Result<Box<dyn GenBackend>, GenError> {
    backends::resolve_backend(backend, kind, cfg, keys)
}

/// 画幅 / 画质 / 背景参数(未知枚举值显式报错,不静默回落)。
fn arg_aspect(args: &Value) -> Result<backends::Aspect, GenError> {
    match args.get("aspect").and_then(Value::as_str) {
        None => Ok(backends::Aspect::Square),
        Some(a) => backends::Aspect::parse(a).ok_or_else(|| {
            GenError::new(GEN_BAD_PARAMS, format!("aspect 须为 square|landscape|portrait,实: {a}"))
        }),
    }
}

fn arg_enum(args: &Value, key: &str, allowed: &[&str]) -> Result<Option<String>, GenError> {
    match args.get(key).and_then(Value::as_str) {
        None => Ok(None),
        Some(v) if allowed.contains(&v) => Ok(Some(v.to_string())),
        Some(v) => Err(GenError::new(
            GEN_BAD_PARAMS,
            format!("{key} 须为 {} 之一,实: {v}", allowed.join("|")),
        )),
    }
}

const QUALITIES: [&str; 4] = ["low", "medium", "high", "auto"];
const BACKGROUNDS: [&str; 3] = ["transparent", "opaque", "auto"];

/// 生成上下文 sidecar(08 §6.4 provenance detail 素材)。
fn sidecar(
    backend_id: &str,
    prompt: &str,
    negative_prompt: Option<&str>,
    seed: u64,
    source_refs: Vec<String>,
    extras: &[(&str, Value)],
) -> Value {
    let mut v = json!({
        "backendId": backend_id,
        "prompt": prompt,
        "seed": seed,
        "sourceRefs": source_refs,
        "generatedAt": utc_now_iso8601(),
    });
    if let Some(np) = negative_prompt {
        v["negativePrompt"] = json!(np);
    }
    for (k, val) in extras {
        v[*k] = val.clone();
    }
    v
}

/// 资产简介绑定(F10-RAG):assetPath → .meta semantic(description+tags)直接并入提示词。
/// 返回 (最终提示词, 绑定信息)。路径非法/资产不存在 → GEN_BAD_PARAMS(显式参数显式失败);
/// 资产无简介 → 原提示词 + descBound=false(如实标注,不伪造绑定)。
fn bind_asset_description(
    proj: &ForgeProject,
    prompt: &str,
    asset_path: Option<&str>,
) -> Result<(String, Value), GenError> {
    let Some(ap) = asset_path.filter(|s| !s.trim().is_empty()) else {
        return Ok((prompt.to_string(), json!({ "descBound": false })));
    };
    let rel = assetd::normalize_rel(ap)
        .map_err(|e| GenError::new(GEN_BAD_PARAMS, format!("assetPath 非法: {ap}({})", e.message)))?;
    if !proj.content_root().join(&rel).is_file() {
        return Err(GenError::new(GEN_BAD_PARAMS, format!("assetPath 资产不存在: {rel}")));
    }
    let meta_path = assetd::meta_path_for(&proj.content_root(), &rel);
    let sem = if meta_path.is_file() {
        assetd::meta::MetaDoc::load(&meta_path).ok().and_then(|m| m.semantic)
    } else {
        None
    };
    let desc = sem
        .as_ref()
        .map(|s| s.description.trim().to_string())
        .unwrap_or_default();
    let tags: Vec<String> = sem.map(|s| s.tags).unwrap_or_default();
    if desc.is_empty() {
        return Ok((
            prompt.to_string(),
            json!({
                "descBound": false,
                "descAssetPath": rel,
                "note": "资产尚无文字简介(检视器「资产」页签可写),按原 prompt 生成",
            }),
        ));
    }
    let mut final_prompt = format!("{prompt}\n资产简介: {desc}");
    if !tags.is_empty() {
        final_prompt.push_str(&format!("\n标签: {}", tags.join(", ")));
    }
    Ok((
        final_prompt,
        json!({
            "descBound": true,
            "descAssetPath": rel,
            "boundDescription": desc,
            "boundTags": tags,
            "promptOriginal": prompt,
        }),
    ))
}

/// PNG 字节 → dataUrl(F5 wave.3:候选网格缩略图;imageFileRef 是项目内路径,client 拿不到
/// 本地文件,故响应直接带 base64 PNG 超集字段)。
fn png_data_url(png_bytes: &[u8]) -> String {
    use base64::Engine;
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png_bytes)
    )
}

/// prompt → 文件名 slug(ascii 安全;空 → "gen")。
fn slugify(prompt: &str) -> String {
    let mut s = String::new();
    let mut last_dash = false;
    for c in prompt.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
            last_dash = false;
        } else if !last_dash && !s.is_empty() {
            s.push('-');
            last_dash = true;
        }
        if s.len() >= 24 {
            break;
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "gen".into() } else { s }
}

fn call_tool(proj: &Arc<Mutex<ForgeProject>>, params: &Value) -> Result<Value, GenError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "invalid params: 缺工具名"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return Err(GenError::new(GEN_BAD_PARAMS, "invalid params: arguments 须为对象"));
    }

    match name {
        "gen_backends_list" => {
            let cfg = GenConfig::load();
            let keys = Keystore::load();
            let list: Vec<Value> = backends::registry()
                .iter()
                .map(|b| {
                    json!({
                        "id": b.id(),
                        "kind": b.kind(),
                        "configured": b.configured(&cfg, &keys),
                        "capabilities": b.capabilities(),
                    })
                })
                .collect();
            Ok(json!({ "backends": list }))
        }
        "gen_image" => {
            let prompt = arg_str(&args, "prompt")?;
            let negative = args.get("negativePrompt").and_then(Value::as_str);
            let size = arg_size(&args)?;
            let n = arg_n(&args, 1)?;
            // styleRefAssetPath:v1 接受参数但不消费(不传远程/不进生成;RD-F5-002 如实标注)。
            let _style_ref_unconsumed = args.get("styleRefAssetPath").and_then(Value::as_str);
            // F10-RAG:assetPath → 该资产 .meta 简介+标签直接并入提示词(先于缺省 seed hash,
            // 保证「同最终提示词 → 同缺省 seed」)。
            let (final_prompt, bind_info) = {
                let p = lock(proj);
                bind_asset_description(&p, prompt, args.get("assetPath").and_then(Value::as_str))?
            };
            let seed = args
                .get("seed")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| fnv1a64(final_prompt.as_bytes()));
            let cfg = GenConfig::load();
            let keys = Keystore::load();
            let backend = resolve_backend(args.get("backend").and_then(Value::as_str), "text2img", &cfg, &keys)?;
            let mut req = GenRequest::square(final_prompt.clone(), negative.map(str::to_string), size, seed, n);
            req.aspect = arg_aspect(&args)?;
            req.quality = arg_enum(&args, "quality", &QUALITIES)?;
            req.background = arg_enum(&args, "background", &BACKGROUNDS)?;
            let cands = backend.generate(&req, &cfg, &keys)?;
            // 绑定发生过(含尝试但无简介)→ sidecar 如实记录;未传 assetPath 保持旧形态。
            let extras: Vec<(&str, Value)> = if bind_info.get("descAssetPath").is_some() {
                vec![("descBinding", bind_info.clone())]
            } else {
                vec![]
            };
            let p = lock(proj);
            let mut out = Vec::with_capacity(cands.len());
            for (i, c) in cands.iter().enumerate() {
                let sc = sidecar(backend.id(), &final_prompt, negative, c.seed, vec![], &extras);
                let r = tmpstore::save_candidate(&p, &c.png_bytes, c.seed, i as u32, &sc)?;
                out.push(json!({
                    "imageFileRef": r,
                    "seed": c.seed,
                    "backendId": backend.id(),
                    "dataUrl": png_data_url(&c.png_bytes),
                }));
            }
            Ok(json!({
                "candidates": out,
                "promptFinal": final_prompt,
                "descBinding": bind_info,
            }))
        }
        "gen_edit" => {
            let prompt = arg_str(&args, "prompt")?;
            let refs: Vec<String> = args
                .get("imageRefs")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            if refs.is_empty() || refs.len() > 4 {
                return Err(GenError::new(GEN_BAD_PARAMS, "imageRefs 须 1..=4 个项目相对路径"));
            }
            let n = arg_n(&args, 1)?;
            let aspect = arg_aspect(&args)?;
            let quality = arg_enum(&args, "quality", &QUALITIES)?;
            let background = arg_enum(&args, "background", &BACKGROUNDS)?;
            // 后端门先于读文件(无配置一律 NOT_CONFIGURED,同 gen_variations)。
            let cfg = GenConfig::load();
            let keys = Keystore::load();
            let backend = resolve_backend(args.get("backend").and_then(Value::as_str), "img2img", &cfg, &keys)?;
            let p = lock(proj);
            let mut images = Vec::with_capacity(refs.len());
            for r in &refs {
                images.push(std::fs::read(tmpstore::resolve_project_file(&p, r)?)?);
            }
            let mask_ref = args.get("maskRef").and_then(Value::as_str).filter(|s| !s.is_empty());
            let mask = match mask_ref {
                Some(m) => Some(std::fs::read(tmpstore::resolve_project_file(&p, m)?)?),
                None => None,
            };
            let seed = args
                .get("seed")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| gend::hash_parts(&[prompt.as_bytes(), &images[0]]));
            let req = backends::EditRequest {
                prompt: prompt.to_string(),
                images,
                mask,
                aspect,
                seed,
                n,
                quality,
                background,
            };
            drop(p);
            let cands = backend.edit(&req, &cfg, &keys)?;
            let mut source_refs = refs.clone();
            if let Some(m) = mask_ref {
                source_refs.push(m.to_string());
            }
            let p = lock(proj);
            let mut out = Vec::with_capacity(cands.len());
            for (i, c) in cands.iter().enumerate() {
                let sc = sidecar(backend.id(), prompt, None, c.seed, source_refs.clone(), &[("op", json!("edit"))]);
                let r = tmpstore::save_candidate(&p, &c.png_bytes, c.seed, i as u32, &sc)?;
                out.push(json!({
                    "imageFileRef": r,
                    "seed": c.seed,
                    "backendId": backend.id(),
                    "dataUrl": png_data_url(&c.png_bytes),
                }));
            }
            Ok(json!({ "candidates": out }))
        }
        "gen_texture_set" => {
            let prompt = arg_str(&args, "prompt")?;
            let material_kind = arg_str(&args, "materialKind")?;
            if material_kind != "pbr" && material_kind != "unlit" {
                return Err(GenError::new(GEN_BAD_PARAMS, format!("materialKind 须 pbr|unlit,实: {material_kind}")));
            }
            let maps_val = args
                .get("maps")
                .and_then(Value::as_array)
                .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "缺参数: maps"))?;
            if maps_val.is_empty() {
                return Err(GenError::new(GEN_BAD_PARAMS, "maps 不可空"));
            }
            let mut maps: Vec<String> = Vec::new();
            for m in maps_val {
                let s = m.as_str().ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "maps 元素须为字符串"))?;
                if !mock::MAP_KINDS.contains(&s) {
                    return Err(GenError::new(GEN_BAD_PARAMS, format!("未知纹理槽位: {s}")));
                }
                if !maps.iter().any(|x| x == s) {
                    maps.push(s.to_string());
                }
            }
            let size = arg_size(&args)?;
            // seamless 缺省 true;mock v1 条纹场非真无缝,如实标注不伪装。
            let _seamless = args.get("seamless").and_then(Value::as_bool).unwrap_or(true);
            let cfg = GenConfig::load();
            let keys = Keystore::load();
            let backend = resolve_backend(args.get("backend").and_then(Value::as_str), "texture-set", &cfg, &keys)?;
            // texture-set 能力门:remote-openai-compatible v1 仅 text2img → 如实拒绝。
            let kinds = backend.capabilities()["kinds"].as_array().cloned().unwrap_or_default();
            if !kinds.iter().any(|k| k == "texture-set") {
                return Err(GenError::new(
                    GEN_BACKEND_ERROR,
                    format!("后端 {} v1 不支持 texture-set(能力面 kinds={kinds:?})", backend.id()),
                ));
            }
            let slug = slugify(prompt);
            // F10-RAG:assetPath 绑该资产简介+标签进提示词(seed 按最终提示词派生)。
            let (final_prompt, bind_info) = {
                let p = lock(proj);
                bind_asset_description(&p, prompt, args.get("assetPath").and_then(Value::as_str))?
            };
            let extras: Vec<(&str, Value)> = if bind_info.get("descAssetPath").is_some() {
                vec![("descBinding", bind_info.clone())]
            } else {
                vec![]
            };
            let p = lock(proj);
            let mut assets = Vec::with_capacity(maps.len());
            for m in &maps {
                let seed = gend::hash_parts(&[final_prompt.as_bytes(), m.as_bytes(), &size.to_le_bytes()]);
                let png = mock::render_map(&final_prompt, m, size, seed)?;
                let mut detail_extras: Vec<(&str, Value)> = vec![("map", json!(m))];
                detail_extras.extend(extras.iter().cloned());
                let detail = sidecar(backend.id(), &final_prompt, None, seed, vec![], &detail_extras);
                let r = tmpstore::save_candidate(&p, &png, seed, 0, &detail)?;
                let acc = gen_accept(&p, &r, "Textures", &format!("{slug}_{m}"), detail)?;
                assets.push(json!({
                    "map": m,
                    "assetPath": acc.asset_path,
                    "dataUrl": png_data_url(&png),
                }));
            }
            Ok(json!({
                "textureAssets": assets,
                "promptFinal": final_prompt,
                "descBinding": bind_info,
            }))
        }
        "gen_accept" => {
            let image_ref = arg_str(&args, "imageFileRef")?;
            let dest = arg_str(&args, "destFolder")?;
            let name = arg_str(&args, "name")?;
            let origin = match args.get("origin").and_then(Value::as_str) {
                None | Some("gen-image") => "gen-image",
                Some("gen-video") => "gen-video",
                Some(other) => {
                    return Err(GenError::new(
                        GEN_BAD_PARAMS,
                        format!("origin 须为 gen-image|gen-video,实: {other}"),
                    ))
                }
            };
            let p = lock(proj);
            // provenance detail 来自候选 sidecar;无 sidecar(人工预放 fixture)如实标注。
            let detail = tmpstore::load_sidecar(&p, image_ref).unwrap_or_else(|| {
                json!({ "sourceRefs": [], "note": "无生成 sidecar(人工预放置产物)" })
            });
            let acc = accept_asset(&p, image_ref, dest, name, origin, detail, None)?;
            Ok(json!({ "assetPath": acc.asset_path, "guid": acc.guid }))
        }
        "gen_video_frames" => {
            let video_ref = arg_str(&args, "videoFileRef")?;
            let opts = frame_options(&args)?;
            let p = lock(proj);
            let out = video_frames::video_to_atlas(&p, video_ref, &opts)?;
            Ok(json!({
                "atlasFileRef": out.atlas_ref,
                "width": out.width,
                "height": out.height,
                "frameCount": out.frame_count,
                "fps": out.fps,
                "boxes": out.boxes,
                // frames 可直接原样传给 sprite_create,省掉调用方自己拼一遍 bbox 映射。
                "frames": video_frames_map(&out.boxes),
            }))
        }
        "gen_variations" => {
            let source_ref = arg_str(&args, "sourceImageRef")?;
            let prompt = args.get("prompt").and_then(Value::as_str).unwrap_or("");
            let strength = args
                .get("strength")
                .and_then(Value::as_f64)
                .ok_or_else(|| GenError::new(GEN_BAD_PARAMS, "缺参数: strength"))?;
            if !(0.0..=1.0).contains(&strength) {
                return Err(GenError::new(GEN_BAD_PARAMS, format!("strength 须 0.0..=1.0,实: {strength}")));
            }
            let n = arg_n(&args, 1)?;
            // 后端门先于源文件解析(G-F5-1:无配置一律 NOT_CONFIGURED)。
            let cfg = GenConfig::load();
            let keys = Keystore::load();
            let backend = resolve_backend(args.get("backend").and_then(Value::as_str), "variations", &cfg, &keys)?;
            // 远程 v1 无 img2img 面(images/generations 不消费源图)→ 如实报错不伪装。
            if backend.kind() != "local" {
                return Err(GenError::new(
                    GEN_BACKEND_ERROR,
                    format!("后端 {} v1 不支持 variations(img2img seam)", backend.id()),
                ));
            }
            let p = lock(proj);
            let src_abs = tmpstore::resolve_project_file(&p, source_ref)?;
            let src_bytes = std::fs::read(&src_abs)?;
            let img = image::load_from_memory(&src_bytes).map_err(|e| {
                GenError::new(GEN_BAD_PARAMS, format!("源图解码失败({source_ref}): {e}"))
            })?;
            let (w, h) = (img.width(), img.height());
            let mut out = Vec::with_capacity(n as usize);
            for i in 0..n {
                let seed = mock::variation_seed(&src_bytes, prompt, strength as f32, i);
                let png = mock::render_map_sized(prompt, "albedo", w, h, seed)?;
                let sc = sidecar(
                    backend.id(),
                    prompt,
                    None,
                    seed,
                    vec![source_ref.to_string()],
                    &[("strength", json!(strength))],
                );
                let r = tmpstore::save_candidate(&p, &png, seed, i, &sc)?;
                out.push(json!({
                    "imageFileRef": r,
                    "seed": seed,
                    "backendId": backend.id(),
                    "dataUrl": png_data_url(&png),
                }));
            }
            Ok(json!({ "candidates": out }))
        }
        _ => Err(GenError::new(GEN_BAD_PARAMS, format!("未知工具:{name}"))),
    }
}

pub fn serve_stdio(proj: Arc<Mutex<ForgeProject>>) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = err(Value::Null, -32700, &format!("parse error: {e}"));
                let _ = writeln!(stdout, "{resp}");
                let _ = stdout.flush();
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let resp: Option<Value> = match method {
            "initialize" => id.map(|i| ok(i, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "gen-image", "version": env!("CARGO_PKG_VERSION") }
            }))),
            "notifications/initialized" | "notifications/cancelled" => None,
            "ping" => id.map(|i| ok(i, json!({}))),
            "tools/list" => id.map(|i| ok(i, tool_list())),
            "tools/call" => id.map(|i| {
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                match call_tool(&proj, &params) {
                    Ok(result) => ok(i, tool_wrap(&result, false)),
                    Err(e) => ok(i, tool_wrap(&json!({ "error": e.code, "message": e.message }), true)),
                }
            }),
            "" => id.map(|i| err(i, -32600, "invalid request: 缺 method")),
            other => id.map(|i| err(i, -32601, &format!("method not found: {other}"))),
        };
        if let Some(r) = resp {
            let _ = writeln!(stdout, "{r}");
            let _ = stdout.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gend::GEN_BACKEND_NOT_CONFIGURED;
    use std::sync::Mutex;

    /// FORGE_GEN_DATA_DIR / FORGE_GEN_API_KEY 进程级,测试串行。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("genimg-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_project(tag: &str) -> Arc<Mutex<ForgeProject>> {
        Arc::new(Mutex::new(ForgeProject::with_defaults(temp_dir(tag))))
    }

    fn call(proj: &Arc<Mutex<ForgeProject>>, tool: &str, args: Value) -> Result<Value, GenError> {
        call_tool(proj, &json!({ "name": tool, "arguments": args }))
    }

    #[test]
    fn tools_list_six() {
        let tl = tool_list();
        let tools = tl["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 7);
        for t in [
            "gen_backends_list",
            "gen_image",
            "gen_edit",
            "gen_texture_set",
            "gen_accept",
            "gen_variations",
            "gen_video_frames",
        ] {
            assert!(tools.iter().any(|x| x["name"] == t), "缺工具 {t}");
        }
    }

    #[test]
    fn gen_video_frames_param_gate_and_missing_ffmpeg() {
        let _g = ENV_LOCK.lock().unwrap();
        let proj = temp_project("vframes");
        // 枚举值未知 → GEN_BAD_PARAMS(先于外部依赖)。
        let e = call(
            &proj,
            "gen_video_frames",
            json!({ "videoFileRef": ".forge/tmp/gen/a.mp4", "crop": "square" }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        let e = call(
            &proj,
            "gen_video_frames",
            json!({ "videoFileRef": ".forge/tmp/gen/a.mp4", "fps": "fast" }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        let e = call(&proj, "gen_video_frames", json!({})).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        // 本机无 ffmpeg → GEN_TOOL_MISSING(不伪造帧)。
        let prev = std::env::var("FORGE_FFMPEG").ok();
        std::env::set_var("FORGE_FFMPEG", "/definitely/not/here/ffmpeg-nope");
        let e = call(
            &proj,
            "gen_video_frames",
            json!({ "videoFileRef": ".forge/tmp/gen/a.mp4" }),
        )
        .unwrap_err();
        assert_eq!(e.code, gend::GEN_TOOL_MISSING);
        match prev {
            Some(v) => std::env::set_var("FORGE_FFMPEG", v),
            None => std::env::remove_var("FORGE_FFMPEG"),
        }
    }

    #[test]
    fn gen_accept_origin_whitelist() {
        let _g = ENV_LOCK.lock().unwrap();
        let proj = temp_project("origin");
        let e = call(
            &proj,
            "gen_accept",
            json!({ "imageFileRef": ".forge/tmp/gen/x.png", "destFolder": "Sprites", "name": "a", "origin": "hand-drawn" }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        assert!(e.message.contains("gen-video"), "{}", e.message);
    }

    #[test]
    fn gen_image_unconfigured_gate() {
        let _g = ENV_LOCK.lock().unwrap();
        let data = temp_dir("gate-data");
        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        std::env::remove_var("FORGE_GEN_API_KEY");
        let proj = temp_project("gate-proj");
        // 空 data 目录(无 gen-backends.json)→ 全工具面 NOT_CONFIGURED(list/accept 除外)。
        let e = call(&proj, "gen_image", json!({ "prompt": "wood" })).unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
        let e = call(
            &proj,
            "gen_texture_set",
            json!({ "prompt": "wood", "materialKind": "pbr", "maps": ["albedo"] }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
        let e = call(
            &proj,
            "gen_variations",
            json!({ "sourceImageRef": ".forge/tmp/gen/x.png", "strength": 0.5, "n": 1 }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
        // list 如实两适配器 configured=false。
        let v = call(&proj, "gen_backends_list", json!({})).unwrap();
        let bs = v["backends"].as_array().unwrap();
        assert_eq!(bs.len(), 2);
        assert!(bs.iter().all(|b| b["configured"] == false));
        // n 越界 → GEN_BAD_PARAMS(参数校验先于后端门?此处后端门先——gen_image 参数校验在
        // resolve_backend 之前,空配置下 n=5 也应报 BAD_PARAMS)。
        let e = call(&proj, "gen_image", json!({ "prompt": "w", "n": 5 })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn gen_edit_and_aspect_with_local_mock() {
        let _g = ENV_LOCK.lock().unwrap();
        let data = temp_dir("edit-data");
        std::fs::write(
            data.join("gen-backends.json"),
            r#"{"backends":[{"id":"local-mock","kind":"local","enabled":true}]}"#,
        )
        .unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        std::env::remove_var("FORGE_GEN_API_KEY");
        let proj = temp_project("edit-proj");
        let v = call(&proj, "gen_image", json!({ "prompt": "menu", "aspect": "landscape" })).unwrap();
        let r = v["candidates"][0]["imageFileRef"].as_str().unwrap().to_string();
        let root = lock(&proj).root.clone();
        let img = image::open(root.join(&r)).unwrap();
        assert_eq!((img.width(), img.height()), (1536, 1024));
        let e = call(&proj, "gen_image", json!({ "prompt": "m", "aspect": "wide" })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        let v = call(&proj, "gen_edit", json!({ "prompt": "make it red", "imageRefs": [r.clone()], "n": 2 })).unwrap();
        let cands = v["candidates"].as_array().unwrap();
        assert_eq!(cands.len(), 2);
        let out_ref = cands[0]["imageFileRef"].as_str().unwrap();
        let side = tmpstore::load_sidecar(&lock(&proj), out_ref).unwrap();
        assert_eq!(side["sourceRefs"][0], json!(r));
        assert_eq!(side["op"], json!("edit"));
        let e = call(&proj, "gen_edit", json!({ "prompt": "x", "imageRefs": [] })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn local_mock_full_flow_deterministic() {
        let _g = ENV_LOCK.lock().unwrap();
        let data = temp_dir("flow-data");
        std::fs::write(
            data.join("gen-backends.json"),
            r#"{"backends":[{"id":"local-mock","kind":"local","enabled":true}]}"#,
        )
        .unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        std::env::remove_var("FORGE_GEN_API_KEY");
        let proj = temp_project("flow-proj");

        // list:local-mock configured=true + capabilities 非空;remote false。
        let v = call(&proj, "gen_backends_list", json!({})).unwrap();
        let lm = v["backends"].as_array().unwrap().iter().find(|b| b["id"] == "local-mock").unwrap();
        assert_eq!(lm["configured"], true);
        assert!(lm["capabilities"]["kinds"].as_array().unwrap().len() >= 2);

        // gen_image n=4 seed=42 → 4 候选落盘;复跑逐字节一致。
        let args = json!({ "prompt": "wood grain 木纹", "n": 4, "size": 256, "seed": 42 });
        let r1 = call(&proj, "gen_image", args.clone()).unwrap();
        let c1 = r1["candidates"].as_array().unwrap();
        assert_eq!(c1.len(), 4);
        let read = |r: &str| std::fs::read(proj.lock().unwrap().root.join(r)).unwrap();
        let b1: Vec<Vec<u8>> = c1.iter().map(|c| read(c["imageFileRef"].as_str().unwrap())).collect();
        let r2 = call(&proj, "gen_image", args).unwrap();
        let c2 = r2["candidates"].as_array().unwrap();
        let b2: Vec<Vec<u8>> = c2.iter().map(|c| read(c["imageFileRef"].as_str().unwrap())).collect();
        assert_eq!(b1, b2, "同参复跑须逐字节一致");
        for (i, c) in c1.iter().enumerate() {
            assert_eq!(c["seed"], 42 + i as u64);
            assert_eq!(c["backendId"], "local-mock");
            // F5 wave.3:dataUrl 超集字段(base64 PNG,与落盘字节一致,供前端候选网格)。
            let du = c["dataUrl"].as_str().expect("候选缺 dataUrl");
            let b64 = du.strip_prefix("data:image/png;base64,").expect("dataUrl 前缀异常");
            use base64::Engine;
            let decoded = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
            assert_eq!(decoded, b1[i], "dataUrl 解码须与落盘 PNG 字节一致");
        }

        // gen_accept 第 1 候选 → Content/Textures + provenance 全字段。
        let acc = call(
            &proj,
            "gen_accept",
            json!({ "imageFileRef": c1[0]["imageFileRef"], "destFolder": "Textures", "name": "ut_wood" }),
        )
        .unwrap();
        assert_eq!(acc["assetPath"], "Textures/ut_wood.png");
        assert!(acc["guid"].as_str().unwrap().len() > 8);
        let meta_path = proj.lock().unwrap().content_root().join("Textures/ut_wood.png.meta");
        let meta = assetd::meta::MetaDoc::load(&meta_path).unwrap();
        let prov = meta.provenance.unwrap();
        assert_eq!(prov.origin, "gen-image");
        let d = prov.detail.unwrap();
        assert_eq!(d["backendId"], "local-mock");
        assert_eq!(d["prompt"], "wood grain 木纹");
        assert_eq!(d["seed"], 42);
        assert!(d["generatedAt"].as_str().unwrap().ends_with('Z'));

        // gen_texture_set 三 map 自动入管线。
        let ts = call(
            &proj,
            "gen_texture_set",
            json!({ "prompt": "wood", "materialKind": "pbr", "maps": ["albedo", "normal", "roughness"], "size": 256 }),
        )
        .unwrap();
        let tas = ts["textureAssets"].as_array().unwrap();
        assert_eq!(tas.len(), 3);
        for ta in tas {
            let ap = ta["assetPath"].as_str().unwrap();
            assert!(ap.starts_with("Textures/wood_"), "{ap}");
            assert!(proj.lock().unwrap().content_root().join(ap).is_file());
            let m = assetd::meta::MetaDoc::load(&assetd::meta_path_for(
                &proj.lock().unwrap().content_root(),
                ap,
            ))
            .unwrap();
            assert_eq!(m.provenance.unwrap().origin, "gen-image");
        }

        // gen_variations:以 accept 产物为源图。
        let var = call(
            &proj,
            "gen_variations",
            json!({ "sourceImageRef": "Content/Textures/ut_wood.png", "prompt": "oak", "strength": 0.5, "n": 2 }),
        )
        .unwrap();
        let vc = var["candidates"].as_array().unwrap();
        assert_eq!(vc.len(), 2);
        assert_ne!(vc[0]["seed"], vc[1]["seed"]);

        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }

    /// F10-RAG 绑定测试夹具:local-mock 后端 + 带/不带简介的资产。
    fn bind_fixture(tag: &str) -> (std::path::PathBuf, Arc<Mutex<ForgeProject>>) {
        let data = temp_dir(&format!("{tag}-data"));
        std::fs::write(
            data.join("gen-backends.json"),
            r#"{"backends":[{"id":"local-mock","kind":"local","enabled":true}]}"#,
        )
        .unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        std::env::remove_var("FORGE_GEN_API_KEY");
        let proj = temp_project(tag);
        let p = proj.lock().unwrap();
        std::fs::create_dir_all(p.content_root().join("Textures")).unwrap();
        std::fs::write(p.content_root().join("Textures/chair.png"), b"fake-png").unwrap();
        std::fs::write(p.content_root().join("Textures/nodesc.png"), b"fake-png").unwrap();
        assetd::ops::set_description(
            &p,
            "Textures/chair.png",
            "北欧风实木餐椅,浅橡木色,适合客厅场景",
            &["家具".to_string(), "椅子".to_string()],
            "human",
            None,
            None,
        )
        .unwrap();
        drop(p);
        (data, proj)
    }

    #[test]
    fn gen_image_binds_asset_description_into_prompt() {
        let _g = ENV_LOCK.lock().unwrap();
        let (data, proj) = bind_fixture("bind");

        let r = call(
            &proj,
            "gen_image",
            json!({ "prompt": "一把椅子", "n": 1, "size": 256, "assetPath": "Textures/chair.png" }),
        )
        .unwrap();
        let final_prompt = r["promptFinal"].as_str().expect("缺 promptFinal");
        assert!(final_prompt.contains("一把椅子"), "{final_prompt}");
        assert!(final_prompt.contains("北欧风实木餐椅"), "简介须绑入:{final_prompt}");
        assert!(final_prompt.contains("家具"), "标签须绑入:{final_prompt}");
        assert_eq!(r["descBinding"]["descBound"], true, "{r}");
        assert_eq!(r["descBinding"]["descAssetPath"], "Textures/chair.png");
        assert_eq!(r["descBinding"]["promptOriginal"], "一把椅子");

        // sidecar 如实记录绑定(实际发送全文 + 绑定信息)。
        let image_ref = r["candidates"][0]["imageFileRef"].as_str().unwrap();
        let p = proj.lock().unwrap();
        let sc = tmpstore::load_sidecar(&p, image_ref).expect("缺 sidecar");
        assert_eq!(sc["prompt"], final_prompt);
        assert_eq!(sc["descBinding"]["descBound"], true);
        assert_eq!(sc["descBinding"]["boundDescription"], "北欧风实木餐椅,浅橡木色,适合客厅场景");
        drop(p);

        // 无简介资产:原 prompt 生成 + descBound=false 如实标注。
        let r = call(
            &proj,
            "gen_image",
            json!({ "prompt": "一张桌子", "n": 1, "size": 256, "assetPath": "Textures/nodesc.png" }),
        )
        .unwrap();
        assert_eq!(r["promptFinal"], "一张桌子");
        assert_eq!(r["descBinding"]["descBound"], false, "{r}");
        assert!(r["descBinding"]["note"].as_str().unwrap().contains("尚无文字简介"), "{r}");

        // 不传 assetPath:响应仍带 promptFinal(= 原 prompt),sidecar 无 descBinding 字段。
        let r = call(&proj, "gen_image", json!({ "prompt": "plain", "n": 1, "size": 256 })).unwrap();
        assert_eq!(r["promptFinal"], "plain");
        assert_eq!(r["descBinding"]["descBound"], false);
        let image_ref = r["candidates"][0]["imageFileRef"].as_str().unwrap();
        let p = proj.lock().unwrap();
        let sc = tmpstore::load_sidecar(&p, image_ref).unwrap();
        assert!(sc.get("descBinding").is_none(), "未绑定不应有 descBinding:{sc}");
        drop(p);

        // assetPath 指向不存在资产 → GEN_BAD_PARAMS(显式参数显式失败)。
        let e = call(
            &proj,
            "gen_image",
            json!({ "prompt": "x", "assetPath": "Textures/ghost.png" }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);

        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn gen_texture_set_binds_asset_description() {
        let _g = ENV_LOCK.lock().unwrap();
        let (data, proj) = bind_fixture("bind-ts");

        let r = call(
            &proj,
            "gen_texture_set",
            json!({
                "prompt": "椅子材质", "materialKind": "pbr", "maps": ["albedo"], "size": 256,
                "assetPath": "Textures/chair.png"
            }),
        )
        .unwrap();
        assert!(r["promptFinal"].as_str().unwrap().contains("北欧风实木餐椅"), "{r}");
        assert_eq!(r["descBinding"]["descBound"], true);
        // 入管线资产 provenance detail 带绑定信息。
        let ap = r["textureAssets"][0]["assetPath"].as_str().unwrap();
        let p = proj.lock().unwrap();
        let meta = assetd::meta::MetaDoc::load(&assetd::meta_path_for(&p.content_root(), ap)).unwrap();
        let detail = meta.provenance.unwrap().detail.unwrap();
        assert_eq!(detail["descBinding"]["descBound"], true, "{detail}");
        assert!(detail["prompt"].as_str().unwrap().contains("北欧风实木餐椅"), "{detail}");
        drop(p);

        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }
}
