//! MCP stdio 服务:initialize / tools/list / tools/call。复刻 asset-pipeline-mcp 骨架。
//! 五工具为子进程包上游 rx CLI/rurixc(H:\rurix),超时+1MB 截断,F4 wave.1。
//! F4 wave.2:+ graph_validate/graph_create/graph_get(校验器在 forge-logic,纯函数)。

use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{json, Value};

use crate::codetool;
use crate::graphtool;
use crate::rxtool;

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "rx_check",
                "description": ".rx 静态检查:子进程包 rurixc <file> --emit=check --error-format=json(H:\\rurix;rx check 不透传 --error-format,契约决策 D-F4-A);结构化诊断 {code,severity,file,span,message,suggestion?};超时+1MB 截断,F4 wave.1",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": ".rx 源文件路径" }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "rx_build",
                "description": ".rx 构建:子进程包 rx build <file> [-o out](H:\\rurix);产物存在才填 artifact(缺省 <file>.exe);诊断从 stderr error[RXnnnn] 行提取;超时+截断,F4 wave.1",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": ".rx 源文件路径" },
                        "out": { "type": "string", "description": "产物路径(可选,缺省 <file>.exe)" }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "rx_run",
                "description": ".rx 构建并运行:子进程包 rx run(透传产物退出码);stdout/stderr 落盘 <workspace>/data/code-runs/<ts>-<pid>/ 后返相对路径;上游不透传程序参数(args 非空 → USAGE 如实报错);缺省 30s 超时,F4 wave.1",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": ".rx 源文件路径" },
                        "args": { "type": "array", "items": { "type": "string" }, "description": "程序参数(上游 rx run 暂不支持,非空即 USAGE 错误)" },
                        "timeoutMs": { "type": "integer", "description": "超时毫秒(缺省 30000)" }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "rx_fmt",
                "description": ".rx 格式化:子进程包 rx fmt;checkOnly=true 只校验不改写(exit 1 = 未格式化,附格式化输出);否则格式化结果写回文件(写失败回滚);F4 wave.1",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": ".rx 源文件路径" },
                        "checkOnly": { "type": "boolean", "description": "true = 只校验(缺省 false 写回)" }
                    },
                    "required": ["file"]
                }
            },
            {
                "name": "rx_test",
                "description": ".rx #[test] 运行:子进程包 rx test(逐测试子进程隔离);解析 PASS/FAIL 汇总与 error[RX7011] 失败明细;契约表 rx_test 无 file 参数但上游 CLI 必需,file 缺省如实 USAGE 报错;缺省 120s 超时,F4 wave.1",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": ".rx 测试文件(契约表未列但上游必需)" },
                        "filter": { "type": "string", "description": "测试名子串过滤" },
                        "timeoutMs": { "type": "integer", "description": "超时毫秒(缺省 120000)" }
                    }
                }
            },
            {
                "name": "graph_validate",
                "description": ".rxgraph 全图校验(10 §5:schema + 节点注册表 + 悬空输入/类型不匹配 + 执行边/数据边双环;校验器 forge-logic 纯函数);graph 与 path 二选一,path 相对项目根;拒绝为 ok:false + errors[{code,message,nodeId?}],F4 wave.2",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "graph": { "type": "object", "description": "图 JSON 本体(与 path 二选一)" },
                        "path": { "type": "string", "description": "图文件路径(相对项目根,如 Content/Graphs/door.rxgraph)" }
                    }
                }
            },
            {
                "name": "graph_create",
                "description": "建/更新 .rxgraph:validate 通过才落 <project>/Content/Graphs/<name>.rxgraph(确定性 JSON,覆盖即更新);name 须 [A-Za-z0-9_-]+(否则 GRAPH_BAD_NAME);校验不过 ok:false 不落盘,F4 wave.2",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "图文件名(不含扩展名;[A-Za-z0-9_-]+)" },
                        "graph": { "type": "object", "description": "图 JSON 本体" }
                    },
                    "required": ["name", "graph"]
                }
            },
            {
                "name": "graph_get",
                "description": "读 .rxgraph:path 相对项目根 → {graph};不存在 → GRAPH_NOT_FOUND,F4 wave.2",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "图文件路径(相对项目根)" }
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "code_symbol_search",
                "description": ".rx 符号搜索:递归扫项目 .rx(排除 target/vendor/点目录),文本扫描 fn/struct/enum/mod/trait/const/static/type 条目头(诚实注记:上游 --emit=reflection 仅枚举 shader entry 且 JSON 无 file/span,RXS-0304/0305 实测,故符号表为文本级非语义级);query 子串大小写不敏感 + kinds 过滤 → {symbols:[{name,kind,file,span}], skipped[]}(读失败文件如实计入 skipped);span 0 基 line/character,F4 wave.4",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "符号名子串(大小写不敏感)" },
                        "kinds": { "type": "array", "items": { "type": "string" }, "description": "条目类过滤(fn/struct/enum/mod/trait/const/static/type)" }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "code_references",
                "description": "符号引用查询:同源符号扫描定位定义(精确→不敏感→子串;0 → SYMBOL_NOT_FOUND,多义 → AMBIGUOUS_SYMBOL)→ rurixc --tooling-server 常驻 LSP 会话 didOpen + textDocument/references(崩溃/超时作废重连一次,再败如实 LSP_ERROR);refs 限定义所在文件(上游 ToolingSession 单文档语义),F4 wave.4",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "symbolQuery": { "type": "string", "description": "符号名字符串" }
                    },
                    "required": ["symbolQuery"]
                }
            },
            {
                "name": "code_structured_edit",
                "description": ".rx 结构化编辑:file 项目相对路径;edits 逐项 span(0 基 line/character 文本区间)replace/insert/delete,或 symbolQuery 定位本文件符号名区间替换;全部解析+重叠校验过 → 倒序应用写回 → rurixc --emit=check --error-format=json 回读 → {applied:true, newDiagnostics[]};任一 edit 越界/定位失败/区间重叠 → 不写盘 {applied:false, error};回读失败 → applied:true + diagError 如实,F4 wave.4",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "file": { "type": "string", "description": ".rx 文件路径(相对项目根)" },
                        "edits": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "kind": { "type": "string", "description": "replace|insert|delete" },
                                    "span": { "type": "object", "description": "{start:{line,character}, end:{line,character}}(0 基;与 symbolQuery 二选一)" },
                                    "symbolQuery": { "type": "string", "description": "符号名(与 span 二选一)" },
                                    "content": { "type": "string", "description": "替换/插入文本(delete 忽略)" }
                                },
                                "required": ["kind"]
                            }
                        }
                    },
                    "required": ["file", "edits"]
                }
            }
        ]
    })
}

