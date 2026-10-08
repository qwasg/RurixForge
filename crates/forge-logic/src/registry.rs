//! 节点类型注册表(10 §4.2 首发冻结子集,照抄不改):schema 单源,机制同组件注册表
//! (09 §3.1)——驱动 UI 节点面板、graph_validate 校验、agent 生成约束。

/// pin 类型(执行引脚 + 八类数据引脚)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinType {
    Exec,
    Bool,
    I32,
    F32,
    String,
    Vec3,
    Entity,
    Transform,
    Any,
}

impl PinType {
    pub fn name(self) -> &'static str {
        match self {
            PinType::Exec => "Exec",
            PinType::Bool => "Bool",
            PinType::I32 => "I32",
            PinType::F32 => "F32",
            PinType::String => "String",
            PinType::Vec3 => "Vec3",
            PinType::Entity => "Entity",
            PinType::Transform => "Transform",
            PinType::Any => "Any",
        }
    }
}

/// 节点行为类别:Event(事件入口)/ Flow(流控)/ Action(动作,执行进+执行出)/ Pure(纯数据)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Event,
    Flow,
    Action,
    Pure,
}

/// 数据 pin 简表(inputs 带 required;outputs 恒 required=false)。
pub struct PinSpec {
    pub name: &'static str,
    pub ty: PinType,
    pub required: bool,
}

/// 节点类型注册项。
pub struct NodeSpec {
    pub ntype: &'static str,
    pub kind: NodeKind,
    /// 是否有执行入口 pin("exec")。
    pub exec_in: bool,
    /// 执行出口 pin 名表(如 branch 的 then/else;空 = 无执行出口)。
    pub exec_out: &'static [&'static str],
    pub inputs: &'static [PinSpec],
    pub outputs: &'static [PinSpec],
}

const EXEC: &[&str] = &["exec"];

const fn inp(name: &'static str, ty: PinType) -> PinSpec {
    PinSpec { name, ty, required: true }
}

/// 可选输入 pin(未接 = 缺省语义,由解释器给缺省值;F-GAME-4 sprite.play.restart)。
const fn opt(name: &'static str, ty: PinType) -> PinSpec {
    PinSpec { name, ty, required: false }
}

const fn out(name: &'static str, ty: PinType) -> PinSpec {
    PinSpec { name, ty, required: false }
}

