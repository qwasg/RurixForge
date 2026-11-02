//! Anthropic Messages 适配(自定义模型「Anthropic 格式」;D-049)。
//!
//! 工具循环内部恒为 chat.completions 消息形态,本模块只在出入站两侧转换,口径与云网关
//! `cloud/internal/gateway/convert`(req_messages.go / msg_to_chat.go)一致:
//! - 请求:system/developer 消息拼成 `system`;user 的 image_url → image 块;assistant.tool_calls →
//!   tool_use;tool 消息 → user 的 tool_result(连续多条合并);相邻同角色合并;tools → input_schema;
//!   reasoning_effort → 思考预算(新款 Claude 走自适应 thinking + output_config.effort)。
//! - 响应:非流式 Messages 体 / SSE 事件流 → `{choices:[{message}], usage}`,交给
//!   `llm::parse_step_response`。带签名的思考块(及 redacted_thinking)以 `anthropic_content`
//!   挂回 assistant 消息,下一轮原样回注——Anthropic 要求工具轮次保留它们。
//! - 自动提示缓存:tools 末项、system 末块、末条消息末块各打一个 ephemeral 断点(Agent 循环
//!   每轮重发同一前缀;不认 cache_control 的兼容端点会忽略它)。
//!
//! 鉴权:`x-api-key` + `anthropic-version`(与云网关一致);非官方域名另带
//! `Authorization: Bearer`(第三方兼容端点多按 Bearer 鉴权)。密钥只进请求头(R-5)。

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};

use serde_json::{json, Map, Value};

use crate::llm::{LlmError, RequestSpec, StreamDelta, StreamSink};

pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";
const LABEL: &str = "anthropic-compat";
/// 非流式请求总超时(与 llm.rs HTTP_TIMEOUT_SECS 同值)。
const HTTP_TIMEOUT_SECS: u64 = 60;

// ---------- 请求 ----------

/// chat.completions 的 content(字符串 / 多模态数组 / null)→ 纯文本。
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p.get("type").and_then(Value::as_str).unwrap_or("text") == "text")
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// image_url → image 块:base64 data URI → base64 源;http(s) → url 源;其余丢弃。
fn image_block(url: &str) -> Option<Value> {
    if let Some(rest) = url.strip_prefix("data:") {
        let (header, data) = rest.split_once(',')?;
        let mut params = header.split(';');
        let media = params.next().filter(|m| !m.is_empty())?;
        if !header.to_ascii_lowercase().ends_with(";base64") {
            return None;
        }
        return Some(json!({
            "type": "image",
            "source": { "type": "base64", "media_type": media.to_ascii_lowercase(), "data": data },
        }));
    }
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        .then(|| json!({ "type": "image", "source": { "type": "url", "url": url } }))
}

