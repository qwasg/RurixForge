//! Thin STDIO MCP facade. The running Forge coordinator owns all jobs and engine state.
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::Duration;

struct Bridge {
    origin: String,
    workspace: String,
    http: ureq::Agent,
}

fn text_result(value: Value, error: bool) -> Value {
    json!({"content":[{"type":"text","text":value.to_string()}],"isError":error})
}

fn tool(name: &str, description: &str, fields: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,"inputSchema":{
        "type":"object","properties":fields,"required":required,"additionalProperties":false
    }})
}

fn tools_list() -> Value {
    let job = json!({"jobId":{"type":"string"}});
    let leased = json!({"jobId":{"type":"string"},"leaseToken":{"type":"string"}});
    let mut progress = leased.clone();
    progress["stage"] = json!({"type":"string"});
    progress["message"] = json!({"type":"string"});
    progress["heartbeat"] = json!({"type":"boolean"});
    let mut bind = leased.clone();
    bind["sourcePath"] =
        json!({"type":"string","description":"Saved .blend path inside this project"});
    bind["autoSync"] = json!({"type":"boolean","default":true});
    let mut preview = job.clone();
    for (name, schema) in [
        (
            "width",
            json!({"type":"integer","minimum":64,"maximum":2048}),
        ),
        (
            "height",
            json!({"type":"integer","minimum":64,"maximum":2048}),
        ),
        ("clip", json!({"type":"string"})),
        ("time", json!({"type":"number","minimum":0})),
        (
            "yaw",
            json!({"type":"number","description":"Orbit angle in radians"}),
        ),
    ] {
        preview[name] = schema;
    }
    json!({"tools":[
        tool("blender_status", "Check Blender installation and the coordinator's actual connection status. This bridge does not provide computer-use itself.", json!({}), &[]),
        tool("blender_job_create", "Create a Blender production job in this project. Use Codex native computer-use for modelling, UVs, texturing, rigging and animation; no alternate desktop provider is invoked.", json!({
            "name":{"type":"string"},"prompt":{"type":"string"},
            "kind":{"type":"string","enum":["prop","map","character"]}
        }), &["name","prompt","kind"]),
        tool("blender_job_list", "List durable Blender jobs in this project, including jobs awaiting Codex.",json!({}), &[]),
        tool("blender_job_get", "Read a job's requirements, exact source path, state, published assets and diagnostics.",job.clone(), &["jobId"]),
        tool("blender_job_claim", "Claim exclusive desktop execution. First verify native computer-use is callable, then attest computerUse=true. Save the returned leaseToken for progress, binding and publishing.", json!({
            "jobId":{"type":"string"},"executorId":{"type":"string"},
            "capabilities":{"type":"object","properties":{"computerUse":{"type":"boolean","const":true}},"required":["computerUse"]}
        }), &["jobId","executorId","capabilities"]),
        tool("blender_job_progress", "Report actual progress or renew your lease during authoring. Do not mark publishing complete from a desktop screenshot.",progress, &["jobId","leaseToken"]),
        tool("blender_source_bind", "Bind the saved Blender source to this job. Subsequent saves synchronize only after the first successful publish.",bind, &["jobId","leaseToken","sourcePath"]),
        tool("blender_publish", "Publish the saved source asynchronously through the fixed Blender exporter, validate its full model and template, then atomically import. Poll job_get for completion.",leased, &["jobId","leaseToken"]),
        tool("blender_job_cancel", "Cancel this job and its queued work without deleting its last published assets.",job.clone(), &["jobId"]),
        tool("blender_sync_retry", "Retry a failed export/sync or return interrupted authoring to awaiting Codex. No desktop action is automatically submitted.",job.clone(), &["jobId"]),
        tool("blender_template_preview", "Render the published template in the running Forge engine; use this to verify geometry, materials and animation. Does not create a separate engine process.",preview, &["jobId"])
    ]})
}

fn route(name: &str, args: &Value) -> Result<(&'static str, String), String> {
    match name {
        "blender_status" => return Ok(("GET", "status".into())),
        "blender_job_create" => return Ok(("POST", "jobs".into())),
        "blender_job_list" => return Ok(("GET", "jobs".into())),
        _ => {}
    }
    let suffix = match name {
        "blender_job_get" => "",
        "blender_job_claim" => "claim",
        "blender_job_progress" => "progress",
        "blender_source_bind" => "bind",
        "blender_publish" => "publish",
        "blender_job_cancel" => "cancel",
        "blender_sync_retry" => "retry",
        "blender_template_preview" => "preview",
        _ => return Err(format!("Unknown tool: {name}")),
    };
    let id = args
        .get("jobId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("jobId is required")?;
    if !id
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("Invalid jobId".into());
    }
    Ok((
        if suffix.is_empty() { "GET" } else { "POST" },
        format!(
            "jobs/{id}{}",
            if suffix.is_empty() {
                String::new()
            } else {
                format!("/{suffix}")
            }
        ),
    ))
}

