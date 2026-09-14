//! `codex app-server` 的 JSON-RPC / stdio 客户端。
//!
//! 协议要点(与本仓 [mcp.rs](crates/forge-agentd/src/mcp.rs) 的 MCP 客户端不同,不能复用):
//! - 双向:除「客户端请求 → 服务端响应」外,服务端还会**主动发请求**(审批、要用户输入、
//!   调 dynamicTools),客户端必须回响应,否则那一轮永久挂住;
//! - 通知按 `threadId` 归属某个线程,一个进程可同时跑多个会话的线程,所以读循环必须
//!   按 threadId 扇出到订阅者,而不是像 MCP 那样「发一条读一条」;
//! - 因此这里是「单读循环 + pending id 表 + 订阅路由」的结构。
//!
//! 传输抽象 [`CodexTransport`] 的唯一目的是单测:本仓纪律是单测不触网、不起子进程,
//! 所有映射/审批/中断逻辑都用 [`ScriptedTransport`] 在全内存里跑。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

/// 客户端信息(codex 侧用于遥测与能力协商)。
const CLIENT_NAME: &str = "rurix_forge";
const CLIENT_VERSION: &str = "0.1.0";
/// 单个请求的等待上限。审批类**服务端请求**不受此限(那是反向的),
/// 但客户端发出的 `thread/start` 等必须有底线,否则 app-server 半死时整轮无声挂住。
const REQUEST_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexError(pub String);

impl std::fmt::Display for CodexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CodexError {}

impl CodexError {
    fn new(msg: impl Into<String>) -> Self {
        CodexError(msg.into())
    }
}

/// 服务端发来的、需要上层处理的消息。
#[derive(Debug, Clone)]
pub enum Inbound {
    /// 单向通知(`item/*`、`turn/*`、`thread/*`、`account/*`)。
    Notification { method: String, params: Value },
    /// 服务端请求:上层**必须**经 [`CodexClient::respond`] 或 [`CodexClient::respond_error`] 回话。
    ServerRequest {
        id: Value,
        method: String,
        params: Value,
    },
}

impl Inbound {
    pub fn method(&self) -> &str {
        match self {
            Inbound::Notification { method, .. } | Inbound::ServerRequest { method, .. } => method,
        }
    }

    pub fn params(&self) -> &Value {
        match self {
            Inbound::Notification { params, .. } | Inbound::ServerRequest { params, .. } => params,
        }
    }
}

/// 一条连接的双向行通道。关停语义 = 丢弃 `outgoing`(stdin 关闭 → codex 自行退出)。
pub struct Channels {
    pub outgoing: mpsc::UnboundedSender<String>,
    pub incoming: mpsc::UnboundedReceiver<String>,
}

/// 行级传输(真实实现 spawn 子进程;单测实现全内存脚本化)。
pub trait CodexTransport: Send + Sync + 'static {
    fn open(&self) -> Result<Channels, CodexError>;
    /// 状态面/日志用的一行描述(不含密钥)。
    fn describe(&self) -> String;
}

/// 真实传输:`codex app-server`。
pub struct StdioTransport {
    launch: super::bin::Launch,
    env: Vec<(String, String)>,
}

impl StdioTransport {
    pub fn new(launch: super::bin::Launch, env: Vec<(String, String)>) -> Self {
        StdioTransport { launch, env }
    }
}

impl CodexTransport for StdioTransport {
    fn describe(&self) -> String {
        format!("{} app-server", self.launch.display())
    }

    fn open(&self) -> Result<Channels, CodexError> {
        use std::process::Stdio;
        let mut cmd = tokio::process::Command::new(&self.launch.program);
        for a in &self.launch.prefix_args {
            cmd.arg(a);
        }
        cmd.arg("app-server");
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // codex 的 stderr 是人类可读日志,不是协议面;丢弃免得填满管道把子进程堵死。
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| CodexError::new(format!("启动 {} 失败: {e}", self.describe())))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| CodexError::new("codex app-server 无 stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CodexError::new("codex app-server 无 stdout"))?;

        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let (in_tx, in_rx) = mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if in_tx.send(line).is_err() {
                    break;
                }
            }
        });
        tokio::spawn(async move {
            while let Some(mut line) = out_rx.recv().await {
                line.push('\n');
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.flush().await.is_err() {
                    break;
                }
            }
            // 出站端被丢弃 = 上层要关连接:关 stdin 让 codex 自己退,收尸兜底 kill。
            drop(stdin);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await;
            let _ = child.start_kill();
        });

        Ok(Channels {
            outgoing: out_tx,
            incoming: in_rx,
        })
    }
}

