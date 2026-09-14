//! MCP stdio 服务:newline-delimited JSON-RPC(initialize / tools/list / tools/call)。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
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
            {"name":"prefab_instantiate","description":"实例化地图/角色模板，保留层级、材质和来源；整次可撤销", "inputSchema":{"type":"object","required":["prefabRef"],"properties":{"prefabRef":{"type":"string"},"translation":{"type":"array","items":{"type":"number"}},"rotation":{"type":"array","items":{"type":"number"}},"scale":{"type":"array","items":{"type":"number"}}}}},
            {"name":"prefab_revert","description":"恢复模板实例的本地覆盖，保留实例位置", "inputSchema":{"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}},
            {"name":"asset_reload","description":"导入发布成功后刷新模型/纹理版本，并合并模板更新；本地覆盖保留，结构冲突明确返回", "inputSchema":{"type":"object","properties":{"guids":{"type":"array","items":{"type":"string"}},"revision":{"type":"integer"}}}},
            {"name":"animation_control","description":"控制真实3D骨骼动画（播放、暂停、停止、时间采样）", "inputSchema":{"type":"object","required":["id","action"],"properties":{"id":{"type":"integer"},"action":{"type":"string","enum":["play","pause","stop","seek"]},"clip":{"type":"string"},"time":{"type":"number"},"loop":{"type":"boolean"}}}},
            {"name":"template_preview","description":"按包围盒渲染模板真实GPU预览，返回rgba8像素；yaw为弧度", "inputSchema":{"type":"object","required":["prefabRef"],"properties":{"prefabRef":{"type":"string"},"width":{"type":"integer"},"height":{"type":"integer"},"clip":{"type":"string"},"time":{"type":"number"},"yaw":{"type":"number"}}}},
            // ---- F0 既有 ----
            {
                "name": "host_ping",
                "description": "探测 engine-host 存活(pong/version/uptimeSec/backend/pid)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "scene_new",
                "description": "新建空场景(F-GAME-3:mode 选 2d 时编辑器相机自动切正交正视 XY 平面;缺省跟随项目 forge.toml [project] mode)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "场景名(可选)" },
                        "mode": { "type": "string", "enum": ["2d", "3d"], "description": "游戏维度模式(可选;缺省跟随项目 forge.toml)" },
                        "gravity": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] 场景重力(可选,缺省 [0,-9.81,0];2D 俯视/零重力写 [0,0,0])" }
                    }
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
            {
                "name": "host_events_drain",
                "description": "排空 host 内存事件环(component.added/scene.changed 等场景域事件;排空式读取,debug 三件套之一,F3)",
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
                "name": "scene_graph_dump",
                "description": "场景图全量转储(实体 id/name/transform/组件快照,单次调用;debug 三件套之一,F3)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "scene_index",
                "description": "场景分类索引:按 角色(role)/地图(map)/交互(interaction) 分组返回实体 id/name 列表与计数",
                "inputSchema": { "type": "object", "properties": {} }
            },
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
            },
            // ---- logic.*(F4 wave.3)----
            {
                "name": "logic_inject_input",
                "description": "注入输入事件(play 态限定;下一逻辑帧按规范序派发 on_input)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "description": "动作名" },
                        "value": { "type": "number", "description": "动作值" }
                    },
                    "required": ["action", "value"]
                }
            },
            {
                "name": "logic_inject_pointer",
                "description": "注入指针点击(play 态限定):x/y 为归一化视口坐标(0..1,左上原点),经游戏相机反投影到游戏平面(2d 场景 z=0),依次派发 <action>_x/_y/_z(世界坐标)与 <action>(value=1);action 缺省 click",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "number", "description": "归一化 x(0..1,左→右)" },
                        "y": { "type": "number", "description": "归一化 y(0..1,上→下)" },
                        "action": { "type": "string", "description": "动作名(缺省 click)" },
                        "width": { "type": "integer", "description": "坐标所在画面宽(可选,只影响 aspect;缺省推流主订阅尺寸)" },
                        "height": { "type": "integer", "description": "坐标所在画面高(可选)" }
                    },
                    "required": ["x", "y"]
                }
            },
            // ---- viewport.*(F1 wave.2)----
            {
                "name": "viewport_frame",
                "description": "GPU 场景实渲染一帧并回读(rgba8 base64;format=h264 时返回 Annex B 码流;无 vulkan 设备 → DEV_ENV_DEGRADE 工具级错误,不充绿)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "width": { "type": "integer", "description": "帧宽(16..=1920,缺省 960)" },
                        "height": { "type": "integer", "description": "帧高(16..=1080,缺省 540)" },
                        "selectedId": { "type": "integer", "description": "选中实体 id(高亮,可选)" },
                        "format": { "type": "string", "enum": ["rgba8", "h264"], "description": "帧格式(F1 wave.4):rgba8(默认) | h264(Annex B 流腿)" }
                    }
                }
            },
            {
                "name": "viewport_pick",
                "description": "视口点选:屏幕像素坐标(左上原点)→ 相机射线 → 最近命中实体",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "number" },
                        "y": { "type": "number" },
                        "width": { "type": "integer", "description": "视口宽(缺省 960)" },
                        "height": { "type": "integer", "description": "视口高(缺省 540)" }
                    },
                    "required": ["x", "y"]
                }
            },
            {
                "name": "viewport_set_camera",
                "description": "编辑器相机子集更新(target/yaw/pitch/dist/fovY/ortho/orthoSize,未给沿用旧值),回显全量。2D 用法:ortho=true + yaw=0 + pitch=0 得正对 XY 平面视图,缩放调 orthoSize",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] 环绕锚点(2D 下=视野中心)" },
                        "yaw": { "type": "number", "description": "方位角(度)" },
                        "pitch": { "type": "number", "description": "俯仰角(度,钳 ±89)" },
                        "dist": { "type": "number", "description": "距离(钳 0.2..500)" },
                        "fovY": { "type": "number", "description": "垂直视场角(度,钳 10..120)" },
                        "ortho": { "type": "boolean", "description": "F-GAME-3:true=正交(2D),false=透视" },
                        "orthoSize": { "type": "number", "description": "正交半高(世界单位,钳 0.01..1000;2D 缩放即调它)" }
                    }
                }
            },
            {
                "name": "viewport_get_camera",
                "description": "取编辑器相机全量状态(target/yaw/pitch/dist/fovY/ortho/orthoSize)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "viewport_stream_info",
                "description": "取视口直连推流通道信息(wsUrl 含随机 token;浏览器直连 WS 收二进制 RGBA 帧/发实时输入,绕开 MCP 轮询链)",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "sprite_create",
                "description": "2D 精灵一步到位(F-GAME-3):创建实体 + Sprite 组件 + TRS。texture 为贴图 GUID(Content/Textures/*.png 的 .meta guid);scale=1 即素材原生尺寸(贴图像素/pixelsPerUnit 米);2D 坐标约定 XY 平面 z=0",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "texture": { "type": "string", "description": "贴图 GUID" },
                        "position": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z](2D 约定 z=0)" },
                        "scale": { "type": "array", "items": { "type": "number" }, "description": "[x,y,z] 尺寸倍率(缺省 1,1,1 = 原生尺寸)" },
                        "sortingOrder": { "type": "number", "description": "叠放次序(小者先绘,大者压上;缺省 0)" },
                        "pixelsPerUnit": { "type": "number", "description": "每世界单位像素数(缺省 100)" },
                        "tint": { "type": "array", "items": { "type": "number" }, "description": "[r,g,b,a] 0..1 染色(缺省白)" },
                        "flipX": { "type": "boolean", "description": "水平镜像" },
                        "flipY": { "type": "boolean", "description": "垂直镜像" }
                    },
                    "required": ["name", "texture"]
                }
            },
            {
                "name": "viewport_share_open",
                "description": "打开 D3D12 共享纹理并把句柄移交 pid 进程(viewport-presenter);返回 texHandle/fenceHandle/width/height",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pid": { "type": "integer", "description": "目标进程 id(句柄接收方)" },
                        "width": { "type": "integer", "description": "纹理宽(16..=1920,缺省 960)" },
                        "height": { "type": "integer", "description": "纹理高(16..=1080,缺省 540)" }
                    },
                    "required": ["pid"]
                }
            },
            {
                "name": "viewport_share_close",
                "description": "关闭共享纹理(幂等;句柄生命周期结束)",
                "inputSchema": { "type": "object", "properties": {} }
            }
        ]
    })
}

