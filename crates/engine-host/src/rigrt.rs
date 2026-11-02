//! F-GAME-5(D-050):宿主侧 Rig2D 运行时(PIE 语义,对位 anim.rs 的 AnimSystem)。
//!
//! 职责边界(契约 D):
//! - 仅作用于 run_scene(play 态);play.enter / play.exit 清空,编辑态零介入(编辑态渲染 =
//!   `pose_from(props.params, props.clip, props.time)` 纯函数,摆姿势写 params);
//! - 每实体一个 `RigSim`(首次见到时创建;rig GUID 变或文档被改 = Arc 身份变 → 重建);
//!   `params` 起始为空(编辑态摆姿势不进入运行态),`authored_clip` = props.clip,seed = 实体 id;
//! - 组件 `Rig2D.params/clip/time` 的**唯一写者**:每固定步 `sim.step(DT_FIXED)` 后回写完整参数快照、
//!   clip、time;图节点只发 `AnimCommand`;
//! - `animator.set_bool/set_trigger` 由 [`route`] 按目标实体组件分流(带 Rig2D → 本系统);
//! - 事件 `rig.warn {entityId, reason}`:命令被忽略、未知参数/clip、目标实体无 Rig2D、文档不可解析。

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use forge_logic::interp::AnimCommand;
use forge_rig2d::sim::SimOptions;
use forge_rig2d::{CompiledRig, RigCommand, RigSim};
use forge_scene::Scene;
use serde_json::{json, Map, Value};

use crate::render_core::assets::rig_doc_cached;

/// 文档解析器(测试注入内存文档;生产走 `rig_doc_cached`)。
pub type RigResolver<'a> = &'a dyn Fn(&str) -> Option<Arc<CompiledRig>>;

struct EntityRig {
    guid: String,
    rig: Arc<CompiledRig>,
    sim: RigSim,
}

/// 宿主 rig 系统(HostState 持有;单写者)。
#[derive(Default)]
pub struct RigSystem {
    states: HashMap<u64, EntityRig>,
    /// 已告警过「文档不可解析」的 (实体, GUID)——同一坏引用每 play 会话只报一次,不逐帧刷屏。
    unresolved_warned: BTreeSet<(u64, String)>,
}

impl std::fmt::Debug for RigSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RigSystem").field("entities", &self.states.len()).finish()
    }
}

fn warn(logs: &mut Vec<(String, Value)>, eid: u64, reason: impl Into<String>) {
    logs.push(("rig.warn".into(), json!({ "entityId": eid, "reason": reason.into() })));
}

/// 实体是否带启用的 Rig2D(分流判据;rig GUID 是否为空不影响归属)。
fn has_rig2d(scene: &Scene, eid: u64) -> bool {
    scene
        .entity(eid)
        .is_some_and(|e| e.components.iter().any(|c| c.ctype == "Rig2D" && c.enabled))
}

/// AnimCommand 分流(契约 E):rig.* → RigSystem;animator.set_bool/set_trigger 目标带 Rig2D → RigSystem;
/// 其余 → AnimSystem。返回 (anim, rig),组内保持声明序。
pub fn route(scene: &Scene, cmds: Vec<AnimCommand>) -> (Vec<AnimCommand>, Vec<AnimCommand>) {
    let (mut anim, mut rig) = (Vec::new(), Vec::new());
    for c in cmds {
        let to_rig = c.is_rig()
            || (matches!(c, AnimCommand::SetBool { .. } | AnimCommand::SetTrigger { .. }) && has_rig2d(scene, c.entity()));
        if to_rig { rig.push(c) } else { anim.push(c) }
    }
    (anim, rig)
}

