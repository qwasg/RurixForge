//! 运行时模拟(PIE 每固定步):animator FSM → clip 推进 → 参数合成
//! (缺省 → clip 轨道 → 逻辑覆盖 → 自动呼吸/眨眼 → 物理输出)。
//!
//! FSM 语义与 engine-host anim.rs 的 .rxsprite animator 完全一致:有序转换表首匹配、
//! 每步至多切换一次、trigger 触发即消费、hasExitTime 等非循环 clip 播完。
//! 同一 seed 下逐位确定。

use std::collections::{BTreeMap, BTreeSet};

use crate::compile::CompiledRig;
use crate::doc::{AnimatorCond, PhysicsInputKind, RigParam};
use crate::eval::{apply_clip, Pose};

/// 逻辑图 / 外部命令。
#[derive(Debug, Clone, PartialEq)]
pub enum RigCommand {
    Play { clip: String, restart: bool },
    Stop,
    /// 绝对覆盖参数;`blend` ∈ (0,1] 时向目标值指数趋近(每步按 blend 比例)。
    SetParam { param: String, value: f32, blend: Option<f32> },
    ClearParam { param: String },
    SetBool { param: String, value: bool },
    SetTrigger { param: String },
}

/// 命令被忽略的原因(宿主转成 rig.warn 事件)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigWarn(pub String);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimOptions {
    pub auto_blink: bool,
    pub auto_breath: bool,
    pub physics: bool,
    pub speed: f32,
}

