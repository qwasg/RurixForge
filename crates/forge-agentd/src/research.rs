//! Headless, credential-free adapter around the existing Forge tool loop.
//! The trusted owner handles HTTPS and allowlisted tools over bounded JSONL.
//! No editor state, MCP/native executor, shell, or direct networking is started.
use crate::llm::{self, ExecFn, LlmError, StepFn, StepOutcome, ToolLoopCfg, ToolLoopPolicy, Usage};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex,
};

const MAX_FRAME_BYTES: usize = 40 * 1024 * 1024;
const RAW_OUTPUT: &str = "_research_response_output";

#[derive(Deserialize)]
struct Init {
    kind: String,
    prompt: String,
    system_prompt: String,
    tools: Vec<Value>,
    model: String,
    context_window: u64,
    context_reserve: u64,
    max_output_tokens: u64,
    max_iters: usize,
    step_timeout_seconds: u64,
    tool_feedback_max_chars: usize,
}

impl Init {
    fn validate(&self) -> Result<(), LlmError> {
        if self.kind != "init"
            || !matches!(self.model.as_str(), "gpt-6-astra" | "gpt-6-sol")
            || self.context_window != 1_000_000
            || self.context_reserve < self.max_output_tokens
            || self.context_reserve >= self.context_window
            || self.max_output_tokens == 0
            || self.max_output_tokens > 16_384
            || self.max_iters == 0
            || self.max_iters > 2_000
            || self.step_timeout_seconds < 930
            || self.step_timeout_seconds > 3_600
            || self.tool_feedback_max_chars == 0
            || self.tool_feedback_max_chars > 120_000
            || self.prompt.is_empty()
            || self.system_prompt.is_empty()
        {
            return Err(LlmError::new("INIT_POLICY"));
        }
        native_tools(&self.tools)?;
        Ok(())
    }
}

/// Read bounded frames without allowing read_line to allocate without limit.
fn read_frame(reader: &mut impl BufRead) -> Result<Value, &'static str> {
    let mut raw = Vec::new();
    loop {
        let buf = reader.fill_buf().map_err(|_| "BRIDGE_READ")?;
        if buf.is_empty() {
            return Err("BRIDGE_EOF");
        }
        let take = buf
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(buf.len());
        if raw.len().saturating_add(take) > MAX_FRAME_BYTES {
            return Err("BRIDGE_FRAME_LIMIT");
        }
        let done = buf[take - 1] == b'\n';
        raw.extend_from_slice(&buf[..take]);
        reader.consume(take);
        if done {
            return serde_json::from_slice(&raw).map_err(|_| "BRIDGE_JSON");
        }
    }
}

struct Bridge {
    input: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Result<Value, &'static str>>>,
    output: Mutex<std::io::Stdout>,
    next_id: AtomicU64,
}

impl Bridge {
    fn start() -> Arc<Self> {
        let (send, receive) = tokio::sync::mpsc::channel(2);
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            loop {
                let value = read_frame(&mut input);
                let failed = value.is_err();
                if send.blocking_send(value).is_err() || failed {
                    break;
                }
            }
        });
        Arc::new(Self {
            input: tokio::sync::Mutex::new(receive),
            output: Mutex::new(std::io::stdout()),
            next_id: AtomicU64::new(1),
        })
    }

    fn emit(&self, value: Value) -> Result<(), LlmError> {
        let mut raw = serde_json::to_vec(&value).map_err(|_| LlmError::new("BRIDGE_SERIALIZE"))?;
        raw.push(b'\n');
        if raw.len() > MAX_FRAME_BYTES {
            return Err(LlmError::new("BRIDGE_FRAME_LIMIT"));
        }
        let mut output = self
            .output
            .lock()
            .map_err(|_| LlmError::new("BRIDGE_LOCK"))?;
        output
            .write_all(&raw)
            .and_then(|_| output.flush())
            .map_err(|_| LlmError::new("BRIDGE_WRITE"))
    }

    async fn initial(&self) -> Result<Init, LlmError> {
        let value = self
            .input
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| LlmError::new("BRIDGE_EOF"))?
            .map_err(LlmError::new)?;
        let init: Init = serde_json::from_value(value).map_err(|_| LlmError::new("INIT_SCHEMA"))?;
        init.validate()?;
        Ok(init)
    }

    async fn request(&self, mut request: Value) -> Result<Value, LlmError> {
        // One transaction per core, no global lock across the ten processes.
        let mut input = self.input.lock().await;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        request["id"] = json!(id);
        self.emit(request)?;
        let reply = input
            .recv()
            .await
            .ok_or_else(|| LlmError::new("BRIDGE_EOF"))?
            .map_err(LlmError::new)?;
        if reply.get("id").and_then(Value::as_u64) != Some(id) {
            return Err(LlmError::new("BRIDGE_REPLY_ID"));
        }
        match reply.get("ok").and_then(Value::as_bool) {
            Some(true) => reply
                .get("result")
                .cloned()
                .ok_or_else(|| LlmError::new("BRIDGE_REPLY_RESULT")),
            Some(false) => {
                let code = reply.get("error").and_then(Value::as_str).unwrap_or("");
                let safe = !code.is_empty()
                    && code.len() <= 100
                    && code
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
                Err(LlmError::new(if safe {
                    code
                } else {
                    "BRIDGE_OWNER_ERROR"
                }))
            }
            None => Err(LlmError::new("BRIDGE_REPLY_STATUS")),
        }
    }
}

