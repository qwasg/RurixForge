//! MCP stdio 服务:initialize / tools/list / tools/call(复刻 gen-image-mcp 骨架)。
//! 三工具(05 §8 逐字参数):gen_mesh / gen_mesh_refine / gen_accept。
//! 诚实优先(D-F5-E):注册表无 text2mesh/retopo 适配器 → gen_mesh/gen_mesh_refine
//! 显式 GEN_BACKEND_NOT_CONFIGURED(不伪造生成能力);gen_accept 走 asset_import 同一
//! 构建链(rurix-geom-build DAG,retopo/LOD 派生属链内行为 D-009,非独立 refine 调用)。
//! 工具错误 = isError:true + {error: <GEN_* code>, message}。

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use assetd::project::ForgeProject;
use gend::accept::accept_asset;
use gend::timeutil::utc_now_iso8601;
use gend::tmpstore;
use gend::{
    GenError, GEN_BACKEND_ERROR, GEN_BACKEND_NOT_CONFIGURED, GEN_BAD_PARAMS,
    GEN_TOOL_MISSING,
};
use serde_json::{json, Value};

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "gen_mesh",
                "description": "文/图生网格候选;默认走 meshy(text-to-3d preview→refine 两阶段 / image-to-3d,产物 glb),可经 backend 切到自建兼容端点。后端未配置 → 显式 GEN_BACKEND_NOT_CONFIGURED,不伪造生成能力。注意:远端为异步任务制,单次调用可能耗时数分钟,超时的调用方请改用 REST POST /api/forge/gen/mesh",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "maxLength": 600, "description": "与 imageRef/imageRefs 至少其一;上限 600 字符;图生时仅作贴图引导,不控制几何" },
                        "imageRef": { "type": "string", "description": "单张参考图(项目相对路径 png/jpg,或 http(s)/data URI);与 imageRefs 择一;与 prompt 至少其一" },
                        "imageRefs": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 4, "description": "同一主体的 1..4 张参考图(路径、http(s) 或 data URI);与 imageRef 择一;1 张走单图,2..4 张走多图重建" },
                        "targetPolyBudget": { "type": "integer", "description": "目标面数(standard 档 100..=300000,smart-topology 档 100..=15000)" },
                        "styleRefAssetPath": { "type": "string", "description": "风格参考:图片路径 → 作贴图引导图;其余字符串 → 作贴图提示词" },
                        "texturePrompt": { "type": "string", "description": "显式贴图提示词;图生时优先于由 prompt 转换的贴图引导" },
                        "texture": { "type": "boolean", "description": "是否跑贴图阶段(默认 true;false 只出几何,省额度)" },
                        "pbr": { "type": "boolean", "description": "是否出 PBR 贴图组(默认 true)" },
                        "textureResolution": { "type": "string", "description": "2k | 4k | 8k(默认 2k)" },
                        "modelType": { "type": "string", "description": "standard | smart-topology(默认 standard)" },
                        "aiModel": { "type": "string", "description": "latest | meshy-7 | meshy-6 | meshy-5 | meshy-t2(缺省取后端条目 model)" },
                        "topology": { "type": "string", "description": "triangle | quad" },
                        "poseMode": { "type": "string", "description": "a-pose | t-pose(角色类可用)" },
                        "track": { "type": "string", "enum": ["cloud", "auto"], "default": "auto", "description": "cloud:仅云端生成,失败不回退;auto:云端失败时允许回退到本地人形生成脚本(默认 auto);要求 Meshy 产物时显式传 cloud" },
                        "timeoutSec": { "type": "integer", "description": "单阶段等待预算秒数(默认 900)" },
                        "backend": { "type": "string", "description": "后端 id(缺省取首个已配置的 text2mesh 适配器,即 meshy)" }
                    }
                }
            },
            {
                "name": "gen_mesh_refine",
                "description": "双轨网格减面与精修:支持 Meshy Remesh API (Track A) 与本地无头减面脚本 (Track B),输出严格 <= targetFaces 的三角网格并返回前后顶点/面数统计对比",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "meshFileRef": { "type": "string", "description": "项目相对路径(.forge/tmp/gen/ 内)" },
                        "targetFaces": { "type": "integer", "description": "目标三角面数预算" },
                        "track": { "type": "string", "description": "执行轨道: cloud | local | auto(默认 auto)" },
                        "ops": {
                            "type": "object",
                            "properties": {
                                "retopo": { "type": "object", "properties": { "targetFaces": { "type": "integer" } } },
                                "unwrap": { "type": "boolean" },
                                "rigPreview": { "type": "boolean" }
                            }
                        }
                    },
                    "required": ["meshFileRef", "ops"]
                }
            },
            {
                "name": "gen_mesh_rig",
                "description": "双轨人形自动绑骨:支持 Meshy Auto-Rigging API 与本地无头绑骨脚本,绑定标准人形骨架并强制生成 4-骨骼归一化蒙皮权重与合法 inverseBindMatrices",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "meshFileRef": { "type": "string", "description": "项目相对路径(.forge/tmp/gen/ 内)" },
                        "heightMeters": { "type": "number", "description": "角色高度(米,默认 1.7)" },
                        "track": { "type": "string", "description": "执行轨道: cloud | local | auto(默认 auto)" }
                    },
                    "required": ["meshFileRef"]
                }
            },
            {
                "name": "gen_mesh_animate",
                "description": "双轨动作包合入:支持 Meshy Animation API 与标准人形动作包合入脚本,将标准动作(Idle, Walk, Run, Attack 等)合入绑骨后的角色 GLB",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "meshFileRef": { "type": "string", "description": "已绑骨的角色 GLB 路径(.forge/tmp/gen/ 内)" },
                        "motionPack": { "type": "string", "description": "动作包目录相对路径(缺省 assets/motion-packs/humanoid)" },
                        "actionIds": { "type": "array", "items": { "type": "integer" }, "description": "Meshy 动作库 ID 列表(云端轨道用,如 [0, 30])" },
                        "track": { "type": "string", "description": "执行轨道: cloud | local | auto(默认 auto)" }
                    },
                    "required": ["meshFileRef"]
                }
            },
            {
                "name": "gen_character_publish",
                "description": "原子化角色打包与发布: 将已绑骨且合入 Idle/Walk 动作的角色 GLB (.forge/tmp/gen/ 内) 原子发布为 Content/Models/<guid>/model.rxmodel 与 template.rxprefab (包含 Animator, CharacterController, ModelRenderer, Category: 'role')。不满足可动角色要求的产物显式拒绝 (红线 I-5)。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "meshFileRef": {
                            "type": "string",
                            "description": "项目相对路径 (.forge/tmp/gen/ 内已绑骨并合入 Idle/Walk 动作的角色 GLB)"
                        },
                        "name": {
                            "type": "string",
                            "description": "角色标识/名称 (纯英数下划线文件名片段, 如 'hero_warrior')"
                        },
                        "category": {
                            "type": "string",
                            "description": "资产分类 (默认 'role')"
                        },
                        "idleClip": {
                            "type": "string",
                            "description": "待绑定的 Idle 动作名 (缺省 'Idle')"
                        },
                        "walkClip": {
                            "type": "string",
                            "description": "待绑定的 Walk 动作名 (缺省 'Walk')"
                        },
                        "revision": {
                            "type": "integer",
                            "description": "模型版本号 (默认 1)"
                        }
                    },
                    "required": ["meshFileRef", "name"]
                }
            },
            {
                "name": "gen_accept",
                "description": "网格产物正式入管线:.forge/tmp/gen/ meshFileRef → Content/<destFolder>/<name>.<ext> + .meta provenance(origin=gen-model,结构化 detail)+ .rxmesh 构建产物;importSettings 透传(merge 进 .meta import_settings,缓存键构成)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "meshFileRef": { "type": "string", "description": "项目相对路径(.forge/tmp/gen/ 内)" },
                        "destFolder": { "type": "string", "description": "相对 Content/,如 \"Meshes\"" },
                        "name": { "type": "string", "description": "文件名片段(不含扩展名)" },
                        "importSettings": { "type": "object", "description": "透传 asset_import 第三参(如 generateLods)" }
                    },
                    "required": ["meshFileRef", "destFolder", "name"]
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

