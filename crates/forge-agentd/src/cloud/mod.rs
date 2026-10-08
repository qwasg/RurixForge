//! 云模式(15_CLOUD_SERVICE.md §8):forge-cloud 账号 BFF、模型目录缓存、资料同步。
//!
//! 模块分工:
//! - 本文件:`CloudService` 门面(登录态、令牌续期、带鉴权的云端调用、模型目录缓存)。
//!   它的公开 API 是 crate 内其它模块(llm / agent / codex / memory / skills / sync)共同依赖的契约,
//!   签名保持稳定;
//! - [rest]:`/api/forge/account/*`(settings 与 sync 两组除外);
//! - [sync]:设置 / 记忆 / 技能同步引擎,以及 `/api/forge/account/settings*`、`/api/forge/account/sync`。
//!
//! 令牌纪律(R-5 延伸):refresh token 与设备 Key 只进 keystore,access token 只在内存;
//! 任何 HTTP 响应、事件、日志都不得出现这三者。

pub mod rest;
pub mod sync;

mod auth;
mod catalog;
pub(crate) mod client;
pub(crate) mod config;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use client::{Reply, Request};
use config::{CloudConfig, CloudState, LastError, Paths};

pub(crate) use catalog::{context_label, snapshot_cards};
pub(crate) use config::{byo_allowed, dev_mock_enabled};

/// keystore 条目:refresh token。
pub(crate) const REFRESH_KEY_ID: &str = "cloud:refresh";
/// keystore 条目:设备 API Key(与 gend embedding 回落读同一条目)。
pub(crate) const DEVICE_KEY_ID: &str = gend::embed::CLOUD_DEVICE_KEY_ID;
/// access token 提前续期的余量(秒)。
const ACCESS_MARGIN_SECS: u64 = 30;
/// 可达性探测结果的有效期。
const REACHABLE_TTL: Duration = Duration::from_secs(30);
/// 余额缓存超过此时长,状态查询顺手后台刷新一次。
const BALANCE_TTL: Duration = Duration::from_secs(60);

/// 云端调用错误。`code` 取值见 15 §8.2(`CLOUD_LOGIN_REQUIRED` / `CLOUD_UNAUTHORIZED` /
/// `CLOUD_UNREACHABLE`)或云端透传的业务码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl CloudError {
    pub fn new(status: u16, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn login_required() -> Self {
        Self::new(401, "CLOUD_LOGIN_REQUIRED", "尚未登录 RurixForge 云账号")
    }

    pub fn unreachable(detail: impl std::fmt::Display) -> Self {
        Self::new(502, "CLOUD_UNREACHABLE", format!("云端不可达:{detail}"))
    }

    /// 续期失败(refresh 会话已失效),本地已登出。
    pub fn unauthorized() -> Self {
        Self::new(401, "CLOUD_UNAUTHORIZED", "云账号登录已失效,请重新登录")
    }
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CloudError {}

fn io_error(e: std::io::Error) -> CloudError {
    CloudError::new(500, "FORGE_IO", format!("本地令牌存储失败:{e}"))
}

/// 云端模型能力位(`/api/v1/models/catalog` 的 capabilities)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogCapabilities {
    pub vision: bool,
    pub reasoning_efforts: Vec<String>,
    pub thinking_mode: String,
    pub thinking_always_on: bool,
    pub context_window: u64,
    pub max_output: u64,
    pub tools: bool,
    pub responses: bool,
}

/// 单价(micros / 1M tokens,已乘分组倍率)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogPricing {
    #[serde(rename = "inputPer1M")]
    pub input_per_1m: i64,
    #[serde(rename = "outputPer1M")]
    pub output_per_1m: i64,
    #[serde(rename = "cacheReadPer1M")]
    pub cache_read_per_1m: i64,
    #[serde(rename = "cacheWritePer1M")]
    pub cache_write_per_1m: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogModel {
    pub id: String,
    pub display_name: String,
    pub platform: String,
    pub capabilities: CatalogCapabilities,
    pub pricing: CatalogPricing,
    pub available: bool,
}

/// 云端模型目录(`GET /api/v1/models/catalog`)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Catalog {
    pub default_model: String,
    pub currency: String,
    pub rate_multiplier: f64,
    pub models: Vec<CatalogModel>,
}

impl Catalog {
    pub fn find(&self, id: &str) -> Option<&CatalogModel> {
        self.models.iter().find(|m| m.id == id)
    }
}

/// 资料同步类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncKind {
    Settings,
    Memory,
    Skills,
}

/// 令牌落点:生产 = keystore.json(Windows DPAPI);单测 = 进程内存。
enum SecretStore {
    Memory(StdMutex<HashMap<String, String>>),
    Keystore(PathBuf),
}