fn user_blocks(content: &Value) -> Vec<Value> {
    match content {
        Value::String(s) if !s.is_empty() => vec![json!({ "type": "text", "text": s })],
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p.get("type").and_then(Value::as_str) {
                Some("image_url") => p
                    .pointer("/image_url/url")
                    .or_else(|| p.get("image_url"))
                    .and_then(Value::as_str)
                    .and_then(image_block),
                _ => p
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                    .map(|t| json!({ "type": "text", "text": t })),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Anthropic 只收 `[A-Za-z0-9_-]` 的工具调用 id;tool_use 与 tool_result 用同一映射,配对不受影响。
fn tool_id(id: &str) -> String {
    let mapped: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    if mapped.is_empty() {
        "call".to_string()
    } else {
        mapped
    }
}

/// arguments 串 → tool_use.input(必须是 JSON 对象;空或非法用 {})。
fn args_object(args: &Value) -> Value {
    let parsed = match args {
        Value::String(s) => serde_json::from_str::<Value>(s.trim()).ok(),
        Value::Object(_) => Some(args.clone()),
        _ => None,
    };
    parsed.filter(Value::is_object).unwrap_or_else(|| json!({}))
}

fn model_key(model: &str) -> String {
    model.to_ascii_lowercase().replace('.', "-")
}

/// 新款 Claude(自适应 thinking + output_config.effort,不再收 budget_tokens):
/// (adaptive, always_on, between_tools)。名字带厂商前缀/点号版本号也认。
fn adaptive_thinking(model: &str) -> (bool, bool, bool) {
    let m = model_key(model);
    let sonnet = m.contains("claude-sonnet-5-5");
    let opus = m.contains("claude-opus-5-5");
    let fable = m.contains("claude-fable-5-1");
    (sonnet || opus || fable, opus || fable, sonnet)
}

/// reasoning_effort → thinking.budget_tokens(0 = 不开)。
fn thinking_budget(effort: &str) -> u64 {
    match effort {
        "minimal" => 1_024,
        "low" => 2_048,
        "medium" => 8_192,
        "high" => 16_384,
        "xhigh" | "max" => 32_000,
        _ => 0,
    }
}

fn is_signed_thinking(block: &Value) -> bool {
    match block.get("type").and_then(Value::as_str) {
        Some("thinking") => block
            .get("signature")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty()),
        Some("redacted_thinking") => block
            .get("data")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty()),
        _ => false,
    }
}

/// 末条 assistant 用了工具却没有带签名的思考块(chat 历史重建出来的):开 thinking 会被拒。
fn last_assistant_tool_without_thinking(msgs: &[(String, Vec<Value>)]) -> bool {
    let Some((_, blocks)) = msgs.iter().rev().find(|(role, _)| role == "assistant") else {
        return false;
    };
    let uses_tool = blocks
        .iter()
        .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"));
    uses_tool && !blocks.iter().any(is_signed_thinking)
}

fn ephemeral() -> Value {
    json!({ "type": "ephemeral" })
}

/// chat.completions 形态(messages + OpenAI tools + 规格)→ Messages 请求体(不含 stream)。
pub(crate) fn build_body(
    model: &str,
    messages: &[Value],
    tools: &[Value],
    spec: &RequestSpec,
    max_tokens: u64,
) -> Value {
    let mut system: Vec<String> = Vec::new();
    let mut msgs: Vec<(String, Vec<Value>)> = Vec::new();
    let mut push = |role: &str, blocks: Vec<Value>| {
        if blocks.is_empty() {
            return;
        }
        match msgs.last_mut() {
            Some((last, existing)) if last == role => existing.extend(blocks),
            _ => msgs.push((role.to_string(), blocks)),
        }
    };
    for m in messages {
        let content = m.get("content").unwrap_or(&Value::Null);
        match m.get("role").and_then(Value::as_str).unwrap_or("") {
            "system" | "developer" => {
                let t = text_of(content);
                if !t.trim().is_empty() {
                    system.push(t);
                }
            }
            "user" => push("user", user_blocks(content)),
            "assistant" => {
                if let Some(native) = m
                    .get("anthropic_content")
                    .and_then(Value::as_array)
                    .filter(|a| !a.is_empty())
                {
                    push("assistant", native.clone());
                    continue;
                }
                let mut blocks = Vec::new();
                let t = text_of(content);
                if !t.trim().is_empty() {
                    blocks.push(json!({ "type": "text", "text": t }));
                }
                for tc in m
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": tool_id(tc.get("id").and_then(Value::as_str).unwrap_or("")),
                        "name": tc.pointer("/function/name").and_then(Value::as_str).unwrap_or(""),
                        "input": args_object(tc.pointer("/function/arguments").unwrap_or(&Value::Null)),
                    }));
                }
                push("assistant", blocks);
            }
            "tool" => {
                let mut block = json!({
                    "type": "tool_result",
                    "tool_use_id": tool_id(m.get("tool_call_id").and_then(Value::as_str).unwrap_or("")),
                });
                let t = text_of(content);
                if !t.is_empty() {
                    block["content"] = json!(t);
                }
                if m.get("forgeToolSuccess").and_then(Value::as_bool) == Some(false) {
                    block["is_error"] = json!(true);
                }
                push("user", vec![block]);
            }
            _ => {}
        }
    }
    if msgs.first().map(|(role, _)| role.as_str()) != Some("user") {
        msgs.insert(
            0,
            ("user".to_string(), vec![json!({ "type": "text", "text": "(continue)" })]),
        );
    }

    let mut tools_out: Vec<Value> = tools
        .iter()
        .filter(|t| t.get("type").and_then(Value::as_str).unwrap_or("function") == "function")
        .filter_map(|t| {
            let f = t.get("function")?;
            let name = f.get("name").and_then(Value::as_str)?;
            let schema = f
                .get("parameters")
                .filter(|p| p.is_object())
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object" }));
            let mut tool = json!({ "name": name, "input_schema": schema });
            if let Some(d) = f.get("description").and_then(Value::as_str).filter(|d| !d.is_empty()) {
                tool["description"] = json!(d);
            }
            Some(tool)
        })
        .collect();

    // ---- 思考 ----
    let effort = spec
        .reasoning_effort
        .as_deref()
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| !e.is_empty() && e != "none");
    let off = effort.is_none() || spec.thinking_enabled == Some(false);
    let mut thinking: Option<Value> = None;
    let mut output_config: Option<Value> = None;
    let (adaptive, always_on, between_tools) = adaptive_thinking(model);
    if adaptive {
        let level = match effort.as_deref() {
            _ if off => "low",
            Some("minimal") | Some("low") => "low",
            Some(e @ ("medium" | "high" | "xhigh" | "max")) => e,
            _ => "medium",
        };
        output_config = Some(json!({ "effort": level }));
        thinking = Some(if off && between_tools {
            json!({ "type": "between_tools" })
        } else if off && !always_on {
            json!({ "type": "disabled" })
        } else if off {
            // 始终思考的模型关不掉:兼容口径的「关」只降强度并隐藏摘要。
            json!({ "type": "adaptive", "display": "omitted" })
        } else {
            json!({ "type": "adaptive", "display": "summarized" })
        });
    } else if !off && !last_assistant_tool_without_thinking(&msgs) {
        // budget_tokens 须 < max_tokens;给可见输出至少留 4096(不够就对半分),
        // 用户声明的「最大输出」是硬上限,不为思考预算擅自抬高。
        let room = if max_tokens > 4_096 + 1_024 {
            max_tokens - 4_096
        } else {
            max_tokens / 2
        };
        let budget = thinking_budget(effort.as_deref().unwrap_or("")).min(room);
        if budget >= 1_024 {
            thinking = Some(json!({ "type": "enabled", "budget_tokens": budget }));
        }
    }

    // ---- 提示缓存断点(≤3 个,上限 4) ----
    if let Some(last) = tools_out.last_mut() {
        last["cache_control"] = ephemeral();
    }
    if let Some((_, blocks)) = msgs.last_mut() {
        if let Some(block) = blocks.last_mut() {
            let cacheable = matches!(
                block.get("type").and_then(Value::as_str),
                Some("text" | "tool_result" | "image")
            );
            if cacheable && block.get("cache_control").is_none() {
                block["cache_control"] = ephemeral();
            }
        }
    }

    let mut body = Map::new();
    body.insert("model".into(), json!(model));
    body.insert("max_tokens".into(), json!(max_tokens));
    let system = system.join("\n\n");
    if !system.is_empty() {
        body.insert(
            "system".into(),
            json!([{ "type": "text", "text": system, "cache_control": ephemeral() }]),
        );
    }
    body.insert(
        "messages".into(),
        Value::Array(
            msgs.into_iter()
                .map(|(role, content)| json!({ "role": role, "content": content }))
                .collect(),
        ),
    );
    if !tools_out.is_empty() {
        body.insert("tools".into(), Value::Array(std::mem::take(&mut tools_out)));
        body.insert("tool_choice".into(), json!({ "type": "auto" }));
    }
    if let Some(t) = thinking {
        body.insert("thinking".into(), t);
    }
    if let Some(o) = output_config {
        body.insert("output_config".into(), o);
    }
    Value::Object(body)
}

