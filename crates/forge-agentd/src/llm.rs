//! RD-F1-002 真 LLM 工具循环:provider 抽象(mock|deepseek)+ POST /api/forge/llm/chat。
//! 循环:tools/list 实测拉取五 server schema → OpenAI tools 格式 → DeepSeek chat.completions
//! (tools, tool_choice=auto)→ tool_calls 逐项进程内 mcp::call_tool → role:tool 回注 →
//! 终止(无 tool_calls 或 max_iters=16)→ { provider, text, toolCalls[{name,ok,summary}], iters }。
//! 密钥红线(R-5):FORGE_LLM_API_KEY env 优先 → gend keystore["deepseek"](key_for 语义,
//! FORGE_GEN_API_KEY 共享 dev-key 覆盖如实标注);皆无 → provider=mock。密钥只进
//! Authorization 请求头,永不进日志/事件/工具返回/错误消息。
//!
//! F7 wave.2:循环核心抽为 run_tool_loop(step/executor/sink/cancel/forbidden 全可注入)——
//! llm/chat handler 以无事件 sink 调用(返回结构与行为不变,RD-F1-002 红线);
//! agent.rs ask:execute 以事件 sink + 可注入 executor 调用(单测 scripted fake 全内存,
//! 禁止网络与子进程)。
//!
//! F8 wave.2:openai-compatible 通用渠道最小落地(D-F8-C)——配置面 baseUrl+model+apiKey;
//! key 复用 gend keystore["openai-compat"](R-5 红线不变),baseUrl/model 落
//! data/llm-openai-compat.json(读-改-写 Mutex 原子写,同 gen-backends.json 纪律);
//! REST:POST /api/forge/llm/openai-compat/config + GET /api/forge/llm/openai-compat/status
//! (响应面无 key);provider 分支经 agent.rs provider_for_session(selectedModelId=="openai-compat"),
//! chat-completions 同形态 POST {baseUrl}/v1/chat/completions + Authorization Bearer;
//! 未配置调用 = 显式 OPENAI_COMPAT_NOT_CONFIGURED(GEN_BACKEND_NOT_CONFIGURED 同族精神)。

use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

use crate::mcp;

/// DeepSeek 官方端点(OpenAI 兼容)。
const DEEPSEEK_URL: &str = "https://api.deepseek.com/chat/completions";
const DEEPSEEK_MODEL: &str = "deepseek-chat";
/// 工具循环上限(防失控;耗尽如实标注 truncated)。
const MAX_ITERS: usize = 16;
/// 回注 LLM 的工具结果截断(控 token);响应记录的 summary 另行 ≤200ch。
const TOOL_FEEDBACK_MAX: usize = 4000;
/// 单次 HTTP 请求超时(工具循环多轮,每轮一个请求)。
const HTTP_TIMEOUT_SECS: u64 = 60;

pub(crate) const SYSTEM_PROMPT: &str = "你是 RurixForge 游戏引擎编辑器的内置助手。\
用户用中文描述场景编辑/资产管理/代码工具意图,你应优先调用提供的工具完成实际操作,而不是只描述步骤。\
工具调用参数严格遵循各工具的 inputSchema;实体创建等操作完成后可用一句话如实汇报结果(成功/失败/数量),不得伪造执行结果。\
场景实体按三类索引:角色(role)=可操控/动态体,地图(map)=静态场景元素,交互(interaction)=带 Script/Trigger 的可交互物;\
可用 scene_index 一次概览分组,entity_list 返回各实体 category 字段;需覆盖分类时给实体加 Category 组件(category=role|map|interaction)。\
检索优先纪律(F10):工作区已建语义索引——找素材/实体/逻辑图/代码/文档先用 context_search 定位\
(返回 tier 如实标注 lexical 词法档或 hybrid 混合档),命中不足再 asset_list/entity_list 全量遍历;\
若返回 INDEX_NOT_BUILT 先调 context_index_build。资产缺文字简介时用 asset_describe_batch 领取待办、\
撰写后 asset_set_description 回写(source 溯源如实:看过缩略图 agent-vision,仅凭事实 agent-facts)。";

#[derive(Deserialize)]
pub struct ChatRequest {
    text: String,
    /// composer 模式(build/plan/debug/ask;multitask 不走本路由)。当前仅记录,不改行为。
    #[serde(default)]
    #[allow(dead_code)]
    mode: Option<String>,
}

/// 单次工具调用记录(响应面;summary ≤200ch)。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallRecord {
    pub name: String,
    pub ok: bool,
    pub summary: String,
}

/// provider 解析结果。
#[derive(Clone, PartialEq)]
pub(crate) enum Provider {
    /// 无密钥:恒绿 seam,不触网不触 MCP。
    Mock,
    /// DeepSeek 官方 API;String 仅用于 Authorization 头组装。
    Deepseek(String),
    /// F8 wave.2:openai-compatible 通用渠道(已配齐 baseUrl+model+key;key 仅用于 Authorization 头)。
    OpenAiCompat {
        base_url: String,
        model: String,
        key: String,
    },
    /// F8 wave.2:会话显式选 openai-compat 但未配齐 → 显式 NOT_CONFIGURED 分支(不静默回落)。
    OpenAiCompatNotConfigured,
}

/// Debug 脱敏(R-5):Deepseek/OpenAiCompat 变体永不打印密钥本体。
impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Mock => f.write_str("Mock"),
            Provider::Deepseek(_) => f.write_str("Deepseek(<redacted>)"),
            Provider::OpenAiCompat { base_url, model, .. } => f
                .debug_struct("OpenAiCompat")
                .field("base_url", base_url)
                .field("model", model)
                .field("key", &"<redacted>")
                .finish(),
            Provider::OpenAiCompatNotConfigured => f.write_str("OpenAiCompatNotConfigured"),
        }
    }
}

/// 密钥面:FORGE_LLM_API_KEY env 优先 → gend keystore["deepseek"]。
/// keystore.key_for 语义含 FORGE_GEN_API_KEY 共享 dev-key 覆盖(D-RDG-B 如实标注)。
/// F7 wave.1:抽成可复用判定(design-snapshot models.availability 同源;R-5 行为不变)。
fn resolve_deepseek_key() -> Option<String> {
    if let Ok(v) = std::env::var("FORGE_LLM_API_KEY") {
        if !v.is_empty() {
            return Some(v);
        }
    }
    let ks = gend::keystore::Keystore::load();
    match ks.key_for("deepseek") {
        Some(k) if !k.is_empty() => Some(k),
        _ => None,
    }
}

pub(crate) fn resolve_provider() -> Provider {
    match resolve_deepseek_key() {
        Some(k) => Provider::Deepseek(k),
        None => Provider::Mock,
    }
}

/// deepseek 密钥可用性(design-snapshot availability 复用;不暴露密钥本体,R-5)。
pub(crate) fn deepseek_key_available() -> bool {
    resolve_deepseek_key().is_some()
}

/// MCP tools/list 条目 → OpenAI tools 格式(type:function;parameters 缺省补 {"type":"object"})。
pub fn to_openai_tools(mcp_tools: &[Value]) -> Vec<Value> {
    mcp_tools
        .iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?;
            let mut params = t
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if params.get("type").is_none() {
                params["type"] = json!("object");
            }
            let desc = t
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some(json!({
                "type": "function",
                "function": { "name": name, "description": desc, "parameters": params },
            }))
        })
        .collect()
}

/// 单轮实发的模型规格(会话三档经 modelspec::resolve 现算而来;Default = 原 F8 行为)。
/// 只带真正进请求体的两项:上下文窗口档不是 chat.completions 参数,不到这一层。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestSpec {
    /// 模型名覆盖(deepseek 思考开 → deepseek-reasoner);None = 用渠道默认名。
    pub model: Option<String>,
    /// reasoning_effort 实发值;None = 请求体不含该字段(行为与规格波之前逐字节一致)。
    pub reasoning_effort: Option<String>,
}

/// chat.completions 请求体组装(纯函数,便于「密钥不进 body」扫描测试;F8:model 参数化)。
/// 规格波:spec.model 覆盖渠道默认模型名;reasoning_effort 仅在有值时出现在 body 里。
fn build_request_body(
    model: &str,
    messages: &[Value],
    tools: &[Value],
    spec: &RequestSpec,
) -> Value {
    let mut body = json!({
        "model": spec.model.as_deref().unwrap_or(model),
        "messages": messages,
        "tools": tools,
        "tool_choice": "auto",
        "stream": false,
    });
    if let Some(effort) = &spec.reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }
    body
}

/// LLM 侧失败(上行 HTTP / 协议 / 工具面不可用);消息保证不含密钥。
#[derive(Debug)]
pub struct LlmError(String);

