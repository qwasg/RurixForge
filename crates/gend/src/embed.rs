//! F10 embedding 渠道(RAG 向量档;D-F8-C openai-compat 同族纪律):
//! 配置面 baseUrl+model 落 data/llm-embedding.json(读-改-写 Mutex 原子写),
//! key 走 keystore["embedding"](R-5:只进 Authorization 头,永不进 JSON/日志/响应)。
//! 调用面 POST {baseUrl}/v1/embeddings {model, input:[...]} → data[*].embedding。
//! 未配齐 = resolve_embedder() → None(调用方显式 EMBEDDING_NOT_CONFIGURED,不静默回落)。

use std::io::Read;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::data_dir;

/// keystore 条目 id。
pub const EMBEDDING_KEYSTORE_ID: &str = "embedding";
/// 未配置显式错误码(OPENAI_COMPAT_NOT_CONFIGURED 同族)。
pub const EMBEDDING_NOT_CONFIGURED: &str = "EMBEDDING_NOT_CONFIGURED";
/// 单请求超时(批量 32 条文本,给足余量)。
const HTTP_TIMEOUT_SECS: u64 = 120;
/// 响应体上限(64MB;批量 embedding JSON 远小于此)。
const BODY_MAX: u64 = 64 * 1024 * 1024;

/// 配置文件(key 永不进本文件)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingFile {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
}

/// 读-改-写串行锁(同 llm-openai-compat.json 纪律)。
static EMBEDDING_FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn embedding_config_path() -> PathBuf {
    data_dir().join("llm-embedding.json")
}

/// 读配置;缺失/解析失败按未配置处理(stderr 如实,不含机密)。
pub fn load_embedding_file() -> EmbeddingFile {
    let path = embedding_config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("llm-embedding.json 解析失败({}): {e},按未配置处理", path.display());
            EmbeddingFile::default()
        }),
        Err(_) => EmbeddingFile::default(),
    }
}

/// 原子写(持锁 + tmp/rename)。
pub fn save_embedding_file(cfg: &EmbeddingFile) -> std::io::Result<()> {
    let _g = EMBEDDING_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = embedding_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

/// keystore["embedding"] 密钥面(key_for 语义含 FORGE_GEN_API_KEY dev-key 覆盖,如实)。
pub fn embedding_key() -> Option<String> {
    let ks = crate::keystore::Keystore::load();
    match ks.key_for(EMBEDDING_KEYSTORE_ID) {
        Some(k) if !k.is_empty() => Some(k),
        _ => None,
    }
}

/// 状态面(REST status 同源;绝无 key)。
#[derive(Debug, Clone)]
pub struct EmbeddingStatus {
    pub configured: bool,
    pub base_url: String,
    pub model: String,
    pub key_configured: bool,
}

pub fn embedding_status() -> EmbeddingStatus {
    let file = load_embedding_file();
    let key_configured = embedding_key().is_some();
    let configured = !file.base_url.is_empty() && !file.model.is_empty() && key_configured;
    EmbeddingStatus {
        configured,
        base_url: file.base_url,
        model: file.model,
        key_configured,
    }
}

/// 三联解析:自配渠道全齐 → Some(RemoteEmbedder);未配齐 → 回落云账号 embedding
/// ([resolve_cloud_embedder]);两者皆无 → None(调用方显式 NOT_CONFIGURED)。
pub fn resolve_embedder() -> Option<RemoteEmbedder> {
    let file = load_embedding_file();
    if !file.base_url.is_empty() && !file.model.is_empty() {
        if let Some(key) = embedding_key() {
            return Some(RemoteEmbedder {
                base_url: file.base_url,
                model: file.model,
                key,
            });
        }
    }
    resolve_cloud_embedder()
}

/// 云账号 embedding 描述文件名(非密;forge-agentd 登录云账号且模型目录里有 embedding
/// 模型时写入 data 目录,登出即删)。MCP 子进程与 agentd 共用同一 data 目录,每次解析现读,
/// 登录/登出无需重启子进程即生效。
pub const CLOUD_EMBEDDING_FILE: &str = "cloud-embedding.json";
/// forge-agentd 云模式设备 Key 的 keystore 条目 id(经 secret_for 读,不受 FORGE_GEN_API_KEY 覆盖)。
pub const CLOUD_DEVICE_KEY_ID: &str = "cloud:device-key";

/// 云账号 embedding 描述:请求 `{base_url}/v1/embeddings`,Key 走 keystore[CLOUD_DEVICE_KEY_ID]。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CloudEmbeddingFile {
    pub base_url: String,
    pub model: String,
}