// ---------- 传输 ----------

fn is_official_host(url: &str) -> bool {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest
        .split(|c| c == '/' || c == ':' || c == '?')
        .next()
        .unwrap_or("");
    host.eq_ignore_ascii_case("api.anthropic.com")
}

fn authed(agent: &ureq::Agent, url: &str, key: &str) -> ureq::Request {
    let mut req = agent
        .post(url)
        .set("x-api-key", key)
        .set("anthropic-version", ANTHROPIC_VERSION)
        .set("Content-Type", "application/json");
    if !is_official_host(url) {
        req = req.set("Authorization", &format!("Bearer {key}"));
    }
    req
}

fn send(req: ureq::Request, body: &Value) -> Result<ureq::Response, LlmError> {
    match req.send_string(&body.to_string()) {
        Ok(r) => Ok(r),
        Err(ureq::Error::Status(code, r)) => Err(crate::llm::llm_error_http(LABEL, code, r)),
        Err(ureq::Error::Transport(t)) => Err(LlmError::new(format!("{LABEL} 连接失败: {t}"))),
    }
}

/// 非流式 Messages 调用(阻塞;调用方 spawn_blocking)→ 原始 Messages 响应体。
pub(crate) fn post(url: &str, key: &str, body: &Value) -> Result<Value, LlmError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build();
    let resp = send(authed(&agent, url, key), body)?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut resp.into_reader(), &mut buf)
        .map_err(|e| LlmError::new(format!("读 {LABEL} 响应体失败: {e}")))?;
    serde_json::from_slice(&buf).map_err(|e| LlmError::new(format!("{LABEL} 响应非 JSON: {e}")))
}

