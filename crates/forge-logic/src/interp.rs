//! 图解释执行运行时(F4 wave.3,10 §3.2 调度契约逐字):
//! 每逻辑帧 输入事件 → 接触事件(物理 contact 规范序 → 逻辑 trigger 沿检测,D-F4-G)→
//! timer → update → message 队列清空;同实体多图按挂载序;跨实体按实体 id 升序。
//!
//! 语义裁决(10 §3.1):on_start 在 load 时立即执行(=「PIE 进入/实体激活」)。
//! 未实现节点(physics.*/audio.*/entity.spawn/entity.destroy/transform.look_at/transform.lerp/
//! flow.for_each/flow.gate/debug.draw_debug_line)如实 log(RD-F4-004:call.call_function 已摘 unsupported → callruntime dll 腿)
//! `logic.unsupported` 后继续执行链,不静默不伪造。

use std::collections::{BTreeMap, BTreeSet};

use forge_scene::{Component, Scene, Transform};
use serde_json::{json, Value};

use crate::graph::{GraphDoc, ValueSource};

/// 执行链递归深度上限(校验器已禁执行环,此为防御性护栏;越限如实 log)。
const MAX_EXEC_DEPTH: usize = 256;

/// 接触相位(rurix-physics ContactPhase 的库内镜像——forge-logic 不依赖物理库)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicPhase {
    Begin,
    Persist,
    End,
}

impl LogicPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            LogicPhase::Begin => "begin",
            LogicPhase::Persist => "persist",
            LogicPhase::End => "end",
        }
    }

    fn event_type(self) -> &'static str {
        match self {
            LogicPhase::Begin => "event.on_contact_begin",
            LogicPhase::Persist => "event.on_contact_persist",
            LogicPhase::End => "event.on_contact_end",
        }
    }
}

/// 物理接触事件(已由宿主从 BodyId 翻译为实体 id;序列保持物理规范序)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogicContact {
    pub a: u64,
    pub b: u64,
    pub phase: LogicPhase,
}

/// 精灵动画命令(F-GAME-4:图节点 sprite.*/animator.* 产出,宿主动画系统消费——
/// 组件 frame/clip 的唯一写者是宿主,图侧只发命令,避免双写者)。
#[derive(Debug, Clone, PartialEq)]
pub enum AnimCommand {
    /// 播放 clip;restart=false 时同 clip 幂等 no-op(状态式脚本缺省,防"每帧重启冻帧"坑)。
    Play { entity: u64, clip: String, restart: bool },
    /// 停止播放(停在当前帧)。
    Stop { entity: u64 },
    /// 手控帧(clip 内序号;越界钳制)。
    SetFrame { entity: u64, index: usize },
    /// animator bool 参数。
    SetBool { entity: u64, param: String, value: bool },
    /// animator trigger 参数(转换触发即消费)。
    SetTrigger { entity: u64, param: String },
}

/// 活跃 tween(rotate = 绕本地 Y 轴 angle 度制 / move = 线性 offset;末帧钳制写最终值)。
#[derive(Debug, Clone, Copy)]
enum TweenKind {
    Rotate { angle_deg: f64, start_rot: [f32; 4] },
    Move { offset: [f64; 3], start_pos: [f32; 3] },
}

#[derive(Debug, Clone, Copy)]
struct Tween {
    target: u64,
    kind: TweenKind,
    elapsed: f64,
    duration: f64,
}

/// flow.delay 挂起的链延续(remaining 秒,帧推)。
#[derive(Debug, Clone)]
struct Delay {
    continuations: Vec<String>,
    remaining: f64,
}

/// 单图运行实例:图文档 + 合并后暴露属性 + 黑板 vars + tween/delay/timer 表。
#[derive(Debug, Clone)]
pub struct GraphInstance {
    doc: GraphDoc,
    /// 暴露属性合并表(default ← Script.props dict 覆盖)。
    props: BTreeMap<String, Value>,
    /// 黑板(var.*)。
    vars: BTreeMap<String, Value>,
    tweens: Vec<Tween>,
    delays: Vec<Delay>,
    /// timerId → 剩余秒(f64,避开 f32 累减精度坑)。
    timers: BTreeMap<String, f64>,
    /// 执行边邻接:(节点 id, 执行出口 pin)→ [目标节点 id](edges 声明序)。
    exec_adj: BTreeMap<(String, String), Vec<String>>,
    /// 动作节点输出暂存(RD-F4-004:call_function result;key = "{nodeId}.{pin}")。
    node_outputs: BTreeMap<String, Value>,
    native_bindings: BTreeMap<String, Result<Vec<crate::callruntime::NativeBinding>, String>>,
}

impl GraphInstance {
    fn new(doc: GraphDoc, props_override: &Value) -> Self {
        let mut props = BTreeMap::new();
        for p in &doc.exposed_props {
            let v = props_override
                .get(&p.name)
                .cloned()
                .unwrap_or_else(|| p.default.clone());
            props.insert(p.name.clone(), v);
        }
        let mut exec_adj: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for e in &doc.edges {
            exec_adj
                .entry((e.from[0].clone(), e.from[1].clone()))
                .or_default()
                .push(e.to[0].clone());
        }
        let native_bindings = doc.nodes.iter().filter(|n|n.ntype=="call.native_frame").map(|n|{
            let bindings=match n.inputs.get("bindings") {
                Some(ValueSource::Const{konst})=>serde_json::from_value(konst.clone()).map_err(|e|format!("invalid native bindings: {e}")),
                _=>Err("native bindings must be a constant array".into()),
            };(n.id.clone(),bindings)
        }).collect();
        GraphInstance {
            doc,
            props,
            vars: BTreeMap::new(),
            tweens: Vec::new(),
            delays: Vec::new(),
            timers: BTreeMap::new(),
            exec_adj,
            node_outputs: BTreeMap::new(),
            native_bindings,
        }
    }

    pub fn graph_id(&self) -> &str {
        &self.doc.id
    }

    fn event_node(&self, event_type: &str) -> Option<&crate::graph::Node> {
        self.doc.nodes.iter().find(|n| n.ntype == event_type)
    }

    fn exec_targets(&self, node_id: &str, out_pin: &str) -> Vec<String> {
        self.exec_adj
            .get(&(node_id.to_string(), out_pin.to_string()))
            .cloned()
            .unwrap_or_default()
    }
}

/// 事件上下文:当前事件节点 id + 其输出 pin 绑定值(供 NodePin 求值)。
struct EventCtx {
    node: String,
    pins: Vec<(&'static str, Value)>,
}

impl EventCtx {
    fn empty() -> Self {
        EventCtx { node: String::new(), pins: Vec::new() }
    }

    fn bound(&self, node: &str, pin: &str) -> Option<&Value> {
        if self.node == node {
            self.pins.iter().find(|(p, _)| *p == pin).map(|(_, v)| v)
        } else {
            None
        }
    }
}

/// 逻辑运行时:全场景图实例集 + trigger 上帧重叠集 + message 队列。
#[derive(Debug, Default)]
pub struct LogicRuntime {
    /// (挂载实体 id, 实例);插入序 = 挂载序。
    graphs: Vec<(u64, GraphInstance)>,
    prev_trigger_overlap: BTreeSet<(u64, u64)>,
    message_queue: Vec<(String, Value)>,
    /// call_function dll 运行时(RD-F4-004;None = 未挂项目根,调用如实 logic.call_error)。
    call_rt: Option<crate::callruntime::CallRuntime>,
    /// 本帧累计的精灵动画命令(F-GAME-4;宿主 frame 后 take 消费)。
    anim_commands: Vec<AnimCommand>,
}

impl LogicRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// 挂 call_function 项目根(engine-host play_enter 接线;rd-f4-004)。
    pub fn set_project_root(&mut self, root: std::path::PathBuf) {
        self.call_rt = Some(crate::callruntime::CallRuntime::new(root));
    }

    pub fn clear(&mut self) {
        self.graphs.clear();
        self.prev_trigger_overlap.clear();
        self.message_queue.clear();
        self.anim_commands.clear();
    }

    /// 取走本帧累计的动画命令(宿主 advance_frame 在 frame() 后调用,声明序)。
    pub fn take_anim_commands(&mut self) -> Vec<AnimCommand> {
        std::mem::take(&mut self.anim_commands)
    }

