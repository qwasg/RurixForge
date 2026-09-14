//! store-mcp 工具面(stdio NDJSON JSON-RPC 2.0)。
//!
//! 十二工具分两类:
//! - **快操作**(同步返结果):`store_sources_list` / `store_search` / `store_info` /
//!   `store_installed_list` / `store_update_check` / `library_list` / `library_add` /
//!   `library_remove` / `library_install`;
//! - **长任务**(异步提交,返 `taskId`):`store_install` / `store_uninstall`,配 `store_task_status` 轮询。
//!
//! 为什么安装要异步:agentd 的 MCP 调用上限是 10s(`mcp.rs CALL_TIMEOUT`),而包下载 +
//! 逐文件 sha256 校验 + assetd 构建链动辄分钟级。同 D-024 对 gen/mesh 的处置逻辑——
//! 前端走 agentd REST 长连接,agent 走「提交拿 id + 轮询」(D-F11-C)。
//!
//! 错误二分(照 context-mcp 惯例):协议级(缺工具名/缺必填参数)走 JSON-RPC error;
//! 业务级返回 `{error: CODE, message}` 由 `serve_stdio` 置 `isError`,不 panic、不静默空返回。
//!
//! 私有源令牌:经 `gend::keystore`(与 LLM/生成渠道同一保险箱),**只用于拼 Authorization 头**,
//! 不进工具返回、不进错误消息、不落 store-sources.json(R-5)。

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use assetd::project::ForgeProject;
use forge_store::install::{
    check_updates, install_package, uninstall_package, InstallOptions, InstalledDb,
};
use forge_store::library::{Library, LibraryItem};
use forge_store::manifest::PackageKind;
use forge_store::registry::{load_sources, search_all, SourcesConfig};
use forge_store::source::{open_source, SearchQuery, SourceConfig};
use forge_store::{Result as StoreResult, StoreError};
use serde_json::{json, Value};

// ---------- JSON-RPC 信封 ----------

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tool_wrap(v: &Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string());
    let mut out = json!({ "content": [{ "type": "text", "text": text }] });
    if is_error {
        out["isError"] = json!(true);
    }
    out
}

/// 业务错误 → 工具返回体(结构与 context-mcp 一致:`{error, message}`)。
fn store_err(e: &StoreError) -> Value {
    json!({ "error": e.code, "message": e.message })
}

// ---------- 长任务表 ----------

#[derive(Debug, Clone)]
struct TaskState {
    id: String,
    kind: &'static str,
    status: &'static str,
    phase: String,
    done: u32,
    total: u32,
    error: Option<(String, String)>,
    result: Option<Value>,
}

impl TaskState {
    fn to_json(&self) -> Value {
        let mut v = json!({
            "taskId": self.id,
            "kind": self.kind,
            "status": self.status,
            "phase": self.phase,
            "done": self.done,
            "total": self.total,
        });
        if let Some((code, msg)) = &self.error {
            v["error"] = json!({ "code": code, "message": msg });
        }
        if let Some(r) = &self.result {
            v["result"] = r.clone();
        }
        v
    }
}

#[derive(Default)]
struct TaskTable {
    inner: Mutex<HashMap<String, TaskState>>,
    seq: AtomicU64,
}

impl TaskTable {
    fn new_task(&self, kind: &'static str) -> String {
        let n = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("stask_{n}");
        let st = TaskState {
            id: id.clone(),
            kind,
            status: "running",
            phase: "queued".into(),
            done: 0,
            total: 0,
            error: None,
            result: None,
        };
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.clone(), st);
        id
    }

    fn progress(&self, id: &str, phase: &str, done: u32, total: u32) {
        if let Some(t) = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(id)
        {
            t.phase = phase.to_string();
            t.done = done;
            t.total = total;
        }
    }

    fn finish(&self, id: &str, outcome: StoreResult<Value>) {
        if let Some(t) = self
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(id)
        {
            match outcome {
                Ok(v) => {
                    t.status = "completed";
                    t.phase = "done".into();
                    t.result = Some(v);
                }
                Err(e) => {
                    t.status = "failed";
                    t.error = Some((e.code.to_string(), e.message));
                }
            }
        }
    }

    fn get(&self, id: &str) -> Option<TaskState> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }
}