/// 步进调用:有 sink 走 SSE(边读边回传增量),否则非流式;结果统一为 chat.completion 形态。
pub(crate) fn call(
    url: &str,
    key: &str,
    mut body: Value,
    sink: Option<&StreamSink>,
) -> Result<Value, LlmError> {
    let Some(sink) = sink else {
        return to_chat_completion(&post(url, key, &body)?);
    };
    body["stream"] = json!(true);
    // 流式不设总时长(大上下文 + 深思考的合法长轮次会被误杀,见 llm.rs chat_completions_stream):
    // 建连 15s、单次读空闲 120s。
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        .timeout_read(std::time::Duration::from_secs(120))
        .build();
    let resp = match send(authed(&agent, url, key).set("Accept", "text/event-stream"), &body) {
        Ok(r) => r,
        Err(e) => {
            sink(StreamDelta::Reset);
            return Err(e);
        }
    };
    let mut acc = StreamAcc::default();
    let mut event = String::new();
    let mut data: Vec<String> = Vec::new();
    for line in BufReader::new(resp.into_reader()).lines() {
        let line = line.map_err(|e| LlmError::new(format!("读 {LABEL} 流失败: {e}")))?;
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if !data.is_empty() {
                acc.feed(&event, &data.join("\n"), Some(sink))?;
            }
            event.clear();
            data.clear();
            if acc.done {
                break;
            }
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        if let Some(v) = line.strip_prefix("event:") {
            event = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("data:") {
            data.push(v.strip_prefix(' ').unwrap_or(v).to_string());
        }
    }
    if !data.is_empty() && !acc.done {
        acc.feed(&event, &data.join("\n"), Some(sink))?;
    }
    acc.finish()
}

// ---------- 响应 ----------

#[derive(Debug, Default, Clone, Copy)]
struct UsageAcc {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    seen: bool,
}

impl UsageAcc {
    /// message_start / 非流式体:各字段有则取。
    fn start(&mut self, u: Option<&Value>) {
        let Some(u) = u.filter(|u| u.is_object()) else {
            return;
        };
        self.seen = true;
        let get = |k: &str| u.get(k).and_then(Value::as_u64);
        if let Some(v) = get("input_tokens") {
            self.input = v;
        }
        if let Some(v) = get("output_tokens") {
            self.output = v;
        }
        if let Some(v) = get("cache_read_input_tokens") {
            self.cache_read = v;
        }
        if let Some(v) = get("cache_creation_input_tokens") {
            self.cache_write = v;
        }
    }

    /// message_delta:output 是累计值;输入/缓存字段非零才覆盖(兼容端点常给占位 0)。
    fn delta(&mut self, u: Option<&Value>) {
        let Some(u) = u.filter(|u| u.is_object()) else {
            return;
        };
        self.seen = true;
        let get = |k: &str| u.get(k).and_then(Value::as_u64);
        if let Some(v) = get("output_tokens") {
            self.output = v;
        }
        for (k, slot) in [
            ("input_tokens", &mut self.input),
            ("cache_read_input_tokens", &mut self.cache_read),
            ("cache_creation_input_tokens", &mut self.cache_write),
        ] {
            if let Some(v) = get(k).filter(|v| *v > 0) {
                *slot = v;
            }
        }
    }

    /// chat 口径:prompt_tokens 含缓存读写(上下文计量看的是窗口占用,不是计费输入)。
    fn json(&self) -> Value {
        if !self.seen {
            return json!({});
        }
        let prompt = self.input + self.cache_read + self.cache_write;
        json!({
            "prompt_tokens": prompt,
            "completion_tokens": self.output,
            "total_tokens": prompt + self.output,
            "prompt_tokens_details": { "cached_tokens": self.cache_read },
        })
    }
}

fn finish_reason(stop: Option<&str>, has_tools: bool) -> &'static str {
    match stop {
        Some("tool_use") => "tool_calls",
        Some("max_tokens" | "model_context_window_exceeded") => "length",
        Some("refusal") => "content_filter",
        _ if has_tools => "tool_calls",
        _ => "stop",
    }
}

fn upstream_error(v: &Value) -> LlmError {
    let e = v.get("error").unwrap_or(v);
    let kind = e.get("type").and_then(Value::as_str).unwrap_or("error");
    let message = e.get("message").and_then(Value::as_str).unwrap_or("无详情");
    LlmError::new(format!("{LABEL} 上游错误: {kind}: {message}"))
}

/// 非流式 Messages 响应体 → chat.completion 形态。
pub(crate) fn to_chat_completion(resp: &Value) -> Result<Value, LlmError> {
    if resp.get("type").and_then(Value::as_str) == Some("error") {
        return Err(upstream_error(resp));
    }
    let blocks = resp
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| LlmError::new(format!("{LABEL} 响应缺 content")))?;
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    for b in blocks {
        match b.get("type").and_then(Value::as_str) {
            Some("text") => text.push_str(b.get("text").and_then(Value::as_str).unwrap_or("")),
            Some("thinking") => {
                reasoning.push_str(b.get("thinking").and_then(Value::as_str).unwrap_or(""))
            }
            Some("tool_use") => tool_calls.push(json!({
                "id": b.get("id").and_then(Value::as_str).unwrap_or(""),
                "type": "function",
                "function": {
                    "name": b.get("name").and_then(Value::as_str).unwrap_or(""),
                    "arguments": args_object(b.get("input").unwrap_or(&Value::Null)).to_string(),
                },
            })),
            _ => {}
        }
    }
    let mut usage = UsageAcc::default();
    usage.start(resp.get("usage"));
    let native = blocks.iter().any(is_signed_thinking).then(|| blocks.clone());
    Ok(completion(
        text,
        reasoning,
        tool_calls,
        native,
        resp.get("stop_reason").and_then(Value::as_str),
        usage,
    ))
}

