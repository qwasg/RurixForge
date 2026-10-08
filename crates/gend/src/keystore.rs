//! keystore(12 §5 R-5):<workspace>/data/keystore.json。
//! RD-F5-001(Windows 腿,D-RDG-D):落盘形态 DPAPI 加密——{"v":1,"dpapi":"<base64(CryptProtectData 密文)>"},
//! 明文域整体加密(每用户熵,无额外熵盐);旧明文形态 {"keys":{...}} 读回兼容并透明迁移(读→加密重写)。
//! macOS/Linux 编译期 fallback 明文形态(如实注释,子项维持 OPEN)。
//! env FORGE_GEN_API_KEY 优先于文件(D-F5-C)。
//! 红线:本类型【不提供任何序列化出口】(无 Serialize/Display;Debug 只列条目 id),
//! 密钥值仅能经 key_for 取出供 remote 适配器组装 Authorization 头,永不进工具返回/日志/事件。

use std::collections::HashMap;
use std::path::PathBuf;

use base64::Engine as _;

use crate::config::data_dir;

/// keystore.json 默认路径。
pub fn keystore_path() -> PathBuf {
    data_dir().join("keystore.json")
}

// ---------- DPAPI(Windows)/ 明文 fallback(非 Windows)----------

/// Windows:CryptProtectData 每用户熵加密(无额外熵盐,D-RDG-D)。
#[cfg(windows)]
fn protect(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(&input, None, None, None, None, 0, &mut output).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("CryptProtectData 失败: {e}"))
        })?;
        let buf = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData as *mut _,
        )));
        Ok(buf)
    }
}

/// Windows:CryptUnprotectData 解密。
#[cfg(windows)]
fn unprotect(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("CryptUnprotectData 失败(密文损坏或非本用户加密): {e}"),
            )
        })?;
        let buf = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            output.pbData as *mut _,
        )));
        Ok(buf)
    }
}

/// 非 Windows:明文 fallback(macOS Keychain / Linux secret-service 子项维持 OPEN,如实标注)。
#[cfg(not(windows))]
fn protect(data: &[u8]) -> std::io::Result<Vec<u8>> {
    Ok(data.to_vec())
}

/// 非 Windows:明文 fallback。
#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> std::io::Result<Vec<u8>> {
    Ok(data.to_vec())
}

/// 落盘明文域 {"keys":{...}} → 加密文件文本(Windows {"v":1,"dpapi"};非 Windows 明文 JSON)。
fn serialize_store(keys: &HashMap<String, String>) -> Result<String, String> {
    let plain = serde_json::json!({ "keys": keys }).to_string();
    if cfg!(windows) {
        let cipher = protect(plain.as_bytes())
            .map_err(|e| format!("DPAPI 加密失败: {e}"))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(cipher);
        Ok(serde_json::json!({ "v": 1, "dpapi": b64 }).to_string())
    } else {
        Ok(serde_json::to_string_pretty(&serde_json::json!({ "keys": keys })).unwrap())
    }
}

/// 文件文本 → 明文域 keys;识别加密形态 {"v","dpapi"} 与旧明文形态 {"keys"}。
fn deserialize_store(text: &str) -> Result<(HashMap<String, String>, bool), String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("keystore 非 JSON: {e}"))?;
    if let Some(b64) = v.get("dpapi").and_then(serde_json::Value::as_str) {
        let cipher = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("dpapi 域 base64 解码失败: {e}"))?;
        let plain = unprotect(&cipher).map_err(|e| format!("DPAPI 解密失败: {e}"))?;
        let inner: serde_json::Value = serde_json::from_slice(&plain)
            .map_err(|e| format!("DPAPI 明文域非 JSON: {e}"))?;
        let keys = inner
            .get("keys")
            .cloned()
            .and_then(|k| serde_json::from_value::<HashMap<String, String>>(k).ok())
            .ok_or_else(|| "DPAPI 明文域缺 keys".to_string())?;
        // (keys, was_encrypted=true)
        return Ok((keys, true));
    }
    let keys = v
        .get("keys")
        .cloned()
        .and_then(|k| serde_json::from_value::<HashMap<String, String>>(k).ok())
        .unwrap_or_default();
    Ok((keys, false))
}