impl SecretStore {
    fn get(&self, id: &str) -> Option<String> {
        match self {
            SecretStore::Memory(m) => m.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned(),
            SecretStore::Keystore(p) => gend::keystore::Keystore::load_from(p).secret_for(id),
        }
    }

    fn set(&self, id: &str, value: &str) -> std::io::Result<()> {
        match self {
            SecretStore::Memory(m) => {
                m.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id.to_string(), value.to_string());
                Ok(())
            }
            SecretStore::Keystore(p) => gend::keystore::set_key_at(p, id, value),
        }
    }

    fn remove(&self, id: &str) -> std::io::Result<()> {
        match self {
            SecretStore::Memory(m) => {
                m.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
                Ok(())
            }
            SecretStore::Keystore(p) => gend::keystore::remove_key_at(p, id).map(|_| ()),
        }
    }
}

/// 可变状态(一把锁;锁内不 await)。
struct Inner {
    config: CloudConfig,
    state: CloudState,
    refresh_token: Option<String>,
    device_key: Option<String>,
    /// (access token, 到期 Unix 秒)。
    access: Option<(String, u64)>,
    catalog_at: Option<Instant>,
    balance_at: Option<Instant>,
    reachable: Option<(bool, Instant)>,
}

impl Inner {
    fn logged_in(&self) -> bool {
        self.refresh_token.is_some() && self.device_key.is_some()
    }
}

/// `POST /api/forge/account/config` 入参(缺省字段不变)。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigPatch {
    #[serde(default)]
    pub server_url: Option<String>,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub sync: Option<SyncPatch>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct SyncPatch {
    #[serde(default)]
    pub settings: Option<bool>,
    #[serde(default)]
    pub memory: Option<bool>,
    #[serde(default)]
    pub skills: Option<bool>,
}

/// 云模式门面。
pub struct CloudService {
    /// None = 纯内存(单测);Some = 读写 data 目录与 keystore。
    paths: Option<Paths>,
    secrets: SecretStore,
    inner: StdMutex<Inner>,
    /// 续期串行化:云端对「用已轮换掉的旧 refresh token」判复用并吊销整个会话,
    /// 并发续期必须排队,后到者复用先到者换回的新令牌。
    refresh_lock: tokio::sync::Mutex<()>,
    generation: AtomicU64,
    balance_inflight: AtomicBool,
    key_check_inflight: AtomicBool,
    catalog_inflight: AtomicBool,
}

impl Default for CloudService {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudService {
    /// 内存态、未登录、不读盘(单测构造 AppState 用)。
    pub fn new() -> Self {
        let config = CloudConfig {
            device_id: config::new_device_id(),
            ..Default::default()
        };
        Self::assemble(
            None,
            SecretStore::Memory(StdMutex::new(HashMap::new())),
            config,
            CloudState::default(),
        )
    }

    /// 从数据目录加载配置与登录态(生产入口,经 [global])。
    pub fn load() -> Self {
        Self::load_from(Paths::production())
    }

    pub(crate) fn load_from(paths: Paths) -> Self {
        let mut cfg = config::load_config(&paths.data_root);
        if cfg.device_id.trim().is_empty() {
            cfg.device_id = config::new_device_id();
            if let Err(e) = config::save_config(&paths.data_root, &cfg) {
                eprintln!("[cloud] 写 cloud-config.json 失败:{e}");
            }
        }
        let state = config::load_state(&paths.data_root);
        let secrets = SecretStore::Keystore(paths.keystore.clone());
        let refresh = secrets.get(REFRESH_KEY_ID);
        let device_key = secrets.get(DEVICE_KEY_ID);
        let svc = Self::assemble(Some(paths), secrets, cfg, state);
        {
            let mut inner = svc.lock();
            inner.refresh_token = refresh;
            inner.device_key = device_key;
        }
        svc
    }

