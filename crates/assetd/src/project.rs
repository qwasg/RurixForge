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

/// forge.toml `[render].backend`(02 §7.2)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderBackendKind {
    #[default]
    Rurix,
    Godot,
}

/// `[render].method`(仅 godot 生效)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMethod {
    ForwardPlus,
    Mobile,
    GlCompatibility,
}

/// `[render].driver`(仅 godot 生效)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderDriver {
    D3d12,
    Vulkan,
    Opengl3,
}

/// `[render]` 段。rurix 下 method / driver 恒 None;godot 下恒 Some(已补缺省)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderConfig {
    pub backend: RenderBackendKind,
    pub method: Option<RenderMethod>,
    pub driver: Option<RenderDriver>,
}

fn render_err(msg: String) -> AssetError {
    AssetError::new("PARSE_ERR", format!("forge.toml [render] {msg}"))
}

impl RenderConfig {
    /// 项目维度的缺省技术栈：2D 用 Godot；3D 保留 rurix，支持显式选 Godot。
    pub fn for_mode(mode: GameMode, backend: Option<&str>) -> Result<Self> {
        let default_backend = match mode {
            GameMode::TwoD => "godot",
            GameMode::ThreeD => "rurix",
        };
        Self::from_parts(Some(backend.unwrap_or(default_backend)), None, None).map(|(cfg, _)| cfg)
    }

    /// §7.2 的取值规则;toml 与 env(FORGE_RENDER_*,§7.3)共用。返回 (配置, 警告)。
    /// - backend 缺省 rurix;非法值 PARSE_ERR;
    /// - rurix 却写了 method / driver:接受并忽略(警告),改一行 backend 就能来回切;
    /// - godot 缺 method → forward_plus;缺 driver → forward_plus / mobile 用 d3d12,gl_compatibility 用 opengl3;
    /// - 只允许 {forward_plus, mobile} × {d3d12, vulkan} 与 gl_compatibility × opengl3。
    pub fn from_parts(backend: Option<&str>, method: Option<&str>, driver: Option<&str>) -> Result<(Self, Vec<String>)> {
        let mut warnings = Vec::new();
        let backend = match backend.unwrap_or("rurix") {
            "rurix" => RenderBackendKind::Rurix,
            "godot" => RenderBackendKind::Godot,
            s => return Err(render_err(format!("backend 须为 \"rurix\"|\"godot\",实际 {s:?}"))),
        };
        if backend == RenderBackendKind::Rurix {
            if method.is_some() || driver.is_some() {
                warnings.push("forge.toml [render] backend = \"rurix\":忽略 method / driver(只对 godot 生效)".to_string());
            }
            return Ok((RenderConfig::default(), warnings));
        }
        let method = match method.unwrap_or("forward_plus") {
            "forward_plus" => RenderMethod::ForwardPlus,
            "mobile" => RenderMethod::Mobile,
            "gl_compatibility" => RenderMethod::GlCompatibility,
            s => return Err(render_err(format!("method 须为 \"forward_plus\"|\"mobile\"|\"gl_compatibility\",实际 {s:?}"))),
        };
        let default_driver = if method == RenderMethod::GlCompatibility { "opengl3" } else { "d3d12" };
        let driver = match driver.unwrap_or(default_driver) {
            "d3d12" => RenderDriver::D3d12,
            "vulkan" => RenderDriver::Vulkan,
            "opengl3" => RenderDriver::Opengl3,
            s => return Err(render_err(format!("driver 须为 \"d3d12\"|\"vulkan\"|\"opengl3\",实际 {s:?}"))),
        };
        let ok = match method {
            RenderMethod::GlCompatibility => driver == RenderDriver::Opengl3,
            _ => driver != RenderDriver::Opengl3,
        };
        if !ok {
            let (_, m, d) = RenderConfig { backend, method: Some(method), driver: Some(driver) }.as_strs();
            return Err(render_err(format!(
                "method {:?} 不能配 driver {:?}(RD 驱动与 GLES3 不能混用)",
                m.unwrap_or_default(),
                d.unwrap_or_default()
            )));
        }
        Ok((RenderConfig { backend, method: Some(method), driver: Some(driver) }, warnings))
    }

