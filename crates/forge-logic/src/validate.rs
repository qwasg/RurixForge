//! 全图校验器(10 §5:保存即全图校验——悬空输入 / 类型不匹配 / 环检测=执行边禁环+数据边禁环)。
//! 纯函数,错误带结构化 code(GRAPH_*),nodeId 可空(图级错误)。

use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::Value;

use crate::graph::{GraphDoc, PropKind, ValueSource};
use crate::registry::{find_spec, NodeKind, PinType};

/// 校验错误(序列化键:code / message / nodeId?)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphError {
    pub code: &'static str,
    pub message: String,
    #[serde(rename = "nodeId", skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

impl GraphError {
    fn at(code: &'static str, node_id: &str, message: impl Into<String>) -> Self {
        GraphError { code, message: message.into(), node_id: Some(node_id.to_string()) }
    }
    fn global(code: &'static str, message: impl Into<String>) -> Self {
        GraphError { code, message: message.into(), node_id: None }
    }
}

pub const GRAPH_SCHEMA: &str = "GRAPH_SCHEMA";
pub const GRAPH_UNKNOWN_NODE: &str = "GRAPH_UNKNOWN_NODE";
pub const GRAPH_DUP_EVENT: &str = "GRAPH_DUP_EVENT";
pub const GRAPH_DANGLING_INPUT: &str = "GRAPH_DANGLING_INPUT";
pub const GRAPH_BAD_SOURCE: &str = "GRAPH_BAD_SOURCE";
pub const GRAPH_TYPE_MISMATCH: &str = "GRAPH_TYPE_MISMATCH";
pub const GRAPH_EXEC_CYCLE: &str = "GRAPH_EXEC_CYCLE";
pub const GRAPH_DATA_CYCLE: &str = "GRAPH_DATA_CYCLE";
// RD-F4-004 第九校验臂(call_function 互绑,10 §4.3)。
pub const GRAPH_CALL_MODULE_NOT_FOUND: &str = "GRAPH_CALL_MODULE_NOT_FOUND";
pub const GRAPH_CALL_FN_NOT_EXPORTED: &str = "GRAPH_CALL_FN_NOT_EXPORTED";
pub const GRAPH_CALL_SIG_MISMATCH: &str = "GRAPH_CALL_SIG_MISMATCH";

/// const 字面量与 pin 类型匹配:number → F32/I32 皆可(integer 字面量进 F32 允许);
/// string → String / Entity($self/$parent 及实体名皆为字符串形态);bool → Bool;
/// [f64;3] → Vec3;object → Transform;Any 兼容任意。
fn const_matches(v: &Value, ty: PinType) -> bool {
    match ty {
        PinType::Any => true,
        PinType::Bool => v.is_boolean(),
        PinType::I32 | PinType::F32 => v.is_number(),
        PinType::String | PinType::Entity => v.is_string(),
        PinType::Vec3 => v
            .as_array()
            .is_some_and(|a| a.len() == 3 && a.iter().all(Value::is_number)),
        PinType::Transform => v.is_object(),
        PinType::Exec => false,
    }
}

/// 数据边两端 pin 类型匹配:相等或任一侧 Any。
fn pin_compatible(out_ty: PinType, in_ty: PinType) -> bool {
    out_ty == in_ty || out_ty == PinType::Any || in_ty == PinType::Any
}

/// 暴露属性 kind → pin 类型。
fn kind_pin(k: PropKind) -> PinType {
    match k {
        PropKind::F32 => PinType::F32,
        PropKind::I32 => PinType::I32,
        PropKind::Bool => PinType::Bool,
        PropKind::String => PinType::String,
        PropKind::Vec3 => PinType::Vec3,
    }
}

