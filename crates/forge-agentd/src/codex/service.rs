//! `CodexService`:整个守护进程一份的 Codex 引擎门面(进 `AppState`)。
//!
//! 职责:持有 app-server 客户端(按配置懒建、配置变更后重建)、托管安装的进度状态、
//! 账户/额度/模型清单的缓存,以及账户面 RPC 的封装。
//!
//! 关于「方法名多候选」:app-server 是仍在演进的接口面,同一能力在不同 codex 版本上
//! 叫法不同(`account/read` vs `getAuthStatus` 之类)。这里对账户面的每个能力按优先序
//! 试多个方法名,只在「方法不存在」这一种错误上继续往下试 —— 其他错误(未登录、
//! 配额耗尽)必须原样上抛,不能被当成「换个名字再试」而把真实原因吞掉。

use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};

use crate::events::{EventBus, EventDraft};

use super::bin;
use super::config;
use super::rpc::{CodexClient, CodexError, CodexTransport, Inbound, StdioTransport};

/// 托管安装的进度状态(REST `GET /codex/status` 的 `install` 段)。
#[derive(Debug, Clone, Default)]
pub struct InstallState {
    pub running: bool,
    /// 最近一次安装的尾部日志(npm 输出;截断防爆内存)。
    pub log: String,
    pub error: Option<String>,
    pub finished_at: Option<String>,
}

const INSTALL_LOG_MAX: usize = 8000;

/// 账户/额度/模型缓存。全部来自 codex 自己的回答,本仓不落密钥(R-5)。
#[derive(Debug, Clone, Default)]
pub struct AccountState {
    /// `chatgpt` = 订阅额度;`apikey` = 按量计费;None = 未登录。
    pub auth_mode: Option<String>,
    pub plan_type: Option<String>,
    pub email: Option<String>,
    /// `account/rateLimits/read` 的原样负载(primary/secondary 用量与重置时间)。
    pub rate_limits: Option<Value>,
    /// `model/list` 的原样条目。
    pub models: Vec<Value>,
    pub last_error: Option<String>,
}

pub struct CodexService {
    client: StdMutex<Option<Arc<CodexClient>>>,
    /// 建客户端时用的启动命令描述;配置改了(换二进制/换 CODEX_HOME)就要重建。
    fingerprint: StdMutex<String>,
    install: Arc<StdMutex<InstallState>>,
    account: Arc<StdMutex<AccountState>>,
    cancelled_logins: Arc<StdMutex<HashSet<String>>>,
    /// 正在消费原生 Goal 自动 turns 的会话。REST 更新 Goal 时据此复用现有桥，
    /// 不能再起第二个订阅者把同一通知映射两遍。
    goal_bridges: StdMutex<HashSet<String>>,
    /// 单测注入:有则一律用它建客户端,不去 spawn 子进程。
    transport_override: StdMutex<Option<Arc<dyn CodexTransport>>>,
}

impl Default for CodexService {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexService {
    pub fn new() -> Self {
        CodexService {
            client: StdMutex::new(None),
            fingerprint: StdMutex::new(String::new()),
            install: Arc::new(StdMutex::new(InstallState::default())),
            account: Arc::new(StdMutex::new(AccountState::default())),
            cancelled_logins: Arc::new(StdMutex::new(HashSet::new())),
            goal_bridges: StdMutex::new(HashSet::new()),
            transport_override: StdMutex::new(None),
        }
    }

    /// 单测/集成测试用:替换传输(全内存脚本化),从此不再 spawn 子进程。
    #[cfg(test)]
    pub fn with_transport(transport: Arc<dyn CodexTransport>) -> Self {
        let s = Self::new();
        *s.transport_override
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(transport);
        s
    }

    /// 当前配置对应的启动指纹(变了就重建客户端)。
    fn compute_fingerprint(cfg: &config::CodexConfig) -> String {
        let launch = bin::resolve().map(|l| l.display()).unwrap_or_default();
        let cloud = crate::cloud::global();
        let auth = config::effective_auth_source(&cloud);
        let home = if auth == config::AUTH_CLOUD && cloud.is_logged_in() {
            config::managed_cloud_home().display().to_string()
        } else if cfg.codex_home.is_empty() {
            config::managed_chatgpt_home().display().to_string()
        } else {
            cfg.codex_home.clone()
        };
        format!("{launch}|{home}|{auth}")
    }

    fn env_for(cfg: &config::CodexConfig) -> Vec<(String, String)> {
        let mut env = Vec::new();
        let cloud = crate::cloud::global();
        let auth = config::effective_auth_source(&cloud);
        env.push(("FORGE_CODEX_AUTH_SOURCE".to_string(), auth.clone()));
        let codex_home = if auth == config::AUTH_CLOUD && cloud.is_logged_in() {
            let url = cloud.server_url();
            if let Err(e) = config::ensure_cloud_codex_home(&url) {
                eprintln!("[codex] 写 codex-cloud-home/config.toml 失败: {e}");
            }
            config::managed_cloud_home()
        } else if !cfg.codex_home.is_empty() {
            std::path::PathBuf::from(&cfg.codex_home)
        } else {
            config::managed_chatgpt_home()
        };
        if cfg.codex_home.is_empty() {
            if let Err(error) = std::fs::create_dir_all(&codex_home) {
                eprintln!("[codex] 无法创建托管 CLI 目录: {error}");
            }
        }
        if !codex_home.as_os_str().is_empty() {
            env.push((
                "CODEX_HOME".to_string(),
                codex_home.to_string_lossy().into_owned(),
            ));
        }
        if auth == config::AUTH_CLOUD {
            if let Some(key) = cloud.device_key() {
                env.push(("FORGE_CLOUD_API_KEY".to_string(), key));
            }
        }
        env
    }

