//! Godot standalone runtime manifest validation.
//!
//! A runtime directory is self-contained: validation checks every required file and
//! its SHA-256 from `runtime-manifest.json`. Development builds may additionally
//! compare the bundled extension with a local build output; portable runtimes must
//! use `validate_runtime` only and therefore need no source checkout.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::{AssetError, Result};

pub const MANIFEST_NAME: &str = "runtime-manifest.json";
pub const REQUIRED_FILES: &[&str] = &[
    "forge-godot.exe",
    "forge-godot_console.exe",
    "project.godot",
    "forge_host.gdextension",
    "forge_runtime.tscn",
    ".godot/extension_list.cfg",
    ".godot/global_script_class_cache.cfg",
    "bin/godot_host.dll",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeManifest {
    pub schema: String,
    pub godot: String,
    pub template: String,
    pub profile: String,
    pub defaults: RuntimeDefaults,
    pub sha256: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDefaults {
    pub method: String,
    pub driver: String,
    pub max_fps: u32,
}

fn invalid(message: impl Into<String>) -> AssetError {
    AssetError::new("GODOT_RUNTIME_INVALID", message)
}

fn safe_relative_file(relative: &str) -> Result<PathBuf> {
    let normalized = relative.replace('\\', "/");
    let path = Path::new(&normalized);
    if normalized.is_empty()
        || path.is_absolute()
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':'))
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(invalid(format!("manifest 含不安全文件路径: {relative:?}")));
    }
    Ok(path.to_path_buf())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Validate manifest schema, required files, and the hash of every declared file.
pub fn validate_runtime(runtime_dir: impl AsRef<Path>) -> Result<RuntimeManifest> {
    let runtime_dir = runtime_dir.as_ref();
    let manifest_path = runtime_dir.join(MANIFEST_NAME);
    let bytes = std::fs::read(&manifest_path)
        .map_err(|e| invalid(format!("运行时清单不可读 {}: {e}", manifest_path.display())))?;
    let manifest: RuntimeManifest = serde_json::from_slice(&bytes).map_err(|e| {
        invalid(format!(
            "运行时清单格式错误 {}: {e}",
            manifest_path.display()
        ))
    })?;
    if manifest.schema != "forge.godot_runtime.v1" {
        return Err(invalid(format!(
            "不支持的运行时清单 schema: {:?}",
            manifest.schema
        )));
    }
    if manifest.godot.trim().is_empty() {
        return Err(invalid("运行时清单缺 Godot 版本"));
    }
    if !matches!(manifest.template.as_str(), "debug" | "release")
        || !matches!(manifest.profile.as_str(), "debug" | "release")
    {
        return Err(invalid(format!(
            "运行时 template/profile 非法: {:?}/{:?}",
            manifest.template, manifest.profile
        )));
    }
    if !(1..=240).contains(&manifest.defaults.max_fps) {
        return Err(invalid(format!(
            "运行时 maxFps 超范围: {}",
            manifest.defaults.max_fps
        )));
    }
    assetd_render_config(&manifest.defaults.method, &manifest.defaults.driver)
        .map_err(|e| invalid(format!("运行时渲染缺省配置非法: {e}")))?;

    for required in REQUIRED_FILES {
        if !manifest.sha256.contains_key(*required) {
            return Err(invalid(format!("运行时清单缺必需文件哈希: {required}")));
        }
    }
    // Check each component, not just the leaf: a linked bin/ directory can otherwise
    // redirect a manifest entry outside the supposedly self-contained runtime.
    for relative in std::iter::once(MANIFEST_NAME).chain(manifest.sha256.keys().map(String::as_str)) {
        let safe = safe_relative_file(relative)?;
        let mut path = runtime_dir.to_path_buf();
        for component in safe.components() {
            path.push(component.as_os_str());
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|e| invalid(format!("运行时必需文件缺失或不可读 {}: {e}", path.display())))?;
            if metadata.file_type().is_symlink() {
                return Err(invalid(format!("运行时路径不允许符号链接: {}", path.display())));
            }
        }
    }
    for (relative, expected) in &manifest.sha256 {
        if !valid_sha256(expected) {
            return Err(invalid(format!("运行时清单 SHA-256 格式错误: {relative}")));
        }
        let safe = safe_relative_file(relative)?;
        let path = runtime_dir.join(safe);
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| {
            invalid(format!(
                "运行时必需文件缺失或不可读 {}: {e}",
                path.display()
            ))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid(format!(
                "运行时清单路径必须是普通文件: {}",
                path.display()
            )));
        }
        let actual = forge_util::hashutil::sha256_file(&path)
            .map_err(|e| invalid(format!("运行时文件无法校验 {}: {e}", path.display())))?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(invalid(format!(
                "运行时文件哈希不符: {relative}(清单 {expected},实际 {actual})"
            )));
        }
    }
    Ok(manifest)
}

fn assetd_render_config(method: &str, driver: &str) -> Result<()> {
    crate::project::RenderConfig::from_parts(Some("godot"), Some(method), Some(driver)).map(|_| ())
}

