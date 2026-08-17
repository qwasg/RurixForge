//! MCP stdio 服务:newline-delimited JSON-RPC(initialize / tools/list / tools/call)。

use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{json, Value};

use crate::supervisor::Supervisor;

/// 工具清单(tools/list 返回):F0 既有 5 个 + F1 场景编辑 27 个。
fn tool_list() -> Value {
    let id_prop = json!({ "id": { "type": "integer", "description": "实体 id" } });
    let trs_props = json!({
        "translation": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z]" },
        "rotation": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z,w] 四元数" },
        "scale": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z]" }
    });
    let mut trs_with_id = trs_props.as_object().unwrap().clone();
    trs_with_id.insert("id".to_string(), id_prop["id"].clone());
    json!({
        "tools": [
            // ---- F0 既有 ----
            {
                "name": "host_ping",
                "description": "探测 engine-host 存活(pong/version/uptimeSec/backend/pid)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "scene_new",
                "description": "新建空场景",
                "inputSchema": {
                    "type": "object",
                    "properties": { "name": { "type": "string", "description": "场景名(可选)" } }
                }
            },
            {
                "name": "scene_summary",
                "description": "场景 + 物理 + 渲染 + 事件 ring 摘要(含 playState)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "render_once",
                "description": "CPU 软光栅渲一帧一个三角形,返回 frames/tris/nonZeroPixels",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "host_events",
                "description": "读 <workspace>/data/host-events.jsonl,返回事件行数组",
                "inputSchema": { "type": "object", "properties": {} }
            },
            // ---- entity.* ----
            {
                "name": "entity_create",
                "description": "创建实体(name 必填;components/translation/rotation/scale 可选)",
                "inputSchema": {
                    "type": "object",
                    "properties": ({
                        let mut m = trs_props.as_object().unwrap().clone();
                        m.insert("name".to_string(), json!({ "type": "string" }));
                        m.insert("components".to_string(), json!({ "type": "array", "description": "[{type,enabled?,props?}]" }));
                        m
                    }),
                    "required": ["name"]
                }
            },
            {
                "name": "entity_destroy",
                "description": "销毁实体(可 undo)",
                "inputSchema": { "type": "object", "properties": id_prop.clone(), "required": ["id"] }
            },
            {
                "name": "entity_rename",
                "description": "重命名实体",
                "inputSchema": {
                    "type": "object",
                    "properties": ({
                        let mut m = id_prop.as_object().unwrap().clone();
                        m.insert("name".to_string(), json!({ "type": "string" }));
                        m
                    }),
                    "required": ["id", "name"]
                }
            },
            {
                "name": "entity_get",
                "description": "取单个实体(id/name/transform/components 全量)",
                "inputSchema": { "type": "object", "properties": id_prop.clone(), "required": ["id"] }
            },
            {
                "name": "entity_list",
                "description": "列出活动场景全部实体(play 态为运行态)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "entity_batch_apply",
                "description": "批量编辑(原子:任一失败全回滚;op ∈ create/transform_set/component_set)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "ops": { "type": "array", "description": "[{op,...}]" } },
                    "required": ["ops"]
                }
            },
            // ---- component.* ----
            {
                "name": "component_add",
                "description": "给实体加组件(type 须在注册表,props 按 schema 校验)",
                "inputSchema": {
                    "type": "object",
                    "properties": ({
                        let mut m = id_prop.as_object().unwrap().clone();
                        m.insert("type".to_string(), json!({ "type": "string" }));
                        m.insert("props".to_string(), json!({ "type": "object" }));
                        m.insert("enabled".to_string(), json!({ "type": "boolean" }));
                        m
                    }),
                    "required": ["id", "type"]
                }
            },
            {
                "name": "component_remove",
                "description": "移除实体上的组件",
                "inputSchema": {
                    "type": "object",
                    "properties": ({
                        let mut m = id_prop.as_object().unwrap().clone();
                        m.insert("type".to_string(), json!({ "type": "string" }));
                        m
                    }),
                    "required": ["id", "type"]
                }
            },
            {
                "name": "component_set",
                "description": "改组件 props/enabled(至少给一项)",
                "inputSchema": {
                    "type": "object",
                    "properties": ({
                        let mut m = id_prop.as_object().unwrap().clone();
                        m.insert("type".to_string(), json!({ "type": "string" }));
                        m.insert("props".to_string(), json!({ "type": "object" }));
                        m.insert("enabled".to_string(), json!({ "type": "boolean" }));
                        m
                    }),
                    "required": ["id", "type"]
                }
            },
            {
                "name": "component_get",
                "description": "取实体上某组件实例",
                "inputSchema": {
                    "type": "object",
                    "properties": ({
                        let mut m = id_prop.as_object().unwrap().clone();
                        m.insert("type".to_string(), json!({ "type": "string" }));
                        m
                    }),
                    "required": ["id", "type"]
                }
            },
            {
                "name": "component_list_types",
                "description": "组件注册表(类型名 + 字段 schema 简表)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            // ---- transform.* ----
            {
                "name": "transform_set",
                "description": "设置实体 TRS(字段可选,未给沿用旧值)",
                "inputSchema": { "type": "object", "properties": trs_with_id, "required": ["id"] }
            },
            {
                "name": "transform_get",
                "description": "取实体 TRS",
                "inputSchema": { "type": "object", "properties": id_prop.clone(), "required": ["id"] }
            },
            {
                "name": "transform_batch_set",
                "description": "批量设置 TRS(原子;items:[{id, translation?, rotation?, scale?}])",
                "inputSchema": {
                    "type": "object",
                    "properties": { "items": { "type": "array" } },
                    "required": ["items"]
                }
            },
            // ---- scene.* 存取 / diff / checkpoint ----
            {
                "name": "scene_save",
                "description": "保存编辑态场景(缺省 <cwd>/data/scene.rxscene;规范字节,确定性)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "目标路径(可选)" } }
                }
            },
            {
                "name": "scene_load",
                "description": "加载 .rxscene 替换编辑态场景(play 态禁止)",
                "inputSchema": {
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }
            },
            {
                "name": "scene_diff",
                "description": "编辑态场景与磁盘文件 diff,返回 {same, summary}",
                "inputSchema": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "对比路径(可选)" } }
                }
            },
            {
                "name": "scene_checkpoint",
                "description": "编辑态场景快照压栈",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "scene_rollback",
                "description": "弹栈恢复最近一次 checkpoint",
                "inputSchema": { "type": "object", "properties": {} }
            },
            // ---- edit.* ----
            {
                "name": "edit_undo",
                "description": "撤销最近一次变更(命令栈)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "edit_redo",
                "description": "重做最近一次撤销",
                "inputSchema": { "type": "object", "properties": {} }
            },
            // ---- play.*(PIE) ----
            {
                "name": "play_enter",
                "description": "进入 PIE:克隆编辑态为运行态(edit → play_running)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "play_pause",
                "description": "暂停(play_running → play_paused)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "play_resume",
                "description": "恢复(play_paused → play_running)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "play_step",
                "description": "单帧推进(仅 play_paused 合法)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "play_exit",
                "description": "退出 PIE:销毁运行态,编辑态原样恢复",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "play_state",
                "description": "PIE 状态:edit | play_running | play_paused",
                "inputSchema": { "type": "object", "properties": {} }
            }
        ]
    })
}

