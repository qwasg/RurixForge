//! 云模式本地状态(15 §8.1):`data/cloud-config.json`(用户可改的配置)与
//! `data/cloud-state.json`(登录用户摘要、设备 Key 前缀、余额缓存、模型目录缓存)。
//! 两个文件都不含任何令牌;令牌只进 keystore(见 [super::SecretStore])。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Catalog;

/// 开发缺省地址(forge-cloud `:8110`)。
pub(crate) const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:8110";

const CONFIG_FILE: &str = "cloud-config.json";
const STATE_FILE: &str = "cloud-state.json";

/// 服务端地址缺省链:env FORGE_CLOUD_URL > 构建期 FORGE_CLOUD_URL > 开发缺省。
pub(crate) fn default_server_url() -> String {
    std::env::var("FORGE_CLOUD_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| option_env!("FORGE_CLOUD_URL").map(str::to_string))
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_server_url(&s))
        .unwrap_or_else(|| DEFAULT_SERVER_URL.to_string())
}

/// 去空白与尾斜杠(调用方拼 `/api/v1/...`)。
pub(crate) fn normalize_server_url(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

/// 主机名:COMPUTERNAME(Windows)> HOSTNAME > 固定兜底。
pub(crate) fn default_device_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| "RurixForge Desktop".to_string())
}

/// 云端 `device.platform`。
pub(crate) fn platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        _ => "other",
    }
}

/// UUID v4 形态的随机设备 ID(每进程随机种子的 RandomState + 时间 + 计数混合;
/// 只要求跨设备不撞,不作密码学用途)。
pub(crate) fn new_device_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut words = [0u64; 2];
    for (i, w) in words.iter_mut().enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        );
        h.write_u32(std::process::id());
        h.write_usize(i);
        *w = h.finish();
    }
    let hi = words[0];
    let lo = words[1];
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (hi >> 32) as u32,
        (hi >> 16) as u16,
        (hi & 0x0fff) as u16,
        ((lo >> 48) as u16 & 0x3fff) | 0x8000,
        lo & 0xffff_ffff_ffff
    )
}

/// 同步开关(缺省全开)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct SyncToggles {
    pub settings: bool,
    pub memory: bool,
    pub skills: bool,
}

impl Default for SyncToggles {
    fn default() -> Self {
        SyncToggles {
            settings: true,
            memory: true,
            skills: true,
        }
    }
}

/// `data/cloud-config.json`。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct CloudConfig {
    /// 用户显式设置的服务端地址;空 = 走 [default_server_url] 缺省链(不把缺省值写死进文件,
    /// 否则之后改 env 就不生效了)。
    pub server_url: String,
    pub device_id: String,
    /// 用户自定义设备名;空 = 主机名。
    pub device_name: String,
    pub sync: SyncToggles,
}

impl CloudConfig {
    pub fn effective_server_url(&self) -> String {
        if self.server_url.trim().is_empty() {
            default_server_url()
        } else {
            normalize_server_url(&self.server_url)
        }
    }

    pub fn effective_device_name(&self) -> String {
        if self.device_name.trim().is_empty() {
            default_device_name()
        } else {
            self.device_name.trim().to_string()
        }
    }
}

/// 最近一次账号级错误(status.lastError)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LastError {
    pub code: String,
    pub message: String,
    pub at: String,
}

/// `data/cloud-state.json`(非密)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct CloudState {
    /// 云端 `User` 原样(未登录 None)。
    pub user: Option<Value>,
    pub device_key_prefix: Option<String>,
    /// 本机登录会话 ID(JWT `sid`;删设备时据此识别「删的是本机」)。
    pub session_id: Option<String>,
    /// 登录所属服务端(换服务端后旧登录态作废)。
    pub server_url: Option<String>,
    pub balance_micros: Option<i64>,
    pub currency: Option<String>,
    pub subscriptions: Vec<Value>,
    pub last_error: Option<LastError>,
    /// 最近一次拉到的模型目录。登出后仍保留,供 design-snapshot 以 needs-login 形态展示;
    /// 对外的 `catalog_cached()` 在未登录时不返回它。
    pub catalog: Option<Catalog>,
    pub logged_in_at: Option<String>,
}

/// 读 JSON 文件;缺失/损坏按缺省处理(配置读不出来不该让守护起不来)。
fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// tmp + rename 原子写(Windows rename 不覆盖既有目标 → 先删)。
fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(&tmp, path)
}

pub(crate) fn load_config(root: &Path) -> CloudConfig {
    read_json(&root.join(CONFIG_FILE))
}

pub(crate) fn save_config(root: &Path, cfg: &CloudConfig) -> std::io::Result<()> {
    write_json(&root.join(CONFIG_FILE), cfg)
}

pub(crate) fn load_state(root: &Path) -> CloudState {
    read_json(&root.join(STATE_FILE))
}

pub(crate) fn save_state(root: &Path, st: &CloudState) -> std::io::Result<()> {
    write_json(&root.join(STATE_FILE), st)
}

/// 落盘位置(生产 = agent 数据根 + gend keystore/data 目录;测试可全指向临时目录)。
#[derive(Debug, Clone)]
pub(crate) struct Paths {
    /// cloud-config.json / cloud-state.json 所在目录。
    pub data_root: PathBuf,
    /// keystore.json 路径(refresh token 与设备 Key)。
    pub keystore: PathBuf,
    /// gend data 目录(cloud-embedding.json,供 MCP 子进程的 embedding 回落)。
    pub gen_data: PathBuf,
}

impl Paths {
    pub fn production() -> Self {
        Paths {
            data_root: crate::agent_data_root(),
            keystore: gend::keystore::keystore_path(),
            gen_data: gend::config::data_dir(),
        }
    }
}

/// FORGE_ALLOW_BYO != "0":允许自带密钥(DeepSeek / openai-compat / Codex ChatGPT 登录)。
pub(crate) fn byo_allowed() -> bool {
    std::env::var("FORGE_ALLOW_BYO")
        .map(|v| v.trim() != "0")
        .unwrap_or(true)
}

/// FORGE_AGENT_DEV_MOCK == "1":允许 mock 模型(仅开发/测试)。单测进程缺省视为开启
/// (既有测试大量依赖 mock 恒绿 seam),显式设 "0" 可模拟生产口径。
pub(crate) fn dev_mock_enabled() -> bool {
    match std::env::var("FORGE_AGENT_DEV_MOCK") {
        Ok(v) if v.trim() == "1" => true,
        Ok(v) if v.trim() == "0" => false,
        _ => cfg!(test),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_uuid_shaped_and_unique() {
        let a = new_device_id();
        let b = new_device_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36, "{a}");
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'), "{a}");
        assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }

    #[test]
    fn config_roundtrip_and_effective_values() {
        let dir = std::env::temp_dir().join(format!(
            "agentd-cloudcfg-{}-{}",
            std::process::id(),
            gend::timeutil::unix_millis()
        ));
        let mut cfg = load_config(&dir);
        assert_eq!(cfg, CloudConfig::default());
        assert!(cfg.sync.settings && cfg.sync.memory && cfg.sync.skills);
        cfg.server_url = " https://cloud.example.com/ ".into();
        cfg.device_id = new_device_id();
        save_config(&dir, &cfg).unwrap();
        save_config(&dir, &cfg).unwrap();
        let back = load_config(&dir);
        assert_eq!(back, cfg);
        assert_eq!(back.effective_server_url(), "https://cloud.example.com");
        assert!(!back.effective_device_name().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
