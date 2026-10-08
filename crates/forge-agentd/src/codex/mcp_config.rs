//! Codex 线程的 `config.mcp_servers` 注入。
//!
//! 「自动接入本项目的 MCP 工程」就落在这里:Codex 自己不知道本仓有哪些 MCP 服务,
//! 但 `thread/start` 接受一份 per-thread 的 MCP 服务表,于是把 [mcp.rs](crates/forge-agentd/src/mcp.rs)
//! 的 `spawn_spec`/`spawn_env` 直接翻译过去 —— 用同一份 spawn 真相,而不是在这里
//! 重抄一遍路径与参数(重抄必漂移)。
//!
//! R-5:store 私有源令牌经 `env` 进子进程,和本地引擎同路;这份 JSON 只交给 codex
//! 子进程的 stdin,不进事件、不进日志、不进 REST 响应(状态面另有 [`status_json`],
//! 它只报服务名与命令,不含 env)。

use serde_json::{json, Value};
use std::path::Path;

use crate::mcp::ServerKind;

/// MCP 服务启动超时(秒)。engine-scene 要拉起 engine-host,冷启动比其他服务慢得多。
const STARTUP_TIMEOUT_SECS: u64 = 60;
/// 单次工具调用超时(秒)。取本地引擎那边最宽的预算(gen_mesh 2000s)同量级,
/// 否则 Codex 会在 3D 生成还没回来时就把工具调用判超时。
const TOOL_TIMEOUT_SECS: u64 = 2100;