/// server 运行期上下文。
struct Ctx {
    project_root: PathBuf,
    workspace_root: PathBuf,
    tasks: Arc<TaskTable>,
}

impl Ctx {
    fn data_dir(&self) -> PathBuf {
        self.workspace_root.join("data")
    }

    fn skills_root(&self) -> PathBuf {
        self.workspace_root.join("skills")
    }

    fn sources(&self) -> SourcesConfig {
        load_sources(&self.data_dir())
    }

    fn project(&self) -> ForgeProject {
        ForgeProject::load(&self.project_root).unwrap_or_else(|e| {
            eprintln!("[store-mcp] forge.toml 加载失败: {e};用缺省配置");
            ForgeProject::with_defaults(self.project_root.clone())
        })
    }
}

/// 私有源令牌:keystore 内 `store:<sourceId>` 槽位(R-5:取出即用,不外泄)。
fn token_of(source_id: &str) -> Option<String> {
    let ks = gend_keystore();
    ks.and_then(|k| k(source_id))
}

/// keystore 取值的间接层。`gend` 是可选依赖面——本 crate 不直接引 gend 以免拖进
/// image/base64 等重依赖,私有源令牌改由环境变量 `FORGE_STORE_TOKEN_<ID>` 提供。
/// 差异留痕(D-F11 实现期):契约里说 token 走 keystore,agentd REST 面确实经 keystore;
/// MCP 子进程这条腿用 env 注入,由 agentd 在 spawn 时传递,避免两个进程争抢 keystore 文件锁。
fn gend_keystore() -> Option<fn(&str) -> Option<String>> {
    fn lookup(source_id: &str) -> Option<String> {
        let key = format!(
            "FORGE_STORE_TOKEN_{}",
            source_id.to_uppercase().replace(['-', '.'], "_")
        );
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }
    Some(lookup)
}

