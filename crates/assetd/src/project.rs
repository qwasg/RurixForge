//! 项目布局:forge.toml + Content/ + .forge/cache/ + .forge/tmp/ (08 §3.1)。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{AssetError, Result};

/// 项目清单(forge.toml)。
#[derive(Debug, Clone)]
pub struct ForgeProject {
    pub root: PathBuf,
    pub name: String,
    pub engine_version: String,
    pub rurix_ref: String,
    pub entry_scene: String,
    pub content_dir: String,
    pub scripts_dir: String,
}

impl ForgeProject {
    /// 从项目根加载 forge.toml;缺失时用缺省填充(08 §3.1 示例)。
    pub fn load(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let manifest = root.join("forge.toml");
        if !manifest.is_file() {
            return Ok(ForgeProject::with_defaults(root));
        }
        let text = std::fs::read_to_string(&manifest)?;
        let doc = rurix_pkg::toml::parse(&text)
            .map_err(|e| AssetError::new("PARSE_ERR", format!("forge.toml 解析失败: {e}")))?;
        let proj = doc
            .get("project")
            .and_then(|v| v.as_table())
            .ok_or_else(|| AssetError::new("PARSE_ERR", "forge.toml 缺 [project]"))?;
        let dirs = doc.get("dirs").and_then(|v| v.as_table());
        Ok(ForgeProject {
            root: root.clone(),
            name: proj.get("name").and_then(|v| v.as_str()).unwrap_or("untitled").into(),
            engine_version: proj
                .get("engine-version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.1.0")
                .into(),
            rurix_ref: proj
                .get("rurix-ref")
                .and_then(|v| v.as_str())
                .unwrap_or("v1.0.1-dist")
                .into(),
            entry_scene: proj
                .get("entry-scene")
                .and_then(|v| v.as_str())
                .unwrap_or("Content/Scenes/Main.rxscene")
                .into(),
            content_dir: dirs
                .and_then(|d| d.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or("Content")
                .into(),
            scripts_dir: dirs
                .and_then(|d| d.get("scripts"))
                .and_then(|v| v.as_str())
                .unwrap_or("Content/Scripts")
                .into(),
        })
    }

    /// 无 forge.toml 时用缺省。
    pub fn with_defaults(root: PathBuf) -> Self {
        ForgeProject {
            root,
            name: "untitled".into(),
            engine_version: "0.1.0".into(),
            rurix_ref: "v1.0.1-dist".into(),
            entry_scene: "Content/Scenes/Main.rxscene".into(),
            content_dir: "Content".into(),
            scripts_dir: "Content/Scripts".into(),
        }
    }

    pub fn content_root(&self) -> PathBuf {
        self.root.join(&self.content_dir)
    }

    pub fn cache_root(&self) -> PathBuf {
        self.root.join(".forge").join("cache")
    }

    pub fn tmp_root(&self) -> PathBuf {
        self.root.join(".forge").join("tmp")
    }

    /// 确保目录结构存在(创建空目录树)。
    pub fn ensure_dirs(&self) -> Result<()> {
        let content = self.content_root();
        let cache = self.cache_root();
        let tmp = self.tmp_root();
        for sub in ["Meshes", "Textures", "Materials", "Prefabs", "Scenes", "Scripts", "Audio"] {
            std::fs::create_dir_all(content.join(sub))?;
        }
        std::fs::create_dir_all(cache.join("rxmesh"))?;
        std::fs::create_dir_all(cache.join("thumbs"))?;
        std::fs::create_dir_all(tmp)?;
        Ok(())
    }

    /// 扫描 Content 全部资产(含子目录),返回相对正斜杠路径列表。
    pub fn scan_content(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let cr = self.content_root();
        Self::scan_dir(&cr, &cr, &mut out)?;
        Ok(out)
    }

    fn scan_dir(abs: &Path, content_root: &Path, out: &mut Vec<String>) -> Result<()> {
        if !abs.is_dir() {
            return Ok(());
        }
        for ent in std::fs::read_dir(abs)? {
            let ent = ent?;
            let p = ent.path();
            let rel = p.strip_prefix(content_root).unwrap_or(&p);
            let rel_s = rel.to_string_lossy().replace('\\', "/");
            if p.is_dir() {
                Self::scan_dir(&p, content_root, out)?;
            } else if rel_s.ends_with(".meta") {
                continue; // .meta 不算资产本身
            } else {
                out.push(rel_s);
            }
        }
        Ok(())
    }
}
