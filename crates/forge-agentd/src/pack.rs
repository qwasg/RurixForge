//! Portable project packaging. The packed root contains the selected runtime, exact
//! project manifest, referenced Content closure, and only the runtime caches needed
//! by that closure. It never embeds a development checkout path in its launcher.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};

use assetd::meta::MetaDoc;
use assetd::project::{ForgeProject, RenderBackendKind, RenderConfig};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRequest {
    /// Scene path, workspace-relative or absolute.
    pub scene_ref: String,
    /// Output directory; a non-empty existing directory is rejected.
    pub out_dir: String,
    /// Optional RPC port for the packaged host (defaults to 17890).
    #[serde(default)]
    pub port: Option<u16>,
}

fn path_error(code: &str, message: impl Into<String>) -> String {
    format!("{code}: {}", message.into())
}

fn safe_content_rel(value: &str) -> bool {
    let path = Path::new(value);
    !path.is_absolute()
        && value
            .replace('\\', "/")
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains(':'))
        && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Recursively gather JSON strings (component props and graph inputs share the same walk).
fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        Value::Object(m) => m.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

/// GUID shape is the 8-4-4-4-12 hexadecimal form written by assetd.
fn looks_like_guid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// GUID -> Content-relative asset path index, reusing assetd's .meta parser.
fn build_guid_index(project_root: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut stack = vec![project_root.join("Content")];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !entry.file_name().to_string_lossy().ends_with(".meta") {
                continue;
            }
            let Ok(meta) = MetaDoc::load(&path) else {
                continue;
            };
            if let Ok(rel) = path.strip_prefix(project_root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(
                    meta.guid,
                    rel.strip_suffix(".meta").unwrap_or(&rel).to_string(),
                );
            }
        }
    }
    out
}

/// Reference-closure BFS: scene -> referenced assets -> nested material/texture/model/graph refs.
/// The returned warnings are preserved for the public report; packaging turns missing/dangling
/// references into errors rather than emitting a knowingly incomplete standalone directory.
pub fn collect_closure(scene_abs: &Path, project_root: &Path) -> (BTreeSet<String>, Vec<String>) {
    let mut closure = BTreeSet::new();
    let mut warnings = Vec::new();
    let guid_index = build_guid_index(project_root);
    let rel_scene = scene_abs
        .strip_prefix(project_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| scene_abs.to_string_lossy().replace('\\', "/"));
    let mut queue = VecDeque::from([rel_scene]);
    while let Some(rel) = queue.pop_front() {
        if closure.contains(&rel) {
            continue;
        }
        if !safe_content_rel(&rel) || !rel.starts_with("Content/") {
            warnings.push(format!("引用路径非法: {rel}"));
            continue;
        }
        let abs = project_root.join(&rel);
        let Ok(bytes) = std::fs::read(&abs) else {
            warnings.push(format!("引用缺失: {rel}({} 不可读)", abs.display()));
            continue;
        };
        closure.insert(rel.clone());
        // Model bundles embed all decoded textures and use local UUIDs for nodes/materials.
        // Those identities are not external asset references (validated below when packing).
        if rel.ends_with(".rxmodel") {
            continue;
        }
        let jsonish = rel.ends_with(".rxscene")
            || rel.ends_with(".rxshadergraph")
            || rel.ends_with(".rxgraph")
            || rel.ends_with(".rxsprite")
            || rel.ends_with(".rxmat")
            || rel.ends_with(".mat")
            || rel.ends_with(".rxprefab")
            || rel.ends_with(".rxmodel")
            || rel.ends_with(".json");
        if !jsonish {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            warnings.push(format!("引用非 UTF-8: {rel}(跳过递归)"));
            continue;
        };
        let Ok(doc) = serde_json::from_str::<Value>(&text) else {
            warnings.push(format!("引用非 JSON: {rel}(跳过递归)"));
            continue;
        };
        let mut found = Vec::new();
        if rel.ends_with(".rxshadergraph") {
            // Graph/node/parameter UUIDs identify local pins, not assets. Only typed texture
            // parameter defaults are external references in the v1 material DAG.
            if let Some(parameters)=doc.get("parameters").and_then(Value::as_array) {
                for p in parameters.iter().filter(|p|p["type"]=="texture2d") {
                    if let Some(reference)=p["default"].as_str().filter(|s|!s.is_empty()){found.push(reference.to_string());}
                }
            }
        } else { collect_strings(&doc, &mut found); }
        for reference in found {
            if reference.starts_with("Content/") {
                if !safe_content_rel(&reference) {
                    warnings.push(format!("引用路径非法: {reference}({rel} 引用)"));
                } else if !closure.contains(&reference) {
                    queue.push_back(reference);
                }
            } else if let Some(target) = guid_index.get(&reference) {
                if !closure.contains(target) {
                    queue.push_back(target.clone());
                }
            } else if looks_like_guid(&reference) {
                warnings.push(format!("guid 悬空: {reference}({rel} 引用,无对应资产)"));
            }
        }
    }
    let metas: Vec<String> = closure
        .iter()
        .map(|rel| format!("{rel}.meta"))
        .filter(|meta| project_root.join(meta).is_file())
        .collect();
    closure.extend(metas);
    (closure, warnings)
}

fn fatal_closure_warning(warnings: &[String]) -> Option<String> {
    warnings
        .iter()
        .find(|w| {
            w.starts_with("引用缺失:")
                || w.starts_with("guid 悬空:")
                || w.starts_with("引用路径非法:")
        })
        .cloned()
}

#[derive(Debug, Clone)]
struct PackArtifact {
    source: PathBuf,
    relative: PathBuf,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("forge-agentd manifest is two parents below workspace root")
        .to_path_buf()
}

fn path_is_under(path: &Path, parent: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let parent = parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf());
    path.starts_with(parent)
}

fn runtime_validation_error(runtime_dir: &Path, error: assetd::AssetError) -> String {
    match error.code {
        "GODOT_RUNTIME_STALE" => path_error("PACK_GODOT_RUNTIME_STALE", error.message),
        _ => path_error(
            "PACK_GODOT_RUNTIME_INVALID",
            format!("{} ({})", error.message, runtime_dir.display()),
        ),
    }
}

fn validate_pack_runtime(runtime_dir: &Path) -> Result<(), String> {
    if !runtime_dir.is_dir() {
        return Err(path_error(
            "PACK_GODOT_RUNTIME_MISSING",
            format!(
                "{}；先运行 scripts\\godot-runtime.ps1 -Build",
                runtime_dir.display()
            ),
        ));
    }
    let manifest = assetd::godot_runtime::validate_runtime(runtime_dir)
        .map_err(|e| runtime_validation_error(runtime_dir, e))?;
    let target = workspace_root().join("target");
    // Only workspace-local runtime directories have a current source build to compare against.
    // A copied/installed runtime outside target is validated solely from its own manifest.
    if path_is_under(runtime_dir, &target) {
        let dll = target.join(&manifest.profile).join("godot_host.dll");
        assetd::godot_runtime::validate_development_runtime(runtime_dir, dll)
            .map_err(|e| runtime_validation_error(runtime_dir, e))?;
    }
    Ok(())
}

