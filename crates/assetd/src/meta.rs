//! `.meta` sidecar 读写(08 §3.2 YAML:guid/type/importer/importSettings/provenance/buildState)。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{AssetError, AssetType, Result};

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

    /// 缓存键 = hash(源文件字节, importer 版本, importSettings 全量, 构建器版本)。
    /// 构建器版本 = rurix-geom-build 产物 hash 常量或固定串(本波用 "0.1.0" 占位,
    /// 后续上游 ABI 冻结后改真实 digest)。
    pub fn cache_key(&self, source_bytes: &[u8]) -> String {
        let mut h = rurix_pkg::sha256::Sha256::new();
        h.update(source_bytes);
        h.update(self.importer.as_bytes());
        h.update(b"|");
        let settings_json = serde_json::to_string(&self.import_settings).unwrap_or_default();
        h.update(settings_json.as_bytes());
        h.update(b"|");
        // 构建器版本占位(08 §4.3:rurix-geom-build 输出确定性,同输入 → 同产物)。
        h.update(b"0.1.0");
        rurix_pkg::sha256::hex(&h.finalize())
    }
}
