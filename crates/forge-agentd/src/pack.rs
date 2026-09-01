//! F6 wave.4 project-pack 最小打包(D-F6-D):场景引用闭包收集(组件 props 内
//! Content/ 引用 + guid 引用经 .meta 索引解析(F9 D6)+ .rxgraph 的 call_function
//! module 链)→ Content 源 + .meta +
//! .forge 缓存(rxdll dll)+ engine-host 二进制 + pack-run.ps1 → 独立目录。
//! 诚实纪律:闭包缺件/缓存缺失/二进制缺失如实报错或进 warnings;闭包外资产不入包。

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};

use assetd::meta::MetaDoc;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRequest {
    /// 场景(workspace 相对或绝对)。
    pub scene_ref: String,
    /// 输出目录(workspace 相对或绝对;存在且非空 → 409)。
    pub out_dir: String,
}

/// 递归收集 JSON 值中的全部字符串(组件 props / 图 inputs 通用;调用侧按
/// Content/ 前缀 / guid 索引 / guid 形态分类,F9 D6:guid 引用亦须入闭包)。
fn collect_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        Value::Object(m) => m.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

/// guid 形态判定(8-4-4-4-12 十六进制连字符,与 assetd::new_guid v4 输出一致)。
fn looks_like_guid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    b.iter().enumerate().all(|(i, c)| match i {
        8 | 13 | 18 | 23 => *c == b'-',
        _ => c.is_ascii_hexdigit(),
    })
}

/// guid → content 相对路径索引:扫 Content/ 下全部 .meta,复用 assetd
/// MetaDoc::load 解析(解析失败的 .meta 跳过,与 refs::RefGraph::rebuild 同纪律)。
fn build_guid_index(project_root: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut stack = vec![project_root.join("Content")];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            if !e.file_name().to_string_lossy().ends_with(".meta") {
                continue;
            }
            let Ok(m) = MetaDoc::load(&p) else { continue };
            if let Ok(rel) = p.strip_prefix(project_root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.insert(m.guid, rel.strip_suffix(".meta").unwrap_or(&rel).to_string());
            }
        }
    }
    out
}

/// 引用闭包 BFS:scene → (graphRef 等 Content/ 引用) → .rxgraph → (module 链)。
/// 返回 (content 相对路径集, warnings);引用文件不存在 → warning 如实(不入闭包)。
pub fn collect_closure(scene_abs: &Path, project_root: &Path) -> (BTreeSet<String>, Vec<String>) {
    let mut closure: BTreeSet<String> = BTreeSet::new();
    let mut warnings: Vec<String> = Vec::new();
    let guid_index = build_guid_index(project_root);
    let rel_scene = scene_abs
        .strip_prefix(project_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| scene_abs.to_string_lossy().replace('\\', "/"));
    let mut queue: VecDeque<String> = VecDeque::from([rel_scene]);
    while let Some(rel) = queue.pop_front() {
        if closure.contains(&rel) {
            continue;
        }
        let abs = project_root.join(&rel);
        // 先读后收:不可读引用 → warning 如实且不入闭包(打包不得拷缺件)。
        // 以字节读判可读性:贴图等二进制资产非 UTF-8,read_to_string 会误判缺失。
        let Ok(bytes) = std::fs::read(&abs) else {
            warnings.push(format!("引用缺失: {rel}({} 不可读)", abs.display()));
            continue;
        };
        closure.insert(rel.clone());
        // 仅 JSON 类资产递归扫引用;.rx 等文本资产为叶子(其引用由图 module 链带入)。
        // .rxmat 亦为 JSON(textures.* 以 guid 引用贴图,F9 D6 链)。
        let jsonish = rel.ends_with(".rxscene")
            || rel.ends_with(".rxgraph")
            || rel.ends_with(".rxmat")
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
        collect_strings(&doc, &mut found);
        for f in found {
            if f.starts_with("Content/") {
                if !closure.contains(&f) {
                    queue.push_back(f);
                }
            } else if let Some(target) = guid_index.get(&f) {
                // guid 引用 → 经 .meta 索引映射回资产路径入闭包。
                if !closure.contains(target) {
                    queue.push_back(target.clone());
                }
            } else if looks_like_guid(&f) {
                // guid 形态但无对应 .meta:如实 warning,不静默不 fail。
                warnings.push(format!("guid 悬空: {f}({rel} 引用,无对应资产)"));
            }
        }
    }
    // .meta 伴随文件(存在才收,不警告——.rx 等类型本无 .meta 先例)。
    let metas: Vec<String> = closure
        .iter()
        .map(|r| format!("{r}.meta"))
        .filter(|m| project_root.join(m).exists())
        .collect();
    closure.extend(metas);
    (closure, warnings)
}