impl Bridge {
    fn call(&self, name: &str, args: Value) -> Value {
        let result = (|| -> Result<Value, String> {
            if !args.is_object() {
                return Err("arguments must be an object".into());
            }
            // Workspace selection is immutable process configuration, never model input.
            if args.get("workspaceId").is_some() {
                return Err("workspaceId is fixed by this MCP configuration".into());
            }
            let (method, path) = route(name, &args)?;
            let url = format!("{}/api/forge/blender/{path}", self.origin);
            let response = if method == "GET" {
                self.http
                    .get(&url)
                    .query("workspaceId", &self.workspace)
                    .call()
            } else {
                let mut body = args;
                body.as_object_mut().unwrap().remove("jobId");
                body["workspaceId"] = json!(self.workspace);
                self.http.post(&url).send_json(body)
            };
            let response = match response {
                Ok(r) => r,
                Err(ureq::Error::Status(_, r)) => return Ok(text_result(
                    r.into_json().unwrap_or(json!({"error":{"code":"BAD_RESPONSE","message":"Forge returned an unreadable error"}})), true)),
                Err(e) => return Err(format!("Forge coordinator unavailable: {e}. Open RurixForge and retry.")),
            };
            let value: Value = response
                .into_json()
                .map_err(|e| format!("Invalid Forge response: {e}"))?;
            // Preview may already be an MCP envelope. Preserve its image blocks.
            if name == "blender_template_preview" && value.get("content").is_some() {
                return Ok(value);
            }
            Ok(text_result(value, false))
        })();
        result.unwrap_or_else(|message| {
            text_result(
                json!({"error":{"code":"BLENDER_BRIDGE_ERROR","message":message}}),
                true,
            )
        })
    }

    fn dispatch(&self, req: Value) -> Option<Value> {
        let id = req.get("id")?.clone();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "initialize" => json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},
                "serverInfo":{"name":"rurix-blender","version":env!("CARGO_PKG_VERSION")},
                "instructions":"Use native Codex computer-use for Blender authoring. This server only manages project jobs, fixed exports, imports and synchronization. Verify results using the engine template preview."}),
            "ping" => json!({}),
            "tools/list" => tools_list(),
            "tools/call" => self.call(
                req["params"]["name"].as_str().unwrap_or(""),
                req["params"].get("arguments").cloned().unwrap_or(json!({})),
            ),
            _ => {
                return Some(
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}),
                )
            }
        };
        Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
    }
}

fn config(args: impl Iterator<Item = String>) -> Result<(String, String), String> {
    let mut origin =
        std::env::var("FORGE_BLENDER_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:8103".into());
    let mut workspace = std::env::var("FORGE_BLENDER_WORKSPACE_ID").unwrap_or_default();
    let mut args = args;
    while let Some(arg) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--origin" => origin = value,
            "--workspace-id" => workspace = value,
            _ => return Err(format!("Unknown option {arg}")),
        }
    }
    origin = origin.trim_end_matches('/').to_string();
    let port = origin
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| origin.strip_prefix("http://localhost:"));
    if !port
        .and_then(|p| p.parse::<u16>().ok())
        .is_some_and(|p| p > 0)
    {
        return Err("--origin must be a loopback HTTP origin with an explicit port".into());
    }
    if workspace.trim().is_empty() {
        return Err(
            "--workspace-id is required (use default explicitly for the default project)".into(),
        );
    }
    Ok((origin, workspace))
}

fn main() {
    let (origin, workspace) = match config(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let bridge = Bridge {
        origin,
        workspace,
        http: ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .redirects(0)
            .build(),
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(req) => bridge.dispatch(req),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            ),
        };
        if let Some(response) = response {
            if writeln!(stdout, "{response}")
                .and_then(|_| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_cross_project_and_path_injection() {
        assert!(route("blender_job_get", &json!({"jobId":"../else"})).is_err());
        assert!(route("blender_job_get", &json!({"jobId":"x?workspaceId=else"})).is_err());
        assert!(config(
            ["--origin", "http://remote.test:8103", "--workspace-id", "a"]
                .map(str::to_string)
                .into_iter()
        )
        .is_err());
        let bridge = Bridge {
            origin: "http://127.0.0.1:1".into(),
            workspace: "a".into(),
            http: ureq::agent(),
        };
        assert_eq!(
            bridge.call("blender_status", json!({"workspaceId":"b"}))["isError"],
            true
        );
    }
    #[test]
    fn every_advertised_tool_has_a_route() {
        for tool in tools_list()["tools"].as_array().unwrap() {
            assert!(route(
                tool["name"].as_str().unwrap(),
                &json!({"jobId":"blender-123"})
            )
            .is_ok());
        }
        assert_eq!(
            route("blender_publish", &json!({"jobId":"x"})).unwrap(),
            ("POST", "jobs/x/publish".into())
        );
    }
    #[test]
    fn notifications_do_not_produce_protocol_responses() {
        let bridge = Bridge {
            origin: "http://127.0.0.1:1".into(),
            workspace: "a".into(),
            http: ureq::agent(),
        };
        assert!(bridge
            .dispatch(json!({"method":"notifications/initialized"}))
            .is_none());
        assert_eq!(
            bridge
                .dispatch(json!({"id":1,"method":"initialize"}))
                .unwrap()["result"]["serverInfo"]["name"],
            "rurix-blender"
        );
    }
}
