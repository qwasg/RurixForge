//! F-GAME-4:宿主侧精灵帧动画运行时(PIE 语义)。
//!
//! 职责边界:
//! - 仅作用于 run_scene(play 态);play.enter 初始化、play.exit 清空,编辑态零介入;
//! - 组件 `Sprite.frame/clip` 的**唯一写者**——图节点(sprite.*/animator.*)只发
//!   `AnimCommand`,由本系统在逻辑帧后按声明序消费,避免双写者;
//! - 时间模型对齐 VibeGame AnimationPlayer:dt 驱动 + while 跨帧不丢帧,
//!   clip 时长 duration 优先于 fps,非循环收尾 hold/first;
//! - animator(.rxsprite 可选段):参数驱动状态机,转换表有序首匹配,trigger
//!   触发即消费,hasExitTime 等当前 clip 播毕;
//! - 模式判定(play 会话初始化时定):实体组件 `clip` 为空且文档带 animator →
//!   **FSM 模式**(状态机独占 clip 选择,sprite.play/stop/set_frame 如实 `anim.warn`
//!   忽略);组件 `clip` 非空 → **手动模式**(纯 clip 播放,FSM 不介入)——同一
//!   .rxsprite 可被不同实体分别以两种模式使用。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use forge_logic::interp::AnimCommand;
use forge_scene::Scene;
use serde_json::{json, Value};

use crate::viewport::sprite_doc_cached;

/// 单实体动画状态(play 会话生命周期)。
#[derive(Debug, Clone)]
struct EntityAnim {
    /// 绑定的 .rxsprite GUID(变更 = 场内换精灵,状态重建)。
    sprite_guid: String,
    clip: String,
    frame_idx: usize,
    /// 当前帧已消耗秒数。
    elapsed: f32,
    playing: bool,
    /// 非循环 clip 已播毕(animator hasExitTime 判据)。
    finished: bool,
    /// animator 当前状态(仅带 animator 的精灵 Some)。
    fsm_state: Option<String>,
    bools: BTreeMap<String, bool>,
    triggers: BTreeSet<String>,
}

/// 宿主动画系统(HostState 持有;单写者)。
#[derive(Debug, Default)]
pub struct AnimSystem {
    states: HashMap<u64, EntityAnim>,
}

impl AnimSystem {
    /// play.exit / play.enter 回滚时清空。
    pub fn clear(&mut self) {
        self.states.clear();
    }

    /// 测试/内省:实体当前 (clip, frame, playing, fsm_state)。
    pub fn debug_state(&self, entity: u64) -> Option<(String, usize, bool, Option<String>)> {
        self.states
            .get(&entity)
            .map(|s| (s.clip.clone(), s.frame_idx, s.playing, s.fsm_state.clone()))
    }

    /// 每固定步推进(advance_frame 在逻辑帧后调用):
    /// 应用命令 → FSM 评估 → 帧推进 → 回写组件 props;返回是否有可视变更。
    pub fn advance(
        &mut self,
        scene: &mut Scene,
        cmds: Vec<AnimCommand>,
        dt: f32,
        logs: &mut Vec<(String, Value)>,
    ) -> bool {
        self.advance_with(scene, cmds, dt, logs, &|guid| sprite_doc_cached(guid))
    }