/// 场景引用的 .rx 脚本对应的 rxdll 缓存 dll(stem-*.dll;缺 → warning)。
fn collect_rxdll(closure: &BTreeSet<String>, project_root: &Path, warnings: &mut Vec<String>) -> Vec<PathBuf> {
    let cache = project_root.join(".forge").join("cache").join("rxdll");
    let mut out = Vec::new();
    for rel in closure.iter().filter(|r| r.ends_with(".rx")) {
        let stem = Path::new(rel)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
        let mut hit = false;
        if let Ok(rd) = std::fs::read_dir(&cache) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(&format!("{stem}-")) && name.ends_with(".dll") {
                    out.push(e.path());
                    hit = true;
                }
            }
        }
        if !hit {
            warnings.push(format!("rxdll 缓存缺失: {rel}(干净机无 rurixc 时 call_function 将 logic.call_error)"));
        }
    }
    out
}

/// pack-run.ps1 内容(独立目录自包含;FORGE_PROJECT_ROOT 指包根)。
fn run_script(scene_content_rel: &str, port: u16) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\r\n$root = Split-Path -Parent $MyInvocation.MyCommand.Path\r\n$env:FORGE_PROJECT_ROOT = $root\r\n& \"$root\\bin\\engine-host.exe\" --port {port} --game \"{scene}\"\r\n",
        port = port,
        scene = scene_content_rel
    )
}