/// 写云账号 embedding 描述(tmp + rename)。
pub fn save_cloud_embedding_in(dir: &std::path::Path, file: &CloudEmbeddingFile) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(CLOUD_EMBEDDING_FILE);
    let text = serde_json::to_string_pretty(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

/// 删云账号 embedding 描述(文件不存在不算错)。
pub fn clear_cloud_embedding_in(dir: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_file(dir.join(CLOUD_EMBEDDING_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// 云账号 embedding:描述文件齐全且 keystore 有设备 Key → Some。
pub fn resolve_cloud_embedder() -> Option<RemoteEmbedder> {
    let text = std::fs::read_to_string(data_dir().join(CLOUD_EMBEDDING_FILE)).ok()?;
    let file: CloudEmbeddingFile = serde_json::from_str(&text).ok()?;
    if file.base_url.is_empty() || file.model.is_empty() {
        return None;
    }
    let key = crate::keystore::Keystore::load().secret_for(CLOUD_DEVICE_KEY_ID)?;
    Some(RemoteEmbedder {
        base_url: file.base_url,
        model: file.model,
        key,
    })
}

/// 远程 embedding 客户端(阻塞 HTTP;异步侧调用方自套 spawn_blocking)。
#[derive(Clone)]
pub struct RemoteEmbedder {
    pub base_url: String,
    pub model: String,
    key: String,
}

/// Debug 脱敏(R-5)。
impl std::fmt::Debug for RemoteEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteEmbedder")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("key", &"<redacted>")
            .finish()
    }
}

impl RemoteEmbedder {
    /// 测试用直构。
    pub fn new(base_url: impl Into<String>, model: impl Into<String>, key: impl Into<String>) -> Self {
        RemoteEmbedder { base_url: base_url.into(), model: model.into(), key: key.into() }
    }

    /// 批量嵌入:POST {baseUrl}/v1/embeddings。错误消息只带 HTTP 状态/上游 error.message,
    /// 不回显请求体与头(R-5)。返回按 data[*].index 排序对齐输入序。
    pub fn embed_batch(&self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/v1/embeddings", self.base_url.trim_end_matches('/'));
        let body = json!({ "model": self.model, "input": texts });
        let agent = ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build();
        let resp = agent
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.key))
            .set("Content-Type", "application/json")
            .send_string(&body.to_string());
        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(code, r)) => {
                let detail = read_body(r)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                    .and_then(|v| {
                        v.get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "无详情".to_string());
                return Err(format!("embedding HTTP {code}: {detail}"));
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(format!("embedding 连接失败: {t}"));
            }
        };
        let bytes = read_body(resp).map_err(|e| format!("读 embedding 响应体失败: {e}"))?;
        let v: Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("embedding 响应非 JSON: {e}"))?;
        let data = v
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| "embedding 响应缺 data 数组".to_string())?;
        if data.len() != texts.len() {
            return Err(format!(
                "embedding 返回条数不匹配:期望 {} 实得 {}",
                texts.len(),
                data.len()
            ));
        }
        // 按 index 对齐输入序(上游可能乱序)。
        let mut out: Vec<(usize, Vec<f32>)> = Vec::with_capacity(data.len());
        for (fallback_idx, item) in data.iter().enumerate() {
            let idx = item
                .get("index")
                .and_then(Value::as_u64)
                .map(|i| i as usize)
                .unwrap_or(fallback_idx);
            let emb = item
                .get("embedding")
                .and_then(Value::as_array)
                .ok_or_else(|| "embedding 条目缺 embedding 数组".to_string())?;
            let vec: Vec<f32> = emb
                .iter()
                .map(|x| x.as_f64().unwrap_or(0.0) as f32)
                .collect();
            if vec.is_empty() {
                return Err("embedding 返回空向量".to_string());
            }
            out.push((idx, vec));
        }
        out.sort_by_key(|(i, _)| *i);
        Ok(out.into_iter().map(|(_, v)| v).collect())
    }
}