    /// 独立解析 `[render]` 表，缺省保留 rurix；项目加载须用 parse_for_mode。
    pub fn parse(table: Option<&std::collections::BTreeMap<String, rurix_pkg::toml::Value>>) -> Result<(Self, Vec<String>)> {
        Self::parse_for_mode(table, GameMode::ThreeD)
    }

    /// 根据项目 mode 补齐缺省后端，显式 backend 始终优先。
    pub fn parse_for_mode(table: Option<&std::collections::BTreeMap<String, rurix_pkg::toml::Value>>, mode: GameMode) -> Result<(Self, Vec<String>)> {
        let default_backend = Self::for_mode(mode, None)?.as_strs().0;
        let Some(t) = table else { return Ok((Self::for_mode(mode, None)?, Vec::new())) };
        let get = |k: &str| -> Result<Option<&str>> {
            match t.get(k) {
                None => Ok(None),
                Some(v) => v.as_str().map(Some).ok_or_else(|| render_err(format!("{k} 须为字符串"))),
            }
        };
        let (cfg, mut warnings) = RenderConfig::from_parts(Some(get("backend")?.unwrap_or(default_backend)), get("method")?, get("driver")?)?;
        for k in t.keys().filter(|k| !matches!(k.as_str(), "backend" | "method" | "driver")) {
            warnings.push(format!("forge.toml [render]:忽略未知键 {k:?}"));
        }
        Ok((cfg, warnings))
    }

    /// (backend, method, driver) 的 forge.toml 取值。
    pub fn as_strs(&self) -> (&'static str, Option<&'static str>, Option<&'static str>) {
        let b = match self.backend {
            RenderBackendKind::Rurix => "rurix",
            RenderBackendKind::Godot => "godot",
        };
        let m = self.method.map(|m| match m {
            RenderMethod::ForwardPlus => "forward_plus",
            RenderMethod::Mobile => "mobile",
            RenderMethod::GlCompatibility => "gl_compatibility",
        });
        let d = self.driver.map(|d| match d {
            RenderDriver::D3d12 => "d3d12",
            RenderDriver::Vulkan => "vulkan",
            RenderDriver::Opengl3 => "opengl3",
        });
        (b, m, d)
    }