/// 加锁(毒化时取回内部值)。
fn lock(s: &Mutex<Supervisor>) -> MutexGuard<'_, Supervisor> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// 构造 result 响应。
fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// 构造 error 响应。
fn err(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

/// MCP 工具结果包装(content text + 可选 isError)。
fn tool_wrap(v: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    let mut out = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error {
        out["isError"] = json!(true);
    }
    out
}

/// 代理 host 方法:host 层失败 → 工具级 isError(非协议层错误)。
fn host_tool(sup: &Arc<Mutex<Supervisor>>, method: &str, params: Value) -> Value {
    match lock(sup).call(method, params) {
        Ok(v) => tool_wrap(&v, false),
        Err(e) => tool_wrap(&e, true),
    }
}

/// F1 新增工具 → host 方法透传映射(snake_case → 点分)。
fn passthrough_method(name: &str) -> Option<&'static str> {
    Some(match name {
        "entity_create" => "entity.create",
        "entity_destroy" => "entity.destroy",
        "entity_rename" => "entity.rename",
        "entity_get" => "entity.get",
        "entity_list" => "entity.list",
        "entity_batch_apply" => "entity.batchApply",
        "component_add" => "component.add",
        "component_remove" => "component.remove",
        "component_set" => "component.set",
        "component_get" => "component.get",
        "component_list_types" => "component.listTypes",
        "transform_set" => "transform.set",
        "transform_get" => "transform.get",
        "transform_batch_set" => "transform.batchSet",
        "scene_save" => "scene.save",
        "scene_load" => "scene.load",
        "scene_diff" => "scene.diff",
        "scene_checkpoint" => "scene.checkpoint",
        "scene_rollback" => "scene.rollback",
        "edit_undo" => "edit.undo",
        "edit_redo" => "edit.redo",
        "play_enter" => "play.enter",
        "play_pause" => "play.pause",
        "play_resume" => "play.resume",
        "play_step" => "play.step",
        "play_exit" => "play.exit",
        "play_state" => "play.state",
        _ => return None,
    })
}