    fn assemble(
        paths: Option<Paths>,
        secrets: SecretStore,
        cfg: CloudConfig,
        state: CloudState,
    ) -> Self {
        CloudService {
            paths,
            secrets,
            inner: StdMutex::new(Inner {
                config: cfg,
                state,
                refresh_token: None,
                device_key: None,
                access: None,
                catalog_at: None,
                balance_at: None,
                reachable: None,
            }),
            refresh_lock: tokio::sync::Mutex::new(()),
            generation: AtomicU64::new(0),
            balance_inflight: AtomicBool::new(false),
            key_check_inflight: AtomicBool::new(false),
            catalog_inflight: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist_state(&self, inner: &Inner) {
        if let Some(p) = &self.paths {
            if let Err(e) = config::save_state(&p.data_root, &inner.state) {
                eprintln!("[cloud] 写 cloud-state.json 失败:{e}");
            }
        }
    }

    /// 是否已登录(持有 refresh token 与设备 Key)。
    pub fn is_logged_in(&self) -> bool {
        self.lock().logged_in()
    }

    /// forge-cloud 根地址(无尾斜杠)。
    pub fn server_url(&self) -> String {
        self.lock().config.effective_server_url()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server_url())
    }

    /// 设备 API Key(本地引擎与 Codex 走网关用;未登录 None)。只在 agentd 进程内使用。
    pub fn device_key(&self) -> Option<String> {
        let inner = self.lock();
        inner.refresh_token.as_ref().and(inner.device_key.clone())
    }

    /// 已登录用户摘要(云端 `User` 形状;未登录 None)。
    pub fn user_summary(&self) -> Option<Value> {
        let inner = self.lock();
        if inner.logged_in() {
            inner.state.user.clone()
        } else {
            None
        }
    }

    /// 登录代次:每次登录/登出 +1。同步引擎据此发现「换了账号」并触发全量同步。
    pub fn login_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// 带 JWT 的云端 JSON 调用。`path` 形如 `/api/v1/me/memories?since=0`。
    /// access token 过期或遇 401 时自动续期一次;续期失败 → 本地登出并返回 `CLOUD_UNAUTHORIZED`。
    pub async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, CloudError> {
        let reply = self.call_raw(method, path, body).await?;
        if reply.ok() {
            reply.json()
        } else {
            Err(reply.to_error())
        }
    }

    /// [call] 的原始回包版本(任意状态码原样返回;头像这类二进制也走它)。
    pub(crate) async fn call_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Reply, CloudError> {
        if !self.is_logged_in() {
            return Err(CloudError::login_required());
        }
        let token = self.access_token().await?;
        let reply = self
            .send(
                Request::new(method, self.url(path))
                    .bearer(Some(token.clone()))
                    .json(body.clone()),
            )
            .await?;
        if reply.status != 401 || !is_token_rejection(&reply) {
            return Ok(reply);
        }
        let token = self.force_refresh(&token).await?;
        self.send(
            Request::new(method, self.url(path))
                .bearer(Some(token))
                .json(body),
        )
        .await
    }

    /// 公开接口(无 JWT):auth/config、email-code、plans、password/reset。
    pub(crate) async fn call_public(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Reply, CloudError> {
        self.send(Request::new(method, self.url(path)).json(body))
            .await
    }

    async fn send(&self, req: Request) -> Result<Reply, CloudError> {
        let result = client::send(req).await;
        let reachable = !matches!(&result, Err(e) if e.code == "CLOUD_UNREACHABLE");
        self.lock().reachable = Some((reachable, Instant::now()));
        result
    }

    fn valid_access(&self) -> Option<String> {
        let inner = self.lock();
        if !inner.logged_in() {
            return None;
        }
        inner
            .access
            .as_ref()
            .filter(|(_, exp)| *exp > auth::now_unix() + ACCESS_MARGIN_SECS)
            .map(|(t, _)| t.clone())
    }

    async fn access_token(&self) -> Result<String, CloudError> {
        if let Some(t) = self.valid_access() {
            return Ok(t);
        }
        let _g = self.refresh_lock.lock().await;
        if let Some(t) = self.valid_access() {
            return Ok(t);
        }
        self.refresh_locked().await
    }

    /// `stale` 被云端拒了:若排队期间别人已换回新令牌就直接用,否则亲自续期。
    async fn force_refresh(&self, stale: &str) -> Result<String, CloudError> {
        let _g = self.refresh_lock.lock().await;
        if let Some(t) = self.valid_access().filter(|t| t != stale) {
            return Ok(t);
        }
        self.refresh_locked().await
    }

    /// 调用方须持 `refresh_lock`。
    async fn refresh_locked(&self) -> Result<String, CloudError> {
        let (refresh, gen) = {
            let inner = self.lock();
            match (&inner.refresh_token, inner.logged_in()) {
                (Some(rt), true) => (rt.clone(), self.login_generation()),
                _ => return Err(CloudError::login_required()),
            }
        };
        let reply = self
            .send(
                Request::new("POST", self.url("/api/v1/auth/refresh"))
                    .json(Some(json!({ "refreshToken": refresh }))),
            )
            .await?;
        if reply.status == 401 {
            // REFRESH_INVALID / REFRESH_REUSED:refresh 会话已死,只能重新登录。
            let code = reply
                .error_code()
                .unwrap_or_else(|| "UNAUTHORIZED".to_string());
            if self.login_generation() == gen {
                self.local_logout(Some(last_error(
                    "CLOUD_UNAUTHORIZED",
                    &format!("登录已失效({code}),请重新登录"),
                )));
            }
            return Err(CloudError::unauthorized());
        }
        if !reply.ok() {
            return Err(reply.to_error());
        }
        let pair = auth::parse_token_pair(&reply.json()?)?;
        let mut inner = self.lock();
        if self.login_generation() != gen || !inner.logged_in() {
            // 续期途中已登出/换号:旧会话换回的令牌作废,不写回。
            return Err(CloudError::login_required());
        }
        // 盘写在锁内:与 local_logout 的「清内存 + 删盘」互斥,登出后不会被写回复活。
        if let Err(e) = self.secrets.set(REFRESH_KEY_ID, &pair.refresh_token) {
            eprintln!("[cloud] 轮换后的 refresh token 落盘失败(本进程内仍可用):{e}");
        }
        inner.refresh_token = Some(pair.refresh_token);
        inner.access = Some((pair.access_token.clone(), pair.access_expires_at));
        let sid = auth::session_id_of(&pair.access_token);
        if sid.is_some() && inner.state.session_id != sid {
            inner.state.session_id = sid;
            self.persist_state(&inner);
        }
        Ok(pair.access_token)
    }

    fn device_info(&self) -> Value {
        let inner = self.lock();
        json!({
            "id": inner.config.device_id,
            "name": inner.config.effective_device_name(),
            "platform": config::platform(),
            "appVersion": env!("CARGO_PKG_VERSION"),
        })
    }

    /// 邮箱密码登录(签发设备 Key)。
    pub(crate) async fn login(&self, email: &str, password: &str) -> Result<(), CloudError> {
        let body = json!({
            "email": email,
            "password": password,
            "device": self.device_info(),
            "issueDeviceKey": true,
        });
        self.authenticate("/api/v1/auth/login", body).await
    }

    /// 注册即登录。`fields` 为白名单字段(email/password/nickname/inviteCode/emailCode)。
    pub(crate) async fn register(
        &self,
        fields: serde_json::Map<String, Value>,
    ) -> Result<(), CloudError> {
        let mut body = Value::Object(fields);
        body["device"] = self.device_info();
        body["issueDeviceKey"] = json!(true);
        self.authenticate("/api/v1/auth/register", body).await
    }

    async fn authenticate(&self, path: &str, body: Value) -> Result<(), CloudError> {
        let reply = self.call_public("POST", path, Some(body)).await?;
        if !reply.ok() {
            return Err(reply.to_error());
        }
        let grant = auth::parse_login(&reply.json()?)?;
        self.accept_login(grant)
    }

    fn accept_login(&self, grant: auth::LoginGrant) -> Result<(), CloudError> {
        let mut inner = self.lock();
        self.secrets
            .set(REFRESH_KEY_ID, &grant.tokens.refresh_token)
            .map_err(io_error)?;
        if let Err(e) = self.secrets.set(DEVICE_KEY_ID, &grant.device_key) {
            let _ = self.secrets.remove(REFRESH_KEY_ID);
            return Err(io_error(e));
        }
        let server_url = inner.config.effective_server_url();
        let balance = grant.user.get("balanceMicros").and_then(Value::as_i64);
        let session_id = auth::session_id_of(&grant.tokens.access_token);
        inner.refresh_token = Some(grant.tokens.refresh_token);
        inner.device_key = Some(grant.device_key);
        inner.access = Some((grant.tokens.access_token, grant.tokens.access_expires_at));
        let previous_catalog = inner.state.catalog.take();
        let currency = inner.state.currency.take();
        inner.state = CloudState {
            user: Some(grant.user),
            device_key_prefix: Some(grant.device_key_prefix),
            session_id,
            server_url: Some(server_url),
            balance_micros: balance,
            currency,
            subscriptions: Vec::new(),
            last_error: None,
            // 换号后旧目录的分组价未必适用:保留作展示,但立即判过期重拉。
            catalog: previous_catalog,
            logged_in_at: Some(gend::timeutil::utc_now_iso8601()),
        };
        inner.catalog_at = None;
        inner.balance_at = Some(Instant::now());
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.persist_state(&inner);
        Ok(())
    }

    /// 云端登出(尽力)+ 本地清令牌(无论云端是否可达)。
    pub(crate) async fn logout(&self) {
        if self.is_logged_in() {
            if let Some(token) = self.valid_access() {
                let _ = self
                    .send(
                        Request::new("POST", self.url("/api/v1/auth/logout"))
                            .bearer(Some(token))
                            .timeout(Duration::from_secs(5)),
                    )
                    .await;
            } else if let Ok(token) = self.access_token().await {
                let _ = self
                    .send(
                        Request::new("POST", self.url("/api/v1/auth/logout"))
                            .bearer(Some(token))
                            .timeout(Duration::from_secs(5)),
                    )
                    .await;
            }
        }
        self.local_logout(None);
    }

    /// 本地登出:清令牌与用户态(模型目录保留作 needs-login 展示),代次 +1。
    pub(crate) fn local_logout(&self, error: Option<LastError>) {
        let mut inner = self.lock();
        inner.refresh_token = None;
        inner.device_key = None;
        inner.access = None;
        inner.catalog_at = None;
        inner.balance_at = None;
        let st = &mut inner.state;
        st.user = None;
        st.device_key_prefix = None;
        st.session_id = None;
        st.server_url = None;
        st.balance_micros = None;
        st.subscriptions.clear();
        st.logged_in_at = None;
        st.last_error = error;
        self.generation.fetch_add(1, Ordering::SeqCst);
        for id in [REFRESH_KEY_ID, DEVICE_KEY_ID] {
            if let Err(e) = self.secrets.remove(id) {
                eprintln!("[cloud] 清除本地令牌 {id} 失败:{e}");
            }
        }
        self.persist_state(&inner);
        drop(inner);
        self.write_embedding_descriptor(None);
    }

    /// 本机登录会话 ID(删设备时识别「删的是本机」)。
    pub(crate) fn current_session_id(&self) -> Option<String> {
        let inner = self.lock();
        if inner.logged_in() {
            inner.state.session_id.clone()
        } else {
            None
        }
    }

    /// 模型目录(缓存 5 分钟;未登录 None;拉取失败时返回旧缓存)。
    pub async fn catalog(&self) -> Option<Catalog> {
        match self.fetch_catalog(false).await {
            Ok(c) => Some(c),
            Err(_) => self.catalog_cached(),
        }
    }

    /// 拉目录(`force` 忽略缓存);错误原样返回(REST 面要如实报错)。
    pub(crate) async fn fetch_catalog(&self, force: bool) -> Result<Catalog, CloudError> {
        if !self.is_logged_in() {
            return Err(CloudError::login_required());
        }
        if !force {
            let inner = self.lock();
            if let (Some(at), Some(c)) = (inner.catalog_at, &inner.state.catalog) {
                if at.elapsed() < catalog::CATALOG_TTL {
                    return Ok(c.clone());
                }
            }
        }
        let gen = self.login_generation();
        let v = self.call("GET", "/api/v1/models/catalog", None).await?;
        let fetched: Catalog = serde_json::from_value(v).map_err(|e| {
            CloudError::new(502, "CLOUD_BAD_RESPONSE", format!("模型目录解析失败:{e}"))
        })?;
        {
            let mut inner = self.lock();
            if self.login_generation() != gen || !inner.logged_in() {
                return Err(CloudError::login_required());
            }
            if !fetched.currency.is_empty() {
                inner.state.currency = Some(fetched.currency.clone());
            }
            inner.state.catalog = Some(fetched.clone());
            inner.catalog_at = Some(Instant::now());
            self.persist_state(&inner);
        }
        self.write_embedding_descriptor(fetched.embedding_model());
        Ok(fetched)
    }

    /// 已缓存的模型目录(不触网;同步上下文用)。
    pub fn catalog_cached(&self) -> Option<Catalog> {
        let inner = self.lock();
        if inner.logged_in() {
            inner.state.catalog.clone()
        } else {
            None
        }
    }

    /// 最近一次拉到的目录(不论登录与否;design-snapshot 的 needs-login 展示用)。
    pub(crate) fn last_catalog(&self) -> Option<Catalog> {
        self.lock().state.catalog.clone()
    }

    /// 目录缺失或过期时后台刷新(不阻塞调用方)。
    pub(crate) fn refresh_catalog_soon(self: &Arc<Self>) {
        let stale = {
            let inner = self.lock();
            inner.logged_in()
                && inner
                    .catalog_at
                    .map_or(true, |at| at.elapsed() >= catalog::CATALOG_TTL)
        };
        if !stale || self.catalog_inflight.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            self.catalog_inflight.store(false, Ordering::SeqCst);
            return;
        };
        let me = Arc::clone(self);
        rt.spawn(async move {
            let _ = me.fetch_catalog(false).await;
            me.catalog_inflight.store(false, Ordering::SeqCst);
        });
    }

    /// MCP 子进程的 embedding 回落描述(gend `cloud-embedding.json`);None = 删除。
    fn write_embedding_descriptor(&self, model: Option<String>) {
        let Some(paths) = &self.paths else {
            return;
        };
        let result = match model.filter(|_| self.is_logged_in()) {
            Some(model) => gend::embed::save_cloud_embedding_in(
                &paths.gen_data,
                &gend::embed::CloudEmbeddingFile {
                    base_url: self.server_url(),
                    model,
                },
            ),
            None => gend::embed::clear_cloud_embedding_in(&paths.gen_data),
        };
        if let Err(e) = result {
            eprintln!("[cloud] 更新 cloud-embedding.json 失败:{e}");
        }
    }

    /// 某类资料同步是否开启(已登录且 cloud-config 开关为真)。
    pub fn sync_enabled(&self, kind: SyncKind) -> bool {
        let inner = self.lock();
        let t = inner.config.sync;
        inner.logged_in()
            && match kind {
                SyncKind::Settings => t.settings,
                SyncKind::Memory => t.memory,
                SyncKind::Skills => t.skills,
            }
    }

    /// 轮末异步刷新余额缓存(不阻塞调用方)。
    pub fn refresh_balance_soon(self: &Arc<Self>) {
        if !self.is_logged_in() || self.balance_inflight.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            self.balance_inflight.store(false, Ordering::SeqCst);
            return;
        };
        let me = Arc::clone(self);
        rt.spawn(async move {
            // 网关在流结束后才结算,稍等再拉,免得拿到扣费前的余额。
            tokio::time::sleep(Duration::from_millis(800)).await;
            if let Ok(v) = me.call("GET", "/api/v1/me/balance", None).await {
                me.absorb_balance(&v);
            }
            me.balance_inflight.store(false, Ordering::SeqCst);
        });
    }

