//! assetd — Forge 资产守护库(08 §1)。
//!
//! 职责:项目布局(forge.toml/Content//.forge/cache/)、`.meta` sidecar(YAML,
//! GUID/type/importer/importSettings/provenance)、缓存键(SHA-256:源字节 +
//! importer 版本 + importSettings 全量 + 构建器版本)、导入(gltf/glb 经
//! rurix-asset;png/jpg 登记+尺寸)、网格构建(rurix-geom-build → RXGB `.rxmesh`)。
//! 本库不暴露网络端口(02 §3),被 asset-pipeline-mcp 内嵌。

pub mod build;
pub mod cleanup;
pub mod import;
pub mod inspect;
pub mod material;
pub mod meta;
pub mod model;
pub mod ops;
pub mod project;
pub mod refs;
pub mod sprite;
pub mod status;
pub mod texture;
pub mod thumb;

use std::fmt;
use std::path::{Path, PathBuf};

/// assetd 统一错误(结构化;工具层映射为 isError,不 panic)。
#[derive(Debug)]
pub struct AssetError {
    pub code: &'static str,
    pub message: String,
}

impl AssetError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        AssetError { code, message: message.into() }
    }
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for AssetError {}

impl From<std::io::Error> for AssetError {
    fn from(e: std::io::Error) -> Self {
        AssetError::new("IO", e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AssetError>;

/// 资产类型(08 §3.2 闭集;F-GAME-4 + Sprite,见 E-08-002)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetType {
    Mesh,
    Model,
    Texture,
    Material,
    Prefab,
    Scene,
    Script,
    Audio,
    /// 精灵图集定义 .rxsprite(F-GAME-4:帧 bbox + pivot + 动画 clip + animator)。
    Sprite,
}

impl AssetType {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetType::Mesh => "mesh",
            AssetType::Model => "model",
            AssetType::Texture => "texture",
            AssetType::Material => "material",
            AssetType::Prefab => "prefab",
            AssetType::Scene => "scene",
            AssetType::Script => "script",
            AssetType::Audio => "audio",
            AssetType::Sprite => "sprite",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "mesh" => AssetType::Mesh,
            "model" => AssetType::Model,
            "texture" => AssetType::Texture,
            "material" => AssetType::Material,
            "prefab" => AssetType::Prefab,
            "scene" => AssetType::Scene,
            "script" => AssetType::Script,
            "audio" => AssetType::Audio,
            "sprite" => AssetType::Sprite,
            _ => return None,
        })
    }

    /// 源文件扩展名 → 资产类型 + 导入器 id。
    pub fn from_extension(ext: &str) -> Option<(Self, &'static str)> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "gltf" | "glb" => (AssetType::Mesh, "gltf"),
            "rxmodel" => (AssetType::Model, "model"),
            "rxprefab" => (AssetType::Prefab, "prefab"),
            "png" => (AssetType::Texture, "png"),
            "jpg" | "jpeg" => (AssetType::Texture, "jpg"),
            "rxscene" => (AssetType::Scene, "scene"),
            "rxmat" => (AssetType::Material, "material"),
            "rx" => (AssetType::Script, "rx"),
            "rxgraph" => (AssetType::Script, "rxgraph"),
            "rxsprite" => (AssetType::Sprite, "sprite"),
            _ => return None,
        })
    }
}

/// 构建状态(08 §4.3 asset_build_status 报告值)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildState {
    Current,
    Stale,
    Building,
    Failed,
}

impl BuildState {
    pub fn as_str(self) -> &'static str {
        match self {
            BuildState::Current => "current",
            BuildState::Stale => "stale",
            BuildState::Building => "building",
            BuildState::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "current" => BuildState::Current,
            "stale" => BuildState::Stale,
            "building" => BuildState::Building,
            "failed" => BuildState::Failed,
            _ => return None,
        })
    }
}

/// 资产条目(asset_list 返回元)。
#[derive(Debug, Clone)]
pub struct AssetEntry {
    /// 相对 Content/ 的正斜杠路径。
    pub path: String,
    pub guid: String,
    pub atype: AssetType,
    pub size: u64,
    /// 依赖(引用)GUID 列表(引用图出边)。
    pub deps: Vec<String>,
    pub build_state: BuildState,
}

/// 规范化资产相对路径:统一正斜杠、去前导 `./`、拒绝越界。
pub fn normalize_rel(p: &str) -> Result<String> {
    let p = p.replace('\\', "/");
    let p = p.strip_prefix("./").unwrap_or(&p);
    if p.is_empty() || p.starts_with('/') || p.contains("..") || p.contains(':') {
        return Err(AssetError::new(
            "PROJECT_OUT_OF_ROOT",
            format!("资产路径越界或非法: {p}"),
        ));
    }
    Ok(p.to_string())
}

/// 资产路径 → .meta 路径(<file>.meta)。
pub fn meta_path_for(content_root: &Path, rel: &str) -> PathBuf {
    let mut p = content_root.join(rel);
    let ext = match p.extension().and_then(|e| e.to_str()) {
        Some(e) => format!("{e}.meta"),
        None => "meta".to_string(),
    };
    p.set_extension(ext);
    p
}

/// 新 GUID v4(连字符小写,与 08 §3.2 示例一致)。
pub fn new_guid() -> String {
    uuid::Uuid::new_v4().to_string()
}