fn native_tools(tools: &[Value]) -> Result<Vec<Value>, LlmError> {
    let mut names = HashSet::new();
    tools.iter().map(|tool| {
        let function = tool.get("function").filter(|_| tool.get("type").and_then(Value::as_str) == Some("function"))
            .ok_or_else(|| LlmError::new("TOOL_SCHEMA"))?;
        let name = function.get("name").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| LlmError::new("TOOL_SCHEMA"))?;
        if !names.insert(name) { return Err(LlmError::new("TOOL_DUPLICATE")); }
        let parameters = function.get("parameters").filter(|p| p.is_object()).ok_or_else(|| LlmError::new("TOOL_SCHEMA"))?;
        Ok(json!({"type":"function", "name":name, "description":function.get("description").and_then(Value::as_str).unwrap_or(""), "parameters":parameters, "strict":false}))
    }).collect()
}

fn response_input(messages: &[Value]) -> Result<Vec<Value>, LlmError> {
    let mut input = Vec::new();
    for message in messages {
        match message.get("role").and_then(Value::as_str) {
            Some("system") | Some("user") => {
                input.push(json!({"role":message["role"], "content":message["content"]}))
            }
            Some("assistant") => {
                let output = message
                    .get(RAW_OUTPUT)
                    .and_then(Value::as_array)
                    .ok_or_else(|| LlmError::new("HISTORY_OUTPUT_MISSING"))?;
                input.extend(output.iter().cloned());
            }
            Some("tool") => {
                let output = message
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| LlmError::new("HISTORY_TOOL_OUTPUT"))?;
                // The owner verifies equality with the tool result before HTTPS.
                serde_json::from_str::<Value>(output)
                    .map_err(|_| LlmError::new("HISTORY_TOOL_JSON"))?;
                input.push(json!({"type":"function_call_output", "call_id":message["tool_call_id"], "output":output}));
            }
            _ => return Err(LlmError::new("HISTORY_ROLE")),
        }
    }
    Ok(input)
}

#[derive(Default)]
struct ContextMeter {
    /// Last provider's input + output count, followed by the number of exact
    /// replay items it covers. New UTF-8 bytes are a conservative token bound.
    checkpoint: Option<(u64, usize)>,
}

impl ContextMeter {
    fn check(&self, input: &[Value], tools: &[Value], init: &Init) -> Result<(), LlmError> {
        let estimate = if let Some((tokens, covered)) = self.checkpoint {
            if covered > input.len() {
                return Err(LlmError::new("HISTORY_CHANGED"));
            }
            tokens
                .saturating_add(serde_json::to_vec(&input[covered..]).unwrap().len() as u64)
                .saturating_add((input.len() - covered) as u64 * 16)
        } else {
            serde_json::to_vec(input).unwrap().len() as u64
                + serde_json::to_vec(tools).unwrap().len() as u64
                + 1024
        };
        if estimate.saturating_add(init.context_reserve) >= init.context_window {
            // Never truncate pending outputs or fabricate a rotation/summary.
            return Err(LlmError::new("CONTEXT_LIMIT"));
        }
        Ok(())
    }
}