    /// 网关拒了设备 Key(`invalid_api_key`):记 lastError 并后台核对会话。
    /// 会话已死 → 续期失败自动登出;会话还活着 → 设备 Key 只在登录时签发,同样登出引导重新登录;
    /// 云端不可达 → 保持现状。
    pub(crate) fn note_device_key_rejected(self: &Arc<Self>) {
        if !self.is_logged_in() {
            return;
        }
        {
            let mut inner = self.lock();
            inner.state.last_error = Some(last_error(
                "CLOUD_UNAUTHORIZED",
                "设备 Key 被云端拒绝,正在核对登录状态",
            ));
            self.persist_state(&inner);
        }
        if self.key_check_inflight.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            self.key_check_inflight.store(false, Ordering::SeqCst);
            return;
        };
        let me = Arc::clone(self);
        let gen = self.login_generation();
        rt.spawn(async move {
            match me.call("GET", "/api/v1/me", None).await {
                Err(e) if e.code == "CLOUD_UNREACHABLE" || e.code == "CLOUD_UNAUTHORIZED" => {}
                _ if me.login_generation() == gen => me.local_logout(Some(last_error(
                    "CLOUD_UNAUTHORIZED",
                    "设备 Key 已失效,请重新登录",
                ))),
                _ => {}
            }
            me.key_check_inflight.store(false, Ordering::SeqCst);
        });
    }