// ---------- 工具声明 ----------

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "store_sources_list",
                "description": "列出已配置的商店源(id/名称/baseUrl/启停;不回显令牌)。返回 {sources:[...]}",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "store_search",
                "description": "跨全部已启用源搜索资产包与技能包。返回 {total, items:[{sourceId, package}], errors:[{sourceId, code, message}]}——某源不可达时进 errors 如实上报,不与「无结果」混为一谈",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "关键词(空串 = 列全部)" },
                        "kind": { "type": "string", "enum": ["asset-pack", "skill"], "description": "限定包类型(缺省全部)" },
                        "sourceId": { "type": "string", "description": "限定单一源(缺省全部已启用源)" },
                        "page": { "type": "integer", "description": "页码(从 1 起,缺省 1)" },
                        "pageSize": { "type": "integer", "description": "每页条数(缺省 20,上限 200)" }
                    }
                }
            },
            {
                "name": "store_info",
                "description": "取包详情与版本列表;给了 version 则一并返回该版本完整清单(含 files[] 与逐文件 sha256)。返回 {summary, versions, manifest?}",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sourceId": { "type": "string" },
                        "packageId": { "type": "string" },
                        "version": { "type": "string", "description": "可选;给了则返回该版本清单" }
                    },
                    "required": ["sourceId", "packageId"]
                }
            },
            {
                "name": "store_installed_list",
                "description": "列出当前项目已安装的商店包(来源/版本/落地资产路径/技能名/安装时间)。返回 {installed:[...]}",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "store_update_check",
                "description": "逐条已装记录查源上最新版本。返回 {updates:[{sourceId, packageId, current, latest, hasUpdate, error?}]}——查询失败如实回填 error,不伪装成「已是最新」",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "store_install",
                "description": "安装包到当前项目(**异步提交**:立即返回 {taskId},用 store_task_status 轮询)。资产走与人工导入完全相同的 asset_import 构建链,.meta.provenance.origin=store-install;逐文件 sha256 校验不符即中止且不留残留;付费包显式拒绝",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sourceId": { "type": "string" },
                        "packageId": { "type": "string" },
                        "version": { "type": "string", "description": "缺省 = 源上最新版本" },
                        "destFolder": { "type": "string", "description": "asset-pack 落 Content/<destFolder>;缺省按扩展名分流(Meshes/Textures/Materials/Scenes/Scripts)" },
                        "force": { "type": "boolean", "description": "已装同版本时是否重装(缺省 false)" }
                    },
                    "required": ["sourceId", "packageId"]
                }
            },
            {
                "name": "store_uninstall",
                "description": "卸载已装包(**异步提交**,返 {taskId})。destructive:被其他资产引用的条目如实进 blockedByRefs 不强删;force=true 需上层 Proposal 已批准",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sourceId": { "type": "string" },
                        "packageId": { "type": "string" },
                        "force": { "type": "boolean", "description": "跳过引用阻断(须 approved Proposal;I-6)" }
                    },
                    "required": ["sourceId", "packageId"]
                }
            },
            {
                "name": "store_task_status",
                "description": "查长任务进度。返回 {taskId, status: running|completed|failed, phase, done, total, error?, result?}",
                "inputSchema": {
                    "type": "object",
                    "properties": { "taskId": { "type": "string" } },
                    "required": ["taskId"]
                }
            },
            {
                "name": "library_list",
                "description": "列出个人资产库(跨项目的内容寻址收藏层,sha256 去重)。返回 {items:[...]}",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "library_search",
                "description": "检索个人资产库(名称/标签/来源/类型子串匹配)。返回 {total, items:[...], nextCursor?}——cursor 为下一页起始下标的十进制串",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "关键词(空 = 列全部)" },
                        "kind": { "type": "string", "description": "限定 kind(mesh/texture/material/misc)" },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "cursor": { "type": "string", "description": "分页游标(上一页 nextCursor)" },
                        "limit": { "type": "integer", "description": "每页条数(缺省 20,上限 100)" }
                    }
                }
            },
            {
                "name": "library_add",
                "description": "把当前项目的一个资产收藏进个人库(按内容寻址,同内容不重复占空间)。返回 {item}",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "assetPath": { "type": "string", "description": "相对 Content/ 的资产路径" },
                        "tags": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["assetPath"]
                }
            },
            {
                "name": "library_remove",
                "description": "从个人库移出一条(blob 在无其他条目引用时才真删)。返回 {removed:true}",
                "inputSchema": {
                    "type": "object",
                    "properties": { "id": { "type": "string" } },
                    "required": ["id"]
                }
            },
            {
                "name": "library_install",
                "description": "把个人库中的一条装进当前项目(走 asset_import 同链)。返回 {assetPath, guid}",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "destFolder": { "type": "string", "description": "缺省按扩展名分流" }
                    },
                    "required": ["id"]
                }
            }
        ]
    })
}

// ---------- 参数辅助 ----------

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn arg_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|v| v.min(u32::MAX as u64) as u32)
}

fn arg_tags(args: &Value) -> Vec<String> {
    args.get("tags")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn library_search(
    lib: &Library,
    query: &str,
    kind: Option<&str>,
    tags: &[String],
    cursor: Option<&str>,
    limit: u32,
) -> Value {
    let q = query.trim().to_ascii_lowercase();
    let kind = kind.map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty());
    let mut items: Vec<LibraryItem> = lib
        .list()
        .into_iter()
        .filter(|it| {
            if let Some(k) = kind.as_deref() {
                if !it.kind.eq_ignore_ascii_case(k) {
                    return false;
                }
            }
            if !tags.is_empty() && !tags.iter().all(|t| it.tags.iter().any(|x| x.eq_ignore_ascii_case(t))) {
                return false;
            }
            if q.is_empty() {
                return true;
            }
            it.name.to_ascii_lowercase().contains(&q)
                || it.kind.to_ascii_lowercase().contains(&q)
                || it.source.to_ascii_lowercase().contains(&q)
                || it.tags.iter().any(|t| t.to_ascii_lowercase().contains(&q))
        })
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    let start = cursor
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(0)
        .min(items.len());
    let page = (limit as usize).clamp(1, 100);
    let end = (start + page).min(items.len());
    let page_items = &items[start..end];
    let next = if end < items.len() {
        Some(end.to_string())
    } else {
        None
    };
    json!({
        "total": items.len(),
        "items": page_items,
        "nextCursor": next,
    })
}