/// 全图校验:返回全部错误(空 = 通过)。未知类型节点跳过后续逐 pin 检查(不级联刷屏)。
pub fn validate_graph(doc: &GraphDoc) -> Vec<GraphError> {
    let mut errs = Vec::new();

    // ---- 1. schema ----
    if doc.version != 1 {
        errs.push(GraphError::global(GRAPH_SCHEMA, format!("version 须为 1,实际 {}", doc.version)));
    }
    if doc.id.is_empty() {
        errs.push(GraphError::global(GRAPH_SCHEMA, "id 须非空"));
    }
    if doc.name.is_empty() {
        errs.push(GraphError::global(GRAPH_SCHEMA, "name 须非空"));
    }
    let mut seen_ids: HashSet<&str> = HashSet::new();
    for n in &doc.nodes {
        if !seen_ids.insert(n.id.as_str()) {
            errs.push(GraphError::at(GRAPH_SCHEMA, &n.id, format!("节点 id 重复: {}", n.id)));
        }
    }
    let mut seen_props: HashSet<&str> = HashSet::new();
    for p in &doc.exposed_props {
        if !seen_props.insert(p.name.as_str()) {
            errs.push(GraphError::global(GRAPH_SCHEMA, format!("exposedProps name 重复: {}", p.name)));
        }
        let ty = kind_pin(p.kind);
        if !const_matches(&p.default, ty) {
            errs.push(GraphError::global(
                GRAPH_TYPE_MISMATCH,
                format!("exposedProps.{} default 类型须为 {},实际 {}", p.name, ty.name(), p.default),
            ));
        }
    }

    // 节点 id → 序号(首现;重复 id 已在上面报错)。
    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, n) in doc.nodes.iter().enumerate() {
        index.entry(n.id.as_str()).or_insert(i);
    }

    // ---- 2. 节点 type 在注册表 ----
    for n in &doc.nodes {
        if find_spec(&n.ntype).is_none() {
            errs.push(GraphError::at(GRAPH_UNKNOWN_NODE, &n.id, format!("未知节点类型: {}", n.ntype)));
        }
    }

    // ---- 3. 每图每事件至多一个入口 ----
    let mut event_count: HashMap<&str, usize> = HashMap::new();
    for n in &doc.nodes {
        if let Some(spec) = find_spec(&n.ntype) {
            if spec.kind == NodeKind::Event {
                let c = event_count.entry(n.ntype.as_str()).or_insert(0);
                *c += 1;
                if *c == 2 {
                    errs.push(GraphError::at(
                        GRAPH_DUP_EVENT,
                        &n.id,
                        format!("事件 {} 每图至多一个入口(10 §4.2)", n.ntype),
                    ));
                }
            }
        }
    }

    // ---- 4/5. 逐节点输入:悬空 / 值来源合法 / 类型匹配 ----
    for n in &doc.nodes {
        let Some(spec) = find_spec(&n.ntype) else { continue };
        for pin in spec.inputs {
            if pin.required && !n.inputs.contains_key(pin.name) {
                errs.push(GraphError::at(
                    GRAPH_DANGLING_INPUT,
                    &n.id,
                    format!("{}({})必填输入 {} 未接", n.ntype, n.id, pin.name),
                ));
            }
        }
        for (pin_name, src) in &n.inputs {
            let Some(pin) = spec.inputs.iter().find(|p| p.name == pin_name) else {
                errs.push(GraphError::at(
                    GRAPH_BAD_SOURCE,
                    &n.id,
                    format!("{} 无输入 pin {pin_name}", n.ntype),
                ));
                continue;
            };
            match src {
                ValueSource::Const { konst } => {
                    if !const_matches(konst, pin.ty) {
                        errs.push(GraphError::at(
                            GRAPH_TYPE_MISMATCH,
                            &n.id,
                            format!("{}.{} const 值类型须为 {},实际 {konst}", n.ntype, pin_name, pin.ty.name()),
                        ));
                    }
                }
                ValueSource::NodePin { node, pin: out_pin } => {
                    let Some(&si) = index.get(node.as_str()) else {
                        errs.push(GraphError::at(
                            GRAPH_BAD_SOURCE,
                            &n.id,
                            format!("{}.{} 引用不存在节点 {node}", n.ntype, pin_name),
                        ));
                        continue;
                    };
                    let src_node = &doc.nodes[si];
                    let Some(src_spec) = find_spec(&src_node.ntype) else { continue };
                    let Some(opin) = src_spec.outputs.iter().find(|p| p.name == out_pin) else {
                        errs.push(GraphError::at(
                            GRAPH_BAD_SOURCE,
                            &n.id,
                            format!("{}.{} 引用 {} 不存在的输出 pin {out_pin}", n.ntype, pin_name, src_node.ntype),
                        ));
                        continue;
                    };
                    if !pin_compatible(opin.ty, pin.ty) {
                        errs.push(GraphError::at(
                            GRAPH_TYPE_MISMATCH,
                            &n.id,
                            format!(
                                "{}.{}({}) ← {}.{}({}) 类型不匹配",
                                n.ntype, pin_name, pin.ty.name(), src_node.ntype, out_pin, opin.ty.name()
                            ),
                        ));
                    }
                }
                ValueSource::Ref { refr } => {
                    let Some(prop) = doc.exposed_props.iter().find(|p| &p.name == refr) else {
                        errs.push(GraphError::at(
                            GRAPH_BAD_SOURCE,
                            &n.id,
                            format!("{}.{} 引用不存在 exposedProp {refr}", n.ntype, pin_name),
                        ));
                        continue;
                    };
                    if !pin_compatible(kind_pin(prop.kind), pin.ty) {
                        errs.push(GraphError::at(
                            GRAPH_TYPE_MISMATCH,
                            &n.id,
                            format!("{}.{}({}) ← ref {}({}) 类型不匹配", n.ntype, pin_name, pin.ty.name(), refr, kind_pin(prop.kind).name()),
                        ));
                    }
                }
            }
        }
    }

    // ---- 7. edges 引用存在性(from 执行出口合法 / to 执行入口合法)----
    for e in &doc.edges {
        let (fid, fpin) = (&e.from[0], &e.from[1]);
        let (tid, tpin) = (&e.to[0], &e.to[1]);
        match index.get(fid.as_str()).and_then(|&i| find_spec(&doc.nodes[i].ntype)) {
            None => errs.push(GraphError::global(GRAPH_BAD_SOURCE, format!("执行边 from 节点不存在或未知类型: {fid}"))),
            Some(spec) => {
                if !spec.exec_out.contains(&fpin.as_str()) {
                    errs.push(GraphError::at(
                        GRAPH_BAD_SOURCE,
                        fid,
                        format!("{} 无执行出口 pin {fpin}", spec.ntype),
                    ));
                }
            }
        }
        match index.get(tid.as_str()).and_then(|&i| find_spec(&doc.nodes[i].ntype)) {
            None => errs.push(GraphError::global(GRAPH_BAD_SOURCE, format!("执行边 to 节点不存在或未知类型: {tid}"))),
            Some(spec) => {
                if !(spec.exec_in && tpin == "exec") {
                    errs.push(GraphError::at(
                        GRAPH_BAD_SOURCE,
                        tid,
                        format!("{} 无执行入口 pin {tpin}", spec.ntype),
                    ));
                }
            }
        }
    }

    // ---- 6. 环检测(DFS 三色)----
    // 执行边邻接:from 节点 → to 节点。
    let exec_adj: Vec<(String, String)> = doc
        .edges
        .iter()
        .map(|e| (e.from[0].clone(), e.to[0].clone()))
        .collect();
    if let Some(cyc) = find_cycle(&doc.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), &exec_adj) {
        errs.push(GraphError::at(GRAPH_EXEC_CYCLE, &cyc, format!("执行边成环(途经节点 {cyc})")));
    }
    // 数据边邻接:本节点 → inputs 引用的节点(仅统计存在的节点)。
    let mut data_adj: Vec<(String, String)> = Vec::new();
    for n in &doc.nodes {
        for src in n.inputs.values() {
            if let ValueSource::NodePin { node, .. } = src {
                if index.contains_key(node.as_str()) {
                    data_adj.push((n.id.clone(), node.clone()));
                }
            }
        }
    }
    if let Some(cyc) = find_cycle(&doc.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), &data_adj) {
        errs.push(GraphError::at(GRAPH_DATA_CYCLE, &cyc, format!("数据边成环(途经节点 {cyc})")));
    }

    errs
}