    /// `User` 形状的回包(profile / avatar / me)并进缓存。
    pub(crate) fn absorb_user(&self, user: &Value) {
        if !user.is_object() {
            return;
        }
        let mut inner = self.lock();
        if !inner.logged_in() {
            return;
        }
        if let Some(b) = user.get("balanceMicros").and_then(Value::as_i64) {
            inner.state.balance_micros = Some(b);
        }
        inner.state.user = Some(user.clone());
        self.persist_state(&inner);
    }

    /// `/me/balance` 或 `/redeem` 回包并进缓存。
    pub(crate) fn absorb_balance(&self, v: &Value) {
        let mut inner = self.lock();
        if !inner.logged_in() {
            return;
        }
        if let Some(b) = v.get("balanceMicros").and_then(Value::as_i64) {
            inner.state.balance_micros = Some(b);
            if let Some(u) = inner.state.user.as_mut().and_then(Value::as_object_mut) {
                u.insert("balanceMicros".into(), json!(b));
            }
        }
        if let Some(c) = v
            .get("currency")
            .and_then(Value::as_str)
            .filter(|c| !c.is_empty())
        {
            inner.state.currency = Some(c.to_string());
        }
        if let Some(subs) = v
            .get("subscriptions")
            .or_else(|| v.get("items"))
            .and_then(Value::as_array)
        {
            inner.state.subscriptions = subs.clone();
        }
        inner.balance_at = Some(Instant::now());
        self.persist_state(&inner);
    }