fn locate_rurixc(engine_bin: &Path) -> Option<PathBuf> {
    if let Ok(value) = std::env::var("FORGE_RURIXC") {
        if !value.trim().is_empty() {
            return Some(PathBuf::from(value));
        }
    }
    let sibling = engine_bin.parent().map(|p| p.join("rurixc.exe"));
    if let Some(path) = sibling.filter(|p| p.is_file()) {
        return Some(path);
    }
    let workspace = workspace_root().join("target/debug/rurixc.exe");
    workspace.is_file().then_some(workspace)
}

fn cache_key_for_native_rx(source: &[u8], compiler: &[u8]) -> String {
    let mut input = Vec::with_capacity(source.len() + compiler.len() + 1);
    input.extend_from_slice(source);
    input.push(b'|');
    input.extend_from_slice(compiler);
    forge_util::hashutil::sha256_hex(&input)
}

/// Resolve native-script binaries by the same cache key as forge-logic. Rust cdylibs are included
/// only when their source and binary hashes match the adjacent native.json manifest; .rx modules
/// also carry the exact rurixc binary because its bytes participate in the runtime cache key.
fn collect_native_artifacts(
    closure: &BTreeSet<String>,
    project_root: &Path,
    engine_bin: &Path,
) -> Result<Vec<PackArtifact>, String> {
    let cache = project_root.join(".forge/cache/rxdll");
    let mut artifacts = BTreeMap::<String, PackArtifact>::new();
    let mut compiler_copy = false;
    for rel in closure
        .iter()
        .filter(|r| r.ends_with(".rx") || r.ends_with(".rs"))
    {
        let source_path = project_root.join(rel);
        let source = std::fs::read(&source_path)
            .map_err(|e| path_error("PACK_REFERENCE_MISSING", format!("{rel}: {e}")))?;
        let stem = Path::new(rel)
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("module");
        if rel.ends_with(".rs") {
            let source_hash = forge_util::hashutil::sha256_hex(&source);
            let mut found = None;
            let entries = std::fs::read_dir(&cache).map_err(|e| {
                path_error(
                    "PACK_CACHE_MISSING",
                    format!("Rust module {rel} 缺已编译缓存 {}: {e}", cache.display()),
                )
            })?;
            let mut manifests: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().and_then(|x| x.to_str()) == Some("json")
                        && p.to_string_lossy().ends_with(".native.json")
                })
                .collect();
            manifests.sort();
            for manifest_path in manifests {
                let bytes = std::fs::read(&manifest_path)
                    .map_err(|e| path_error("PACK_CACHE_INVALID", e.to_string()))?;
                let Ok(doc) = serde_json::from_slice::<Value>(&bytes) else {
                    continue;
                };
                if doc["backend"].as_str() != Some("rust-cdylib-v1")
                    || doc["module"].as_str() != Some(rel.as_str())
                    || doc["sourceSha256"].as_str() != Some(source_hash.as_str())
                {
                    continue;
                }
                let Some(name) = doc["dll"].as_str() else {
                    continue;
                };
                if Path::new(name).file_name().and_then(|n| n.to_str()) != Some(name)
                    || !name.ends_with(".dll")
                {
                    return Err(path_error(
                        "PACK_CACHE_INVALID",
                        format!("{rel} native.json DLL 路径非法"),
                    ));
                }
                let dll = cache.join(name);
                let dll_bytes = std::fs::read(&dll).map_err(|e| {
                    path_error("PACK_CACHE_INVALID", format!("{}: {e}", dll.display()))
                })?;
                let actual = forge_util::hashutil::sha256_hex(&dll_bytes);
                if doc["dllSha256"].as_str() != Some(actual.as_str()) {
                    return Err(path_error(
                        "PACK_CACHE_INVALID",
                        format!("{rel} 预编译 DLL 哈希不符: {name}"),
                    ));
                }
                let dll_rel = PathBuf::from(".forge/cache/rxdll").join(name);
                artifacts.insert(
                    dll_rel.to_string_lossy().replace('\\', "/"),
                    PackArtifact {
                        source: dll,
                        relative: dll_rel,
                    },
                );
                let manifest_rel =
                    PathBuf::from(".forge/cache/rxdll").join(manifest_path.file_name().unwrap());
                artifacts.insert(
                    manifest_rel.to_string_lossy().replace('\\', "/"),
                    PackArtifact {
                        source: manifest_path,
                        relative: manifest_rel,
                    },
                );
                found = Some(());
                break;
            }
            if found.is_none() {
                return Err(path_error("PACK_CACHE_MISSING", format!("Rust 模块 {rel} 缺与当前源码匹配且校验通过的预编译 DLL/native.json；请先在开发机执行一次 play/build 生成缓存")));
            }
        } else {
            let compiler = locate_rurixc(engine_bin).ok_or_else(|| {
                path_error("PACK_NATIVE_RUNTIME_MISSING", format!("模块 {rel} 需要 rurixc 版本字节才能验证其缓存键；设置 FORGE_RURIXC 或提供与 engine-host.exe 同目录的 rurixc.exe"))
            })?;
            let compiler_bytes = std::fs::read(&compiler).map_err(|e| {
                path_error(
                    "PACK_NATIVE_RUNTIME_MISSING",
                    format!("rurixc 不可读 {}: {e}", compiler.display()),
                )
            })?;
            let key = cache_key_for_native_rx(&source, &compiler_bytes);
            let dll_name = format!("{stem}-{}.dll", &key[..16]);
            let dll = cache.join(&dll_name);
            if !dll.is_file() {
                return Err(path_error("PACK_CACHE_MISSING", format!(".rx 模块 {rel} 缺精确缓存 {}；请先在有 rurixc 的开发机运行一次对应场景，再重新打包", dll.display())));
            }
            let relative = PathBuf::from(".forge/cache/rxdll").join(&dll_name);
            artifacts.insert(
                relative.to_string_lossy().replace('\\', "/"),
                PackArtifact {
                    source: dll,
                    relative,
                },
            );
            if !compiler_copy {
                let relative = PathBuf::from("bin/rurixc.exe");
                artifacts.insert(
                    relative.to_string_lossy().replace('\\', "/"),
                    PackArtifact {
                        source: compiler,
                        relative,
                    },
                );
                compiler_copy = true;
            }
        }
    }
    Ok(artifacts.into_values().collect())
}