/// 订阅者集合。
///
/// 刻意**不**按 threadId 分路:线程 id 是 `thread/start` 的返回值,而该线程的头几条通知
/// 可能比响应先到 —— 按 id 登记必然漏掉开头。改成全量扇出 + 消费侧按
/// [`thread_id_of`] 过滤,顺带让「无 threadId 的账户/额度通知」天然可见。
type Subscribers = Vec<mpsc::UnboundedSender<Inbound>>;

type Pending = HashMap<i64, oneshot::Sender<Result<Value, CodexError>>>;

/// app-server 客户端。整个守护进程一份(`CodexService` 持有),多会话共用一个子进程。
pub struct CodexClient {
    transport: Arc<dyn CodexTransport>,
    /// 串行化「建立连接」——并发首轮不该开出两个子进程。
    connect_lock: tokio::sync::Mutex<()>,
    /// Arc:读循环发现 EOF 时要把它清空,好让下一次 `ensure_started` 重建连接。
    outgoing: Arc<StdMutex<Option<mpsc::UnboundedSender<String>>>>,
    pending: Arc<StdMutex<Pending>>,
    subscribers: Arc<StdMutex<Subscribers>>,
    next_id: AtomicI64,
    /// `initialize` 返回的 app-server 版本。只保留可展示的版本字符串，绝不缓存认证信息。
    server_version: StdMutex<Option<String>>,
    /// 连接世代号。旧连接的读循环发现 EOF 时往往已经有新连接建好了(teardown 会让旧的
    /// transport 侧收摊,而这是异步的),它必须认出「我已经不是当前连接」才不会把
    /// 新连接一起清掉。
    epoch: Arc<AtomicI64>,
    /// False after a deliberate safety/config retirement. The background watcher
    /// may reconnect transport failures, but must not revive an explicitly stopped
    /// client (which could continue an unconfirmed active Goal).
    auto_restart: AtomicBool,
}

impl CodexClient {
    pub fn new(transport: Arc<dyn CodexTransport>) -> Self {
        CodexClient {
            transport,
            connect_lock: tokio::sync::Mutex::new(()),
            outgoing: Arc::new(StdMutex::new(None)),
            pending: Arc::new(StdMutex::new(HashMap::new())),
            subscribers: Arc::new(StdMutex::new(Vec::new())),
            next_id: AtomicI64::new(0),
            server_version: StdMutex::new(None),
            epoch: Arc::new(AtomicI64::new(0)),
            auto_restart: AtomicBool::new(true),
        }
    }

    pub fn describe(&self) -> String {
        self.transport.describe()
    }

    /// 子进程是否在跑(状态面用;不主动拉起)。
    pub fn running(&self) -> bool {
        self.outgoing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// 最近一次成功握手识别出的 Codex 版本（尚未启动或上游未返回时为 `None`）。
    pub fn server_version(&self) -> Option<String> {
        self.server_version
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn sender(&self) -> Option<mpsc::UnboundedSender<String>> {
        self.outgoing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 确保已连接并完成 `initialize` 握手。连接掉了会在这里自动重建(退避由调用侧节流)。
    pub async fn ensure_started(&self) -> Result<(), CodexError> {
        self.auto_restart.store(true, Ordering::Release);
        self.ensure_started_inner().await
    }

    pub async fn restart_if_allowed(&self) -> Result<(), CodexError> {
        if !self.auto_restart.load(Ordering::Acquire) {
            return Err(CodexError::new("codex app-server 自动重启已暂停"));
        }
        self.ensure_started_inner().await
    }

    async fn ensure_started_inner(&self) -> Result<(), CodexError> {
        if self.sender().is_some() {
            return Ok(());
        }
        let _g = self.connect_lock.lock().await;
        // 双检:排队期间别人可能已经连上了。
        if self.sender().is_some() {
            return Ok(());
        }
        let Channels { outgoing, incoming } = self.transport.open()?;
        let epoch = self.epoch.fetch_add(1, Ordering::Relaxed) + 1;
        *self.outgoing.lock().unwrap_or_else(|e| e.into_inner()) = Some(outgoing);
        self.spawn_reader(incoming, epoch);
        match self.handshake().await {
            Ok(()) => Ok(()),
            Err(e) => {
                // 握手失败的连接不能留着:留着会让后续请求一直发给一个不认协议的进程。
                self.teardown();
                Err(e)
            }
        }
    }

    async fn handshake(&self) -> Result<(), CodexError> {
        let initialized = self
            .request(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": CLIENT_NAME,
                        "title": "RurixForge",
                        "version": CLIENT_VERSION
                    },
                    // experimentalApi:goal(thread/goal/*)与 dynamicTools 都在实验面后面。
                    "capabilities": {
                        "experimentalApi": true,
                        "mcpServerOpenaiFormElicitation": true
                    },
                }),
            )
            .await?;
        *self
            .server_version
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = version_from_initialize(&initialized);
        self.notify("initialized", json!({}))
    }