pub struct Keystore {
    keys: HashMap<String, String>,
}

impl Keystore {
    /// Give an adapter only the named connection's key; never fall back to its template's key.
    pub fn for_backend_alias(&self, alias: &str, adapter: &str) -> Self {
        let mut keys = HashMap::new();
        if let Some(key) = self.secret_for(alias) {
            keys.insert(adapter.to_string(), key);
        }
        Self { keys }
    }

    /// 从默认路径加载;文件缺失 = 空 keystore(不算错误)。
    pub fn load() -> Self {
        Self::load_from(&keystore_path())
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        let keys = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| deserialize_store(&t).ok())
            .map(|(keys, was_encrypted)| {
                // RD-F5-001 透明迁移:旧明文形态读回 → 加密重写(best-effort,写失败不阻断读取)。
                if !was_encrypted {
                    if let Ok(text) = serialize_store(&keys) {
                        let _ = std::fs::write(path, text);
                    }
                }
                keys
            })
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

    /// 只取文件条目本身,不受 env FORGE_GEN_API_KEY 覆盖(该覆盖面向生成后端;
    /// 云账号 refresh token / 设备 Key 这类条目绝不能被它顶替)。空值视同缺失。
    pub fn secret_for(&self, id: &str) -> Option<String> {
        self.keys.get(id).filter(|v| !v.is_empty()).cloned()
    }
}

/// 写入/更新单个密钥条目(F5 wave.3 agentd configure REST 用):读-改-写 keystore.json,
/// 保留其他条目;空 key 拒绝(调用方只在 apiKey 非空时调)。本函数是唯一写盘出口,
/// 密钥值不进日志,错误仅带 IO 信息。RD-F5-001:Windows 下落盘为 DPAPI 加密形态。
pub fn set_key(backend_id: &str, key: &str) -> std::io::Result<()> {
    set_key_at(&keystore_path(), backend_id, key)
}

/// [set_key] 的显式路径版本。
pub fn set_key_at(path: &std::path::Path, backend_id: &str, key: &str) -> std::io::Result<()> {
    if backend_id.is_empty() || key.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "backend_id/key 不可空",
        ));
    }
    let mut keys: HashMap<String, String> = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| deserialize_store(&t).ok())
        .map(|(k, _)| k)
        .unwrap_or_default();
    keys.insert(backend_id.to_string(), key.to_string());
    let text = serialize_store(&keys)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

/// 删除单个密钥条目(云账号登出清令牌):读-改-写 keystore.json,保留其他条目。
/// 返回是否真的删掉了条目;文件缺失或条目不存在 = Ok(false),不写盘。
pub fn remove_key(backend_id: &str) -> std::io::Result<bool> {
    remove_key_at(&keystore_path(), backend_id)
}