    /// `/me` 回包(`{user, subscriptions, currency}`)。
    pub(crate) fn absorb_me(&self, v: &Value) {
        if let Some(u) = v.get("user") {
            self.absorb_user(u);
        }
        self.absorb_balance(&json!({
            "currency": v.get("currency"),
            "subscriptions": v.get("subscriptions"),
            "balanceMicros": v.pointer("/user/balanceMicros"),
        }));
    }

    /// 会员变更(购买档位 / 取消预约,15 §11.4)后作废余额缓存:回包 `Membership` 里的新余额立即并入,
    /// 订阅列表判过期并后台重拉——`/status` 不必等 60 秒 TTL 才看到扣款、退款与新订阅。
    pub(crate) fn invalidate_balance(self: &Arc<Self>, membership: Option<&Value>) {
        if let Some(m) = membership {
            // 只取余额与币种:Membership 没有 subscriptions 数组,订阅交给下面的重拉。
            self.absorb_balance(&json!({
                "balanceMicros": m.get("balanceMicros"),
                "currency": m.get("currency"),
            }));
        }
        self.lock().balance_at = None;
        self.refresh_balance_soon();
    }

    fn balance_stale(&self) -> bool {
        let inner = self.lock();
        inner.logged_in()
            && inner
                .balance_at
                .map_or(true, |at| at.elapsed() >= BALANCE_TTL)
    }