    fn spawn_reader(&self, mut incoming: mpsc::UnboundedReceiver<String>, epoch: i64) {
        let pending = Arc::clone(&self.pending);
        let subscribers = Arc::clone(&self.subscribers);
        let outgoing = Arc::clone(&self.outgoing);
        let epoch_now = Arc::clone(&self.epoch);
        tokio::spawn(async move {
            while let Some(line) = incoming.recv().await {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Ok(msg) = serde_json::from_str::<Value>(line) else {
                    // app-server 偶尔混入非协议行(启动横幅之类);跳过而不是把整条连接判死。
                    continue;
                };
                dispatch(&pending, &subscribers, msg);
            }
            // EOF:连接状态归零(下次 ensure_started 会重建),等待中的请求如实失败,
            // 订阅者的 receiver 因 sender 被清而看到 None。
            // 已被换代的旧连接到这里只需静默退场:它清掉的会是别人的连接。
            if epoch_now.load(Ordering::Relaxed) != epoch {
                return;
            }
            *outgoing.lock().unwrap_or_else(|e| e.into_inner()) = None;
            fail_all(&pending, &subscribers);
        });
    }

    /// 主动断开(状态清零;下次 `ensure_started` 会重建)。
    pub fn teardown(&self) {
        *self.outgoing.lock().unwrap_or_else(|e| e.into_inner()) = None;
        fail_all(&self.pending, &self.subscribers);
    }

    /// Deliberate retirement/safety stop. Explicit foreground use through
    /// `ensure_started` may enable it again; watcher-only restarts may not.
    pub fn suspend(&self) {
        self.auto_restart.store(false, Ordering::Release);
        self.teardown();
    }

    /// 发请求并等响应。
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, CodexError> {
        self.request_with_timeout(
            method,
            params,
            std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS),
        )
        .await
    }

    /// 发请求并使用调用点指定的短预算。中止/安全收尾不能被普通 120s RPC
    /// 预算拖住；本方法仍负责从 pending 表移除超时 id，避免迟到响应泄漏。
    pub async fn request_with_timeout(
        &self,
        method: &str,
        params: Value,
        budget: std::time::Duration,
    ) -> Result<Value, CodexError> {
        let tx = self
            .sender()
            .ok_or_else(|| CodexError::new("codex app-server 未连接"))?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (rx_tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, rx_tx);
        // app-server 使用 JSON-RPC 的 id/method/result 语义，但线上明确省略 `jsonrpc` 头。
        let line = serde_json::to_string(&json!({
            "id": id, "method": method, "params": params
        }))
        .map_err(|e| CodexError::new(format!("序列化 {method} 失败: {e}")))?;
        if tx.send(line).is_err() {
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            return Err(CodexError::new("写 codex app-server 失败(连接已断)"));
        }
        match tokio::time::timeout(budget, rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err(CodexError::new(format!("{method} 响应通道被丢弃"))),
            Err(_) => {
                self.pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                Err(CodexError::new(format!(
                    "{method} 超时({}ms)",
                    budget.as_millis()
                )))
            }
        }
    }

    /// 发通知(无响应)。
    pub fn notify(&self, method: &str, params: Value) -> Result<(), CodexError> {
        let tx = self
            .sender()
            .ok_or_else(|| CodexError::new("codex app-server 未连接"))?;
        let line = serde_json::to_string(&json!({
            "method": method, "params": params
        }))
        .map_err(|e| CodexError::new(format!("序列化 {method} 失败: {e}")))?;
        tx.send(line)
            .map_err(|_| CodexError::new("写 codex app-server 失败(连接已断)"))
    }

    /// 回服务端请求(成功)。
    pub fn respond(&self, id: &Value, result: Value) -> Result<(), CodexError> {
        let tx = self
            .sender()
            .ok_or_else(|| CodexError::new("codex app-server 未连接"))?;
        let line = serde_json::to_string(&json!({ "id": id, "result": result }))
            .map_err(|e| CodexError::new(format!("序列化响应失败: {e}")))?;
        tx.send(line)
            .map_err(|_| CodexError::new("写 codex app-server 失败(连接已断)"))
    }

    /// 回服务端请求(失败)。
    pub fn respond_error(&self, id: &Value, code: i64, message: &str) -> Result<(), CodexError> {
        let tx = self
            .sender()
            .ok_or_else(|| CodexError::new("codex app-server 未连接"))?;
        let line = serde_json::to_string(&json!({
            "id": id, "error": { "code": code, "message": message }
        }))
        .map_err(|e| CodexError::new(format!("序列化错误响应失败: {e}")))?;
        tx.send(line)
            .map_err(|_| CodexError::new("写 codex app-server 失败(连接已断)"))
    }

    /// 订阅全部通知与服务端请求。消费侧按 [`thread_id_of`] 自行过滤;
    /// 返回的 receiver 收到 `None` = 连接断了。
    pub fn subscribe(&self) -> mpsc::UnboundedReceiver<Inbound> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        rx
    }
}

