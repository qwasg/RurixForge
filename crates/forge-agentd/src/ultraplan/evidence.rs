//! Bind verification to authored game files, not mutable cache/report output.
//! Forge publishes assets and scripts through its manifest's declared roots;
//! legacy data scenes and common loose source files are included as well.

use forge_util::hashutil::{sha256_file, Sha256Stream};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

fn plain_metadata(path: &Path) -> Result<std::fs::Metadata, String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("项目证据文件无法读取 {}: {e}", path.display()))?;
    let linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || metadata.file_attributes() & 0x400 != 0
    };
    if linked {
        return Err(format!("项目证据不接受链接或重解析点: {}", path.display()));
    }
    Ok(metadata)
}

fn excluded(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        ".forge" | ".git" | ".godot" | "node_modules" | "target" | "__pycache__"
    )
}

fn declared_path(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let relative = Path::new(raw);
    if raw.trim().is_empty()
        || relative.is_absolute()
        || raw.contains(':')
        || relative
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err("项目发布目录必须位于项目根内".into());
    }
    let mut path = root.to_path_buf();
    for part in relative.components() {
        if let Component::Normal(name) = part {
            if excluded(&name.to_string_lossy()) {
                return Err("项目发布目录不能指向缓存或流程记录".into());
            }
            path.push(name);
            if path.exists() {
                plain_metadata(&path)?;
            }
        }
    }
    Ok(path)
}

fn collect(path: &Path, files: &mut BTreeSet<PathBuf>, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err("项目证据目录超过64层".into());
    }
    let metadata = plain_metadata(path)?;
    if metadata.is_file() {
        files.insert(path.to_path_buf());
        if files.len() > 100_000 {
            return Err("项目发布文件过多，无法完成自动证据核对".into());
        }
    } else if metadata.is_dir() {
        for child in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            let child = child.map_err(|e| e.to_string())?;
            if !excluded(&child.file_name().to_string_lossy()) {
                collect(&child.path(), files, depth + 1)?;
            }
        }
    } else {
        return Err("项目证据只能来自普通文件".into());
    }
    Ok(())
}

/// A deterministic streaming hash of the manifest, published Content assets,
/// scripts, entry scene and legacy/loose source files. No bytes from `.forge`
/// (verification reports, jobs, caches), `.godot`, or compiler caches enter it.
fn published_files(project_root: &Path) -> Result<(PathBuf, BTreeSet<PathBuf>), String> {
    plain_metadata(project_root)?;
    let root = project_root.canonicalize().map_err(|e| e.to_string())?;
    if !root.join("forge.toml").is_file() {
        return Err("正式项目缺少forge.toml，不能生成验证指纹".into());
    }
    let project = assetd::project::ForgeProject::load(&root).map_err(|e| e.to_string())?;
    let mut files = BTreeSet::new();
    collect(&root.join("forge.toml"), &mut files, 0)?;
    for raw in [
        &project.content_dir,
        &project.scripts_dir,
        &project.entry_scene,
    ] {
        let path = declared_path(&root, raw)?;
        if path.exists() {
            collect(&path, &mut files, 0)?;
        }
    }
    // Older Forge games use data/scene.rxscene; authored source directories can
    // also live outside Content. Do not scan sibling projects/history trees.
    for name in [
        "data", "Scripts", "scripts", "Scenes", "scenes", "src", "Assets", "assets",
    ] {
        let path = root.join(name);
        if path.exists() {
            collect(&path, &mut files, 0)?;
        }
    }
    for child in std::fs::read_dir(&root).map_err(|e| e.to_string())? {
        let path = child.map_err(|e| e.to_string())?.path();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if path.is_file()
            && (matches!(
                extension.as_str(),
                "rxscene"
                    | "rxgraph"
                    | "rx"
                    | "gd"
                    | "tscn"
                    | "tres"
                    | "gdshader"
                    | "rs"
                    | "cs"
                    | "lua"
                    | "json"
                    | "meta"
            ) || path.file_name().is_some_and(|n| n == "project.godot"))
        {
            collect(&path, &mut files, 0)?;
        }
    }
    Ok((root, files))
}

/// A check cannot point at an untracked cache scene and then claim the stable
/// fingerprint of unrelated published files as its version binding.
pub fn require_published_file(project_root: &Path, file: &Path) -> Result<(), String> {
    let (_, files) = published_files(project_root)?;
    let canonical = file.canonicalize().map_err(|e| e.to_string())?;
    if !files.contains(&canonical) {
        return Err("检查场景必须属于forge.toml声明的发布内容、脚本或受跟踪的正式源文件".into());
    }
    Ok(())
}

pub fn project_fingerprint(project_root: &Path) -> Result<String, String> {
    let (root, files) = published_files(project_root)?;
    let mut hash = Sha256Stream::new();
    hash.update(b"forge-project-evidence-v1\0");
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let before = plain_metadata(&path)?;
        let digest = sha256_file(&path).map_err(|e| e.to_string())?;
        let after = plain_metadata(&path)?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
            return Err("项目文件正在变化，请完成写入后重新验证".into());
        }
        hash.update(&(relative.len() as u64).to_le_bytes());
        hash.update(relative.as_bytes());
        hash.update(&before.len().to_le_bytes());
        hash.update(digest.as_bytes());
    }
    Ok(hash.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameplay_and_assets_invalidate_evidence_but_reports_and_caches_do_not() {
        let dir = std::env::temp_dir().join(crate::events::new_id("project-fingerprint"));
        std::fs::create_dir_all(dir.join("Content/Scripts")).unwrap();
        std::fs::create_dir_all(dir.join("Content/Scenes")).unwrap();
        std::fs::write(
            dir.join("forge.toml"),
            "[project]\nname=\"test\"\nentry-scene=\"Content/Scenes/Main.rxscene\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("Content/Scenes/Main.rxscene"), "scene-one").unwrap();
        std::fs::write(dir.join("Content/Scripts/player.rx"), "move right").unwrap();
        let baseline = project_fingerprint(&dir).unwrap();
        std::fs::create_dir_all(dir.join(".forge/ultraplan/flow")).unwrap();
        std::fs::write(dir.join(".forge/ultraplan/flow/report.json"), "new report").unwrap();
        std::fs::create_dir_all(dir.join(".godot")).unwrap();
        std::fs::write(dir.join(".godot/cache"), "changed").unwrap();
        assert!(require_published_file(&dir, &dir.join("Content/Scenes/Main.rxscene")).is_ok());
        assert!(
            require_published_file(&dir, &dir.join(".forge/ultraplan/flow/report.json")).is_err()
        );
        assert_eq!(project_fingerprint(&dir).unwrap(), baseline);
        std::fs::write(dir.join("Content/Scripts/player.rx"), "move left").unwrap();
        assert_ne!(project_fingerprint(&dir).unwrap(), baseline);
        std::fs::write(dir.join("Content/Scripts/player.rx"), "move right").unwrap();
        assert_eq!(project_fingerprint(&dir).unwrap(), baseline);
        std::fs::write(dir.join("Content/texture.bin"), [1, 2, 3, 4]).unwrap();
        assert_ne!(project_fingerprint(&dir).unwrap(), baseline);
        std::fs::remove_file(dir.join("Content/texture.bin")).unwrap();
        std::fs::write(dir.join("Content/Scenes/Main.rxscene"), "scene-two").unwrap();
        assert_ne!(project_fingerprint(&dir).unwrap(), baseline);
        assert_eq!(
            dir.canonicalize().unwrap().parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