fn read_body(resp: ureq::Response) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    resp.into_reader().take(BODY_MAX).read_to_end(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TEST_ENV_LOCK as ENV_LOCK;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 隔离数据目录守卫(FORGE_GEN_DATA_DIR)。
    struct DirGuard {
        dir: PathBuf,
    }
    impl DirGuard {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "gend-embed-{tag}-{}-{}",
                std::process::id(),
                forge_util::timeutil::unix_millis()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
            DirGuard { dir }
        }
    }
    impl Drop for DirGuard {
        fn drop(&mut self) {
            std::env::remove_var("FORGE_GEN_DATA_DIR");
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    #[test]
    fn config_roundtrip_never_contains_key() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        let guard = DirGuard::new("cfg");
        // 未配置前置。
        let st = embedding_status();
        assert!(!st.configured && !st.key_configured);
        assert!(resolve_embedder().is_none());
        // 写配置 + key。
        save_embedding_file(&EmbeddingFile {
            base_url: "http://127.0.0.1:9300".into(),
            model: "bge-m3".into(),
        })
        .unwrap();
        let secret = "sk-embed-REDLINE-test";
        crate::keystore::set_key(EMBEDDING_KEYSTORE_ID, secret).unwrap();
        let st = embedding_status();
        assert!(st.configured && st.key_configured);
        assert_eq!(st.base_url, "http://127.0.0.1:9300");
        assert_eq!(st.model, "bge-m3");
        // 配置 JSON 绝不含 key(R-5)。
        let text = std::fs::read_to_string(guard.dir.join("llm-embedding.json")).unwrap();
        assert!(!text.contains(secret), "配置 JSON 落密钥(R-5): {text}");
        // resolve 三联。
        let e = resolve_embedder().expect("已配齐应 Some");
        assert_eq!(e.base_url, "http://127.0.0.1:9300");
        assert_eq!(e.model, "bge-m3");
        // Debug 脱敏。
        let dbg = format!("{e:?}");
        assert!(!dbg.contains(secret), "Debug 泄漏密钥: {dbg}");
    }

    #[test]
    fn cloud_embedding_is_fallback_only() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        let guard = DirGuard::new("cloud");
        assert!(resolve_embedder().is_none());
        save_cloud_embedding_in(
            &guard.dir,
            &CloudEmbeddingFile {
                base_url: "http://127.0.0.1:8110".into(),
                model: "text-embedding-3-small".into(),
            },
        )
        .unwrap();
        assert!(resolve_embedder().is_none(), "无设备 Key 不得回落云账号");
        crate::keystore::set_key(CLOUD_DEVICE_KEY_ID, "sk-rf-cloud-device").unwrap();
        let e = resolve_embedder().expect("云账号回落应生效");
        assert_eq!(e.base_url, "http://127.0.0.1:8110");
        assert_eq!(e.model, "text-embedding-3-small");
        // 自配渠道配齐后优先于云账号。
        save_embedding_file(&EmbeddingFile {
            base_url: "http://127.0.0.1:9300".into(),
            model: "bge-m3".into(),
        })
        .unwrap();
        crate::keystore::set_key(EMBEDDING_KEYSTORE_ID, "sk-byo").unwrap();
        assert_eq!(resolve_embedder().unwrap().model, "bge-m3");
        std::fs::remove_file(guard.dir.join("llm-embedding.json")).unwrap();
        clear_cloud_embedding_in(&guard.dir).unwrap();
        clear_cloud_embedding_in(&guard.dir).unwrap();
        assert!(resolve_embedder().is_none(), "描述删除后回落失效");
    }

    #[test]
    fn embed_batch_empty_input_no_network() {
        let e = RemoteEmbedder::new("http://127.0.0.1:1", "m", "k");
        assert!(e.embed_batch(&[]).unwrap().is_empty());
    }

    #[test]
    fn embed_batch_transport_error_message_has_no_key() {
        // 连接必失败端口;错误消息不得含密钥。
        let secret = "sk-secret-in-error-check";
        let e = RemoteEmbedder::new("http://127.0.0.1:9", "m", secret);
        let err = e.embed_batch(&["x".into()]).unwrap_err();
        assert!(err.contains("embedding"), "{err}");
        assert!(!err.contains(secret), "错误消息泄漏密钥: {err}");
    }
}