impl LlmError {
    /// F7 wave.2:agent.rs 构造工具面不可用等失败(消息面不含密钥)。
    pub fn new(msg: impl Into<String>) -> Self {
        LlmError(msg.into())
    }
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 单次 chat.completions POST 共享核(阻塞,调用方须 spawn_blocking;F8 抽出供双渠道复用)。
/// 错误消息只带 provider 标签 + HTTP 状态码/传输错误,不回显请求体与头(R-5)。
fn post_chat_completions(
    url: &str,
    provider_label: &str,
    key: &str,
    body: &Value,
) -> Result<Value, LlmError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build();
    let resp = agent
        .post(url)
        .set("Authorization", &format!("Bearer {key}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    let resp = match resp {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            // 读响应体取 error.message(上行错误面;不含我方密钥)。
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
            return Err(LlmError(format!("{provider_label} HTTP {code}: {detail}")));
        }
        Err(ureq::Error::Transport(t)) => {
            return Err(LlmError(format!("{provider_label} 连接失败: {t}")));
        }
    };
    let bytes =
        read_body(resp).map_err(|e| LlmError(format!("读 {provider_label} 响应体失败: {e}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| LlmError(format!("{provider_label} 响应非 JSON: {e}")))
}

/// 单次 DeepSeek chat.completions 调用(阻塞,调用方须 spawn_blocking)。
/// 错误消息只带 HTTP 状态码/传输错误,不回显请求体与头(R-5)。
fn chat_completions(
    key: &str,
    messages: &[Value],
    tools: &[Value],
    spec: &RequestSpec,
) -> Result<Value, LlmError> {
    let body = build_request_body(DEEPSEEK_MODEL, messages, tools, spec);
    post_chat_completions(DEEPSEEK_URL, "DeepSeek", key, &body)
}

/// 单次 openai-compat chat.completions 调用(阻塞,调用方须 spawn_blocking;F8 wave.2)。
/// URL = {baseUrl 去尾斜杠}/v1/chat/completions;错误面同 R-5 纪律(不含密钥)。
fn chat_completions_openai_compat(
    base_url: &str,
    model: &str,
    key: &str,
    messages: &[Value],
    tools: &[Value],
    spec: &RequestSpec,
) -> Result<Value, LlmError> {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let body = build_request_body(model, messages, tools, spec);
    post_chat_completions(&url, "openai-compat", key, &body)
}

fn read_body(resp: ureq::Response) -> Result<Vec<u8>, std::io::Error> {
    use std::io::Read;
    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf)?;
    Ok(buf)
}

/// SSE stream:true 调用,边读边回传 StreamDelta,再合成非流式 chat.completions JSON 供 parse_step_response。
fn chat_completions_stream(
    url: &str,
    provider_label: &str,
    model: &str,
    key: &str,
    messages: &[Value],
    tools: &[Value],
    spec: &RequestSpec,
    sink: Option<&StreamSink>,
) -> Result<Value, LlmError> {
    use std::io::{BufRead, BufReader};
    let mut body = build_request_body(model, messages, tools, spec);
    body["stream"] = json!(true);
    body["stream_options"] = json!({ "include_usage": true });
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS.max(180)))
        .build();
    let resp = agent
        .post(url)
        .set("Authorization", &format!("Bearer {key}"))
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
            if let Some(s) = sink {
                s(StreamDelta::Reset);
            }
            return Err(LlmError(format!("{provider_label} HTTP {code}: {detail}")));
        }
        Err(ureq::Error::Transport(t)) => {
            if let Some(s) = sink {
                s(StreamDelta::Reset);
            }
            return Err(LlmError(format!("{provider_label} 连接失败: {t}")));
        }
    };
    let reader = BufReader::new(resp.into_reader());
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut tool_acc: std::collections::BTreeMap<u32, (String, String, String)> =
        std::collections::BTreeMap::new();
    let mut usage = json!({});
    for line in reader.lines() {
        let line = line.map_err(|e| LlmError(format!("读 {provider_label} 流失败: {e}")))?;
        let line = line.trim();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(u) = v.get("usage") {
            usage = u.clone();
        }
        let Some(choice) = v
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
        else {
            continue;
        };
        let Some(delta) = choice.get("delta") else {
            continue;
        };
        if let Some(t) = delta.get("content").and_then(|x| x.as_str()) {
            if !t.is_empty() {
                content.push_str(t);
                if let Some(s) = sink {
                    s(StreamDelta::Text(t.to_string()));
                }
            }
        }
        if let Some(t) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(|x| x.as_str())
        {
            if !t.is_empty() {
                reasoning.push_str(t);
                if let Some(s) = sink {
                    s(StreamDelta::Reasoning(t.to_string()));
                }
            }
        }
        if let Some(tcs) = delta.get("tool_calls").and_then(|x| x.as_array()) {
            for tc in tcs {
                let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                let entry = tool_acc.entry(idx).or_insert_with(|| {
                    (
                        tc.get("id")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        String::new(),
                        String::new(),
                    )
                });
                if let Some(id) = tc.get("id").and_then(|x| x.as_str()) {
                    if !id.is_empty() {
                        entry.0 = id.to_string();
                    }
                }
                if let Some(name) = tc
                    .pointer("/function/name")
                    .and_then(|x| x.as_str())
                {
                    entry.1.push_str(name);
                }
                if let Some(args) = tc
                    .pointer("/function/arguments")
                    .and_then(|x| x.as_str())
                {
                    entry.2.push_str(args);
                    if let Some(s) = sink {
                        s(StreamDelta::ToolArgs {
                            index: idx,
                            tool_call_id: entry.0.clone(),
                            name: entry.1.clone(),
                            delta: args.to_string(),
                        });
                    }
                }
            }
        }
    }
    let tool_calls: Vec<Value> = tool_acc
        .into_iter()
        .map(|(_, (id, name, arguments))| {
            json!({
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": arguments },
            })
        })
        .collect();
    let mut message = json!({ "role": "assistant", "content": content });
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if !tool_calls.is_empty() {
        message["tool_calls"] = json!(tool_calls);
    }
    Ok(json!({
        "choices": [{ "message": message }],
        "usage": usage,
    }))
}

/// 截断到 max 字符(按 char 边界,防 UTF-8 切断)。
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let t: String = s.chars().take(max).collect();
    format!("{t}…")
}

/// MCP result 信封 → 文本(content[0].text 优先,退化本体 JSON)。
fn envelope_text(result: &Value) -> String {
    if let Some(t) = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
    {
        return t.to_string();
    }
    result.to_string()
}

/// 取 assistant message 的 tool_calls 数组(非空才 Some)。
fn tool_calls_of(msg: &Value) -> Option<&Vec<Value>> {
    msg.get("tool_calls")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
}

/// DeepSeek 工具循环主流程(F7 wave.2:薄壳——工具面拉取 + 无事件 sink 调 run_tool_loop,
/// 返回 (最终文本, 工具调用记录, 实际轮数) 与 RD-F1-002 原行为逐字一致)。
async fn run_deepseek_loop(
    text: &str,
    key: &str,
) -> Result<(String, Vec<ToolCallRecord>, usize), LlmError> {
    let mcp_tools = mcp::list_all_tools()
        .await
        .map_err(|e| LlmError(format!("MCP 工具面拉取失败: {e}")))?;
    let tools = to_openai_tools(&mcp_tools);
    // llm/chat 是无会话的调试路由,没有规格三档可解析 → 默认 spec(行为同规格波之前)。
    let step = deepseek_step(key, &RequestSpec::default());
    let execute = mcp_executor();
    let out = run_tool_loop(
        SYSTEM_PROMPT,
        text,
        ToolLoopCfg {
            tools,
            step: step.as_ref(),
            execute: execute.as_ref(),
            // llm/chat 是 deepseek 专用调试路由,该渠道无视觉面。
            vision: false,
            sink: None,
            forbidden: None,
            cancelled: None,
            stream: None,
            preamble: None,
        },
    )
    .await?;
    Ok((out.text, out.records, out.iters))
}

// ---------- F7 wave.2:可注入工具循环核心(agent.rs 事件化复用;llm/chat 无 sink 行为不变) ----------

/// 盒装异步返回(注入闭包统一签名)。
pub type BoxFut<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'static>>;

/// provider 单轮 token 用量(deepseek 响应 usage;mock 无此概念 → 不发 agent.usage)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// provider 步进结果:assistant message 本体(可含 tool_calls)+ 可选 usage。
pub struct StepOutcome {
    pub message: Value,
    pub usage: Option<Usage>,
}

/// provider 步进(可注入;单测用 scripted fake,禁止网络/子进程):
/// (messages, openai tools, 可选 stream sink) → assistant message + usage。
pub type StepFn =
    dyn Fn(Vec<Value>, Vec<Value>, Option<StreamSink>) -> BoxFut<Result<StepOutcome, LlmError>>
        + Send
        + Sync;

/// 工具反馈:回注文本 + 可选图片(data URI)。
/// 图片单列而不并进文本:base64 一进文本就会被 TOOL_FEEDBACK_MAX 截成废串,还白烧 token。
/// 从 &str/String 可直接 into(),既有 executor 构造点无需改写语义。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolFeedback {
    pub text: String,
    /// data:image/...;base64,... 形态;仅在模型有视觉面时才会进上行消息。
    pub images: Vec<String>,
}

impl From<String> for ToolFeedback {
    fn from(text: String) -> Self {
        ToolFeedback { text, images: Vec::new() }
    }
}

impl From<&str> for ToolFeedback {
    fn from(text: &str) -> Self {
        ToolFeedback { text: text.to_string(), images: Vec::new() }
    }
}

/// 工具执行器(可注入;单测假成功/假失败):(name, args) → (ok, feedback)。
/// 失败以 (false, 原因) 表达——工具错误不打断循环(RD-F1-002 原语义)。
pub type ExecFn = dyn Fn(String, Value) -> BoxFut<(bool, ToolFeedback)> + Send + Sync;

/// 流式增量(经 StepFn 第三参回传;agent 侧转 ephemeral 事件)。
#[derive(Debug, Clone)]
pub enum StreamDelta {
    Text(String),
    Reasoning(String),
    ToolArgs {
        index: u32,
        tool_call_id: String,
        name: String,
        delta: String,
    },
    Reset,
}

/// 流式回调(Arc,可进 spawn_blocking)。
pub type StreamSink = std::sync::Arc<dyn Fn(StreamDelta) + Send + Sync>;

