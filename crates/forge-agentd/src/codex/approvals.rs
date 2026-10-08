//! Codex 服务端请求 → Forge 审批面。
//!
//! Codex 在 `approvalPolicy:"on-request"` 下会**反向**发请求问客户端:这条命令能不能跑、
//! 这几个文件能不能改、这个工具要用户填点东西。这些请求不回话,那一轮就永久挂住。
//!
//! 本仓已有审批面([permission.rs](crates/forge-agentd/src/permission.rs) 的
//! `permission.requested` / `POST /permissions/{id}/approve|deny`),所以这里不另造一套 UI:
//! 把 Codex 的请求翻成同一个事件,前端复用同一张审批卡,回话再翻回 Codex 的决定枚举。

use serde_json::{json, Value};

/// 回话形态。
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// `{ decision: "accept" | "acceptForSession" | "decline" }`
    Decision,
    /// 内建 `request_permissions` 要回授获准的请求子集，而不是通用 decision 枚举。
    Permissions { requested: Value },
    /// `{ answers: { questionId: { answers: string[] } } }`(要用户填表)
    Answers,
    /// MCP elicitation:`{ action: "accept" | "decline", content }`
    Elicitation,
}

/// 一条待批请求的 Forge 侧形态。
#[derive(Debug, Clone, PartialEq)]
pub struct Approval {
    /// `command` | `fileChange` | `permissions` | `userInput` | `elicitation`
    pub kind: &'static str,
    /// 进 `permission.requested` 的附加字段(前端审批卡直接读)。
    pub payload: Value,
    pub reply: Reply,
}