/// Resolve referenced mesh, model-revision, and image dependencies into a portable cache closure.
fn collect_asset_cache_artifacts(
    closure: &BTreeSet<String>,
    project_root: &Path,
) -> Result<Vec<PackArtifact>, String> {
    let project = ForgeProject::load(project_root)
        .map_err(|e| path_error("PACK_PROJECT_CONFIG_INVALID", e.to_string()))?;
    if project.content_dir.replace('\\', "/") != "Content" {
        return Err(path_error(
            "PACK_PROJECT_LAYOUT_UNSUPPORTED",
            "便携打包要求 forge.toml [dirs].content = \"Content\"",
        ));
    }
    let cache = project_root.join(".forge/cache");
    let guid_index = build_guid_index(project_root);
    let mut artifacts = BTreeMap::<String, PackArtifact>::new();

    for rel in closure {
        let source = project_root.join(rel);
        let extension = Path::new(rel)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension == "rxmodel" {
            let bundle: assetd::model::ModelBundle = serde_json::from_slice(
                &std::fs::read(&source).map_err(|e| path_error("PACK_ASSET_INVALID", format!("{rel}: {e}")))?
            ).map_err(|e| path_error("PACK_ASSET_INVALID", format!("模型 {rel} JSON: {e}")))?;
            assetd::model::validate_model_bundle(&bundle)
                .map_err(|e| path_error("PACK_ASSET_INVALID", format!("模型 {rel}: {e}")))?;
        }
        if matches!(extension.as_str(), "png" | "jpg" | "jpeg") {
            assetd::texture::decode_size(&source).map_err(|e| {
                path_error("PACK_ASSET_INVALID", format!("贴图 {rel} 无法解码: {e}"))
            })?;
        }
        if !matches!(extension.as_str(), "gltf" | "glb") {
            continue;
        }
        let meta_path = PathBuf::from(format!("{}.meta", source.to_string_lossy()));
        let meta = MetaDoc::load(&meta_path).map_err(|e| {
            path_error(
                "PACK_ASSET_INVALID",
                format!("网格资产 {rel} 缺少有效 .meta: {e}"),
            )
        })?;
        let artifact = assetd::build::build_mesh(&source, &meta, &cache).map_err(|e| {
            path_error(
                "PACK_CACHE_INVALID",
                format!("网格缓存构建/校验失败 {rel}: {e}"),
            )
        })?;
        let relative = PathBuf::from(".forge/cache").join(artifact.rel_path);
        let key = relative.to_string_lossy().replace('\\', "/");
        artifacts.insert(
            key,
            PackArtifact {
                source: cache.join(&relative.strip_prefix(".forge/cache").unwrap()),
                relative,
            },
        );
    }

    // Find explicit historical model revisions in scene/prefab JSON. The current model asset is
    // already in Content closure; historical snapshots live under .forge/cache/models/<guid>/.
    let mut requested = BTreeSet::<(String, u64)>::new();
    for rel in closure
        .iter()
        .filter(|r| r.ends_with(".rxscene") || r.ends_with(".rxprefab"))
    {
        let text = std::fs::read_to_string(project_root.join(rel))
            .map_err(|e| path_error("PACK_ASSET_INVALID", format!("{rel}: {e}")))?;
        let doc: Value = serde_json::from_str(&text)
            .map_err(|e| path_error("PACK_ASSET_INVALID", format!("{rel} JSON: {e}")))?;
        fn scan_model_revisions(v: &Value, out: &mut Vec<(String, u64)>) {
            match v {
                Value::Object(object) => {
                    if let (Some(model), Some(revision)) = (
                        object.get("model").and_then(Value::as_str),
                        object.get("revision").and_then(Value::as_u64),
                    ) {
                        if revision > 0 && !model.is_empty() {
                            out.push((model.to_string(), revision));
                        }
                    }
                    object
                        .values()
                        .for_each(|child| scan_model_revisions(child, out));
                }
                Value::Array(array) => array
                    .iter()
                    .for_each(|child| scan_model_revisions(child, out)),
                _ => {}
            }
        }
        let mut refs = Vec::new();
        scan_model_revisions(&doc, &mut refs);
        for (reference, revision) in refs {
            let model_rel = guid_index
                .get(&reference)
                .cloned()
                .or_else(|| {
                    let rel = reference.strip_prefix("Content/").unwrap_or(&reference);
                    let safe = format!("Content/{rel}");
                    safe_content_rel(&safe).then_some(safe)
                })
                .ok_or_else(|| {
                    path_error(
                        "PACK_REFERENCE_MISSING",
                        format!("模型引用无法定位: {reference}"),
                    )
                })?;
            let bytes = std::fs::read(project_root.join(model_rel)).map_err(|e| {
                path_error("PACK_REFERENCE_MISSING", format!("模型 {reference}: {e}"))
            })?;
            let bundle: assetd::model::ModelBundle =
                serde_json::from_slice(&bytes).map_err(|e| {
                    path_error("PACK_ASSET_INVALID", format!("模型 {reference} JSON: {e}"))
                })?;
            if bundle.revision != revision {
                requested.insert((bundle.guid.clone(), revision));
            }
        }
    }
    for (guid, revision) in requested {
        let relative = PathBuf::from(format!(".forge/cache/models/{guid}/{revision}.rxmodel"));
        let source = project_root.join(&relative);
        let bytes = std::fs::read(&source).map_err(|e| {
            path_error(
                "PACK_CACHE_MISSING",
                format!(
                    "模型 {guid} 的历史版本 {revision} 缺失: {}: {e}",
                    source.display()
                ),
            )
        })?;
        let bundle: assetd::model::ModelBundle = serde_json::from_slice(&bytes).map_err(|e| {
            path_error(
                "PACK_CACHE_INVALID",
                format!("模型历史版本 {} 无效: {e}", source.display()),
            )
        })?;
        assetd::model::validate_model_bundle(&bundle).map_err(|e| {
            path_error(
                "PACK_CACHE_INVALID",
                format!("模型历史版本 {} 校验失败: {e}", source.display()),
            )
        })?;
        if bundle.guid != guid || bundle.revision != revision {
            return Err(path_error(
                "PACK_CACHE_INVALID",
                format!("模型历史版本身份与路径不符: {}", source.display()),
            ));
        }
        let key = relative.to_string_lossy().replace('\\', "/");
        artifacts.insert(key, PackArtifact { source, relative });
    }
    Ok(artifacts.into_values().collect())
}

fn render_strs(
    config: &RenderConfig,
) -> Result<(&'static str, Option<&'static str>, Option<&'static str>), String> {
    let (backend, method, driver) = config.as_strs();
    let (checked, _) = RenderConfig::from_parts(Some(backend), method, driver)
        .map_err(|e| path_error("PACK_RENDER_CONFIG_INVALID", e.to_string()))?;
    Ok(checked.as_strs())
}