/// 工具循环事件(sink;llm/chat 传 None = 零事件 = 原行为;agent.rs 接 EventBus)。
#[derive(Debug)]
pub enum LoopEvent {
    ToolInvoked {
        name: String,
        args: Value,
        tool_call_id: String,
    },
    ToolCompleted {
        name: String,
        tool_call_id: String,
        duration_ms: u64,
        output: String,
    },
    ToolFailed {
        name: String,
        error: String,
        tool_call_id: String,
        duration_ms: u64,
    },
    ToolDenied {
        name: String,
        error: String,
        tool_call_id: String,
    },
    Reasoning(String),
    Usage(Usage),
    TextDelta(String),
    ReasoningDelta(String),
    ToolArgsDelta {
        index: u32,
        tool_call_id: String,
        name: String,
        delta: String,
    },
    StreamReset,
}

/// 工具循环结果。
pub struct ToolLoopOutcome {
    pub text: String,
    pub records: Vec<ToolCallRecord>,
    pub iters: usize,
    pub cancelled: bool,
}

/// 循环配置(sink/forbidden/cancelled 三注入点全 Option,None = llm/chat 原行为)。
pub struct ToolLoopCfg<'a> {
    /// 给 provider 的 openai tools(ask 模式传空 = 纯对话)。
    pub tools: Vec<Value>,
    pub step: &'a StepFn,
    pub execute: &'a ExecFn,
    /// 该模型是否收图片。false 时工具产出的图片一律不进上行消息(见 vision_enabled)。
    pub vision: bool,
    /// 事件汇。
    pub sink: Option<&'a (dyn Fn(LoopEvent) + Send + Sync)>,
    /// 禁用工具判定(plan 模式写工具门):命中 → 不执行,该调用 TOOL_FORBIDDEN 收尾。
    pub forbidden: Option<&'a (dyn Fn(&str) -> bool + Send + Sync)>,
    /// 取消令牌判定(每迭代开头检查一次)。
    pub cancelled: Option<&'a (dyn Fn() -> bool + Send + Sync)>,
    /// 流式增量(生产 ask:execute 传入;llm/chat 与旧单测传 None)。
    pub stream: Option<StreamSink>,
    /// F10 预检索上下文:Some(非空) = 在 system 与 user 之间插入第三条 role:system
    /// 「工作区上下文」消息(不污染 system prompt 本体);None/空 = 原两条消息行为。
    pub preamble: Option<String>,
}

/// chat.completions 响应 → StepOutcome(message + usage;usage 缺省 None;F8:provider 标签参数化)。
fn parse_step_response(resp: &Value, provider_label: &str) -> Result<StepOutcome, LlmError> {
    let msg = resp
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|c| c.get("message"))
        .cloned()
        .ok_or_else(|| LlmError(format!("{provider_label} 响应缺 choices[0].message")))?;
    let usage = resp.get("usage").map(|u| Usage {
        prompt_tokens: u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
        completion_tokens: u
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: u.get("total_tokens").and_then(Value::as_u64).unwrap_or(0),
    });
    Ok(StepOutcome {
        message: msg,
        usage,
    })
}

/// deepseek 步进工厂(阻塞 HTTP 走 spawn_blocking;R-5:密钥只进请求头,闭包不外泄)。
/// spec 承载会话规格(思考开 → spec.model = deepseek-reasoner);默认 spec = 原 F8 行为。
pub fn deepseek_step(key: &str, spec: &RequestSpec) -> Box<StepFn> {
    let key = key.to_string();
    let spec = spec.clone();
    Box::new(move |messages, tools, stream| {
        let key = key.clone();
        let spec = spec.clone();
        Box::pin(async move {
            let resp = tokio::task::spawn_blocking(move || {
                if let Some(sink) = stream {
                    chat_completions_stream(
                        DEEPSEEK_URL,
                        "DeepSeek",
                        DEEPSEEK_MODEL,
                        &key,
                        &messages,
                        &tools,
                        &spec,
                        Some(&sink),
                    )
                } else {
                    chat_completions(&key, &messages, &tools, &spec)
                }
            })
            .await
            .map_err(|e| LlmError(format!("spawn_blocking join 失败: {e}")))??;
            parse_step_response(&resp, "DeepSeek")
        })
    })
}

/// openai-compat 步进工厂(F8 wave.2;同 deepseek 形态:spawn_blocking + Bearer 头,闭包不外泄)。
/// spec 承载会话规格(思考开 + 该渠道收 reasoning_effort → body 带该字段)。
pub fn openai_compat_step(
    base_url: &str,
    model: &str,
    key: &str,
    spec: &RequestSpec,
) -> Box<StepFn> {
    let base_url = base_url.to_string();
    let model = model.to_string();
    let key = key.to_string();
    let spec = spec.clone();
    Box::new(move |messages, tools, stream| {
        let base_url = base_url.clone();
        let model = model.clone();
        let key = key.clone();
        let spec = spec.clone();
        Box::pin(async move {
            let resp = tokio::task::spawn_blocking(move || {
                if let Some(sink) = stream {
                    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
                    chat_completions_stream(
                        &url,
                        "openai-compat",
                        &model,
                        &key,
                        &messages,
                        &tools,
                        &spec,
                        Some(&sink),
                    )
                } else {
                    chat_completions_openai_compat(
                        &base_url, &model, &key, &messages, &tools, &spec,
                    )
                }
            })
            .await
            .map_err(|e| LlmError(format!("spawn_blocking join 失败: {e}")))??;
            parse_step_response(&resp, "openai-compat")
        })
    })
}

/// mock 文案(llm/chat 与 agent.rs mock 步进同一字符串,行为不变红线)。
pub(crate) fn mock_reply_text(input: &str) -> String {
    format!("mock:已收到「{}」(无 LLM 密钥,真实工具循环未启用;配 FORGE_LLM_API_KEY 或 keystore[deepseek] 后走 deepseek)", truncate_chars(input, 80))
}

/// mock 步进:首轮即终稿,不产工具调用,无 usage(无密钥恒绿 seam,不触网不触 MCP)。
pub fn mock_step() -> Box<StepFn> {
    Box::new(|messages, _tools, stream| {
        // 取末条 user 消息作输入(与 llm/chat mock 文案同源)。
        let input = messages
            .iter()
            .rev()
            .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Box::pin(async move {
            let text = mock_reply_text(&input);
            if let Some(sink) = stream {
                for chunk in text.chars().collect::<Vec<_>>().chunks(8) {
                    sink(StreamDelta::Text(chunk.iter().collect()));
                }
            }
            Ok(StepOutcome {
                message: json!({ "role": "assistant", "content": text }),
                usage: None,
            })
        })
    })
}

/// 单次工具最多回注几张图(视觉模型的图片 token 昂贵,四向预览即上限)。
const TOOL_IMAGE_MAX: usize = 4;
/// 单张回注图上限(超限跳过——宁可少一张,不可把上行请求撑爆)。
const TOOL_IMAGE_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// 工具结果里的图片约定:顶层 `imageRefs[]` = 项目相对 png 路径。
/// 走路径而非内联 base64,是为了不让几 MB 的图占满 MCP stdio 管道与工具文本。
fn extract_tool_images(text: &str) -> Vec<String> {
    use base64::Engine as _;
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(refs) = doc.get("imageRefs").and_then(Value::as_array) else {
        return Vec::new();
    };
    let root = mcp::asset_project_root();
    let project = assetd::project::ForgeProject::load(&root)
        .unwrap_or_else(|_| assetd::project::ForgeProject::with_defaults(root.clone()));
    let mut out = Vec::new();
    for r in refs.iter().filter_map(Value::as_str).take(TOOL_IMAGE_MAX) {
        let Ok(abs) = gend::tmpstore::resolve_project_file(&project, r) else {
            continue;
        };
        match std::fs::metadata(&abs) {
            Ok(m) if m.len() <= TOOL_IMAGE_MAX_BYTES => {}
            _ => continue,
        }
        let Ok(bytes) = std::fs::read(&abs) else { continue };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        out.push(format!("data:image/png;base64,{b64}"));
    }
    out
}

/// 生产 executor:进程内 mcp::call_tool(RD-F1-002 原语义:ok = !isError;错误不打断循环)。
pub fn mcp_executor() -> Box<ExecFn> {
    mcp_executor_in(mcp::default_project_root())
}

/// 按项目根选 MCP 连接池的生产 executor。
pub fn mcp_executor_in(project_root: std::path::PathBuf) -> Box<ExecFn> {
    Box::new(move |name, args| {
        let root = project_root.clone();
        Box::pin(async move {
            match mcp::call_tool_in(&root, &name, Some(args)).await {
                Ok(result) => {
                    let is_err = result
                        .get("isError")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let text = envelope_text(&result);
                    let images = if is_err { Vec::new() } else { extract_tool_images(&text) };
                    (!is_err, ToolFeedback { text, images })
                }
                Err(e) => (false, format!("工具调用失败: {e}").into()),
            }
        })
    })
}