/// 九族首发冻结子集(10 §4.2;事件数据输出按 10 §3.1 负载)。
pub const REGISTRY: &[NodeSpec] = &[
    NodeSpec { ntype: "call.native_frame", kind: NodeKind::Action, exec_in: true, exec_out: EXEC,
        inputs: &[inp("module", PinType::String), inp("fn", PinType::String), inp("dt", PinType::F32), inp("bindings", PinType::Any), opt("animatorParam", PinType::String)], outputs: &[] },
    // Data constructors keep native script outputs composable in scene graphs.
    NodeSpec { ntype: "math.vec3", kind: NodeKind::Pure, exec_in: false, exec_out: &[], inputs: &[inp("x", PinType::F32), inp("y", PinType::F32), inp("z", PinType::F32)], outputs: &[out("out", PinType::Vec3)] },
    NodeSpec { ntype: "transform.compose", kind: NodeKind::Pure, exec_in: false, exec_out: &[], inputs: &[inp("translation", PinType::Vec3), opt("scale", PinType::Vec3)], outputs: &[out("out", PinType::Transform)] },
    // ---- event.*(§3.1 全部事件入口;每图每事件至多一个,validate 强制)----
    NodeSpec { ntype: "event.on_start", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[] },
    NodeSpec { ntype: "event.on_update", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[out("dt", PinType::F32)] },
    NodeSpec { ntype: "event.on_contact_begin", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[out("otherEntity", PinType::Entity)] },
    NodeSpec { ntype: "event.on_contact_persist", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[out("otherEntity", PinType::Entity)] },
    NodeSpec { ntype: "event.on_contact_end", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[out("otherEntity", PinType::Entity)] },
    NodeSpec { ntype: "event.on_trigger_enter", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[out("otherEntity", PinType::Entity)] },
    NodeSpec { ntype: "event.on_trigger_exit", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[] },
    NodeSpec { ntype: "event.on_input", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[out("action", PinType::String), out("value", PinType::F32)] },
    NodeSpec { ntype: "event.on_message", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[out("name", PinType::String), out("payload", PinType::Any)] },
    NodeSpec { ntype: "event.on_timer", kind: NodeKind::Event, exec_in: false, exec_out: EXEC, inputs: &[], outputs: &[out("timerId", PinType::String)] },
    // ---- flow.* ----
    NodeSpec { ntype: "flow.branch", kind: NodeKind::Flow, exec_in: true, exec_out: &["then", "else"], inputs: &[inp("condition", PinType::Bool)], outputs: &[] },
    NodeSpec { ntype: "flow.sequence", kind: NodeKind::Flow, exec_in: true, exec_out: &["seq0", "seq1"], inputs: &[], outputs: &[] },
    NodeSpec { ntype: "flow.for_each", kind: NodeKind::Flow, exec_in: true, exec_out: &["loop", "done"], inputs: &[inp("collection", PinType::Any)], outputs: &[out("item", PinType::Any), out("index", PinType::I32)] },
    NodeSpec { ntype: "flow.delay", kind: NodeKind::Flow, exec_in: true, exec_out: EXEC, inputs: &[inp("duration", PinType::F32)], outputs: &[] },
    NodeSpec { ntype: "flow.timer_start", kind: NodeKind::Flow, exec_in: true, exec_out: EXEC, inputs: &[inp("timerId", PinType::String), inp("duration", PinType::F32)], outputs: &[] },
    NodeSpec { ntype: "flow.timer_cancel", kind: NodeKind::Flow, exec_in: true, exec_out: EXEC, inputs: &[inp("timerId", PinType::String)], outputs: &[] },
    NodeSpec { ntype: "flow.gate", kind: NodeKind::Flow, exec_in: true, exec_out: EXEC, inputs: &[inp("open", PinType::Bool)], outputs: &[] },
    // ---- entity.* ----
    NodeSpec { ntype: "entity.get_transform", kind: NodeKind::Pure, exec_in: false, exec_out: &[], inputs: &[inp("entity", PinType::Entity)], outputs: &[out("transform", PinType::Transform)] },
    NodeSpec { ntype: "entity.set_transform", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity), inp("transform", PinType::Transform)], outputs: &[] },
    NodeSpec { ntype: "entity.spawn", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("prefabRef", PinType::String)], outputs: &[out("entity", PinType::Entity)] },
    NodeSpec { ntype: "entity.destroy", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity)], outputs: &[] },
    // has_tag 输出 pin 名照 10 §4.1 示例为 "out"。
    NodeSpec { ntype: "entity.has_tag", kind: NodeKind::Pure, exec_in: false, exec_out: &[], inputs: &[inp("entity", PinType::Entity), inp("tag", PinType::String)], outputs: &[out("out", PinType::Bool)] },
    NodeSpec { ntype: "entity.add_tag", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity), inp("tag", PinType::String)], outputs: &[] },
    NodeSpec { ntype: "entity.find_by_tag", kind: NodeKind::Pure, exec_in: false, exec_out: &[], inputs: &[inp("tag", PinType::String)], outputs: &[out("entity", PinType::Entity)] },
    // ---- transform.* ----
    NodeSpec { ntype: "transform.move_tween", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("target", PinType::Entity), inp("offset", PinType::Vec3), inp("duration", PinType::F32)], outputs: &[] },
    NodeSpec { ntype: "transform.rotate_tween", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("target", PinType::Entity), inp("angle", PinType::F32), inp("duration", PinType::F32)], outputs: &[] },
    NodeSpec { ntype: "transform.look_at", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity), inp("target", PinType::Vec3)], outputs: &[] },
    NodeSpec { ntype: "transform.lerp", kind: NodeKind::Pure, exec_in: false, exec_out: &[], inputs: &[inp("a", PinType::F32), inp("b", PinType::F32), inp("t", PinType::F32)], outputs: &[out("out", PinType::F32)] },
    // ---- physics.* ----
    NodeSpec { ntype: "physics.cast_ray", kind: NodeKind::Pure, exec_in: false, exec_out: &[], inputs: &[inp("origin", PinType::Vec3), inp("dir", PinType::Vec3)], outputs: &[out("hit", PinType::Bool), out("entity", PinType::Entity)] },
    NodeSpec { ntype: "physics.apply_impulse", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity), inp("impulse", PinType::Vec3)], outputs: &[] },
    NodeSpec { ntype: "physics.overlap", kind: NodeKind::Pure, exec_in: false, exec_out: &[], inputs: &[inp("entity", PinType::Entity)], outputs: &[out("entities", PinType::Any)] },
    // ---- audio.*(占位,10 §4.2)----
    NodeSpec { ntype: "audio.play", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("sound", PinType::String)], outputs: &[] },
    NodeSpec { ntype: "audio.stop", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("sound", PinType::String)], outputs: &[] },
    // ---- var.*(图内黑板变量)----
    NodeSpec { ntype: "var.get", kind: NodeKind::Pure, exec_in: false, exec_out: &[], inputs: &[inp("name", PinType::String)], outputs: &[out("out", PinType::Any)] },
    NodeSpec { ntype: "var.set", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("name", PinType::String), inp("value", PinType::Any)], outputs: &[] },
    NodeSpec { ntype: "var.add", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("name", PinType::String), inp("value", PinType::F32)], outputs: &[] },
    // ---- call.*(§4.3 与 .rx 互绑;RD-F4-004:call_function result pin 加性扩展 D-RD4-D,40 节点集不变)----
    NodeSpec { ntype: "call.call_function", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("module", PinType::String), inp("fn", PinType::String), inp("args", PinType::Any)], outputs: &[out("result", PinType::Any)] },
    NodeSpec { ntype: "call.send_message", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("name", PinType::String), inp("payload", PinType::Any)], outputs: &[] },
    // ---- debug.*(编辑态)----
    NodeSpec { ntype: "debug.log", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("message", PinType::String)], outputs: &[] },
    NodeSpec { ntype: "debug.draw_debug_line", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("a", PinType::Vec3), inp("b", PinType::Vec3)], outputs: &[] },
    // ---- sprite.* / animator.*(F-GAME-4 帧动画,加性扩展 40→45;10 §4.2 Errata)----
    // sprite.play:restart 可选缺省 false = 幂等(同 clip 重复调用 no-op,防每帧重启冻帧坑)。
    NodeSpec { ntype: "sprite.play", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity), inp("clip", PinType::String), opt("restart", PinType::Bool)], outputs: &[] },
    NodeSpec { ntype: "sprite.stop", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity)], outputs: &[] },
    NodeSpec { ntype: "sprite.set_frame", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity), inp("index", PinType::I32)], outputs: &[] },
    NodeSpec { ntype: "animator.set_bool", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity), inp("param", PinType::String), inp("value", PinType::Bool)], outputs: &[] },
    NodeSpec { ntype: "animator.set_trigger", kind: NodeKind::Action, exec_in: true, exec_out: EXEC, inputs: &[inp("entity", PinType::Entity), inp("param", PinType::String)], outputs: &[] },
];