fn version_from_initialize(result: &Value) -> Option<String> {
    if let Some(v) = result
        .get("version")
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
    {
        return Some(v.to_string());
    }
    let user_agent = result.get("userAgent").and_then(Value::as_str)?;
    user_agent
        .split(|c: char| c == '/' || c.is_ascii_whitespace() || c == '(' || c == ')')
        .map(|part| part.trim_matches(|c: char| c == ';' || c == ','))
        .find(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_string)
}

fn fail_all(pending: &Arc<StdMutex<Pending>>, subscribers: &Arc<StdMutex<Subscribers>>) {
    let drained: Vec<_> = pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain()
        .collect();
    for (_, tx) in drained {
        let _ = tx.send(Err(CodexError::new("codex app-server 连接已关闭")));
    }
    subscribers
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// 从通知/请求参数里取线程 id(app-server 用 camelCase,兼容读一手 snake_case 兜底)。
pub fn thread_id_of(params: &Value) -> Option<&str> {
    params
        .get("threadId")
        .or_else(|| params.get("thread_id"))
        .and_then(Value::as_str)
}

fn dispatch(
    pending: &Arc<StdMutex<Pending>>,
    subscribers: &Arc<StdMutex<Subscribers>>,
    msg: Value,
) {
    let has_method = msg.get("method").and_then(Value::as_str).is_some();
    // 响应:有 id 且无 method。
    if !has_method {
        let Some(id) = msg.get("id").and_then(Value::as_i64) else {
            return;
        };
        let Some(tx) = pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
        else {
            return;
        };
        let out = match msg.get("error") {
            Some(err) => {
                let message = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("未知错误");
                Err(CodexError::new(format!("codex 返回错误: {message}")))
            }
            None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = tx.send(out);
        return;
    }
    let method = msg["method"].as_str().unwrap_or_default().to_string();
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    let inbound = match msg.get("id") {
        // 有 id 且有 method = 服务端请求(审批 / 要用户输入 / dynamicTool 调用)。
        Some(id) if !id.is_null() => Inbound::ServerRequest {
            id: id.clone(),
            method,
            params,
        },
        _ => Inbound::Notification { method, params },
    };
    subscribers
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        // 退订 = receiver 被丢弃;这里顺手清掉死订阅者,免得每条消息都往空洞里发。
        .retain(|tx| tx.send(inbound.clone()).is_ok());
}

/// 全内存脚本化传输(单测专用):把每条出站消息交给 `handler`,其返回的消息按序回灌。
///
/// 还提供 [`Injector`],让测试在任意时刻塞入服务端主动发起的通知/请求。
/// 本仓纪律:单测不触网、不起子进程,所以映射/审批/中断这些逻辑全在这里跑。
#[cfg(test)]
pub struct ScriptedTransport {
    handler: Arc<dyn Fn(&Value) -> Vec<Value> + Send + Sync>,
    injector: Arc<StdMutex<Option<mpsc::UnboundedSender<String>>>>,
}

/// 服务端主动消息注入句柄。
#[cfg(test)]
#[derive(Clone)]
pub struct Injector(Arc<StdMutex<Option<mpsc::UnboundedSender<String>>>>);

#[cfg(test)]
impl Injector {
    /// 塞一条服务端消息;连接未建立或已断 → false。
    pub fn push(&self, msg: Value) -> bool {
        self.push_raw(msg.to_string())
    }

    /// 塞一行原文(测试非协议行的容错)。
    pub fn push_raw(&self, line: impl Into<String>) -> bool {
        let guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(tx) => tx.send(line.into()).is_ok(),
            None => false,
        }
    }
}

#[cfg(test)]
impl ScriptedTransport {
    pub fn new(handler: impl Fn(&Value) -> Vec<Value> + Send + Sync + 'static) -> Self {
        ScriptedTransport {
            handler: Arc::new(handler),
            injector: Arc::new(StdMutex::new(None)),
        }
    }

    pub fn injector(&self) -> Injector {
        Injector(Arc::clone(&self.injector))
    }
}

#[cfg(test)]
impl CodexTransport for ScriptedTransport {
    fn describe(&self) -> String {
        "scripted (in-memory)".to_string()
    }

    fn open(&self) -> Result<Channels, CodexError> {
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        let (in_tx, in_rx) = mpsc::unbounded_channel::<String>();
        *self.injector.lock().unwrap_or_else(|e| e.into_inner()) = Some(in_tx.clone());
        let handler = Arc::clone(&self.handler);
        tokio::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                for reply in handler(&msg) {
                    if in_tx.send(reply.to_string()).is_err() {
                        return;
                    }
                }
            }
        });
        Ok(Channels {
            outgoing: out_tx,
            incoming: in_rx,
        })
    }
}