/// Validate a development runtime and reject an extension DLL built after its manifest.
pub fn validate_development_runtime(
    runtime_dir: impl AsRef<Path>,
    source_dll: impl AsRef<Path>,
) -> Result<RuntimeManifest> {
    let runtime_dir = runtime_dir.as_ref();
    let source_dll = source_dll.as_ref();
    let manifest = validate_runtime(runtime_dir)?;
    if !source_dll.is_file() {
        return Err(AssetError::new(
            "GODOT_RUNTIME_STALE",
            format!(
                "开发运行时缺少当前扩展 {}；请先运行 `cargo build -p godot-host --locked`，再运行 `scripts\\godot-runtime.ps1 -Build`。",
                source_dll.display()
            ),
        ));
    }
    let bundled = manifest
        .sha256
        .get("bin/godot_host.dll")
        .expect("required hash validated");
    let current = forge_util::hashutil::sha256_file(source_dll).map_err(|e| {
        AssetError::new(
            "GODOT_RUNTIME_STALE",
            format!("读取当前 Godot 扩展失败 {}: {e}", source_dll.display()),
        )
    })?;
    if !current.eq_ignore_ascii_case(bundled) {
        return Err(AssetError::new(
            "GODOT_RUNTIME_STALE",
            format!(
                "开发运行时中的 bin/godot_host.dll 已过期(清单 {bundled},当前构建 {current})；请运行 `cargo build -p godot-host --locked`，随后运行 `scripts\\godot-runtime.ps1 -Build`。"
            ),
        ));
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(tag: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("assetd-godot-runtime-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for rel in REQUIRED_FILES {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("runtime file: {rel}")).unwrap();
        }
        root
    }

    fn write_manifest(root: &Path) {
        let mut hashes = serde_json::Map::new();
        for rel in REQUIRED_FILES {
            hashes.insert(
                (*rel).to_string(),
                json!(forge_util::hashutil::sha256_file(&root.join(rel)).unwrap()),
            );
        }
        let doc = json!({
            "schema": "forge.godot_runtime.v1",
            "godot": "4.7.2.stable.official",
            "template": "release",
            "profile": "debug",
            "defaults": {"method":"forward_plus", "driver":"d3d12", "maxFps":60},
            "sha256": hashes
        });
        std::fs::write(root.join(MANIFEST_NAME), serde_json::to_vec(&doc).unwrap()).unwrap();
    }

    #[test]
    fn runtime_manifest_requires_files_and_hashes() {
        let root = fixture("good");
        write_manifest(&root);
        let parsed = validate_runtime(&root).unwrap();
        assert_eq!(parsed.defaults.method, "forward_plus");
        std::fs::write(root.join("bin/godot_host.dll"), b"tampered").unwrap();
        assert!(validate_runtime(&root)
            .unwrap_err()
            .message
            .contains("哈希不符"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_manifest_reports_missing_mandatory_file_or_hash() {
        let root = fixture("missing");
        write_manifest(&root);
        std::fs::remove_file(root.join("forge-godot.exe")).unwrap();
        assert!(validate_runtime(&root)
            .unwrap_err()
            .message
            .contains("文件缺失"));
        std::fs::write(root.join("forge-godot.exe"), "repaired runtime binary").unwrap();
        write_manifest(&root);
        // Recreate the manifest after the missing-file assertion; the helper hashes every file.
        let path = root.join(MANIFEST_NAME);
        let mut doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        doc["sha256"]
            .as_object_mut()
            .unwrap()
            .remove("project.godot");
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        assert!(validate_runtime(&root)
            .unwrap_err()
            .message
            .contains("缺必需文件哈希"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn development_runtime_detects_stale_extension_but_portable_validation_does_not_need_source() {
        let root = fixture("stale");
        write_manifest(&root);
        let source = root.join("current-godot-host.dll");
        std::fs::write(&source, b"newer extension").unwrap();
        assert_eq!(
            validate_development_runtime(&root, &source)
                .unwrap_err()
                .code,
            "GODOT_RUNTIME_STALE"
        );
        assert!(
            validate_runtime(&root).is_ok(),
            "便携运行时只依赖自身文件和 manifest，不要求源码 DLL"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_rejects_traversal_and_illegal_render_defaults() {
        let root = fixture("bad-config");
        write_manifest(&root);
        let path = root.join(MANIFEST_NAME);
        let mut doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        doc["defaults"]["driver"] = json!("opengl3");
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        assert!(validate_runtime(&root)
            .unwrap_err()
            .message
            .contains("渲染缺省配置非法"));
        write_manifest(&root);
        let mut doc: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        doc["sha256"]["../outside.dll"] = json!("0".repeat(64));
        std::fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        assert!(validate_runtime(&root).unwrap_err().message.contains("不安全文件路径"));
        for relative in ["/absolute", "C:/outside", "bin/../outside", "bin//file", "bin\\..\\file", "bin/file:stream"] {
            assert!(safe_relative_file(relative).is_err(), "{relative}");
        }
        let _ = std::fs::remove_dir_all(root);
    }
}