fn ok(id: Value, result: Value) -> Value { json!({ "jsonrpc": "2.0", "id": id, "result": result }) }
fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_wrap(v: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    let mut out = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error { out["isError"] = json!(true); }
    out
}

/// 返 (result, is_error):工具级错误 → isError:true + {error,message} 文本(如实,不 JSON-RPC 层报错)。
fn call_tool(params: &Value, project_root: &Path) -> Result<(Value, bool), Value> {
    let name = params.get("name").and_then(Value::as_str)
        .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺工具名"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() { return Err(err(Value::Null, -32602, "invalid params: arguments 须为对象")); }

    let r = match name {
        "rx_check" => rxtool::check(&args),
        "rx_build" => rxtool::build(&args),
        "rx_run" => rxtool::run_tool(&args),
        "rx_fmt" => rxtool::fmt(&args),
        "rx_test" => rxtool::test(&args),
        "graph_validate" => graphtool::validate(&args, project_root),
        "graph_create" => graphtool::create(&args, project_root),
        "graph_get" => graphtool::get(&args, project_root),
        "code_symbol_search" => codetool::symbol_search(&args, project_root),
        "code_references" => codetool::references(&args, project_root),
        "code_structured_edit" => codetool::structured_edit(&args, project_root),
        _ => return Err(err(Value::Null, -32602, &format!("未知工具:{name}"))),
    };
    match r {
        Ok(v) => Ok((v, false)),
        Err(e) => Ok((json!({ "error": e.code, "message": e.message }), true)),
    }
}

pub fn serve_stdio(project_root: &Path) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line { Ok(l) => l, Err(_) => break };
        if line.trim().is_empty() { continue; }
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
            "initialize" => id.map(|i| ok(i, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "code-forge", "version": env!("CARGO_PKG_VERSION") }
            }))),
            "notifications/initialized" | "notifications/cancelled" => None,
            "ping" => id.map(|i| ok(i, json!({}))),
            "tools/list" => id.map(|i| ok(i, tool_list())),
            "tools/call" => id.map(|i| {
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                match call_tool(&params, project_root) {
                    Ok((result, is_error)) => ok(i, tool_wrap(&result, is_error)),
                    Err(mut e) => { e["id"] = i.clone(); e }
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