/// 打包主流程(可测试内核;engine_bin 由路由侧定位 target\debug\engine-host.exe)。
pub fn build_pack(
    scene_abs: &Path,
    project_root: &Path,
    out_dir: &Path,
    engine_bin: &Path,
    port: u16,
) -> Result<Value, String> {
    if !scene_abs.exists() {
        return Err(format!("PACK_SCENE_NOT_FOUND: {}", scene_abs.display()));
    }
    if !engine_bin.exists() {
        return Err(format!("PACK_ENGINE_MISSING: {}", engine_bin.display()));
    }
    if out_dir.exists() && std::fs::read_dir(out_dir).map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err(format!("PACK_OUTDIR_CONFLICT: {} 已存在且非空", out_dir.display()));
    }
    let (closure, mut warnings) = collect_closure(scene_abs, project_root);
    // 场景 content 相对路径(--game 参数;须在 Content/ 下)。
    let scene_rel = scene_abs
        .strip_prefix(project_root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .map_err(|e| e.to_string())?;
    if !scene_rel.starts_with("Content/") {
        return Err(format!("PACK_SCENE_OUTSIDE_CONTENT: {scene_rel}"));
    }
    let dlls = collect_rxdll(&closure, project_root, &mut warnings);
    let mut files: Vec<Value> = Vec::new();
    let mut total: u64 = 0;
    let mut copy_in = |src: &Path, dst: &Path| -> Result<(), String> {
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p).map_err(|e| format!("建目录失败 {}: {e}", p.display()))?;
        }
        std::fs::copy(src, dst).map_err(|e| format!("拷贝失败 {} → {}: {e}", src.display(), dst.display()))?;
        let bytes = std::fs::metadata(dst).map(|m| m.len()).unwrap_or(0);
        total += bytes;
        files.push(json!({ "path": dst.strip_prefix(out_dir).unwrap_or(dst).to_string_lossy().replace('\\', "/"), "bytes": bytes }));
        Ok(())
    };
    std::fs::create_dir_all(out_dir).map_err(|e| format!("建输出目录失败 {}: {e}", out_dir.display()))?;
    for rel in &closure {
        copy_in(&project_root.join(rel), &out_dir.join(rel))?;
    }
    for dll in &dlls {
        let name = dll.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        copy_in(dll, &out_dir.join(".forge").join("cache").join("rxdll").join(name))?;
    }
    copy_in(engine_bin, &out_dir.join("bin").join("engine-host.exe"))?;
    let script = run_script(&scene_rel, port);
    let script_path = out_dir.join("pack-run.ps1");
    std::fs::write(&script_path, &script).map_err(|e| format!("写启动脚本失败: {e}"))?;
    files.push(json!({ "path": "pack-run.ps1", "bytes": script.len() }));
    total += script.len() as u64;
    Ok(json!({
        "outDir": out_dir.to_string_lossy().replace('\\', "/"),
        "scene": scene_rel,
        "closure": closure.iter().collect::<Vec<_>>(),
        "files": files,
        "totalBytes": total,
        "debugEngine": true,
        "warnings": warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时项目:scene → graph → script 链 + 闭包外资产(不入包断言)。
    /// tag 区分并发测试临时目录(同进程多测试共享 pid,须标签隔离)。
    fn fixture_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("forge_f6w4_pack_test_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Content/Scenes")).unwrap();
        std::fs::create_dir_all(dir.join("Content/Graphs")).unwrap();
        std::fs::create_dir_all(dir.join("Content/Scripts")).unwrap();
        std::fs::write(
            dir.join("Content/Scenes/s.rxscene"),
            r#"{ "name": "s", "next_id": 2, "entities": [ { "id": 1, "name": "e", "transform": { "translation": [0,0,0], "rotation": [0,0,0,1], "scale": [1,1,1] }, "components": [ { "type": "Script", "enabled": true, "props": { "module": "", "graphRef": "Content/Graphs/g.rxgraph", "props": {} } } ] } ] }"#,
        )
        .unwrap();
        std::fs::write(dir.join("Content/Scenes/s.rxscene.meta"), "guid: x\ntype: scene\n").unwrap();
        std::fs::write(
            dir.join("Content/Graphs/g.rxgraph"),
            r#"{ "version": 1, "id": "g", "name": "g", "nodes": [ { "id": "c", "type": "call.call_function", "inputs": { "module": { "const": "Content/Scripts/m.rx" }, "fn": { "const": "f" }, "args": { "const": [] } } } ], "edges": [] }"#,
        )
        .unwrap();
        std::fs::write(dir.join("Content/Graphs/g.rxgraph.meta"), "guid: y\ntype: script\n").unwrap();
        std::fs::write(dir.join("Content/Scripts/m.rx"), "#[export(c)]\npub fn f() -> i32 { 1 }\n").unwrap();
        // 闭包外资产(不得入包)。
        std::fs::write(dir.join("Content/Graphs/other.rxgraph"), "{}").unwrap();
        dir
    }

    #[test]
    fn closure_bfs_scene_graph_script() {
        let root = fixture_project("bfs");
        let (closure, warnings) = collect_closure(&root.join("Content/Scenes/s.rxscene"), &root);
        assert!(closure.contains("Content/Scenes/s.rxscene"));
        assert!(closure.contains("Content/Scenes/s.rxscene.meta"));
        assert!(closure.contains("Content/Graphs/g.rxgraph"));
        assert!(closure.contains("Content/Graphs/g.rxgraph.meta"));
        assert!(closure.contains("Content/Scripts/m.rx"), "graph module 链须入闭包: {closure:?}");
        assert!(!closure.contains("Content/Graphs/other.rxgraph"), "闭包外资产不得入集");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn closure_missing_ref_warns_not_fails() {
        let root = fixture_project("missing");
        std::fs::remove_file(root.join("Content/Scripts/m.rx")).unwrap();
        let (closure, warnings) = collect_closure(&root.join("Content/Scenes/s.rxscene"), &root);
        assert!(warnings.iter().any(|w| w.contains("m.rx")), "{warnings:?}");
        assert!(!closure.contains("Content/Scripts/m.rx"));
    }

    /// 临时项目(F9 D6 实测形态):场景 MeshRenderer.material 以 guid 引用材质,
    /// 材质 textures.albedo 再以 guid 引用贴图(material→texture 链)。
    fn fixture_project_guid(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("forge_f6w4_pack_guid_{}_{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Content/Scenes")).unwrap();
        std::fs::create_dir_all(dir.join("Content/Materials")).unwrap();
        std::fs::create_dir_all(dir.join("Content/Textures")).unwrap();
        std::fs::write(
            dir.join("Content/Scenes/g.rxscene"),
            r#"{ "name": "g", "entities": [ { "id": 1, "name": "box", "components": [ { "type": "MeshRenderer", "enabled": true, "props": { "mesh": "cube", "material": "2a1004f5-6469-437f-84d4-04b3482df41c" } } ] } ] }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("Content/Materials/m.rxmat"),
            r#"{ "version": 1, "shader": "pbr-default", "params": {}, "textures": { "albedo": "e858d90b-19b7-425e-84a0-01a92733d053" } }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("Content/Materials/m.rxmat.meta"),
            "guid: 2a1004f5-6469-437f-84d4-04b3482df41c\ntype: material\nimporter: material\n",
        )
        .unwrap();
        std::fs::write(dir.join("Content/Textures/t.png"), b"PNG-dummy").unwrap();
        std::fs::write(
            dir.join("Content/Textures/t.png.meta"),
            "guid: e858d90b-19b7-425e-84a0-01a92733d053\ntype: texture\nimporter: png\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn closure_guid_ref_resolves_to_asset() {
        let root = fixture_project_guid("resolve");
        let (closure, warnings) = collect_closure(&root.join("Content/Scenes/g.rxscene"), &root);
        assert!(closure.contains("Content/Materials/m.rxmat"), "guid 引用材质须入闭包: {closure:?}");
        assert!(closure.contains("Content/Materials/m.rxmat.meta"), "材质 .meta 须伴随入闭包: {closure:?}");
        assert!(closure.contains("Content/Textures/t.png"), "材质 textures guid 链须入闭包: {closure:?}");
        assert!(closure.contains("Content/Textures/t.png.meta"), "{closure:?}");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn closure_dangling_guid_warns_not_fails() {
        let root = fixture_project_guid("dangling");
        std::fs::remove_file(root.join("Content/Materials/m.rxmat.meta")).unwrap();
        let (closure, warnings) = collect_closure(&root.join("Content/Scenes/g.rxscene"), &root);
        assert!(
            warnings.iter().any(|w| w.contains("guid 悬空") && w.contains("2a1004f5-6469-437f-84d4-04b3482df41c")),
            "悬空 guid 须如实进 warnings: {warnings:?}"
        );
        assert!(!closure.contains("Content/Materials/m.rxmat"));
        assert!(closure.contains("Content/Scenes/g.rxscene"), "不 fail,场景仍在闭包");
    }

    #[test]
    fn build_pack_layout_and_exclusion() {
        let root = fixture_project("layout");
        let out = std::env::temp_dir().join(format!("forge_f6w4_pack_out_{}_layout", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let bin = root.join("engine-host.exe");
        std::fs::write(&bin, b"MZ-dummy").unwrap();
        let r = build_pack(&root.join("Content/Scenes/s.rxscene"), &root, &out, &bin, 17890)
            .expect("打包须成功");
        assert_eq!(r["scene"], "Content/Scenes/s.rxscene");
        assert!(out.join("Content/Scenes/s.rxscene").exists());
        assert!(out.join("Content/Graphs/g.rxgraph").exists());
        assert!(out.join("Content/Scripts/m.rx").exists());
        assert!(out.join("bin/engine-host.exe").exists());
        assert!(out.join("pack-run.ps1").exists());
        assert!(!out.join("Content/Graphs/other.rxgraph").exists(), "闭包外资产不入包");
        let script = std::fs::read_to_string(out.join("pack-run.ps1")).unwrap();
        assert!(script.contains("--game \"Content/Scenes/s.rxscene\""), "{script}");
        assert!(script.contains("FORGE_PROJECT_ROOT"));
        // rxdll 缺失如实 warning(fixture 无 .forge 缓存)。
        assert!(r["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("rxdll 缓存缺失")));
        // 输出目录非空冲突。
        let err = build_pack(&root.join("Content/Scenes/s.rxscene"), &root, &out, &bin, 17890).unwrap_err();
        assert!(err.contains("PACK_OUTDIR_CONFLICT"), "{err}");
        let _ = std::fs::remove_dir_all(&out);
    }
}