    /// 解析器可注入版(测试直供文档,免全局缓存/env 依赖;生产走 sprite_doc_cached)。
    pub fn advance_with(
        &mut self,
        scene: &mut Scene,
        cmds: Vec<AnimCommand>,
        dt: f32,
        logs: &mut Vec<(String, Value)>,
        resolve: &dyn Fn(&str) -> Option<std::sync::Arc<assetd::sprite::SpriteDoc>>,
    ) -> bool {
        // 命令按目标实体分组(组内保持声明序)。
        let mut cmd_map: HashMap<u64, Vec<AnimCommand>> = HashMap::new();
        for c in cmds {
            let eid = match &c {
                AnimCommand::Play { entity, .. }
                | AnimCommand::Stop { entity }
                | AnimCommand::SetFrame { entity, .. }
                | AnimCommand::SetBool { entity, .. }
                | AnimCommand::SetTrigger { entity, .. } => *entity,
            };
            cmd_map.entry(eid).or_default().push(c);
        }

        let mut changed = false;
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        // 实体 id 升序(确定性,与图解释器同纪律)。
        // The entity vector is not resized by animation evaluation. Retain its
        // indices instead of linearly searching the whole scene for every id;
        // large native publication scenes contain many non-Sprite entities.
        let mut ids: Vec<(u64, usize)> = scene.entities.iter().enumerate()
            .filter(|(_, e)| e.components.iter().any(|c| c.ctype == "Sprite" && c.enabled))
            .map(|(index, e)| (e.id, index)).collect();
        ids.sort_unstable_by_key(|(id, _)| *id);
        for (eid, entity_index) in ids {
            let entity = &scene.entities[entity_index];
            let Some(sp) = entity
                .components
                .iter()
                .find(|c| c.ctype == "Sprite" && c.enabled)
            else {
                continue;
            };
            let sprite_guid = sp
                .props
                .get("sprite")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if sprite_guid.is_empty() {
                continue; // texture 直贴模式无帧动画语义
            }
            let Some(doc) = resolve(&sprite_guid) else {
                continue; // 文档不可解析:渲染侧已有 cube 回退腿,此处不刷屏
            };
            seen.insert(eid);

            // 初始化 / 换精灵重建。模式判定(仅初始化时读场景作者写的 clip):
            // 组件 clip 为空且文档带 animator → FSM 模式(状态机独占 clip 选择);
            // 组件 clip 非空 → 手动模式(纯 clip 播放,FSM 不介入)——同一 .rxsprite
            // 可被不同实体分别以两种模式使用。
            let stale = self
                .states
                .get(&eid)
                .map(|s| s.sprite_guid != sprite_guid)
                .unwrap_or(true);
            if stale {
                let authored_clip = sp
                    .props
                    .get("clip")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let (fsm_state, clip, playing) = match &doc.animator {
                    Some(a) if authored_clip.is_empty() => {
                        let clip = a
                            .states
                            .get(&a.default_state)
                            .map(|s| s.clip.clone())
                            .unwrap_or_default();
                        (Some(a.default_state.clone()), clip, true)
                    }
                    _ => {
                        let ok = doc.clips.contains_key(&authored_clip);
                        (None, if ok { authored_clip } else { String::new() }, ok)
                    }
                };
                let frame_idx = sp
                    .props
                    .get("frame")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    .max(0.0) as usize;
                self.states.insert(
                    eid,
                    EntityAnim {
                        sprite_guid: sprite_guid.clone(),
                        clip,
                        frame_idx,
                        elapsed: 0.0,
                        playing,
                        finished: false,
                        fsm_state,
                        bools: BTreeMap::new(),
                        triggers: BTreeSet::new(),
                    },
                );
            }
            let st = self.states.get_mut(&eid).expect("刚插入");

            // ── 应用图命令(声明序;FSM 模式看运行态 fsm_state,而非文档是否带 animator) ──
            let fsm_active = st.fsm_state.is_some();
            for cmd in cmd_map.remove(&eid).unwrap_or_default() {
                match (doc.animator.as_ref().filter(|_| fsm_active), cmd) {
                    // FSM 模式独占 clip 选择:手控命令如实警告忽略(I-5 不静默)。
                    (Some(_), AnimCommand::Play { .. })
                    | (Some(_), AnimCommand::Stop { .. })
                    | (Some(_), AnimCommand::SetFrame { .. }) => {
                        logs.push((
                            "anim.warn".into(),
                            json!({ "entityId": eid, "reason": "实体带 animator,sprite.play/stop/set_frame 被忽略,请用 animator.set_bool/set_trigger" }),
                        ));
                    }
                    (Some(a), AnimCommand::SetBool { param, value, .. }) => {
                        if a.parameters.get(&param).map(String::as_str) == Some("bool") {
                            st.bools.insert(param, value);
                        } else {
                            logs.push((
                                "anim.warn".into(),
                                json!({ "entityId": eid, "reason": format!("animator 无 bool 参数 {param}") }),
                            ));
                        }
                    }
                    (Some(a), AnimCommand::SetTrigger { param, .. }) => {
                        if a.parameters.get(&param).map(String::as_str) == Some("trigger") {
                            st.triggers.insert(param);
                        } else {
                            logs.push((
                                "anim.warn".into(),
                                json!({ "entityId": eid, "reason": format!("animator 无 trigger 参数 {param}") }),
                            ));
                        }
                    }
                    (None, AnimCommand::Play { clip, restart, .. }) => {
                        if !doc.clips.contains_key(&clip) {
                            logs.push((
                                "anim.warn".into(),
                                json!({ "entityId": eid, "reason": format!(".rxsprite 无 clip {clip}") }),
                            ));
                        } else if st.clip != clip || restart || !st.playing {
                            if st.clip != clip || restart {
                                st.frame_idx = 0;
                                st.elapsed = 0.0;
                            }
                            st.clip = clip;
                            st.playing = true;
                            st.finished = false;
                        } // 同 clip 播放中且 restart=false:幂等 no-op
                    }
                    (None, AnimCommand::Stop { .. }) => {
                        st.playing = false;
                    }
                    (None, AnimCommand::SetFrame { index, .. }) => {
                        let len = doc.clips.get(&st.clip).map(|c| c.frames.len()).unwrap_or(0);
                        st.frame_idx = if len == 0 { index } else { index.min(len - 1) };
                        st.elapsed = 0.0;
                        changed = true;
                    }
                    (None, AnimCommand::SetBool { param, .. })
                    | (None, AnimCommand::SetTrigger { param, .. }) => {
                        logs.push((
                            "anim.warn".into(),
                            json!({ "entityId": eid, "reason": format!("实体非 animator 模式(无 animator 段或场景显式写了 clip),参数 {param} 被忽略") }),
                        ));
                    }
                }
            }

            // ── animator FSM:有序转换表首匹配,每帧至多切换一次 ──
            if let (Some(a), Some(cur)) = (&doc.animator, st.fsm_state.clone()) {
                for t in &a.transitions {
                    if !t.from.matches(&cur) {
                        continue;
                    }
                    if t.has_exit_time && !st.finished {
                        continue;
                    }
                    let met = t.when.iter().all(|c| match c {
                        assetd::sprite::AnimatorCond::Param { param, eq } => {
                            st.bools.get(param).copied().unwrap_or(false) == *eq
                        }
                        assetd::sprite::AnimatorCond::Trigger { trigger } => {
                            st.triggers.contains(trigger)
                        }
                    });
                    if !met {
                        continue;
                    }
                    // 触发:消费本转换用到的 trigger,进入新状态(restart 语义)。
                    for c in &t.when {
                        if let assetd::sprite::AnimatorCond::Trigger { trigger } = c {
                            st.triggers.remove(trigger);
                        }
                    }
                    if t.to != cur || t.has_exit_time {
                        st.fsm_state = Some(t.to.clone());
                        st.clip = a
                            .states
                            .get(&t.to)
                            .map(|s| s.clip.clone())
                            .unwrap_or_default();
                        st.frame_idx = 0;
                        st.elapsed = 0.0;
                        st.playing = true;
                        st.finished = false;
                        changed = true;
                    }
                    break;
                }
            }

            // ── clip 帧推进(dt 驱动,while 跨帧;loop / hold / first) ──
            if st.playing {
                if let Some(clip) = doc.clips.get(&st.clip) {
                    let len = clip.frames.len();
                    if len > 0 {
                        let fd = clip.frame_duration().max(1e-4);
                        st.elapsed += dt;
                        while st.elapsed >= fd {
                            st.elapsed -= fd;
                            if st.frame_idx + 1 < len {
                                st.frame_idx += 1;
                                changed = true;
                            } else if clip.looped {
                                st.frame_idx = 0;
                                changed = true;
                            } else {
                                // 非循环播毕:hold 停末帧 / first 回首帧。
                                st.finished = true;
                                st.playing = false;
                                if clip.on_finish == "first" && st.frame_idx != 0 {
                                    st.frame_idx = 0;
                                    changed = true;
                                }
                                st.elapsed = 0.0;
                                break;
                            }
                        }
                    }
                } else {
                    st.playing = false; // clip 不存在(编辑期被删):停播不 panic
                }
            }

            // ── 回写组件 props(唯一写者;渲染/点选/流媒体自然拾取) ──
            let (clip_v, frame_v) = (st.clip.clone(), st.frame_idx as f64);
            if let Some(e) = scene.entities.get_mut(entity_index) {
                if let Some(c) = e.component_mut("Sprite") {
                    if let Some(obj) = c.props.as_object_mut() {
                        let new_clip = Value::String(clip_v);
                        let new_frame = json!(frame_v);
                        if obj.get("clip") != Some(&new_clip) {
                            obj.insert("clip".into(), new_clip);
                            changed = true;
                        }
                        if obj.get("frame") != Some(&new_frame) {
                            obj.insert("frame".into(), new_frame);
                            changed = true;
                        }
                    }
                }
            }
        }

        // 场内已不存在/已换形态的实体状态修剪;落空的命令如实警告(目标无 .rxsprite)。
        self.states.retain(|eid, _| seen.contains(eid));
        for (eid, cs) in cmd_map {
            if !cs.is_empty() {
                logs.push((
                    "anim.warn".into(),
                    json!({ "entityId": eid, "reason": "目标实体无 .rxsprite 精灵组件,动画命令被忽略" }),
                ));
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_scene::{Component, Entity, Transform};
    use std::sync::Arc;

    type Resolver = Box<dyn Fn(&str) -> Option<Arc<assetd::sprite::SpriteDoc>>>;

    /// 内存文档解析器(免文件/env/全局缓存依赖):walk 2 帧 8fps 循环;
    /// attack 2 帧非循环 hold;animator=true 时附 idle/run/atk 状态机。
    fn setup(animator: bool) -> (Scene, Resolver) {
        let mut doc = json!({
            "version": 1,
            "texture": "guid-sheet",
            "frames": {
                "w0": { "bbox": [0, 0, 2, 2] }, "w1": { "bbox": [2, 0, 2, 2] }
            },
            "clips": {
                "walk": { "frames": ["w0", "w1"], "fps": 8, "loop": true },
                "attack": { "frames": ["w0", "w1"], "fps": 8, "loop": false, "onFinish": "hold" }
            }
        });
        if animator {
            doc["animator"] = json!({
                "defaultState": "idle",
                "parameters": { "isMoving": "bool", "hit": "trigger" },
                "states": { "idle": { "clip": "walk" }, "run": { "clip": "walk" }, "atk": { "clip": "attack" } },
                "transitions": [
                    { "from": "idle", "to": "run", "when": [{ "param": "isMoving", "eq": true }] },
                    { "from": "run", "to": "idle", "when": [{ "param": "isMoving", "eq": false }] },
                    { "from": "Any", "to": "atk", "when": [{ "trigger": "hit" }] },
                    { "from": "atk", "to": "idle", "hasExitTime": true }
                ]
            });
        }
        let parsed = Arc::new(assetd::sprite::parse_rxsprite(&doc).expect("测试文档须合法"));
        let resolver: Resolver = Box::new(move |guid| {
            (guid == "guid-hero").then(|| parsed.clone())
        });

        let mut scene = Scene::new("t");
        let id = scene.alloc_id();
        scene.entities.push(Entity {
            id,
            name: "hero".into(),
            transform: Transform::default(),
            components: vec![Component::new(
                "Sprite",
                json!({ "sprite": "guid-hero", "clip": if animator { "" } else { "walk" }, "frame": 0.0 }),
            )],
        });
        (scene, resolver)
    }

    fn frame_of(scene: &Scene, id: u64) -> (String, f64) {
        let e = scene.entity(id).unwrap();
        let c = e.component("Sprite").unwrap();
        (
            c.props.get("clip").and_then(Value::as_str).unwrap_or("").to_string(),
            c.props.get("frame").and_then(Value::as_f64).unwrap_or(-1.0),
        )
    }

    #[test]
    fn clip_advances_loops_and_is_idempotent() {
        let (mut scene, rs) = setup(false);
        let mut sys = AnimSystem::default();
        let mut logs = Vec::new();
        let dt = 1.0 / 60.0;
        // 8fps → 每帧 0.125s = 7.5 逻辑步;走 8 步应进第 2 帧(idx 1)。
        for _ in 0..8 {
            sys.advance_with(&mut scene, Vec::new(), dt, &mut logs, &rs);
        }
        assert_eq!(frame_of(&scene, 1), ("walk".into(), 1.0));
        // 再 8 步循环回 idx 0。
        for _ in 0..8 {
            sys.advance_with(&mut scene, Vec::new(), dt, &mut logs, &rs);
        }
        assert_eq!(frame_of(&scene, 1).1, 0.0, "循环 clip 应回卷");
        // 幂等 play(restart=false)不重置帧。
        for _ in 0..8 {
            sys.advance_with(
                &mut scene,
                vec![AnimCommand::Play { entity: 1, clip: "walk".into(), restart: false }],
                dt,
                &mut logs,
                &rs,
            );
        }
        assert_eq!(frame_of(&scene, 1).1, 1.0, "幂等 play 不得冻帧");
        // restart=true 重置到 0。
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::Play { entity: 1, clip: "walk".into(), restart: true }],
            dt,
            &mut logs,
            &rs,
        );
        assert_eq!(frame_of(&scene, 1).1, 0.0);
        // stop 停帧。
        sys.advance_with(&mut scene, vec![AnimCommand::Stop { entity: 1 }], dt, &mut logs, &rs);
        let before = frame_of(&scene, 1).1;
        for _ in 0..16 {
            sys.advance_with(&mut scene, Vec::new(), dt, &mut logs, &rs);
        }
        assert_eq!(frame_of(&scene, 1).1, before, "stop 后帧不再推进");
        assert!(logs.is_empty(), "合法命令不应产生警告: {logs:?}");
    }

    #[test]
    fn nonloop_holds_and_setframe_clamps() {
        let (mut scene, rs) = setup(false);
        let mut sys = AnimSystem::default();
        let mut logs = Vec::new();
        let dt = 1.0 / 60.0;
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::Play { entity: 1, clip: "attack".into(), restart: true }],
            dt,
            &mut logs,
            &rs,
        );
        // 2 帧 × 0.125s ≈ 15 步后播毕停末帧。
        for _ in 0..30 {
            sys.advance_with(&mut scene, Vec::new(), dt, &mut logs, &rs);
        }
        assert_eq!(frame_of(&scene, 1), ("attack".into(), 1.0), "hold 停末帧");
        // set_frame 钳制越界。
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::SetFrame { entity: 1, index: 99 }],
            dt,
            &mut logs,
            &rs,
        );
        assert_eq!(frame_of(&scene, 1).1, 1.0);
        // 未知 clip 如实警告。
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::Play { entity: 1, clip: "nosuch".into(), restart: false }],
            dt,
            &mut logs,
            &rs,
        );
        assert!(
            logs.iter().any(|(n, _)| n == "anim.warn"),
            "未知 clip 须警告: {logs:?}"
        );
    }

    #[test]
    fn animator_fsm_transitions_and_trigger_consumed() {
        let (mut scene, rs) = setup(true);
        let mut sys = AnimSystem::default();
        let mut logs = Vec::new();
        let dt = 1.0 / 60.0;
        // 初始 = defaultState idle。
        sys.advance_with(&mut scene, Vec::new(), dt, &mut logs, &rs);
        assert_eq!(sys.debug_state(1).unwrap().3.as_deref(), Some("idle"));
        // isMoving=true → run。
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::SetBool { entity: 1, param: "isMoving".into(), value: true }],
            dt,
            &mut logs,
            &rs,
        );
        assert_eq!(sys.debug_state(1).unwrap().3.as_deref(), Some("run"));
        // trigger hit → atk(Any 通配),trigger 消费。
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::SetTrigger { entity: 1, param: "hit".into() }],
            dt,
            &mut logs,
            &rs,
        );
        assert_eq!(sys.debug_state(1).unwrap().3.as_deref(), Some("atk"));
        assert_eq!(frame_of(&scene, 1).0, "attack");
        // attack 非循环播毕 → hasExitTime 转回 idle;isMoving 仍 true → 随即进 run。
        for _ in 0..30 {
            sys.advance_with(&mut scene, Vec::new(), dt, &mut logs, &rs);
        }
        let st = sys.debug_state(1).unwrap().3;
        assert!(
            st.as_deref() == Some("run") || st.as_deref() == Some("idle"),
            "atk 播毕应回 idle/run,实际 {st:?}"
        );
        // animator 实体的手控 play 被如实警告忽略。
        logs.clear();
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::Play { entity: 1, clip: "walk".into(), restart: true }],
            dt,
            &mut logs,
            &rs,
        );
        assert!(logs.iter().any(|(n, _)| n == "anim.warn"));
    }

    /// 场景显式写 clip 的实体走手动模式:animator 存在但不介入,同一文档两种用法并存。
    #[test]
    fn authored_clip_opts_out_of_animator() {
        let (mut scene, rs) = setup(true); // 文档带 animator
        // 实体 clip 显式写 walk → 手动模式。
        {
            let e = scene.entity_mut(1).unwrap();
            let c = e.component_mut("Sprite").unwrap();
            c.props["clip"] = json!("walk");
        }
        let mut sys = AnimSystem::default();
        let mut logs = Vec::new();
        let dt = 1.0 / 60.0;
        sys.advance_with(&mut scene, Vec::new(), dt, &mut logs, &rs);
        let (clip, _, playing, fsm) = sys.debug_state(1).unwrap();
        assert_eq!(clip, "walk", "手动模式应播场景作者写的 clip");
        assert!(playing);
        assert!(fsm.is_none(), "手动模式 FSM 不介入");
        // 手动模式下 sprite.play 可用(不被 animator 拦截)。
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::Play { entity: 1, clip: "attack".into(), restart: true }],
            dt,
            &mut logs,
            &rs,
        );
        assert_eq!(sys.debug_state(1).unwrap().0, "attack");
        assert!(logs.is_empty(), "手动模式 play 不应警告: {logs:?}");
        // 手动模式下 animator 参数被如实警告忽略。
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::SetBool { entity: 1, param: "isMoving".into(), value: true }],
            dt,
            &mut logs,
            &rs,
        );
        assert!(logs.iter().any(|(n, _)| n == "anim.warn"));
    }

    #[test]
    fn command_to_nonsprite_entity_warns() {
        let (mut scene, rs) = setup(false);
        let mut sys = AnimSystem::default();
        let mut logs = Vec::new();
        sys.advance_with(
            &mut scene,
            vec![AnimCommand::Play { entity: 999, clip: "walk".into(), restart: false }],
            1.0 / 60.0,
            &mut logs,
            &rs,
        );
        assert!(logs.iter().any(|(n, _)| n == "anim.warn"));
    }
}