/// 按类型名查注册项。
pub fn find_spec(ntype: &str) -> Option<&'static NodeSpec> {
    REGISTRY.iter().find(|s| s.ntype == ntype)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_nine_families_frozen_subset() {
        // 10 §4.2 首发冻结子集 40 + F-GAME-4 加性扩展(sprite 3 + animator 2)= 45。
        assert_eq!(REGISTRY.len(), 48);
        for t in [
            "event.on_start", "event.on_update", "event.on_contact_begin", "event.on_contact_persist",
            "event.on_contact_end", "event.on_trigger_enter", "event.on_trigger_exit", "event.on_input",
            "event.on_message", "event.on_timer",
            "flow.branch", "flow.sequence", "flow.for_each", "flow.delay", "flow.timer_start",
            "flow.timer_cancel", "flow.gate",
            "entity.get_transform", "entity.set_transform", "entity.spawn", "entity.destroy",
            "entity.has_tag", "entity.add_tag", "entity.find_by_tag",
            "transform.move_tween", "transform.rotate_tween", "transform.look_at", "transform.lerp",
            "physics.cast_ray", "physics.apply_impulse", "physics.overlap",
            "audio.play", "audio.stop",
            "var.get", "var.set", "var.add",
            "call.call_function", "call.send_message",
            "debug.log", "debug.draw_debug_line",
            "sprite.play", "sprite.stop", "sprite.set_frame",
            "animator.set_bool", "animator.set_trigger",
        ] {
            assert!(find_spec(t).is_some(), "注册表缺 {t}");
        }
    }

    #[test]
    fn sprite_animator_nodes_pins() {
        // F-GAME-4:sprite.play restart 可选(缺省幂等);其余 pin 必填。
        let p = find_spec("sprite.play").unwrap();
        assert_eq!(p.kind, NodeKind::Action);
        assert!(p.exec_in);
        assert_eq!(p.exec_out, &["exec"]);
        assert_eq!(p.inputs.len(), 3);
        assert!(p.inputs[0].required && p.inputs[1].required);
        assert!(!p.inputs[2].required, "restart 须为可选 pin");
        assert_eq!(p.inputs[2].name, "restart");
        let sb = find_spec("animator.set_bool").unwrap();
        assert_eq!(sb.inputs[1].ty, PinType::String);
        assert_eq!(sb.inputs[2].ty, PinType::Bool);
        let sf = find_spec("sprite.set_frame").unwrap();
        assert_eq!(sf.inputs[1].ty, PinType::I32);
    }

    #[test]
    fn registry_spot_check_pins() {
        let b = find_spec("flow.branch").unwrap();
        assert_eq!(b.kind, NodeKind::Flow);
        assert!(b.exec_in);
        assert_eq!(b.exec_out, &["then", "else"]);
        assert_eq!(b.inputs[0].name, "condition");
        assert_eq!(b.inputs[0].ty, PinType::Bool);
        assert!(b.inputs[0].required);

        let h = find_spec("entity.has_tag").unwrap();
        assert_eq!(h.kind, NodeKind::Pure);
        assert!(!h.exec_in && h.exec_out.is_empty());
        assert_eq!(h.outputs[0].name, "out", "10 §4.1 示例输出 pin 名为 out");

        let e = find_spec("event.on_trigger_enter").unwrap();
        assert_eq!(e.kind, NodeKind::Event);
        assert_eq!(e.exec_out, &["exec"]);
        assert_eq!(e.outputs[0].name, "otherEntity");
        assert_eq!(e.outputs[0].ty, PinType::Entity);
    }
}