    /// An isolated workflow process with the same authentication configuration.
    /// Never install it as the ordinary session/Goal client: interrupt fallback
    /// may retire this entire process without affecting unrelated sessions.
    pub fn isolated_client(&self) -> Result<Arc<CodexClient>, CodexError> {
        let transport = match self
            .transport_override
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            Some(transport) => transport,
            None => {
                let launch = bin::resolve().ok_or_else(|| {
                    CodexError("CODEX_NOT_INSTALLED: 未找到 Codex 可执行文件".to_string())
                })?;
                Arc::new(StdioTransport::new(launch, Self::env_for(&config::load())))
            }
        };
        Ok(Arc::new(CodexClient::new(transport)))
    }

    /// 取(必要时新建)客户端。未安装 codex → 显式 `CODEX_NOT_INSTALLED`。
    pub fn client(&self) -> Result<Arc<CodexClient>, CodexError> {
        let cfg = config::load();
        let fp = if self.transport_override.lock().is_ok_and(|g| g.is_some()) {
            "scripted".to_string()
        } else {
            Self::compute_fingerprint(&cfg)
        };
        {
            let existing = self.client.lock().unwrap_or_else(|e| e.into_inner());
            let same = *self.fingerprint.lock().unwrap_or_else(|e| e.into_inner()) == fp;
            if let (Some(c), true) = (existing.as_ref(), same) {
                return Ok(Arc::clone(c));
            }
        }
        let transport: Arc<dyn CodexTransport> = match self
            .transport_override
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            Some(t) => t,
            None => {
                let launch = bin::resolve().ok_or_else(|| {
                    CodexError(
                        "CODEX_NOT_INSTALLED:未找到 codex 可执行文件。请在设置·Codex 里安装,\
                         或指定已有的 codex 路径"
                            .to_string(),
                    )
                })?;
                Arc::new(StdioTransport::new(launch, Self::env_for(&cfg)))
            }
        };
        let fresh = Arc::new(CodexClient::new(transport));
        // 换了客户端 = 旧连接作废(旧子进程随出站端丢弃而退出)。
        let old = self
            .client
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(Arc::clone(&fresh));
        if let Some(o) = old {
            o.suspend();
        }
        *self.fingerprint.lock().unwrap_or_else(|e| e.into_inner()) = fp;
        self.spawn_account_watcher(Arc::clone(&fresh));
        Ok(fresh)
    }

    /// 已连接并握手完毕的客户端。
    pub async fn ready_client(&self) -> Result<Arc<CodexClient>, CodexError> {
        let c = self.client()?;
        c.ensure_started().await?;
        Ok(c)
    }

    /// 只返回已运行的客户端，状态查询不能为了“看一眼”暗中启动进程。
    pub fn running_client(&self) -> Option<Arc<CodexClient>> {
        self.client
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|client| client.running())
            .cloned()
    }

    pub fn goal_bridge_active(&self, session_id: &str) -> bool {
        self.goal_bridges
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(session_id)
    }

    pub fn set_goal_bridge(&self, session_id: &str, active: bool) {
        let mut bridges = self.goal_bridges.lock().unwrap_or_else(|e| e.into_inner());
        if active {
            bridges.insert(session_id.to_string());
        } else {
            bridges.remove(session_id);
        }
    }

    /// 服务启动预热：连接、握手并并行填充设置页会用到的三个缓存。
    /// 预热是 best-effort，任何失败都不阻塞 HTTP 服务启动。
    pub async fn prewarm(&self) {
        if self.ready_client().await.is_err() {
            return;
        }
        let _ = tokio::join!(
            self.refresh_account(),
            self.models(true),
            self.refresh_rate_limits()
        );
    }

    /// 账户/额度通知的常驻旁听:把 codex 主动推来的账户变化落进缓存,
    /// 好让状态栏与设置页不必每次都发一次 RPC。
    fn spawn_account_watcher(&self, client: Arc<CodexClient>) {
        let account = Arc::clone(&self.account);
        let cancelled_logins = Arc::clone(&self.cancelled_logins);
        let client = Arc::downgrade(&client);
        tokio::spawn(async move {
            loop {
                let Some(current) = client.upgrade() else {
                    // The service replaced this client after a config change. A weak
                    // watcher must never resurrect the retired process.
                    break;
                };
                if !current.running() {
                    if current.restart_if_allowed().await.is_err() {
                        drop(current);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                }
                let mut rx = current.subscribe();
                drop(current);
                while let Some(msg) = rx.recv().await {
                    let Inbound::Notification { method, params } = msg else {
                        continue;
                    };
                    if method == "account/login/completed" {
                        let cancelled = cancelled_logins.lock().unwrap_or_else(|e| e.into_inner());
                        if cancelled.contains("*") || params["loginId"].as_str().is_some_and(|id| cancelled.contains(id)) {
                            continue;
                        }
                    }
                    let mut a = account.lock().unwrap_or_else(|e| e.into_inner());
                    match method.as_str() {
                        "account/rateLimits/updated" => {
                            let update = rate_limits_snapshot(&params);
                            a.rate_limits = Some(merge_non_null(a.rate_limits.take(), update));
                        }
                        "account/login/completed" => merge_login_completed(&mut a, &params),
                        "account/updated" | "authStatusChange" => merge_account(&mut a, &params),
                        _ => {}
                    }
                }
            }
        });
    }

    pub fn account_snapshot(&self) -> AccountState {
        self.account
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn install_snapshot(&self) -> InstallState {
        self.install
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 拉账户态(未登录不算错误:`authMode = null`)。
    pub async fn refresh_account(&self) -> Result<AccountState, CodexError> {
        let c = self.ready_client().await?;
        let v = request_first_supported(
            &c,
            &[
                ("account/read", json!({})),
                ("account/get", json!({})),
                ("getAuthStatus", json!({ "includeToken": false })),
            ],
        )
        .await?;
        let mut a = self.account.lock().unwrap_or_else(|e| e.into_inner());
        merge_account(&mut a, &v);
        Ok(a.clone())
    }

    /// 拉额度(订阅制的 primary/secondary 窗口用量)。
    pub async fn refresh_rate_limits(&self) -> Result<Option<Value>, CodexError> {
        let c = self.ready_client().await?;
        let v = request_first_supported(
            &c,
            &[
                ("account/rateLimits/read", json!({})),
                ("account/rate_limits/read", json!({})),
            ],
        )
        .await?;
        let limits = rate_limits_snapshot(&v);
        let mut a = self.account.lock().unwrap_or_else(|e| e.into_inner());
        a.rate_limits = Some(limits.clone());
        Ok(Some(limits))
    }

    /// 拉模型清单(缓存;`force=false` 且已有缓存则直接返回)。
    pub async fn models(&self, force: bool) -> Result<Vec<Value>, CodexError> {
        if !force {
            let cached = self
                .account
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .models
                .clone();
            if !cached.is_empty() {
                return Ok(cached);
            }
        }
        let c = self.ready_client().await?;
        let (method, mut page) = match c.request("model/list", json!({})).await {
            Ok(value) => ("model/list", value),
            Err(error) if is_method_not_found(&error) => {
                ("models/list", c.request("models/list", json!({})).await?)
            }
            Err(error) => return Err(error),
        };
        let mut items = Vec::new();
        let mut seen_cursors = HashSet::new();
        for _ in 0..100 {
            items.extend(
                page.get("data")
                    .or_else(|| page.get("models"))
                    .or_else(|| page.get("items"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|model| model.get("hidden") != Some(&Value::Bool(true))),
            );
            let Some(cursor) = page
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|cursor| !cursor.is_empty())
            else {
                break;
            };
            if !seen_cursors.insert(cursor.to_string()) {
                return Err(CodexError("model/list 返回了重复 nextCursor".to_string()));
            }
            page = c.request(method, json!({ "cursor": cursor })).await?;
        }
        if page
            .get("nextCursor")
            .and_then(Value::as_str)
            .is_some_and(|cursor| !cursor.is_empty() && seen_cursors.len() >= 100)
        {
            return Err(CodexError("model/list 分页超过 100 页".to_string()));
        }
        self.account
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .models = items.clone();
        Ok(items)
    }

    /// 发起登录。`kind`:`chatgpt`(订阅额度,浏览器 OAuth)/ `deviceCode` / `apiKey`。
    /// 返回体原样透传给前端(chatgpt 流里含要打开的 `authUrl`)。
    pub async fn login(&self, kind: &str, api_key: Option<&str>) -> Result<Value, CodexError> {
        self.cancelled_logins.lock().unwrap_or_else(|e| e.into_inner()).remove("*");
        self.account.lock().unwrap_or_else(|e| e.into_inner()).last_error = None;
        let c = self.ready_client().await?;
        let out = match kind {
            "chatgpt" => {
                request_first_supported(
                    &c,
                    &[
                        ("account/login/start", json!({ "type": "chatgpt" })),
                        ("loginChatGpt", json!({})),
                    ],
                )
                .await?
            }
            "deviceCode" => {
                request_first_supported(
                    &c,
                    &[(
                        "account/login/start",
                        json!({ "type": "chatgptDeviceCode" }),
                    )],
                )
                .await?
            }
            "apiKey" => {
                let key = api_key
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .ok_or_else(|| CodexError("apiKey 不可空".to_string()))?;
                request_first_supported(
                    &c,
                    &[
                        (
                            "account/login/start",
                            json!({ "type": "apiKey", "apiKey": key }),
                        ),
                        ("loginApiKey", json!({ "apiKey": key })),
                    ],
                )
                .await?
            }
            other => return Err(CodexError(format!("未知登录方式: {other}"))),
        };
        Ok(out)
    }

    pub async fn cancel_login(&self, login_id: Option<&str>) -> Result<Value, CodexError> {
        {
            let mut cancelled = self.cancelled_logins.lock().unwrap_or_else(|e| e.into_inner());
            if cancelled.len() >= 32 { cancelled.clear(); }
            cancelled.insert(login_id.unwrap_or("*").to_string());
        }
        let c = self.ready_client().await?;
        let params = match login_id {
            Some(id) => json!({ "loginId": id }),
            None => json!({}),
        };
        let result = request_first_supported(
            &c,
            &[
                ("account/login/cancel", params.clone()),
                ("cancelLoginChatGpt", params),
            ],
        )
        .await?;
        self.account.lock().unwrap_or_else(|e| e.into_inner()).last_error = None;
        Ok(result)
    }

    pub async fn logout(&self) -> Result<(), CodexError> {
        let c = self.ready_client().await?;
        request_first_supported(&c, &[("account/logout", json!({})), ("logout", json!({}))])
            .await?;
        *self.account.lock().unwrap_or_else(|e| e.into_inner()) = AccountState::default();
        Ok(())
    }

    /// 托管安装(后台跑 npm;重复调用在跑的那次直接返回 false)。
    pub fn start_install(
        &self,
        events: Arc<EventBus>,
        session_ids: Vec<String>,
    ) -> Result<bool, String> {
        {
            let mut st = self.install.lock().unwrap_or_else(|e| e.into_inner());
            if st.running {
                return Ok(false);
            }
            *st = InstallState {
                running: true,
                log: String::new(),
                error: None,
                finished_at: None,
            };
        }
        emit_install_event(
            &events,
            &session_ids,
            "codex.install.progress",
            json!({ "stage": "starting", "progress": 0 }),
            false,
        );
        let npm = match bin::npm_launch() {
            Some(p) => p,
            None => {
                let msg = "未找到 npm。Codex 与 open-computer-use 都以 npm 包分发,\
                           请先安装 Node.js(含 npm)后重试"
                    .to_string();
                let mut st = self.install.lock().unwrap_or_else(|e| e.into_inner());
                st.running = false;
                st.error = Some(msg.clone());
                st.finished_at = Some(now_iso());
                drop(st);
                emit_install_event(
                    &events,
                    &session_ids,
                    "codex.install.completed",
                    json!({ "ok": false, "error": msg }),
                    true,
                );
                return Err(msg);
            }
        };
        let root = bin::managed_root();
        let state = Arc::clone(&self.install);
        tokio::spawn(async move {
            let outcome = run_install(&npm, &root).await;
            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            st.running = false;
            st.finished_at = Some(now_iso());
            let payload = match outcome {
                Ok(log) => {
                    st.log = tail(&log, INSTALL_LOG_MAX);
                    st.error = None;
                    json!({ "ok": true })
                }
                Err((log, err)) => {
                    st.log = tail(&log, INSTALL_LOG_MAX);
                    st.error = Some(err.clone());
                    json!({ "ok": false, "error": err })
                }
            };
            drop(st);
            emit_install_event(
                &events,
                &session_ids,
                "codex.install.progress",
                json!({ "stage": "finished", "progress": 100 }),
                false,
            );
            emit_install_event(
                &events,
                &session_ids,
                "codex.install.completed",
                payload,
                true,
            );
        });
        Ok(true)
    }

    /// design-snapshot 的 `agents` 段(状态栏 / AgentSwitcher 共用)。
    pub fn agents_json(&self) -> Value {
        let cfg = config::load();
        let acct = self.account_snapshot();
        let launch = bin::resolve();
        let running = {
            let guard = self.client.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().is_some_and(|c| c.running())
        };
        json!({
            "defaultEngine": cfg.default_engine,
            "engines": [
                { "id": "local" },
                {
                    "id": "codex",
                    "installed": launch.is_some(),
                    "running": running,
                    "authMode": acct.auth_mode,
                    "planType": acct.plan_type,
                    "email": acct.email,
                    "rateLimits": acct.rate_limits,
                }
            ]
        })
    }

    /// Codex `model/list` 缓存翻成本仓 SnapshotModel 条目(id 加 `codex:` 前缀)。
    pub fn model_cards(&self) -> Vec<Value> {
        let acct = self.account_snapshot();
        let avail = if acct.auth_mode.is_some() {
            "available"
        } else {
            "needs-key"
        };
        acct.models
            .iter()
            .filter_map(|m| {
                let id = m
                    .get("id")
                    .or_else(|| m.get("slug"))
                    .or_else(|| m.get("model"))
                    .and_then(Value::as_str)?;
                let label = m
                    .get("displayName")
                    .or_else(|| m.get("label"))
                    .and_then(Value::as_str)
                    .unwrap_or(id);
                let efforts = m
                    .get("supportedReasoningEfforts")
                    .or_else(|| m.get("reasoningEfforts"))
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|e| {
                                let eid = e
                                    .as_str()
                                    .or_else(|| e.get("reasoningEffort").and_then(Value::as_str))
                                    .or_else(|| e.get("id").and_then(Value::as_str))?;
                                Some(json!({
                                    "id": eid,
                                    "label": eid,
                                    "description": e.get("description").cloned().unwrap_or(Value::Null),
                                }))
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_else(|| {
                        vec![
                            json!({ "id": "low", "label": "low" }),
                            json!({ "id": "medium", "label": "medium" }),
                            json!({ "id": "high", "label": "high" }),
                        ]
                    });
                let default_effort = m
                    .get("defaultReasoningEffort")
                    .cloned()
                    .filter(|v| v.is_string())
                    .or_else(|| efforts.first().and_then(|e| e.get("id")).cloned())
                    .unwrap_or(json!("medium"));
                Some(json!({
                    "id": format!("codex:{id}"),
                    "label": label,
                    "provider": "codex",
                    "availability": avail,
                    "group": "Codex",
                    "supportsThinking": true,
                    "effortOptions": efforts,
                    "defaultEffort": default_effort,
                    "contextOptions": [
                        { "id": "128k", "label": "128K", "tokens": 128000 },
                        { "id": "1m", "label": "1M", "tokens": 1_000_000 },
                    ],
                    "defaultContext": "1m",
                }))
            })
            .collect()
    }

    /// `GET /codex/status` 的完整负载。
    pub fn status_json(&self, project_root: &Path) -> Value {
        let cfg = config::load();
        let cloud = crate::cloud::global();
        let auth_source = config::effective_auth_source(&cloud);
        let (codex_installed, cu_installed) = bin::managed_installed();
        let launch = bin::resolve();
        let acct = self.account_snapshot();
        let inst = self.install_snapshot();
        let (running, transport, version) = {
            let guard = self.client.lock().unwrap_or_else(|e| e.into_inner());
            match guard.as_ref() {
                Some(c) => (c.running(), Some(c.describe()), c.server_version()),
                None => (false, None, None),
            }
        };
        json!({
            "ok": true,
            "installed": launch.is_some(),
            "managedInstalled": codex_installed,
            "computerUseInstalled": cu_installed,
            "command": launch.map(|l| l.display()),
            "npmAvailable": bin::npm_launch().is_some(),
            "running": running,
            "version": version,
            "transport": transport,
            "config": cfg.to_json(),
            "authSource": auth_source,
            "cloud": {
                "loggedIn": cloud.is_logged_in(),
                "serverUrl": cloud.server_url(),
            },
            "account": {
                "authMode": acct.auth_mode,
                "planType": acct.plan_type,
                "email": acct.email,
                "rateLimits": acct.rate_limits,
                "lastError": acct.last_error,
            },
            "models": acct.models,
            "install": {
                "running": inst.running,
                "log": inst.log,
                "error": inst.error,
                "finishedAt": inst.finished_at,
            },
            "mcp": super::mcp_config::status_json(project_root),
        })
    }
}

/// 按优先序试方法名;只有「方法不存在」才继续往下试,其余错误原样上抛。
async fn request_first_supported(
    client: &CodexClient,
    candidates: &[(&str, Value)],
) -> Result<Value, CodexError> {
    let mut last: Option<CodexError> = None;
    for (method, params) in candidates {
        match client.request(method, params.clone()).await {
            Ok(v) => return Ok(v),
            Err(e) if is_method_not_found(&e) => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| CodexError("无可用方法候选".to_string())))
}

fn is_method_not_found(e: &CodexError) -> bool {
    let s = e.0.to_lowercase();
    s.contains("method not found") || s.contains("unknown method") || s.contains("unsupported")
}

/// 把账户负载里认得的字段并进缓存(codex 各版本字段名有出入,认多种拼法)。
fn merge_account(a: &mut AccountState, v: &Value) {
    let root = v.get("account").unwrap_or(v);
    // account/read 的未登录正式形态是 `{account:null}`；账户通知登出则是
    // `{authMode:null}`。两者都必须清旧缓存，不能让 UI 在登出后继续显示 Pro。
    if root.is_null() || root.get("authMode") == Some(&Value::Null) {
        a.auth_mode = None;
        a.plan_type = None;
        a.email = None;
        a.rate_limits = None;
        return;
    }
    if let Some(m) = root
        .get("authMode")
        .or_else(|| root.get("auth_mode"))
        .or_else(|| root.get("method"))
        .or_else(|| root.get("type"))
        .and_then(Value::as_str)
    {
        a.auth_mode = Some(match m {
            "apiKey" => "apikey".to_string(),
            other => other.to_string(),
        });
        a.last_error = None;
    }
    if root.get("planType") == Some(&Value::Null) || root.get("plan_type") == Some(&Value::Null) {
        a.plan_type = None;
    } else if let Some(p) = root
        .get("planType")
        .or_else(|| root.get("plan_type"))
        .or_else(|| root.get("plan"))
        .and_then(Value::as_str)
    {
        a.plan_type = Some(p.to_string());
    }
    if root.get("email") == Some(&Value::Null) {
        a.email = None;
    } else if let Some(e) = root.get("email").and_then(Value::as_str) {
        a.email = Some(e.to_string());
    }
    if let Some(r) = root
        .get("rateLimits")
        .or_else(|| root.get("rate_limits"))
        .filter(|r| !r.is_null())
    {
        a.rate_limits = Some(r.clone());
    }
    // 显式的「已登出」形态:authenticated:false / authMode:null。
    if root.get("authenticated") == Some(&Value::Bool(false)) {
        a.auth_mode = None;
        a.plan_type = None;
        a.email = None;
    }
}

fn merge_login_completed(account: &mut AccountState, params: &Value) {
    if params.get("success") == Some(&Value::Bool(true)) {
        account.last_error = None;
        return;
    }
    account.last_error = Some(
        params
            .get("error")
            .map(safe_login_error)
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "Codex 登录失败".to_string()),
    );
}

/// Login errors may grow extra fields in newer app-server versions. Only surface
/// code/type and message; never serialize the whole object where tokens or request
/// metadata could be present.
fn safe_login_error(error: &Value) -> String {
    match error {
        Value::String(message) => {
            if let Ok(structured) = serde_json::from_str::<Value>(message) {
                if structured.is_object() {
                    return safe_login_error(&structured);
                }
            }
            message.chars().take(1_000).collect()
        }
        Value::Object(object) => {
            let code = object
                .get("code")
                .or_else(|| object.get("type"))
                .and_then(Value::as_str);
            let message = object.get("message").and_then(Value::as_str);
            let text = match (code, message) {
                (Some(code), Some(message)) if !message.contains(code) => {
                    format!("{code}: {message}")
                }
                (_, Some(message)) => message.to_string(),
                (Some(code), None) => code.to_string(),
                _ => "Codex 登录失败".to_string(),
            };
            text.chars().take(1_000).collect()
        }
        _ => "Codex 登录失败".to_string(),
    }
}

fn emit_install_event(
    events: &EventBus,
    session_ids: &[String],
    event_type: &str,
    payload: Value,
    persistent: bool,
) {
    for session_id in session_ids {
        let draft = EventDraft::new(session_id, event_type, "codex").payload(payload.clone());
        if persistent {
            events.emit(draft);
        } else {
            events.emit_ephemeral(draft);
        }
    }
}

/// 保留历史 primary/secondary 兼容形态，同时把新版多 bucket 与额度补充数据带给 UI。
fn rate_limits_snapshot(v: &Value) -> Value {
    let mut out = v.get("rateLimits").cloned().unwrap_or_else(|| v.clone());
    if let Some(obj) = out.as_object_mut() {
        for key in [
            "rateLimitsByLimitId",
            "accountId",
            "rateLimitResetCredits",
            "rateLimitUpsell",
        ] {
            if let Some(value) = v.get(key) {
                obj.insert(key.to_string(), value.clone());
            }
        }
    }
    out
}

fn merge_non_null(existing: Option<Value>, update: Value) -> Value {
    let (Some(mut old), Some(new)) = (existing, update.as_object()) else {
        return update;
    };
    let Some(old_obj) = old.as_object_mut() else {
        return update;
    };
    for (key, value) in new {
        if !value.is_null() {
            old_obj.insert(key.clone(), value.clone());
        }
    }
    old
}

async fn run_install(npm: &Path, root: &Path) -> Result<String, (String, String)> {
    if let Err(e) = std::fs::create_dir_all(root) {
        return Err((String::new(), format!("建托管目录失败: {e}")));
    }
    let args = bin::npm_install_args(root, &[bin::CODEX_NPM_SPEC, bin::COMPUTER_USE_NPM_SPEC]);
    let out = tokio::process::Command::new(npm)
        .args(&args)
        .current_dir(root)
        .output()
        .await;
    let out = match out {
        Ok(o) => o,
        Err(e) => return Err((String::new(), format!("启动 npm 失败: {e}"))),
    };
    let mut log = String::from_utf8_lossy(&out.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(log)
    } else {
        Err((
            log,
            format!("npm install 失败(退出码 {:?})", out.status.code()),
        ))
    }
}

fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // 按字符边界切,别把多字节 UTF-8 切一半。
    let start = s.len() - max;
    let start = (start..s.len())
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(s.len());
    format!("…（前段省略）\n{}", &s[start..])
}

fn now_iso() -> String {
    gend::timeutil::utc_now_iso8601()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::rpc::scripted_with_handshake;

    fn svc(f: impl Fn(&str, &Value, &Value) -> Vec<Value> + Send + Sync + 'static) -> CodexService {
        CodexService::with_transport(Arc::new(scripted_with_handshake(f)))
    }

    /// 方法名候选:第一个报「方法不存在」→ 试下一个;成功即采纳。
    #[tokio::test]
    async fn falls_back_to_next_method_name() {
        let s = svc(|method, _p, id| match method {
            "account/read" => vec![json!({ "jsonrpc": "2.0", "id": id, "error": {
                "code": -32601, "message": "Method not found"
            }})],
            "account/get" => vec![json!({ "jsonrpc": "2.0", "id": id, "result": {
                "authMode": "chatgpt", "planType": "pro", "email": "a@b.c"
            }})],
            _ => vec![],
        });
        let a = s.refresh_account().await.expect("应回落到 account/get");
        assert_eq!(a.auth_mode.as_deref(), Some("chatgpt"));
        assert_eq!(a.plan_type.as_deref(), Some("pro"));
    }

    /// 非「方法不存在」的错误必须原样上抛,不能被回落逻辑吞掉换个名字重试。
    #[tokio::test]
    async fn real_errors_are_not_swallowed_by_fallback() {
        let s = svc(|method, _p, id| {
            if method == "initialized" {
                return vec![];
            }
            if method == "account/read" {
                return vec![json!({ "jsonrpc": "2.0", "id": id, "error": {
                    "code": -32000, "message": "UsageLimitExceeded"
                }})];
            }
            panic!("不该继续试 {method}——真实错误必须首发即败");
        });
        let e = s.refresh_account().await.expect_err("应上抛真实错误");
        assert!(e.0.contains("UsageLimitExceeded"), "{e}");
    }

    /// 登录:chatgpt 流原样透传 authUrl;登出清空账户缓存。
    #[tokio::test]
    async fn chatgpt_login_passthrough_and_logout_clears_cache() {
        let s = svc(|method, params, id| match method {
            "account/login/start" => {
                assert_eq!(params["type"], "chatgpt");
                vec![json!({ "jsonrpc": "2.0", "id": id, "result": {
                    "loginId": "lg_1", "authUrl": "https://auth.openai.com/x"
                }})]
            }
            "account/logout" => vec![json!({ "jsonrpc": "2.0", "id": id, "result": {} })],
            _ => vec![],
        });
        let v = s.login("chatgpt", None).await.unwrap();
        assert_eq!(v["authUrl"], "https://auth.openai.com/x");
        s.account.lock().unwrap().auth_mode = Some("chatgpt".into());
        s.logout().await.unwrap();
        assert!(s.account_snapshot().auth_mode.is_none());
    }

    /// 模型清单走缓存:第二次不再发 RPC。
    #[tokio::test]
    async fn model_list_is_cached() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = Arc::clone(&calls);
        let s = svc(move |method, _p, id| {
            if method == "model/list" {
                c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return vec![json!({ "jsonrpc": "2.0", "id": id, "result": {
                    "data": [
                        {
                            "id": "gpt-5.6-terra", "hidden": false,
                            "defaultReasoningEffort": "high",
                            "supportedReasoningEfforts": [
                                { "reasoningEffort": "low", "description": "Fast" },
                                { "reasoningEffort": "high", "description": "Deep" }
                            ]
                        },
                        { "id": "hidden-model", "hidden": true }
                    ]
                }})];
            }
            vec![]
        });
        let a = s.models(false).await.unwrap();
        assert_eq!(a.len(), 1);
        let b = s.models(false).await.unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "第二次应命中缓存"
        );
        let cards = s.model_cards();
        assert_eq!(cards[0]["effortOptions"][1]["id"], "high");
        assert_eq!(cards[0]["defaultEffort"], "high");
    }

    #[tokio::test]
    async fn model_list_follows_next_cursor() {
        let s = svc(|method, params, id| {
            if method != "model/list" {
                return vec![];
            }
            let result = if params.get("cursor").and_then(Value::as_str) == Some("page-2") {
                json!({ "data": [{ "id": "second", "hidden": false }], "nextCursor": null })
            } else {
                json!({ "data": [{ "id": "first", "hidden": false }], "nextCursor": "page-2" })
            };
            vec![json!({ "jsonrpc": "2.0", "id": id, "result": result })]
        });
        let models = s.models(true).await.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["id"], "first");
        assert_eq!(models[1]["id"], "second");
    }

    /// codex 主动推的账户/额度通知进缓存(状态栏不必逐次 RPC)。
    #[tokio::test]
    async fn official_cancel_ignores_late_completion_error_and_keeps_real_failures() {
        let transport = scripted_with_handshake(|_method, _params, id| vec![json!({"id":id,"result":{}})]);
        let injector = transport.injector();
        let service = CodexService::with_transport(Arc::new(transport));
        service.cancel_login(Some("cancelled-login")).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        injector.push(json!({"method":"account/login/completed","params":{
            "loginId":"cancelled-login","success":false,"error":"Login was not completed"}}));
        injector.push(json!({"method":"account/rateLimits/updated","params":{
            "rateLimits":{"primary":{"usedPercent":25}}}}));
        for _ in 0..50 {
            if service.account_snapshot().rate_limits.is_some() { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(service.account_snapshot().rate_limits.is_some());
        assert!(service.account_snapshot().last_error.is_none());
        injector.push(json!({"method":"account/login/completed","params":{
            "loginId":"different-login","success":false,"error":"Authorization denied"}}));
        for _ in 0..50 {
            if service.account_snapshot().last_error.is_some() { break; }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(service.account_snapshot().last_error.as_deref(), Some("Authorization denied"));
        service.login("chatgpt", None).await.unwrap();
        assert!(service.account_snapshot().last_error.is_none());
    }

    #[tokio::test]
    async fn account_watcher_absorbs_push_notifications() {
        let t = scripted_with_handshake(|_m, _p, _id| vec![]);
        let inj = t.injector();
        let s = CodexService::with_transport(Arc::new(t));
        s.ready_client().await.unwrap();
        assert!(inj.push(
            json!({ "jsonrpc": "2.0", "method": "account/rateLimits/updated",
            "params": { "rateLimits": { "primary": { "usedPercent": 42.5 } } } })
        ));
        for _ in 0..50 {
            if s.account_snapshot().rate_limits.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let limits = s.account_snapshot().rate_limits.expect("额度应进缓存");
        assert_eq!(limits["primary"]["usedPercent"], 42.5);
    }

    #[tokio::test]
    async fn login_completion_error_survives_unauthenticated_refresh_and_clears_on_success() {
        let transport = scripted_with_handshake(|method, _params, id| {
            if method == "account/read" {
                return vec![json!({ "id": id, "result": { "account": null } })];
            }
            vec![]
        });
        let injector = transport.injector();
        let service = CodexService::with_transport(Arc::new(transport));
        service.ready_client().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        assert!(injector.push(json!({
            "method": "account/login/completed",
            "params": {
                "success": false,
                "error": {
                    "code": "AUTH_DENIED",
                    "message": "用户拒绝授权",
                    "apiKey": "sk-must-not-leak",
                    "accessToken": "secret-token"
                }
            }
        })));
        for _ in 0..50 {
            if service.account_snapshot().last_error.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let error = service.account_snapshot().last_error.unwrap();
        assert_eq!(error, "AUTH_DENIED: 用户拒绝授权");
        assert!(!error.contains("sk-must-not-leak"));
        assert!(!error.contains("secret-token"));

        let refreshed = service.refresh_account().await.unwrap();
        assert!(refreshed.auth_mode.is_none());
        assert_eq!(refreshed.last_error.as_deref(), Some(error.as_str()));

        assert!(injector.push(json!({
            "method": "account/login/completed",
            "params": { "success": true, "error": null }
        })));
        for _ in 0..50 {
            if service.account_snapshot().last_error.is_none() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(service.account_snapshot().last_error.is_none());

        merge_login_completed(
            &mut service.account.lock().unwrap_or_else(|e| e.into_inner()),
            &json!({ "success": false, "error": "temporary failure" }),
        );
        assert!(service.account_snapshot().last_error.is_some());
        assert!(injector.push(json!({
            "method": "account/updated",
            "params": { "account": { "authMode": "chatgpt", "planType": "plus" } }
        })));
        for _ in 0..50 {
            if service.account_snapshot().auth_mode.as_deref() == Some("chatgpt") {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let account = service.account_snapshot();
        assert_eq!(account.auth_mode.as_deref(), Some("chatgpt"));
        assert!(account.last_error.is_none());
    }

    /// 日志截断按字符边界切(中文日志切一半会让 status JSON 序列化炸掉)。
    #[test]
    fn tail_respects_char_boundaries() {
        let s = "安装进度".repeat(2000);
        let out = tail(&s, 100);
        assert!(out.len() <= 130, "截断后仍过长: {}", out.len());
        assert!(out.contains("前段省略"));
    }

    #[test]
    fn account_null_and_null_auth_clear_stale_login() {
        let mut account = AccountState {
            auth_mode: Some("chatgpt".into()),
            plan_type: Some("pro".into()),
            email: Some("a@b.c".into()),
            rate_limits: Some(json!({ "primary": { "usedPercent": 50 } })),
            ..Default::default()
        };
        merge_account(
            &mut account,
            &json!({ "account": null, "requiresOpenaiAuth": true }),
        );
        assert!(account.auth_mode.is_none());
        assert!(account.plan_type.is_none());
        assert!(account.rate_limits.is_none());

        merge_account(
            &mut account,
            &json!({ "account": { "type": "chatgpt", "planType": "plus", "email": null } }),
        );
        assert_eq!(account.auth_mode.as_deref(), Some("chatgpt"));
        assert_eq!(account.plan_type.as_deref(), Some("plus"));
        merge_account(&mut account, &json!({ "authMode": null, "planType": null }));
        assert!(account.auth_mode.is_none());
    }

    #[test]
    fn rate_limit_snapshot_keeps_new_multi_bucket_data() {
        let v = rate_limits_snapshot(&json!({
            "rateLimits": { "primary": { "usedPercent": 12 } },
            "rateLimitsByLimitId": {
                "codex": { "primary": { "usedPercent": 34 } }
            }
        }));
        assert_eq!(v["primary"]["usedPercent"], 12);
        assert_eq!(
            v["rateLimitsByLimitId"]["codex"]["primary"]["usedPercent"],
            34
        );
    }
}