/// 服务端请求 → 待批形态。`None` = 不是审批类请求(如 `item/tool/call`,由 tools 处理)。
pub fn classify(method: &str, params: &Value) -> Option<Approval> {
    let item_id = params
        .get("itemId")
        .or_else(|| params.get("item_id"))
        .cloned()
        .unwrap_or(Value::Null);
    let reason = params
        .get("reason")
        .or_else(|| params.get("explanation"))
        .cloned()
        .unwrap_or(Value::Null);
    match method {
        "item/commandExecution/requestApproval" => Some(Approval {
            kind: "command",
            payload: json!({
                "itemId": item_id,
                "tool": "shell",
                "command": command_line(params),
                "cwd": params.get("cwd").cloned().unwrap_or(Value::Null),
                "reason": reason,
                "approvalKind": params.get("kind").cloned().unwrap_or(Value::Null),
                "environmentId": params.get("environmentId").cloned().unwrap_or(Value::Null),
                "networkApprovalContext": params.get("networkApprovalContext").cloned().unwrap_or(Value::Null),
                "proposedExecpolicyAmendment": params.get("proposedExecpolicyAmendment").cloned().unwrap_or(Value::Null),
                "proposedNetworkPolicyAmendments": params.get("proposedNetworkPolicyAmendments").cloned().unwrap_or(Value::Null),
                "availableDecisions": available_decisions(params, &["accept", "acceptForSession", "decline"]),
            }),
            reply: Reply::Decision,
        }),
        "item/fileChange/requestApproval" => Some(Approval {
            kind: "fileChange",
            payload: json!({
                "itemId": item_id,
                "tool": "apply_patch",
                "changes": changes(params),
                "reason": reason,
                "grantRoot": params.get("grantRoot").cloned().unwrap_or(Value::Null),
                "availableDecisions": available_decisions(params, &["accept", "acceptForSession", "decline"]),
            }),
            reply: Reply::Decision,
        }),
        "item/permissions/requestApproval" => {
            let requested = requested_permissions(params);
            Some(Approval {
                kind: "permissions",
                payload: json!({
                    "itemId": item_id,
                    "tool": "permissions",
                    "permissions": requested,
                    "reason": reason,
                    "availableDecisions": available_decisions(params, &["accept", "acceptForSession", "decline"]),
                }),
                reply: Reply::Permissions { requested },
            })
        }
        "item/tool/requestUserInput" => Some(Approval {
            kind: "userInput",
            payload: json!({
                "itemId": item_id,
                "tool": params.get("tool").cloned().unwrap_or(Value::Null),
                "questions": params
                    .get("questions")
                    .or_else(|| params.get("inputs"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "reason": reason,
                "availableDecisions": available_decisions(params, &["accept", "decline"]),
            }),
            reply: Reply::Answers,
        }),
        "mcpServer/elicitation/request" | "elicitation/create" | "item/tool/elicitation" => {
            Some(Approval {
                kind: "elicitation",
                payload: json!({
                    "itemId": item_id,
                    "tool": params.get("tool").cloned().unwrap_or(Value::Null),
                    "serverName": params.get("serverName").cloned().unwrap_or(Value::Null),
                    "mode": params.get("mode").cloned().unwrap_or(Value::Null),
                    "message": params.get("message").cloned().unwrap_or(Value::Null),
                    "url": params.get("url").cloned().unwrap_or(Value::Null),
                    "elicitationId": params.get("elicitationId").cloned().unwrap_or(Value::Null),
                    "_meta": params.get("_meta").cloned().unwrap_or(Value::Null),
                    "schema": params
                        .get("requestedSchema")
                        .or_else(|| params.get("schema"))
                        .cloned()
                        .unwrap_or(Value::Null),
                    "availableDecisions": available_decisions(params, &["accept", "decline"]),
                }),
                reply: Reply::Elicitation,
            })
        }
        _ => None,
    }
}

/// 前端回话(`permission.resolved` 的决定原文)→ Codex 期望的响应体。
pub fn to_codex_result(reply: &Reply, decision: &Value) -> Value {
    let choice = decision
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or("accept");
    let allowed = !matches!(choice, "decline" | "deny" | "reject");
    match reply {
        Reply::Decision => {
            // 「本会话都允许」必须原样传给 Codex,否则下一条同类命令又来问一遍,
            // 用户点的那个「本会话允许」等于没生效。
            let d = match choice {
                "acceptForSession" => "acceptForSession",
                c if !allowed => "decline",
                _ => "accept",
            };
            json!({ "decision": d })
        }
        Reply::Permissions { requested } => {
            let permissions = if allowed {
                requested_permissions(&json!({ "permissions": requested }))
            } else {
                json!({})
            };
            json!({
                "permissions": permissions,
                "scope": if choice == "acceptForSession" && allowed { "session" } else { "turn" },
            })
        }
        Reply::Answers => {
            let answers = if allowed {
                normalize_answers(decision.get("answers"))
            } else {
                json!({})
            };
            json!({ "answers": answers })
        }
        Reply::Elicitation => {
            if !allowed {
                return json!({ "action": "decline", "content": Value::Null });
            }
            json!({
                "action": "accept",
                "content": decision.get("answers").cloned().unwrap_or(Value::Null),
            })
        }
    }
}

/// 连接断了/turn 被中止时给 Codex 的兜底回话:一律拒,让它干净收场而不是挂着。
pub fn abandon_result(reply: &Reply) -> Value {
    to_codex_result(reply, &json!({ "decision": "decline" }))
}

fn available_decisions(params: &Value, fallback: &[&str]) -> Value {
    params
        .get("availableDecisions")
        .filter(|value| value.is_array())
        .cloned()
        .unwrap_or_else(|| json!(fallback))
}

fn requested_permissions(params: &Value) -> Value {
    let Some(src) = params.get("permissions").and_then(Value::as_object) else {
        return json!({});
    };
    let mut out = serde_json::Map::new();
    for key in ["fileSystem", "network"] {
        if let Some(value) = src.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    Value::Object(out)
}

/// 兼容前端已有的数组答卷与 app-server 的对象答卷，线上统一输出正式 schema。
fn normalize_answers(raw: Option<&Value>) -> Value {
    let mut out = serde_json::Map::new();
    match raw {
        Some(Value::Object(obj)) => {
            for (id, answer) in obj {
                if let Some(values) = answer_values(answer) {
                    out.insert(id.clone(), json!({ "answers": values }));
                }
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                let Some(id) = item
                    .get("id")
                    .or_else(|| item.get("questionId"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                if let Some(values) = item
                    .get("answers")
                    .or_else(|| item.get("value"))
                    .or_else(|| item.get("answer"))
                    .and_then(answer_values)
                {
                    out.insert(id.to_string(), json!({ "answers": values }));
                }
            }
        }
        _ => {}
    }
    Value::Object(out)
}

fn answer_values(value: &Value) -> Option<Vec<String>> {
    if let Some(inner) = value.get("answers") {
        return answer_values(inner);
    }
    match value {
        Value::String(s) => Some(vec![s.clone()]),
        Value::Array(values) => Some(
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        ),
        _ => None,
    }
}

fn command_line(params: &Value) -> Value {
    for k in ["command", "commandLine", "cmd"] {
        match params.get(k) {
            Some(Value::Array(arr)) => {
                let parts: Vec<String> = arr
                    .iter()
                    .map(|p| p.as_str().unwrap_or_default().to_string())
                    .collect();
                return json!(parts.join(" "));
            }
            Some(v) if !v.is_null() => return v.clone(),
            _ => {}
        }
    }
    Value::Null
}

fn changes(params: &Value) -> Value {
    match params.get("changes") {
        Some(Value::Object(obj)) => Value::Array(
            obj.iter()
                .map(|(path, v)| {
                    json!({
                        "path": path,
                        "kind": v.get("kind").or_else(|| v.get("type")).cloned().unwrap_or(Value::Null),
                        "diff": v.get("diff").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect(),
        ),
        Some(v) => v.clone(),
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 命令审批:argv 数组拼成人看得懂的一行,决定档次全给出。
    #[test]
    fn command_approval_shape() {
        let a = classify(
            "item/commandExecution/requestApproval",
            &json!({
                "threadId": "th_1", "itemId": "c1",
                "command": ["cargo", "build", "--release"], "cwd": "D:/proj",
                "reason": "需要编译验证"
            }),
        )
        .expect("应识别为命令审批");
        assert_eq!(a.kind, "command");
        assert_eq!(a.reply, Reply::Decision);
        assert_eq!(a.payload["command"], "cargo build --release");
        assert_eq!(a.payload["cwd"], "D:/proj");
        assert_eq!(a.payload["tool"], "shell");
        assert_eq!(
            a.payload["availableDecisions"],
            json!(["accept", "acceptForSession", "decline"])
        );
    }

    /// 文件改动审批:对象形态的 changes 摊平成数组(审批卡只认一种形状)。
    #[test]
    fn file_change_approval_flattens_changes() {
        let a = classify(
            "item/fileChange/requestApproval",
            &json!({ "threadId": "th_1", "grantRoot": "D:/proj/generated", "changes": {
                "src/main.rs": { "kind": "modify", "diff": "@@ -1 +1 @@" }
            }}),
        )
        .unwrap();
        assert_eq!(a.kind, "fileChange");
        assert_eq!(a.payload["changes"][0]["path"], "src/main.rs");
        assert_eq!(a.payload["changes"][0]["kind"], "modify");
        assert_eq!(a.payload["grantRoot"], "D:/proj/generated");
    }

    /// 非审批类请求不该被这里截走(dynamicTool 调用归 tools 处理)。
    #[test]
    fn tool_call_is_not_an_approval() {
        assert!(classify("item/tool/call", &json!({ "name": "forge.project_list" })).is_none());
    }

    /// 「本会话都允许」必须原样传下去,否则同类命令会一直重复问。
    #[test]
    fn accept_for_session_is_preserved() {
        assert_eq!(
            to_codex_result(&Reply::Decision, &json!({ "decision": "acceptForSession" })),
            json!({ "decision": "acceptForSession" })
        );
        assert_eq!(
            to_codex_result(&Reply::Decision, &json!({ "decision": "accept" })),
            json!({ "decision": "accept" })
        );
        assert_eq!(
            to_codex_result(&Reply::Decision, &json!({ "decision": "decline" })),
            json!({ "decision": "decline" })
        );
    }

    /// 答卷:允许 → 带答案;拒绝 → 明确取消(不能回空答卷,那会被当成「填了个空的」)。
    #[test]
    fn user_input_answers_and_cancel() {
        let ok = to_codex_result(
            &Reply::Answers,
            &json!({ "decision": "accept", "answers": [{ "id": "q1", "value": "2d" }] }),
        );
        assert_eq!(ok["answers"]["q1"]["answers"], json!(["2d"]));
        let no = to_codex_result(&Reply::Answers, &json!({ "decision": "decline" }));
        assert_eq!(no, json!({ "answers": {} }));
    }

    /// 放弃(连接断/被中止)一律回拒绝,让 Codex 干净收场。
    #[test]
    fn abandon_declines_every_shape() {
        assert_eq!(abandon_result(&Reply::Decision)["decision"], "decline");
        assert_eq!(abandon_result(&Reply::Answers)["answers"], json!({}));
        assert_eq!(abandon_result(&Reply::Elicitation)["action"], "decline");
    }

    #[test]
    fn permissions_grant_echoes_only_requested_subset_and_scope() {
        let a = classify(
            "item/permissions/requestApproval",
            &json!({
                "permissions": {
                    "network": { "enabled": true },
                    "fileSystem": { "write": ["D:/proj"] },
                    "unexpected": { "secret": "must-not-echo" }
                }
            }),
        )
        .unwrap();
        let yes = to_codex_result(&a.reply, &json!({ "decision": "acceptForSession" }));
        assert_eq!(yes["scope"], "session");
        assert_eq!(yes["permissions"]["network"]["enabled"], true);
        assert!(yes["permissions"].get("unexpected").is_none());
        let no = to_codex_result(&a.reply, &json!({ "decision": "decline" }));
        assert_eq!(no, json!({ "permissions": {}, "scope": "turn" }));
    }

    #[test]
    fn official_mcp_elicitation_method_is_classified() {
        let a = classify(
            "mcpServer/elicitation/request",
            &json!({
                "serverName": "store", "mode": "url", "message": "登录后继续",
                "url": "https://example.test/login", "elicitationId": "el_1",
                "_meta": { "trace": "t1" }
            }),
        )
        .expect("正式方法名必须被处理，否则 turn 会永久挂住");
        assert_eq!(a.kind, "elicitation");
        assert_eq!(a.payload["serverName"], "store");
        assert_eq!(a.payload["mode"], "url");
        assert_eq!(a.payload["url"], "https://example.test/login");
        assert_eq!(a.payload["elicitationId"], "el_1");
        assert_eq!(a.payload["_meta"]["trace"], "t1");
        let no = to_codex_result(&a.reply, &json!({ "decision": "decline" }));
        assert_eq!(no, json!({ "action": "decline", "content": null }));
    }

    #[test]
    fn command_approval_preserves_upstream_risk_context_and_decisions() {
        let a = classify(
            "item/commandExecution/requestApproval",
            &json!({
                "itemId": "cmd_1",
                "availableDecisions": ["accept", "decline"],
                "networkApprovalContext": { "host": "api.example.test" },
                "proposedExecpolicyAmendment": ["prefix_rule", "cargo"],
                "proposedNetworkPolicyAmendments": [{ "host": "api.example.test", "action": "allow" }]
            }),
        )
        .unwrap();
        assert_eq!(
            a.payload["availableDecisions"],
            json!(["accept", "decline"])
        );
        assert_eq!(
            a.payload["networkApprovalContext"]["host"],
            "api.example.test"
        );
        assert_eq!(a.payload["proposedExecpolicyAmendment"][1], "cargo");
        assert_eq!(
            a.payload["proposedNetworkPolicyAmendments"][0]["action"],
            "allow"
        );
    }
}