fn to_rig_command(c: AnimCommand) -> Option<RigCommand> {
    Some(match c {
        AnimCommand::RigPlay { clip, restart, .. } => RigCommand::Play { clip, restart },
        AnimCommand::RigStop { .. } => RigCommand::Stop,
        AnimCommand::RigSetParam { param, value, blend, .. } => RigCommand::SetParam { param, value, blend },
        AnimCommand::SetBool { param, value, .. } => RigCommand::SetBool { param, value },
        AnimCommand::SetTrigger { param, .. } => RigCommand::SetTrigger { param },
        AnimCommand::Play { .. } | AnimCommand::Stop { .. } | AnimCommand::SetFrame { .. } => return None,
    })
}

fn sim_options(props: &Value) -> SimOptions {
    let b = |k: &str| props.get(k).and_then(Value::as_bool).unwrap_or(true);
    SimOptions {
        auto_blink: b("autoBlink"),
        auto_breath: b("autoBreath"),
        physics: b("physics"),
        speed: props.get("speed").and_then(Value::as_f64).unwrap_or(1.0) as f32,
    }
}

impl RigSystem {
    /// play.exit / play.enter(及回滚)时清空。
    pub fn clear(&mut self) {
        self.states.clear();
        self.unresolved_warned.clear();
    }

    /// 测试 / 内省:实体当前 (clip, time, playing, fsm_state)。
    pub fn debug_state(&self, entity: u64) -> Option<(String, f32, bool, Option<String>)> {
        self.states
            .get(&entity)
            .map(|s| (s.sim.clip.clone(), s.sim.time, s.sim.playing, s.sim.fsm_state.clone()))
    }

    /// 每固定步推进(advance_frame 在 AnimSystem 之后调用);返回是否有可视变更。
    pub fn advance(&mut self, scene: &mut Scene, cmds: Vec<AnimCommand>, dt: f32, logs: &mut Vec<(String, Value)>) -> bool {
        self.advance_with(scene, cmds, dt, logs, &|guid| rig_doc_cached(guid))
    }