fn completion(
    text: String,
    reasoning: String,
    tool_calls: Vec<Value>,
    native: Option<Vec<Value>>,
    stop: Option<&str>,
    usage: UsageAcc,
) -> Value {
    let has_tools = !tool_calls.is_empty();
    let mut message = json!({ "role": "assistant", "content": text });
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if has_tools {
        message["tool_calls"] = json!(tool_calls);
    }
    if let Some(blocks) = native {
        message["anthropic_content"] = json!(blocks);
    }
    json!({
        "choices": [{ "message": message, "finish_reason": finish_reason(stop, has_tools) }],
        "usage": usage.json(),
    })
}

/// 一个 tool_use 块的累积(按块序号键)。
#[derive(Debug, Default)]
struct ToolAcc {
    call_index: u32,
    id: String,
    name: String,
    args: String,
}

/// SSE 事件流的累积器:正文/思考/工具参数边到边回传,原生块(含签名)按序号重组。
#[derive(Debug, Default)]
struct StreamAcc {
    text: String,
    reasoning: String,
    native: BTreeMap<u64, Value>,
    tools: BTreeMap<u64, ToolAcc>,
    usage: UsageAcc,
    stop: Option<String>,
    done: bool,
}

impl StreamAcc {
    fn append_native(&mut self, index: u64, field: &str, delta: &str) {
        if let Some(block) = self.native.get_mut(&index) {
            let prev = block.get(field).and_then(Value::as_str).unwrap_or("").to_string();
            block[field] = json!(prev + delta);
        }
    }

    fn emit_text(&mut self, t: &str, sink: Option<&StreamSink>) {
        if t.is_empty() {
            return;
        }
        self.text.push_str(t);
        if let Some(s) = sink {
            s(StreamDelta::Text(t.to_string()));
        }
    }

    fn emit_reasoning(&mut self, t: &str, sink: Option<&StreamSink>) {
        if t.is_empty() {
            return;
        }
        self.reasoning.push_str(t);
        if let Some(s) = sink {
            s(StreamDelta::Reasoning(t.to_string()));
        }
    }