fn ps_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Launcher sets every core env value explicitly and only refers to files inside the package root.
fn run_script(
    scene_rel: &str,
    port: u16,
    render: &RenderConfig,
    has_rurix: bool,
) -> Result<String, String> {
    let (backend, method, driver) = render_strs(render)?;
    let method = method.unwrap_or("");
    let driver = driver.unwrap_or("");
    let source = if render.backend == RenderBackendKind::Rurix && render == &RenderConfig::default()
    {
        "default"
    } else {
        "forge.toml"
    };
    let mut script = format!(
        "$ErrorActionPreference = 'Stop'\r\n$root = Split-Path -Parent $MyInvocation.MyCommand.Path\r\n$env:FORGE_PROJECT_ROOT = $root\r\n$env:FORGE_HOST_PORT = {port}\r\n$env:FORGE_GAME_SCENE = {scene}\r\n$env:FORGE_RENDER_BACKEND = {backend}\r\n$env:FORGE_RENDER_METHOD = {method}\r\n$env:FORGE_RENDER_DRIVER = {driver}\r\n$env:FORGE_RENDER_SOURCE = {source}\r\n",
        port = port,
        scene = ps_literal(scene_rel),
        backend = ps_literal(backend),
        method = ps_literal(method),
        driver = ps_literal(driver),
        source = ps_literal(source),
    );
    if has_rurix {
        script.push_str("$env:FORGE_RURIXC = Join-Path $root 'bin\\rurixc.exe'\r\n");
    }
    if render.backend == RenderBackendKind::Godot {
        let method = render
            .method
            .map(|m| match m {
                assetd::project::RenderMethod::ForwardPlus => "forward_plus",
                assetd::project::RenderMethod::Mobile => "mobile",
                assetd::project::RenderMethod::GlCompatibility => "gl_compatibility",
            })
            .unwrap_or("forward_plus");
        let driver = render
            .driver
            .map(|d| match d {
                assetd::project::RenderDriver::D3d12 => "d3d12",
                assetd::project::RenderDriver::Vulkan => "vulkan",
                assetd::project::RenderDriver::Opengl3 => "opengl3",
            })
            .unwrap_or("d3d12");
        script.push_str(&format!(
            "$runtime = Join-Path $root 'runtime'\r\n$env:FORGE_GODOT_RUNTIME_DIR = $runtime\r\nSet-Location $runtime\r\n& (Join-Path $runtime 'forge-godot_console.exe') --rendering-method {method} --rendering-driver {driver} --audio-driver Dummy\r\nexit $LASTEXITCODE\r\n"
        ));
    } else {
        script.push_str("Set-Location $root\r\n");
        script.push_str(&format!(
            "& \"$root\\bin\\engine-host.exe\" --port {port} --game \"{scene_cli}\"\r\nexit $LASTEXITCODE\r\n",
            scene_cli = scene_rel.replace('"', "`\"")
        ));
    }
    Ok(script)
}

fn validate_project_layout(project: &ForgeProject) -> Result<(), String> {
    if project.content_dir.replace('\\', "/") != "Content" {
        return Err(path_error(
            "PACK_PROJECT_LAYOUT_UNSUPPORTED",
            "便携打包要求 forge.toml [dirs].content = \"Content\"",
        ));
    }
    Ok(())
}

fn copy_runtime_tree(
    source: &Path,
    destination: &Path,
    copy_in: &mut dyn FnMut(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let mut stack = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((src_dir, dst_dir)) = stack.pop() {
        for entry in std::fs::read_dir(&src_dir).map_err(|e| {
            path_error(
                "PACK_GODOT_RUNTIME_INVALID",
                format!("{}: {e}", src_dir.display()),
            )
        })? {
            let entry =
                entry.map_err(|e| path_error("PACK_GODOT_RUNTIME_INVALID", e.to_string()))?;
            let src = entry.path();
            let dst = dst_dir.join(entry.file_name());
            let metadata = std::fs::symlink_metadata(&src)
                .map_err(|e| path_error("PACK_GODOT_RUNTIME_INVALID", e.to_string()))?;
            if metadata.file_type().is_symlink() {
                return Err(path_error(
                    "PACK_GODOT_RUNTIME_INVALID",
                    format!("运行时不允许符号链接: {}", src.display()),
                ));
            }
            if metadata.is_dir() {
                std::fs::create_dir_all(&dst)
                    .map_err(|e| path_error("PACK_COPY_FAILED", e.to_string()))?;
                stack.push((src, dst));
            } else if metadata.is_file() {
                copy_in(&src, &dst)?;
            } else {
                return Err(path_error(
                    "PACK_GODOT_RUNTIME_INVALID",
                    format!("运行时包含非普通文件: {}", src.display()),
                ));
            }
        }
    }
    Ok(())
}

fn configure_packed_godot_runtime(runtime_dir: &Path, render: &RenderConfig) -> Result<(), String> {
    let (_, method, driver) = render_strs(render)?;
    let method =
        method.ok_or_else(|| path_error("PACK_RENDER_CONFIG_INVALID", "Godot 缺 method"))?;
    let driver =
        driver.ok_or_else(|| path_error("PACK_RENDER_CONFIG_INVALID", "Godot 缺 driver"))?;
    let project_path = runtime_dir.join("project.godot");
    let text = std::fs::read_to_string(&project_path).map_err(|e| {
        path_error(
            "PACK_GODOT_RUNTIME_INVALID",
            format!("读取 {} 失败: {e}", project_path.display()),
        )
    })?;
    let mut method_seen = false;
    let mut driver_seen = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("renderer/rendering_method=") {
            lines.push(format!("renderer/rendering_method=\"{method}\""));
            method_seen = true;
        } else if trimmed.starts_with("rendering_device/driver.windows=") {
            lines.push(format!("rendering_device/driver.windows=\"{driver}\""));
            driver_seen = true;
        } else if trimmed.starts_with("gl_compatibility/driver.windows=") {
            lines.push("gl_compatibility/driver.windows=\"opengl3\"".to_string());
        } else {
            lines.push(line.to_string());
        }
    }
    // Fixtures and older generated runtimes may use the canonical Godot section keys
    // with spaces around `=`. Match either spelling before rejecting a malformed runtime.
    if !method_seen || !driver_seen {
        return Err(path_error(
            "PACK_GODOT_RUNTIME_INVALID",
            format!(
                "{} 缺 rendering_method 或 driver.windows 设置",
                project_path.display()
            ),
        ));
    }
    let project_bytes = format!("{}\n", lines.join("\n"));
    std::fs::write(&project_path, project_bytes.as_bytes()).map_err(|e| {
        path_error(
            "PACK_COPY_FAILED",
            format!("写 {} 失败: {e}", project_path.display()),
        )
    })?;

    let manifest_path = runtime_dir.join(assetd::godot_runtime::MANIFEST_NAME);
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|e| {
            path_error(
                "PACK_GODOT_RUNTIME_INVALID",
                format!("读取 {} 失败: {e}", manifest_path.display()),
            )
        })?)
        .map_err(|e| {
            path_error(
                "PACK_GODOT_RUNTIME_INVALID",
                format!("运行时清单 JSON 无效: {e}"),
            )
        })?;
    manifest["defaults"]["method"] = json!(method);
    manifest["defaults"]["driver"] = json!(driver);
    let project_hash = forge_util::hashutil::sha256_hex(project_bytes.as_bytes());
    manifest["sha256"]["project.godot"] = json!(project_hash);
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| {
        path_error(
            "PACK_GODOT_RUNTIME_INVALID",
            format!("序列化运行时清单失败: {e}"),
        )
    })?;
    std::fs::write(manifest_path, manifest_bytes)
        .map_err(|e| path_error("PACK_COPY_FAILED", format!("更新运行时清单失败: {e}")))?;
    Ok(())
}