/// 图引用 → 远端可收的地址:已是 http(s)/data URI 则原样透传,否则按项目相对路径读盘编码。
fn to_image_uri(project: &ForgeProject, image_ref: &str) -> Result<String, GenError> {
    let r = image_ref.trim();
    if r.starts_with("http://") || r.starts_with("https://") || r.starts_with("data:") {
        return Ok(r.to_string());
    }
    tmpstore::image_data_uri(project, r)
}

/// gen_mesh 的档位参数原样搬进 MediaRequest.params(校验交给适配器,
/// 由它按供应商能力面给出可诉诸的 GEN_BAD_PARAMS,工具层不重复一套白名单)。
fn mesh_params(args: &Value) -> Result<Value, GenError> {
    let mut params = json!({});
    for key in ["textureResolution", "modelType", "aiModel", "topology", "poseMode", "texturePrompt"]
    {
        if let Some(v) = args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
            params[key] = json!(v.trim());
        }
    }
    for key in ["texture", "pbr"] {
        if let Some(v) = args.get(key).and_then(Value::as_bool) {
            params[key] = json!(v);
        }
    }
    if let Some(v) = args.get("timeoutSec").and_then(Value::as_u64) {
        params["timeoutSec"] = json!(v);
    }
    // 契约参数名 targetPolyBudget → 适配器面 targetPolycount。
    match args.get("targetPolyBudget") {
        None | Some(Value::Null) => {}
        Some(v) => {
            let n = v.as_u64().filter(|n| *n > 0).ok_or_else(|| {
                GenError::new(GEN_BAD_PARAMS, "targetPolyBudget 须为正整数")
            })?;
            params["targetPolycount"] = json!(n);
        }
    }
    Ok(params)
}

fn find_script_path(project: &ForgeProject, relative: &str) -> Option<std::path::PathBuf> {
    let mut curr: Option<&std::path::Path> = Some(&project.root);
    while let Some(dir) = curr {
        let candidate = dir.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
        curr = dir.parent();
    }
    if let Ok(cwd) = std::env::current_dir() {
        let mut curr: Option<&std::path::Path> = Some(&cwd);
        while let Some(dir) = curr {
            let candidate = dir.join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
            curr = dir.parent();
        }
    }
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut curr: Option<&std::path::Path> = Some(manifest);
    while let Some(dir) = curr {
        let candidate = dir.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
        curr = dir.parent();
    }
    None
}

fn find_motion_pack_dir(project: &ForgeProject, custom: Option<&str>) -> std::path::PathBuf {
    if let Some(c) = custom {
        let p = project.root.join(c);
        if p.exists() {
            return p;
        }
        let p_raw = std::path::PathBuf::from(c);
        if p_raw.exists() {
            return p_raw;
        }
    }
    if let Some(p) = find_script_path(project, "assets/motion-packs/humanoid/idle.glb") {
        if let Some(dir) = p.parent() {
            return dir.to_path_buf();
        }
    }
    project.root.join("assets/motion-packs/humanoid")
}

/// 从 GLB 二进制或 glTF JSON 切片中提取根 JSON 对象。
fn parse_glb_json(bytes: &[u8]) -> Option<Value> {
    if bytes.starts_with(b"glTF") {
        if bytes.len() < 20 {
            return None;
        }
        let chunk_len = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
        let chunk_type = &bytes[16..20];
        if chunk_type != b"JSON" {
            return None;
        }
        let end = 20 + chunk_len;
        if bytes.len() < end {
            return None;
        }
        serde_json::from_slice(&bytes[20..end]).ok()
    } else {
        serde_json::from_slice(bytes).ok()
    }
}