    /// 解析器可注入版(测试直供文档,免全局缓存 / env 依赖)。
    pub fn advance_with(
        &mut self,
        scene: &mut Scene,
        cmds: Vec<AnimCommand>,
        dt: f32,
        logs: &mut Vec<(String, Value)>,
        resolve: RigResolver<'_>,
    ) -> bool {
        let mut cmd_map: HashMap<u64, Vec<AnimCommand>> = HashMap::new();
        for c in cmds {
            cmd_map.entry(c.entity()).or_default().push(c);
        }

        let mut changed = false;
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        // 实体 id 升序(确定性);索引在本函数内不变(不增删实体)。
        let mut ids: Vec<(u64, usize)> = scene
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| e.components.iter().any(|c| c.ctype == "Rig2D" && c.enabled))
            .map(|(i, e)| (e.id, i))
            .collect();
        ids.sort_unstable_by_key(|(id, _)| *id);
        for (eid, index) in ids {
            let Some(comp) = scene.entities[index].components.iter().find(|c| c.ctype == "Rig2D" && c.enabled) else {
                continue;
            };
            let props = comp.props.clone();
            let guid = props.get("rig").and_then(Value::as_str).unwrap_or("").to_string();
            if guid.is_empty() {
                if cmd_map.remove(&eid).is_some_and(|c| !c.is_empty()) {
                    warn(logs, eid, "Rig2D.rig 为空,rig 命令被忽略");
                }
                continue;
            }
            let Some(rig) = resolve(&guid) else {
                if self.unresolved_warned.insert((eid, guid.clone())) {
                    warn(logs, eid, format!(".rxrig {guid} 不可解析(缺文件或文档非法),实体不参与 rig 运行时"));
                }
                cmd_map.remove(&eid);
                continue;
            };
            seen.insert(eid);

            // 初始化 / 换 rig / 文档被改(Arc 身份变)→ 重建 RigSim。
            let stale = self
                .states
                .get(&eid)
                .is_none_or(|s| s.guid != guid || !Arc::ptr_eq(&s.rig, &rig));
            if stale {
                let authored_clip = props.get("clip").and_then(Value::as_str).unwrap_or("");
                let mut sim = RigSim::new(&rig, authored_clip, eid, sim_options(&props));
                if !authored_clip.is_empty() && !rig.doc.clips.contains_key(authored_clip) {
                    warn(logs, eid, format!(".rxrig 无 clip {authored_clip}"));
                }
                if props.get("playing").and_then(Value::as_bool) == Some(false) && !sim.is_fsm() {
                    sim.playing = false;
                }
                if rig.doc.parts.iter().any(|p| p.blend != forge_rig2d::doc::PartBlend::Alpha) {
                    warn(logs, eid, "v1 渲染统一按 alpha 混合,部件 blend=additive/multiply 未生效");
                }
                self.states.insert(eid, EntityRig { guid: guid.clone(), rig: rig.clone(), sim });
            }
            let st = self.states.get_mut(&eid).expect("刚插入");
            // 运行中可改的开关(speed / autoBlink / autoBreath / physics)逐步跟随 props。
            st.sim.opts = sim_options(&props);

            for c in cmd_map.remove(&eid).unwrap_or_default() {
                let Some(rc) = to_rig_command(c) else {
                    warn(logs, eid, "sprite.* 命令目标为 Rig2D 实体,已忽略(请用 rig.play/rig.stop/rig.set_param)");
                    continue;
                };
                if let Err(w) = st.sim.apply(&st.rig, rc) {
                    warn(logs, eid, w.0);
                }
            }

            let pose = st.sim.step(&st.rig, dt);
            let params: Map<String, Value> = pose
                .param_map(&st.rig)
                .into_iter()
                .map(|(k, v)| (k, json!(v as f64)))
                .collect();
            let (clip_v, time_v) = (Value::String(st.sim.clip.clone()), json!(st.sim.time as f64));

            // 回写(唯一写者;渲染 / 点选 / 流媒体自然拾取)。
            if let Some(c) = scene.entities[index].component_mut("Rig2D") {
                if let Some(obj) = c.props.as_object_mut() {
                    let new_params = Value::Object(params);
                    for (k, v) in [("params", new_params), ("clip", clip_v), ("time", time_v)] {
                        if obj.get(k) != Some(&v) {
                            obj.insert(k.into(), v);
                            changed = true;
                        }
                    }
                }
            }
        }