fn find_source(cfg: &SourcesConfig, id: &str) -> StoreResult<SourceConfig> {
    cfg.find(id)
        .cloned()
        .map(|s| SourceConfig { token: token_of(id), ..s })
        .ok_or_else(|| {
            StoreError::new(
                forge_store::STORE_SOURCE_NOT_FOUND,
                format!("未配置源: {id}"),
            )
        })
}

// ---------- 工具分派 ----------

fn call_tool(ctx: &Ctx, params: &Value) -> std::result::Result<Value, Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺工具名"))?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return Err(err(Value::Null, -32602, "invalid params: arguments 须为对象"));
    }

    match name {
        "store_sources_list" => {
            let cfg = ctx.sources();
            let items: Vec<Value> = cfg
                .sources
                .iter()
                .map(|s| {
                    json!({
                        "id": s.id,
                        "name": s.name,
                        "baseUrl": s.base_url,
                        "enabled": s.enabled,
                        // 只报「有没有」,不报值(R-5)。
                        "hasToken": token_of(&s.id).is_some(),
                    })
                })
                .collect();
            Ok(json!({ "sources": items }))
        }

        "store_search" => {
            let mut q = SearchQuery::new(&arg_str(&args, "query").unwrap_or_default());
            q.kind = match arg_str(&args, "kind").as_deref() {
                Some("asset-pack") => Some(PackageKind::AssetPack),
                Some("skill") => Some(PackageKind::Skill),
                _ => None,
            };
            if let Some(p) = arg_u32(&args, "page") {
                q.page = p;
            }
            if let Some(p) = arg_u32(&args, "pageSize") {
                q.page_size = p;
            }
            let mut cfg = ctx.sources();
            if let Some(only) = arg_str(&args, "sourceId") {
                cfg.sources.retain(|s| s.id == only);
                if cfg.sources.is_empty() {
                    return Ok(store_err(&StoreError::new(
                        forge_store::STORE_SOURCE_NOT_FOUND,
                        format!("未配置源: {only}"),
                    )));
                }
            }
            let agg = search_all(&cfg, &q, &token_of);
            let items: Vec<Value> = agg
                .items
                .iter()
                .map(|(sid, pkg)| {
                    json!({
                        "sourceId": sid,
                        "package": serde_json::to_value(pkg).unwrap_or(Value::Null),
                    })
                })
                .collect();
            let errors: Vec<Value> = agg
                .errors
                .iter()
                .map(|(sid, e)| json!({ "sourceId": sid, "code": e.code, "message": e.message }))
                .collect();
            Ok(json!({
                "total": items.len(),
                "partial": agg.is_partial(),
                "items": items,
                "errors": errors,
            }))
        }

        "store_info" => {
            let source_id = arg_str(&args, "sourceId")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 sourceId"))?;
            let pkg_id = arg_str(&args, "packageId")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 packageId"))?;
            let cfg = ctx.sources();
            let scfg = match find_source(&cfg, &source_id) {
                Ok(s) => s,
                Err(e) => return Ok(store_err(&e)),
            };
            let src = match open_source(&scfg) {
                Ok(s) => s,
                Err(e) => return Ok(store_err(&e)),
            };
            let detail = match src.detail(&pkg_id) {
                Ok(d) => d,
                Err(e) => return Ok(store_err(&e)),
            };
            let mut out = json!({
                "sourceId": source_id,
                "summary": serde_json::to_value(&detail.summary).unwrap_or(Value::Null),
                "versions": detail.versions,
            });
            let want = arg_str(&args, "version");
            if let Some(v) = want {
                match src.manifest(&pkg_id, &v) {
                    Ok(m) => out["manifest"] = serde_json::to_value(&m).unwrap_or(Value::Null),
                    Err(e) => return Ok(store_err(&e)),
                }
            }
            Ok(out)
        }

        "store_installed_list" => {
            let project = ctx.project();
            match InstalledDb::load(&project) {
                Ok(db) => Ok(json!({
                    "installed": serde_json::to_value(db.list()).unwrap_or(Value::Null),
                })),
                Err(e) => Ok(store_err(&e)),
            }
        }

        "store_update_check" => {
            let project = ctx.project();
            let db = match InstalledDb::load(&project) {
                Ok(d) => d,
                Err(e) => return Ok(store_err(&e)),
            };
            let updates = check_updates(&db, &ctx.sources(), &token_of);
            Ok(json!({ "updates": serde_json::to_value(&updates).unwrap_or(Value::Null) }))
        }

        "store_install" => {
            let source_id = arg_str(&args, "sourceId")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 sourceId"))?;
            let pkg_id = arg_str(&args, "packageId")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 packageId"))?;
            let version = arg_str(&args, "version");
            let dest = arg_str(&args, "destFolder");
            let force = arg_bool(&args, "force");
            let task_id = ctx.tasks.new_task("install");
            let tasks = Arc::clone(&ctx.tasks);
            let project_root = ctx.project_root.clone();
            let skills_root = ctx.skills_root();
            let data_dir = ctx.data_dir();
            let tid = task_id.clone();
            // 后台线程跑:MCP 响应立即返回 taskId,不占 agentd 的 10s 预算(D-F11-C)。
            std::thread::spawn(move || {
                let outcome = run_install(
                    &project_root,
                    &data_dir,
                    &skills_root,
                    &source_id,
                    &pkg_id,
                    version.as_deref(),
                    dest.as_deref(),
                    force,
                    &tasks,
                    &tid,
                );
                tasks.finish(&tid, outcome);
            });
            Ok(json!({ "taskId": task_id, "status": "running" }))
        }

        "store_uninstall" => {
            let source_id = arg_str(&args, "sourceId")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 sourceId"))?;
            let pkg_id = arg_str(&args, "packageId")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 packageId"))?;
            let force = arg_bool(&args, "force");
            let task_id = ctx.tasks.new_task("uninstall");
            let tasks = Arc::clone(&ctx.tasks);
            let project_root = ctx.project_root.clone();
            let skills_root = ctx.skills_root();
            let tid = task_id.clone();
            std::thread::spawn(move || {
                let outcome = run_uninstall(
                    &project_root,
                    &skills_root,
                    &source_id,
                    &pkg_id,
                    force,
                    &tasks,
                    &tid,
                );
                tasks.finish(&tid, outcome);
            });
            Ok(json!({ "taskId": task_id, "status": "running" }))
        }

        "store_task_status" => {
            let id = arg_str(&args, "taskId")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 taskId"))?;
            match ctx.tasks.get(&id) {
                Some(t) => Ok(t.to_json()),
                None => Ok(store_err(&StoreError::new(
                    forge_store::STORE_TASK_NOT_FOUND,
                    format!("无此任务: {id}(任务表在进程内,agentd 重启即空)"),
                ))),
            }
        }

        "library_list" => match Library::open(&ctx.data_dir()) {
            Ok(lib) => Ok(json!({
                "items": serde_json::to_value(lib.list()).unwrap_or(Value::Null),
            })),
            Err(e) => Ok(store_err(&e)),
        },

        "library_search" => match Library::open(&ctx.data_dir()) {
            Ok(lib) => Ok(library_search(
                &lib,
                &arg_str(&args, "query").unwrap_or_default(),
                arg_str(&args, "kind").as_deref(),
                &arg_tags(&args),
                arg_str(&args, "cursor").as_deref(),
                arg_u32(&args, "limit").unwrap_or(20),
            )),
            Err(e) => Ok(store_err(&e)),
        },

        "library_add" => {
            let asset_path = arg_str(&args, "assetPath")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 assetPath"))?;
            let tags = arg_tags(&args);
            match library_add(&ctx.project(), &ctx.data_dir(), &asset_path, &tags) {
                Ok(item) => Ok(json!({ "item": serde_json::to_value(&item).unwrap_or(Value::Null) })),
                Err(e) => Ok(store_err(&e)),
            }
        }

        "library_remove" => {
            let id = arg_str(&args, "id")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 id"))?;
            let mut lib = match Library::open(&ctx.data_dir()) {
                Ok(l) => l,
                Err(e) => return Ok(store_err(&e)),
            };
            match lib.remove(&id) {
                Ok(()) => Ok(json!({ "removed": true, "id": id })),
                Err(e) => Ok(store_err(&e)),
            }
        }

        "library_install" => {
            let id = arg_str(&args, "id")
                .ok_or_else(|| err(Value::Null, -32602, "invalid params: 缺 id"))?;
            let dest = arg_str(&args, "destFolder");
            match library_install(&ctx.project(), &ctx.data_dir(), &id, dest.as_deref()) {
                Ok(v) => Ok(v),
                Err(e) => Ok(store_err(&e)),
            }
        }

        _ => Err(err(Value::Null, -32602, &format!("未知工具:{name}"))),
    }
}