    /// 非缺省时的 `[render]` 段文本(to_toml 用);缺省返回 None。
    pub fn to_toml(&self) -> Option<String> {
        if *self == RenderConfig::default() {
            return None;
        }
        let (b, m, d) = self.as_strs();
        let mut s = format!("[render]\nbackend = {b:?}\n");
        if let Some(m) = m {
            s.push_str(&format!("method = {m:?}\n"));
        }
        if let Some(d) = d {
            s.push_str(&format!("driver = {d:?}\n"));
        }
        Some(s)
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
    /// [render] 段：2D 缺省 Godot，3D 缺省 rurix；显式配置优先。
    pub render: RenderConfig,
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
        let mode = proj
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
            .unwrap_or(GameMode::ThreeD);
        let render_table = match doc.get("render") {
            None => None,
            Some(v) => Some(v.as_table().ok_or_else(|| AssetError::new("PARSE_ERR", "forge.toml [render] 须为表"))?),
        };
        let (render, warnings) = RenderConfig::parse_for_mode(render_table, mode)?;
        for w in warnings {
            eprintln!("assetd: {w}");
        }
        Ok(ForgeProject {
            root: root.clone(),
            render,
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
            mode,
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
            render: RenderConfig::default(),
        }
    }

    /// 序列化为 forge.toml 文本(项目脚手架/设置回写用;[project] + [dirs] 两表)。
    /// Godot 与 2D 的显式 rurix 均写 [render]，以保证重载保留选定技术栈。
    pub fn to_toml(&self) -> String {
        let mut s = format!(
            "[project]\nname = {:?}\nengine-version = {:?}\nrurix-ref = {:?}\nentry-scene = {:?}\nmode = {:?}\n\n[dirs]\ncontent = {:?}\nscripts = {:?}\n",
            self.name,
            self.engine_version,
            self.rurix_ref,
            self.entry_scene,
            self.mode.as_str(),
            self.content_dir,
            self.scripts_dir,
        );
        if let Some(r) = self.render.to_toml() {
            s.push('\n');
            s.push_str(&r);
        } else if self.mode == GameMode::TwoD {
            // 2D 的 rurix 选择不能省略，否则重载会落回 Godot。
            s.push_str("\n[render]\nbackend = \"rurix\"\n");
        }
        s
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

    /// 02 §7.2 的缺省与校验表。
    #[test]
    fn render_config_rules_follow_02_7_2() {
        let p = |b, m, d| RenderConfig::from_parts(b, m, d).map(|(c, w)| (c.as_strs(), w.len()));
        assert_eq!(p(None, None, None).unwrap(), (("rurix", None, None), 0), "缺省 rurix");
        assert_eq!(p(Some("rurix"), Some("mobile"), None).unwrap(), (("rurix", None, None), 1), "rurix 忽略 method 并警告");
        assert_eq!(p(Some("godot"), None, None).unwrap(), (("godot", Some("forward_plus"), Some("d3d12")), 0));
        assert_eq!(p(Some("godot"), Some("mobile"), None).unwrap().0, ("godot", Some("mobile"), Some("d3d12")));
        assert_eq!(p(Some("godot"), Some("gl_compatibility"), None).unwrap().0, ("godot", Some("gl_compatibility"), Some("opengl3")));
        assert_eq!(p(Some("godot"), Some("forward_plus"), Some("vulkan")).unwrap().0, ("godot", Some("forward_plus"), Some("vulkan")));
        for (b, m, d) in [
            (Some("unity"), None, None),
            (Some("godot"), Some("deferred"), None),
            (Some("godot"), None, Some("metal")),
            (Some("godot"), Some("gl_compatibility"), Some("d3d12")),
            (Some("godot"), Some("mobile"), Some("opengl3")),
        ] {
            let e = RenderConfig::from_parts(b, m, d).unwrap_err();
            assert_eq!(e.code, "PARSE_ERR", "{b:?}/{m:?}/{d:?}");
        }
    }

    #[test]
    fn render_section_load_and_roundtrip() {
        let dir = std::env::temp_dir().join(format!("assetd-render-{}-{}", std::process::id(), forge_util::timeutil::unix_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        // 没有 [render] → rurix;缺省项目写出的字节不带 [render]。
        let p = ForgeProject::with_defaults(dir.clone());
        assert_eq!(p.render, RenderConfig::default());
        assert!(!p.to_toml().contains("[render]"));
        std::fs::write(dir.join("forge.toml"), "[project]\nname = \"g\"\n\n[render]\nbackend = \"godot\"\nmethod = \"mobile\"\nextra = \"x\"\n").unwrap();
        let loaded = ForgeProject::load(&dir).unwrap();
        assert_eq!(loaded.render.as_strs(), ("godot", Some("mobile"), Some("d3d12")));
        loaded.save_manifest().unwrap();
        let again = ForgeProject::load(&dir).unwrap();
        assert_eq!(again.render, loaded.render, "to_toml 保留 [render]");
        std::fs::write(dir.join("forge.toml"), "[project]\n\n[render]\nbackend = 3\n").unwrap();
        assert!(ForgeProject::load(&dir).is_err(), "backend 不是字符串");
        std::fs::write(dir.join("forge.toml"), "[project]\nrender = \"godot\"\n").unwrap();
        assert_eq!(ForgeProject::load(&dir).unwrap().render, RenderConfig::default(), "[project] 里的同名键不算 [render] 段");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_mode_defaults_and_explicit_backends_roundtrip() {
        let dir = std::env::temp_dir().join(format!("assetd-mode-backend-{}-{}", std::process::id(), forge_util::timeutil::unix_millis()));
        std::fs::create_dir_all(&dir).unwrap();
        for (manifest, backend) in [
            ("[project]\nmode = \"2d\"\n", "godot"),
            ("[project]\nmode = \"3d\"\n", "rurix"),
            ("[project]\nmode = \"2d\"\n\n[render]\nmethod = \"mobile\"\n", "godot"),
            ("[project]\nmode = \"2d\"\n\n[render]\nbackend = \"rurix\"\n", "rurix"),
            ("[project]\nmode = \"3d\"\n\n[render]\nbackend = \"godot\"\n", "godot"),
        ] {
            std::fs::write(dir.join("forge.toml"), manifest).unwrap();
            let loaded = ForgeProject::load(&dir).unwrap();
            assert_eq!(loaded.render.as_strs().0, backend, "{manifest}");
            loaded.save_manifest().unwrap();
            assert_eq!(ForgeProject::load(&dir).unwrap().render, loaded.render, "保存后必须保留后端选择");
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