        self.states.retain(|eid, _| seen.contains(eid));
        for (eid, cs) in cmd_map {
            if !cs.is_empty() && !seen.contains(&eid) && !has_rig2d(scene, eid) {
                warn(logs, eid, "目标实体无 Rig2D 组件,rig 命令被忽略");
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_scene::{Component, Entity, Transform};

    fn fixture_rig() -> Arc<CompiledRig> {
        let mut v = crate::render_core::rig::tests::rig_doc_json();
        v["parameters"].as_array_mut().unwrap().push(json!({ "id": "ParamEyeOpen", "min": 0, "max": 1, "default": 1 }));
        v["auto"] = json!({ "blink": { "params": ["ParamEyeOpen"], "intervalSec": [0.05, 0.05] } });
        v["clips"]["once"] = json!({ "duration": 0.1, "loop": false, "tracks": [] });
        v["animator"] = json!({
            "defaultState": "idle",
            "parameters": { "go": "trigger", "moving": "bool" },
            "states": { "idle": { "clip": "slide" }, "jump": { "clip": "once" } },
            "transitions": [
                { "from": "idle", "to": "jump", "when": [ { "trigger": "go" } ] },
                { "from": "jump", "to": "idle", "hasExitTime": true }
            ]
        });
        Arc::new(CompiledRig::compile(forge_rig2d::parse_rxrig(&v).unwrap()).unwrap())
    }

    fn scene_with(props: Value) -> Scene {
        let mut scene = Scene::new("t");
        let id = scene.alloc_id();
        scene.entities.push(Entity {
            entity_guid: None,
            id,
            name: "girl".into(),
            transform: Transform::default(),
            components: vec![Component::new("Rig2D", props)],
        });
        scene
    }

    fn props_of(scene: &Scene) -> Value {
        scene.entity(1).unwrap().component("Rig2D").unwrap().props.clone()
    }

    #[test]
    fn manual_mode_clip_advances_and_params_written_back() {
        let rig = fixture_rig();
        let rs = move |g: &str| (g == "guid-rig").then(|| rig.clone());
        let mut scene = scene_with(json!({ "rig": "guid-rig", "clip": "slide", "params": { "ParamX": -40.0 }, "autoBlink": false }));
        let mut sys = RigSystem::default();
        let mut logs = Vec::new();
        for _ in 0..30 {
            sys.advance_with(&mut scene, vec![], 1.0 / 60.0, &mut logs, &rs);
        }
        let p = props_of(&scene);
        assert_eq!(p["clip"], "slide");
        let t = p["time"].as_f64().unwrap();
        assert!((t - 0.5).abs() < 1e-4, "30 步 ≈ 0.5s,实际 {t}");
        // 编辑态 params(-40)不进入运行态:快照 = clip 轨道值(t=0.5 → 25)。
        let x = p["params"]["ParamX"].as_f64().unwrap();
        assert!((x - 25.0).abs() < 0.1, "ParamX 应来自 clip,实际 {x}");
        assert_eq!(p["params"]["ParamEyeOpen"], 1.0, "autoBlink=false 时眼睛保持缺省");
        assert!(logs.is_empty(), "{logs:?}");
    }

    #[test]
    fn set_param_play_stop_and_warnings() {
        let rig = fixture_rig();
        let rs = move |g: &str| (g == "guid-rig").then(|| rig.clone());
        let mut scene = scene_with(json!({ "rig": "guid-rig", "clip": "slide", "autoBlink": false }));
        let mut sys = RigSystem::default();
        let mut logs = Vec::new();
        let dt = 1.0 / 60.0;
        sys.advance_with(&mut scene, vec![AnimCommand::RigSetParam { entity: 1, param: "ParamX".into(), value: -20.0, blend: None }], dt, &mut logs, &rs);
        assert_eq!(props_of(&scene)["params"]["ParamX"], -20.0, "set_param 覆盖 clip 轨道");
        sys.advance_with(&mut scene, vec![AnimCommand::RigStop { entity: 1 }], dt, &mut logs, &rs);
        let t0 = props_of(&scene)["time"].clone();
        sys.advance_with(&mut scene, vec![], dt, &mut logs, &rs);
        assert_eq!(props_of(&scene)["time"], t0, "stop 后 time 不推进");
        sys.advance_with(&mut scene, vec![AnimCommand::RigPlay { entity: 1, clip: "slide".into(), restart: true }], dt, &mut logs, &rs);
        assert!(sys.debug_state(1).unwrap().2, "play 恢复播放");
        assert!(logs.is_empty(), "{logs:?}");
        // 未知参数 / clip / 无 Rig2D 目标 → rig.warn。
        sys.advance_with(
            &mut scene,
            vec![
                AnimCommand::RigSetParam { entity: 1, param: "Nope".into(), value: 1.0, blend: None },
                AnimCommand::RigPlay { entity: 1, clip: "nosuch".into(), restart: false },
                AnimCommand::RigStop { entity: 99 },
            ],
            dt,
            &mut logs,
            &rs,
        );
        let warns: Vec<_> = logs.iter().filter(|(n, _)| n == "rig.warn").collect();
        assert_eq!(warns.len(), 3, "{logs:?}");
        assert_eq!(warns[2].1["entityId"], 99);
    }

    #[test]
    fn fsm_mode_routes_animator_and_rejects_manual_play() {
        let rig = fixture_rig();
        let rs = move |g: &str| (g == "guid-rig").then(|| rig.clone());
        let mut scene = scene_with(json!({ "rig": "guid-rig" }));
        let mut sys = RigSystem::default();
        let mut logs = Vec::new();
        let dt = 1.0 / 60.0;
        sys.advance_with(&mut scene, vec![], dt, &mut logs, &rs);
        assert_eq!(sys.debug_state(1).unwrap().3.as_deref(), Some("idle"));
        // animator.set_trigger 目标带 Rig2D → 分流进 RigSystem。
        let (anim, rigc) = route(&scene, vec![
            AnimCommand::SetTrigger { entity: 1, param: "go".into() },
            AnimCommand::SetBool { entity: 7, param: "x".into(), value: true },
        ]);
        assert_eq!((anim.len(), rigc.len()), (1, 1));
        sys.advance_with(&mut scene, rigc, dt, &mut logs, &rs);
        assert_eq!(sys.debug_state(1).unwrap().3.as_deref(), Some("jump"));
        assert_eq!(props_of(&scene)["clip"], "once", "FSM 选中的 clip 回写 props");
        for _ in 0..12 {
            sys.advance_with(&mut scene, vec![], dt, &mut logs, &rs);
        }
        assert_eq!(sys.debug_state(1).unwrap().3.as_deref(), Some("idle"), "hasExitTime 播毕回 idle");
        assert!(logs.is_empty(), "{logs:?}");
        sys.advance_with(&mut scene, vec![AnimCommand::RigPlay { entity: 1, clip: "slide".into(), restart: true }], dt, &mut logs, &rs);
        assert!(logs.iter().any(|(n, _)| n == "rig.warn"), "FSM 模式手控 play 须告警");
    }

    #[test]
    fn auto_blink_is_deterministic_and_doc_change_rebuilds_sim() {
        let rig = fixture_rig();
        let r2 = rig.clone();
        let rs = move |g: &str| (g == "guid-rig").then(|| r2.clone());
        let run = |n: usize| {
            let mut scene = scene_with(json!({ "rig": "guid-rig", "clip": "slide" }));
            let mut sys = RigSystem::default();
            let mut logs = Vec::new();
            let mut eye = Vec::new();
            for _ in 0..n {
                sys.advance_with(&mut scene, vec![], 1.0 / 60.0, &mut logs, &rs);
                eye.push(props_of(&scene)["params"]["ParamEyeOpen"].as_f64().unwrap());
            }
            eye
        };
        let a = run(40);
        assert_eq!(a, run(40), "同 seed(实体 id)逐位确定");
        assert!(a.iter().any(|v| *v < 0.5), "间隔 0.05s 的自动眨眼应出现闭眼帧: {a:?}");

        // 文档换新 Arc(编辑后重载)→ RigSim 重建(time 从 0 重新计)。
        let mut scene = scene_with(json!({ "rig": "guid-rig", "clip": "slide" }));
        let mut sys = RigSystem::default();
        let mut logs = Vec::new();
        let cur = std::cell::RefCell::new(rig.clone());
        let dyn_rs = |g: &str| (g == "guid-rig").then(|| cur.borrow().clone());
        for _ in 0..30 {
            sys.advance_with(&mut scene, vec![], 1.0 / 60.0, &mut logs, &dyn_rs);
        }
        *cur.borrow_mut() = fixture_rig();
        sys.advance_with(&mut scene, vec![], 1.0 / 60.0, &mut logs, &dyn_rs);
        assert!(sys.debug_state(1).unwrap().1 < 0.02, "新文档 → 新 RigSim");
    }

    #[test]
    fn unresolvable_doc_warns_once() {
        let mut scene = scene_with(json!({ "rig": "missing" }));
        let mut sys = RigSystem::default();
        let mut logs = Vec::new();
        for _ in 0..5 {
            sys.advance_with(&mut scene, vec![], 1.0 / 60.0, &mut logs, &|_| None);
        }
        assert_eq!(logs.len(), 1, "{logs:?}");
        assert!(sys.debug_state(1).is_none());
    }
}