/// 工具循环主流程(F7 wave.2 抽出;llm/chat 以 sink/forbidden/cancelled 全 None 调用,
/// 行为与 RD-F1-002 原 run_deepseek_loop 逐行等价:同一消息序/截断/记录面/轮数语义)。
pub async fn run_tool_loop(
    system_prompt: &str,
    user_text: &str,
    cfg: ToolLoopCfg<'_>,
) -> Result<ToolLoopOutcome, LlmError> {
    let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
    // F10:预检索上下文作独立 system 消息插在 system 与 user 之间(空串视同 None)。
    if let Some(p) = cfg.preamble.as_deref().filter(|p| !p.is_empty()) {
        messages.push(json!({ "role": "system", "content": p }));
    }
    messages.push(json!({ "role": "user", "content": user_text }));
    let mut records: Vec<ToolCallRecord> = Vec::new();
    let mut iters_done = 0usize;

    for iter in 1..=MAX_ITERS {
        // 每迭代检查取消令牌(F7 wave.2 runs cancel;llm/chat 传 None 恒 false)。
        if cfg.cancelled.map(|c| c()).unwrap_or(false) {
            return Ok(ToolLoopOutcome {
                text: String::new(),
                records,
                iters: iters_done,
                cancelled: true,
            });
        }
        let out = (cfg.step)(messages.clone(), cfg.tools.clone(), cfg.stream.clone()).await?;
        if let (Some(sink), Some(u)) = (cfg.sink, out.usage) {
            sink(LoopEvent::Usage(u));
        }
        let msg = out.message;
        if let Some(r) = msg
            .get("reasoning_content")
            .or_else(|| msg.get("reasoning"))
            .and_then(Value::as_str)
        {
            if !r.is_empty() {
                if let Some(sink) = cfg.sink {
                    sink(LoopEvent::Reasoning(r.to_string()));
                }
            }
        }

        let Some(calls) = tool_calls_of(&msg).cloned() else {
            // 无工具调用:终止,文本即最终答复。
            let content = msg
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            // 无 live stream 时把终稿切成 delta(mock/scripted);有 HTTP 流则步进内已发过。
            if cfg.stream.is_none() {
                if let Some(sink) = cfg.sink {
                    emit_text_chunks(sink, &content);
                }
            }
            return Ok(ToolLoopOutcome {
                text: content,
                records,
                iters: iter,
                cancelled: false,
            });
        };
        iters_done = iter;

        // assistant 消息(含 tool_calls)原样回注,再逐项执行工具并回注 role:tool。
        messages.push(msg);
        // 本轮工具产出的图片。攒到 calls 循环之后统一发:tool 消息必须紧跟 assistant
        // 逐个配对 tool_call_id,中间插一条 user 会打断配对。
        let mut pending_images: Vec<(String, String)> = Vec::new();
        for c in &calls {
            let call_id = c.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            let name = c
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let args_raw = c
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let args = serde_json::from_str::<Value>(args_raw).unwrap_or_else(|_| json!({}));
            if let Some(sink) = cfg.sink {
                sink(LoopEvent::ToolInvoked {
                    name: name.clone(),
                    args: args.clone(),
                    tool_call_id: call_id.clone(),
                });
            }
            // 禁用工具门(plan 写工具):不执行,该调用 TOOL_FORBIDDEN 收尾并如实回注。
            if cfg.forbidden.map(|f| f(&name)).unwrap_or(false) {
                let feedback = format!("TOOL_FORBIDDEN: 当前模式禁止调用写工具 {name}");
                if let Some(sink) = cfg.sink {
                    sink(LoopEvent::ToolFailed {
                        name: name.clone(),
                        error: feedback.clone(),
                        tool_call_id: call_id.clone(),
                        duration_ms: 0,
                    });
                }
                records.push(ToolCallRecord {
                    name,
                    ok: false,
                    summary: truncate_chars(&feedback, 200),
                });
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": truncate_chars(&feedback, TOOL_FEEDBACK_MAX),
                }));
                continue;
            }
            let started = std::time::Instant::now();
            let (ok, feedback) = (cfg.execute)(name.clone(), args).await;
            let duration_ms = started.elapsed().as_millis() as u64;
            if let Some(sink) = cfg.sink {
                if ok {
                    sink(LoopEvent::ToolCompleted {
                        name: name.clone(),
                        tool_call_id: call_id.clone(),
                        duration_ms,
                        output: feedback.text.clone(),
                    });
                } else if feedback.text.starts_with("TOOL_FORBIDDEN") {
                    sink(LoopEvent::ToolDenied {
                        name: name.clone(),
                        error: truncate_chars(&feedback.text, 200),
                        tool_call_id: call_id.clone(),
                    });
                } else {
                    sink(LoopEvent::ToolFailed {
                        name: name.clone(),
                        error: truncate_chars(&feedback.text, 200),
                        tool_call_id: call_id.clone(),
                        duration_ms,
                    });
                }
            }
            // 无视觉面的模型不该收到图:发过去只会被拒或被当成噪声。
            if cfg.vision {
                for img in &feedback.images {
                    pending_images.push((name.clone(), img.clone()));
                }
            }
            records.push(ToolCallRecord {
                name,
                ok,
                summary: truncate_chars(&feedback.text, 200),
            });
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": truncate_chars(&feedback.text, TOOL_FEEDBACK_MAX),
            }));
        }
        if !pending_images.is_empty() {
            messages.push(tool_image_message(&pending_images));
        }
    }
    // 轮数耗尽:如实标注,不伪造收尾。
    Ok(ToolLoopOutcome {
        text: format!("(工具循环已达上限 {MAX_ITERS} 轮,未收束;以上为已执行部分)"),
        records,
        iters: MAX_ITERS,
        cancelled: false,
    })
}

/// 工具产出的图片 → 一条 user 多模态消息(chat.completions 的 tool 消息只收字符串,
/// 图片只能另起一条 user;这是 OpenAI 兼容面的通行做法)。
fn tool_image_message(images: &[(String, String)]) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    let names: Vec<&str> = {
        let mut v: Vec<&str> = images.iter().map(|(n, _)| n.as_str()).collect();
        v.dedup();
        v
    };
    blocks.push(json!({
        "type": "text",
        "text": format!(
            "以下 {} 张图片是上一步工具({})的产出渲染,供你据实判断,不要凭空描述未出现的内容。",
            images.len(),
            names.join("、")
        ),
    }));
    for (_, url) in images {
        blocks.push(json!({ "type": "image_url", "image_url": { "url": url } }));
    }
    json!({ "role": "user", "content": blocks })
}

fn emit_text_chunks(sink: &(dyn Fn(LoopEvent) + Send + Sync), text: &str) {
    if text.is_empty() {
        return;
    }
    let chars: Vec<char> = text.chars().collect();
    for chunk in chars.chunks(8) {
        sink(LoopEvent::TextDelta(chunk.iter().collect()));
    }
}

/// POST /api/forge/llm/chat 处理:provider 分派。
pub async fn chat(
    axum::Json(req): axum::Json<ChatRequest>,
) -> Result<axum::Json<Value>, axum::response::Response> {
    if req.text.trim().is_empty() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({ "error": { "code": "EMPTY_TEXT", "message": "text 不可空" } })),
        )
            .into_response());
    }
    match resolve_provider() {
        Provider::Mock => Ok(axum::Json(json!({
            "provider": "mock",
            "text": mock_reply_text(&req.text),
            "toolCalls": [],
            "iters": 0,
        }))),
        Provider::Deepseek(key) => match run_deepseek_loop(&req.text, &key).await {
            Ok((text, records, iters)) => Ok(axum::Json(json!({
                "provider": "deepseek",
                "text": text,
                "toolCalls": records
                    .iter()
                    .map(|r| json!({ "name": r.name, "ok": r.ok, "summary": r.summary }))
                    .collect::<Vec<_>>(),
                "iters": iters,
            }))),
            Err(e) => Err((
                axum::http::StatusCode::BAD_GATEWAY,
                axum::Json(json!({
                    "error": { "code": "LLM_UPSTREAM_ERROR", "message": e.to_string() }
                })),
            )
                .into_response()),
        },
        // llm/chat 无模型选择面:resolve_provider 只产 Mock/Deepseek;
        // openai-compat 渠道经 agent.rs ask:execute(selectedModelId)分派,不走本路由。
        Provider::OpenAiCompat { .. } | Provider::OpenAiCompatNotConfigured => {
            unreachable!("llm/chat resolve_provider 不产 openai-compat 分支")
        }
    }
}

// ---------- F7 wave.5:POST /api/forge/llm/key(设置·模型页 deepseek 渠道配置面) ----------

#[derive(Deserialize)]
pub struct LlmKeyRequest {
    /// deepseek API Key;只写 gend keystore["deepseek"],永不回显/永不进日志(R-5)。
    #[serde(rename = "apiKey")]
    api_key: String,
}

/// 写 keystore["deepseek"](复用 gend::keystore::set_key 读-改-写面,DPAPI 落盘形态不变)。
/// 响应只给 {ok, configured}(configured = 写后 availability 重判定);空 key → 400 EMPTY_KEY;
/// 写盘失败 → 500 FORGE_IO(错误信息仅 IO 面,不带 key 值)。
pub async fn set_llm_key(axum::Json(req): axum::Json<LlmKeyRequest>) -> axum::response::Response {
    let key = req.api_key.trim();
    if key.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({ "error": { "code": "EMPTY_KEY", "message": "apiKey 不可空" } })),
        )
            .into_response();
    }
    if let Err(e) = gend::keystore::set_key("deepseek", key) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
        )
            .into_response();
    }
    axum::Json(json!({ "ok": true, "configured": deepseek_key_available() })).into_response()
}

// ---------- F8 wave.2:openai-compatible 通用渠道(设置·模型页渠道卡 + ask:execute provider 分支) ----------

/// keystore 条目 id(R-5:key 只进 keystore/Authorization 头,永不落本模块 JSON/响应/日志)。
pub(crate) const OPENAI_COMPAT_KEYSTORE_ID: &str = "openai-compat";
/// 会话选模 id(design-snapshot models 条目 id;client 菜单经 snapshot 数据面自动纳入)。
pub(crate) const OPENAI_COMPAT_MODEL_ID: &str = "openai-compat";
/// 未配置显式错误码(GEN_BACKEND_NOT_CONFIGURED 同族精神;消息面不含 key/机密)。
pub(crate) const OPENAI_COMPAT_NOT_CONFIGURED: &str = "OPENAI_COMPAT_NOT_CONFIGURED";

