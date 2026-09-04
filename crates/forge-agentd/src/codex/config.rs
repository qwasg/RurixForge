//! Codex 引擎配置面:`data/codex-config.json`(读-改-写 Mutex 原子落盘,
//! 同 gen-backends.json / llm-openai-compat.json 纪律)。
//!
//! R-5:此文件绝不落密钥——Codex 的 ChatGPT OAuth 令牌与 API Key 由 codex 自己
//! 持久到 `CODEX_HOME/auth.json`,本仓只记「用哪个二进制、哪个 CODEX_HOME」。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;

/// 新会话默认引擎(`local` = 本仓自研工具循环;`codex` = codex app-server)。
pub const ENGINE_LOCAL: &str = "local";
pub const ENGINE_CODEX: &str = "codex";

/// 引擎标识合法性(sessions PATCH / create 入参校验同源)。
pub fn is_known_engine(id: &str) -> bool {
    id == ENGINE_LOCAL || id == ENGINE_CODEX
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexConfig {
    /// 显式 codex 可执行文件路径(空 = 自动决议:env → PATH → 托管安装)。
    #[serde(default)]
    pub codex_bin: String,
    /// CODEX_HOME(空 = 交给 codex 自己的缺省 `~/.codex`)。
    /// 独立配置的意义:Forge 托管的登录态可与用户自己的 CLI 登录态隔离或共用,由人拍板。
    #[serde(default)]
    pub codex_home: String,
    /// 新会话默认引擎。
    #[serde(default = "default_engine")]
    pub default_engine: String,
    /// Codex 会话缺省模型(空 = 用 codex 自己的缺省)。
    #[serde(default)]
    pub default_model: String,
    /// 自动把本项目的 MCP 服务注入 Codex 线程(关掉后 Codex 只有自己的内建工具)。
    #[serde(default = "default_true")]
    pub auto_register_mcp: bool,
    /// 注册 open-computer-use(桌面级 Computer Use;两种引擎共用同一开关)。
    #[serde(default)]
    pub computer_use: bool,
}

fn default_engine() -> String {
    ENGINE_LOCAL.to_string()
}

fn default_true() -> bool {
    true
}

impl Default for CodexConfig {
    fn default() -> Self {
        CodexConfig {
            codex_bin: String::new(),
            codex_home: String::new(),
            default_engine: default_engine(),
            default_model: String::new(),
            auto_register_mcp: true,
            computer_use: false,
        }
    }
}

impl CodexConfig {
    /// 响应面 JSON(无密钥字段,整体可直接上屏)。
    pub fn to_json(&self) -> Value {
        json!({
            "codexBin": self.codex_bin,
            "codexHome": self.codex_home,
            "defaultEngine": self.default_engine,
            "defaultModel": self.default_model,
            "autoRegisterMcp": self.auto_register_mcp,
            "computerUse": self.computer_use,
        })
    }
}

/// PATCH 面(缺省字段不变;字符串空串 = 显式清空)。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexConfigPatch {
    #[serde(default)]
    pub codex_bin: Option<String>,
    #[serde(default)]
    pub codex_home: Option<String>,
    #[serde(default)]
    pub default_engine: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub auto_register_mcp: Option<bool>,
    #[serde(default)]
    pub computer_use: Option<bool>,
}

fn path() -> PathBuf {
    crate::agent_data_root().join("codex-config.json")
}

fn write_lock() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 读配置(文件缺失/损坏 → 缺省;不 panic——配置读不出来不该让守护起不来)。
pub fn load() -> CodexConfig {
    let p = path();
    let Ok(text) = std::fs::read_to_string(&p) else {
        return CodexConfig::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// 应用 PATCH 并落盘,返回落盘后的配置。
pub fn patch(req: CodexConfigPatch) -> Result<CodexConfig, String> {
    let _g = write_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut cfg = load();
    if let Some(v) = req.codex_bin {
        cfg.codex_bin = v.trim().to_string();
    }
    if let Some(v) = req.codex_home {
        cfg.codex_home = v.trim().to_string();
    }
    if let Some(v) = req.default_engine {
        let v = v.trim().to_string();
        if !is_known_engine(&v) {
            return Err(format!("未知引擎: {v}(支持 local|codex)"));
        }
        cfg.default_engine = v;
    }
    if let Some(v) = req.default_model {
        cfg.default_model = v.trim().to_string();
    }
    if let Some(v) = req.auto_register_mcp {
        cfg.auto_register_mcp = v;
    }
    if let Some(v) = req.computer_use {
        cfg.computer_use = v;
    }
    save_locked(&cfg)?;
    Ok(cfg)
}

fn save_locked(cfg: &CodexConfig) -> Result<(), String> {
    let p = path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|e| format!("序列化失败: {e}"))?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写临时文件失败: {e}"))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("原子替换失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DataDirGuard(PathBuf);
    impl DataDirGuard {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "agentd-codexcfg-{tag}-{}-{}",
                std::process::id(),
                gend::timeutil::unix_millis()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("FORGE_AGENTD_DATA_DIR", &dir);
            DataDirGuard(dir)
        }
    }
    impl Drop for DataDirGuard {
        fn drop(&mut self) {
            std::env::remove_var("FORGE_AGENTD_DATA_DIR");
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    /// 缺省 → PATCH 往返 → 落盘可复读;未知引擎如实拒。
    #[test]
    fn config_roundtrip_and_reject_unknown_engine() {
        let _lock = crate::llm::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _g = DataDirGuard::new("rt");
        let cfg = load();
        assert_eq!(cfg.default_engine, ENGINE_LOCAL);
        assert!(cfg.auto_register_mcp);
        assert!(!cfg.computer_use);

        let saved = patch(CodexConfigPatch {
            default_engine: Some("codex".into()),
            computer_use: Some(true),
            default_model: Some("  gpt-5.6-terra  ".into()),
            ..Default::default()
        })
        .expect("PATCH 应成功");
        assert_eq!(saved.default_engine, ENGINE_CODEX);
        assert!(saved.computer_use);
        // trim 生效(前端输入框尾随空格不该污染 spawn 参数)。
        assert_eq!(saved.default_model, "gpt-5.6-terra");
        assert_eq!(load().default_engine, ENGINE_CODEX);

        let err = patch(CodexConfigPatch {
            default_engine: Some("gemini".into()),
            ..Default::default()
        })
        .expect_err("未知引擎须拒");
        assert!(err.contains("gemini"), "{err}");
        // 拒绝的 PATCH 不得改动已落盘配置。
        assert_eq!(load().default_engine, ENGINE_CODEX);
    }
}
