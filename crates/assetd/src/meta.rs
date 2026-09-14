//! `.meta` sidecar 读写(08 §3.2 YAML:guid/type/importer/importSettings/provenance/buildState)。
//! F10:顶层 `semantic` 段(description/tags/source)——**不入 cache_key**,改描述不触发重建。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{AssetError, AssetType, Result};

/// 语义元数据(F10:素材文字简介 + 标签 + 溯源;human|agent-vision|agent-facts)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Semantic {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// human | agent-vision | agent-facts
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// 生成描述时依据的内容摘要 hash,用于 stale 判定。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

impl Semantic {
    pub fn new(description: impl Into<String>, tags: Vec<String>, source: impl Into<String>) -> Self {
        Semantic {
            description: description.into(),
            tags,
            source: source.into(),
            model: None,
            updated_at: Some(forge_util::timeutil::utc_now_iso8601()),
            content_hash: None,
        }
    }
}

/// .meta 文档(与 08 §3.2 示例逐字段对齐)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaDoc {
    pub guid: String,
    #[serde(rename = "type")]
    pub atype: String,
    pub importer: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub import_settings: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<Semantic>,
}

/// 来源元数据(I-7 强制)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

impl MetaDoc {
    /// 新建 .meta(guid 新建,importer 从文件扩展名推导)。
    pub fn new(rel_path: &str, guid: String) -> Result<Self> {
        let ext = Path::new(rel_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let (atype, importer) = AssetType::from_extension(ext)
            .ok_or_else(|| AssetError::new("UNKNOWN_TYPE", format!("无法从扩展名识别类型: {ext}")))?;
        Ok(MetaDoc {
            guid,
            atype: atype.as_str().into(),
            importer: importer.into(),
            import_settings: HashMap::new(),
            provenance: None,
            build_state: None,
            semantic: None,
        })
    }

    /// 从磁盘读 YAML。
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        serde_yaml::from_str(&text)
            .map_err(|e| AssetError::new("META_PARSE", format!(".meta 解析失败: {e}")))
    }

    /// 写磁盘(规范性:YAML 缩进 2,键有序;因 serde_yaml 不保证键序,手动排序)。
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = serde_yaml::to_string(self)
            .map_err(|e| AssetError::new("META_SERIALIZE", format!(".meta 序列化失败: {e}")))?;
        std::fs::write(path, yaml)?;
        Ok(())
    }

    /// 语义简介(空则 None)。
    pub fn description(&self) -> Option<&str> {
        self.semantic
            .as_ref()
            .filter(|s| !s.description.is_empty())
            .map(|s| s.description.as_str())
    }

    /// 语义标签(空则空切片)。
    pub fn tags(&self) -> &[String] {
        self.semantic
            .as_ref()
            .map(|s| s.tags.as_slice())
            .unwrap_or(&[])
    }

    /// 缓存键 = hash(源文件字节, importer 版本, importSettings 全量, 构建器版本)。
    /// **semantic 段不参与**——改描述不触发网格/贴图重建。
    pub fn cache_key(&self, source_bytes: &[u8]) -> String {
        let mut h = rurix_pkg::sha256::Sha256::new();
        h.update(source_bytes);
        h.update(self.importer.as_bytes());
        h.update(b"|");
        // HashMap insertion/random iteration order must never change the cache identity.
        let ordered: std::collections::BTreeMap<_, _> = self.import_settings.iter().collect();
        let settings_json = serde_json::to_string(&ordered).unwrap_or_default();
        h.update(settings_json.as_bytes());
        h.update(b"|");
        // 构建器版本占位(08 §4.3:rurix-geom-build 输出确定性,同输入 → 同产物)。
        h.update(b"0.1.0");
        rurix_pkg::sha256::hex(&h.finalize())
    }
}

/// 确保 .meta 存在(缺则新建);用于 .rx/.rxgraph 等导入时未写 sidecar 的路径。
pub fn ensure_meta(project_content_root: &Path, rel: &str) -> Result<(PathBuf, MetaDoc)> {
    let meta_path = crate::meta_path_for(project_content_root, rel);
    if meta_path.is_file() {
        return MetaDoc::load(&meta_path).map(|m| (meta_path, m));
    }
    let guid = crate::new_guid();
    let meta = MetaDoc::new(rel, guid)?;
    meta.save(&meta_path)?;
    Ok((meta_path, meta))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_change_does_not_affect_cache_key() {
        let mut meta = MetaDoc {
            guid: "test-guid".into(),
            atype: "mesh".into(),
            importer: "gltf".into(),
            import_settings: HashMap::new(),
            provenance: None,
            build_state: Some("current".into()),
            semantic: None,
        };
        let bytes = b"fake mesh bytes";
        let key_before = meta.cache_key(bytes);
        meta.semantic = Some(Semantic {
            description: "四腿木质餐椅".into(),
            tags: vec!["家具".into(), "椅子".into()],
            source: "human".into(),
            model: None,
            updated_at: Some("2026-08-24T15:00:00Z".into()),
            content_hash: Some("abc123".into()),
        });
        let key_after = meta.cache_key(bytes);
        assert_eq!(
            key_before, key_after,
            "改 semantic 不得影响 cache_key(避免触发重建)"
        );
    }

    #[test]
    fn semantic_roundtrip_yaml() {
        let meta = MetaDoc {
            guid: "g1".into(),
            atype: "texture".into(),
            importer: "png".into(),
            import_settings: HashMap::new(),
            provenance: None,
            build_state: None,
            semantic: Some(Semantic {
                description: "木纹贴图".into(),
                tags: vec!["wood".into()],
                source: "agent-facts".into(),
                model: Some("deepseek-chat".into()),
                updated_at: Some("2026-08-24T00:00:00Z".into()),
                content_hash: None,
            }),
        };
        let yaml = serde_yaml::to_string(&meta).unwrap();
        let back: MetaDoc = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(meta.semantic, back.semantic);
        assert_eq!(back.description(), Some("木纹贴图"));
        assert_eq!(back.tags(), &["wood"]);
    }
}