/// 一条 MCP 服务的注入规格。
pub struct ServerSpec {
    pub name: &'static str,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// 生成当前生效的服务规格清单(七工 + 可选 computer-use)。
pub fn specs_for(project_root: &Path) -> Vec<ServerSpec> {
    ServerKind::active()
        .into_iter()
        .map(|kind| {
            let (bin, args) = crate::mcp::spawn_spec(kind, project_root);
            ServerSpec {
                name: kind.server_name(),
                command: bin.to_string_lossy().into_owned(),
                args,
                env: crate::mcp::spawn_env(kind, project_root),
            }
        })
        .collect()
}

/// `thread/start` 的 `config.mcp_servers` 值。`auto_register_mcp` 关掉 → 空表
/// (Codex 只剩自己的内建工具,这是用户显式选择的形态)。
pub fn mcp_servers_json(project_root: &Path, auto_register: bool) -> Value {
    if !auto_register {
        return json!({});
    }
    let mut map = serde_json::Map::new();
    for spec in specs_for(project_root) {
        let mut env = serde_json::Map::new();
        for (k, v) in &spec.env {
            env.insert(k.clone(), Value::String(v.clone()));
        }
        map.insert(
            spec.name.to_string(),
            json!({
                "command": spec.command,
                "args": spec.args,
                "env": Value::Object(env),
                "startup_timeout_sec": STARTUP_TIMEOUT_SECS,
                "tool_timeout_sec": TOOL_TIMEOUT_SECS,
            }),
        );
    }
    Value::Object(map)
}

/// Host collaboration is infrastructure, independent from the project MCP
/// auto-registration preference. The capability is runtime-only and never
/// appears in status JSON, events, command arguments, or global Codex config.
pub fn collaboration_server(endpoint: &str, token: &str) -> Result<Value, String> {
    let command =
        std::env::current_exe().map_err(|_| "无法确定 Forge 协作桥可执行文件".to_string())?;
    Ok(json!({
        "command": command.to_string_lossy(),
        "args": ["collaboration-stdio"],
        "env": {
            (crate::collaboration_stdio::ENDPOINT_ENV): endpoint,
            (crate::collaboration_stdio::TOKEN_ENV): token,
        },
        "required": true,
        "enabled": true,
        // These are host routing operations. Actual child tools continue through
        // Forge's existing permission service; a second Codex MCP gate would
        // reject even agent_list when the session uses approvalPolicy=never.
        "default_tools_approval_mode": "approve",
        "startup_timeout_sec": 15,
        "tool_timeout_sec": TOOL_TIMEOUT_SECS,
    }))
}

/// Replace each project MCP process with a capability-bound host transport.
pub fn scoped_servers(endpoint: &str, token: &str, auto_register: bool) -> Result<Value, String> {
    if !auto_register { return Ok(json!({})); }
    let command = std::env::current_exe().map_err(|e|e.to_string())?;
    let mut map = serde_json::Map::new();
    for kind in ServerKind::active() {
        map.insert(kind.server_name().into(), json!({
            "command":command.to_string_lossy(),"args":["editor-stdio"],
            "env":{"FORGE_EDITOR_ENDPOINT":endpoint,"FORGE_EDITOR_TOKEN":token,"FORGE_EDITOR_SERVER":kind.server_name()},
            "startup_timeout_sec":STARTUP_TIMEOUT_SECS,"tool_timeout_sec":TOOL_TIMEOUT_SECS,
            "default_tools_approval_mode":"approve"
        }));
    }
    Ok(Value::Object(map))
}

/// 设置页的 MCP 状态表(服务名 + 命令 + 二进制是否就位;无 env)。
pub fn status_json(project_root: &Path) -> Value {
    let auto = crate::codex::config::load().auto_register_mcp;
    let servers: Vec<Value> = specs_for(project_root)
        .into_iter()
        .map(|spec| {
            let p = Path::new(&spec.command);
            // 裸命令名(node 之类)由 PATH 解析,不能按「文件不存在」判缺失。
            let bare = p.parent().map(|d| d.as_os_str().is_empty()).unwrap_or(true);
            json!({
                "name": spec.name,
                "command": spec.command,
                "args": spec.args,
                "available": bare || p.exists(),
                "envKeys": spec.env.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "autoRegisterMcp": auto,
        "projectRoot": project_root.to_string_lossy(),
        "servers": servers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_servers_are_host_transports_and_never_spawn_another_engine() {
        let servers=scoped_servers("http://127.0.0.1:8123","opaque-test-capability",true).unwrap();
        for spec in servers.as_object().unwrap().values(){
            assert_eq!(spec["args"],json!(["editor-stdio"]));
            assert_eq!(spec["env"]["FORGE_EDITOR_TOKEN"],"opaque-test-capability");
            assert!(spec["env"].get("FORGE_ASSET_PROJECT_ROOT").is_none());
        }
        assert_eq!(scoped_servers("unused","unused",false).unwrap(),json!({}));
    }

    /// 注入表覆盖七工、路径与参数取自 spawn_spec、engine-scene 的项目根经 env 传。
    #[test]
    fn injects_builtin_servers_with_spawn_truth() {
        let root = Path::new("D:/proj/demo");
        let v = mcp_servers_json(root, true);
        let map = v.as_object().expect("应是对象");
        for name in [
            "engine-scene",
            "asset-pipeline",
            "code-forge",
            "gen-image",
            "gen-model",
            "context",
            "store",
        ] {
            assert!(map.contains_key(name), "缺服务 {name}: {v}");
        }
        let scene = &map["engine-scene"];
        assert_eq!(scene["env"]["FORGE_PROJECT_ROOT"], "D:/proj/demo");
        // engine-scene 无 --project 参数面,项目根只走 env(写成参数会被上游忽略)。
        assert_eq!(scene["args"], json!([]));
        assert!(scene["command"]
            .as_str()
            .unwrap()
            .contains("engine-scene-mcp"));
        // asset-pipeline 反过来:只认 --project 参数。
        assert_eq!(
            map["asset-pipeline"]["args"],
            json!(["--project", "D:/proj/demo"])
        );
        // 超时给足:engine-host 冷启动与 3D 生成都比缺省宽。
        assert!(scene["startup_timeout_sec"].as_u64().unwrap() >= 60);
        assert!(scene["tool_timeout_sec"].as_u64().unwrap() >= 2000);
    }

    /// 关掉自动注册 → 空表(不是「少几个」,是一个都不注入)。
    #[test]
    fn auto_register_off_yields_empty_table() {
        assert_eq!(mcp_servers_json(Path::new("/p"), false), json!({}));
    }

    #[test]
    fn collaboration_bridge_is_required_and_uses_only_scoped_environment() {
        let config =
            collaboration_server("http://127.0.0.1:8103", "private-test-capability").unwrap();
        assert_eq!(config["required"], true);
        assert_eq!(config["default_tools_approval_mode"], "approve");
        assert_eq!(config["args"], json!(["collaboration-stdio"]));
        assert_eq!(
            config["env"][crate::collaboration_stdio::TOKEN_ENV],
            "private-test-capability"
        );
        assert!(!config["args"]
            .to_string()
            .contains("private-test-capability"));
        assert!(!status_json(Path::new("/p"))
            .to_string()
            .contains("private-test-capability"));
    }

    /// 状态面只报服务名/命令/可用性与 env 的**键名**,绝不回显 env 值。
    #[test]
    fn status_reports_env_keys_only() {
        let v = status_json(Path::new("/p"));
        let text = v.to_string();
        assert!(text.contains("engine-scene"));
        let scene = v["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "engine-scene")
            .unwrap()
            .clone();
        assert_eq!(scene["envKeys"], json!(["FORGE_PROJECT_ROOT"]));
        assert!(!text.contains("\"FORGE_PROJECT_ROOT\":\"/p\""), "{text}");
    }
}