// ── RD-F4-004 第九校验臂:call_function 互绑(10 §4.3)────────────────────────
//
// 首发标量子集(D-RD4-C):参数/返回仅 f32/f64/i32/bool + void;其余 C 子集 v1 类型
// (i8/i16/i64/u*/指针/String/Vec3/数组)如实 GRAPH_CALL_SIG_MISMATCH 拒绝,不静默截断。

/// 项目感校验:纯校验八臂 + call_function 第九臂(读 module 文件,root = 项目根)。
pub fn validate_graph_with_project(doc: &GraphDoc, root: &std::path::Path) -> Vec<GraphError> {
    let mut errs = validate_graph(doc);
    // 扫描器缓存:同 module 文件只读一次(文本级,D-RD4-A)。
    let mut export_cache: HashMap<String, Option<Vec<crate::rxexport::ExportedFn>>> = HashMap::new();
    for n in &doc.nodes {
        if n.ntype != "call.call_function" && n.ntype != "call.native_frame" {
            continue;
        }
        // module/fn 缺失/悬空已被八臂 GRAPH_DANGLING_INPUT 报过,跳过不级联。
        let (Some(module_src), Some(fn_src)) = (n.inputs.get("module"), n.inputs.get("fn")) else {
            continue;
        };
        // module/fn 须为 const 字符串(校验期静态解析;动态来源如实拒)。
        let module = match const_str(module_src) {
            Some(m) => m,
            None => {
                errs.push(GraphError::at(GRAPH_CALL_SIG_MISMATCH, &n.id, "call_function.module 须为 const 字符串(校验期静态解析;动态 module 来源登记 deferred)"));
                continue;
            }
        };
        let fn_name = match const_str(fn_src) {
            Some(f) => f,
            None => {
                errs.push(GraphError::at(GRAPH_CALL_SIG_MISMATCH, &n.id, "call_function.fn 须为 const 字符串(校验期静态解析)"));
                continue;
            }
        };
        // module 路径安全:相对项目根,拒绝对路径与 .. 越界(I-5 同源纪律)。
        let mpath = std::path::Path::new(&module);
        let rel_ok = mpath.is_relative() && !mpath.components().any(|c| matches!(c, std::path::Component::ParentDir));
        let exports = if rel_ok {
            export_cache
                .entry(module.clone())
                .or_insert_with(|| {
                    let p = root.join(&module);
                    std::fs::read_to_string(p)
                        .ok()
                        .map(|text| if mpath.extension().and_then(|s| s.to_str()) == Some("rs") {
                            crate::rxexport::scan_rust_c_fns(&text)
                        } else { crate::rxexport::scan_export_c_fns(&text) })
                })
                .clone()
        } else {
            None
        };
        let Some(exports) = exports else {
            errs.push(GraphError::at(
                GRAPH_CALL_MODULE_NOT_FOUND,
                &n.id,
                format!("call_function module 不可解析: {module}(须项目根相对路径,不越界,文件存在)"),
            ));
            continue;
        };
        let Some(efn) = exports.iter().find(|e| e.name == fn_name) else {
            errs.push(GraphError::at(
                GRAPH_CALL_FN_NOT_EXPORTED,
                &n.id,
                format!("{module} 无 #[export(c)] pub fn {fn_name}(文本级扫描;导出表: [{}])", exports.iter().map(|e| e.name.as_str()).collect::<Vec<_>>().join(", ")),
            ));
            continue;
        };
        if n.ntype == "call.native_frame" {
            let signature:Vec<&str>=efn.params.iter().map(|(_,t)|t.as_str()).collect();
            if efn.ret!="u32" || signature!=["u32","f32","*const NativeBinding","u32","*mut NativeUpdate","u32"] {
                errs.push(GraphError::at(GRAPH_CALL_SIG_MISMATCH,&n.id,"native_frame requires ABI v1 (u32, f32, *const NativeBinding, u32, *mut NativeUpdate, u32) -> u32"));
            }
            match n.inputs.get("bindings") {
                Some(ValueSource::Const{konst})=>match serde_json::from_value::<Vec<crate::callruntime::NativeBinding>>(konst.clone()) {
                    Ok(bindings)=>{let ids:std::collections::HashSet<u64>=bindings.iter().map(|b|b.entity_id).collect();if bindings.len()>4096||ids.len()!=bindings.len(){errs.push(GraphError::at(GRAPH_CALL_SIG_MISMATCH,&n.id,"native frame bindings exceed 4096 or contain duplicate entity IDs"));}},
                    Err(error)=>errs.push(GraphError::at(GRAPH_CALL_SIG_MISMATCH,&n.id,format!("invalid native frame bindings: {error}"))),
                },
                _=>errs.push(GraphError::at(GRAPH_CALL_SIG_MISMATCH,&n.id,"native frame bindings must be a constant array")),
            }
            continue;
        }
        // 返回类型子集门(result 编组面)。
        if !matches!(efn.ret.as_str(), "void" | "f32" | "f64" | "i32" | "bool") {
            errs.push(GraphError::at(
                GRAPH_CALL_SIG_MISMATCH,
                &n.id,
                format!("{fn_name} 返回类型 {} 超首发标量子集(void/f32/f64/i32/bool)", efn.ret),
            ));
            continue;
        }
        // 同构性门(D-RD4-C:运行时无混合类型蹦床,校验期如实拒)。
        if let Some(t0) = efn.params.first().map(|(_, t)| t.as_str()) {
            if efn.params.iter().any(|(_, t)| t != t0) {
                errs.push(GraphError::at(
                    GRAPH_CALL_SIG_MISMATCH,
                    &n.id,
                    format!("{fn_name} 混合参数类型超首发编组面(运行时同构限定;须全部同型)"),
                ));
                continue;
            }
        }
        // args:const 数组 → 全静态检查;NodePin/Ref → 运行时检查(D-RD4-E),校验期放行。
        if let Some(args_src) = n.inputs.get("args") {
            if let ValueSource::Const { konst } = args_src {
                let Some(args) = konst.as_array() else {
                    errs.push(GraphError::at(GRAPH_CALL_SIG_MISMATCH, &n.id, "call_function.args const 须为数组"));
                    continue;
                };
                if args.len() != efn.params.len() {
                    errs.push(GraphError::at(
                        GRAPH_CALL_SIG_MISMATCH,
                        &n.id,
                        format!("{fn_name} 参数个数不匹配:签名 {},args {}", efn.params.len(), args.len()),
                    ));
                    continue;
                }
                for (i, ((pname, pty), arg)) in efn.params.iter().zip(args.iter()).enumerate() {
                    match scalar_const_ok(pty, arg) {
                        Some(true) => {}
                        Some(false) => {
                            errs.push(GraphError::at(
                                GRAPH_CALL_SIG_MISMATCH,
                                &n.id,
                                format!("{fn_name} 第 {} 参 {pname}: {pty} 与 const {arg} 类型不匹配", i + 1),
                            ));
                            break;
                        }
                        None => {
                            errs.push(GraphError::at(
                                GRAPH_CALL_SIG_MISMATCH,
                                &n.id,
                                format!("{fn_name} 第 {} 参 {pname} 类型 {pty} 超首发标量子集(f32/f64/i32/bool)", i + 1),
                            ));
                            break;
                        }
                    }
                }
            }
        }
    }
    errs
}