// ---------- 长任务实现 ----------

#[allow(clippy::too_many_arguments)]
fn run_install(
    project_root: &Path,
    data_dir: &Path,
    skills_root: &Path,
    source_id: &str,
    pkg_id: &str,
    version: Option<&str>,
    dest: Option<&str>,
    force: bool,
    tasks: &TaskTable,
    task_id: &str,
) -> StoreResult<Value> {
    let cfg = load_sources(data_dir);
    let scfg = find_source(&cfg, source_id)?;
    let source_name = scfg.name.clone();
    let src = open_source(&scfg)?;
    // 未指定版本 = 源上最新(detail.versions 已按版本序排好,末位最新)。
    let ver = match version {
        Some(v) => v.to_string(),
        None => {
            let d = src.detail(pkg_id)?;
            d.versions
                .last()
                .cloned()
                .unwrap_or(d.summary.latest_version)
        }
    };
    let manifest = src.manifest(pkg_id, &ver)?;
    let project = ForgeProject::load(project_root)
        .unwrap_or_else(|_| ForgeProject::with_defaults(project_root.to_path_buf()));

    let tid = task_id.to_string();
    let progress = move |phase: &str, done: u32, total: u32| {
        tasks.progress(&tid, phase, done, total);
    };
    let opts = InstallOptions {
        dest_folder: dest,
        skills_root,
        force,
        progress: Some(&progress),
        source_name: Some(&source_name),
    };
    let rec = install_package(&project, src.as_ref(), source_id, &manifest, &opts)?;
    serde_json::to_value(&rec).map_err(|e| StoreError::new("SERIALIZE", e.to_string()))
}