/// 测试常用:对 `initialize` 回空 result、其余请求交给 `f`。
#[cfg(test)]
pub fn scripted_with_handshake(
    f: impl Fn(&str, &Value, &Value) -> Vec<Value> + Send + Sync + 'static,
) -> ScriptedTransport {
    ScriptedTransport::new(move |msg| {
        let method = msg
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        if method == "initialize" {
            return vec![json!({ "jsonrpc": "2.0", "id": id, "result": {
                "userAgent": "codex-fake/0.0.0"
            }})];
        }
        if method == "initialized" || id.is_null() {
            // 通知与客户端回给服务端的响应都不需要回话;交给 f 以便测试断言收到了什么。
            return f(method, &params, &id);
        }
        f(method, &params, &id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(t: ScriptedTransport) -> Arc<CodexClient> {
        Arc::new(CodexClient::new(Arc::new(t)))
    }

    #[tokio::test]
    async fn wire_omits_jsonrpc_and_handshake_order_and_version_match_protocol() {
        let seen: Arc<StdMutex<Vec<Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let c = client(ScriptedTransport::new(move |msg| {
            assert!(
                msg.get("jsonrpc").is_none(),
                "app-server JSONL 不接受 jsonrpc 头: {msg}"
            );
            sink.lock().unwrap().push(msg.clone());
            let method = msg
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(id) = msg.get("id") else {
                return Vec::new();
            };
            match method {
                "initialize" => vec![json!({
                    "id": id,
                    "result": { "userAgent": "codex-cli/0.153.0 (Windows 11)" }
                })],
                "thread/read" => vec![json!({ "id": id, "result": {} })],
                _ => Vec::new(),
            }
        }));
        c.ensure_started().await.unwrap();
        c.request("thread/read", json!({ "threadId": "th_1" }))
            .await
            .unwrap();
        let methods = seen
            .lock()
            .unwrap()
            .iter()
            .filter_map(|v| v.get("method").and_then(Value::as_str))
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert_eq!(methods, ["initialize", "initialized", "thread/read"]);
        assert_eq!(c.server_version().as_deref(), Some("0.153.0"));
    }

    /// 握手 + 请求/响应配对 + 错误响应如实转述。
    #[tokio::test]
    async fn handshake_then_request_response_pairing() {
        let c = client(scripted_with_handshake(|method, _p, id| match method {
            "thread/start" => vec![json!({ "jsonrpc": "2.0", "id": id, "result": {
                "thread": { "id": "th_1" }
            }})],
            "turn/start" => vec![json!({ "jsonrpc": "2.0", "id": id, "error": {
                "code": -32000, "message": "UsageLimitExceeded"
            }})],
            _ => vec![],
        }));
        c.ensure_started().await.expect("握手应成功");
        assert!(c.running());
        let r = c.request("thread/start", json!({})).await.unwrap();
        assert_eq!(r["thread"]["id"], "th_1");
        let err = c
            .request("turn/start", json!({}))
            .await
            .expect_err("错误响应须转成 Err");
        // 配额耗尽这类语义必须原文上屏,不能被包装成「未知错误」。
        assert!(err.0.contains("UsageLimitExceeded"), "{err}");
    }

    /// 通知全量扇出到每个订阅者,threadId 可从 params 取出供消费侧过滤;
    /// 无 threadId 的账户通知照样送达。
    #[tokio::test]
    async fn notifications_fan_out_with_thread_id_available() {
        let t = scripted_with_handshake(|_m, _p, _id| vec![]);
        let inj = t.injector();
        let c = client(t);
        c.ensure_started().await.unwrap();
        let mut a = c.subscribe();
        let mut b = c.subscribe();

        assert!(inj.push(json!({ "jsonrpc": "2.0", "method": "item/started",
            "params": { "threadId": "th_a", "item": { "id": "i1" } } })));
        assert!(
            inj.push(json!({ "jsonrpc": "2.0", "method": "account/updated",
            "params": { "planType": "pro" } }))
        );

        let got = a.recv().await.expect("订阅者应收到通知");
        assert_eq!(got.method(), "item/started");
        assert_eq!(thread_id_of(got.params()), Some("th_a"));
        // 两个订阅者各收一份(多会话并发时互不吞消息)。
        assert_eq!(b.recv().await.unwrap().method(), "item/started");
        let acct = a.recv().await.unwrap();
        assert_eq!(acct.method(), "account/updated");
        assert_eq!(thread_id_of(acct.params()), None);
    }

    /// 服务端请求带 id → 归类为 ServerRequest,并能回响应(测试侧断言收到了回话)。
    #[tokio::test]
    async fn server_request_is_answerable() {
        let seen: Arc<StdMutex<Vec<Value>>> = Arc::new(StdMutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let t = ScriptedTransport::new(move |msg| {
            let method = msg
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if method == "initialize" {
                return vec![json!({ "jsonrpc": "2.0", "id": msg["id"].clone(), "result": {} })];
            }
            if method.is_empty() {
                sink.lock().unwrap().push(msg.clone());
            }
            vec![]
        });
        let inj = t.injector();
        let c = client(t);
        c.ensure_started().await.unwrap();
        let mut rx = c.subscribe();
        inj.push(json!({ "jsonrpc": "2.0", "id": 77,
            "method": "item/commandExecution/requestApproval",
            "params": { "threadId": "th_1", "command": "cargo test" } }));
        let got = rx.recv().await.unwrap();
        let Inbound::ServerRequest { id, method, params } = &got else {
            panic!("带 id 的 method 必须归为 ServerRequest,实得 {got:?}");
        };
        assert_eq!(method, "item/commandExecution/requestApproval");
        assert_eq!(params["command"], "cargo test");
        c.respond(id, json!({ "decision": "accept" })).unwrap();
        // 回话是「无 method 的 id+result」形态,不能误发成新请求。
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let recorded = seen.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1, "应恰好回一条: {recorded:?}");
        assert_eq!(recorded[0]["id"], 77);
        assert_eq!(recorded[0]["result"]["decision"], "accept");
        assert!(recorded[0].get("jsonrpc").is_none());
    }

    /// 连接断开:等待中的请求如实失败,订阅者看到 EOF,状态归零可重连。
    #[tokio::test]
    async fn disconnect_fails_pending_and_allows_restart() {
        let c = client(scripted_with_handshake(|_m, _p, _id| vec![]));
        c.ensure_started().await.unwrap();
        let mut rx = c.subscribe();
        c.teardown();
        assert!(!c.running());
        assert!(rx.recv().await.is_none(), "断开后订阅者应看到 EOF");
        let err = c.request("thread/start", json!({})).await.unwrap_err();
        assert!(err.0.contains("未连接"), "{err}");
        // 重连后照常可用(自动重启的基础)。
        c.ensure_started().await.expect("应能重连");
        assert!(c.running());
    }

    /// 非协议行(启动横幅之类)不得把连接判死。
    #[tokio::test]
    async fn junk_lines_are_skipped() {
        let t = scripted_with_handshake(|_m, _p, _id| vec![]);
        let inj = t.injector();
        let c = client(t);
        c.ensure_started().await.unwrap();
        let mut rx = c.subscribe();
        inj.push_raw("codex app-server listening on stdio");
        inj.push_raw("");
        inj.push(json!({ "jsonrpc": "2.0", "method": "item/started",
            "params": { "threadId": "th_1" } }));
        assert_eq!(rx.recv().await.unwrap().method(), "item/started");
    }
}