    pub fn graph_count(&self) -> usize {
        self.graphs.len()
    }

    /// 测试/host 内省:取实体首个图的黑板变量。
    pub fn debug_var(&self, entity: u64, name: &str) -> Option<Value> {
        self.graphs
            .iter()
            .find(|(e, _)| *e == entity)
            .and_then(|(_, g)| g.vars.get(name).cloned())
    }

    /// 加载图并立即执行 on_start 链(10 §3.1「PIE 进入/实体激活」语义)。
    pub fn load(
        &mut self,
        entity: u64,
        doc: GraphDoc,
        props_override: &Value,
        scene: &mut Scene,
        log: &mut Vec<(String, Value)>,
    ) {
        let inst = GraphInstance::new(doc, props_override);
        let graph_id = inst.graph_id().to_string();
        self.graphs.push((entity, inst));
        log.push((
            "logic.start".to_string(),
            json!({ "entityId": entity, "graphId": graph_id }),
        ));
        let gi = self.graphs.len() - 1;
        self.exec_event(gi, "event.on_start", Vec::new(), scene, log);
    }

    /// 热重载(D-F4-B):该实体图实例重建 + on_start 重发 + 黑板/tween/timer 重置。
    pub fn reload(
        &mut self,
        entity: u64,
        doc: GraphDoc,
        props_override: &Value,
        scene: &mut Scene,
        log: &mut Vec<(String, Value)>,
    ) {
        self.unload(entity);
        self.load(entity, doc, props_override, scene, log);
    }

    /// 卸载实体全部图实例(graphRef 置空路径)。
    pub fn unload(&mut self, entity: u64) {
        self.graphs.retain(|(e, _)| *e != entity);
    }

    /// 实体 id 升序(稳定,同实体保持挂载序)的实例下标表。
    fn order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.graphs.len()).collect();
        idx.sort_by_key(|&i| self.graphs[i].0);
        idx
    }

    fn order_for(&self, entity: u64) -> Vec<usize> {
        self.graphs
            .iter()
            .enumerate()
            .filter(|(_, (e, _))| *e == entity)
            .map(|(i, _)| i)
            .collect()
    }

    /// 在实例 gi 上派发事件:有对应事件节点才执行(无节点 = 不消费,调用方不 log)。
    fn exec_event(
        &mut self,
        gi: usize,
        event_type: &str,
        binds: Vec<(&'static str, Value)>,
        scene: &mut Scene,
        log: &mut Vec<(String, Value)>,
    ) {
        let Self { graphs, message_queue, call_rt, anim_commands, .. } = self;
        let (eid, inst) = &mut graphs[gi];
        let Some(node) = inst.event_node(event_type) else { return };
        let starts = inst.exec_targets(&node.id, "exec");
        let ev = EventCtx { node: node.id.clone(), pins: binds };
        for t in starts {
            exec_node(inst, *eid, &t, &ev, scene, message_queue, call_rt, anim_commands, log, 0);
        }
    }

    /// 实例 gi 是否有某事件节点。
    fn has_event(&self, gi: usize, event_type: &str) -> bool {
        self.graphs[gi].1.event_node(event_type).is_some()
    }

    /// 完整逻辑帧(规范序;dt 秒)。
    pub fn frame(
        &mut self,
        scene: &mut Scene,
        dt: f32,
        inputs: Vec<(String, f64)>,
        contacts: Vec<LogicContact>,
        log: &mut Vec<(String, Value)>,
    ) {
        let dt64 = f64::from(dt);

        // ---- ① 输入事件 ----
        for (action, value) in inputs {
            for gi in self.order() {
                if !self.has_event(gi, "event.on_input") {
                    continue;
                }
                let (eid, g) = &self.graphs[gi];
                log.push((
                    "logic.input".to_string(),
                    json!({ "entityId": eid, "graphId": g.graph_id(), "action": action, "value": value }),
                ));
                self.exec_event(
                    gi,
                    "event.on_input",
                    vec![("action", json!(action)), ("value", json!(value))],
                    scene,
                    log,
                );
            }
        }

        // ---- ② 接触事件:物理 contact(规范序)→ trigger 沿检测 ----
        let mut disp: Vec<(u64, u64, LogicPhase)> = Vec::with_capacity(contacts.len() * 2);
        for c in contacts {
            disp.push((c.a, c.b, c.phase));
            disp.push((c.b, c.a, c.phase));
        }
        // 稳定排序:跨实体按目标实体 id 升序;同实体内保持物理规范序。
        disp.sort_by_key(|(target, _, _)| *target);
        for (target, other, phase) in disp {
            for gi in self.order_for(target) {
                if !self.has_event(gi, phase.event_type()) {
                    continue;
                }
                let g = &self.graphs[gi].1;
                log.push((
                    "logic.contact".to_string(),
                    json!({ "entityId": target, "graphId": g.graph_id(), "otherEntity": other, "phase": phase.as_str() }),
                ));
                self.exec_event(gi, phase.event_type(), vec![("otherEntity", json!(other))], scene, log);
            }
        }
        // trigger:AABB overlap 沿检测(D-F4-G;Trigger 不建物理 body)。
        let cur = detect_trigger_overlap(scene);
        let enters: Vec<(u64, u64)> = cur.difference(&self.prev_trigger_overlap).copied().collect();
        let exits: Vec<(u64, u64)> = self.prev_trigger_overlap.difference(&cur).copied().collect();
        self.prev_trigger_overlap = cur;
        for (trigger, other) in enters {
            for gi in self.order_for(trigger) {
                if !self.has_event(gi, "event.on_trigger_enter") {
                    continue;
                }
                let g = &self.graphs[gi].1;
                log.push((
                    "logic.trigger".to_string(),
                    json!({ "entityId": trigger, "graphId": g.graph_id(), "otherEntity": other, "phase": "enter" }),
                ));
                self.exec_event(gi, "event.on_trigger_enter", vec![("otherEntity", json!(other))], scene, log);
            }
        }
        for (trigger, other) in exits {
            for gi in self.order_for(trigger) {
                if !self.has_event(gi, "event.on_trigger_exit") {
                    continue;
                }
                let g = &self.graphs[gi].1;
                log.push((
                    "logic.trigger".to_string(),
                    json!({ "entityId": trigger, "graphId": g.graph_id(), "otherEntity": other, "phase": "exit" }),
                ));
                self.exec_event(gi, "event.on_trigger_exit", Vec::new(), scene, log);
            }
        }

        // ---- ③ timer ----
        for gi in self.order() {
            let mut fired: Vec<String> = Vec::new();
            {
                let inst = &mut self.graphs[gi].1;
                for (id, remaining) in inst.timers.iter_mut() {
                    *remaining -= dt64;
                    if *remaining <= 0.0 {
                        fired.push(id.clone());
                    }
                }
                for id in &fired {
                    inst.timers.remove(id);
                }
            }
            for id in fired {
                let (eid, g) = &self.graphs[gi];
                log.push((
                    "logic.timer".to_string(),
                    json!({ "entityId": eid, "graphId": g.graph_id(), "timerId": id }),
                ));
                self.exec_event(gi, "event.on_timer", vec![("timerId", json!(id))], scene, log);
            }
        }

        // ---- ④ update:delay 链延续 → tween 推进 → on_update ----
        for gi in self.order() {
            let mut resumed: Vec<Vec<String>> = Vec::new();
            {
                let Self { graphs, .. } = self;
                let (eid, inst) = &mut graphs[gi];
                let mut keep: Vec<Delay> = Vec::new();
                for mut d in std::mem::take(&mut inst.delays) {
                    d.remaining -= dt64;
                    if d.remaining <= 0.0 {
                        resumed.push(d.continuations);
                    } else {
                        keep.push(d);
                    }
                }
                inst.delays = keep;
                advance_tweens(inst, scene, dt64);
                let _ = eid;
            }
            for conts in resumed {
                let Self { graphs, message_queue, call_rt, anim_commands, .. } = self;
                let (eid, inst) = &mut graphs[gi];
                let ev = EventCtx::empty();
                for node_id in conts {
                    exec_node(inst, *eid, &node_id, &ev, scene, message_queue, call_rt, anim_commands, log, 0);
                }
            }
            if self.has_event(gi, "event.on_update") {
                let (eid, g) = &self.graphs[gi];
                log.push((
                    "logic.update".to_string(),
                    json!({ "entityId": eid, "graphId": g.graph_id(), "dt": dt }),
                ));
                self.exec_event(gi, "event.on_update", vec![("dt", json!(dt))], scene, log);
            }
        }

        // ---- ⑤ message 队列清空(快照派发;派发期间新消息入下帧) ----
        let msgs = std::mem::take(&mut self.message_queue);
        for (name, payload) in msgs {
            for gi in self.order() {
                if !self.has_event(gi, "event.on_message") {
                    continue;
                }
                let (eid, g) = &self.graphs[gi];
                log.push((
                    "logic.message".to_string(),
                    json!({ "entityId": eid, "graphId": g.graph_id(), "name": name }),
                ));
                self.exec_event(
                    gi,
                    "event.on_message",
                    vec![("name", json!(name)), ("payload", payload.clone())],
                    scene,
                    log,
                );
            }
        }
    }
}