fn parse_response(response: &Value, tools: &[Value]) -> Result<StepOutcome, LlmError> {
    if response.get("status").and_then(Value::as_str) != Some("completed") {
        return Err(LlmError::new("RESPONSE_NOT_COMPLETED"));
    }
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| LlmError::new("RESPONSE_OUTPUT"))?;
    let mut calls = Vec::new();
    let mut text = Vec::new();
    let mut ids = HashSet::new();
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                let id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| LlmError::new("RESPONSE_CALL_ID"))?;
                if !ids.insert(id) {
                    return Err(LlmError::new("RESPONSE_CALL_DUPLICATE"));
                }
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| LlmError::new("RESPONSE_CALL_NAME"))?;
                if !tools
                    .iter()
                    .any(|t| t.get("name").and_then(Value::as_str) == Some(name))
                {
                    return Err(LlmError::new("RESPONSE_TOOL_NOT_ALLOWED"));
                }
                let args = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .ok_or_else(|| LlmError::new("RESPONSE_CALL_ARGUMENTS"))?;
                if !serde_json::from_str::<Value>(args)
                    .map_err(|_| LlmError::new("RESPONSE_CALL_ARGUMENTS"))?
                    .is_object()
                {
                    return Err(LlmError::new("RESPONSE_CALL_ARGUMENTS"));
                }
                calls.push(
                    json!({"id":id,"type":"function","function":{"name":name,"arguments":args}}),
                );
            }
            Some("message") => {
                for part in item
                    .get("content")
                    .and_then(Value::as_array)
                    .ok_or_else(|| LlmError::new("RESPONSE_MESSAGE"))?
                {
                    if part.get("type").and_then(Value::as_str) == Some("output_text") {
                        text.push(
                            part.get("text")
                                .and_then(Value::as_str)
                                .ok_or_else(|| LlmError::new("RESPONSE_TEXT"))?,
                        );
                    }
                }
            }
            Some("reasoning") => {}
            _ => return Err(LlmError::new("RESPONSE_OUTPUT_TYPE")),
        }
    }
    let usage = response
        .get("usage")
        .ok_or_else(|| LlmError::new("RESPONSE_USAGE"))?;
    let count = |name| {
        usage
            .get(name)
            .and_then(Value::as_u64)
            .ok_or_else(|| LlmError::new("RESPONSE_USAGE"))
    };
    let usage = Usage {
        prompt_tokens: count("input_tokens")?,
        completion_tokens: count("output_tokens")?,
        total_tokens: count("total_tokens")?,
    };
    if usage.prompt_tokens.saturating_add(usage.completion_tokens) != usage.total_tokens {
        return Err(LlmError::new("RESPONSE_USAGE_MISMATCH"));
    }
    let mut message = json!({"role":"assistant", "content":text.join("\n"), RAW_OUTPUT:output});
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    Ok(StepOutcome {
        message,
        usage: Some(usage),
    })
}