    fn feed(&mut self, event: &str, data: &str, sink: Option<&StreamSink>) -> Result<(), LlmError> {
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return Ok(());
        };
        let kind = v.get("type").and_then(Value::as_str).unwrap_or(event);
        let index = v.get("index").and_then(Value::as_u64).unwrap_or(0);
        match kind {
            "message_start" => self.usage.start(v.pointer("/message/usage")),
            "content_block_start" => {
                let block = v.get("content_block").cloned().unwrap_or_else(|| json!({}));
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let t = block.get("text").and_then(Value::as_str).unwrap_or("").to_string();
                        self.emit_text(&t, sink);
                    }
                    Some("thinking") => {
                        let t = block
                            .get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        self.emit_reasoning(&t, sink);
                    }
                    Some("tool_use") => {
                        // 流式时 input 恒为 {},参数随 input_json_delta 到达。
                        let preset = block
                            .get("input")
                            .filter(|i| i.as_object().is_some_and(|o| !o.is_empty()))
                            .map(Value::to_string)
                            .unwrap_or_default();
                        let call_index = self.tools.len() as u32;
                        self.tools.insert(
                            index,
                            ToolAcc {
                                call_index,
                                id: block.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
                                name: block.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                                args: preset,
                            },
                        );
                    }
                    _ => {}
                }
                self.native.insert(index, block);
            }
            "content_block_delta" => {
                let delta = v.get("delta").cloned().unwrap_or_else(|| json!({}));
                let field = |k: &str| delta.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let t = field("text");
                        self.append_native(index, "text", &t);
                        self.emit_text(&t, sink);
                    }
                    Some("thinking_delta") => {
                        let t = field("thinking");
                        self.append_native(index, "thinking", &t);
                        self.emit_reasoning(&t, sink);
                    }
                    Some("signature_delta") => {
                        let s = field("signature");
                        self.append_native(index, "signature", &s);
                    }
                    Some("input_json_delta") => {
                        let part = field("partial_json");
                        if let Some(tool) = self.tools.get_mut(&index) {
                            tool.args.push_str(&part);
                            if let (Some(s), false) = (sink, part.is_empty()) {
                                s(StreamDelta::ToolArgs {
                                    index: tool.call_index,
                                    tool_call_id: tool.id.clone(),
                                    name: tool.name.clone(),
                                    delta: part,
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(tool) = self.tools.get(&index) {
                    let input = args_object(&json!(tool.args));
                    if let Some(block) = self.native.get_mut(&index) {
                        block["input"] = input;
                    }
                }
            }
            "message_delta" => {
                if let Some(stop) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop = Some(stop.to_string());
                }
                self.usage.delta(v.get("usage"));
            }
            "message_stop" => self.done = true,
            "error" => return Err(upstream_error(&v)),
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<Value, LlmError> {
        if !self.done && self.stop.is_none() {
            // 流在 message_stop 之前断掉:半截工具参数不能执行,交给步进重试(瞬时错误)。
            return Err(LlmError::new(format!("{LABEL} 流中断:未收到 message_stop")));
        }
        let mut tools: Vec<&ToolAcc> = self.tools.values().collect();
        tools.sort_by_key(|t| t.call_index);
        let tool_calls: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "arguments": if t.args.trim().is_empty() { "{}".to_string() } else { t.args.clone() },
                    },
                })
            })
            .collect();
        if self.text.trim().is_empty() && tool_calls.is_empty() {
            let hint = if matches!(self.stop.as_deref(), Some("max_tokens")) {
                "(已达最大输出上限,可在 设置→模型 调大该模型的「最大输出」)"
            } else {
                ""
            };
            return Err(LlmError::new(format!(
                "EMPTY_COMPLETION: 模型没有返回正文(也没有工具调用){hint}"
            )));
        }
        let native = self
            .native
            .values()
            .any(is_signed_thinking)
            .then(|| self.native.values().cloned().collect::<Vec<Value>>());
        Ok(completion(
            self.text,
            self.reasoning,
            tool_calls,
            native,
            self.stop.as_deref(),
            self.usage,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn spec(effort: Option<&str>) -> RequestSpec {
        RequestSpec {
            model: None,
            reasoning_effort: effort.map(str::to_string),
            thinking_enabled: None,
        }
    }

    fn tool(name: &str) -> Value {
        json!({ "type": "function", "function": { "name": name, "description": "d", "parameters": { "type": "object", "properties": {} } } })
    }

    #[test]
    fn request_maps_roles_tools_and_merges_tool_results() {
        let messages = vec![
            json!({ "role": "system", "content": "sys A" }),
            json!({ "role": "system", "content": "sys B" }),
            json!({ "role": "user", "content": "建两个方块" }),
            json!({ "role": "assistant", "content": "好的", "tool_calls": [
                { "id": "call:1", "type": "function", "function": { "name": "entity_create", "arguments": "{\"name\":\"a\"}" } },
                { "id": "call:2", "type": "function", "function": { "name": "entity_create", "arguments": "not json" } },
            ] }),
            json!({ "role": "tool", "tool_call_id": "call:1", "content": "ok a" }),
            json!({ "role": "tool", "tool_call_id": "call:2", "content": "ok b", "forgeToolSuccess": false }),
            json!({ "role": "user", "content": [
                { "type": "text", "text": "渲染图" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } },
            ] }),
        ];
        let body = build_body("glm-4.6", &messages, &[tool("entity_create")], &spec(None), 8192);
        assert_eq!(body["model"], "glm-4.6");
        assert_eq!(body["max_tokens"], 8192);
        assert_eq!(body["system"][0]["text"], "sys A\n\nsys B");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3, "user / assistant / user(工具结果 + 图片合并)");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"][1]["type"], "tool_use");
        assert_eq!(msgs[1]["content"][1]["id"], "call_1");
        assert_eq!(msgs[1]["content"][1]["input"]["name"], "a");
        assert_eq!(msgs[1]["content"][2]["input"], json!({}));
        let last = msgs[2]["content"].as_array().unwrap();
        assert_eq!(last[0]["type"], "tool_result");
        assert_eq!(last[0]["tool_use_id"], "call_1");
        assert_eq!(last[1]["is_error"], true);
        assert_eq!(last[2]["text"], "渲染图");
        assert_eq!(last[3]["source"]["type"], "base64");
        assert_eq!(last[3]["source"]["media_type"], "image/png");
        assert_eq!(last[3]["cache_control"]["type"], "ephemeral", "末条消息末块打缓存断点");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(body["tools"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["tool_choice"]["type"], "auto");
        assert!(body.get("thinking").is_none(), "思考关 → 不发 thinking");
    }

    #[test]
    fn thinking_budget_respects_declared_max_output() {
        let messages = vec![json!({ "role": "user", "content": "hi" })];
        let body = build_body("claude-haiku-4-5", &messages, &[], &spec(Some("max")), 32_768);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 28_672, "给可见输出留 4096");
        assert_eq!(body["max_tokens"], 32_768, "不擅自抬高最大输出");
        let small = build_body("deepseek-chat", &messages, &[], &spec(Some("medium")), 4_096);
        assert_eq!(small["thinking"]["budget_tokens"], 2_048);
        let tiny = build_body("x", &messages, &[], &spec(Some("high")), 1_500);
        assert!(tiny.get("thinking").is_none(), "预算不足 1024 不开思考");
        assert!(body.get("tools").is_none() && body.get("tool_choice").is_none());
    }

    #[test]
    fn adaptive_claude_uses_effort_and_keeps_thinking_rules() {
        let messages = vec![json!({ "role": "user", "content": "hi" })];
        let on = build_body("anthropic/claude-opus-5.5", &messages, &[], &spec(Some("xhigh")), 8192);
        assert_eq!(on["thinking"]["type"], "adaptive");
        assert_eq!(on["thinking"]["display"], "summarized");
        assert_eq!(on["output_config"]["effort"], "xhigh");
        assert!(on["thinking"].get("budget_tokens").is_none());
        let opus_off = build_body("claude-opus-5-5", &messages, &[], &spec(None), 8192);
        assert_eq!(opus_off["thinking"]["display"], "omitted", "始终思考的模型关不掉");
        assert_eq!(opus_off["output_config"]["effort"], "low");
        let sonnet_off = build_body("claude-sonnet-5-5", &messages, &[], &spec(None), 8192);
        assert_eq!(sonnet_off["thinking"]["type"], "between_tools");
    }

    #[test]
    fn tool_history_without_signed_thinking_disables_budget_thinking() {
        let messages = vec![
            json!({ "role": "user", "content": "go" }),
            json!({ "role": "assistant", "content": "", "tool_calls": [
                { "id": "t1", "type": "function", "function": { "name": "f", "arguments": "{}" } } ] }),
            json!({ "role": "tool", "tool_call_id": "t1", "content": "done" }),
        ];
        let body = build_body("claude-haiku-4-5", &messages, &[tool("f")], &spec(Some("high")), 32_768);
        assert!(body.get("thinking").is_none());
        // 带签名的原生块原样回注 → 可以继续思考。
        let mut native = messages.clone();
        native[1]["anthropic_content"] = json!([
            { "type": "thinking", "thinking": "plan", "signature": "sig" },
            { "type": "tool_use", "id": "t1", "name": "f", "input": {} },
        ]);
        let body = build_body("claude-haiku-4-5", &native, &[tool("f")], &spec(Some("high")), 32_768);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["messages"][1]["content"][0]["signature"], "sig");
    }

    #[test]
    fn non_stream_response_converts_to_chat_completion() {
        let resp = json!({
            "id": "msg_1", "type": "message", "stop_reason": "tool_use",
            "content": [
                { "type": "thinking", "thinking": "想一下", "signature": "s1" },
                { "type": "text", "text": "调用工具" },
                { "type": "tool_use", "id": "toolu_1", "name": "entity_create", "input": { "name": "a" } },
            ],
            "usage": { "input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 20 },
        });
        let out = to_chat_completion(&resp).unwrap();
        let msg = &out["choices"][0]["message"];
        assert_eq!(msg["content"], "调用工具");
        assert_eq!(msg["reasoning_content"], "想一下");
        assert_eq!(msg["tool_calls"][0]["id"], "toolu_1");
        assert_eq!(msg["tool_calls"][0]["function"]["arguments"], "{\"name\":\"a\"}");
        assert_eq!(msg["anthropic_content"].as_array().unwrap().len(), 3);
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(out["usage"]["prompt_tokens"], 130);
        assert_eq!(out["usage"]["completion_tokens"], 5);
        let step = crate::llm::parse_step_response(&out, LABEL).unwrap();
        assert_eq!(step.usage.unwrap().total_tokens, 135);
        // 上游 error 体如实成错。
        let err = to_chat_completion(&json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } }))
            .unwrap_err();
        assert!(err.to_string().contains("overloaded_error"));
    }

    fn collect() -> (StreamSink, Arc<Mutex<Vec<String>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let sink: StreamSink = Arc::new(move |d| {
            let tag = match d {
                StreamDelta::Text(t) => format!("text:{t}"),
                StreamDelta::Reasoning(t) => format!("think:{t}"),
                StreamDelta::ReasoningStarted { item_id } => format!("think-start:{item_id}"),
                StreamDelta::ReasoningDelta { item_id, delta } => format!("think:{item_id}:{delta}"),
                StreamDelta::ReasoningCompleted { item_id, text } => format!("think-end:{item_id}:{text}"),
                StreamDelta::ToolArgs { index, name, delta, .. } => format!("args:{index}:{name}:{delta}"),
                StreamDelta::Reset => "reset".to_string(),
            };
            s.lock().unwrap().push(tag);
        });
        (sink, seen)
    }

    #[test]
    fn stream_events_reassemble_text_tools_signature_and_usage() {
        let (sink, seen) = collect();
        let mut acc = StreamAcc::default();
        for (event, data) in [
            ("message_start", r#"{"type":"message_start","message":{"id":"m","usage":{"input_tokens":12,"cache_read_input_tokens":30}}}"#),
            ("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"plan"}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-"}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"two"}}"#),
            ("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
            ("content_block_start", r#"{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"好的"}}"#),
            ("content_block_stop", r#"{"type":"content_block_stop","index":1}"#),
            ("content_block_start", r#"{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_9","name":"entity_create","input":{}}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"name\":"}}"#),
            ("content_block_delta", r#"{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\"a\"}"}}"#),
            ("content_block_stop", r#"{"type":"content_block_stop","index":2}"#),
            ("ping", r#"{"type":"ping"}"#),
            ("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}"#),
            ("message_stop", r#"{"type":"message_stop"}"#),
        ] {
            acc.feed(event, data, Some(&sink)).unwrap();
        }
        assert!(acc.done);
        let out = acc.finish().unwrap();
        let msg = &out["choices"][0]["message"];
        assert_eq!(msg["content"], "好的");
        assert_eq!(msg["reasoning_content"], "plan");
        assert_eq!(msg["tool_calls"][0]["function"]["arguments"], "{\"name\":\"a\"}");
        let native = msg["anthropic_content"].as_array().unwrap();
        assert_eq!(native[0]["signature"], "sig-two");
        assert_eq!(native[2]["input"]["name"], "a");
        assert_eq!(out["usage"]["prompt_tokens"], 42, "12 输入 + 30 缓存读");
        assert_eq!(out["usage"]["completion_tokens"], 42);
        let seen = seen.lock().unwrap();
        assert_eq!(
            *seen,
            vec![
                "think:plan".to_string(),
                "text:好的".to_string(),
                "args:0:entity_create:{\"name\":".to_string(),
                "args:0:entity_create:\"a\"}".to_string(),
            ]
        );
    }

    #[test]
    fn stream_truncation_and_empty_and_error_events_fail_honestly() {
        // 没有 message_stop/stop_reason 就断流 → 瞬时错误(交给步进重试)。
        let mut acc = StreamAcc::default();
        acc.feed("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"半"}}"#, None)
            .unwrap();
        let err = acc.finish().unwrap_err().to_string();
        assert!(err.contains("流中断"), "{err}");

        // 只有思考、撞上最大输出 → EMPTY_COMPLETION 并提示调大最大输出。
        let mut acc = StreamAcc::default();
        acc.feed("content_block_start", r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"..."}}"#, None)
            .unwrap();
        acc.feed("message_delta", r#"{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}"#, None)
            .unwrap();
        let err = acc.finish().unwrap_err().to_string();
        assert!(err.starts_with("EMPTY_COMPLETION") && err.contains("最大输出"), "{err}");

        let mut acc = StreamAcc::default();
        let err = acc
            .feed("error", r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#, None)
            .unwrap_err();
        assert!(err.to_string().contains("Overloaded"));
    }

    #[test]
    fn official_host_detection() {
        assert!(is_official_host("https://api.anthropic.com/v1/messages"));
        assert!(is_official_host("https://API.anthropic.com:443/v1/messages"));
        assert!(!is_official_host("https://open.bigmodel.cn/api/anthropic/v1/messages"));
        assert!(!is_official_host("https://api.anthropic.com.evil.example/v1/messages"));
    }

    /// 端到端:本地假 Messages 服务(SSE)→ call() → chat.completion;核对请求头与请求体。
    #[test]
    fn call_streams_against_local_server_with_expected_headers() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            // 读到请求体完整(Content-Length)。
            loop {
                let n = stream.read(&mut chunk).unwrap();
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(pos) = text.find("\r\n\r\n") {
                    let len = text[..pos]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if buf.len() >= pos + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let sse = [
                r#"{"type":"message_start","message":{"id":"m","usage":{"input_tokens":3}}}"#,
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"pong"}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}"#,
                r#"{"type":"message_stop"}"#,
            ]
            .iter()
            .map(|d| {
                let kind: Value = serde_json::from_str(d).unwrap();
                format!("event: {}\ndata: {d}\n\n", kind["type"].as_str().unwrap())
            })
            .collect::<String>();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{sse}",
                sse.len()
            );
            stream.write_all(resp.as_bytes()).unwrap();
            String::from_utf8_lossy(&buf).to_string()
        });
        let (sink, seen) = collect();
        let url = format!("http://127.0.0.1:{port}/v1/messages");
        let body = build_body("glm-4.6", &[json!({ "role": "user", "content": "ping" })], &[], &spec(None), 1024);
        let out = call(&url, "sk-test-anthropic-local", body, Some(&sink)).unwrap();
        assert_eq!(out["choices"][0]["message"]["content"], "pong");
        assert_eq!(*seen.lock().unwrap(), vec!["text:pong".to_string()]);
        let request = server.join().unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(lower.starts_with("post /v1/messages"));
        assert!(lower.contains("x-api-key: sk-test-anthropic-local"));
        assert!(lower.contains("anthropic-version: 2023-06-01"));
        assert!(lower.contains("authorization: bearer sk-test-anthropic-local"), "非官方域名带 Bearer");
        assert!(request.contains("\"stream\":true"));
        assert!(request.contains("\"max_tokens\":1024"));
    }
}