fn const_str(src: &ValueSource) -> Option<String> {
    match src {
        ValueSource::Const { konst } => konst.as_str().map(str::to_string),
        _ => None,
    }
}

/// 首发标量子集类型 × const 字面量:Some(true/false) = 子集内匹配判定;None = 超子集。
fn scalar_const_ok(rx_ty: &str, v: &Value) -> Option<bool> {
    match rx_ty {
        "f32" | "f64" | "i32" => Some(v.is_number()),
        "bool" => Some(v.is_boolean()),
        _ => None,
    }
}

/// DFS 三色找环:返回任一环节点 id(无环 → None)。重复 id 去重按首现。
fn find_cycle(nodes: &[&str], edges: &[(String, String)]) -> Option<String> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b) in edges {
        adj.entry(a.as_str()).or_default().push(b.as_str());
    }
    // 0 = 白(未访),1 = 灰(在栈),2 = 黑(完成)。
    let mut color: HashMap<&str, u8> = HashMap::new();
    for n in nodes {
        color.entry(*n).or_insert(0);
    }
    for start in nodes {
        if color.get(start).copied() != Some(0) {
            continue;
        }
        // 显式栈避免递归:(节点, 子迭代游标)。
        let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
        color.insert(start, 1);
        while let Some((node, ci)) = stack.last().copied() {
            let children: &[&str] = adj.get(node).map(Vec::as_slice).unwrap_or(&[]);
            if ci < children.len() {
                stack.last_mut().unwrap().1 += 1;
                let next = children[ci];
                match color.get(next).copied().unwrap_or(0) {
                    1 => return Some(next.to_string()),
                    0 => {
                        color.insert(next, 1);
                        stack.push((next, 0));
                    }
                    _ => {}
                }
            } else {
                color.insert(node, 2);
                stack.pop();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphDoc;
    use serde_json::json;

    fn codes(doc: &GraphDoc) -> Vec<&'static str> {
        validate_graph(doc).iter().map(|e| e.code).collect()
    }

    /// 10 §4.1 DoorOpener 逐字结构(fixture 蓝本同构)。
    fn door_opener() -> GraphDoc {
        serde_json::from_value(json!({
            "version": 1,
            "id": "g_door",
            "name": "DoorOpener",
            "exposedProps": [ { "name": "openSpeed", "kind": "F32", "default": 90.0 } ],
            "nodes": [
                { "id": "n1", "type": "event.on_trigger_enter", "pos": [40, 80] },
                { "id": "n2", "type": "flow.branch", "pos": [240, 80],
                  "inputs": { "condition": { "node": "n3", "pin": "out" } } },
                { "id": "n3", "type": "entity.has_tag", "pos": [60, 200],
                  "inputs": { "entity": { "node": "n1", "pin": "otherEntity" }, "tag": { "const": "player" } } },
                { "id": "n4", "type": "transform.rotate_tween", "pos": [460, 80],
                  "inputs": { "target": { "const": "$self" }, "angle": { "ref": "openSpeed" }, "duration": { "const": 1.2 } } }
            ],
            "edges": [ { "from": ["n1", "exec"], "to": ["n2", "exec"] }, { "from": ["n2", "then"], "to": ["n4", "exec"] } ]
        }))
        .unwrap()
    }

    #[test]
    fn door_opener_passes() {
        let errs = validate_graph(&door_opener());
        assert!(errs.is_empty(), "DoorOpener 须零错误: {errs:?}");
    }

    #[test]
    fn schema_errors() {
        let mut d = door_opener();
        d.version = 2;
        d.id.clear();
        d.name.clear();
        d.nodes.push(d.nodes[0].clone()); // dup id n1
        d.exposed_props.push(d.exposed_props[0].clone()); // dup prop
        let errs = validate_graph(&d);
        let c: Vec<_> = errs.iter().map(|e| e.code).collect();
        assert_eq!(c.iter().filter(|&&x| x == GRAPH_SCHEMA).count(), 5, "{errs:?}");
        assert!(errs.iter().all(|e| e.code != GRAPH_SCHEMA || e.message.len() > 3));
    }

    #[test]
    fn unknown_node_rejected() {
        let mut d = door_opener();
        d.nodes[1].ntype = "flow.teleport".into();
        let c = codes(&d);
        assert!(c.contains(&GRAPH_UNKNOWN_NODE), "{c:?}");
    }

    #[test]
    fn dup_event_rejected() {
        let mut d = door_opener();
        let mut n = d.nodes[0].clone();
        n.id = "n9".into();
        d.nodes.push(n);
        let errs = validate_graph(&d);
        assert!(errs.iter().any(|e| e.code == GRAPH_DUP_EVENT && e.node_id.as_deref() == Some("n9")), "{errs:?}");
    }

    #[test]
    fn dangling_input_rejected() {
        let mut d = door_opener();
        d.nodes[1].inputs.clear(); // branch.condition 未接
        let errs = validate_graph(&d);
        assert!(errs.iter().any(|e| e.code == GRAPH_DANGLING_INPUT && e.node_id.as_deref() == Some("n2")), "{errs:?}");
    }

    #[test]
    fn const_type_mismatch_rejected() {
        let mut d = door_opener();
        d.nodes[1].inputs.insert("condition".into(), ValueSource::Const { konst: json!("yes") });
        let c = codes(&d);
        assert!(c.contains(&GRAPH_TYPE_MISMATCH), "{c:?}");
    }

    #[test]
    fn bad_node_source_and_pin_rejected() {
        let mut d = door_opener();
        d.nodes[1].inputs.insert("condition".into(), ValueSource::NodePin { node: "ghost".into(), pin: "out".into() });
        assert!(codes(&d).contains(&GRAPH_BAD_SOURCE));
        let mut d2 = door_opener();
        d2.nodes[1].inputs.insert("condition".into(), ValueSource::NodePin { node: "n3".into(), pin: "nope".into() });
        assert!(codes(&d2).contains(&GRAPH_BAD_SOURCE));
    }

    #[test]
    fn data_pin_type_mismatch_rejected() {
        // lerp.out(F32) → branch.condition(Bool)。
        let mut d = door_opener();
        d.nodes.push(serde_json::from_value(json!({
            "id": "n5", "type": "transform.lerp", "pos": [0, 0],
            "inputs": { "a": { "const": 0.0 }, "b": { "const": 1.0 }, "t": { "const": 0.5 } }
        })).unwrap());
        d.nodes[1].inputs.insert("condition".into(), ValueSource::NodePin { node: "n5".into(), pin: "out".into() });
        assert!(codes(&d).contains(&GRAPH_TYPE_MISMATCH));
    }

    #[test]
    fn ref_source_checks() {
        // ref 不存在 → GRAPH_BAD_SOURCE。
        let mut d = door_opener();
        d.nodes[3].inputs.insert("angle".into(), ValueSource::Ref { refr: "ghost".into() });
        assert!(codes(&d).contains(&GRAPH_BAD_SOURCE));
        // String prop → F32 pin → GRAPH_TYPE_MISMATCH。
        let mut d2 = door_opener();
        d2.exposed_props[0].kind = PropKind::String;
        d2.exposed_props[0].default = json!("fast");
        assert!(codes(&d2).contains(&GRAPH_TYPE_MISMATCH));
        // Any pin 兼容任意 kind:var.set value:Any ← ref F32 合法(正例)。
        let d3: GraphDoc = serde_json::from_value(json!({
            "version": 1, "id": "g", "name": "g",
            "exposedProps": [ { "name": "v", "kind": "F32", "default": 1.0 } ],
            "nodes": [
                { "id": "a", "type": "event.on_start", "pos": [0, 0] },
                { "id": "b", "type": "var.set", "pos": [1, 1],
                  "inputs": { "name": { "const": "x" }, "value": { "ref": "v" } } }
            ],
            "edges": [ { "from": ["a", "exec"], "to": ["b", "exec"] } ]
        })).unwrap();
        assert!(validate_graph(&d3).is_empty(), "{:?}", validate_graph(&d3));
    }

    #[test]
    fn exec_cycle_rejected() {
        let d: GraphDoc = serde_json::from_value(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "a", "type": "flow.delay", "pos": [0, 0], "inputs": { "duration": { "const": 1.0 } } },
                { "id": "b", "type": "flow.delay", "pos": [1, 1], "inputs": { "duration": { "const": 1.0 } } }
            ],
            "edges": [ { "from": ["a", "exec"], "to": ["b", "exec"] }, { "from": ["b", "exec"], "to": ["a", "exec"] } ]
        })).unwrap();
        assert!(codes(&d).contains(&GRAPH_EXEC_CYCLE));
    }

    #[test]
    fn data_cycle_rejected() {
        let d: GraphDoc = serde_json::from_value(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "a", "type": "transform.lerp", "pos": [0, 0],
                  "inputs": { "a": { "node": "b", "pin": "out" }, "b": { "const": 1.0 }, "t": { "const": 0.5 } } },
                { "id": "b", "type": "transform.lerp", "pos": [1, 1],
                  "inputs": { "a": { "node": "a", "pin": "out" }, "b": { "const": 1.0 }, "t": { "const": 0.5 } } }
            ]
        })).unwrap();
        assert!(codes(&d).contains(&GRAPH_DATA_CYCLE));
        // DoorOpener 数据边 n2←n3←n1 无环(反例对照)。
        assert!(!codes(&door_opener()).contains(&GRAPH_DATA_CYCLE));
    }

    #[test]
    fn edge_ref_checks() {
        // from 引用不存在节点。
        let mut d = door_opener();
        d.edges[0].from[0] = "ghost".into();
        assert!(codes(&d).contains(&GRAPH_BAD_SOURCE));
        // from 执行出口非法(branch 无 exec 出口)。
        let mut d2 = door_opener();
        d2.edges[1].from[1] = "exec".into();
        assert!(codes(&d2).contains(&GRAPH_BAD_SOURCE));
        // to 指向事件节点(无执行入口)。
        let mut d3 = door_opener();
        d3.edges[0].to = ["n1".into(), "exec".into()];
        assert!(codes(&d3).contains(&GRAPH_BAD_SOURCE));
    }

    #[test]
    fn integer_const_into_f32_allowed() {
        // integer 字面量进 F32 允许;number 进 I32 亦可(10 §5 值来源匹配放宽)。
        let d: GraphDoc = serde_json::from_value(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "a", "type": "event.on_start", "pos": [0, 0] },
                { "id": "b", "type": "flow.delay", "pos": [1, 1], "inputs": { "duration": { "const": 2 } } }
            ],
            "edges": [ { "from": ["a", "exec"], "to": ["b", "exec"] } ]
        })).unwrap();
        assert!(validate_graph(&d).is_empty(), "{:?}", validate_graph(&d));
    }

    // ── RD-F4-004 第九校验臂 ──

    fn tmp_project(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "forge_logic_call_{}_{}_{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(dir.join("Content/Scripts")).unwrap();
        std::fs::write(
            dir.join("Content/Scripts/math.rx"),
            "#[export(c)]\npub fn add(a: f32, b: f32) -> f32 { a + b }\n\
             #[export(c)]\npub fn is_ready(flag: bool) -> bool { flag }\n\
             #[export(c)]\npub fn wide(x: i64) -> i64 { x }\n\
             pub fn helper(x: i32) -> i32 { x }\n",
        )
        .unwrap();
        dir
    }

    fn call_graph(module: &str, fn_name: &str, args: serde_json::Value) -> GraphDoc {
        serde_json::from_value(json!({
            "version": 1, "id": "g_call", "name": "CallProbe",
            "nodes": [
                { "id": "a", "type": "event.on_start", "pos": [0, 0] },
                { "id": "c", "type": "call.call_function", "pos": [1, 0],
                  "inputs": { "module": { "const": module }, "fn": { "const": fn_name }, "args": { "const": args } } },
                { "id": "s", "type": "var.set", "pos": [2, 0],
                  "inputs": { "name": { "const": "r" }, "value": { "node": "c", "pin": "result" } } }
            ],
            "edges": [ { "from": ["a", "exec"], "to": ["c", "exec"] }, { "from": ["c", "exec"], "to": ["s", "exec"] } ]
        }))
        .unwrap()
    }

    #[test]
    fn call_arm_good_graph_passes() {
        let root = tmp_project("good");
        let d = call_graph("Content/Scripts/math.rx", "add", json!([2, 3.5]));
        let errs = validate_graph_with_project(&d, &root);
        assert!(errs.is_empty(), "{errs:?}");
        // bool 参数 + bool 返回。
        let d2 = call_graph("Content/Scripts/math.rx", "is_ready", json!([true]));
        assert!(validate_graph_with_project(&d2, &root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn call_arm_module_not_found() {
        let root = tmp_project("mnf");
        let d = call_graph("Content/Scripts/ghost.rx", "add", json!([1, 2]));
        let errs = validate_graph_with_project(&d, &root);
        assert!(errs.iter().any(|e| e.code == GRAPH_CALL_MODULE_NOT_FOUND && e.node_id.as_deref() == Some("c")), "{errs:?}");
        // 越界路径同码拒。
        let d2 = call_graph("../outside.rx", "add", json!([1, 2]));
        assert!(validate_graph_with_project(&d2, &root).iter().any(|e| e.code == GRAPH_CALL_MODULE_NOT_FOUND));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn call_arm_fn_not_exported() {
        let root = tmp_project("fne");
        // helper 无 #[export(c)]。
        let d = call_graph("Content/Scripts/math.rx", "helper", json!([1]));
        let errs = validate_graph_with_project(&d, &root);
        assert!(errs.iter().any(|e| e.code == GRAPH_CALL_FN_NOT_EXPORTED), "{errs:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn call_arm_sig_mismatch() {
        let root = tmp_project("sig");
        // 个数不匹配。
        let d = call_graph("Content/Scripts/math.rx", "add", json!([1]));
        assert!(validate_graph_with_project(&d, &root).iter().any(|e| e.code == GRAPH_CALL_SIG_MISMATCH));
        // 类型不匹配(bool 进 f32)。
        let d2 = call_graph("Content/Scripts/math.rx", "add", json!([1, true]));
        assert!(validate_graph_with_project(&d2, &root).iter().any(|e| e.code == GRAPH_CALL_SIG_MISMATCH));
        // 超首发子集(i64 参数与返回)。
        let d3 = call_graph("Content/Scripts/math.rx", "wide", json!([1]));
        let errs3 = validate_graph_with_project(&d3, &root);
        assert!(errs3.iter().any(|e| e.code == GRAPH_CALL_SIG_MISMATCH), "{errs3:?}");
        // args 非数组。
        let d4 = call_graph("Content/Scripts/math.rx", "add", json!(3.5));
        assert!(validate_graph_with_project(&d4, &root).iter().any(|e| e.code == GRAPH_CALL_SIG_MISMATCH));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn call_arm_pure_validate_untouched() {
        // 纯 validate_graph 不含第九臂(无 root 不炸,保持八臂语义)。
        let d = call_graph("Content/Scripts/ghost.rx", "nope", json!([]));
        assert!(validate_graph(&d).is_empty(), "纯校验不查 module/fn: {:?}", validate_graph(&d));
    }
}