/// tools/call 分派:Err = JSON-RPC 协议错误(-32602 等),Ok = 工具结果。
fn call_tool(sup: &Arc<Mutex<Supervisor>>, params: &Value) -> Result<Value, Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺工具名"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return Err(err(Value::Null, -32602, "invalid params: arguments 须为对象"));
    }
    match name {
        "host_ping" => Ok(host_tool(sup, "host.ping", json!({}))),
        "scene_new" => {
            let mut rpc_params = json!({});
            if let Some(n) = args.get("name") {
                match n.as_str() {
                    Some(s) => rpc_params["name"] = json!(s),
                    None => {
                        return Err(err(Value::Null, -32602, "invalid params: name 须为字符串"));
                    }
                }
            }
            Ok(host_tool(sup, "scene.new", rpc_params))
        }
        "scene_summary" => Ok(host_tool(sup, "scene.summary", json!({}))),
        "render_once" => Ok(host_tool(sup, "render.once", json!({}))),
        "host_events" => {
            let events = lock(sup).read_events_log();
            Ok(tool_wrap(&Value::Array(events), false))
        }
        other => match passthrough_method(other) {
            Some(method) => Ok(host_tool(sup, method, args)),
            None => Err(err(Value::Null, -32602, &format!("未知工具:{other}"))),
        },
    }
}

/// stdio 主循环:逐行 NDJSON;notification(无 id)不回包。
pub fn serve_stdio(sup: Arc<Mutex<Supervisor>>) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = err(Value::Null, -32700, &format!("parse error: {e}"));
                let _ = writeln!(stdout, "{resp}");
                let _ = stdout.flush();
                continue;
            }
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let resp: Option<Value> = match method {
            "initialize" => id.map(|i| {
                ok(
                    i,
                    json!({
                        "protocolVersion": "2024-11-05",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "engine-scene-mcp", "version": env!("CARGO_PKG_VERSION") }
                    }),
                )
            }),
            "notifications/initialized" | "notifications/cancelled" => None,
            "ping" => id.map(|i| ok(i, json!({}))),
            "tools/list" => id.map(|i| ok(i, tool_list())),
            "tools/call" => id.map(|i| {
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                match call_tool(&sup, &params) {
                    Ok(result) => ok(i, result),
                    Err(mut e) => {
                        // call_tool 协议错误的 id 占位为 Null,此处回填真实 id。
                        e["id"] = i.clone();
                        e
                    }
                }
            }),
            "" => id.map(|i| err(i, -32600, "invalid request: 缺 method")),
            other => id.map(|i| err(i, -32601, &format!("method not found: {other}"))),
        };
        if let Some(r) = resp {
            let _ = writeln!(stdout, "{r}");
            let _ = stdout.flush();
        }
    }
}
