//! 项目布局:forge.toml + Content/ + .forge/cache/ + .forge/tmp/ (08 §3.1)。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{AssetError, Result};

/// 游戏维度模式(F-GAME-3:项目选型,forge.toml [project] mode = "2d"|"3d")。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMode {
    /// 2D:XY 平面侧视约定(正交相机朝 -Z,重力 -Y,Sprite 精灵)。
    TwoD,
    /// 3D:自由三维(缺省,旧项目无 mode 字段时回退此值)。
    ThreeD,
}

impl GameMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            GameMode::TwoD => "2d",
            GameMode::ThreeD => "3d",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "2d" => Some(GameMode::TwoD),
            "3d" => Some(GameMode::ThreeD),
            _ => None,
        }
    }
}

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
    /// 游戏维度模式(缺省 ThreeD)。
    pub mode: GameMode,
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
            mode: proj
                .get("mode")
                .and_then(|v| v.as_str())
                .map(|s| {
                    GameMode::parse(s).ok_or_else(|| {
                        AssetError::new(
                            "PARSE_ERR",
                            format!("forge.toml [project] mode 须为 \"2d\"|\"3d\",实际 {s:?}"),
                        )
                    })
                })
                .transpose()?
                .unwrap_or(GameMode::ThreeD),
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
            mode: GameMode::ThreeD,
        }
    }

    /// 序列化为 forge.toml 文本(项目脚手架/设置回写用;[project] + [dirs] 两表)。
    pub fn to_toml(&self) -> String {
        format!(
            "[project]\nname = {:?}\nengine-version = {:?}\nrurix-ref = {:?}\nentry-scene = {:?}\nmode = {:?}\n\n[dirs]\ncontent = {:?}\nscripts = {:?}\n",
            self.name,
            self.engine_version,
            self.rurix_ref,
            self.entry_scene,
            self.mode.as_str(),
            self.content_dir,
            self.scripts_dir,
        )
    }

    /// 写 forge.toml 到项目根(已存在则覆盖——调用方负责确认时机)。
    pub fn save_manifest(&self) -> Result<()> {
        std::fs::write(self.root.join("forge.toml"), self.to_toml())?;
        Ok(())
    }

    pub fn content_root(&self) -> PathBuf {
        self.root.join(&self.content_dir)
    }

    /// 相对 Content/ 的用户路径 → 根内绝对路径。`..` / UNC / 盘符 / junction 逃逸一律拒绝。
    pub fn resolve_content_path(&self, rel: &str) -> Result<PathBuf> {
        let root = self.content_root();
        forge_util::pathutil::confine_under(&[&root], rel)
            .map_err(|e| AssetError::new("PROJECT_OUT_OF_ROOT", format!("{e}: {rel}")))
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
        for sub in ["Meshes", "Models", "Textures", "Materials", "Prefabs", "Scenes", "Scripts", "Audio", "Sprites"] {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_content_path_rejects_traversal() {
        let dir = std::env::temp_dir().join(format!(
            "assetd-confine-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(dir.join("Content").join("Textures")).unwrap();
        std::fs::write(dir.join("Content").join("Textures").join("a.png"), b"x").unwrap();
        let p = ForgeProject::with_defaults(dir.clone());
        assert!(p.resolve_content_path("Textures/a.png").is_ok());
        assert!(p.resolve_content_path("../secret.txt").is_err());
        assert!(p.resolve_content_path("C:/Windows/notepad.exe").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn game_mode_parse_and_default() {
        assert_eq!(GameMode::parse("2d"), Some(GameMode::TwoD));
        assert_eq!(GameMode::parse("3d"), Some(GameMode::ThreeD));
        assert_eq!(GameMode::parse("2D"), None);
        assert_eq!(GameMode::parse(""), None);
        // 无 forge.toml → 缺省 3d。
        let p = ForgeProject::with_defaults(PathBuf::from("x"));
        assert_eq!(p.mode, GameMode::ThreeD);
    }

    #[test]
    fn manifest_roundtrip_with_mode() {
        let dir = std::env::temp_dir().join(format!(
            "assetd-mode-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // 2d 模式落盘 → 重载回读一致。
        let mut p = ForgeProject::with_defaults(dir.clone());
        p.name = "demo2d".into();
        p.mode = GameMode::TwoD;
        p.save_manifest().unwrap();
        let text = std::fs::read_to_string(dir.join("forge.toml")).unwrap();
        assert!(text.contains("mode = \"2d\""), "forge.toml 须含 mode:{text}");
        let loaded = ForgeProject::load(&dir).unwrap();
        assert_eq!(loaded.mode, GameMode::TwoD);
        assert_eq!(loaded.name, "demo2d");
        // 非法 mode 如实报错。
        std::fs::write(dir.join("forge.toml"), "[project]\nmode = \"5d\"\n").unwrap();
        assert!(ForgeProject::load(&dir).is_err());
        // 无 mode 字段的旧 forge.toml → 3d。
        std::fs::write(dir.join("forge.toml"), "[project]\nname = \"legacy\"\n").unwrap();
        assert_eq!(ForgeProject::load(&dir).unwrap().mode, GameMode::ThreeD);
        std::fs::remove_dir_all(&dir).ok();
    }
}
