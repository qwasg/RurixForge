//! keystore(12 §5 R-5):<workspace>/data/keystore.json,形态 {"keys":{"<backendId>":"<key>"}}。
//! env FORGE_GEN_API_KEY 优先于文件(D-F5-C)。
//! 红线:本类型【不提供任何序列化出口】(无 Serialize/Display;Debug 只列条目 id),
//! 密钥值仅能经 key_for 取出供 remote 适配器组装 Authorization 头,永不进工具返回/日志/事件。

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::data_dir;

/// keystore.json 默认路径。
pub fn keystore_path() -> PathBuf {
    data_dir().join("keystore.json")
}

pub struct Keystore {
    keys: HashMap<String, String>,
}

impl Keystore {
    /// 从默认路径加载;文件缺失 = 空 keystore(不算错误)。
    pub fn load() -> Self {
        Self::load_from(&keystore_path())
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        let keys = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("keys").cloned())
            .and_then(|v| serde_json::from_value::<HashMap<String, String>>(v).ok())
            .unwrap_or_default();
        Keystore { keys }
    }

    /// 取后端密钥:env FORGE_GEN_API_KEY 优先 → 文件条目。返回值仅用于 HTTP 头组装。
    pub fn key_for(&self, backend_id: &str) -> Option<String> {
        if let Ok(v) = std::env::var("FORGE_GEN_API_KEY") {
            if !v.is_empty() {
                return Some(v);
            }
        }
        self.keys.get(backend_id).cloned()
    }
}

/// 写入/更新单个密钥条目(F5 wave.3 agentd configure REST 用):读-改-写 keystore.json,
/// 保留其他条目;空 key 拒绝(调用方只在 apiKey 非空时调)。本函数是唯一写盘出口,
/// 密钥值不进日志,错误仅带 IO 信息。
pub fn set_key(backend_id: &str, key: &str) -> std::io::Result<()> {
    if backend_id.is_empty() || key.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "backend_id/key 不可空",
        ));
    }
    let path = keystore_path();
    let mut doc: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if doc.get("keys").and_then(|k| k.as_object()).is_none() {
        doc["keys"] = serde_json::json!({});
    }
    doc["keys"][backend_id] = serde_json::Value::String(key.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&doc)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

/// Debug 只暴露条目 id 清单,值一律脱敏(R-5)。
impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ids: Vec<&str> = self.keys.keys().map(String::as_str).collect();
        ids.sort();
        write!(f, "Keystore(entries={ids:?})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TEST_ENV_LOCK as ENV_LOCK;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("gend-ks-{}", std::process::id()));
        let ks = Keystore::load_from(&dir.join("no-such.json"));
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        assert!(ks.key_for("remote-openai-compatible").is_none());
    }

    #[test]
    fn env_overrides_file() {
        let _g = env_lock();
        let dir = std::env::temp_dir().join(format!("gend-ks2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("keystore.json");
        std::fs::write(&p, r#"{"keys":{"remote-openai-compatible":"file-key"}}"#).unwrap();
        let ks = Keystore::load_from(&p);
        std::env::set_var("FORGE_GEN_API_KEY", "env-key");
        assert_eq!(ks.key_for("remote-openai-compatible").as_deref(), Some("env-key"));
        std::env::remove_var("FORGE_GEN_API_KEY");
        assert_eq!(ks.key_for("remote-openai-compatible").as_deref(), Some("file-key"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn debug_redacts_values() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        let dir = std::env::temp_dir().join(format!("gend-ks3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("keystore.json");
        std::fs::write(&p, r#"{"keys":{"b1":"sk-SECRET-xyz"}}"#).unwrap();
        let ks = Keystore::load_from(&p);
        let dbg = format!("{ks:?}");
        assert!(!dbg.contains("sk-SECRET-xyz"), "Debug 泄漏密钥: {dbg}");
        assert!(dbg.contains("b1"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