async fn run(bridge: Arc<Bridge>, init: Init) -> Result<Value, LlmError> {
    let init = Arc::new(init);
    let native = Arc::new(native_tools(&init.tools)?);
    let context = Arc::new(Mutex::new(ContextMeter::default()));
    let iterations = Arc::new(AtomicUsize::new(0));
    let pending = Arc::new(AtomicBool::new(false));
    let fatal = Arc::new(Mutex::new(None::<String>));
    let step: Box<StepFn> = {
        let (bridge, init, native, context, iterations, pending) = (
            bridge.clone(),
            init.clone(),
            native.clone(),
            context.clone(),
            iterations.clone(),
            pending.clone(),
        );
        Box::new(move |messages, _, _| {
            let (bridge, init, native, context, iterations, pending) = (
                bridge.clone(),
                init.clone(),
                native.clone(),
                context.clone(),
                iterations.clone(),
                pending.clone(),
            );
            Box::pin(async move {
                let input = response_input(&messages)?;
                context.lock().unwrap().check(&input, &native, &init)?;
                let input_len = input.len();
                let body = json!({"model":init.model, "store":false, "reasoning":{"effort":"high"},
                    "include":["reasoning.encrypted_content"], "max_output_tokens":init.max_output_tokens,
                    "tools":*native, "tool_choice":"auto", "parallel_tool_calls":false,
                    "truncation":"disabled", "input":input});
                let response = bridge.request(json!({"kind":"model", "body":body})).await?;
                let out = parse_response(&response, &native)?;
                let usage = out.usage.unwrap();
                if usage.total_tokens > init.context_window {
                    return Err(LlmError::new("CONTEXT_WINDOW_EXCEEDED"));
                }
                context.lock().unwrap().checkpoint = Some((
                    usage.total_tokens,
                    input_len + response["output"].as_array().unwrap().len(),
                ));
                pending.store(out.message.get("tool_calls").is_some(), Ordering::Relaxed);
                iterations.fetch_add(1, Ordering::Relaxed);
                bridge.emit(json!({"kind":"event", "event":"usage", "data":{
                    "input_tokens":usage.prompt_tokens, "output_tokens":usage.completion_tokens, "total_tokens":usage.total_tokens,
                    "cached_input_tokens":response.pointer("/usage/input_tokens_details/cached_tokens").and_then(Value::as_u64).unwrap_or(0),
                    "reasoning_output_tokens":response.pointer("/usage/output_tokens_details/reasoning_tokens").and_then(Value::as_u64).unwrap_or(0)
                }}))?;
                Ok(out)
            })
        })
    };
    let execute: Box<ExecFn> = {
        let (bridge, fatal) = (bridge.clone(), fatal.clone());
        Box::new(move |name, arguments| {
            let (bridge, fatal) = (bridge.clone(), fatal.clone());
            Box::pin(async move {
                if fatal.lock().unwrap().is_some() {
                    return (false, json!({"error":"BRIDGE_ABORTED"}).to_string().into());
                }
                match bridge
                    .request(json!({"kind":"tool", "name":name, "arguments":arguments}))
                    .await
                {
                    Ok(value) => (true, value.to_string().into()),
                    Err(error) => {
                        *fatal.lock().unwrap() = Some(error.to_string());
                        (
                            false,
                            json!({"error":"BRIDGE_TOOL_FAILURE"}).to_string().into(),
                        )
                    }
                }
            })
        })
    };
    let cancelled = || fatal.lock().unwrap().is_some();
    let result = llm::run_tool_loop_with_policy(
        &init.system_prompt,
        &init.prompt,
        ToolLoopCfg {
            tools: init.tools.clone(),
            step: step.as_ref(),
            execute: execute.as_ref(),
            vision: false,
            sink: None,
            forbidden: None,
            cancelled: Some(&cancelled),
            stream: None,
            preamble: None,
            max_iters: Some(init.max_iters),
            inbox: None,
            history: Vec::new(),
        },
        ToolLoopPolicy {
            step_attempts: 1,
            step_timeout_seconds: init.step_timeout_seconds,
            tool_feedback_max_chars: init.tool_feedback_max_chars,
            reject_oversize_feedback: true,
            parallel_task_calls: false,
        },
    )
    .await;
    if let Some(error) = fatal.lock().unwrap().as_ref() {
        return Ok(
            json!({"kind":"terminal", "status":"error", "error":error, "text":"", "iters":iterations.load(Ordering::Relaxed)}),
        );
    }
    match result {
        Ok(out) => Ok(
            json!({"kind":"terminal", "status":if out.cancelled {"cancelled"} else if pending.load(Ordering::Relaxed) {"iteration_limit"} else {"agent_finished"}, "text":out.text, "iters":out.iters}),
        ),
        Err(error) => Ok(
            json!({"kind":"terminal", "status":"error", "error":error.to_string(), "text":"", "iters":iterations.load(Ordering::Relaxed)}),
        ),
    }
}

