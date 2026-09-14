//! gen 后端配置:<workspace>/data/gen-backends.json(D-F5-A:条目缺失 = configured=false)。
//!
//! 形态:`{"backends":[{"id":"local-mock","kind":"local","enabled":true},
//!   {"id":"remote-openai-compatible","kind":"remote","enabled":true,
//!    "endpoint":"https://...","model":"..."}]}`。
//! 引擎级配置(非项目级);测试用 env FORGE_GEN_DATA_DIR 覆盖数据目录。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 数据目录:env FORGE_GEN_DATA_DIR 优先(测试隔离),否则 <workspace>/data。
pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("FORGE_GEN_DATA_DIR") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    workspace_root().join("data")
}

/// workspace 根(CARGO_MANIFEST_DIR = crates/gend,上两级)。
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR 应有上两级(workspace 根)")
        .to_path_buf()
}

/// gen-backends.json 默认路径。
pub fn config_path() -> PathBuf {
    data_dir().join("gen-backends.json")
}

/// 单个后端条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendEntry {
    pub id: String,
    /// local | remote。
    pub kind: String,
    #[serde(default)]
    pub enabled: bool,
    /// remote 类端点(如 https://api.example.com;调用时拼 /v1/images/generations)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// gen-backends.json 文档。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenConfig {
    #[serde(default)]
    pub backends: Vec<BackendEntry>,
}

impl GenConfig {
    /// 从默认路径加载;文件缺失 = 空配置(全后端 configured=false,诚实缺省 I-5);
    /// 解析失败按空配置处理并 stderr 如实提示(不静默伪装成「用户没配」之外的态)。
    pub fn load() -> Self {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                eprintln!("[gend] gen-backends.json 解析失败({e}),按空配置处理");
                GenConfig::default()
            }),
            Err(_) => GenConfig::default(),
        }
    }

    pub fn entry(&self, id: &str) -> Option<&BackendEntry> {
        self.backends.iter().find(|b| b.id == id)
    }

    /// 写回默认路径(F5 wave.3 agentd configure REST 用;目录不存在则创建)。
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&config_path())
    }

    pub fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, text)
    }

    /// 读-改-写单条目(保留其他条目与条目内未触及字段,如 model)。
    pub fn upsert_entry(&mut self, entry: BackendEntry) {
        match self.backends.iter_mut().find(|b| b.id == entry.id) {
            Some(e) => {
                e.kind = entry.kind;
                e.enabled = entry.enabled;
                if entry.endpoint.is_some() {
                    e.endpoint = entry.endpoint;
                }
                if entry.model.is_some() {
                    e.model = entry.model;
                }
            }
            None => self.backends.push(entry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty_config() {
        let dir = std::env::temp_dir().join(format!("gend-cfg-{}", std::process::id()));
        let cfg = GenConfig::load_from(&dir.join("no-such.json"));
        assert!(cfg.backends.is_empty());
        assert!(cfg.entry("local-mock").is_none());
    }

    #[test]
    fn parses_entries() {
        let dir = std::env::temp_dir().join(format!("gend-cfg2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("gen-backends.json");
        std::fs::write(
            &p,
            r#"{"backends":[{"id":"local-mock","kind":"local","enabled":true},
                {"id":"remote-openai-compatible","kind":"remote","enabled":false,"endpoint":"https://x"}]}"#,
        )
        .unwrap();
        let cfg = GenConfig::load_from(&p);
        assert_eq!(cfg.backends.len(), 2);
        let e = cfg.entry("local-mock").unwrap();
        assert!(e.enabled);
        let r = cfg.entry("remote-openai-compatible").unwrap();
        assert!(!r.enabled);
        assert_eq!(r.endpoint.as_deref(), Some("https://x"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