    /// 可达性(30 秒内的探测结果直接复用;否则 3 秒超时探 `/healthz`)。
    pub(crate) async fn probe_reachable(&self) -> bool {
        let fresh = self
            .lock()
            .reachable
            .filter(|(_, at)| at.elapsed() < REACHABLE_TTL);
        if let Some((r, _)) = fresh {
            return r;
        }
        self.send(Request::new("GET", self.url("/healthz")).timeout(Duration::from_secs(3)))
            .await
            .is_ok()
    }

    /// `GET /status` 完整负载(15 §8.2);顺手后台刷新过期的余额与目录。
    pub(crate) async fn status(self: &Arc<Self>) -> Value {
        self.probe_reachable().await;
        if self.balance_stale() {
            self.refresh_balance_soon();
        }
        self.refresh_catalog_soon();
        self.status_json()
    }

    /// 状态 JSON(不触网)。令牌三件套一律不出现。
    pub(crate) fn status_json(&self) -> Value {
        let byo_allowed = byo_allowed();
        let byo_configured = byo_configured();
        let inner = self.lock();
        let logged_in = inner.logged_in();
        let st = &inner.state;
        let user = if logged_in { st.user.clone() } else { None };
        let balance = st
            .balance_micros
            .or_else(|| user.as_ref()?.get("balanceMicros")?.as_i64())
            .filter(|_| logged_in)
            .unwrap_or(0);
        let t = inner.config.sync;
        json!({
            "serverUrl": inner.config.effective_server_url(),
            "loggedIn": logged_in,
            "reachable": inner.reachable.map(|(r, _)| r).unwrap_or(true),
            "user": user,
            "balanceMicros": balance,
            "currency": st.currency.clone().unwrap_or_else(|| "USD".to_string()),
            "subscriptions": if logged_in { st.subscriptions.clone() } else { Vec::new() },
            "deviceKeyPrefix": if logged_in { st.device_key_prefix.clone() } else { None },
            "byoAllowed": byo_allowed,
            "byoConfigured": byo_configured,
            "devMock": dev_mock_enabled(),
            "sync": {
                "settings": t.settings,
                "memory": t.memory,
                "skills": t.skills,
                "lastSyncAt": Value::Null,
                "lastError": Value::Null,
            },
            "lastError": st.last_error,
        })
    }