/// Config-aware packaging entry point. `build_pack` below remains source-compatible and resolves
/// RenderConfig from the project's forge.toml itself.
pub fn build_pack_with_runtime(
    scene_abs: &Path,
    project_root: &Path,
    out_dir: &Path,
    engine_bin: &Path,
    godot_runtime_dir: &Path,
    port: u16,
) -> Result<Value, String> {
    build_pack_impl(
        scene_abs,
        project_root,
        out_dir,
        engine_bin,
        godot_runtime_dir,
        port,
    )
}

/// Backward-compatible original signature; runtime selection now follows the project's RenderConfig.
pub fn build_pack(
    scene_abs: &Path,
    project_root: &Path,
    out_dir: &Path,
    engine_bin: &Path,
    port: u16,
) -> Result<Value, String> {
    let runtime = std::env::var("FORGE_GODOT_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root().join("target/godot-runtime"));
    build_pack_impl(scene_abs, project_root, out_dir, engine_bin, &runtime, port)
}

fn build_pack_impl(
    scene_abs: &Path,
    project_root: &Path,
    out_dir: &Path,
    engine_bin: &Path,
    godot_runtime_dir: &Path,
    port: u16,
) -> Result<Value, String> {
    if port == 0 {
        return Err(path_error("PACK_PORT_INVALID", "port 必须为 1..=65535"));
    }
    if !scene_abs.is_file() {
        return Err(path_error(
            "PACK_SCENE_NOT_FOUND",
            scene_abs.display().to_string(),
        ));
    }
    let project = ForgeProject::load(project_root).map_err(|e| {
        if e.message.contains("forge.toml [render]") {
            path_error(
                "PACK_RENDER_CONFIG_INVALID",
                format!("{}: {}", e.code, e.message),
            )
        } else {
            path_error(
                "PACK_PROJECT_CONFIG_INVALID",
                format!("{}: {}", e.code, e.message),
            )
        }
    })?;
    validate_project_layout(&project)?;
    let (backend, _, _) = render_strs(&project.render)?;
    let godot = backend == "godot";
    if !godot && !engine_bin.is_file() {
        return Err(path_error(
            "PACK_ENGINE_MISSING",
            engine_bin.display().to_string(),
        ));
    }
    if godot {
        validate_pack_runtime(godot_runtime_dir)?;
    }
    if out_dir.exists()
        && std::fs::read_dir(out_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
    {
        return Err(path_error(
            "PACK_OUTDIR_CONFLICT",
            format!("{} 已存在且非空", out_dir.display()),
        ));
    }
    let scene_rel = scene_abs
        .strip_prefix(project_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_err(|e| path_error("PACK_SCENE_OUTSIDE_CONTENT", e.to_string()))?;
    if !scene_rel.starts_with("Content/") || !safe_content_rel(&scene_rel) {
        return Err(path_error("PACK_SCENE_OUTSIDE_CONTENT", scene_rel));
    }
    let (closure, warnings) = collect_closure(scene_abs, project_root);
    if let Some(warning) = fatal_closure_warning(&warnings) {
        return Err(path_error("PACK_REFERENCE_MISSING", warning));
    }
    let asset_artifacts = collect_asset_cache_artifacts(&closure, project_root)?;
    // Logic/native modules are backend-neutral and are required by Godot packages too.
    let native_artifacts = collect_native_artifacts(&closure, project_root, engine_bin)?;

    std::fs::create_dir_all(out_dir).map_err(|e| {
        path_error(
            "PACK_COPY_FAILED",
            format!("建输出目录 {}: {e}", out_dir.display()),
        )
    })?;
    let mut files: Vec<Value> = Vec::new();
    let mut total: u64 = 0;
    let mut copy_in = |src: &Path, dst: &Path| -> Result<(), String> {
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                path_error(
                    "PACK_COPY_FAILED",
                    format!("建目录 {}: {e}", parent.display()),
                )
            })?;
        }
        std::fs::copy(src, dst).map_err(|e| {
            path_error(
                "PACK_COPY_FAILED",
                format!("拷贝失败 {} → {}: {e}", src.display(), dst.display()),
            )
        })?;
        let bytes = std::fs::metadata(dst).map(|m| m.len()).unwrap_or(0);
        total += bytes;
        files.push(json!({"path": dst.strip_prefix(out_dir).unwrap_or(dst).to_string_lossy().replace('\\', "/"), "bytes": bytes}));
        Ok(())
    };

    for rel in &closure {
        copy_in(&project_root.join(rel), &out_dir.join(rel))?;
    }
    let forge_manifest = project_root.join("forge.toml");
    if forge_manifest.is_file() {
        copy_in(&forge_manifest, &out_dir.join("forge.toml"))?;
    } else {
        let staging = out_dir.join(".forge.toml.pack-staging");
        std::fs::write(&staging, project.to_toml())
            .map_err(|e| path_error("PACK_COPY_FAILED", format!("写 forge.toml: {e}")))?;
        copy_in(&staging, &out_dir.join("forge.toml"))?;
        let _ = std::fs::remove_file(staging);
    }
    for artifact in asset_artifacts.iter().chain(native_artifacts.iter()) {
        copy_in(&artifact.source, &out_dir.join(&artifact.relative))?;
    }
    if godot {
        copy_runtime_tree(godot_runtime_dir, &out_dir.join("runtime"), &mut copy_in)?;
        configure_packed_godot_runtime(&out_dir.join("runtime"), &project.render)?;
    } else {
        copy_in(engine_bin, &out_dir.join("bin/engine-host.exe"))?;
    }
    let has_rurix = native_artifacts
        .iter()
        .any(|a| a.relative == Path::new("bin/rurixc.exe"));
    let script = run_script(&scene_rel, port, &project.render, has_rurix)?;
    std::fs::write(out_dir.join("pack-run.ps1"), &script)
        .map_err(|e| path_error("PACK_COPY_FAILED", format!("写 pack-run.ps1: {e}")))?;
    total += script.len() as u64;
    files.push(json!({"path":"pack-run.ps1", "bytes":script.len()}));

    Ok(json!({
        "outDir": out_dir.to_string_lossy().replace('\\', "/"),
        "scene": scene_rel,
        "backend": backend,
        "method": project.render.as_strs().1,
        "driver": project.render.as_strs().2,
        "runtimeDir": if godot { Some("runtime") } else { None },
        "closure": closure.iter().collect::<Vec<_>>(),
        "files": files,
        "totalBytes": total,
        "debugEngine": !godot,
        "warnings": warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn fixture_project(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("forge_pack_test_{}_{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Content/Scenes")).unwrap();
        std::fs::create_dir_all(dir.join("Content/Graphs")).unwrap();
        std::fs::create_dir_all(dir.join("Content/Scripts")).unwrap();
        std::fs::write(dir.join("forge.toml"), "[project]\nname = \"portable-test\"\n\n[dirs]\ncontent = \"Content\"\nscripts = \"Content/Scripts\"\n").unwrap();
        std::fs::write(
            dir.join("Content/Scenes/s.rxscene"),
            r#"{ "name": "s", "next_id": 2, "entities": [ { "id": 1, "name": "e", "transform": { "translation": [0,0,0], "rotation": [0,0,0,1], "scale": [1,1,1] }, "components": [ { "type": "Script", "enabled": true, "props": { "module": "", "graphRef": "Content/Graphs/g.rxgraph", "props": {} } } ] } ] }"#,
        ).unwrap();
        std::fs::write(
            dir.join("Content/Scenes/s.rxscene.meta"),
            "guid: 11111111-1111-4111-8111-111111111111\ntype: scene\nimporter: scene\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("Content/Graphs/g.rxgraph"),
            r#"{ "version": 1, "id": "g", "name": "g", "nodes": [ { "id": "c", "type": "call.call_function", "inputs": { "module": { "const": "Content/Scripts/m.rx" }, "fn": { "const": "f" }, "args": { "const": [] } } } ], "edges": [] }"#,
        ).unwrap();
        std::fs::write(
            dir.join("Content/Graphs/g.rxgraph.meta"),
            "guid: 22222222-2222-4222-8222-222222222222\ntype: script\nimporter: rxgraph\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("Content/Scripts/m.rx"),
            "#[export(c)]\npub fn f() -> i32 { 1 }\n",
        )
        .unwrap();
        // Closed-over cache fixture for the interpreter's source+compiler content key.
        std::fs::create_dir_all(dir.join(".forge/cache/rxdll")).unwrap();
        std::fs::write(dir.join("rurixc.exe"), b"fixture-rurixc").unwrap();
        let source = std::fs::read(dir.join("Content/Scripts/m.rx")).unwrap();
        let compiler = std::fs::read(dir.join("rurixc.exe")).unwrap();
        let key = cache_key_for_native_rx(&source, &compiler);
        std::fs::write(
            dir.join(format!(".forge/cache/rxdll/m-{}.dll", &key[..16])),
            b"fixture dll",
        )
        .unwrap();
        std::fs::write(dir.join("Content/Graphs/other.rxgraph"), "{}").unwrap();
        dir
    }

    #[test]
    fn closure_bfs_scene_graph_script_and_exclusion() {
        let root = fixture_project("bfs");
        let (closure, warnings) = collect_closure(&root.join("Content/Scenes/s.rxscene"), &root);
        assert!(closure.contains("Content/Scenes/s.rxscene"));
        assert!(closure.contains("Content/Scenes/s.rxscene.meta"));
        assert!(closure.contains("Content/Graphs/g.rxgraph"));
        assert!(closure.contains("Content/Scripts/m.rx"));
        assert!(!closure.contains("Content/Graphs/other.rxgraph"));
        assert!(warnings.is_empty(), "{warnings:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closure_keeps_graph_material_textures_and_ignores_local_shader_ids(){
        let root=fixture_project("shader-closure");
        let material="aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";let shader="bbbbbbbb-1111-4111-8111-bbbbbbbbbbbb";let texture="cccccccc-1111-4111-8111-cccccccccccc";
        std::fs::write(root.join("Content/Scenes/s.rxscene"),json!({"material":material}).to_string()).unwrap();
        std::fs::write(root.join("Content/surface.rxmat"),json!({"version":2,"shaderGraph":shader,"params":{},"textures":{}}).to_string()).unwrap();
        std::fs::write(root.join("Content/surface.rxmat.meta"),format!("guid: {material}\ntype: material\nimporter: material\n")).unwrap();
        std::fs::write(root.join("Content/surface.rxshadergraph"),json!({"version":1,"id":"dddddddd-1111-4111-8111-dddddddddddd","nodes":[{"id":"eeeeeeee-1111-4111-8111-eeeeeeeeeeee"}],"parameters":[{"id":"map","type":"texture2d","default":texture}]}).to_string()).unwrap();
        std::fs::write(root.join("Content/surface.rxshadergraph.meta"),format!("guid: {shader}\ntype: shadergraph\nimporter: shader-graph\n")).unwrap();
        add_png(&root.join("Content/color.png"));std::fs::write(root.join("Content/color.png.meta"),format!("guid: {texture}\ntype: texture\nimporter: texture\n")).unwrap();
        let(closure,warnings)=collect_closure(&root.join("Content/Scenes/s.rxscene"),&root);assert!(warnings.is_empty(),"{warnings:?}");for path in["Content/surface.rxmat","Content/surface.rxshadergraph","Content/color.png"]{assert!(closure.contains(path),"{path}");assert!(closure.contains(&format!("{path}.meta")));}
        let _=std::fs::remove_dir_all(root);
    }

    #[test]
    fn closure_reports_dangling_and_missing_references_for_the_compatibility_api() {
        let root = fixture_project("missing");
        std::fs::remove_file(root.join("Content/Scripts/m.rx")).unwrap();
        let (closure, warnings) = collect_closure(&root.join("Content/Scenes/s.rxscene"), &root);
        assert!(warnings.iter().any(|w| w.contains("m.rx")), "{warnings:?}");
        assert!(!closure.contains("Content/Scripts/m.rx"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closure_distinguishes_embedded_model_ids_and_follows_mat_textures() {
        let root = fixture_project("model-identities");
        std::fs::write(root.join("Content/Scenes/s.rxscene"), r#"{"model":"Content/model.rxmodel","material":"Content/surface.mat"}"#).unwrap();
        std::fs::write(root.join("Content/model.rxmodel"), r#"{"materials":[{"guid":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"}],"textures":[]}"#).unwrap();
        std::fs::write(root.join("Content/surface.mat"), r#"{"textures":{"albedo":"Content/color.png"}}"#).unwrap();
        add_png(&root.join("Content/color.png"));
        let (closure, warnings) = collect_closure(&root.join("Content/Scenes/s.rxscene"), &root);
        assert!(warnings.is_empty(), "embedded model IDs are not dangling asset GUIDs: {warnings:?}");
        assert!(closure.contains("Content/color.png"), "legacy .mat must retain its texture");
        let _ = std::fs::remove_dir_all(root);
    }

    fn runtime_fixture(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("forge_pack_runtime_{}_{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&root);
        for rel in assetd::godot_runtime::REQUIRED_FILES {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let bytes = if *rel == "project.godot" {
                "[rendering]\nrenderer/rendering_method=\"forward_plus\"\nrendering_device/driver.windows=\"d3d12\"\ngl_compatibility/driver.windows=\"opengl3\"\n".to_string()
            } else {
                format!("fixture runtime {rel}")
            };
            std::fs::write(&path, bytes).unwrap();
        }
        let hashes = assetd::godot_runtime::REQUIRED_FILES
            .iter()
            .map(|rel| {
                (
                    (*rel).to_string(),
                    forge_util::hashutil::sha256_file(&root.join(rel)).unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let doc = json!({
            "schema":"forge.godot_runtime.v1", "godot":"4.7.2.stable.official", "template":"release", "profile":"debug",
            "defaults":{"method":"forward_plus","driver":"d3d12","maxFps":60}, "sha256":hashes
        });
        std::fs::write(
            root.join("runtime-manifest.json"),
            serde_json::to_vec(&doc).unwrap(),
        )
        .unwrap();
        root
    }

    fn add_png(path: &Path) {
        use image::ImageEncoder;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let image = image::RgbaImage::from_raw(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ],
        )
        .unwrap();
        let mut bytes = Cursor::new(Vec::new());
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), 2, 2, image::ExtendedColorType::Rgba8)
            .unwrap();
        std::fs::write(path, bytes.into_inner()).unwrap();
    }

    #[test]
    fn portable_pack_contains_usable_mesh_model_revision_and_texture_caches() {
        let root = fixture_project("asset-cache-closure");
        let content = root.join("Content");
        std::fs::create_dir_all(content.join("Meshes")).unwrap();
        std::fs::create_dir_all(content.join("Models")).unwrap();
        std::fs::create_dir_all(content.join("Materials")).unwrap();
        std::fs::create_dir_all(content.join("Textures")).unwrap();
        let gltf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("crates/assetd/tests/fixtures/tri_min.gltf");
        std::fs::copy(&gltf, content.join("Meshes/tri.gltf")).unwrap();
        let mesh_meta = MetaDoc::new(
            "Meshes/tri.gltf",
            "33333333-3333-4333-8333-333333333333".into(),
        )
        .unwrap();
        mesh_meta
            .save(&content.join("Meshes/tri.gltf.meta"))
            .unwrap();
        let texture = content.join("Textures/checker.png");
        add_png(&texture);
        let texture_meta = MetaDoc::new(
            "Textures/checker.png",
            "44444444-4444-4444-8444-444444444444".into(),
        )
        .unwrap();
        texture_meta
            .save(&content.join("Textures/checker.png.meta"))
            .unwrap();
        let material_rel = "Materials/checker.rxmat";
        std::fs::write(content.join(material_rel), r#"{"version":1,"shader":"pbr-default","params":{},"textures":{"albedo":"44444444-4444-4444-8444-444444444444"}}"#).unwrap();
        let material_meta =
            MetaDoc::new(material_rel, "55555555-5555-4555-8555-555555555555".into()).unwrap();
        material_meta
            .save(&content.join(format!("{material_rel}.meta")))
            .unwrap();
        let manifest = assetd::model::ModelManifest {
            version: 1,
            source_id: "pack-history-model".into(),
            name: "pack model".into(),
            kind: "prop".into(),
            revision: 1,
            source_blend: None,
            object_ids: BTreeMap::new(),
            idle_clip: None,
            walk_clip: None,
        };
        let mut old = assetd::model::inspect_model_source(&gltf, &manifest).unwrap();
        let model_guid = old.guid.clone();
        let old_bytes = serde_json::to_vec(&old).unwrap();
        let model_rel = "Models/tri.rxmodel";
        old.revision = 2;
        std::fs::write(content.join(model_rel), serde_json::to_vec(&old).unwrap()).unwrap();
        let model_meta = MetaDoc::new(model_rel, model_guid.clone()).unwrap();
        model_meta
            .save(&content.join(format!("{model_rel}.meta")))
            .unwrap();
        let history_dir = root.join(format!(".forge/cache/models/{model_guid}"));
        std::fs::create_dir_all(&history_dir).unwrap();
        std::fs::write(history_dir.join("1.rxmodel"), &old_bytes).unwrap();
        std::fs::write(content.join("Scenes/s.rxscene"), format!(r#"{{"name":"s","next_id":3,"entities":[
            {{"id":1,"name":"mesh","transform":{{"translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]}},"components":[{{"type":"MeshRenderer","enabled":true,"props":{{"mesh":"33333333-3333-4333-8333-333333333333","material":"55555555-5555-4555-8555-555555555555"}}}}]}},
            {{"id":2,"name":"model","transform":{{"translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]}},"components":[{{"type":"ModelRenderer","enabled":true,"props":{{"model":"{model_guid}","revision":1}}}}]}}
        ]}}"#)).unwrap();
        let out = std::env::temp_dir().join(format!("forge_pack_asset_out_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let engine = root.join("engine-host.exe");
        std::fs::write(&engine, b"MZ fixture").unwrap();
        let report = build_pack(
            &content.join("Scenes/s.rxscene"),
            &root,
            &out,
            &engine,
            17890,
        )
        .unwrap();
        let files = report["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["path"].as_str())
            .collect::<Vec<_>>();
        assert!(
            files.iter().any(|p| p.starts_with(".forge/cache/rxmesh/")),
            "{files:?}"
        );
        assert!(
            files
                .iter()
                .any(|p| *p == format!(".forge/cache/models/{model_guid}/1.rxmodel")),
            "{files:?}"
        );
        assert!(out.join("Content/Textures/checker.png").is_file());
        assetd::texture::decode_rgba(&out.join("Content/Textures/checker.png")).unwrap();
        let packed_project = ForgeProject::load(&out).unwrap();
        let loaded = assetd::model::load_model_revision(&packed_project, &model_guid, 1).unwrap();
        assert_eq!(
            loaded.revision, 1,
            "historical model is directly loadable from the portable cache closure"
        );
        let mesh_files = files
            .iter()
            .filter(|p| p.starts_with(".forge/cache/rxmesh/"))
            .collect::<Vec<_>>();
        assert_eq!(mesh_files.len(), 1);
        let artifact = assetd::build::build_mesh(
            &out.join("Content/Meshes/tri.gltf"),
            &MetaDoc::load(&out.join("Content/Meshes/tri.gltf.meta")).unwrap(),
            &out.join(".forge/cache"),
        )
        .unwrap();
        assert_eq!(
            artifact.triangle_count, 1,
            "copied RXGB cache must be parseable by the runtime builder"
        );
        assert!(report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|w| !w.as_str().unwrap().contains("缓存缺失")));
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn package_uses_selected_godot_runtime_and_is_root_relative() {
        let root = fixture_project("godot-pack");
        std::fs::write(root.join("forge.toml"), "[project]\nname = \"portable-godot\"\n\n[dirs]\ncontent = \"Content\"\nscripts = \"Content/Scripts\"\n\n[render]\nbackend = \"godot\"\nmethod = \"mobile\"\n").unwrap();
        let runtime = runtime_fixture("valid");
        let out = std::env::temp_dir().join(format!("forge_pack_godot_out_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let report = build_pack_with_runtime(
            &root.join("Content/Scenes/s.rxscene"),
            &root,
            &out,
            &root.join("engine-host.exe"), // sibling fixture rurixc supplies the native cache key
            &runtime,
            19001,
        )
        .unwrap();
        assert_eq!(report["backend"], "godot");
        assert_eq!(report["method"], "mobile");
        assert_eq!(report["driver"], "d3d12");
        assert!(out.join("bin/rurixc.exe").is_file(), "Godot retains backend-neutral native logic");
        assert!(report["files"].as_array().unwrap().iter().any(|f| f["path"].as_str().unwrap_or("").starts_with(".forge/cache/rxdll/")));
        for rel in assetd::godot_runtime::REQUIRED_FILES {
            assert!(
                out.join("runtime").join(rel).is_file(),
                "runtime copy missing {rel}"
            );
        }
        assert!(out.join("forge.toml").is_file());
        let script = std::fs::read_to_string(out.join("pack-run.ps1")).unwrap();
        for required in [
            "FORGE_PROJECT_ROOT",
            "FORGE_HOST_PORT = 19001",
            "FORGE_GAME_SCENE",
            "FORGE_RENDER_BACKEND = 'godot'",
            "FORGE_RENDER_METHOD = 'mobile'",
            "FORGE_RENDER_DRIVER = 'd3d12'",
            "FORGE_GODOT_RUNTIME_DIR",
            "FORGE_RURIXC = Join-Path $root 'bin\\rurixc.exe'",
            "forge-godot_console.exe",
        ] {
            assert!(
                script.contains(required),
                "launcher missing {required}: {script}"
            );
        }
        assert!(
            !script.contains(&runtime.to_string_lossy().to_string()),
            "portable launcher must not reference build-machine runtime: {script}"
        );
        assert!(!script.contains("target\\godot-runtime"));
        assert_eq!(
            assetd::godot_runtime::validate_runtime(out.join("runtime"))
                .unwrap()
                .defaults
                .method,
            "mobile"
        );
        assert_eq!(
            assetd::godot_runtime::validate_runtime(out.join("runtime"))
                .unwrap()
                .defaults
                .driver,
            "d3d12"
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(runtime);
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn pack_rejects_missing_corrupt_runtime_and_illegal_render_config() {
        let root = fixture_project("godot-errors");
        std::fs::write(
            root.join("forge.toml"),
            "[project]\nname = \"bad\"\n\n[render]\nbackend = \"godot\"\n",
        )
        .unwrap();
        let runtime = runtime_fixture("damaged");
        std::fs::write(runtime.join("bin/godot_host.dll"), b"corrupt").unwrap();
        let out = std::env::temp_dir().join(format!("forge_pack_godot_err_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let err = build_pack_with_runtime(
            &root.join("Content/Scenes/s.rxscene"),
            &root,
            &out,
            Path::new("unused"),
            &runtime,
            17890,
        )
        .unwrap_err();
        assert!(err.starts_with("PACK_GODOT_RUNTIME_INVALID:"), "{err}");
        let missing = build_pack_with_runtime(
            &root.join("Content/Scenes/s.rxscene"),
            &root,
            &out,
            Path::new("unused"),
            &root.join("missing-runtime"),
            17890,
        )
        .unwrap_err();
        assert!(
            missing.starts_with("PACK_GODOT_RUNTIME_MISSING:"),
            "{missing}"
        );
        std::fs::write(root.join("forge.toml"), "[project]\nname = \"bad\"\n\n[render]\nbackend = \"godot\"\nmethod = \"gl_compatibility\"\ndriver = \"d3d12\"\n").unwrap();
        let bad_config = build_pack_with_runtime(
            &root.join("Content/Scenes/s.rxscene"),
            &root,
            &out,
            Path::new("unused"),
            &runtime,
            17890,
        )
        .unwrap_err();
        assert!(
            bad_config.starts_with("PACK_RENDER_CONFIG_INVALID:"),
            "{bad_config}"
        );
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(runtime);
    }

    #[test]
    fn build_pack_compatibility_signature_keeps_rurix_launcher_and_rejects_nonempty_output() {
        let root = fixture_project("compat");
        let out =
            std::env::temp_dir().join(format!("forge_pack_compat_out_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let engine = root.join("engine-host.exe");
        std::fs::write(&engine, b"MZ fixture").unwrap();
        let report = build_pack(
            &root.join("Content/Scenes/s.rxscene"),
            &root,
            &out,
            &engine,
            17890,
        )
        .unwrap();
        assert_eq!(report["backend"], "rurix");
        assert!(out.join("Content/Graphs/g.rxgraph").is_file());
        assert!(out.join("Content/Scripts/m.rx").is_file());
        assert!(out.join("bin/engine-host.exe").is_file());
        assert!(out.join("bin/rurixc.exe").is_file());
        assert!(out.join("pack-run.ps1").is_file());
        assert!(!out.join("Content/Graphs/other.rxgraph").exists());
        let script = std::fs::read_to_string(out.join("pack-run.ps1")).unwrap();
        assert!(script.contains("FORGE_PROJECT_ROOT"));
        assert!(script.contains("FORGE_GAME_SCENE = 'Content/Scenes/s.rxscene'"));
        assert!(script.contains("FORGE_HOST_PORT = 17890"));
        assert!(
            script.contains("--port 17890 --game \"Content/Scenes/s.rxscene\""),
            "{script}"
        );
        let err = build_pack(
            &root.join("Content/Scenes/s.rxscene"),
            &root,
            &out,
            &engine,
            17890,
        )
        .unwrap_err();
        assert!(err.contains("PACK_OUTDIR_CONFLICT"), "{err}");
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(out);
    }
}