fn run_uninstall(
    project_root: &Path,
    skills_root: &Path,
    source_id: &str,
    pkg_id: &str,
    force: bool,
    tasks: &TaskTable,
    task_id: &str,
) -> StoreResult<Value> {
    tasks.progress(task_id, "resolve", 0, 1);
    let project = ForgeProject::load(project_root)
        .unwrap_or_else(|_| ForgeProject::with_defaults(project_root.to_path_buf()));
    let mut db = InstalledDb::load(&project)?;
    tasks.progress(task_id, "remove", 0, 1);
    let outcome = uninstall_package(&project, &mut db, source_id, pkg_id, skills_root, force)?;
    tasks.progress(task_id, "record", 1, 1);
    let blocked: Vec<Value> = outcome
        .blocked_by_refs
        .iter()
        .map(|(p, refs)| json!({ "assetPath": p, "referencedBy": refs }))
        .collect();
    Ok(json!({
        "removedAssets": outcome.removed_assets,
        "removedSkills": outcome.removed_skills,
        "blockedByRefs": blocked,
    }))
}

// ---------- 个人库桥接 ----------

fn library_add(
    project: &ForgeProject,
    data_dir: &Path,
    asset_path: &str,
    tags: &[String],
) -> StoreResult<forge_store::library::LibraryItem> {
    let rel = forge_store::manifest::safe_rel_path(asset_path)?;
    let abs = project.content_root().join(&rel);
    if !abs.is_file() {
        return Err(StoreError::new(
            forge_store::STORE_PACKAGE_NOT_FOUND,
            format!("项目内无此资产: {rel}"),
        ));
    }
    let name = Path::new(&rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("asset")
        .to_string();
    let mut lib = Library::open(data_dir)?;
    let item = lib.add_file(&name, &abs, "project", tags)?;
    lib.save()?;
    Ok(item)
}

fn library_install(
    project: &ForgeProject,
    data_dir: &Path,
    id: &str,
    dest: Option<&str>,
) -> StoreResult<Value> {
    let lib = Library::open(data_dir)?;
    let item = lib.get(id).ok_or_else(|| {
        StoreError::new(
            forge_store::STORE_NOT_INSTALLED,
            format!("资产库无此条目: {id}"),
        )
    })?;
    let blob = lib.blob_path(&item.sha256);
    if !blob.is_file() {
        return Err(StoreError::new(
            forge_store::STORE_PACKAGE_NOT_FOUND,
            format!("资产库 blob 缺失(索引与内容不一致): {}", item.sha256),
        ));
    }
    // 与商店安装同路:staging 命名副本 → import_assets(不自己拷进 Content/)。
    let staging = project.root.join(".forge/tmp/store/library");
    std::fs::create_dir_all(&staging)?;
    let file_name = if item.ext.is_empty() {
        item.name.clone()
    } else {
        format!("{}.{}", item.name, item.ext)
    };
    let staged = staging.join(&file_name);
    std::fs::copy(&blob, &staged)?;
    let dest_folder = dest
        .map(str::to_string)
        .unwrap_or_else(|| forge_store::install::default_dest_folder(&item.ext).to_string());
    let staged_rel = format!(".forge/tmp/store/library/{file_name}");
    let outcome = assetd::import::import_assets(project, &[staged_rel], &dest_folder, None)?;
    std::fs::remove_file(&staged).ok();
    if let Some(f) = outcome.failed.first() {
        return Err(StoreError::new(
            forge_store::STORE_MANIFEST_INVALID,
            format!("入管线失败({}): {}", f.source, f.error),
        ));
    }
    let one = outcome.imported.into_iter().next().ok_or_else(|| {
        StoreError::new(forge_store::STORE_MANIFEST_INVALID, "入管线无结果")
    })?;
    Ok(json!({ "assetPath": one.asset_path, "guid": one.guid }))
}

// ---------- stdio 主循环 ----------

pub fn serve_stdio(project_root: PathBuf, workspace_root: PathBuf) {
    let ctx = Ctx {
        project_root,
        workspace_root,
        tasks: Arc::new(TaskTable::default()),
    };
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
                        "serverInfo": { "name": "store-mcp", "version": env!("CARGO_PKG_VERSION") }
                    }),
                )
            }),
            "notifications/initialized" | "notifications/cancelled" => None,
            "ping" => id.map(|i| ok(i, json!({}))),
            "tools/list" => id.map(|i| ok(i, tool_list())),
            "tools/call" => id.map(|i| {
                let params = req.get("params").cloned().unwrap_or(Value::Null);
                match call_tool(&ctx, &params) {
                    Ok(result) => {
                        let is_error = result.get("error").is_some();
                        ok(i, tool_wrap(&result, is_error))
                    }
                    Err(mut e) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_for(dir: &Path) -> Ctx {
        Ctx {
            project_root: dir.join("proj"),
            workspace_root: dir.to_path_buf(),
            tasks: Arc::new(TaskTable::default()),
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "store-mcp-{tag}-{}-{}",
            std::process::id(),
            forge_util::timeutil::unix_millis()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tool_list_declares_thirteen_tools_with_schema() {
        let v = tool_list();
        let tools = v["tools"].as_array().expect("tools 应为数组");
        assert_eq!(tools.len(), 13, "工具数须与 agentd KNOWN_TOOLS 登记一致");
        for t in tools {
            let name = t["name"].as_str().expect("每个工具须有 name");
            assert!(!name.is_empty());
            assert!(
                t["description"].as_str().map(|d| d.len() > 10).unwrap_or(false),
                "{name} 的 description 太短"
            );
            assert_eq!(t["inputSchema"]["type"], "object", "{name} schema 须为 object");
        }
    }

    #[test]
    fn unknown_tool_is_protocol_error() {
        let dir = tmp("unknown");
        let ctx = ctx_for(&dir);
        let e = call_tool(&ctx, &json!({ "name": "no_such_tool" })).unwrap_err();
        assert_eq!(e["error"]["code"], -32602);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_required_arg_is_protocol_error() {
        let dir = tmp("missing-arg");
        let ctx = ctx_for(&dir);
        let e = call_tool(&ctx, &json!({ "name": "store_info", "arguments": {} })).unwrap_err();
        assert_eq!(e["error"]["code"], -32602);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_source_is_business_error_not_panic() {
        let dir = tmp("bad-source");
        let ctx = ctx_for(&dir);
        let v = call_tool(
            &ctx,
            &json!({ "name": "store_info", "arguments": { "sourceId": "nope", "packageId": "x" } }),
        )
        .unwrap();
        assert_eq!(v["error"], "STORE_SOURCE_NOT_FOUND");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn task_status_of_unknown_id_reports_not_found() {
        let dir = tmp("task");
        let ctx = ctx_for(&dir);
        let v = call_tool(
            &ctx,
            &json!({ "name": "store_task_status", "arguments": { "taskId": "stask_999" } }),
        )
        .unwrap();
        assert_eq!(v["error"], "STORE_TASK_NOT_FOUND");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn task_table_lifecycle() {
        let t = TaskTable::default();
        let id = t.new_task("install");
        assert_eq!(t.get(&id).unwrap().status, "running");
        t.progress(&id, "download", 2, 5);
        let s = t.get(&id).unwrap();
        assert_eq!(s.phase, "download");
        assert_eq!((s.done, s.total), (2, 5));
        t.finish(&id, Ok(json!({ "ok": true })));
        let s = t.get(&id).unwrap();
        assert_eq!(s.status, "completed");
        assert_eq!(s.to_json()["result"]["ok"], true);

        let id2 = t.new_task("uninstall");
        t.finish(
            &id2,
            Err(StoreError::new(forge_store::STORE_NOT_INSTALLED, "未装")),
        );
        let s2 = t.get(&id2).unwrap();
        assert_eq!(s2.status, "failed");
        assert_eq!(s2.to_json()["error"]["code"], "STORE_NOT_INSTALLED");
    }

    #[test]
    fn sources_list_never_leaks_token_value() {
        // R-5:令牌只报「有无」。设 env 后 hasToken=true,但响应体里不得出现值本身。
        let dir = tmp("token");
        std::env::set_var("FORGE_STORE_TOKEN_OFFICIAL", "super-secret-value");
        let ctx = ctx_for(&dir);
        let v = call_tool(&ctx, &json!({ "name": "store_sources_list", "arguments": {} })).unwrap();
        let text = serde_json::to_string(&v).unwrap();
        assert!(!text.contains("super-secret-value"), "令牌值泄漏: {text}");
        std::env::remove_var("FORGE_STORE_TOKEN_OFFICIAL");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn library_search_filters_and_pages() {
        let dir = tmp("lib-search");
        std::fs::create_dir_all(dir.join("data").join("store").join("library")).unwrap();
        let mut lib = Library::open(&dir.join("data")).unwrap();
        lib.add_bytes("wood-albedo", "png", b"tex-a", "project", &["wood".into()])
            .unwrap();
        lib.add_bytes("metal-plate", "png", b"tex-b", "import", &["metal".into()])
            .unwrap();
        lib.add_bytes("hero-mesh", "gltf", b"mesh-c", "project", &["hero".into()])
            .unwrap();
        let v = library_search(&lib, "wood", Some("texture"), &[], None, 10);
        assert_eq!(v["total"], 1);
        assert_eq!(v["items"][0]["name"], "wood-albedo");
        let page = library_search(&lib, "", None, &[], None, 2);
        assert_eq!(page["total"], 3);
        assert_eq!(page["items"].as_array().unwrap().len(), 2);
        assert_eq!(page["nextCursor"], "2");
        std::fs::remove_dir_all(&dir).ok();
    }
}
