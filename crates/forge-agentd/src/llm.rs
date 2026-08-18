//! RD-F1-002 真 LLM 工具循环:provider 抽象(mock|deepseek)+ POST /api/forge/llm/chat。
//! 循环:tools/list 实测拉取五 server schema → OpenAI tools 格式 → DeepSeek chat.completions
//! (tools, tool_choice=auto)→ tool_calls 逐项进程内 mcp::call_tool → role:tool 回注 →
//! 终止(无 tool_calls 或 max_iters=16)→ { provider, text, toolCalls[{name,ok,summary}], iters }。
//! 密钥红线(R-5):FORGE_LLM_API_KEY env 优先 → gend keystore["deepseek"](key_for 语义,
//! FORGE_GEN_API_KEY 共享 dev-key 覆盖如实标注);皆无 → provider=mock。密钥只进
//! Authorization 请求头,永不进日志/事件/工具返回/错误消息。

use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{json, Value};

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

const SYSTEM_PROMPT: &str = "你是 RurixForge 游戏引擎编辑器的内置助手。\
用户用中文描述场景编辑/资产管理/代码工具意图,你应优先调用提供的工具完成实际操作,而不是只描述步骤。\
工具调用参数严格遵循各工具的 inputSchema;实体创建等操作完成后可用一句话如实汇报结果(成功/失败/数量),不得伪造执行结果。";

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
enum Provider {
    /// 无密钥:恒绿 seam,不触网不触 MCP。
    Mock,
    /// DeepSeek 官方 API;String 仅用于 Authorization 头组装。
    Deepseek(String),
}

/// Debug 脱敏(R-5):Deepseek 变体永不打印密钥本体。
impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provider::Mock => f.write_str("Mock"),
            Provider::Deepseek(_) => f.write_str("Deepseek(<redacted>)"),
        }
    }
}

/// 密钥面:FORGE_LLM_API_KEY env 优先 → gend keystore["deepseek"]。
/// keystore.key_for 语义含 FORGE_GEN_API_KEY 共享 dev-key 覆盖(D-RDG-B 如实标注)。
fn resolve_provider() -> Provider {
    if let Ok(v) = std::env::var("FORGE_LLM_API_KEY") {
        if !v.is_empty() {
            return Provider::Deepseek(v);
        }
    }
    let ks = gend::keystore::Keystore::load();
    match ks.key_for("deepseek") {
        Some(k) if !k.is_empty() => Provider::Deepseek(k),
        _ => Provider::Mock,
    }
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

/// chat.completions 请求体组装(纯函数,便于「密钥不进 body」扫描测试)。
fn build_request_body(messages: &[Value], tools: &[Value]) -> Value {
    json!({
        "model": DEEPSEEK_MODEL,
        "messages": messages,
        "tools": tools,
        "tool_choice": "auto",
        "stream": false,
    })
}

/// LLM 侧失败(上行 HTTP / 协议 / 工具面不可用);消息保证不含密钥。
#[derive(Debug)]
pub struct LlmError(String);

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 单次 DeepSeek chat.completions 调用(阻塞,调用方须 spawn_blocking)。
/// 错误消息只带 HTTP 状态码/传输错误,不回显请求体与头(R-5)。
fn chat_completions(key: &str, messages: &[Value], tools: &[Value]) -> Result<Value, LlmError> {
    let body = build_request_body(messages, tools);
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build();
    let resp = agent
        .post(DEEPSEEK_URL)
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
            return Err(LlmError(format!("DeepSeek HTTP {code}: {detail}")));
        }
        Err(ureq::Error::Transport(t)) => {
            return Err(LlmError(format!("DeepSeek 连接失败: {t}")));
        }
    };
    let bytes = read_body(resp).map_err(|e| LlmError(format!("读 DeepSeek 响应体失败: {e}")))?;
    serde_json::from_slice(&bytes).map_err(|e| LlmError(format!("DeepSeek 响应非 JSON: {e}")))
}

fn read_body(resp: ureq::Response) -> Result<Vec<u8>, std::io::Error> {
    use std::io::Read;
    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf)?;
    Ok(buf)
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

/// DeepSeek 工具循环主流程:返回 (最终文本, 工具调用记录, 实际轮数)。
async fn run_deepseek_loop(
    text: &str,
    key: &str,
) -> Result<(String, Vec<ToolCallRecord>, usize), LlmError> {
    let mcp_tools = mcp::list_all_tools()
        .await
        .map_err(|e| LlmError(format!("MCP 工具面拉取失败: {e}")))?;
    let tools = to_openai_tools(&mcp_tools);
    let mut messages = vec![
        json!({ "role": "system", "content": SYSTEM_PROMPT }),
        json!({ "role": "user", "content": text }),
    ];
    let mut records: Vec<ToolCallRecord> = Vec::new();

    for iter in 1..=MAX_ITERS {
        let key_owned = key.to_string();
        let msgs = messages.clone();
        let tls = tools.clone();
        let resp = tokio::task::spawn_blocking(move || chat_completions(&key_owned, &msgs, &tls))
            .await
            .map_err(|e| LlmError(format!("spawn_blocking join 失败: {e}")))??;
        let msg = resp
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|c| c.get("message"))
            .cloned()
            .ok_or_else(|| LlmError("DeepSeek 响应缺 choices[0].message".to_string()))?;

        let Some(calls) = tool_calls_of(&msg).cloned() else {
            // 无工具调用:终止,文本即最终答复。
            let content = msg
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            return Ok((content, records, iter));
        };

        // assistant 消息(含 tool_calls)原样回注,再逐项执行工具并回注 role:tool。
        messages.push(msg);
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
            let (ok, feedback) = match mcp::call_tool(&name, Some(args)).await {
                Ok(result) => {
                    let is_err = result
                        .get("isError")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    (!is_err, envelope_text(&result))
                }
                Err(e) => (false, format!("工具调用失败: {e}")),
            };
            records.push(ToolCallRecord {
                name: name.clone(),
                ok,
                summary: truncate_chars(&feedback, 200),
            });
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": truncate_chars(&feedback, TOOL_FEEDBACK_MAX),
            }));
        }
    }
    // 轮数耗尽:如实标注,不伪造收尾。
    Ok((
        format!("(工具循环已达上限 {MAX_ITERS} 轮,未收束;以上为已执行部分)"),
        records,
        MAX_ITERS,
    ))
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
            "text": format!("mock:已收到「{}」(无 LLM 密钥,真实工具循环未启用;配 FORGE_LLM_API_KEY 或 keystore[deepseek] 后走 deepseek)", truncate_chars(&req.text, 80)),
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
    }
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
        let body = build_request_body(&messages, &tools);
        let body_text = body.to_string();
        assert!(!body_text.contains(key), "请求体含密钥子串: {body_text}");
        assert_eq!(body["model"], DEEPSEEK_MODEL);
        assert_eq!(body["tool_choice"], "auto");
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
}