// ---------- 值求值 ----------

/// 实体值解析:number → id;"$self" → 挂载实体;"$parent" → 无层级概念不可解析;
/// 其他字符串 → 按实体名查;其余 → None。
fn resolve_entity(v: &Value, self_id: u64, scene: &Scene) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) if s == "$self" => Some(self_id),
        Value::String(s) if s == "$parent" => None,
        Value::String(s) => scene.entities.iter().find(|e| &e.name == s).map(|e| e.id),
        _ => None,
    }
}

fn as_f64(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

fn as_string(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// has_tag = 实体上启用 Tag 组件且 props.tag 匹配(D-F4-F 存在性 + 值查询)。
fn scene_has_tag(scene: &Scene, entity: u64, tag: &str) -> bool {
    scene.entity(entity).is_some_and(|e| {
        e.components
            .iter()
            .any(|c| c.ctype == "Tag" && c.enabled && c.props.get("tag").and_then(Value::as_str) == Some(tag))
    })
}

/// find_by_tag:匹配实体中 id 最小者(确定性)。
fn scene_find_by_tag(scene: &Scene, tag: &str) -> Option<u64> {
    scene
        .entities
        .iter()
        .filter(|e| {
            e.components
                .iter()
                .any(|c| c.ctype == "Tag" && c.enabled && c.props.get("tag").and_then(Value::as_str) == Some(tag))
        })
        .map(|e| e.id)
        .min()
}

fn transform_json(t: &Transform) -> Value {
    json!({ "translation": t.translation, "rotation": t.rotation, "scale": t.scale })
}

/// 节点输入 pin 求值:const / ref(合并后暴露属性)/ node+pin(事件绑定 → 纯节点即时求值)。
fn eval_pin(
    inst: &GraphInstance,
    eid: u64,
    node_id: &str,
    pin: &str,
    ev: &EventCtx,
    scene: &Scene,
    log: &mut Vec<(String, Value)>,
) -> Value {
    let Some(node) = inst.doc.nodes.iter().find(|n| n.id == node_id) else {
        return Value::Null;
    };
    let Some(src) = node.inputs.get(pin) else {
        return Value::Null;
    };
    match src {
        ValueSource::Const { konst } => konst.clone(),
        ValueSource::Ref { refr } => inst.props.get(refr).cloned().unwrap_or(Value::Null),
        ValueSource::NodePin { node: src_node, pin: out } => {
            if let Some(v) = ev.bound(src_node, out) {
                return v.clone();
            }
            // 动作节点输出暂存命中(call_function result 等,RD-F4-004)。
            if let Some(v) = inst.node_outputs.get(&format!("{src_node}.{out}")) {
                return v.clone();
            }
            eval_pure(inst, eid, src_node, out, ev, scene, log)
        }
    }
}

/// 纯节点即时求值(事件上下文透传——纯节点输入可引用事件 pin;
/// 未实现 → logic.unsupported + Null,不静默不伪造)。
#[allow(clippy::too_many_arguments)]
fn eval_pure(
    inst: &GraphInstance,
    eid: u64,
    src_node: &str,
    out_pin: &str,
    ev: &EventCtx,
    scene: &Scene,
    log: &mut Vec<(String, Value)>,
) -> Value {
    let Some(node) = inst.doc.nodes.iter().find(|n| n.id == src_node) else {
        return Value::Null;
    };
    let ntype = node.ntype.as_str();
    let nid = node.id.clone();
    match ntype {
        "math.vec3" => json!([
            as_f64(&eval_pin(inst, eid, &nid, "x", ev, scene, log)),
            as_f64(&eval_pin(inst, eid, &nid, "y", ev, scene, log)),
            as_f64(&eval_pin(inst, eid, &nid, "z", ev, scene, log))
        ]),
        "transform.compose" => {
            let translation = eval_pin(inst, eid, &nid, "translation", ev, scene, log);
            let scale = eval_pin(inst, eid, &nid, "scale", ev, scene, log);
            json!({"translation": parse_vec3(&translation).unwrap_or([0.0; 3]),
                "scale": parse_vec3(&scale).unwrap_or([1.0; 3]),
                "rotation": [0.0, 0.0, 0.0, 1.0]})
        }
        "entity.has_tag" => {
            let entity_v = eval_pin(inst, eid, &nid, "entity", ev, scene, log);
            let tv = eval_pin(inst, eid, &nid, "tag", ev, scene, log);
            match resolve_entity(&entity_v, eid, scene) {
                Some(id) => Value::Bool(scene_has_tag(scene, id, &as_string(&tv))),
                None => Value::Bool(false),
            }
        }
        "entity.find_by_tag" => {
            let tv = eval_pin(inst, eid, &nid, "tag", ev, scene, log);
            scene_find_by_tag(scene, &as_string(&tv)).map_or(Value::Null, |id| json!(id))
        }
        "entity.get_transform" => {
            let entity_v = eval_pin(inst, eid, &nid, "entity", ev, scene, log);
            resolve_entity(&entity_v, eid, scene)
                .and_then(|id| scene.entity(id))
                .map_or(Value::Null, |e| transform_json(&e.transform))
        }
        "var.get" => {
            let nv = eval_pin(inst, eid, &nid, "name", ev, scene, log);
            inst.vars.get(&as_string(&nv)).cloned().unwrap_or(Value::Null)
        }
        other => {
            log.push((
                "logic.unsupported".to_string(),
                json!({ "entityId": eid, "graphId": inst.graph_id(), "nodeId": nid, "nodeType": other, "pin": out_pin }),
            ));
            Value::Null
        }
    }
}

// ---------- 执行链 ----------

/// 执行动作/流控节点并沿执行边续链。event 上下文跨 delay 不保留(延续链以空上下文恢复)。
#[allow(clippy::too_many_arguments)]
fn exec_node(
    inst: &mut GraphInstance,
    eid: u64,
    node_id: &str,
    ev: &EventCtx,
    scene: &mut Scene,
    mq: &mut Vec<(String, Value)>,
    call_rt: &mut Option<crate::callruntime::CallRuntime>,
    anim: &mut Vec<AnimCommand>,
    log: &mut Vec<(String, Value)>,
    depth: usize,
) {
    if depth > MAX_EXEC_DEPTH {
        log.push((
            "logic.unsupported".to_string(),
            json!({ "entityId": eid, "graphId": inst.graph_id(), "nodeId": node_id, "reason": "exec_depth" }),
        ));
        return;
    }
    let Some((node_type,nid)) = inst.doc.nodes.iter().find(|n| n.id == node_id)
        .map(|n|(n.ntype.clone(),n.id.clone())) else {
        return;
    };
    let ntype = node_type.as_str();
    let next_exec = |inst: &GraphInstance| inst.exec_targets(&nid, "exec");
    let nexts: Vec<String> = match ntype {
        "call.native_frame" => {
            let module=as_string(&eval_pin(inst,eid,&nid,"module",ev,scene,log));
            let function=as_string(&eval_pin(inst,eid,&nid,"fn",ev,scene,log));
            let dt=as_f64(&eval_pin(inst,eid,&nid,"dt",ev,scene,log))as f32;
            let parameter=as_string(&eval_pin(inst,eid,&nid,"animatorParam",ev,scene,log));
            let result=match(inst.native_bindings.get(&nid),call_rt.as_mut()){
                (Some(Ok(bindings)),Some(rt))=>rt.invoke_frame(&module,&function,dt,bindings).map_err(|e|e.to_string()),
                (Some(Err(error)),_)=>Err(error.clone()),
                _=>Err("native frame runtime or bindings not configured".into()),
            };
            match result {
                Ok(updates)=>{
                    let indices:std::collections::HashMap<u64,usize>=scene.entities.iter().enumerate().map(|(i,e)|(e.id,i)).collect();
                    for update in &updates {
                        if let Some(&index)=indices.get(&update.entity_id){
                            let entity=&mut scene.entities[index];
                            entity.transform.translation=update.translation;entity.transform.scale=update.scale;
                            if update.frame>=0 {
                                let current=entity.component("Sprite").and_then(|s|s.props.get("frame")).and_then(Value::as_f64);
                                if current!=Some(update.frame as f64){anim.push(AnimCommand::SetFrame{entity:update.entity_id,index:update.frame as usize});}
                            }
                            if update.animator_bool>=0&&!parameter.is_empty(){anim.push(AnimCommand::SetBool{entity:update.entity_id,param:parameter.clone(),value:update.animator_bool!=0});}
                        }
                    }
                    log.push(("logic.native_frame".into(),json!({"entityId":eid,"graphId":inst.graph_id(),"module":module,"updates":updates.len()})));
                }
                Err(reason)=>log.push(("logic.call_error".into(),json!({"entityId":eid,"graphId":inst.graph_id(),"module":module,"fn":function,"reason":reason}))),
            }
            next_exec(inst)
        }
        "flow.branch" => {
            let c = eval_pin(inst, eid, &nid, "condition", ev, scene, log);
            let pin = if c.as_bool().unwrap_or(false) { "then" } else { "else" };
            inst.exec_targets(&nid, pin)
        }
        "flow.sequence" => {
            let mut v = inst.exec_targets(&nid, "seq0");
            v.extend(inst.exec_targets(&nid, "seq1"));
            v
        }
        "flow.delay" => {
            let duration = as_f64(&eval_pin(inst, eid, &nid, "duration", ev, scene, log));
            let continuations = next_exec(inst);
            inst.delays.push(Delay { continuations, remaining: duration });
            Vec::new()
        }
        "flow.timer_start" => {
            let id = as_string(&eval_pin(inst, eid, &nid, "timerId", ev, scene, log));
            let duration = as_f64(&eval_pin(inst, eid, &nid, "duration", ev, scene, log));
            inst.timers.insert(id, duration);
            next_exec(inst)
        }
        "flow.timer_cancel" => {
            let id = as_string(&eval_pin(inst, eid, &nid, "timerId", ev, scene, log));
            inst.timers.remove(&id);
            next_exec(inst)
        }
        "entity.set_transform" => {
            let target = eval_pin(inst, eid, &nid, "entity", ev, scene, log);
            let tv = eval_pin(inst, eid, &nid, "transform", ev, scene, log);
            if let Some(id) = resolve_entity(&target, eid, scene) {
                if let Some(e) = scene.entity_mut(id) {
                    apply_transform_json(&mut e.transform, &tv);
                }
            }
            next_exec(inst)
        }
        "entity.add_tag" => {
            let target = eval_pin(inst, eid, &nid, "entity", ev, scene, log);
            let tag = as_string(&eval_pin(inst, eid, &nid, "tag", ev, scene, log));
            if let Some(id) = resolve_entity(&target, eid, scene) {
                if let Some(e) = scene.entity_mut(id) {
                    // 一组件一标签(D-F4-F):已有 Tag → 改写;无 → 新增。
                    if let Some(c) = e.component_mut("Tag") {
                        c.props = json!({ "tag": tag });
                        c.enabled = true;
                    } else {
                        e.components.push(Component::new("Tag", json!({ "tag": tag })));
                    }
                }
            }
            next_exec(inst)
        }
        "transform.rotate_tween" => {
            let target = eval_pin(inst, eid, &nid, "target", ev, scene, log);
            let angle = as_f64(&eval_pin(inst, eid, &nid, "angle", ev, scene, log));
            let duration = as_f64(&eval_pin(inst, eid, &nid, "duration", ev, scene, log));
            if let Some(id) = resolve_entity(&target, eid, scene) {
                if let Some(e) = scene.entity(id) {
                    let start_rot = e.transform.rotation;
                    if duration <= 0.0 {
                        let rot = quat_mul(start_rot, quat_y(angle));
                        if let Some(em) = scene.entity_mut(id) {
                            em.transform.rotation = rot;
                        }
                    } else {
                        // 同目标同类 tween 重启(从当前姿态重测)。
                        inst.tweens.retain(|t| !(t.target == id && matches!(t.kind, TweenKind::Rotate { .. })));
                        inst.tweens.push(Tween {
                            target: id,
                            kind: TweenKind::Rotate { angle_deg: angle, start_rot },
                            elapsed: 0.0,
                            duration,
                        });
                    }
                }
            }
            next_exec(inst)
        }
        "transform.move_tween" => {
            let target = eval_pin(inst, eid, &nid, "target", ev, scene, log);
            let offset_v = eval_pin(inst, eid, &nid, "offset", ev, scene, log);
            let duration = as_f64(&eval_pin(inst, eid, &nid, "duration", ev, scene, log));
            let offset = parse_vec3(&offset_v).unwrap_or([0.0; 3]);
            if let Some(id) = resolve_entity(&target, eid, scene) {
                if let Some(e) = scene.entity(id) {
                    let start_pos = e.transform.translation;
                    if duration <= 0.0 {
                        if let Some(em) = scene.entity_mut(id) {
                            em.transform.translation = [
                                start_pos[0] + offset[0] as f32,
                                start_pos[1] + offset[1] as f32,
                                start_pos[2] + offset[2] as f32,
                            ];
                        }
                    } else {
                        inst.tweens.retain(|t| !(t.target == id && matches!(t.kind, TweenKind::Move { .. })));
                        inst.tweens.push(Tween {
                            target: id,
                            kind: TweenKind::Move { offset, start_pos },
                            elapsed: 0.0,
                            duration,
                        });
                    }
                }
            }
            next_exec(inst)
        }
        "var.set" => {
            let name = as_string(&eval_pin(inst, eid, &nid, "name", ev, scene, log));
            let value = eval_pin(inst, eid, &nid, "value", ev, scene, log);
            inst.vars.insert(name, value);
            next_exec(inst)
        }
        "var.add" => {
            let name = as_string(&eval_pin(inst, eid, &nid, "name", ev, scene, log));
            let delta = as_f64(&eval_pin(inst, eid, &nid, "value", ev, scene, log));
            let cur = inst.vars.get(&name).and_then(Value::as_f64).unwrap_or(0.0);
            inst.vars.insert(name, json!(cur + delta));
            next_exec(inst)
        }
        "call.send_message" => {
            let name = as_string(&eval_pin(inst, eid, &nid, "name", ev, scene, log));
            let payload = eval_pin(inst, eid, &nid, "payload", ev, scene, log);
            mq.push((name, payload));
            next_exec(inst)
        }
        // ---- sprite.* / animator.*(F-GAME-4:帧动画命令;宿主动画系统消费)----
        "sprite.play" => {
            let target = eval_pin(inst, eid, &nid, "entity", ev, scene, log);
            let clip = as_string(&eval_pin(inst, eid, &nid, "clip", ev, scene, log));
            let restart = eval_pin(inst, eid, &nid, "restart", ev, scene, log)
                .as_bool()
                .unwrap_or(false);
            if let Some(id) = resolve_entity(&target, eid, scene) {
                anim.push(AnimCommand::Play { entity: id, clip, restart });
            }
            next_exec(inst)
        }
        "sprite.stop" => {
            let target = eval_pin(inst, eid, &nid, "entity", ev, scene, log);
            if let Some(id) = resolve_entity(&target, eid, scene) {
                anim.push(AnimCommand::Stop { entity: id });
            }
            next_exec(inst)
        }
        "sprite.set_frame" => {
            let target = eval_pin(inst, eid, &nid, "entity", ev, scene, log);
            let index = as_f64(&eval_pin(inst, eid, &nid, "index", ev, scene, log)).max(0.0) as usize;
            if let Some(id) = resolve_entity(&target, eid, scene) {
                anim.push(AnimCommand::SetFrame { entity: id, index });
            }
            next_exec(inst)
        }
        "animator.set_bool" => {
            let target = eval_pin(inst, eid, &nid, "entity", ev, scene, log);
            let param = as_string(&eval_pin(inst, eid, &nid, "param", ev, scene, log));
            let value = eval_pin(inst, eid, &nid, "value", ev, scene, log)
                .as_bool()
                .unwrap_or(false);
            if let Some(id) = resolve_entity(&target, eid, scene) {
                anim.push(AnimCommand::SetBool { entity: id, param, value });
            }
            next_exec(inst)
        }
        "animator.set_trigger" => {
            let target = eval_pin(inst, eid, &nid, "entity", ev, scene, log);
            let param = as_string(&eval_pin(inst, eid, &nid, "param", ev, scene, log));
            if let Some(id) = resolve_entity(&target, eid, scene) {
                anim.push(AnimCommand::SetTrigger { entity: id, param });
            }
            next_exec(inst)
        }
        "debug.log" => {
            let message = eval_pin(inst, eid, &nid, "message", ev, scene, log);
            log.push((
                "logic.log".to_string(),
                json!({ "entityId": eid, "graphId": inst.graph_id(), "message": message }),
            ));
            next_exec(inst)
        }
        // call.call_function(RD-F4-004):dll 运行时调用 → result 写 node_outputs;
        // 失败如实 logic.call_error 续链(D-RD4-E,不静默不伪造返回值)。
        "call.call_function" => {
            let module = as_string(&eval_pin(inst, eid, &nid, "module", ev, scene, log));
            let fn_name = as_string(&eval_pin(inst, eid, &nid, "fn", ev, scene, log));
            let args_v = eval_pin(inst, eid, &nid, "args", ev, scene, log);
            // args:数组直用;Null = 无参;标量自动包一元数组(F6 wave.2 动态单参链,
            // 黑板/var.get 单值直喂 .rx 单参函数;类型/arity 由 callruntime 复核如实报错)。
            let args: Vec<Value> = match args_v {
                Value::Array(a) => a,
                Value::Null => Vec::new(),
                other => vec![other],
            };
            let invoked = match call_rt.as_mut() {
                Some(rt) => rt.invoke(&module, &fn_name, &args).map_err(|e| e.to_string()),
                None => Err("CallRuntime 未配置(project_root 未挂)".to_string()),
            };
            match invoked {
                Ok(v) => {
                    inst.node_outputs.insert(format!("{nid}.result"), v.clone());
                    log.push((
                        "logic.call".to_string(),
                        json!({ "entityId": eid, "graphId": inst.graph_id(), "nodeId": nid, "module": module, "fn": fn_name, "result": v }),
                    ));
                }
                Err(reason) => {
                    log.push((
                        "logic.call_error".to_string(),
                        json!({ "entityId": eid, "graphId": inst.graph_id(), "nodeId": nid, "module": module, "fn": fn_name, "reason": reason }),
                    ));
                }
            }
            next_exec(inst)
        }
        // 未实现节点:如实 log 后续链(不静默不伪造)。
        "flow.for_each" | "flow.gate" | "entity.spawn" | "entity.destroy" | "transform.look_at"
        | "transform.lerp" | "physics.cast_ray" | "physics.apply_impulse" | "physics.overlap"
        | "audio.play" | "audio.stop" | "debug.draw_debug_line" => {
            log.push((
                "logic.unsupported".to_string(),
                json!({ "entityId": eid, "graphId": inst.graph_id(), "nodeId": nid, "nodeType": ntype }),
            ));
            next_exec(inst)
        }
        _ => Vec::new(), // 事件/纯节点不作为链目标(校验器已挡);到达即终止。
    };
    for n in nexts {
        exec_node(inst, eid, &n, ev, scene, mq, call_rt, anim, log, depth + 1);
    }
}

/// call_function args 归一:数组直用;Null = 无参;标量自动包一元数组(F6 wave.2 动态单参链,
/// 黑板/var.get 单值直喂 .rx 单参函数;类型/arity 由 callruntime 复核如实报错)。
fn normalize_call_args(v: Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a,
        Value::Null => Vec::new(),
        other => vec![other],
    }
}

/// tween 帧推:t = min(elapsed/duration, 1);末帧钳制写最终值并移除。
fn advance_tweens(inst: &mut GraphInstance, scene: &mut Scene, dt: f64) {
    let mut done: Vec<usize> = Vec::new();
    for (i, tw) in inst.tweens.iter_mut().enumerate() {
        tw.elapsed += dt;
        let t = if tw.duration <= 0.0 { 1.0 } else { (tw.elapsed / tw.duration).min(1.0) };
        match tw.kind {
            TweenKind::Rotate { angle_deg, start_rot } => {
                let rot = quat_mul(start_rot, quat_y(angle_deg * t));
                if let Some(e) = scene.entity_mut(tw.target) {
                    e.transform.rotation = rot;
                }
            }
            TweenKind::Move { offset, start_pos } => {
                if let Some(e) = scene.entity_mut(tw.target) {
                    e.transform.translation = [
                        start_pos[0] + (offset[0] * t) as f32,
                        start_pos[1] + (offset[1] * t) as f32,
                        start_pos[2] + (offset[2] * t) as f32,
                    ];
                }
            }
        }
        if t >= 1.0 {
            done.push(i);
        }
    }
    for i in done.into_iter().rev() {
        inst.tweens.remove(i);
    }
}

/// transform JSON 对象应用(出现的字段才改)。
fn apply_transform_json(t: &mut Transform, v: &Value) {
    if let Some(a) = v.get("translation").and_then(|x| parse_vec3(x)) {
        t.translation = [a[0] as f32, a[1] as f32, a[2] as f32];
    }
    if let Some(arr) = v.get("rotation").and_then(Value::as_array) {
        if arr.len() == 4 && arr.iter().all(Value::is_number) {
            t.rotation = [
                arr[0].as_f64().unwrap() as f32,
                arr[1].as_f64().unwrap() as f32,
                arr[2].as_f64().unwrap() as f32,
                arr[3].as_f64().unwrap() as f32,
            ];
        }
    }
    if let Some(a) = v.get("scale").and_then(|x| parse_vec3(x)) {
        t.scale = [a[0] as f32, a[1] as f32, a[2] as f32];
    }
}

fn parse_vec3(v: &Value) -> Option<[f64; 3]> {
    let arr = v.as_array()?;
    if arr.len() == 3 && arr.iter().all(Value::is_number) {
        Some([
            arr[0].as_f64().unwrap(),
            arr[1].as_f64().unwrap(),
            arr[2].as_f64().unwrap(),
        ])
    } else {
        None
    }
}

/// 绕 Y 轴度制四元数(xyzw)。
fn quat_y(deg: f64) -> [f32; 4] {
    let half = deg.to_radians() / 2.0;
    [0.0, half.sin() as f32, 0.0, half.cos() as f32]
}

/// 四元数乘 a⊗b(xyzw;先 a 后 b 的局部复合)。
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let (ax, ay, az, aw) = (a[0], a[1], a[2], a[3]);
    let (bx, by, bz, bw) = (b[0], b[1], b[2], b[3]);
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

// ---------- Trigger 沿检测(D-F4-G) ----------

/// 场景 Trigger 实体 × 其他实体 AABB overlap(含边界):
/// Trigger AABB = translation ± extents/2;其他实体 AABB = translation ± |scale|/2。
fn detect_trigger_overlap(scene: &Scene) -> BTreeSet<(u64, u64)> {
    let mut out = BTreeSet::new();
    for t in &scene.entities {
        let Some(tr) = t.component("Trigger").filter(|c| c.enabled) else {
            continue;
        };
        if tr.props.get("kind").and_then(Value::as_str) != Some("box") {
            continue;
        }
        let Some(ext) = tr.props.get("extents").and_then(parse_vec3) else {
            continue;
        };
        let th = [ext[0].abs() / 2.0, ext[1].abs() / 2.0, ext[2].abs() / 2.0];
        let tc = t.transform.translation;
        for o in &scene.entities {
            if o.id == t.id {
                continue;
            }
            let oc = o.transform.translation;
            let oh = [
                f64::from(o.transform.scale[0].abs()) / 2.0,
                f64::from(o.transform.scale[1].abs()) / 2.0,
                f64::from(o.transform.scale[2].abs()) / 2.0,
            ];
            let overlap = (0..3).all(|i| {
                (f64::from(tc[i]) - f64::from(oc[i])).abs() <= th[i] + oh[i]
            });
            if overlap {
                out.insert((t.id, o.id));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_scene::{Component, Entity, Transform};
    use serde_json::json;

    const DT: f32 = 1.0 / 60.0;

    fn entity(id: u64, name: &str, translation: [f32; 3], components: Vec<Component>) -> Entity {
        Entity {
            id,
            name: name.into(),
            transform: Transform { translation, ..Transform::default() },
            components,
        }
    }

    fn tag(tag: &str) -> Component {
        Component::new("Tag", json!({ "tag": tag }))
    }

    fn trigger(extents: [f32; 3]) -> Component {
        Component::new("Trigger", json!({ "kind": "box", "extents": extents }))
    }

    fn doc(v: Value) -> GraphDoc {
        serde_json::from_value(v).unwrap()
    }

    fn names(log: &[(String, Value)]) -> Vec<&str> {
        log.iter().map(|(n, _)| n.as_str()).collect()
    }

    fn yaw_deg(rot: [f32; 4]) -> f64 {
        let (x, y, z, w) = (f64::from(rot[0]), f64::from(rot[1]), f64::from(rot[2]), f64::from(rot[3]));
        (2.0 * (w * y + x * z)).atan2(1.0 - 2.0 * (y * y + z * z)).to_degrees()
    }

    /// rotate_tween 逐帧推进 + 末帧钳制:90°/1.0s,60 帧后恰好 90°。
    #[test]
    fn rotate_tween_frames_then_exact_clamp() {
        let mut scene = Scene::new("t");
        scene.entities = vec![
            entity(1, "door", [0.0, 0.0, 0.0], vec![trigger([2.0, 2.0, 2.0])]),
            entity(2, "player", [0.0, 0.0, 0.0], vec![tag("player")]),
        ];
        let g = doc(json!({
            "version": 1, "id": "g_door", "name": "DoorOpener",
            "exposedProps": [ { "name": "openSpeed", "kind": "F32", "default": 90.0 } ],
            "nodes": [
                { "id": "n1", "type": "event.on_trigger_enter", "pos": [0, 0] },
                { "id": "n4", "type": "transform.rotate_tween", "pos": [1, 1],
                  "inputs": { "target": { "const": "$self" }, "angle": { "ref": "openSpeed" }, "duration": { "const": 1.0 } } }
            ],
            "edges": [ { "from": ["n1", "exec"], "to": ["n4", "exec"] } ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        assert_eq!(names(&log), ["logic.start"]);
        for _ in 0..60 {
            rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        }
        let rot = scene.entity(1).unwrap().transform.rotation;
        let yaw = yaw_deg(rot);
        assert!((yaw - 90.0).abs() < 0.01, "60 帧后 yaw 须恰为 90°,实际 {yaw}(quat {rot:?})");
        // trigger enter 恰好一次(tween 重启幂等,无重复 enter)。
        assert_eq!(log.iter().filter(|(n, _)| n == "logic.trigger").count(), 1);
    }

    /// F-GAME-4:sprite.*/animator.* 节点产出 AnimCommand(声明序;实体解析 $self/名称)。
    #[test]
    fn sprite_animator_nodes_emit_commands() {
        let mut scene = Scene::new("t");
        scene.entities = vec![
            entity(1, "hero", [0.0; 3], vec![]),
            entity(2, "zombie", [0.0; 3], vec![]),
        ];
        let g = doc(json!({
            "version": 1, "id": "g_anim", "name": "Anim",
            "nodes": [
                { "id": "s", "type": "event.on_start", "pos": [0, 0] },
                { "id": "p", "type": "sprite.play", "pos": [1, 0],
                  "inputs": { "entity": { "const": "$self" }, "clip": { "const": "walk" } } },
                { "id": "b", "type": "animator.set_bool", "pos": [2, 0],
                  "inputs": { "entity": { "const": "zombie" }, "param": { "const": "isMoving" }, "value": { "const": true } } },
                { "id": "t", "type": "animator.set_trigger", "pos": [3, 0],
                  "inputs": { "entity": { "const": 2 }, "param": { "const": "hit" } } },
                { "id": "f", "type": "sprite.set_frame", "pos": [4, 0],
                  "inputs": { "entity": { "const": "$self" }, "index": { "const": 3 } } },
                { "id": "st", "type": "sprite.stop", "pos": [5, 0],
                  "inputs": { "entity": { "const": "$self" } } }
            ],
            "edges": [
                { "from": ["s", "exec"], "to": ["p", "exec"] },
                { "from": ["p", "exec"], "to": ["b", "exec"] },
                { "from": ["b", "exec"], "to": ["t", "exec"] },
                { "from": ["t", "exec"], "to": ["f", "exec"] },
                { "from": ["f", "exec"], "to": ["st", "exec"] }
            ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        let cmds = rt.take_anim_commands();
        assert_eq!(
            cmds,
            vec![
                // restart 可选 pin 未接 → 缺省 false(幂等语义)。
                AnimCommand::Play { entity: 1, clip: "walk".into(), restart: false },
                AnimCommand::SetBool { entity: 2, param: "isMoving".into(), value: true },
                AnimCommand::SetTrigger { entity: 2, param: "hit".into() },
                AnimCommand::SetFrame { entity: 1, index: 3 },
                AnimCommand::Stop { entity: 1 },
            ],
            "命令须按声明序且实体解析正确"
        );
        // take 后清空;无节点执行则无命令。
        assert!(rt.take_anim_commands().is_empty());
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        assert!(rt.take_anim_commands().is_empty(), "无 on_update 挂链不应再产命令");
        // 校验器接受新节点(必填 pin 齐全)。
        assert!(names(&log).iter().all(|n| *n != "logic.unsupported"), "新节点不得报 unsupported: {log:?}");
    }

    /// timer_start → 到期 on_timer(0.05s @60Hz = 第 3 帧)。
    #[test]
    fn timer_fires_on_third_frame() {
        let mut scene = Scene::new("t");
        scene.entities = vec![entity(1, "e", [0.0; 3], vec![])];
        let g = doc(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "s", "type": "event.on_start", "pos": [0, 0] },
                { "id": "ts", "type": "flow.timer_start", "pos": [1, 0],
                  "inputs": { "timerId": { "const": "t1" }, "duration": { "const": 0.05 } } },
                { "id": "te", "type": "event.on_timer", "pos": [0, 1] },
                { "id": "dl", "type": "debug.log", "pos": [1, 1], "inputs": { "message": { "const": "fired" } } }
            ],
            "edges": [
                { "from": ["s", "exec"], "to": ["ts", "exec"] },
                { "from": ["te", "exec"], "to": ["dl", "exec"] }
            ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        assert!(!names(&log).contains(&"logic.timer"), "2 帧内不得触发: {:?}", names(&log));
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        let n = names(&log);
        let it = n.iter().position(|x| *x == "logic.timer").expect("第 3 帧须触发 timer");
        let il = n.iter().position(|x| *x == "logic.log").expect("on_timer 链须 debug.log");
        assert!(it < il, "logic.timer 先于链上 logic.log");
        assert_eq!(log[il].1["message"], json!("fired"));
        // 一次性:第 4 帧不再触发。
        let before = n.len();
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        assert_eq!(names(&log).len(), before);
    }

    /// message 帧末清空 + 同帧 on_message 派发。
    #[test]
    fn message_dispatched_same_frame_then_cleared() {
        let mut scene = Scene::new("t");
        scene.entities = vec![entity(1, "e", [0.0; 3], vec![])];
        let g = doc(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "i", "type": "event.on_input", "pos": [0, 0] },
                { "id": "sm", "type": "call.send_message", "pos": [1, 0],
                  "inputs": { "name": { "const": "m" }, "payload": { "const": 1 } } },
                { "id": "me", "type": "event.on_message", "pos": [0, 1] },
                { "id": "dl", "type": "debug.log", "pos": [1, 1],
                  "inputs": { "message": { "node": "me", "pin": "name" } } }
            ],
            "edges": [
                { "from": ["i", "exec"], "to": ["sm", "exec"] },
                { "from": ["me", "exec"], "to": ["dl", "exec"] }
            ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        log.clear();
        rt.frame(&mut scene, DT, vec![("jump".into(), 1.0)], vec![], &mut log);
        let n = names(&log);
        assert_eq!(n, ["logic.input", "logic.message", "logic.log"], "实际序: {n:?}");
        assert_eq!(log[2].1["message"], json!("m"));
        // 队列已清空:下帧无 message。
        log.clear();
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        assert!(names(&log).is_empty());
    }

    /// 规范序:input 先于 contact 先于 update(同帧)。
    #[test]
    fn canonical_order_input_contact_update() {
        let mut scene = Scene::new("t");
        scene.entities = vec![entity(1, "e", [0.0; 3], vec![]), entity(2, "o", [0.0; 3], vec![])];
        let g = doc(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "i", "type": "event.on_input", "pos": [0, 0] },
                { "id": "li", "type": "debug.log", "pos": [1, 0], "inputs": { "message": { "const": "i" } } },
                { "id": "c", "type": "event.on_contact_begin", "pos": [0, 1] },
                { "id": "lc", "type": "debug.log", "pos": [1, 1], "inputs": { "message": { "const": "c" } } },
                { "id": "u", "type": "event.on_update", "pos": [0, 2] },
                { "id": "lu", "type": "debug.log", "pos": [1, 2], "inputs": { "message": { "const": "u" } } }
            ],
            "edges": [
                { "from": ["i", "exec"], "to": ["li", "exec"] },
                { "from": ["c", "exec"], "to": ["lc", "exec"] },
                { "from": ["u", "exec"], "to": ["lu", "exec"] }
            ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        log.clear();
        rt.frame(
            &mut scene,
            DT,
            vec![("jump".into(), 1.0)],
            vec![LogicContact { a: 2, b: 1, phase: LogicPhase::Begin }],
            &mut log,
        );
        // 规范序:事件派发序 = input → contact → update(链上 logic.log 紧随各自事件)。
        let n: Vec<&str> = names(&log).into_iter().filter(|x| *x != "logic.log").collect();
        assert_eq!(n, ["logic.input", "logic.contact", "logic.update"], "规范序: {n:?}");
        // contact otherEntity = 对侧实体 2(contact 入参 a=2,b=1 → 目标实体 1 视角 other=2)。
        let contact = log.iter().find(|(n, _)| n == "logic.contact").expect("contact 事件须存在");
        assert_eq!(contact.1["otherEntity"], json!(2));
    }

    /// 跨实体按实体 id 升序派发 contact。
    #[test]
    fn contact_cross_entity_ascending() {
        let mut scene = Scene::new("t");
        scene.entities = vec![entity(5, "hi", [0.0; 3], vec![]), entity(3, "lo", [0.0; 3], vec![])];
        let mk = |gid: &str| {
            doc(json!({
                "version": 1, "id": gid, "name": gid,
                "nodes": [
                    { "id": "c", "type": "event.on_contact_begin", "pos": [0, 0] },
                    { "id": "l", "type": "debug.log", "pos": [1, 0], "inputs": { "message": { "const": "x" } } }
                ],
                "edges": [ { "from": ["c", "exec"], "to": ["l", "exec"] } ]
            }))
        };
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(5, mk("g5"), &json!({}), &mut scene, &mut log);
        rt.load(3, mk("g3"), &json!({}), &mut scene, &mut log);
        log.clear();
        rt.frame(&mut scene, DT, vec![], vec![LogicContact { a: 5, b: 3, phase: LogicPhase::Begin }], &mut log);
        let seq: Vec<u64> = log
            .iter()
            .filter(|(n, _)| n == "logic.contact")
            .map(|(_, v)| v["entityId"].as_u64().unwrap())
            .collect();
        assert_eq!(seq, [3, 5], "跨实体按实体 id 升序,与图加载序无关");
    }

    /// 黑板 var 三节点链:set → add → get。
    #[test]
    fn blackboard_set_add_get_chain() {
        let mut scene = Scene::new("t");
        scene.entities = vec![entity(1, "e", [0.0; 3], vec![])];
        let g = doc(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "s", "type": "event.on_start", "pos": [0, 0] },
                { "id": "vs", "type": "var.set", "pos": [1, 0],
                  "inputs": { "name": { "const": "x" }, "value": { "const": 1 } } },
                { "id": "va", "type": "var.add", "pos": [2, 0],
                  "inputs": { "name": { "const": "x" }, "value": { "const": 2 } } },
                { "id": "dl", "type": "debug.log", "pos": [3, 0],
                  "inputs": { "message": { "node": "vg", "pin": "out" } } },
                { "id": "vg", "type": "var.get", "pos": [2, 1], "inputs": { "name": { "const": "x" } } }
            ],
            "edges": [
                { "from": ["s", "exec"], "to": ["vs", "exec"] },
                { "from": ["vs", "exec"], "to": ["va", "exec"] },
                { "from": ["va", "exec"], "to": ["dl", "exec"] }
            ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        assert_eq!(rt.debug_var(1, "x"), Some(json!(3.0)));
        let ll = log.iter().find(|(n, _)| n == "logic.log").expect("debug.log 须出现");
        assert_eq!(ll.1["message"], json!(3.0), "var.get 读出 1+2=3");
    }

    #[test]
    fn runtime_input_vector_composes_a_transform_without_losing_scale() {
        let mut scene = Scene::new("dynamic-transform");
        scene.entities = vec![entity(1, "actor", [0.; 3], vec![])];
        let g = doc(json!({"version":1,"id":"move","name":"move","nodes":[
            {"id":"input","type":"event.on_input","pos":[0,0]},
            {"id":"v","type":"math.vec3","pos":[1,0],"inputs":{
                "x":{"node":"input","pin":"value"},"y":{"const":2.0},"z":{"const":0.0}}},
            {"id":"t","type":"transform.compose","pos":[2,0],"inputs":{
                "translation":{"node":"v","pin":"out"},"scale":{"const":[2.0,3.0,1.0]}}},
            {"id":"set","type":"entity.set_transform","pos":[3,0],"inputs":{
                "entity":{"const":"$self"},"transform":{"node":"t","pin":"out"}}}
        ],"edges":[{"from":["input","exec"],"to":["set","exec"]}]}));
        let mut rt=LogicRuntime::new(); let mut log=Vec::new();
        rt.load(1,g,&json!({}),&mut scene,&mut log);
        rt.frame(&mut scene,DT,vec![("move".into(),7.5)],vec![],&mut log);
        assert_eq!(scene.entity(1).unwrap().transform.translation,[7.5,2.,0.]);
        assert_eq!(scene.entity(1).unwrap().transform.scale,[2.,3.,1.]);
        assert!(!log.iter().any(|(kind,_)|kind=="logic.unsupported"));
    }

    /// 热重载:on_start 重发 + 黑板重置 + props 覆盖生效。
    #[test]
    fn reload_restarts_and_resets_blackboard() {
        let mut scene = Scene::new("t");
        scene.entities = vec![entity(1, "e", [0.0; 3], vec![])];
        let mk = || {
            doc(json!({
                "version": 1, "id": "g", "name": "g",
                "exposedProps": [ { "name": "v", "kind": "F32", "default": 1.0 } ],
                "nodes": [
                    { "id": "s", "type": "event.on_start", "pos": [0, 0] },
                    { "id": "vs", "type": "var.set", "pos": [1, 0],
                      "inputs": { "name": { "const": "x" }, "value": { "ref": "v" } } }
                ],
                "edges": [ { "from": ["s", "exec"], "to": ["vs", "exec"] } ]
            }))
        };
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, mk(), &json!({}), &mut scene, &mut log);
        assert_eq!(rt.debug_var(1, "x"), Some(json!(1.0)));
        assert_eq!(log.iter().filter(|(n, _)| n == "logic.start").count(), 1);
        // 改黑板后 reload:props 覆盖 41 → on_start 重发,x 重置为 41。
        rt.graphs[0].1.vars.insert("y".into(), json!(9));
        rt.reload(1, mk(), &json!({ "v": 41.0 }), &mut scene, &mut log);
        assert_eq!(log.iter().filter(|(n, _)| n == "logic.start").count(), 2, "on_start 须重发");
        assert_eq!(rt.debug_var(1, "x"), Some(json!(41.0)), "props 覆盖须生效");
        assert_eq!(rt.debug_var(1, "y"), None, "黑板须重置");
        assert_eq!(rt.graph_count(), 1, "reload 重建而非叠加");
    }

    /// trigger 沿 enter/exit 差集 + has_tag 门控(非 player 不开门)。
    #[test]
    fn trigger_enter_exit_diff_and_tag_gate() {
        let mut scene = Scene::new("t");
        scene.entities = vec![
            entity(1, "door", [0.0, 0.0, 0.0], vec![trigger([2.0, 2.0, 2.0])]),
            entity(2, "player", [10.0, 0.0, 0.0], vec![tag("player")]),
            entity(3, "crate", [10.0, 0.0, 0.0], vec![]),
        ];
        let g = doc(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "n1", "type": "event.on_trigger_enter", "pos": [0, 0] },
                { "id": "n2", "type": "flow.branch", "pos": [1, 0],
                  "inputs": { "condition": { "node": "n3", "pin": "out" } } },
                { "id": "n3", "type": "entity.has_tag", "pos": [0, 1],
                  "inputs": { "entity": { "node": "n1", "pin": "otherEntity" }, "tag": { "const": "player" } } },
                { "id": "n4", "type": "debug.log", "pos": [2, 0], "inputs": { "message": { "const": "open" } } },
                { "id": "n5", "type": "event.on_trigger_exit", "pos": [0, 2] },
                { "id": "n6", "type": "debug.log", "pos": [1, 2], "inputs": { "message": { "const": "bye" } } }
            ],
            "edges": [
                { "from": ["n1", "exec"], "to": ["n2", "exec"] },
                { "from": ["n2", "then"], "to": ["n4", "exec"] },
                { "from": ["n5", "exec"], "to": ["n6", "exec"] }
            ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        // 无 tag 实体进入:enter 派发但 branch else,无 open。
        scene.entity_mut(3).unwrap().transform.translation = [0.0, 0.0, 0.0];
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        assert!(log.iter().any(|(n, v)| n == "logic.trigger" && v["phase"] == "enter" && v["otherEntity"] == 3));
        assert!(!log.iter().any(|(n, v)| n == "logic.log" && v["message"] == "open"), "无 tag 不得开门");
        // player 进入:开门。
        log.clear();
        scene.entity_mut(2).unwrap().transform.translation = [0.0, 0.0, 0.0];
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        assert!(log.iter().any(|(n, v)| n == "logic.log" && v["message"] == "open"), "player 须开门");
        // player 离开:exit 差集。
        log.clear();
        scene.entity_mut(2).unwrap().transform.translation = [10.0, 0.0, 0.0];
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        let n = names(&log);
        assert_eq!(n, ["logic.trigger", "logic.log"], "exit 派发序: {n:?}");
        assert_eq!(log[0].1["phase"], json!("exit"));
        assert_eq!(log[1].1["message"], json!("bye"));
    }

    /// flow.delay 帧推:0.05s @60Hz 第 3 帧 update 段恢复链。
    #[test]
    fn delay_resumes_after_three_frames() {
        let mut scene = Scene::new("t");
        scene.entities = vec![entity(1, "e", [0.0; 3], vec![])];
        let g = doc(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "s", "type": "event.on_start", "pos": [0, 0] },
                { "id": "d", "type": "flow.delay", "pos": [1, 0], "inputs": { "duration": { "const": 0.05 } } },
                { "id": "l", "type": "debug.log", "pos": [2, 0], "inputs": { "message": { "const": "after" } } }
            ],
            "edges": [
                { "from": ["s", "exec"], "to": ["d", "exec"] },
                { "from": ["d", "exec"], "to": ["l", "exec"] }
            ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        assert!(!names(&log).contains(&"logic.log"), "delay 当帧不续链");
        for i in 0..2 {
            rt.frame(&mut scene, DT, vec![], vec![], &mut log);
            assert!(!names(&log).contains(&"logic.log"), "第 {} 帧不得恢复", i + 1);
        }
        rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        assert!(log.iter().any(|(n, v)| n == "logic.log" && v["message"] == "after"), "第 3 帧须恢复");
    }

    /// 未实现节点如实 logic.unsupported 且链继续。
    #[test]
    fn unsupported_node_logged_and_chain_continues() {
        let mut scene = Scene::new("t");
        scene.entities = vec![entity(1, "e", [0.0; 3], vec![])];
        let g = doc(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "s", "type": "event.on_start", "pos": [0, 0] },
                { "id": "p", "type": "physics.apply_impulse", "pos": [1, 0],
                  "inputs": { "entity": { "const": "$self" }, "impulse": { "const": [0, 1, 0] } } },
                { "id": "l", "type": "debug.log", "pos": [2, 0], "inputs": { "message": { "const": "tail" } } }
            ],
            "edges": [
                { "from": ["s", "exec"], "to": ["p", "exec"] },
                { "from": ["p", "exec"], "to": ["l", "exec"] }
            ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        let un = log.iter().find(|(n, _)| n == "logic.unsupported").expect("须如实报 unsupported");
        assert_eq!(un.1["nodeType"], json!("physics.apply_impulse"));
        assert!(log.iter().any(|(n, v)| n == "logic.log" && v["message"] == "tail"), "unsupported 后链须继续");
    }

    /// move_tween 末帧钳制 + set/get_transform 往返。
    #[test]
    fn move_tween_and_transform_roundtrip() {
        let mut scene = Scene::new("t");
        scene.entities = vec![
            entity(1, "door", [0.0, 0.0, 0.0], vec![trigger([4.0, 4.0, 4.0])]),
            entity(2, "player", [0.0, 0.0, 0.0], vec![]),
        ];
        let g = doc(json!({
            "version": 1, "id": "g", "name": "g",
            "nodes": [
                { "id": "n1", "type": "event.on_trigger_enter", "pos": [0, 0] },
                { "id": "mt", "type": "transform.move_tween", "pos": [1, 0],
                  "inputs": { "target": { "const": "$self" }, "offset": { "const": [0, 3, 0] }, "duration": { "const": 0.5 } } },
                { "id": "st", "type": "entity.set_transform", "pos": [2, 0],
                  "inputs": { "entity": { "const": "$self" }, "transform": { "node": "gt", "pin": "transform" } } },
                { "id": "gt", "type": "entity.get_transform", "pos": [1, 1],
                  "inputs": { "entity": { "const": "$self" } } }
            ],
            "edges": [
                { "from": ["n1", "exec"], "to": ["mt", "exec"] },
                { "from": ["mt", "exec"], "to": ["st", "exec"] }
            ]
        }));
        let mut rt = LogicRuntime::new();
        let mut log = Vec::new();
        rt.load(1, g, &json!({}), &mut scene, &mut log);
        for _ in 0..30 {
            rt.frame(&mut scene, DT, vec![], vec![], &mut log);
        }
        let t = scene.entity(1).unwrap().transform.translation;
        assert!((t[1] - 3.0).abs() < 1e-4, "30 帧(0.5s)后 y 须恰为 3,实际 {t:?}");
    }

    /// call_function args 归一(F6 wave.2):数组直用 / Null 空 / 标量包一元 / 对象亦包一元(类型由 callruntime 复核)。
    #[test]
    fn call_args_scalar_autowrap() {
        assert_eq!(normalize_call_args(json!([2, 3.5])), vec![json!(2), json!(3.5)]);
        assert!(normalize_call_args(Value::Null).is_empty());
        assert_eq!(normalize_call_args(json!(34)), vec![json!(34)]);
        assert_eq!(normalize_call_args(json!(true)), vec![json!(true)]);
    }
}