    /// design-snapshot 顶层 `account` 摘要(15 §8.4)。
    pub(crate) fn account_summary(&self) -> Value {
        let inner = self.lock();
        let logged_in = inner.logged_in();
        let user = inner.state.user.as_ref().filter(|_| logged_in);
        let field = |k: &str| user.and_then(|u| u.get(k)).cloned().unwrap_or(Value::Null);
        json!({
            "loggedIn": logged_in,
            "nickname": field("nickname"),
            "email": field("email"),
            "balanceMicros": if logged_in {
                inner.state.balance_micros.or_else(|| user?.get("balanceMicros")?.as_i64()).unwrap_or(0)
            } else {
                0
            },
            "currency": inner.state.currency.clone().unwrap_or_else(|| "USD".to_string()),
            "hasAvatar": user.and_then(|u| u.get("hasAvatar")).and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// `GET /config` 负载。
    pub(crate) fn config_json(&self) -> Value {
        let inner = self.lock();
        json!({
            "serverUrl": inner.config.effective_server_url(),
            "deviceId": inner.config.device_id,
            "deviceName": inner.config.effective_device_name(),
            "sync": inner.config.sync,
        })
    }

    /// `POST /config`:已登录时改 serverUrl → 409 LOGGED_IN。
    pub(crate) fn update_config(&self, patch: ConfigPatch) -> Result<Value, CloudError> {
        {
            let mut inner = self.lock();
            let mut cfg = inner.config.clone();
            let mut server_changed = false;
            if let Some(raw) = patch.server_url {
                let url = config::normalize_server_url(&raw);
                if !url.is_empty() && !(url.starts_with("http://") || url.starts_with("https://")) {
                    return Err(CloudError::new(
                        400,
                        "INVALID_SERVER_URL",
                        "serverUrl 须以 http:// 或 https:// 开头",
                    ));
                }
                let next = if url.is_empty() {
                    config::default_server_url()
                } else {
                    url.clone()
                };
                server_changed = next != cfg.effective_server_url();
                if server_changed && inner.logged_in() {
                    return Err(CloudError::new(
                        409,
                        "LOGGED_IN",
                        "已登录时不能修改服务端地址,请先登出",
                    ));
                }
                cfg.server_url = url;
            }
            if let Some(name) = patch.device_name {
                let name = name.trim();
                if name.chars().count() > 64 {
                    return Err(CloudError::new(
                        400,
                        "INVALID_DEVICE_NAME",
                        "设备名不超过 64 字",
                    ));
                }
                cfg.device_name = name.to_string();
            }
            if let Some(s) = patch.sync {
                cfg.sync.settings = s.settings.unwrap_or(cfg.sync.settings);
                cfg.sync.memory = s.memory.unwrap_or(cfg.sync.memory);
                cfg.sync.skills = s.skills.unwrap_or(cfg.sync.skills);
            }
            if let Some(p) = &self.paths {
                config::save_config(&p.data_root, &cfg).map_err(|e| {
                    CloudError::new(500, "FORGE_IO", format!("写 cloud-config.json 失败:{e}"))
                })?;
            }
            inner.config = cfg;
            if server_changed {
                // 旧服务端的目录与可达性不再适用。
                inner.state.catalog = None;
                inner.catalog_at = None;
                inner.reachable = None;
                self.persist_state(&inner);
            }
        }
        Ok(self.config_json())
    }
}

/// 401 是否是「令牌不认」(JWT 中间件口径 `UNAUTHORIZED`;无结构体也按令牌问题处理)。
/// 其它 401(如改密码时旧密码错)是业务错误,不能触发续期重放。
fn is_token_rejection(reply: &Reply) -> bool {
    match reply.error_code() {
        None => true,
        Some(code) => matches!(
            code.as_str(),
            "UNAUTHORIZED" | "TOKEN_EXPIRED" | "INVALID_TOKEN"
        ),
    }
}

fn last_error(code: &str, message: &str) -> LastError {
    LastError {
        code: code.to_string(),
        message: message.to_string(),
        at: gend::timeutil::utc_now_iso8601(),
    }
}

/// A connected local provider or official subscription satisfies the local account gate.
pub(crate) fn byo_configured() -> bool {
    byo_allowed()
        && (crate::llm::deepseek_key_available() || crate::llm::openai_compat_status().configured
            || crate::channels::ready("kimi") || crate::channels::ready("glm") || crate::channels::ready("antigravity"))
}

/// 进程级单例(生产路径;AppState.cloud 与无 state 的调用点共用同一实例)。
/// 单测进程恒为内存态未登录实例,绝不读写开发机的真实 data 目录与 keystore。
pub fn global() -> Arc<CloudService> {
    static CELL: OnceLock<Arc<CloudService>> = OnceLock::new();
    Arc::clone(CELL.get_or_init(|| {
        if cfg!(test) {
            Arc::new(CloudService::new())
        } else {
            Arc::new(CloudService::load())
        }
    }))
}

#[cfg(test)]
impl CloudService {
    /// 单测:已登录的内存实例(access token 远期有效,目录已缓存)。
    pub(crate) fn test_logged_in(
        server_url: &str,
        device_key: &str,
        catalog: Option<Catalog>,
    ) -> Self {
        let svc = CloudService::new();
        {
            let mut inner = svc.lock();
            inner.config.server_url = server_url.to_string();
            inner.refresh_token = Some("rt_test".to_string());
            inner.device_key = Some(device_key.to_string());
            inner.access = Some(("access-test".to_string(), auth::now_unix() + 3600));
            inner.state.user = Some(json!({
                "id": 1, "email": "tester@example.com", "nickname": "tester",
                "hasAvatar": false, "balanceMicros": 1_000_000
            }));
            inner.state.device_key_prefix = Some(device_key.chars().take(10).collect());
            inner.state.currency = Some("USD".to_string());
            inner.catalog_at = catalog.as_ref().map(|_| Instant::now());
            inner.state.catalog = catalog;
        }
        svc.generation.fetch_add(1, Ordering::SeqCst);
        svc
    }

    pub(crate) fn expire_access_for_test(&self) {
        if let Some((_, exp)) = self.lock().access.as_mut() {
            *exp = 0;
        }
    }

    pub(crate) fn secret_for_test(&self, id: &str) -> Option<String> {
        self.secrets.get(id)
    }
}