impl Default for SimOptions {
    fn default() -> Self {
        Self { auto_blink: true, auto_breath: true, physics: true, speed: 1.0 }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Chain {
    pos: Vec<[f32; 2]>,
    prev: Vec<[f32; 2]>,
    initialized: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Blink {
    rng: u64,
    next_in: f32,
    /// 闭眼动画已进行秒数;None = 未在眨眼。
    phase: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RigSim {
    pub clip: String,
    pub time: f32,
    pub playing: bool,
    pub finished: bool,
    pub opts: SimOptions,
    pub fsm_state: Option<String>,
    bools: BTreeMap<String, bool>,
    triggers: BTreeSet<String>,
    /// 参数下标 → (目标值, blend, 当前值)。
    overrides: BTreeMap<usize, (f32, Option<f32>, Option<f32>)>,
    chains: Vec<Chain>,
    blink: Blink,
    breath_t: f32,
}

fn xorshift(s: &mut u64) -> f32 {
    let mut x = *s;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *s = x;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// 参数值归一化到 [-1, 1](以 default 为 0)。
pub fn normalize_param(p: &RigParam, v: f32) -> f32 {
    if v >= p.default {
        let span = p.max - p.default;
        if span <= 1e-9 { 0.0 } else { (v - p.default) / span }
    } else {
        let span = p.default - p.min;
        if span <= 1e-9 { 0.0 } else { (v - p.default) / span }
    }
}

impl RigSim {
    /// 初始化。模式判定与 .rxsprite 相同:`authored_clip` 为空且文档带 animator → FSM 模式。
    pub fn new(rig: &CompiledRig, authored_clip: &str, seed: u64, opts: SimOptions) -> RigSim {
        let (fsm_state, clip, playing) = match &rig.doc.animator {
            Some(a) if authored_clip.is_empty() => (
                Some(a.default_state.clone()),
                a.states.get(&a.default_state).map(|s| s.clip.clone()).unwrap_or_default(),
                true,
            ),
            _ => {
                let ok = rig.doc.clips.contains_key(authored_clip);
                (None, if ok { authored_clip.to_string() } else { String::new() }, ok)
            }
        };
        let mut rng = seed ^ 0x9E37_79B9_7F4A_7C15;
        if rng == 0 {
            rng = 1;
        }
        let mut blink = Blink { rng, next_in: 0.0, phase: None };
        if let Some(b) = &rig.doc.auto.blink {
            let u = xorshift(&mut blink.rng);
            blink.next_in = b.interval_sec[0] + (b.interval_sec[1] - b.interval_sec[0]) * u;
        }
        RigSim {
            clip,
            time: 0.0,
            playing,
            finished: false,
            opts,
            fsm_state,
            bools: BTreeMap::new(),
            triggers: BTreeSet::new(),
            overrides: BTreeMap::new(),
            chains: rig
                .doc
                .physics
                .iter()
                .map(|p| Chain {
                    pos: vec![[0.0, 0.0]; p.segments.len() + 1],
                    prev: vec![[0.0, 0.0]; p.segments.len() + 1],
                    initialized: false,
                })
                .collect(),
            blink,
            breath_t: 0.0,
        }
    }

    pub fn is_fsm(&self) -> bool {
        self.fsm_state.is_some()
    }

    /// 应用一条命令;被忽略时返回原因(I-5:不静默)。
    pub fn apply(&mut self, rig: &CompiledRig, cmd: RigCommand) -> Result<(), RigWarn> {
        let fsm = rig.doc.animator.as_ref().filter(|_| self.is_fsm());
        match (fsm, cmd) {
            (Some(_), RigCommand::Play { .. }) | (Some(_), RigCommand::Stop) => Err(RigWarn(
                "实体带 animator(FSM 模式),rig.play/stop 被忽略,请用 animator.set_bool/set_trigger".into(),
            )),
            (Some(a), RigCommand::SetBool { param, value }) => {
                if a.parameters.get(&param).map(String::as_str) == Some("bool") {
                    self.bools.insert(param, value);
                    Ok(())
                } else {
                    Err(RigWarn(format!("animator 无 bool 参数 {param}")))
                }
            }
            (Some(a), RigCommand::SetTrigger { param }) => {
                if a.parameters.get(&param).map(String::as_str) == Some("trigger") {
                    self.triggers.insert(param);
                    Ok(())
                } else {
                    Err(RigWarn(format!("animator 无 trigger 参数 {param}")))
                }
            }
            (None, RigCommand::Play { clip, restart }) => {
                if !rig.doc.clips.contains_key(&clip) {
                    return Err(RigWarn(format!(".rxrig 无 clip {clip}")));
                }
                if self.clip != clip || restart {
                    self.time = 0.0;
                }
                self.clip = clip;
                self.playing = true;
                self.finished = false;
                Ok(())
            }
            (None, RigCommand::Stop) => {
                self.playing = false;
                Ok(())
            }
            (None, RigCommand::SetBool { param, .. }) | (None, RigCommand::SetTrigger { param }) => {
                Err(RigWarn(format!(
                    "实体非 animator 模式(无 animator 段或场景显式写了 clip),参数 {param} 被忽略"
                )))
            }
            (_, RigCommand::SetParam { param, value, blend }) => match rig.param_index.get(&param) {
                Some(&i) => {
                    let v = rig.doc.parameters[i].clamp(value);
                    let blend = blend.filter(|b| *b > 0.0 && *b < 1.0);
                    let cur = self.overrides.get(&i).and_then(|o| o.2);
                    self.overrides.insert(i, (v, blend, cur));
                    Ok(())
                }
                None => Err(RigWarn(format!(".rxrig 无参数 {param}"))),
            },
            (_, RigCommand::ClearParam { param }) => match rig.param_index.get(&param) {
                Some(&i) => {
                    self.overrides.remove(&i);
                    Ok(())
                }
                None => Err(RigWarn(format!(".rxrig 无参数 {param}"))),
            },
        }
    }

    fn fsm_step(&mut self, rig: &CompiledRig) {
        let (Some(a), Some(cur)) = (&rig.doc.animator, self.fsm_state.clone()) else { return };
        for t in &a.transitions {
            if !t.from.matches(&cur) || (t.has_exit_time && !self.finished) {
                continue;
            }
            let met = t.when.iter().all(|c| match c {
                AnimatorCond::Param { param, eq } => self.bools.get(param).copied().unwrap_or(false) == *eq,
                AnimatorCond::Trigger { trigger } => self.triggers.contains(trigger),
            });
            if !met {
                continue;
            }
            for c in &t.when {
                if let AnimatorCond::Trigger { trigger } = c {
                    self.triggers.remove(trigger);
                }
            }
            if t.to != cur || t.has_exit_time {
                self.fsm_state = Some(t.to.clone());
                self.clip = a.states.get(&t.to).map(|s| s.clip.clone()).unwrap_or_default();
                self.time = 0.0;
                self.playing = true;
                self.finished = false;
            }
            break;
        }
    }

    /// 推进一个固定步,返回本帧完整 pose。
    pub fn step(&mut self, rig: &CompiledRig, dt: f32) -> Pose {
        let dt = dt.max(0.0);
        self.fsm_step(rig);

        if self.playing {
            match rig.doc.clips.get(&self.clip) {
                Some(c) => {
                    self.time += dt * self.opts.speed;
                    if !c.looped && self.time >= c.duration {
                        self.time = c.duration;
                        self.finished = true;
                        self.playing = false;
                    } else if c.looped && self.time >= c.duration {
                        self.time = self.time.rem_euclid(c.duration);
                    }
                }
                None => self.playing = false,
            }
        }

        let mut pose = Pose::rest(rig);
        if !self.clip.is_empty() {
            apply_clip(rig, &mut pose, &self.clip, self.time);
        }

        // 逻辑覆盖(含 blend 平滑趋近)
        for (&i, o) in self.overrides.iter_mut() {
            let (target, blend, cur) = *o;
            let v = match blend {
                Some(b) => {
                    let from = cur.unwrap_or(pose.params[i]);
                    from + (target - from) * b
                }
                None => target,
            };
            o.2 = Some(v);
            pose.params[i] = rig.doc.parameters[i].clamp(v);
        }

        // 自动呼吸
        if self.opts.auto_breath {
            if let Some(b) = &rig.doc.auto.breath {
                self.breath_t = (self.breath_t + dt).rem_euclid(b.period_sec);
                if let Some(&i) = rig.param_index.get(&b.param) {
                    let w = 0.5 - 0.5 * (std::f32::consts::TAU * self.breath_t / b.period_sec).cos();
                    let p = &rig.doc.parameters[i];
                    pose.params[i] = p.clamp(pose.params[i] + b.amplitude * w * (p.max - p.default).max(0.0));
                }
            }
        }

        // 自动眨眼(开合度相乘)
        if self.opts.auto_blink {
            if let Some(b) = &rig.doc.auto.blink {
                let total = b.close_sec + b.hold_sec + b.open_sec;
                let mut openness = 1.0;
                match self.blink.phase {
                    None => {
                        self.blink.next_in -= dt;
                        if self.blink.next_in <= 0.0 {
                            self.blink.phase = Some(0.0);
                        }
                    }
                    Some(t) => {
                        let t = t + dt;
                        openness = if t < b.close_sec {
                            1.0 - t / b.close_sec.max(1e-4)
                        } else if t < b.close_sec + b.hold_sec {
                            0.0
                        } else if t < total {
                            (t - b.close_sec - b.hold_sec) / b.open_sec.max(1e-4)
                        } else {
                            1.0
                        };
                        if t >= total {
                            self.blink.phase = None;
                            let u = xorshift(&mut self.blink.rng);
                            self.blink.next_in = b.interval_sec[0] + (b.interval_sec[1] - b.interval_sec[0]) * u;
                        } else {
                            self.blink.phase = Some(t);
                        }
                    }
                }
                for pid in &b.params {
                    if let Some(&i) = rig.param_index.get(pid) {
                        let p = &rig.doc.parameters[i];
                        pose.params[i] = p.clamp(p.min + (pose.params[i] - p.min) * openness);
                    }
                }
            }
        }

        // 物理
        if self.opts.physics {
            self.physics_step(rig, &mut pose, dt);
        }
        pose
    }

    fn physics_step(&mut self, rig: &CompiledRig, pose: &mut Pose, dt: f32) {
        const SUB: f32 = 1.0 / 120.0;
        for (ci, pr) in rig.doc.physics.iter().enumerate() {
            let mut root_x = 0.0;
            let mut angle = 0.0;
            for inp in &pr.inputs {
                let Some(&i) = rig.param_index.get(&inp.param) else { continue };
                let n = normalize_param(&rig.doc.parameters[i], pose.params[i]) * inp.weight;
                match inp.kind {
                    PhysicsInputKind::X => root_x += n * pr.input_scale,
                    PhysicsInputKind::Angle => angle += n * pr.input_scale,
                }
            }
            let (s, c) = angle.to_radians().sin_cos();
            // 根坐标系「下」方向(画布 Y 向下)。
            let rest_dir = [-s, c];
            let chain = &mut self.chains[ci];
            if !chain.initialized {
                let mut p = [root_x, 0.0];
                chain.pos[0] = p;
                for (k, seg) in pr.segments.iter().enumerate() {
                    p = [p[0] + rest_dir[0] * seg.length, p[1] + rest_dir[1] * seg.length];
                    chain.pos[k + 1] = p;
                }
                chain.prev = chain.pos.clone();
                chain.initialized = true;
            }
            let steps = ((dt / SUB).ceil() as usize).clamp(1, 16);
            let h = dt / steps as f32;
            for _ in 0..steps {
                chain.pos[0] = [root_x, 0.0];
                chain.prev[0] = chain.pos[0];
                for (k, seg) in pr.segments.iter().enumerate() {
                    let j = k + 1;
                    let anchor = chain.pos[k];
                    let p = chain.pos[j];
                    let retain = (1.0 - seg.damping).powf(h * 60.0);
                    let vel = [(p[0] - chain.prev[j][0]) * retain, (p[1] - chain.prev[j][1]) * retain];
                    let target = [anchor[0] + rest_dir[0] * seg.length, anchor[1] + rest_dir[1] * seg.length];
                    let acc = [
                        seg.stiffness * (target[0] - p[0]),
                        pr.gravity + seg.stiffness * (target[1] - p[1]),
                    ];
                    let mut np = [p[0] + vel[0] + acc[0] * h * h, p[1] + vel[1] + acc[1] * h * h];
                    let d = [np[0] - anchor[0], np[1] - anchor[1]];
                    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
                    if len > 1e-6 {
                        np = [anchor[0] + d[0] / len * seg.length, anchor[1] + d[1] / len * seg.length];
                    }
                    chain.prev[j] = p;
                    chain.pos[j] = np;
                }
            }
            for o in &pr.outputs {
                let Some(&i) = rig.param_index.get(&o.param) else { continue };
                let a = chain.pos[o.segment];
                let b = chain.pos[o.segment + 1];
                let d = [b[0] - a[0], b[1] - a[1]];
                let cross = rest_dir[0] * d[1] - rest_dir[1] * d[0];
                let dot = rest_dir[0] * d[0] + rest_dir[1] * d[1];
                // 正值 = 向画布 +X 摆(相对根坐标系「下」方向)。
                let deg = -cross.atan2(dot).to_degrees();
                let p = &rig.doc.parameters[i];
                pose.params[i] = p.clamp(p.default + deg * o.scale);
            }
        }
    }
}