/// openai-compat 配置文件(baseUrl/model 面;key 永不进本文件——走 keystore)。
/// 路径 = gend::config::data_dir()/llm-openai-compat.json(与 keystore/gen-backends 同根)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct OpenAiCompatFile {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    /// 该端点后面接的模型是否收图片。无法从端点探知(同一个 baseUrl 可以换任意模型),
    /// 故由用户在渠道配置里声明;缺省 false = 不发图,宁可少一项能力也不发出会被拒的请求。
    #[serde(default)]
    pub vision: bool,
}

/// 配置 JSON 读-改-写串行锁(同 sessions/todos 纪律;整文件原子写)。
static OPENAI_COMPAT_FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn openai_compat_config_path() -> PathBuf {
    gend::config::data_dir().join("llm-openai-compat.json")
}

/// 读配置文件;缺失/解析失败按未配置处理(eprintln 如实,不含机密面)。
pub(crate) fn load_openai_compat_file() -> OpenAiCompatFile {
    let path = openai_compat_config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("llm-openai-compat.json 解析失败({}): {e},按未配置处理", path.display());
            OpenAiCompatFile::default()
        }),
        Err(_) => OpenAiCompatFile::default(),
    }
}

/// 原子写:tmp 全量写 + rename(调用方须持 OPENAI_COMPAT_FILE_LOCK)。
fn save_openai_compat_file(cfg: &OpenAiCompatFile) -> std::io::Result<()> {
    let path = openai_compat_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
}

/// keystore["openai-compat"] 密钥面(key_for 语义含 FORGE_GEN_API_KEY 共享 dev-key 覆盖,如实)。
fn openai_compat_key() -> Option<String> {
    let ks = gend::keystore::Keystore::load();
    match ks.key_for(OPENAI_COMPAT_KEYSTORE_ID) {
        Some(k) if !k.is_empty() => Some(k),
        _ => None,
    }
}

/// 状态面(REST status 与 snapshot availability 同源;只布尔+baseUrl+model,绝无 key)。
pub(crate) struct OpenAiCompatStatus {
    pub configured: bool,
    pub base_url: String,
    pub model: String,
    pub key_configured: bool,
    pub vision: bool,
}

pub(crate) fn openai_compat_status() -> OpenAiCompatStatus {
    let file = load_openai_compat_file();
    let key_configured = openai_compat_key().is_some();
    let configured = !file.base_url.is_empty() && !file.model.is_empty() && key_configured;
    OpenAiCompatStatus {
        configured,
        base_url: file.base_url,
        model: file.model,
        key_configured,
        vision: file.vision,
    }
}

/// 该 provider 能否收图片。判定看「实际要调的渠道」而非会话选了什么名字。
/// deepseek chat 与本地 mock 都没有视觉面 → 恒 false(发过去只会被拒或当噪声);
/// openai-compat 端点背后接什么模型只有人知道 → 读渠道配置里用户声明的那一位。
pub(crate) fn provider_vision(provider: &Provider) -> bool {
    matches!(provider, Provider::OpenAiCompat { .. }) && load_openai_compat_file().vision
}

/// provider 解析:baseUrl+model+key 全齐 → Some;任一缺 → None(调用方走显式 NOT_CONFIGURED)。
pub(crate) fn resolve_openai_compat() -> Option<(String, String, String)> {
    let file = load_openai_compat_file();
    if file.base_url.is_empty() || file.model.is_empty() {
        return None;
    }
    openai_compat_key().map(|k| (file.base_url, file.model, k))
}

/// 未配置步进:首轮即 Err(显式 OPENAI_COMPAT_NOT_CONFIGURED;agent.rs 选中未配齐分支复用)。
pub fn openai_compat_not_configured_step() -> Box<StepFn> {
    Box::new(|_m, _t, _s| {
        Box::pin(async move {
            Err(LlmError(format!(
                "{OPENAI_COMPAT_NOT_CONFIGURED}: openai-compat 渠道未配齐(baseUrl/model/key 缺一);请在 设置→模型 页配置"
            )))
        })
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiCompatConfigRequest {
    /// OpenAI 兼容端点根(调用时拼 /v1/chat/completions);必填,空 → 400 EMPTY_BASE_URL。
    #[serde(default)]
    base_url: String,
    /// 模型名(请求体 model 域);必填,空 → 400 EMPTY_MODEL。
    #[serde(default)]
    model: String,
    /// API Key;可省略 = 只改 baseUrl/model;非空 → 写 keystore["openai-compat"](永不回显)。
    #[serde(default)]
    key: Option<String>,
    /// 该模型是否收图片;省略 = 保留既有(免得只改 baseUrl 的请求把它悄悄关掉)。
    #[serde(default)]
    vision: Option<bool>,
}

/// POST /api/forge/llm/openai-compat/config {baseUrl, model, key?}。
/// 空 baseUrl → 400 EMPTY_BASE_URL;空 model → 400 EMPTY_MODEL;写盘失败 → 500 FORGE_IO(仅 IO 面)。
/// 响应 {ok, configured, baseUrl, model, keyConfigured}——绝无 key(R-5)。
pub async fn set_openai_compat_config(
    axum::Json(req): axum::Json<OpenAiCompatConfigRequest>,
) -> axum::response::Response {
    let base_url = req.base_url.trim().to_string();
    if base_url.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(
                json!({ "error": { "code": "EMPTY_BASE_URL", "message": "baseUrl 不可空" } }),
            ),
        )
            .into_response();
    }
    let model = req.model.trim().to_string();
    if model.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(json!({ "error": { "code": "EMPTY_MODEL", "message": "model 不可空" } })),
        )
            .into_response();
    }
    // baseUrl/model 落 JSON(Mutex 串行 + 原子写;key 不进本文件)。
    {
        let _g = OPENAI_COMPAT_FILE_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let file = OpenAiCompatFile {
            base_url: base_url.clone(),
            model: model.clone(),
            vision: req.vision.unwrap_or_else(|| load_openai_compat_file().vision),
        };
        if let Err(e) = save_openai_compat_file(&file) {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
            )
                .into_response();
        }
    }
    // key 非空 → 写 keystore(读-改-写保留其他条目;错误仅 IO 面不带 key 值)。
    if let Some(k) = req.key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
        if let Err(e) = gend::keystore::set_key(OPENAI_COMPAT_KEYSTORE_ID, k) {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({ "error": { "code": "FORGE_IO", "message": e.to_string() } })),
            )
                .into_response();
        }
    }
    let st = openai_compat_status();
    axum::Json(json!({
        "ok": true,
        "configured": st.configured,
        "baseUrl": st.base_url,
        "model": st.model,
        "keyConfigured": st.key_configured,
        "vision": st.vision,
    }))
    .into_response()
}