/// [remove_key] 的显式路径版本。
pub fn remove_key_at(path: &std::path::Path, backend_id: &str) -> std::io::Result<bool> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(false);
    };
    let (mut keys, _) = deserialize_store(&text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if keys.remove(backend_id).is_none() {
        return Ok(false);
    }
    let text = serialize_store(&keys)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)?;
    Ok(true)
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
    fn secret_for_ignores_env_override() {
        let _g = env_lock();
        let dir = std::env::temp_dir().join(format!("gend-ks6-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("keystore.json");
        std::fs::write(&p, r#"{"keys":{"cloud:refresh":"rt_file","empty":""}}"#).unwrap();
        let ks = Keystore::load_from(&p);
        std::env::set_var("FORGE_GEN_API_KEY", "env-key");
        assert_eq!(ks.secret_for("cloud:refresh").as_deref(), Some("rt_file"));
        assert!(ks.secret_for("cloud:device-key").is_none(), "缺失条目不得回落到 env");
        assert!(ks.secret_for("empty").is_none(), "空值视同缺失");
        assert_eq!(ks.key_for("cloud:device-key").as_deref(), Some("env-key"));
        std::env::remove_var("FORGE_GEN_API_KEY");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_key_keeps_other_entries() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        let dir = std::env::temp_dir().join(format!("gend-ks7-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
        set_key("cloud:refresh", "rt_secret").unwrap();
        set_key("deepseek", "sk-neighbor").unwrap();
        assert!(remove_key("cloud:refresh").unwrap());
        assert!(!remove_key("cloud:refresh").unwrap(), "二次删除应报告未删除");
        assert!(!remove_key("never-existed").unwrap());
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        let ks = Keystore::load_from(&dir.join("keystore.json"));
        assert!(ks.secret_for("cloud:refresh").is_none());
        assert_eq!(ks.secret_for("deepseek").as_deref(), Some("sk-neighbor"));
        let text = std::fs::read_to_string(dir.join("keystore.json")).unwrap();
        assert!(!text.contains("rt_secret"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_key_on_missing_file_is_noop() {
        let dir = std::env::temp_dir().join(format!("gend-ks8-{}", std::process::id()));
        let p = dir.join("no-such.json");
        assert!(!remove_key_at(&p, "cloud:refresh").unwrap());
        assert!(!p.exists(), "缺失文件不应被创建");
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

    // ---------- RD-F5-001:DPAPI 加固(Windows 腿)----------

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip() {
        let plain = r#"{"keys":{"deepseek":"sk-roundtrip-123"}}"#;
        let cipher = protect(plain.as_bytes()).expect("DPAPI 加密失败");
        assert_ne!(cipher, plain.as_bytes(), "密文不应等于明文");
        let back = unprotect(&cipher).expect("DPAPI 解密失败");
        assert_eq!(String::from_utf8(back).unwrap(), plain);
    }

    #[cfg(windows)]
    #[test]
    fn set_key_writes_encrypted_form_and_reads_back() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        let dir = std::env::temp_dir().join(format!("gend-ks4-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
        set_key("deepseek", "sk-DPAPI-secret-999").expect("set_key 失败");
        std::env::remove_var("FORGE_GEN_DATA_DIR");
        let text = std::fs::read_to_string(dir.join("keystore.json")).unwrap();
        // 加密形态:{"v":1,"dpapi":"..."};密文文件不含明文子串。
        assert!(text.contains("\"dpapi\""), "应为 DPAPI 加密形态: {text}");
        assert!(!text.contains("sk-DPAPI-secret-999"), "密文文件含明文密钥: {text}");
        let ks = Keystore::load_from(&dir.join("keystore.json"));
        assert_eq!(ks.key_for("deepseek").as_deref(), Some("sk-DPAPI-secret-999"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(windows)]
    #[test]
    fn legacy_plaintext_migrates_to_encrypted() {
        let _g = env_lock();
        std::env::remove_var("FORGE_GEN_API_KEY");
        let dir = std::env::temp_dir().join(format!("gend-ks5-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("keystore.json");
        std::fs::write(&p, r#"{"keys":{"b1":"sk-legacy-777"}}"#).unwrap();
        let ks = Keystore::load_from(&p);
        assert_eq!(ks.key_for("b1").as_deref(), Some("sk-legacy-777"));
        // 读回后透明迁移:文件已被加密重写,不含明文子串。
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("\"dpapi\""), "迁移后应为加密形态: {text}");
        assert!(!text.contains("sk-legacy-777"), "迁移后文件仍含明文: {text}");
        // 二次加载(加密形态)仍可读。
        let ks2 = Keystore::load_from(&p);
        assert_eq!(ks2.key_for("b1").as_deref(), Some("sk-legacy-777"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