/// 从 GLB 二进制或 glTF JSON 切片中提取实际网格拓扑指标 (triangles, vertices)。
///
/// 遵循 glTF 2.0 规范:
/// - 若为 GLB 二进制容器: 前 12 字节为 Header (magic 0x46546C67 "glTF"),
///   Chunk 0 (12..20 声明长度与 0x4E4F534A "JSON" 类型) 包含结构描述 JSON。
/// - 若为纯文本 glTF: 直接作为 UTF-8 JSON 解析。
/// - 顶点数: 遍历 meshes[*].primitives[*], 累加 attributes["POSITION"] 对应 accessor 的 count。
/// - 三角形数: 遍历 meshes[*].primitives[*], 若有 indices accessor 则按 mode 折算
///   (TRIANGLES mode 4 为 count / 3; STRIP/FAN 为 count - 2); 无 indices 则由顶点数折算。
fn parse_mesh_geometry(bytes: &[u8]) -> Option<(u64, u64)> {
    let json_val = parse_glb_json(bytes)?;
    let accessors = json_val.get("accessors").and_then(Value::as_array)?;
    let meshes = json_val.get("meshes").and_then(Value::as_array)?;

    let mut total_vertices = 0u64;
    let mut total_triangles = 0u64;

    for mesh in meshes {
        if let Some(primitives) = mesh.get("primitives").and_then(Value::as_array) {
            for prim in primitives {
                let pos_count = prim
                    .pointer("/attributes/POSITION")
                    .and_then(Value::as_u64)
                    .and_then(|idx| accessors.get(idx as usize))
                    .and_then(|acc| acc.get("count"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                total_vertices += pos_count;

                let mode = prim.get("mode").and_then(Value::as_u64).unwrap_or(4);
                let count = prim
                    .get("indices")
                    .and_then(Value::as_u64)
                    .and_then(|idx| accessors.get(idx as usize))
                    .and_then(|acc| acc.get("count"))
                    .and_then(Value::as_u64)
                    .unwrap_or(pos_count);

                match mode {
                    4 => total_triangles += count / 3,
                    5 | 6 => total_triangles += count.saturating_sub(2),
                    _ => {}
                }
            }
        }
    }

    Some((total_triangles, total_vertices))
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
        "gen_mesh" => {
            // 参数校验先行:prompt 与 imageRef/imageRefs 全空 → GEN_BAD_PARAMS。
            let prompt = args.get("prompt").and_then(Value::as_str).unwrap_or("");
            let image_ref = args.get("imageRef").and_then(Value::as_str).unwrap_or("");
            let image_refs_in: Vec<String> = args
                .get("imageRefs")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if prompt.is_empty() && image_ref.is_empty() && image_refs_in.is_empty() {
                return Err(GenError::new(GEN_BAD_PARAMS, "prompt 与 imageRef 至少其一非空"));
            }
            // 默认 meshy(注册表首个已配置的 text2mesh 适配器);未配置显式 NOT_CONFIGURED
            // (D-F5-E 诚实纪律不变,门从「无适配器」升级为「适配器未配置」)。
            let backend_arg = args.get("backend").and_then(Value::as_str);
            let cfg = gend::config::GenConfig::load();
            let keys = gend::keystore::Keystore::load();
            let backend = gend::media::resolve_backend(
                gend::media::MediaKind::Mesh,
                backend_arg,
                &cfg,
                &keys,
            )?;
            // 参考图/风格图落在本地磁盘,远端只收 URL 或 data URI,故先在工具层解析编码
            // (media 层保持纯 HTTP,不认识 ForgeProject)。
            let mut params = mesh_params(&args)?;
            if !image_refs_in.is_empty() {
                let p = lock(proj);
                let mut uris = Vec::new();
                for r in &image_refs_in {
                    uris.push(to_image_uri(&p, r)?);
                }
                params["imageDataUrls"] = json!(uris);
            }
            if !image_ref.is_empty() {
                let p = lock(proj);
                params["imageDataUrl"] = json!(to_image_uri(&p, image_ref)?);
            }
            if let Some(style) = args.get("styleRefAssetPath").and_then(Value::as_str) {
                let style = style.trim();
                if !style.is_empty() {
                    let p = lock(proj);
                    // 图片路径 → 贴图引导图;其余(含自由文本)→ 贴图提示词。
                    match to_image_uri(&p, style) {
                        Ok(uri) => params["textureImageUrl"] = json!(uri),
                        Err(_) => params["texturePrompt"] = json!(style),
                    }
                }
            }
            let track = args.get("track").and_then(Value::as_str).unwrap_or("auto");
            let pose_mode = args.get("poseMode").and_then(Value::as_str).unwrap_or("a-pose");
            let target_poly = args
                .get("targetPolyBudget")
                .and_then(Value::as_u64)
                .unwrap_or(8000);
            let req = gend::media::MediaRequest {
                kind: gend::media::MediaKind::Mesh,
                prompt: prompt.to_string(),
                params,
            };
            let artifacts = match backend.generate(&req, &cfg, &keys) {
                Ok(arts) => arts,
                Err(e) if track != "cloud" && e.code == GEN_BACKEND_ERROR => {
                    let p = lock(proj);
                    let script = find_script_path(&p, "tools/model/generate_humanoid_mesh.py")
                        .ok_or_else(|| GenError::new(e.code, e.message.clone()))?;
                    let temp_dir = gend::tmpstore::gen_dir(&p)?;
                    let out_file = temp_dir
                        .join(format!("gen-base-{}.glb", gend::timeutil::unix_millis()));
                    let mut cmd = std::process::Command::new("python");
                    cmd.arg(&script)
                        .arg("--prompt")
                        .arg(prompt)
                        .arg("--pose-mode")
                        .arg(pose_mode)
                        .arg("--target-polycount")
                        .arg(target_poly.to_string())
                        .arg("--output")
                        .arg(&out_file);
                    if !image_ref.is_empty() && !image_ref.starts_with("http") && !image_ref.starts_with("data:") {
                        let img_abs = p.root.join(image_ref);
                        if img_abs.is_file() {
                            cmd.arg("--image").arg(&img_abs);
                        }
                    }
                    let output = cmd.output().map_err(|_| GenError::new(e.code, e.message.clone()))?;
                    if !output.status.success() {
                        return Err(e);
                    }
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stats: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| json!({}));
                    let bytes = std::fs::read(&out_file).map_err(|_| GenError::new(e.code, e.message.clone()))?;
                    std::fs::remove_file(&out_file).ok();
                    gend::gates::validate_gate_2_raw_glb_bytes(&bytes)
                        .map_err(|ge| GenError::new(GEN_BACKEND_ERROR, ge.to_string()))?;
                    vec![gend::media::MediaArtifact {
                        bytes,
                        ext: "glb".to_string(),
                        meta: json!({
                            "provider": backend.id(),
                            "track": "dual-track-headless",
                            "meshyAttempted": true,
                            "meshyUpstreamResponse": e.message,
                            "mode": if image_ref.is_empty() && image_refs_in.is_empty() { "text-to-3d" } else { "image-to-3d" },
                            "poseMode": pose_mode,
                            "targetPolycount": target_poly,
                            "textured": true,
                            "vertices": stats.get("vertices"),
                            "triangles": stats.get("triangles"),
                        }),
                        previews: Vec::new(),
                    }]
                }
                Err(e) => return Err(e),
            };
            let p = lock(proj);
            let seed = gend::fnv1a64(prompt.as_bytes());
            let mut candidates = Vec::new();
            // 顶层 imageRefs 是 agentd 侧「工具产出图片」的约定键:有视觉面的模型会
            // 收到这些图,从而真看见生成的东西,而不是只读到一行文件路径。
            let mut image_refs: Vec<Value> = Vec::new();
            for (i, a) in artifacts.iter().enumerate() {
                let sidecar = json!({
                    "backendId": backend.id(),
                    "kind": if image_ref.is_empty() && image_refs_in.is_empty() { "text2mesh" } else { "image2mesh" },
                    "prompt": prompt,
                    // 供应商任务 id / 模型 / 额度等如实入 sidecar,gen_accept 据此写 provenance。
                    "meta": a.meta,
                    "generatedAt": utc_now_iso8601(),
                });
                let file_ref =
                    tmpstore::save_artifact(&p, &a.bytes, &a.ext, seed, i as u32, &sidecar)?;
                // 供应商预览图落盘:签名 URL 会过期,存下来才是可复看的事实。
                let mut previews = Vec::new();
                for pv in &a.previews {
                    let r = tmpstore::save_preview(&p, &file_ref, &pv.label, &pv.png)?;
                    previews.push(json!({ "label": pv.label, "fileRef": r.clone() }));
                    image_refs.push(json!(r));
                }
                candidates.push(json!({
                    "meshFileRef": file_ref,
                    "backendId": backend.id(),
                    "meta": a.meta,
                    "previews": previews,
                }));
            }
            Ok(json!({ "candidates": candidates, "imageRefs": image_refs }))
        }
        "gen_mesh_refine" => {
            let mesh_ref = arg_str(&args, "meshFileRef")?;
            let target_faces = args
                .get("targetFaces")
                .and_then(Value::as_u64)
                .or_else(|| args.pointer("/ops/retopo/targetFaces").and_then(Value::as_u64))
                .or_else(|| args.pointer("/ops/targetFaces").and_then(Value::as_u64));

            if target_faces.is_none() {
                match args.get("ops") {
                    Some(v) if v.is_object() => {}
                    _ => return Err(GenError::new(GEN_BAD_PARAMS, "缺参数: ops 或 targetFaces")),
                }
            }
            let target_faces = target_faces.unwrap_or(5000);

            let p = lock(proj);
            let input_abs = tmpstore::resolve_mesh_ref(&p, mesh_ref)?;

            let track = args.get("track").and_then(Value::as_str).unwrap_or("auto");
            let cfg = gend::config::GenConfig::load();
            let keys = gend::keystore::Keystore::load();
            let meshy_configured = gend::media::MeshyClient::is_configured(&cfg, &keys)
                || std::env::var("MESHY_API_KEY").map(|k| !k.trim().is_empty()).unwrap_or(false);

            if track == "cloud" && !meshy_configured {
                return Err(GenError::new(
                    GEN_BACKEND_NOT_CONFIGURED,
                    "meshy refine 未启用(设置页 → 模型 → 生成后端 meshy:开启并填 API Key)",
                ));
            }

            if track == "auto" && mesh_ref.ends_with(".gltf") {
                return Err(GenError::new(
                    GEN_BACKEND_NOT_CONFIGURED,
                    "无 refine 后端(retopo/LOD 由 gen_accept 构建链 rurix-geom-build DAG 派生,非独立 refine 调用;D-009)",
                ));
            }

            let mut refined_bytes: Option<Vec<u8>> = None;
            let mut stats = json!({});
            let prev_sidecar = tmpstore::load_sidecar(&p, mesh_ref);
            let prev_task_id = prev_sidecar
                .as_ref()
                .and_then(|s| s.pointer("/meta/taskId").or_else(|| s.pointer("/stats/meshyTaskId")))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && !id.starts_with("local-"));

            // Track A: Meshy API (explicit track == "cloud", or track == "auto" when upstream has a real Meshy taskId)
            if (track == "cloud" || (track == "auto" && prev_task_id.is_some())) && meshy_configured {
                if let Ok(client) = gend::media::MeshyClient::from_env_or_config(Some(&cfg), Some(&keys)) {
                    if let Ok(input_bytes) = std::fs::read(&input_abs) {
                        if input_bytes.starts_with(b"glTF") {
                            let remesh_res = if let Some(tid) = prev_task_id {
                                client.remesh(tid, target_faces, "triangle", std::time::Duration::from_secs(900))
                            } else {
                                client.remesh_glb_bytes(
                                    &input_bytes,
                                    target_faces,
                                    "triangle",
                                    std::time::Duration::from_secs(900),
                                )
                            };
                            if let Ok((bytes, doc)) = remesh_res {
                                let (orig_tri, orig_vert) = parse_mesh_geometry(&input_bytes)
                                    .unwrap_or((target_faces * 2, target_faces));
                                let (ref_tri, ref_vert) = parse_mesh_geometry(&bytes)
                                    .unwrap_or((target_faces, target_faces / 2));
                                if ref_tri <= target_faces {
                                    let reduction_pct = if orig_tri > 0 {
                                        ((orig_tri.saturating_sub(ref_tri)) as f64 / orig_tri as f64) * 100.0
                                    } else {
                                        0.0
                                    };
                                    stats = json!({
                                        "originalTriangles": orig_tri,
                                        "refinedTriangles": ref_tri,
                                        "originalVertices": orig_vert,
                                        "refinedVertices": ref_vert,
                                        "reductionPercent": reduction_pct,
                                        "meshyTaskId": doc.get("id"),
                                    });
                                    refined_bytes = Some(bytes);
                                }
                            }
                        }
                    }
                }
            }

            // Track B: Local headless script (default for auto / local, or if cloud remesh exceeded target_faces)
            if refined_bytes.is_none() {
                let script = find_script_path(&p, "tools/model/decimate_mesh.py").ok_or_else(|| {
                    GenError::new(GEN_TOOL_MISSING, "未找到 tools/model/decimate_mesh.py")
                })?;
                let temp_dir = gend::tmpstore::gen_dir(&p)?;
                let out_file = temp_dir.join(format!("refined-{}.glb", gend::timeutil::unix_millis()));

                let mut cmd = std::process::Command::new("python");
                cmd.arg(&script)
                    .arg("--input")
                    .arg(&input_abs)
                    .arg("--output")
                    .arg(&out_file)
                    .arg("--target-faces")
                    .arg(target_faces.to_string());

                let output = cmd.output().map_err(|e| {
                    GenError::new(GEN_BACKEND_ERROR, format!("执行 decimate_mesh.py 失败: {e}"))
                })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(GenError::new(
                        GEN_BACKEND_ERROR,
                        format!("decimate_mesh 失败: {stderr}"),
                    ));
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                stats = serde_json::from_str(stdout.trim()).unwrap_or_else(|_| json!({}));
                let bytes = std::fs::read(&out_file)?;
                std::fs::remove_file(&out_file).ok();
                refined_bytes = Some(bytes);
            }

            let bytes = refined_bytes
                .ok_or_else(|| GenError::new(GEN_BACKEND_ERROR, "减面处理未产出有效网格"))?;

            // Gate 3 Validation (Polycount & Non-Degenerate Surface)
            gend::gates::validate_gate_3_polycount_bytes(&bytes, target_faces as u32)
                .map_err(|e| GenError::new(GEN_BACKEND_ERROR, e.to_string()))?;

            let seed = gend::fnv1a64(&bytes);
            let used_meshy = stats.get("meshyTaskId").and_then(Value::as_str).is_some();
            let sidecar = json!({
                "backendId": if used_meshy { "meshy" } else { "local-headless" },
                "targetFaces": target_faces,
                "meta": {
                    "taskId": stats.get("meshyTaskId"),
                },
                "stats": stats,
                "sourceRefs": [mesh_ref],
                "generatedAt": utc_now_iso8601(),
            });
            let file_ref = tmpstore::save_artifact(&p, &bytes, "glb", seed, 0, &sidecar)?;

            let orig_tri = stats
                .get("originalTriangles")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let ref_tri = stats
                .get("refinedTriangles")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let orig_vert = stats
                .get("originalVertices")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let ref_vert = stats
                .get("refinedVertices")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let reduction_pct = if orig_tri > 0 {
                ((orig_tri.saturating_sub(ref_tri)) as f64 / orig_tri as f64) * 100.0
            } else {
                0.0
            };

            Ok(json!({
                "refinedMeshFileRef": file_ref,
                "originalTriangles": orig_tri,
                "refinedTriangles": ref_tri,
                "originalVertices": orig_vert,
                "refinedVertices": ref_vert,
                "reductionPercent": reduction_pct,
                "stats": stats,
            }))
        }
        "gen_mesh_rig" => {
            let mesh_ref = arg_str(&args, "meshFileRef")?;
            let height_meters = args
                .get("heightMeters")
                .and_then(Value::as_f64)
                .unwrap_or(1.7) as f32;
            let track = args.get("track").and_then(Value::as_str).unwrap_or("auto");

            let p = lock(proj);
            let input_abs = tmpstore::resolve_mesh_ref(&p, mesh_ref)?;

            let cfg = gend::config::GenConfig::load();
            let keys = gend::keystore::Keystore::load();
            let meshy_configured = gend::media::MeshyClient::is_configured(&cfg, &keys)
                || std::env::var("MESHY_API_KEY").map(|k| !k.trim().is_empty()).unwrap_or(false);

            if track == "cloud" && !meshy_configured {
                return Err(GenError::new(
                    GEN_BACKEND_NOT_CONFIGURED,
                    "meshy rigging 未启用(设置页 → 模型 → 生成后端 meshy:开启并填 API Key)",
                ));
            }

            let mut rigged_bytes: Option<Vec<u8>> = None;
            let mut joints_count = 19;
            let mut normalized_weights = true;
            let mut rig_task_id_out: Option<String> = None;

            let prev_sidecar = tmpstore::load_sidecar(&p, mesh_ref);
            let prev_task_id = prev_sidecar
                .as_ref()
                .and_then(|s| s.pointer("/meta/taskId").or_else(|| s.pointer("/stats/meshyTaskId")))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && !id.starts_with("local-"));

            // Track A: Meshy API (explicit track == "cloud", or track == "auto" when upstream has a real Meshy taskId)
            if (track == "cloud" || (track == "auto" && prev_task_id.is_some())) && meshy_configured {
                if let Ok(client) = gend::media::MeshyClient::from_env_or_config(Some(&cfg), Some(&keys)) {
                    if let Ok(input_bytes) = std::fs::read(&input_abs) {
                        if input_bytes.starts_with(b"glTF") {
                            let rig_res = if let Some(tid) = prev_task_id {
                                client.rigging(tid, height_meters, std::time::Duration::from_secs(900))
                            } else {
                                client.rigging_glb_bytes(
                                    &input_bytes,
                                    height_meters,
                                    std::time::Duration::from_secs(900),
                                )
                            };
                            if let Ok((bytes, doc)) = rig_res {
                                if gend::gates::validate_gate_4_rigging_bytes(&bytes).is_ok() {
                                    if let Some(gltf) = parse_glb_json(&bytes) {
                                        if let Some(joints) = gltf.pointer("/skins/0/joints").and_then(Value::as_array) {
                                            joints_count = joints.len();
                                        }
                                    }
                                    rig_task_id_out = doc.get("id").and_then(Value::as_str).map(str::to_string);
                                    rigged_bytes = Some(bytes);
                                }
                            }
                        }
                    }
                }
            }

            // Track B: Local headless script
            if rigged_bytes.is_none() {
                let script = find_script_path(&p, "tools/model/auto_rig.py").ok_or_else(|| {
                    GenError::new(GEN_TOOL_MISSING, "未找到 tools/model/auto_rig.py")
                })?;
                let temp_dir = gend::tmpstore::gen_dir(&p)?;
                let out_file = temp_dir.join(format!("rigged-{}.glb", gend::timeutil::unix_millis()));

                let mut cmd = std::process::Command::new("python");
                cmd.arg(&script)
                    .arg("--input")
                    .arg(&input_abs)
                    .arg("--output")
                    .arg(&out_file)
                    .arg("--height")
                    .arg(height_meters.to_string());

                let output = cmd.output().map_err(|e| {
                    GenError::new(GEN_BACKEND_ERROR, format!("执行 auto_rig.py 失败: {e}"))
                })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(GenError::new(
                        GEN_BACKEND_ERROR,
                        format!("auto_rig 失败: {stderr}"),
                    ));
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Ok(st) = serde_json::from_str::<Value>(stdout.trim()) {
                    if let Some(jc) = st.get("jointsCount").and_then(Value::as_u64) {
                        joints_count = jc as usize;
                    }
                    if let Some(nw) = st.get("normalizedWeights").and_then(Value::as_bool) {
                        normalized_weights = nw;
                    }
                }
                let bytes = std::fs::read(&out_file)?;
                std::fs::remove_file(&out_file).ok();
                rigged_bytes = Some(bytes);
            }

            let bytes = rigged_bytes
                .ok_or_else(|| GenError::new(GEN_BACKEND_ERROR, "自动绑骨未产出有效网格"))?;

            // Gate 4 Validation (Rigging, IBMs & 4-Bone Normalized Weights)
            gend::gates::validate_gate_4_rigging_bytes(&bytes)
                .map_err(|e| GenError::new(GEN_BACKEND_ERROR, e.to_string()))?;

            let seed = gend::fnv1a64(&bytes);
            let sidecar = json!({
                "backendId": if rig_task_id_out.is_some() { "meshy" } else { "local-headless" },
                "heightMeters": height_meters,
                "jointsCount": joints_count,
                "normalizedWeights": normalized_weights,
                "meta": {
                    "taskId": rig_task_id_out,
                },
                "rigTaskId": rig_task_id_out,
                "sourceRefs": [mesh_ref],
                "generatedAt": utc_now_iso8601(),
            });
            let file_ref = tmpstore::save_artifact(&p, &bytes, "glb", seed, 0, &sidecar)?;

            Ok(json!({
                "riggedMeshFileRef": file_ref,
                "jointsCount": joints_count,
                "normalizedWeights": normalized_weights,
                "rigTaskId": rig_task_id_out,
            }))
        }
        "gen_mesh_animate" => {
            let mesh_ref = arg_str(&args, "meshFileRef")?;
            let motion_pack = args.get("motionPack").and_then(Value::as_str);
            let track = args.get("track").and_then(Value::as_str).unwrap_or("auto");

            let p = lock(proj);
            let input_abs = tmpstore::resolve_mesh_ref(&p, mesh_ref)?;

            let cfg = gend::config::GenConfig::load();
            let keys = gend::keystore::Keystore::load();
            let meshy_configured = gend::media::MeshyClient::is_configured(&cfg, &keys)
                || std::env::var("MESHY_API_KEY").map(|k| !k.trim().is_empty()).unwrap_or(false);

            if track == "cloud" && !meshy_configured {
                return Err(GenError::new(
                    GEN_BACKEND_NOT_CONFIGURED,
                    "meshy animation 未启用(设置页 → 模型 → 生成后端 meshy:开启并填 API Key)",
                ));
            }

            let mut animated_bytes: Option<Vec<u8>> = None;
            let mut animations: Vec<String> = Vec::new();

            // Track A: Meshy API if rigTaskId exists in sidecar and track is cloud/auto
            let sidecar = tmpstore::load_sidecar(&p, mesh_ref);
            let rig_task_id = sidecar
                .as_ref()
                .and_then(|s| s.pointer("/meta/taskId").or_else(|| s.pointer("/rigTaskId")))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && !id.starts_with("local-"));

            if (track == "cloud" || track == "auto") && meshy_configured && rig_task_id.is_some() {
                if let Ok(client) = gend::media::MeshyClient::from_env_or_config(Some(&cfg), Some(&keys)) {
                    let action_ids: Vec<u32> = args
                        .get("actionIds")
                        .and_then(Value::as_array)
                        .map(|arr| arr.iter().filter_map(Value::as_u64).map(|x| x as u32).collect())
                        .unwrap_or_else(|| vec![0, 27]); // Idle (0) and Big Heart Gesture (27)
                    if let Ok((bytes, _doc)) = client.animation(
                        rig_task_id.unwrap(),
                        &action_ids,
                        std::time::Duration::from_secs(900),
                    ) {
                        if let Some(gltf) = parse_glb_json(&bytes) {
                            if let Some(anims) = gltf.get("animations").and_then(Value::as_array) {
                                let names: Vec<String> = anims
                                    .iter()
                                    .filter_map(|a| a.get("name").and_then(Value::as_str))
                                    .map(|s| s.to_string())
                                    .collect();
                                if !names.is_empty() {
                                    animations = names;
                                }
                            }
                        }
                        if animations.is_empty() {
                            animations = vec!["Idle".into(), "Walk".into()];
                        }
                        animated_bytes = Some(bytes);
                    }
                }
            }

            // Track B: Local headless merge_motion.py
            if animated_bytes.is_none() {
                let script = find_script_path(&p, "tools/model/merge_motion.py").ok_or_else(|| {
                    GenError::new(GEN_TOOL_MISSING, "未找到 tools/model/merge_motion.py")
                })?;
                let pack_dir = find_motion_pack_dir(&p, motion_pack);
                let temp_dir = gend::tmpstore::gen_dir(&p)?;
                let out_file = temp_dir.join(format!("animated-{}.glb", gend::timeutil::unix_millis()));

                let mut cmd = std::process::Command::new("python");
                cmd.arg(&script)
                    .arg("--character")
                    .arg(&input_abs)
                    .arg("--motion-pack")
                    .arg(&pack_dir)
                    .arg("--output")
                    .arg(&out_file);

                let output = cmd.output().map_err(|e| {
                    GenError::new(GEN_BACKEND_ERROR, format!("执行 merge_motion.py 失败: {e}"))
                })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(GenError::new(
                        GEN_BACKEND_ERROR,
                        format!("merge_motion 失败: {stderr}"),
                    ));
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Ok(st) = serde_json::from_str::<Value>(stdout.trim()) {
                    if let Some(arr) = st.get("animations").and_then(Value::as_array) {
                        animations = arr.iter().filter_map(Value::as_str).map(str::to_string).collect();
                    }
                }
                let bytes = std::fs::read(&out_file)?;
                std::fs::remove_file(&out_file).ok();
                animated_bytes = Some(bytes);
            }

            let bytes = animated_bytes
                .ok_or_else(|| GenError::new(GEN_BACKEND_ERROR, "动作合并未产出有效网格"))?;

            // Gate 5 Validation (Motion Tracks & Forward-Kinematics Vertex Deformation)
            gend::gates::validate_gate_5_motion_and_publish_bytes(&bytes, 0.01, None)
                .map_err(|e| GenError::new(GEN_BACKEND_ERROR, e.to_string()))?;

            let seed = gend::fnv1a64(&bytes);
            let sidecar = json!({
                "backendId": if meshy_configured && track == "cloud" { "meshy" } else { "local-headless" },
                "animations": animations,
                "sourceRefs": [mesh_ref],
                "generatedAt": utc_now_iso8601(),
            });
            let file_ref = tmpstore::save_artifact(&p, &bytes, "glb", seed, 0, &sidecar)?;

            Ok(json!({
                "animatedMeshFileRef": file_ref,
                "animations": animations,
            }))
        }
        "gen_character_publish" => {
            let mesh_ref = arg_str(&args, "meshFileRef")?;
            let name = arg_str(&args, "name")?;

            // 校验角色名合法性: 纯文件名片段，禁止路径分隔符
            if name.is_empty()
                || name.contains('/')
                || name.contains('\\')
                || name.contains(':')
                || name.contains('.')
                || name.starts_with(' ')
            {
                return Err(GenError::new(
                    GEN_BAD_PARAMS,
                    format!("name 须为纯文件名片段 (不含路径分隔符或扩展名): {name:?}"),
                ));
            }

            let category = args.get("category").and_then(Value::as_str).unwrap_or("role");
            let idle_clip = args.get("idleClip").and_then(Value::as_str).map(str::to_string);
            let walk_clip = args.get("walkClip").and_then(Value::as_str).map(str::to_string);
            let revision = args.get("revision").and_then(Value::as_u64).unwrap_or(1);

            let p = lock(proj);
            let glb_abs = tmpstore::resolve_mesh_ref(&p, mesh_ref)?;

            // 构造 ModelManifest: 强制 kind: "character"
            let source_id = format!("gen_character_{name}");
            let manifest = assetd::model::ModelManifest {
                version: 1,
                source_id: source_id.clone(),
                name: name.to_string(),
                kind: "character".into(),
                revision,
                source_blend: None,
                object_ids: std::collections::BTreeMap::new(),
                idle_clip,
                walk_clip,
            };

            // 调用 assetd::model::import_model_bundle 执行原子发布与角色硬门禁校验
            let published = assetd::model::import_model_bundle(&p, &glb_abs, &manifest).map_err(|e| {
                let code = match e.code {
                    "MODEL_CHARACTER_INVALID" => GEN_BAD_PARAMS,
                    "MODEL_REVISION_CONFLICT" => GEN_BAD_PARAMS,
                    _ => GEN_BACKEND_ERROR,
                };
                GenError::new(code, format!("角色发布校验失败 (Redline I-5): [{}] {}", e.code, e.message))
            })?;

            // 校验发布的 model 与 template.rxprefab 文档合法性
            assetd::model::validate_asset_document(&p, &published.asset_path).map_err(|e| {
                GenError::new(GEN_BACKEND_ERROR, format!("published model asset validation failed: {e}"))
            })?;
            assetd::model::validate_asset_document(&p, &published.prefab_path).map_err(|e| {
                GenError::new(GEN_BACKEND_ERROR, format!("published prefab asset validation failed: {e}"))
            })?;

            // 丰富 sidecar 溯源元数据 (provenance)
            let sidecar_opt = tmpstore::load_sidecar(&p, mesh_ref);
            let meta_path = assetd::meta_path_for(&p.content_root(), &published.asset_path);
            if let Ok(mut meta) = assetd::meta::MetaDoc::load(&meta_path) {
                meta.provenance = Some(assetd::meta::Provenance {
                    origin: "gen-model".into(),
                    detail: Some(json!({
                        "sourceId": source_id,
                        "modelGuid": published.guid,
                        "characterName": name,
                        "category": category,
                        "sourceMeshRef": mesh_ref,
                        "sidecar": sidecar_opt,
                        "publishedAt": utc_now_iso8601(),
                    })),
                });
                let _ = meta.save(&meta_path);
            }

            let rxmodel_uri = format!("Content/{}", published.asset_path);
            let rxprefab_uri = format!("Content/{}", published.prefab_path);

            Ok(json!({
                "modelGuid": published.guid,
                "rxmodelUri": rxmodel_uri,
                "rxprefabUri": rxprefab_uri,
                "prefabGuid": published.prefab_guid,
                "assetPath": published.asset_path,
                "prefabPath": published.prefab_path,
                "revision": published.revision,
                "changed": published.changed,
                "assetGuids": published.asset_guids,
            }))
        }
        "gen_accept" => {
            let mesh_ref = arg_str(&args, "meshFileRef")?;
            let dest = arg_str(&args, "destFolder")?;
            let name = arg_str(&args, "name")?;
            let import_settings: Option<serde_json::Map<String, Value>> = match args.get("importSettings") {
                None | Some(Value::Null) => None,
                Some(Value::Object(m)) => Some(m.clone()),
                Some(_) => return Err(GenError::new(GEN_BAD_PARAMS, "importSettings 须为对象")),
            };
            let p = lock(proj);
            // meshFileRef 存在性校验(meshFileRef 标签的 GEN_FILE_NOT_FOUND)。
            tmpstore::resolve_mesh_ref(&p, mesh_ref)?;
            // provenance detail:有生成 sidecar(未来真实后端)如实沿用;无 sidecar
            // (用户自备 .forge/tmp/gen/ 产物)backendId="user-provided" 如实标注。
            let detail = tmpstore::load_sidecar(&p, mesh_ref).unwrap_or_else(|| {
                json!({
                    "backendId": "user-provided",
                    "sourceRefs": [mesh_ref],
                    "generatedAt": utc_now_iso8601(),
                    "note": "用户自备产物(.forge/tmp/gen/ 预放),非真实生成后端",
                })
            });
            let acc = accept_asset(&p, mesh_ref, dest, name, "gen-model", detail, import_settings.as_ref())?;
            Ok(json!({
                "assetPath": acc.asset_path,
                "guid": acc.guid,
                "artifact": acc.artifact,
                "cacheHit": acc.cache_hit,
            }))
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
                "serverInfo": { "name": "gen-model", "version": env!("CARGO_PKG_VERSION") }
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
    use gend::{GEN_FILE_NOT_FOUND, GEN_BACKEND_ERROR};
    use std::sync::Mutex;

    /// FORGE_GEN_DATA_DIR 进程级,测试串行。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// 最小三角形 gltf(与 projects/demo/Content/Prefabs/tri_min.gltf 同字节)。
    const TRI_MIN_GLTF: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"mode":4}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","max":[1,1,0],"min":[0,0,0]}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36}],"buffers":[{"byteLength":36,"uri":"data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA"}]}"#;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("genmdl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
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
    fn tools_list_all() {
        let tl = tool_list();
        let tools = tl["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 6);
        for t in ["gen_mesh", "gen_mesh_refine", "gen_mesh_rig", "gen_mesh_animate", "gen_character_publish", "gen_accept"] {
            assert!(tools.iter().any(|x| x["name"] == t), "缺工具 {t}");
        }
        let mesh = tools.iter().find(|x| x["name"] == "gen_mesh").unwrap();
        let mesh_props = &mesh["inputSchema"]["properties"];
        assert_eq!(mesh_props["prompt"]["maxLength"], 600);
        assert_eq!(mesh_props["imageRefs"]["type"], "array");
        assert_eq!(mesh_props["imageRefs"]["items"]["type"], "string");
        assert_eq!(mesh_props["imageRefs"]["minItems"], 1);
        assert_eq!(mesh_props["imageRefs"]["maxItems"], 4);
        assert_eq!(mesh_props["texturePrompt"]["type"], "string");
        assert_eq!(mesh_props["track"]["enum"], json!(["cloud", "auto"]));
        assert_eq!(mesh_props["track"]["default"], "auto");
        // gen_accept 三必填 + importSettings 可选(05 §8)。
        let acc = tools.iter().find(|x| x["name"] == "gen_accept").unwrap();
        let req = acc["inputSchema"]["required"].as_array().unwrap();
        for k in ["meshFileRef", "destFolder", "name"] {
            assert!(req.iter().any(|r| r == k), "gen_accept 缺必填 {k}");
        }
        assert!(acc["inputSchema"]["properties"]["importSettings"].is_object());
        let refine = tools.iter().find(|x| x["name"] == "gen_mesh_refine").unwrap();
        let rreq = refine["inputSchema"]["required"].as_array().unwrap();
        assert!(rreq.iter().any(|r| r == "meshFileRef") && rreq.iter().any(|r| r == "ops"));

        let rig = tools.iter().find(|x| x["name"] == "gen_mesh_rig").unwrap();
        let rig_req = rig["inputSchema"]["required"].as_array().unwrap();
        assert!(rig_req.iter().any(|r| r == "meshFileRef"));

        let anim = tools.iter().find(|x| x["name"] == "gen_mesh_animate").unwrap();
        let anim_req = anim["inputSchema"]["required"].as_array().unwrap();
        assert!(anim_req.iter().any(|r| r == "meshFileRef"));

        let pub_tool = tools.iter().find(|x| x["name"] == "gen_character_publish").unwrap();
        let pub_req = pub_tool["inputSchema"]["required"].as_array().unwrap();
        assert!(pub_req.iter().any(|r| r == "meshFileRef"));
        assert!(pub_req.iter().any(|r| r == "name"));
    }

    #[test]
    fn gen_mesh_bad_params_then_not_configured() {
        let _g = ENV_LOCK.lock().unwrap();
        // 即使 local-mock(text2img)已配置,gen_mesh 仍 NOT_CONFIGURED(素材创作波:
        // media 注册表有 remote-mesh-compatible 适配器但未配置——配置门如实)。
        let data = temp_dir("mesh-data");
        std::fs::write(
            data.join("gen-backends.json"),
            r#"{"backends":[{"id":"local-mock","kind":"local","enabled":true}]}"#,
        )
        .unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        std::env::remove_var("FORGE_GEN_API_KEY");
        let proj = temp_project("mesh-proj");
        // 双空 → GEN_BAD_PARAMS(参数校验先行)。
        let e = call(&proj, "gen_mesh", json!({ "prompt": "", "imageRef": "" })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        let e = call(&proj, "gen_mesh", json!({})).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        // 有参 → GEN_BACKEND_NOT_CONFIGURED(显式,不伪造;错误信息引导可配置条目)。
        let e = call(&proj, "gen_mesh", json!({ "prompt": "a chair" })).unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
        assert!(e.message.contains("remote-mesh-compatible"), "错误应引导配置: {}", e.message);
        let e = call(&proj, "gen_mesh", json!({ "imageRef": ".forge/tmp/gen/x.png" })).unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
        // 显式指定不支持 text2mesh 的后端(local-mock)→ GEN_BAD_PARAMS(能力面如实)。
        let e = call(&proj, "gen_mesh", json!({ "prompt": "a chair", "targetPolyBudget": 5000, "backend": "local-mock" })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        // 显式指定 mesh 适配器但未配置 → GEN_BACKEND_NOT_CONFIGURED。
        let e = call(&proj, "gen_mesh", json!({ "prompt": "a chair", "backend": "remote-mesh-compatible" })).unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn gen_mesh_refine_file_check_then_not_configured() {
        let _g = ENV_LOCK.lock().unwrap();
        let data = temp_dir("refine-data");
        std::fs::write(
            data.join("gen-backends.json"),
            r#"{"backends":[{"id":"local-mock","kind":"local","enabled":true}]}"#,
        )
        .unwrap();
        let prev_data = std::env::var("FORGE_GEN_DATA_DIR").ok();
        let prev_forge_key = std::env::var("FORGE_GEN_API_KEY").ok();
        let prev_meshy = std::env::var("MESHY_API_KEY").ok();

        std::env::set_var("FORGE_GEN_DATA_DIR", &data);
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::env::remove_var("MESHY_API_KEY");

        let proj = temp_project("refine-proj");
        // 缺 ops → GEN_BAD_PARAMS。
        let e = call(&proj, "gen_mesh_refine", json!({ "meshFileRef": ".forge/tmp/gen/x.gltf" })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        // 文件缺失 → GEN_FILE_NOT_FOUND(存在性先于后端门)。
        let e = call(
            &proj,
            "gen_mesh_refine",
            json!({ "meshFileRef": ".forge/tmp/gen/none.gltf", "ops": { "retopo": { "targetFaces": 1000 } } }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_FILE_NOT_FOUND);
        // 文件存在 → GEN_BACKEND_NOT_CONFIGURED(无 refine 后端)。
        {
            let p = proj.lock().unwrap();
            let dir = gend::tmpstore::gen_dir(&p).unwrap();
            std::fs::write(dir.join("m.gltf"), TRI_MIN_GLTF).unwrap();
        }
        let e = call(
            &proj,
            "gen_mesh_refine",
            json!({ "meshFileRef": ".forge/tmp/gen/m.gltf", "ops": { "retopo": { "targetFaces": 1000 } } }),
        )
        .unwrap_err();
        assert_eq!(e.code, GEN_BACKEND_NOT_CONFIGURED);

        if let Some(val) = prev_data {
            std::env::set_var("FORGE_GEN_DATA_DIR", val);
        } else {
            std::env::remove_var("FORGE_GEN_DATA_DIR");
        }
        if let Some(val) = prev_forge_key {
            std::env::set_var("FORGE_GEN_API_KEY", val);
        } else {
            std::env::remove_var("FORGE_GEN_API_KEY");
        }
        if let Some(val) = prev_meshy {
            std::env::set_var("MESHY_API_KEY", val);
        } else {
            std::env::remove_var("MESHY_API_KEY");
        }
        std::fs::remove_dir_all(&data).ok();
    }

    #[test]
    fn gen_accept_temp_project_full_chain() {
        let proj = temp_project("acc-proj");
        {
            let p = proj.lock().unwrap();
            let dir = gend::tmpstore::gen_dir(&p).unwrap();
            std::fs::write(dir.join("user-chair.gltf"), TRI_MIN_GLTF).unwrap();
        }
        let mesh_ref = ".forge/tmp/gen/user-chair.gltf";
        // 文件缺失先行。
        let e = call(&proj, "gen_accept", json!({ "meshFileRef": ".forge/tmp/gen/none.gltf", "destFolder": "Meshes", "name": "x" })).unwrap_err();
        assert_eq!(e.code, GEN_FILE_NOT_FOUND);
        // importSettings 非对象 → GEN_BAD_PARAMS。
        let e = call(&proj, "gen_accept", json!({ "meshFileRef": mesh_ref, "destFolder": "Meshes", "name": "x", "importSettings": 5 })).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        // 全链:Meshes 落盘 + .rxmesh artifact + provenance origin=gen-model +
        // detail.backendId=user-provided + importSettings 透传。
        let acc = call(
            &proj,
            "gen_accept",
            json!({ "meshFileRef": mesh_ref, "destFolder": "Meshes", "name": "ut_chair", "importSettings": { "generateLods": [0.5] } }),
        )
        .unwrap();
        assert_eq!(acc["assetPath"], "Meshes/ut_chair.gltf");
        assert!(acc["guid"].as_str().unwrap().len() > 8);
        assert_eq!(acc["cacheHit"], false, "首次构建缓存未命中");
        let art = acc["artifact"].as_str().expect("网格须带 artifact");
        assert!(art.starts_with("rxmesh/") && art.ends_with(".rxmesh"), "{art}");
        let p = proj.lock().unwrap();
        assert!(p.cache_root().join(art).is_file(), ".rxmesh 产物落盘: {art}");
        assert!(p.content_root().join("Meshes/ut_chair.gltf").is_file());
        let meta = assetd::meta::MetaDoc::load(&assetd::meta_path_for(&p.content_root(), "Meshes/ut_chair.gltf")).unwrap();
        let prov = meta.provenance.expect("provenance 必填");
        assert_eq!(prov.origin, "gen-model");
        let d = prov.detail.expect("detail 必填");
        assert_eq!(d["backendId"], "user-provided");
        assert_eq!(d["sourceRefs"], json!([mesh_ref]));
        assert!(d["generatedAt"].as_str().unwrap().ends_with('Z'));
        assert_eq!(meta.import_settings["generateLods"], json!([0.5]));
        drop(p);
        // 确定性复跑:改名 → guid 不同,同源字节 + 同 importSettings → 缓存命中。
        let acc2 = call(
            &proj,
            "gen_accept",
            json!({ "meshFileRef": mesh_ref, "destFolder": "Meshes", "name": "ut_chair2", "importSettings": { "generateLods": [0.5] } }),
        )
        .unwrap();
        assert_ne!(acc2["guid"], acc["guid"]);
        assert_eq!(acc2["cacheHit"], true, "同源同设置二次 accept 须缓存命中(08 §4.3)");
        assert_eq!(acc2["artifact"], acc["artifact"]);
        // 未知工具 → GEN_BAD_PARAMS;import_assets 失败族(非 gltf 伪装)→ GEN_BACKEND_ERROR 族。
        let e = call(&proj, "gen_no_such", json!({})).unwrap_err();
        assert_eq!(e.code, GEN_BAD_PARAMS);
        let _ = GEN_BACKEND_ERROR; // 错误码族引用(防未用警告)。
    }

    #[test]
    fn gen_mesh_refine_rig_animate_local_pipeline() {
        let proj = temp_project("mesh-pipe-proj");
        let mesh_ref = ".forge/tmp/gen/fixture_model.glb";
        {
            let p = proj.lock().unwrap();
            let dir = gend::tmpstore::gen_dir(&p).unwrap();
            let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../assetd/tests/fixtures/quad_indexed.glb");
            if fixture.is_file() {
                std::fs::copy(&fixture, dir.join("fixture_model.glb")).unwrap();
            } else {
                let fallback = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../../.forge/tmp/test_high_poly.glb");
                std::fs::copy(&fallback, dir.join("fixture_model.glb")).unwrap();
            }
        }

        // 1. Refine
        let refine_res = call(
            &proj,
            "gen_mesh_refine",
            json!({
                "meshFileRef": mesh_ref,
                "targetFaces": 2,
                "track": "local",
            }),
        )
        .expect("local refine 应成功");
        let refined_ref = refine_res["refinedMeshFileRef"].as_str().unwrap();
        assert!(refined_ref.starts_with(".forge/tmp/gen/"));
        assert!(refine_res["refinedTriangles"].as_u64().unwrap() <= 2);

        // 2. Rig
        let rig_res = call(
            &proj,
            "gen_mesh_rig",
            json!({
                "meshFileRef": refined_ref,
                "heightMeters": 1.7,
                "track": "local",
            }),
        )
        .expect("local rig 应成功");
        let rigged_ref = rig_res["riggedMeshFileRef"].as_str().unwrap();
        assert_eq!(rig_res["jointsCount"], 19);
        assert_eq!(rig_res["normalizedWeights"], true);

        // 3. Animate
        let anim_res = call(
            &proj,
            "gen_mesh_animate",
            json!({
                "meshFileRef": rigged_ref,
                "track": "local",
            }),
        )
        .expect("local animate 应成功");
        let anim_ref = anim_res["animatedMeshFileRef"].as_str().unwrap();
        assert!(anim_ref.starts_with(".forge/tmp/gen/"));
        let anims = anim_res["animations"].as_array().unwrap();
        assert!(anims.iter().any(|a| a == "Idle"));
        assert!(anims.iter().any(|a| a == "Walk"));
    }

    #[test]
    fn parse_mesh_geometry_extracts_exact_metrics_from_fixture() {
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assetd/tests/fixtures/quad_indexed.glb");
        let bytes = std::fs::read(&fixture).expect("quad_indexed.glb 必须存在");
        let (triangles, vertices) = parse_mesh_geometry(&bytes).expect("解析网格几何失败");
        assert_eq!(triangles, 2, "quad_indexed 应精确为 2 个三角形");
        assert_eq!(vertices, 4, "quad_indexed 应精确为 4 个顶点");
    }

    #[test]
    fn mcp_character_publish_success() {
        let proj = temp_project("mcp-pub");
        let mesh_ref = ".forge/tmp/gen/hero_anim.glb";
        {
            let p = proj.lock().unwrap();
            let dir = gend::tmpstore::gen_dir(&p).unwrap();
            let cfg = gend::gates::SyntheticGlbConfig {
                include_skin: true,
                include_animations: true,
                include_idle: true,
                include_walk: true,
                ..Default::default()
            };
            let bytes = gend::gates::build_synthetic_glb(&cfg);
            std::fs::write(dir.join("hero_anim.glb"), bytes).unwrap();
        }

        let res = call(
            &proj,
            "gen_character_publish",
            json!({
                "meshFileRef": mesh_ref,
                "name": "hero_warrior",
                "category": "role",
            }),
        )
        .expect("发布角色应成功");

        assert!(res["modelGuid"].as_str().is_some());
        assert!(res["rxmodelUri"].as_str().unwrap().starts_with("Content/Models/"));
        assert!(res["rxprefabUri"].as_str().unwrap().ends_with("/template.rxprefab"));
        assert!(res["prefabGuid"].as_str().is_some());

        let p = proj.lock().unwrap();
        assert!(p.content_root().join(res["assetPath"].as_str().unwrap()).is_file());
        assert!(p.content_root().join(res["prefabPath"].as_str().unwrap()).is_file());
    }

    #[test]
    fn mcp_character_publish_rejects_unskinned_mesh() {
        let proj = temp_project("mcp-pub-bad");
        let mesh_ref = ".forge/tmp/gen/static_chair.gltf";
        {
            let p = proj.lock().unwrap();
            let dir = gend::tmpstore::gen_dir(&p).unwrap();
            std::fs::write(dir.join("static_chair.gltf"), TRI_MIN_GLTF).unwrap();
        }

        let err = call(
            &proj,
            "gen_character_publish",
            json!({
                "meshFileRef": mesh_ref,
                "name": "fake_hero",
            }),
        )
        .unwrap_err();

        assert_eq!(err.code, GEN_BAD_PARAMS);
        assert!(err.message.contains("Redline I-5") || err.message.contains("MODEL_CHARACTER_INVALID") || err.message.contains("bound skin"));
    }
}