/// 加锁(毒化时取回内部值)。
fn lock(s: &Mutex<Supervisor>) -> MutexGuard<'_, Supervisor> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------- F4 wave.2:Script 组件引用防护 ----------
// 10 §6.3:挂载 component_set 写 Script { graphRef } 或 { module }。graphRef/module 为
// 路径形态(非 GUID);非空时校验 <workspace>/projects/demo/<ref> 文件存在,不存在 →
// 结构化错误 SCRIPT_REF_NOT_FOUND(照 F2 UNKNOWN_GUID 语义)。
// 如实记录:F2 未在 component_set 层做资产引用防护(仅 asset_delete 引用阻断),本校验
// 落 mcp 层(转发 host 前);component_add / entity_batch_apply 内的 Script 不在本波范围。

/// 资产项目根 = <workspace>/projects/demo(照 forge-agentd asset_project_root 先例;
/// CARGO_MANIFEST_DIR = crates/mcp/engine-scene-mcp,上三级 = workspace 根)。
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("CARGO_MANIFEST_DIR 上三级须存在")
        .to_path_buf()
}

fn confine_scene_args(mut args: Value) -> Result<Value, Value> {
    let Some(path) = args.get("path").and_then(Value::as_str).map(str::to_string) else {
        return Ok(args);
    };
    let project = std::env::var("FORGE_PROJECT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| project_root());
    let ws = workspace_root();
    match forge_util::pathutil::confine_under(&[&project, &ws], &path) {
        Ok(p) => {
            args["path"] = json!(p.to_string_lossy());
            Ok(args)
        }
        Err(e) => Ok(tool_wrap(
            &json!({ "error": "PATH_OUTSIDE_ROOT", "message": e }),
            true,
        )),
    }
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("CARGO_MANIFEST_DIR 上三级须存在")
        .join("projects")
        .join("demo")
}

/// 单个 Script props 检查:graphRef/module 非空字符串 → 文件须存在。
fn check_script_props(props: &Value, root: &Path) -> Result<(), String> {
    let Some(obj) = props.as_object() else { return Ok(()) };
    for key in ["graphRef", "module"] {
        if let Some(s) = obj.get(key).and_then(Value::as_str) {
            if !s.is_empty() && !root.join(s).is_file() {
                return Err(format!(
                    "Script.{key} 引用的文件不存在: {s}(项目根 {})",
                    root.display()
                ));
            }
        }
    }
    Ok(())
}

/// component_set(type=Script,props 整体替换)与 entity_create(内联 components)的挂载防护。
fn script_ref_guard(tool: &str, args: &Value) -> Result<(), String> {
    let root = project_root();
    match tool {
        "component_set" => {
            if args.get("type").and_then(Value::as_str) != Some("Script") {
                return Ok(());
            }
            if let Some(props) = args.get("props") {
                check_script_props(props, &root)?;
            }
            Ok(())
        }
        "entity_create" => {
            if let Some(comps) = args.get("components").and_then(Value::as_array) {
                for c in comps {
                    if c.get("type").and_then(Value::as_str) == Some("Script") {
                        if let Some(props) = c.get("props") {
                            check_script_props(props, &root)?;
                        }
                    }
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
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
        "prefab_instantiate"=>"prefab.instantiate",
        "prefab_revert"=>"prefab.revert",
        "asset_reload"=>"asset.reload",
        "animation_control"=>"animation.control",
        "template_preview"=>"template.preview",
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
        "scene_graph_dump" => "scene.graph_dump",
        "scene_index" => "scene.index",
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
        "logic_inject_input" => "logic.inject_input",
        "logic_inject_pointer" => "logic.inject_pointer",
        "viewport_frame" => "viewport.frame",
        "viewport_pick" => "viewport.pick",
        "viewport_set_camera" => "viewport.setCamera",
        "viewport_get_camera" => "viewport.getCamera",
        "viewport_stream_info" => "viewport.streamInfo",
        "viewport_share_open" => "viewport.shareOpen",
        "viewport_share_close" => "viewport.shareClose",
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
            // F-GAME-3:mode/gravity 透传(host 侧做枚举/形状校验)。
            if let Some(m) = args.get("mode") {
                rpc_params["mode"] = m.clone();
            }
            if let Some(g) = args.get("gravity") {
                rpc_params["gravity"] = g.clone();
            }
            Ok(host_tool(sup, "scene.new", rpc_params))
        }
        // F-GAME-3 2D 工效工具:sprite_create = entity.create + Sprite 组件组合调用。
        "sprite_create" => {
            let name = match args.get("name").and_then(Value::as_str) {
                Some(s) if !s.is_empty() => s,
                _ => return Err(err(Value::Null, -32602, "invalid params: 缺 name")),
            };
            let texture = match args.get("texture").and_then(Value::as_str) {
                Some(s) if !s.is_empty() => s,
                _ => {
                    return Err(err(
                        Value::Null,
                        -32602,
                        "invalid params: 缺 texture(贴图 GUID)",
                    ))
                }
            };
            let mut props = json!({ "texture": texture });
            for k in ["tint", "flipX", "flipY", "pixelsPerUnit", "sortingOrder"] {
                if let Some(v) = args.get(k) {
                    props[k] = v.clone();
                }
            }
            let mut create = json!({
                "name": name,
                "components": [ { "type": "Sprite", "props": props } ],
            });
            if let Some(p) = args.get("position") {
                create["translation"] = p.clone();
            }
            if let Some(s) = args.get("scale") {
                create["scale"] = s.clone();
            }
            Ok(host_tool(sup, "entity.create", create))
        }
        "scene_summary" => Ok(host_tool(sup, "scene.summary", json!({}))),
        "render_once" => Ok(host_tool(sup, "render.once", json!({}))),
        "host_events" => {
            let events = lock(sup).read_events_log();
            Ok(tool_wrap(&Value::Array(events), false))
        }
        "host_events_drain" => Ok(host_tool(sup, "events.drain", json!({}))),
        // F4 wave.2:Script 挂载引用防护(转发 host 前的 mcp 层校验)。
        "component_set" | "entity_create" => {
            if let Err(msg) = script_ref_guard(name, &args) {
                return Ok(tool_wrap(
                    &json!({ "error": "SCRIPT_REF_NOT_FOUND", "message": msg }),
                    true,
                ));
            }
            let method = passthrough_method(name).expect("component_set/entity_create 必有映射");
            Ok(host_tool(sup, method, args))
        }
        "scene_save" | "scene_load" | "scene_diff" => {
            let method = passthrough_method(name).expect("scene_* 必有映射");
            match confine_scene_args(args) {
                Ok(args) => Ok(host_tool(sup, method, args)),
                Err(wrapped) => Ok(wrapped),
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forge_script_guard_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("Content/Graphs")).unwrap();
        std::fs::create_dir_all(dir.join("Content/Scripts")).unwrap();
        dir
    }

    #[test]
    fn script_props_ref_existence() {
        let root = tmp_root("props");
        std::fs::write(root.join("Content/Graphs/g.rxgraph"), b"{}\n").unwrap();
        std::fs::write(root.join("Content/Scripts/door.rx"), b"fn main() {}\n").unwrap();
        // 存在的 graphRef / module 通过;空字符串跳过(未挂载该轨)。
        assert!(check_script_props(
            &json!({ "module": "", "graphRef": "Content/Graphs/g.rxgraph", "props": {} }),
            &root
        )
        .is_ok());
        assert!(check_script_props(
            &json!({ "module": "Content/Scripts/door.rx", "graphRef": "", "props": {} }),
            &root
        )
        .is_ok());
        // 不存在 → Err(graphRef / module 均查)。
        let e = check_script_props(
            &json!({ "module": "", "graphRef": "Content/Graphs/ghost.rxgraph", "props": {} }),
            &root,
        )
        .unwrap_err();
        assert!(e.contains("Script.graphRef") && e.contains("ghost.rxgraph"), "{e}");
        let e = check_script_props(
            &json!({ "module": "Content/Scripts/ghost.rx", "graphRef": "", "props": {} }),
            &root,
        )
        .unwrap_err();
        assert!(e.contains("Script.module"), "{e}");
        // 双轨均空 → 通过(占位挂载)。
        assert!(check_script_props(&json!({ "module": "", "graphRef": "", "props": {} }), &root).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn guard_only_intercepts_script() {
        // 非 Script 组件不查(引用路径再假也放行——本层只管 Script)。
        assert!(script_ref_guard(
            "component_set",
            &json!({ "id": 1, "type": "MeshRenderer", "props": { "mesh": "ghost", "material": "ghost" } })
        )
        .is_ok());
        // component_set Script 不带 props(仅改 enabled)不查。
        assert!(script_ref_guard(
            "component_set",
            &json!({ "id": 1, "type": "Script", "enabled": false })
        )
        .is_ok());
        // entity_create 内联 Script + 不存在 graphRef → Err。
        let e = script_ref_guard(
            "entity_create",
            &json!({
                "name": "door",
                "components": [
                    { "type": "Script", "props": { "module": "", "graphRef": "Content/Graphs/definitely_missing.rxgraph", "props": {} } }
                ]
            }),
        )
        .unwrap_err();
        assert!(e.contains("definitely_missing.rxgraph"), "{e}");
        // entity_create 无 components → 通过。
        assert!(script_ref_guard("entity_create", &json!({ "name": "plain" })).is_ok());
    }
}