/// GET /api/forge/llm/openai-compat/status → {configured, baseUrl, model, keyConfigured, vision}(无 key)。
pub async fn openai_compat_status_handler() -> axum::Json<Value> {
    let st = openai_compat_status();
    axum::Json(json!({
        "configured": st.configured,
        "baseUrl": st.base_url,
        "model": st.model,
        "keyConfigured": st.key_configured,
        "vision": st.vision,
    }))
}

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    /// 环境变量互斥锁(provider 解析读进程 env,并行测试须互斥;main.rs 路由测试共用)。
    use super::TEST_ENV_LOCK as ENV_LOCK;

    #[test]
    fn openai_tools_conversion() {
        let mcp_tools = vec![
            json!({ "name": "mcp__engine-scene__entity_create", "description": "创建实体", "inputSchema": { "type": "object", "properties": { "name": { "type": "string" } } } }),
            json!({ "name": "mcp__engine-scene__scene_summary", "description": "场景摘要" }),
        ];
        let tools = to_openai_tools(&mcp_tools);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(
            tools[0]["function"]["name"],
            "mcp__engine-scene__entity_create"
        );
        assert_eq!(tools[0]["function"]["parameters"]["type"], "object");
        // 缺 inputSchema 的补 {"type":"object"}
        assert_eq!(tools[1]["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn request_body_never_contains_key() {
        // R-5 组装点扫描:密钥只进 Authorization 头,body 全文不得含密钥子串。
        let key = "sk-test-SECRET-assembly";
        let messages = vec![json!({ "role": "user", "content": "hi" })];
        let tools = to_openai_tools(&[json!({ "name": "t", "description": "d" })]);
        let body = build_request_body(DEEPSEEK_MODEL, &messages, &tools, &RequestSpec::default());
        let body_text = body.to_string();
        assert!(!body_text.contains(key), "请求体含密钥子串: {body_text}");
        assert_eq!(body["model"], DEEPSEEK_MODEL);
        assert_eq!(body["tool_choice"], "auto");
    }

    /// 规格波:默认 spec 的 body 与规格波之前逐键一致(不多出 reasoning_effort);
    /// 有 spec 时 model 被覆盖、reasoning_effort 如实出现。
    #[test]
    fn request_body_carries_spec_only_when_set() {
        let messages = vec![json!({ "role": "user", "content": "hi" })];
        let tools: Vec<Value> = Vec::new();
        let plain = build_request_body(DEEPSEEK_MODEL, &messages, &tools, &RequestSpec::default());
        assert!(
            plain.get("reasoning_effort").is_none(),
            "默认 spec 不该给请求体加字段"
        );
        assert_eq!(plain.as_object().unwrap().len(), 5);

        let spec = RequestSpec {
            model: Some("deepseek-reasoner".to_string()),
            reasoning_effort: Some("xhigh".to_string()),
        };
        let body = build_request_body(DEEPSEEK_MODEL, &messages, &tools, &spec);
        assert_eq!(body["model"], "deepseek-reasoner");
        assert_eq!(body["reasoning_effort"], "xhigh");
    }

    #[test]
    fn provider_resolution_prefers_llm_env() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("FORGE_LLM_API_KEY", "sk-env-llm");
        std::env::remove_var("FORGE_GEN_API_KEY");
        match resolve_provider() {
            Provider::Deepseek(k) => assert_eq!(k, "sk-env-llm"),
            _ => panic!("FORGE_LLM_API_KEY 应命中 deepseek"),
        }
        std::env::remove_var("FORGE_LLM_API_KEY");
    }

    #[test]
    fn provider_falls_back_to_mock_without_key() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("FORGE_LLM_API_KEY");
        std::env::remove_var("FORGE_GEN_API_KEY");
        // keystore 指到不存在目录 → 空 keystore。
        let dir = std::env::temp_dir().join(format!("agentd-llm-{}", std::process::id()));
        std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
        assert_eq!(resolve_provider(), Provider::Mock);
        std::env::remove_var("FORGE_GEN_DATA_DIR");
    }

    #[test]
    fn truncate_respects_char_boundary() {
        let s = "创建三个立方体";
        assert_eq!(truncate_chars(s, 3), "创建三…");
        assert_eq!(truncate_chars(s, 100), s);
    }

    // ---------- F7 wave.2:run_tool_loop 可注入核心(全内存 scripted fake,禁网络/子进程) ----------

    /// scripted step:依次弹出预置 assistant message;记录每轮收到的 tools。
    fn scripted_step(
        messages: Vec<Value>,
        tools_seen: std::sync::Arc<std::sync::Mutex<Vec<usize>>>,
    ) -> Box<StepFn> {
        let queue = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
            messages,
        )));
        Box::new(move |_msgs, tools, _s| {
            tools_seen.lock().unwrap().push(tools.len());
            let next = queue.lock().unwrap().pop_front().expect("script 耗尽");
            Box::pin(async move {
                Ok(StepOutcome {
                    message: next,
                    usage: None,
                })
            })
        })
    }
    fn tool_call_msg(name: &str, args: &str) -> Value {
        json!({
            "role": "assistant",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": { "name": name, "arguments": args },
            }],
        })
    }
    fn final_msg(text: &str) -> Value {
        json!({ "role": "assistant", "content": text })
    }
    /// 事件收集 sink(类型序列 + 细节)。
    fn collect_sink() -> (
        impl Fn(LoopEvent) + Send + Sync,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log2 = log.clone();
        let sink = move |ev: LoopEvent| {
            let s = match ev {
                LoopEvent::ToolInvoked { name, .. } => format!("invoked:{name}"),
                LoopEvent::ToolCompleted { name, duration_ms, .. } => {
                    format!("completed:{name}:{duration_ms}")
                }
                LoopEvent::ToolFailed { name, error, .. } => format!("failed:{name}:{error}"),
                LoopEvent::ToolDenied { name, error, .. } => format!("denied:{name}:{error}"),
                LoopEvent::Reasoning(_) => return,
                LoopEvent::Usage(u) => format!("usage:{}", u.total_tokens),
                LoopEvent::TextDelta(_)
                | LoopEvent::ReasoningDelta(_)
                | LoopEvent::ToolArgsDelta { .. }
                | LoopEvent::StreamReset => return,
            };
            log2.lock().unwrap().push(s);
        };
        (sink, log)
    }

    #[tokio::test]
    async fn loop_one_tool_call_then_final_event_order() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let step = scripted_step(
            vec![
                tool_call_msg("mcp__engine-scene__entity_list", "{}"),
                final_msg("终稿"),
            ],
            seen.clone(),
        );
        let execute: Box<ExecFn> = Box::new(|name, _args| {
            Box::pin(async move { (true, format!("{name} ok").into()) })
        });
        let (sink, log) = collect_sink();
        let out = run_tool_loop(
            "sys",
            "用户输入",
            ToolLoopCfg {
                tools: vec![json!({"type":"function"})],
                step: step.as_ref(),
                execute: execute.as_ref(),
                vision: false,
                sink: Some(&sink),
                forbidden: None,
                cancelled: None,
                stream: None,
                preamble: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(out.text, "终稿");
        assert_eq!(out.iters, 2);
        assert!(!out.cancelled);
        assert_eq!(out.records.len(), 1);
        assert!(out.records[0].ok);
        assert_eq!(out.records[0].name, "mcp__engine-scene__entity_list");
        assert_eq!(seen.lock().unwrap().len(), 2, "两轮 step");
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], "invoked:mcp__engine-scene__entity_list");
        assert!(
            log[1].starts_with("completed:mcp__engine-scene__entity_list:"),
            "completed 带 durationMs≥0: {}",
            log[1]
        );
    }

    // ---------- 工具产出图片的多模态回注 ----------

    /// 用捕获 messages 的 step 跑一轮工具调用,返回工具执行后那一轮的上行消息序列。
    /// (scripted_step 只记录 tools 数量,看不到消息体,故这里另起一个捕获 step。)
    async fn messages_after_tool_with_images(vision: bool) -> Vec<Value> {
        let seen: std::sync::Arc<std::sync::Mutex<Vec<Vec<Value>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_step = seen.clone();
        let queue = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::from(
            vec![tool_call_msg("mcp__gen-model__gen_mesh", "{}"), final_msg("好了")],
        )));
        let step: Box<StepFn> = Box::new(move |msgs, _t, _s| {
            seen_step.lock().unwrap().push(msgs);
            let next = queue.lock().unwrap().pop_front().expect("script 耗尽");
            Box::pin(async move { Ok(StepOutcome { message: next, usage: None }) })
        });
        let execute: Box<ExecFn> = Box::new(|_n, _a| {
            Box::pin(async move {
                (
                    true,
                    ToolFeedback {
                        text: r#"{"candidates":[{"meshFileRef":"a.glb"}]}"#.to_string(),
                        images: vec![
                            "data:image/png;base64,AAA".to_string(),
                            "data:image/png;base64,BBB".to_string(),
                        ],
                    },
                )
            })
        });
        run_tool_loop(
            "sys",
            "生成一个木桶",
            ToolLoopCfg {
                tools: vec![],
                step: step.as_ref(),
                execute: execute.as_ref(),
                vision,
                sink: None,
                forbidden: None,
                cancelled: None,
                stream: None,
                preamble: None,
            },
        )
        .await
        .unwrap();
        // 末轮 step 的入参 = 工具执行后的完整消息序列。
        let calls = seen.lock().unwrap().clone();
        calls.last().cloned().unwrap_or_default()
    }

    #[tokio::test]
    async fn tool_images_become_user_image_blocks_when_vision_on() {
        let msgs = messages_after_tool_with_images(true).await;
        let last = msgs.last().expect("末条应是图片消息");
        assert_eq!(last["role"], "user");
        let blocks = last["content"].as_array().expect("content 应为多模态块数组");
        // 首块文字交代来历,其后每图一块。
        assert_eq!(blocks[0]["type"], "text");
        assert!(blocks[0]["text"].as_str().unwrap().contains("gen_mesh"));
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1]["type"], "image_url");
        assert_eq!(blocks[1]["image_url"]["url"], "data:image/png;base64,AAA");
        assert_eq!(blocks[2]["image_url"]["url"], "data:image/png;base64,BBB");
        // 图片不进 tool 消息:base64 会被 4000 字符截断成废串,还白烧 token。
        let tool_msg = msgs.iter().find(|m| m["role"] == "tool").expect("须有 tool 消息");
        assert!(tool_msg["content"].is_string());
        assert!(!tool_msg["content"].as_str().unwrap().contains("base64"));
        // tool 消息必须紧跟 assistant,图片消息排在其后(否则 tool_call 配对被打断)。
        let tool_idx = msgs.iter().position(|m| m["role"] == "tool").unwrap();
        assert_eq!(msgs[tool_idx - 1]["role"], "assistant");
        assert_eq!(tool_idx, msgs.len() - 2);
    }

    #[tokio::test]
    async fn tool_images_dropped_when_vision_off() {
        let msgs = messages_after_tool_with_images(false).await;
        // content 为数组即多模态块;assistant 那条本就没有 content(只有 tool_calls),
        // 故判「不是数组」而非「是字符串」。
        assert!(
            msgs.iter().all(|m| !m["content"].is_array()),
            "无视觉面的渠道不得出现多模态块: {msgs:?}"
        );
        assert_eq!(msgs.last().unwrap()["role"], "tool", "末条应是 tool,无追加图片消息");
    }

    /// 无 imageRefs 的普通结果不该被误判出图片;非法/越界路径静默跳过不炸。
    #[test]
    fn extract_tool_images_ignores_plain_and_bad_refs() {
        assert!(extract_tool_images("not json").is_empty());
        assert!(extract_tool_images(r#"{"ok":true}"#).is_empty());
        assert!(extract_tool_images(r#"{"imageRefs":[]}"#).is_empty());
        assert!(extract_tool_images(r#"{"imageRefs":["../../etc/passwd"]}"#).is_empty());
        assert!(extract_tool_images(r#"{"imageRefs":[".forge/tmp/gen/nope.png"]}"#).is_empty());
    }

    #[test]
    fn tool_image_message_shape() {
        let m = tool_image_message(&[
            ("gen_mesh".to_string(), "data:image/png;base64,X".to_string()),
            ("gen_mesh".to_string(), "data:image/png;base64,Y".to_string()),
        ]);
        assert_eq!(m["role"], "user");
        let blocks = m["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        // 同一工具名去重,不重复罗列。
        assert_eq!(blocks[0]["text"].as_str().unwrap().matches("gen_mesh").count(), 1);
    }

    #[tokio::test]
    async fn loop_executor_failure_emits_failed_and_continues() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let step = scripted_step(
            vec![tool_call_msg("mcp__engine-scene__entity_get", "{}"), final_msg("收尾")],
            seen,
        );
        let execute: Box<ExecFn> = Box::new(|_n, _a| Box::pin(async move { (false, "假失败".into()) }));
        let (sink, log) = collect_sink();
        let out = run_tool_loop(
            "sys",
            "x",
            ToolLoopCfg {
                tools: vec![],
                step: step.as_ref(),
                execute: execute.as_ref(),
                vision: false,
                sink: Some(&sink),
                forbidden: None,
                cancelled: None,
                stream: None,
                preamble: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(out.text, "收尾", "工具失败不打断循环");
        assert!(!out.records[0].ok);
        let log = log.lock().unwrap();
        assert_eq!(log[0], "invoked:mcp__engine-scene__entity_get");
        assert!(
            log[1].starts_with("failed:mcp__engine-scene__entity_get:假失败"),
            "failed 如实: {}",
            log[1]
        );
    }

    #[tokio::test]
    async fn loop_forbidden_tool_not_executed() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let step = scripted_step(
            vec![
                tool_call_msg("mcp__engine-scene__entity_create", "{}"),
                final_msg("计划"),
            ],
            seen,
        );
        let executed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let executed2 = executed.clone();
        let execute: Box<ExecFn> = Box::new(move |name, _a| {
            executed2.lock().unwrap().push(name);
            Box::pin(async move { (true, "ok".into()) })
        });
        let (sink, log) = collect_sink();
        let forbid = |n: &str| n == "mcp__engine-scene__entity_create";
        let out = run_tool_loop(
            "sys",
            "x",
            ToolLoopCfg {
                tools: vec![],
                step: step.as_ref(),
                execute: execute.as_ref(),
                vision: false,
                sink: Some(&sink),
                forbidden: Some(&forbid),
                cancelled: None,
                stream: None,
                preamble: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(out.text, "计划");
        assert!(executed.lock().unwrap().is_empty(), "禁用工具不得执行");
        assert!(!out.records[0].ok);
        assert!(out.records[0].summary.starts_with("TOOL_FORBIDDEN"));
        let log = log.lock().unwrap();
        assert_eq!(log.len(), 2);
        assert!(log[1].contains("TOOL_FORBIDDEN"), "{}", log[1]);
    }

    #[tokio::test]
    async fn loop_cancel_between_iterations() {
        // step 恒产工具调用;executor 首调后置取消旗 → 第二迭代开头收束 cancelled。
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag2 = flag.clone();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let step = scripted_step(
            vec![
                tool_call_msg("mcp__engine-scene__entity_list", "{}"),
                tool_call_msg("mcp__engine-scene__entity_list", "{}"),
                final_msg("不应到达"),
            ],
            seen,
        );
        let execute: Box<ExecFn> = Box::new(move |_n, _a| {
            flag2.store(true, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move { (true, "ok".into()) })
        });
        let flag3 = flag.clone();
        let cancelled = move || flag3.load(std::sync::atomic::Ordering::SeqCst);
        let (sink, log) = collect_sink();
        let out = run_tool_loop(
            "sys",
            "x",
            ToolLoopCfg {
                tools: vec![],
                step: step.as_ref(),
                execute: execute.as_ref(),
                vision: false,
                sink: Some(&sink),
                forbidden: None,
                cancelled: Some(&cancelled),
                stream: None,
                preamble: None,
            },
        )
        .await
        .unwrap();
        assert!(out.cancelled);
        assert_eq!(out.text, "");
        assert_eq!(out.iters, 1, "仅完成一迭代");
        assert_eq!(log.lock().unwrap().len(), 2, "invoked+completed 后取消");
    }

    #[tokio::test]
    async fn loop_usage_event_only_when_present() {
        // 带 usage 的 step 一轮终稿 → Usage 事件;无 usage(如 mock)→ 无事件。
        let with_usage: Box<StepFn> = Box::new(|_m, _t, _s| {
            Box::pin(async move {
                Ok(StepOutcome {
                    message: final_msg("ok"),
                    usage: Some(Usage {
                        prompt_tokens: 3,
                        completion_tokens: 2,
                        total_tokens: 5,
                    }),
                })
            })
        });
        let execute: Box<ExecFn> = Box::new(|_n, _a| Box::pin(async move { (true, "x".into()) }));
        let (sink, log) = collect_sink();
        run_tool_loop(
            "sys",
            "x",
            ToolLoopCfg {
                tools: vec![],
                step: with_usage.as_ref(),
                execute: execute.as_ref(),
                vision: false,
                sink: Some(&sink),
                forbidden: None,
                cancelled: None,
                stream: None,
                preamble: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(log.lock().unwrap().as_slice(), ["usage:5"]);
        let mock = mock_step();
        let (sink2, log2) = collect_sink();
        let out = run_tool_loop(
            "sys",
            "你好",
            ToolLoopCfg {
                tools: vec![],
                step: mock.as_ref(),
                execute: execute.as_ref(),
                vision: false,
                sink: Some(&sink2),
                forbidden: None,
                cancelled: None,
                stream: None,
                preamble: None,
            },
        )
        .await
        .unwrap();
        assert!(out.text.contains("mock:已收到「你好」"), "mock 步进终稿: {}", out.text);
        assert!(log2.lock().unwrap().is_empty(), "mock 无 usage 事件");
    }

    // ---------- F8 wave.2:openai-compat 渠道(配置面/keystore 红线/显式错误/mock HTTP 闭环) ----------

    /// 隔离数据目录 + env 守卫(三环境变量统一清/复;锁由调用方持有)。
    struct DataDirGuard {
        dir: PathBuf,
    }
    impl DataDirGuard {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "agentd-oai-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::env::remove_var("FORGE_LLM_API_KEY");
            std::env::remove_var("FORGE_GEN_API_KEY");
            std::env::set_var("FORGE_GEN_DATA_DIR", &dir);
            DataDirGuard { dir }
        }
    }
    impl Drop for DataDirGuard {
        fn drop(&mut self) {
            std::env::remove_var("FORGE_GEN_DATA_DIR");
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[tokio::test]
    async fn oai_config_write_read_roundtrip_and_key_only_in_keystore() {
        let _g = env_lock();
        let guard = DataDirGuard::new("roundtrip");
        let secret = "sk-test-oai-REDLINE-roundtrip";
        // 未配置前置:status configured=false / keyConfigured=false。
        let v = openai_compat_status_handler().await.0;
        assert_eq!(v["configured"], false);
        assert_eq!(v["keyConfigured"], false);
        assert_eq!(v["baseUrl"], "");
        // POST config(baseUrl+model+key)→ {ok,configured,...} 响应面无 key。
        let resp = set_openai_compat_config(axum::Json(OpenAiCompatConfigRequest {
            base_url: "http://127.0.0.1:9100".to_string(),
            model: "qwen2.5-7b".to_string(),
            key: Some(secret.to_string()),
            vision: None,
        }))
        .await;
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["configured"], true);
        assert_eq!(v["baseUrl"], "http://127.0.0.1:9100");
        assert_eq!(v["model"], "qwen2.5-7b");
        assert_eq!(v["keyConfigured"], true);
        assert!(!v.to_string().contains(secret), "config 响应回显密钥(R-5): {v}");
        // 配置 JSON 落盘:含 baseUrl/model,绝不含 key 子串。
        let text = std::fs::read_to_string(guard.dir.join("llm-openai-compat.json")).unwrap();
        assert!(text.contains("http://127.0.0.1:9100"), "{text}");
        assert!(text.contains("qwen2.5-7b"), "{text}");
        assert!(!text.contains(secret), "配置 JSON 落密钥(R-5): {text}");
        assert!(!text.contains("sk-"), "配置 JSON 含 sk- 串(R-5): {text}");
        // key 进 keystore(条目独立,与 deepseek 不互踩见下);Windows DPAPI 密文无明文。
        let ks_text = std::fs::read_to_string(guard.dir.join("keystore.json")).unwrap();
        assert!(!ks_text.contains(secret), "keystore 落盘含明文(R-5): {ks_text}");
        let ks = gend::keystore::Keystore::load_from(&guard.dir.join("keystore.json"));
        assert_eq!(ks.key_for(OPENAI_COMPAT_KEYSTORE_ID).as_deref(), Some(secret));
        // resolve 三联 = (base_url, model, key)。
        let (bu, m, k) = resolve_openai_compat().expect("已配齐应 Some");
        assert_eq!((bu.as_str(), m.as_str(), k.as_str()), ("http://127.0.0.1:9100", "qwen2.5-7b", secret));
        // key 省略 = 只改 baseUrl/model;keystore 既有 key 保留。
        let resp = set_openai_compat_config(axum::Json(OpenAiCompatConfigRequest {
            base_url: "http://127.0.0.1:9200".to_string(),
            model: "glm-4-air".to_string(),
            key: None,
            vision: None,
        }))
        .await;
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["configured"], true, "key 省略后仍 configured: {v}");
        assert_eq!(v["baseUrl"], "http://127.0.0.1:9200");
        let ks2 = gend::keystore::Keystore::load_from(&guard.dir.join("keystore.json"));
        assert_eq!(
            ks2.key_for(OPENAI_COMPAT_KEYSTORE_ID).as_deref(),
            Some(secret),
            "key 省略不得清既有 keystore 条目"
        );
        // 渠道 key 不互踩:deepseek 条目写入后 openai-compat 原样。
        gend::keystore::set_key("deepseek", "sk-test-oai-deepseek-neighbor").unwrap();
        let ks3 = gend::keystore::Keystore::load_from(&guard.dir.join("keystore.json"));
        assert_eq!(ks3.key_for(OPENAI_COMPAT_KEYSTORE_ID).as_deref(), Some(secret));
        assert_eq!(ks3.key_for("deepseek").as_deref(), Some("sk-test-oai-deepseek-neighbor"));
        // status 响应面终态仍无 key。
        let v = openai_compat_status_handler().await.0;
        assert_eq!(v["configured"], true);
        assert!(!v.to_string().contains("sk-"), "status 响应含 sk- 串(R-5): {v}");
    }

    #[tokio::test]
    async fn oai_config_empty_fields_explicit_400() {
        let _g = env_lock();
        let _guard = DataDirGuard::new("empty");
        // 空 baseUrl → 400 EMPTY_BASE_URL。
        let resp = set_openai_compat_config(axum::Json(OpenAiCompatConfigRequest {
            base_url: "  ".to_string(),
            model: "m".to_string(),
            key: None,
            vision: None,
        }))
        .await;
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, axum::http::StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "EMPTY_BASE_URL");
        // 空 model → 400 EMPTY_MODEL。
        let resp = set_openai_compat_config(axum::Json(OpenAiCompatConfigRequest {
            base_url: "http://x".to_string(),
            model: "".to_string(),
            key: None,
            vision: None,
        }))
        .await;
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, axum::http::StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["error"]["code"], "EMPTY_MODEL");
        // 两次 400 后配置面仍未配置(无副作用落盘)。
        assert_eq!(load_openai_compat_file().base_url, "");
    }

    #[tokio::test]
    async fn oai_not_configured_explicit_error_and_resolve_none() {
        let _g = env_lock();
        let _guard = DataDirGuard::new("notcfg");
        // 空目录:resolve → None;not_configured 步进首轮即显式错。
        assert!(resolve_openai_compat().is_none());
        let step = openai_compat_not_configured_step();
        let execute: Box<ExecFn> = Box::new(|_n, _a| Box::pin(async move { (true, "x".into()) }));
        let err = match run_tool_loop(
            "sys",
            "你好",
            ToolLoopCfg {
                tools: vec![],
                step: step.as_ref(),
                execute: execute.as_ref(),
                vision: false,
                sink: None,
                forbidden: None,
                cancelled: None,
                stream: None,
                preamble: None,
            },
        )
        .await
        {
            Ok(_) => panic!("未配置步进应 Err"),
            Err(e) => e,
        };
        assert!(
            err.to_string().starts_with(OPENAI_COMPAT_NOT_CONFIGURED),
            "显式 NOT_CONFIGURED 同族: {err}"
        );
        // 缺 key 腿(有 baseUrl/model 无 key)同样 None + 显式错;消息面不含 key 域值。
        {
            let _f = OPENAI_COMPAT_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            save_openai_compat_file(&OpenAiCompatFile {
                base_url: "http://127.0.0.1:1".to_string(),
                model: "m".to_string(),
                vision: false,
            })
            .unwrap();
        }
        assert!(resolve_openai_compat().is_none(), "缺 key 不得判 configured");
        assert!(!openai_compat_status().configured);
        assert!(!openai_compat_status().key_configured);
    }

    #[test]
    fn oai_provider_debug_never_prints_key() {
        let p = Provider::OpenAiCompat {
            base_url: "http://x".to_string(),
            model: "m".to_string(),
            key: "sk-test-oai-REDLINE-debug".to_string(),
        };
        let dbg = format!("{p:?}");
        assert!(!dbg.contains("sk-test-oai-REDLINE-debug"), "Debug 泄漏密钥(R-5): {dbg}");
        assert!(dbg.contains("<redacted>"), "{dbg}");
    }

    /// mock OpenAI 兼容 HTTP 服务器:首个无 tool 角色请求 → tool_calls 响应;
    /// 含 tool 角色请求 → 终稿文本;逐项记录请求面(method/path/Auth 头/body)供断言。
    async fn spawn_oai_mock_server() -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<(String, String, String, Value)>>>,
    ) {
        use axum::{routing::post, Router};
        let seen: std::sync::Arc<std::sync::Mutex<Vec<(String, String, String, Value)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |req: axum::extract::Request| {
                let seen = seen2.clone();
                async move {
                    let auth = req
                        .headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    let bytes = axum::body::to_bytes(req.into_body(), 1 << 20).await.unwrap();
                    let body: Value = serde_json::from_slice(&bytes).unwrap();
                    seen.lock().unwrap().push((
                        "POST".to_string(),
                        "/v1/chat/completions".to_string(),
                        auth,
                        body.clone(),
                    ));
                    let has_tool_role = body
                        .get("messages")
                        .and_then(Value::as_array)
                        .map(|ms| ms.iter().any(|m| m.get("role").and_then(Value::as_str) == Some("tool")))
                        .unwrap_or(false);
                    if has_tool_role {
                        axum::Json(json!({
                            "choices": [{ "message": { "role": "assistant", "content": "openai-compat 终稿:立方体已创建" } }],
                            "usage": { "prompt_tokens": 11, "completion_tokens": 7, "total_tokens": 18 },
                        }))
                    } else {
                        axum::Json(json!({
                            "choices": [{ "message": {
                                "role": "assistant",
                                "tool_calls": [{
                                    "id": "call_oai_1",
                                    "type": "function",
                                    "function": { "name": "mcp__engine-scene__entity_create", "arguments": r#"{"name":"Cube"}"# },
                                }],
                            } }],
                            "usage": { "prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8 },
                        }))
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), seen)
    }

    #[tokio::test]
    async fn oai_mock_server_tool_loop_closed_round() {
        let (base_url, seen) = spawn_oai_mock_server().await;
        let secret = "sk-test-oai-REDLINE-mockhttp";
        // 规格波:顺带在这条真 HTTP 闭环里验证 reasoning_effort 确实进了实发请求体。
        let spec = RequestSpec {
            model: None,
            reasoning_effort: Some("xhigh".to_string()),
        };
        let step = openai_compat_step(&base_url, "qwen2.5-7b", secret, &spec);
        // 假 executor:不触 MCP,记录调用。
        let executed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let executed2 = executed.clone();
        let execute: Box<ExecFn> = Box::new(move |name, _a| {
            executed2.lock().unwrap().push(name);
            Box::pin(async move { (true, r#"{"ok":true,"entityId":42}"#.into()) })
        });
        let (sink, log) = collect_sink();
        let out = run_tool_loop(
            "sys",
            "创建一个立方体",
            ToolLoopCfg {
                tools: to_openai_tools(&[json!({ "name": "mcp__engine-scene__entity_create", "description": "d" })]),
                step: step.as_ref(),
                execute: execute.as_ref(),
                vision: false,
                sink: Some(&sink),
                forbidden: None,
                cancelled: None,
                stream: None,
                preamble: None,
            },
        )
        .await
        .unwrap();
        // 闭环:tool_calls → executor → 终稿;usage 事件两轮。
        assert_eq!(out.text, "openai-compat 终稿:立方体已创建");
        assert_eq!(out.iters, 2);
        assert_eq!(out.records.len(), 1);
        assert!(out.records[0].ok);
        assert_eq!(out.records[0].name, "mcp__engine-scene__entity_create");
        assert_eq!(executed.lock().unwrap().as_slice(), ["mcp__engine-scene__entity_create"]);
        {
            let log = log.lock().unwrap();
            assert_eq!(log.len(), 4, "usage+invoked+completed+usage: {log:?}");
            assert_eq!(log[0], "usage:8");
            assert_eq!(log[1], "invoked:mcp__engine-scene__entity_create");
            assert!(log[2].starts_with("completed:mcp__engine-scene__entity_create:"), "{}", log[2]);
            assert_eq!(log[3], "usage:18");
        }
        // 请求面:两发 POST /v1/chat/completions,Authorization Bearer = 配置 key;
        // body model = 配置模型;第二轮 messages 含 assistant tool_calls + role:tool 回注(格式复用 deepseek 分支)。
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "工具循环两发请求: {seen:?}");
        for (method, path, auth, body) in seen.iter() {
            assert_eq!(method, "POST");
            assert_eq!(path, "/v1/chat/completions");
            assert_eq!(auth, &format!("Bearer {secret}"), "Bearer 头 = 配置 key");
            assert_eq!(body["model"], "qwen2.5-7b");
            assert_eq!(body["tool_choice"], "auto");
            assert_eq!(body["reasoning_effort"], "xhigh", "会话规格未进实发请求体");
            assert!(!body.to_string().contains(secret), "请求体含密钥子串(R-5)");
        }
        let msgs2 = seen[1].3["messages"].as_array().unwrap();
        assert!(
            msgs2.iter().any(|m| m.get("tool_calls").and_then(Value::as_array).is_some()),
            "第二轮含 assistant tool_calls 回注: {msgs2:?}"
        );
        let tool_msg = msgs2
            .iter()
            .find(|m| m.get("role").and_then(Value::as_str) == Some("tool"))
            .expect("第二轮含 role:tool 回注");
        assert_eq!(tool_msg["tool_call_id"], "call_oai_1");
        assert!(tool_msg["content"].as_str().unwrap().contains("entityId"), "tool 回注带执行反馈");
    }
}