pub(crate) async fn run_stdio() {
    let bridge = Bridge::start();
    let result = match bridge.initial().await {
        Ok(init) => run(bridge.clone(), init).await,
        Err(error) => Err(error),
    };
    let terminal = result.unwrap_or_else(|error| json!({"kind":"terminal", "status":"error", "text":"", "iters":0, "error":error.to_string()}));
    let _ = bridge.emit(terminal);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Vec<Value> {
        vec![
            json!({"type":"function","function":{"name":"read_source","parameters":{"type":"object"}}}),
        ]
    }
    fn init() -> Init {
        Init {
            kind: "init".into(),
            prompt: "task".into(),
            system_prompt: "system".into(),
            tools: tools(),
            model: "gpt-6-astra".into(),
            context_window: 1_000_000,
            context_reserve: 32768,
            max_output_tokens: 16384,
            max_iters: 2000,
            step_timeout_seconds: 930,
            tool_feedback_max_chars: 120000,
        }
    }
    fn response() -> Value {
        json!({"status":"completed", "output":[
        {"type":"reasoning","id":"rs_1","summary":[],"encrypted_content":"opaque"},
        {"type":"function_call","id":"fc_1","call_id":"call_1","name":"read_source","arguments":"{}"}],
        "usage":{"input_tokens":100,"output_tokens":20,"total_tokens":120}})
    }

    #[test]
    fn opaque_output_and_json_feedback_roundtrip_without_loss() {
        let raw = response();
        let out = parse_response(&raw, &native_tools(&tools()).unwrap()).unwrap();
        assert!(out.message.get("reasoning_content").is_none());
        let feedback = json!({"text":"多字节 JSON \" preserved ".repeat(5000)}).to_string();
        let messages = vec![
            json!({"role":"system","content":"system"}),
            json!({"role":"user","content":"task"}),
            out.message,
            json!({"role":"tool","tool_call_id":"call_1","content":feedback}),
        ];
        let input = response_input(&messages).unwrap();
        assert_eq!(&input[2..4], raw["output"].as_array().unwrap());
        assert_eq!(input[4]["output"], feedback);
        assert_eq!(input[4]["call_id"], "call_1");
    }

    #[test]
    fn invalid_or_unadvertised_calls_fail_before_execution() {
        let native = native_tools(&tools()).unwrap();
        let mut raw = response();
        raw["output"][1]["name"] = json!("shell");
        assert_eq!(
            parse_response(&raw, &native).err().unwrap().to_string(),
            "RESPONSE_TOOL_NOT_ALLOWED"
        );
        raw["output"][1]["name"] = json!("read_source");
        raw["output"][1]["arguments"] = json!("broken JSON");
        assert_eq!(
            parse_response(&raw, &native).err().unwrap().to_string(),
            "RESPONSE_CALL_ARGUMENTS"
        );
        raw["status"] = json!("incomplete");
        assert_eq!(
            parse_response(&raw, &native).err().unwrap().to_string(),
            "RESPONSE_NOT_COMPLETED"
        );
    }

    #[test]
    fn sol_is_explicitly_supported_without_model_fallback() {
        let mut value = init();
        assert!(value.validate().is_ok());
        value.model = "gpt-6-sol".into();
        assert!(value.validate().is_ok());
        value.model = "gpt-6sol".into();
        assert_eq!(value.validate().err().unwrap().to_string(), "INIT_POLICY");
        value.model = "unrequested-model".into();
        assert_eq!(value.validate().err().unwrap().to_string(), "INIT_POLICY");
    }

    #[test]
    fn unsupported_output_does_not_become_silent_completion() {
        let mut raw = response();
        raw["output"] = json!([{"type":"unrecognized_provider_item","payload":"opaque"}]);
        assert_eq!(
            parse_response(&raw, &native_tools(&tools()).unwrap())
                .err()
                .unwrap()
                .to_string(),
            "RESPONSE_OUTPUT_TYPE"
        );
    }

    #[test]
    fn context_limit_is_explicit_and_preserves_pending_items() {
        let input = vec![
            json!({"role":"user","content":"original task"}),
            json!({"type":"function_call_output","call_id":"call_1","output":"{}"}),
        ];
        let before = input.clone();
        let meter = ContextMeter {
            checkpoint: Some((967_200, 1)),
        };
        assert_eq!(
            meter.check(&input, &[], &init()).err().unwrap().to_string(),
            "CONTEXT_LIMIT"
        );
        assert_eq!(input, before);
        ContextMeter {
            checkpoint: Some((500_000, 1)),
        }
        .check(&input, &[], &init())
        .unwrap();
    }

    #[test]
    fn bounded_frames_and_configuration() {
        let mut reader = std::io::Cursor::new(b"{\"kind\":\"init\"}\n");
        assert_eq!(read_frame(&mut reader).unwrap()["kind"], "init");
        assert_eq!(read_frame(&mut reader).unwrap_err(), "BRIDGE_EOF");
        init().validate().unwrap();
        let mut bad = init();
        bad.context_window = 128000;
        assert_eq!(bad.validate().unwrap_err().to_string(), "INIT_POLICY");
    }

    #[test]
    fn python_float_bits_survive_tool_feedback_roundtrip() {
        // Generated by CPython random.Random(24926) + json.dumps. The default
        // serde_json parser changed each of these values by one ULP.
        let cases = [
            ("9.692212628088841", 4621645848702290465u64),
            ("431.78457264156356", 4646303650724629921),
            ("8.393751348036327e-10", 4471185168906614508),
            ("1406014.9357428697", 4698789599886759999),
            ("0.0009172425649051659", 4561599292695730715),
        ];
        for (python_json, bits) in cases {
            let tool_result: Value = serde_json::from_str(&format!(
                "{{\"budget\":{{\"wall_elapsed_seconds\":{python_json}}}}}"
            ))
            .unwrap();
            assert_eq!(
                tool_result["budget"]["wall_elapsed_seconds"]
                    .as_f64()
                    .unwrap()
                    .to_bits(),
                bits
            );
            let messages = [
                json!({"role":"tool", "tool_call_id":"call_float", "content":tool_result.to_string()}),
            ];
            let input = response_input(&messages).unwrap();
            let body = json!({"input":input});
            let wire: Value = serde_json::from_str(&body.to_string()).unwrap();
            let delivered: Value =
                serde_json::from_str(wire["input"][0]["output"].as_str().unwrap()).unwrap();
            assert_eq!(
                delivered["budget"]["wall_elapsed_seconds"]
                    .as_f64()
                    .unwrap()
                    .to_bits(),
                bits
            );
        }
    }

    #[tokio::test]
    async fn research_policy_uses_real_core_preserves_large_feedback_and_never_retries() {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let step: Box<StepFn> = Box::new(move |messages, _, _| {
            let n = seen.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move {
                if n == 0 {
                    return Ok(StepOutcome {
                        message: json!({"role":"assistant","tool_calls":[{"id":"one","function":{"name":"read_source","arguments":"{}"}}]}),
                        usage: None,
                    });
                }
                let feedback = messages.last().unwrap()["content"].as_str().unwrap();
                assert!(feedback.len() > 4000);
                assert_eq!(
                    serde_json::from_str::<Value>(feedback).unwrap()["content"]
                        .as_str()
                        .unwrap()
                        .len(),
                    20000
                );
                Err(LlmError::new("NETWORK_TEST_FAILURE"))
            })
        });
        let executor: Box<ExecFn> = Box::new(|_, _| {
            Box::pin(async {
                (
                    true,
                    json!({"content":"x".repeat(20000)}).to_string().into(),
                )
            })
        });
        let result = llm::run_tool_loop_with_policy(
            "system",
            "task",
            ToolLoopCfg {
                tools: tools(),
                step: step.as_ref(),
                execute: executor.as_ref(),
                vision: false,
                sink: None,
                forbidden: None,
                cancelled: None,
                stream: None,
                preamble: None,
                max_iters: Some(3),
                inbox: None,
                history: Vec::new(),
            },
            ToolLoopPolicy {
                step_attempts: 1,
                step_timeout_seconds: 930,
                tool_feedback_max_chars: 120000,
                reject_oversize_feedback: true,
                parallel_task_calls: false,
            },
        )
        .await;
        assert_eq!(result.err().unwrap().to_string(), "NETWORK_TEST_FAILURE");
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }
}
