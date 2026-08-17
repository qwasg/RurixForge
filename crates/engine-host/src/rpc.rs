//! JSON-RPC 2.0 方法分派与宿主共享状态。
//!
//! F1:实体/组件/变换全量 CRUD、命令栈 undo/redo、checkpoint/rollback、
//! PIE 双态(编辑态 / 运行态),所有变更类方法记逆操作、可撤销。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use forge_scene::{Component, Entity, Scene, Transform};
use rurix_physics::{BackendKind, PhysicsWorld, WorldDesc};
use serde_json::{json, Value};

use crate::timeutil::utc_now_iso8601;

/// 固定步长(秒),与 WorldDesc.dt_fixed 位级一致(step 只收此值)。
pub const DT_FIXED: f32 = 1.0 / 60.0;
/// 事件 ring 容量(溢出丢最旧)。
pub const EVENT_RING_CAP: usize = 1024;

/// PIE 状态机:edit / play_running / play_paused。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Edit,
    Running,
    Paused,
}

impl PlayState {
    pub fn as_str(self) -> &'static str {
        match self {
            PlayState::Edit => "edit",
            PlayState::Running => "play_running",
            PlayState::Paused => "play_paused",
        }
    }
}

/// 可逆操作:apply 返回其逆操作(undo 栈存逆、redo 栈存正,互推即可)。
#[derive(Debug, Clone)]
enum Op {
    /// 全量替换场景(scene.new / scene.load 用)。
    ReplaceScene(Scene),
    /// 批量(原子:任一失败全回滚;undo/redo 以整批为粒度)。
    Batch(Vec<Op>),
    CreateEntity {
        id: u64,
        name: String,
        transform: Transform,
        components: Vec<Component>,
    },
    DestroyEntity {
        id: u64,
    },
    RestoreEntity {
        index: usize,
        entity: Entity,
    },
    RenameEntity {
        id: u64,
        name: String,
    },
    SetTransform {
        id: u64,
        transform: Transform,
    },
    AddComponent {
        id: u64,
        component: Component,
    },
    InsertComponent {
        id: u64,
        index: usize,
        component: Component,
    },
    RemoveComponent {
        id: u64,
        ctype: String,
    },
    SetComponent {
        id: u64,
        component: Component,
    },
}

impl Op {
    /// 应用到场景,返回逆操作;失败保证场景未被修改(先校验后落地)。
    fn apply(&self, scene: &mut Scene) -> Result<Op, String> {
        match self {
            Op::ReplaceScene(new) => {
                let old = std::mem::replace(scene, new.clone());
                Ok(Op::ReplaceScene(old))
            }
            Op::Batch(ops) => {
                let mut inverses: Vec<Op> = Vec::with_capacity(ops.len());
                for (i, o) in ops.iter().enumerate() {
                    match o.apply(scene) {
                        Ok(inv) => inverses.push(inv),
                        Err(e) => {
                            // 回滚已完成的操作(逆序);回滚自身不应失败。
                            for inv in inverses.iter().rev() {
                                let _ = inv.apply(scene);
                            }
                            return Err(format!("第 {} 个 op 失败:{e}", i + 1));
                        }
                    }
                }
                inverses.reverse();
                Ok(Op::Batch(inverses))
            }
            Op::CreateEntity {
                id,
                name,
                transform,
                components,
            } => {
                if scene.entity(*id).is_some() {
                    return Err(format!("实体 id {id} 已存在"));
                }
                for c in components {
                    forge_scene::validate_component(c)?;
                }
                scene.entities.push(Entity {
                    id: *id,
                    name: name.clone(),
                    transform: *transform,
                    components: components.clone(),
                });
                if *id >= scene.next_id {
                    scene.next_id = id + 1;
                }
                Ok(Op::DestroyEntity { id: *id })
            }
            Op::DestroyEntity { id } => {
                let idx = scene
                    .entities
                    .iter()
                    .position(|e| e.id == *id)
                    .ok_or_else(|| format!("实体 {id} 不存在"))?;
                let entity = scene.entities.remove(idx);
                Ok(Op::RestoreEntity { index: idx, entity })
            }
            Op::RestoreEntity { index, entity } => {
                if scene.entity(entity.id).is_some() {
                    return Err(format!("实体 id {} 已存在", entity.id));
                }
                let at = (*index).min(scene.entities.len());
                let id = entity.id;
                scene.entities.insert(at, entity.clone());
                if id >= scene.next_id {
                    scene.next_id = id + 1;
                }
                Ok(Op::DestroyEntity { id })
            }
            Op::RenameEntity { id, name } => {
                let e = scene
                    .entity_mut(*id)
                    .ok_or_else(|| format!("实体 {id} 不存在"))?;
                let old = std::mem::replace(&mut e.name, name.clone());
                Ok(Op::RenameEntity { id: *id, name: old })
            }
            Op::SetTransform { id, transform } => {
                let e = scene
                    .entity_mut(*id)
                    .ok_or_else(|| format!("实体 {id} 不存在"))?;
                let old = std::mem::replace(&mut e.transform, *transform);
                Ok(Op::SetTransform {
                    id: *id,
                    transform: old,
                })
            }
            Op::AddComponent { id, component } => {
                forge_scene::validate_component(component)?;
                let e = scene
                    .entity_mut(*id)
                    .ok_or_else(|| format!("实体 {id} 不存在"))?;
                if e.component(&component.ctype).is_some() {
                    return Err(format!("实体 {id} 已有组件 {}", component.ctype));
                }
                e.components.push(component.clone());
                Ok(Op::RemoveComponent {
                    id: *id,
                    ctype: component.ctype.clone(),
                })
            }
            Op::InsertComponent {
                id,
                index,
                component,
            } => {
                let e = scene
                    .entity_mut(*id)
                    .ok_or_else(|| format!("实体 {id} 不存在"))?;
                if e.component(&component.ctype).is_some() {
                    return Err(format!("实体 {id} 已有组件 {}", component.ctype));
                }
                let at = (*index).min(e.components.len());
                e.components.insert(at, component.clone());
                Ok(Op::RemoveComponent {
                    id: *id,
                    ctype: component.ctype.clone(),
                })
            }
            Op::RemoveComponent { id, ctype } => {
                let e = scene
                    .entity_mut(*id)
                    .ok_or_else(|| format!("实体 {id} 不存在"))?;
                let idx = e
                    .components
                    .iter()
                    .position(|c| c.ctype == *ctype)
                    .ok_or_else(|| format!("实体 {id} 无组件 {ctype}"))?;
                let old = e.components.remove(idx);
                Ok(Op::InsertComponent {
                    id: *id,
                    index: idx,
                    component: old,
                })
            }
            Op::SetComponent { id, component } => {
                forge_scene::validate_component(component)?;
                let e = scene
                    .entity_mut(*id)
                    .ok_or_else(|| format!("实体 {id} 不存在"))?;
                let slot = e
                    .component_mut(&component.ctype)
                    .ok_or_else(|| format!("实体 {id} 无组件 {}", component.ctype))?;
                let old = std::mem::replace(slot, component.clone());
                Ok(Op::SetComponent {
                    id: *id,
                    component: old,
                })
            }
        }
    }
}

/// 宿主共享状态(单 Mutex 全量守护,简洁优先)。
pub struct HostState {
    /// 编辑态场景(唯一真相源;play 态冻结但可检视语义由 active 选择保证)。
    pub scene: Scene,
    /// 运行态场景(play.enter 时克隆编辑态;exit 销毁)。
    pub run_scene: Option<Scene>,
    /// PIE 状态。
    pub play: PlayState,
    /// undo 栈(存逆操作)。
    undo: Vec<Op>,
    /// redo 栈(存正操作)。
    redo: Vec<Op>,
    /// checkpoint 快照栈(编辑态)。
    pub checkpoints: Vec<Scene>,
    /// 物理世界(后端不可构造时为 None,summary 如实上报)。
    pub physics: Option<PhysicsWorld>,
    /// 实际后端名("jolt"/"rapier"/"none")。
    pub backend: String,
    /// 成功固定步数。
    pub steps: u64,
    /// step 错误计数(不 panic)。
    pub step_errors: u64,
    /// 已渲染帧数。
    pub frames: u64,
    /// 最近一帧三角形数。
    pub last_tris: usize,
    /// 最近一帧非零像素数。
    pub last_nonzero: usize,
    /// CPU 上传帧计数(F1 wave.3 零拷贝证据面:zero_copy 档恒 0 增量)。
    pub cpu_uploads: u64,
    /// 事件 ring(cap 1024)。
    pub events: VecDeque<Value>,
    /// 启动时刻(uptime 计算)。
    pub started: Instant,
    /// 编辑器相机(wave.2;视口状态,非场景状态——不入 .rxscene、不参与 undo)。
    pub camera: crate::viewport::EditorCamera,
    /// H.264 编码器状态(F1 wave.4 流腿;懒加载,尺寸变化重建)。
    pub h264: H264State,
}

/// H.264 编码器状态(F1 wave.4):懒加载 + 尺寸变化重建 + 帧计数。
pub struct H264State {
    encoder: Option<openh264::encoder::Encoder>,
    width: u32,
    height: u32,
    /// 已编码帧数(关键帧周期判定:每 60 帧一 IDR)。
    encoded: u64,
}

impl H264State {
    pub fn new() -> Self {
        H264State { encoder: None, width: 0, height: 0, encoded: 0 }
    }

    /// RGBA8 → I420(YUV420)转换(BT.601 有限范围;2x2 块采 U/V)。
    fn rgba_to_i420(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
        let (w, h) = (w as usize, h as usize);
        let mut yuv = vec![0u8; w * h * 3 / 2];
        let (ys, us, vs) = (0, w * h, w * h * 5 / 4);
        for j in 0..h {
            for i in 0..w {
                let si = (j * w + i) * 4;
                let (r, g, b) = (rgba[si] as f32, rgba[si + 1] as f32, rgba[si + 2] as f32);
                yuv[ys + j * w + i] = (0.299 * r + 0.587 * g + 0.114 * b).clamp(0.0, 255.0) as u8;
                if j % 2 == 0 && i % 2 == 0 {
                    let ci = (j / 2) * (w / 2) + (i / 2);
                    yuv[us + ci] = (-0.169 * r - 0.331 * g + 0.5 * b + 128.0).clamp(0.0, 255.0) as u8;
                    yuv[vs + ci] = (0.5 * r - 0.419 * g - 0.081 * b + 128.0).clamp(0.0, 255.0) as u8;
                }
            }
        }
        yuv
    }

    /// 编码一帧 RGBA8 → Annex B H.264 码流;返回 (码流字节, 是否关键帧)。
    pub fn encode_frame(&mut self, rgba: &[u8], w: u32, h: u32) -> Result<(Vec<u8>, bool), String> {
        if self.encoder.is_none() || self.width != w || self.height != h {
            // openh264 0.6:尺寸从 YUVSource 读(编码器自动重建),config 仅调码率/帧率。
            let config = openh264::encoder::EncoderConfig::new()
                .set_bitrate_bps(2_000_000)
                .max_frame_rate(60.0);
            self.encoder = Some(
                openh264::encoder::Encoder::with_api_config(
                    openh264::OpenH264API::from_source(),
                    config,
                )
                .map_err(|e| format!("H.264 编码器创建失败({w}x{h}): {e:?}"))?,
            );
            self.width = w;
            self.height = h;
            self.encoded = 0;
        }
        let yuv = Self::rgba_to_i420(rgba, w, h);
        let buf = openh264::formats::YUVBuffer::from_vec(yuv, w as usize, h as usize);
        let enc = self.encoder.as_mut().expect("刚初始化");
        let stream = enc.encode(&buf).map_err(|e| format!("H.264 编码失败: {e:?}"))?;
        let keyframe = self.encoded % 60 == 0;
        self.encoded += 1;
        Ok((stream.to_vec(), keyframe))
    }
}


impl HostState {
    /// 初始化:建物理世界(按 Jolt→Rapier 序取首个可构造后端;全失败则 None)。
    pub fn new() -> Self {
        let (world, backend) = create_world();
        let mut events = VecDeque::with_capacity(16);
        events.push_back(json!({
            "ts": utc_now_iso8601(),
            "event": "host.started",
            "backend": backend,
        }));
        HostState {
            scene: Scene::new("Untitled"),
            run_scene: None,
            play: PlayState::Edit,
            undo: Vec::new(),
            redo: Vec::new(),
            checkpoints: Vec::new(),
            physics: world,
            backend: backend.to_string(),
            steps: 0,
            step_errors: 0,
            frames: 0,
            last_tris: 0,
            last_nonzero: 0,
            cpu_uploads: 0,
            h264: H264State::new(),
            events,
            started: Instant::now(),
            camera: crate::viewport::EditorCamera::default(),
        }
    }

    /// uptime(秒)。
    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// 当前活动场景(play 态为运行态,否则编辑态)。
    fn active(&self) -> &Scene {
        match self.play {
            PlayState::Edit => &self.scene,
            _ => self.run_scene.as_ref().unwrap_or(&self.scene),
        }
    }

    /// 当前活动场景(可变)。
    fn active_mut(&mut self) -> &mut Scene {
        match self.play {
            PlayState::Edit => &mut self.scene,
            _ => self.run_scene.as_mut().unwrap_or(&mut self.scene),
        }
    }

    /// 变更类操作统一入口:应用到活动场景,逆操作压 undo 栈,清 redo 栈。
    fn apply_tracked(&mut self, op: Op) -> Result<(), String> {
        let inverse = op.apply(self.active_mut())?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }
}

/// 加锁(毒化时取回内部值,不 panic)。
pub fn lock(s: &Mutex<HostState>) -> MutexGuard<'_, HostState> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// 追加事件(cap 1024,满则丢最旧)。
pub fn push_event(st: &mut HostState, event: &str, extra: Value) {
    let mut v = json!({ "ts": utc_now_iso8601(), "event": event });
    if let (Value::Object(m), Value::Object(e)) = (&mut v, extra) {
        m.extend(e);
    }
    if st.events.len() >= EVENT_RING_CAP {
        st.events.pop_front();
    }
    st.events.push_back(v);
}

/// 建物理世界:Jolt 生产默认优先,未编译则 Rapier 快路径;全失败 → (None, "none")。
fn create_world() -> (Option<PhysicsWorld>, &'static str) {
    for (kind, name) in [(BackendKind::Jolt, "jolt"), (BackendKind::Rapier, "rapier")] {
        let desc = WorldDesc {
            backend: kind,
            dt_fixed: DT_FIXED,
            ..WorldDesc::default()
        };
        match PhysicsWorld::new(desc) {
            Ok(w) => return (Some(w), name),
            Err(_) => continue, // 后端未编译/不可用 → 试下一个(宿主层如实记录)
        }
    }
    (None, "none")
}

/// 构造 JSON-RPC result 响应。
fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// 构造 JSON-RPC error 响应。
pub fn err(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

// ---------- 参数解析辅助 ----------

/// 处理器结果:Ok(result) 或 (code, message)。
type HResult = Result<Value, (i64, String)>;

fn param_err<T>(msg: impl Into<String>) -> Result<T, (i64, String)> {
    Err((-32602, msg.into()))
}

fn domain_err<T>(msg: impl Into<String>) -> Result<T, (i64, String)> {
    Err((-32000, msg.into()))
}

/// 取 params.id(u64,必传)。
fn req_id(params: &Value) -> Result<u64, (i64, String)> {
    params
        .get("id")
        .and_then(Value::as_u64)
        .ok_or((-32602, "invalid params: id 须为非负整数".to_string()))
}

/// 取 params.name(字符串,必传)。
fn req_name(params: &Value) -> Result<String, (i64, String)> {
    params
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or((-32602, "invalid params: name 须为字符串".to_string()))
}

/// 取 params.type(组件类型名,必传)。
fn req_ctype(params: &Value) -> Result<String, (i64, String)> {
    params
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or((-32602, "invalid params: type 须为字符串".to_string()))
}

/// 解析定长 f32 数组(translation [3] / rotation [4])。
fn parse_f32_array<const N: usize>(v: &Value, field: &str) -> Result<[f32; N], (i64, String)> {
    let arr = v
        .as_array()
        .ok_or((-32602, format!("invalid params: {field} 须为 {N} 元素数组")))?;
    if arr.len() != N {
        return param_err(format!("invalid params: {field} 须为 {N} 元素数组"));
    }
    let mut out = [0.0f32; N];
    for (i, x) in arr.iter().enumerate() {
        out[i] = x
            .as_f64()
            .ok_or((-32602, format!("invalid params: {field}[{i}] 须为数值")))?
            as f32;
    }
    Ok(out)
}

/// 从 params 合并变换:未给字段沿用 old。
fn merge_transform(old: Transform, params: &Value) -> Result<Transform, (i64, String)> {
    let mut t = old;
    if let Some(v) = params.get("translation") {
        t.translation = parse_f32_array(v, "translation")?;
    }
    if let Some(v) = params.get("rotation") {
        t.rotation = parse_f32_array(v, "rotation")?;
    }
    if let Some(v) = params.get("scale") {
        t.scale = parse_f32_array(v, "scale")?;
    }
    Ok(t)
}

/// 从 JSON 值解析组件实例({type, enabled?, props?};props 缺省 {})。
fn parse_component(v: &Value) -> Result<Component, (i64, String)> {
    let obj = v
        .as_object()
        .ok_or((-32602, "invalid params: 组件须为对象".to_string()))?;
    let ctype = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or((-32602, "invalid params: 组件缺 type".to_string()))?
        .to_string();
    let enabled = match obj.get("enabled") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return param_err("invalid params: enabled 须为布尔"),
    };
    let props = match obj.get("props") {
        None | Some(Value::Null) => json!({}),
        Some(p) if p.is_object() => p.clone(),
        Some(_) => return param_err("invalid params: props 须为对象"),
    };
    let c = Component {
        ctype,
        enabled,
        props,
    };
    forge_scene::validate_component(&c).map_err(|e| (-32602, format!("invalid params: {e}")))?;
    Ok(c)
}

/// 解析组件数组参数(create 用)。
fn parse_components_opt(params: &Value) -> Result<Vec<Component>, (i64, String)> {
    match params.get("components") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a.iter().map(parse_component).collect(),
        Some(_) => param_err("invalid params: components 须为数组"),
    }
}

/// 解析单个 batchApply op 为 Op。
fn parse_batch_op(v: &Value, scene: &Scene) -> Result<Op, (i64, String)> {
    let op_name = v
        .get("op")
        .and_then(Value::as_str)
        .ok_or((-32602, "invalid params: op 缺 op 字段".to_string()))?;
    match op_name {
        "create" => {
            let name = v
                .get("name")
                .and_then(Value::as_str)
                .ok_or((-32602, "invalid params: create 缺 name".to_string()))?;
            let components = parse_components_opt(v)?;
            let transform = merge_transform(Transform::default(), v)?;
            let _ = scene;
            // id 占位为 0,由 batch 入口统一预分配(见 entity_batch_apply)。
            Ok(Op::CreateEntity {
                id: 0,
                name: name.to_string(),
                transform,
                components,
            })
        }
        "transform_set" => {
            let id = v
                .get("id")
                .and_then(Value::as_u64)
                .ok_or((-32602, "invalid params: transform_set 缺 id".to_string()))?;
            let old = scene
                .entity(id)
                .ok_or((-32000, format!("实体 {id} 不存在")))?
                .transform;
            let t = merge_transform(old, v)?;
            Ok(Op::SetTransform { id, transform: t })
        }
        "component_set" => {
            let id = v
                .get("id")
                .and_then(Value::as_u64)
                .ok_or((-32602, "invalid params: component_set 缺 id".to_string()))?;
            let ctype = v
                .get("type")
                .and_then(Value::as_str)
                .ok_or((-32602, "invalid params: component_set 缺 type".to_string()))?;
            let e = scene
                .entity(id)
                .ok_or((-32000, format!("实体 {id} 不存在")))?;
            let old = e
                .component(ctype)
                .ok_or((-32000, format!("实体 {id} 无组件 {ctype}")))?;
            let mut new = old.clone();
            if let Some(p) = v.get("props") {
                if !p.is_object() {
                    return param_err("invalid params: props 须为对象");
                }
                new.props = p.clone();
            }
            if let Some(b) = v.get("enabled") {
                new.enabled = b
                    .as_bool()
                    .ok_or((-32602, "invalid params: enabled 须为布尔".to_string()))?;
            }
            forge_scene::validate_component(&new)
                .map_err(|e| (-32602, format!("invalid params: {e}")))?;
            Ok(Op::SetComponent {
                id,
                component: new,
            })
        }
        other => param_err(format!("invalid params: 未知 op:{other}")),
    }
}

/// 场景默认落盘路径:<cwd>/data/scene.rxscene。
fn default_scene_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("data")
        .join("scene.rxscene")
}

/// 分派单条请求(请求已合法解析为 JSON;坏 JSON 由连接层回 -32700)。
pub fn dispatch(state: &Mutex<HostState>, req: &Value) -> Value {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = match req.get("method").and_then(Value::as_str) {
        Some(m) => m,
        None => return err(id, -32600, "invalid request: 缺 method"),
    };
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let mut st = lock(state);
    match handle(&mut st, method, &params) {
        Ok(result) => ok(id, result),
        Err((code, msg)) => err(id, code, &msg),
    }
}

/// 方法分派(持锁内)。
fn handle(st: &mut HostState, method: &str, params: &Value) -> HResult {
    match method {
        "host.ping" => Ok(json!({
            "pong": true,
            "version": env!("CARGO_PKG_VERSION"),
            "uptimeSec": st.uptime_secs(),
            "backend": st.backend,
            "pid": std::process::id(),
        })),
        "scene.new" => scene_new(st, params),
        "scene.summary" => Ok(scene_summary(st)),
        "render.once" => Ok(render_once(st)),
        "events.drain" => {
            let drained: Vec<Value> = st.events.drain(..).collect();
            Ok(Value::Array(drained))
        }
        "entity.create" => entity_create(st, params),
        "entity.destroy" => entity_destroy(st, params),
        "entity.rename" => entity_rename(st, params),
        "entity.get" => entity_get(st, params),
        "entity.list" => Ok(json!({ "entities": st.active().entities.iter().map(|e| json!(e)).collect::<Vec<_>>() })),
        "entity.batchApply" => entity_batch_apply(st, params),
        "component.add" => component_add(st, params),
        "component.remove" => component_remove(st, params),
        "component.set" => component_set(st, params),
        "component.get" => component_get(st, params),
        "component.listTypes" => Ok(forge_scene::list_types_json()),
        "transform.set" => transform_set(st, params),
        "transform.get" => transform_get(st, params),
        "transform.batchSet" => transform_batch_set(st, params),
        "scene.save" => scene_save(st, params),
        "scene.load" => scene_load(st, params),
        "scene.diff" => scene_diff(st, params),
        "scene.checkpoint" => scene_checkpoint(st),
        "scene.rollback" => scene_rollback(st),
        "edit.undo" => edit_undo(st),
        "edit.redo" => edit_redo(st),
        "play.enter" => play_enter(st),
        "play.pause" => play_pause(st),
        "play.resume" => play_resume(st),
        "play.step" => play_step(st),
        "play.exit" => play_exit(st),
        "play.state" => Ok(json!({ "state": st.play.as_str() })),
        "viewport.frame" => viewport_frame(st, params),
        "viewport.setCamera" => viewport_set_camera(st, params),
        "viewport.getCamera" => Ok(st.camera.to_json()),
        "viewport.pick" => viewport_pick(st, params),
        "viewport.shareOpen" => viewport_share_open(params),
        "viewport.shareClose" => {
            crate::share::close();
            Ok(json!({ "closed": true }))
        }
        _ => Err((-32601, format!("method not found: {method}"))),
    }
}

// ---------- F0 既有方法 ----------

fn scene_new(st: &mut HostState, params: &Value) -> HResult {
    let name = match params.get("name") {
        None | Some(Value::Null) => "Untitled".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return param_err("invalid params: name 须为字符串"),
    };
    if !params.is_null() && !params.is_object() {
        return param_err("invalid params: 须为对象");
    }
    if st.play != PlayState::Edit {
        return domain_err("play 态禁止 scene.new,请先 play.exit");
    }
    st.apply_tracked(Op::ReplaceScene(Scene::new(&name)))
        .map_err(|e| (-32000, e))?;
    push_event(st, "scene.created", json!({ "name": name }));
    let s = st.scene.summary();
    Ok(json!({ "name": s.name, "entityCount": s.entity_count }))
}

fn scene_summary(st: &HostState) -> Value {
    let s = st.active().summary();
    json!({
        "name": s.name,
        "entityCount": s.entity_count,
        "playState": st.play.as_str(),
        "physics": {
            "backend": st.backend,
            "steps": st.steps,
            "stepErrors": st.step_errors,
            "dtFixed": DT_FIXED,
        },
        "render": {
            "frames": st.frames,
            "lastTris": st.last_tris,
            "lastNonZeroPixels": st.last_nonzero,
        },
        "events": st.events.len(),
    })
}

/// CPU 软光栅渲一个三角形到内存,统计非零像素,frames+1。
fn render_once(st: &mut HostState) -> Value {
    let tri = soft_raster::Tri {
        v: [
            soft_raster::Vertex { x: 2.0, y: 2.0, z: 0.5, r: 0.9, g: 0.2, b: 0.1 },
            soft_raster::Vertex { x: 29.0, y: 4.0, z: 0.5, r: 0.9, g: 0.2, b: 0.1 },
            soft_raster::Vertex { x: 6.0, y: 21.0, z: 0.5, r: 0.9, g: 0.2, b: 0.1 },
        ],
    };
    let hdr = soft_raster::render_hdr(&[tri]);
    let nonzero = hdr.iter().filter(|p| **p != [0.0, 0.0, 0.0]).count();
    st.frames += 1;
    st.last_tris = 1;
    st.last_nonzero = nonzero;
    push_event(
        st,
        "render.frame",
        json!({ "frames": st.frames, "tris": 1, "nonZeroPixels": nonzero }),
    );
    json!({ "frames": st.frames, "tris": 1, "nonZeroPixels": nonzero })
}

// ---------- F1 wave.2 Viewport ----------

/// 取 width/height 参数(缺省 960×540;钳 16..=1920 / 16..=1080)。
fn viewport_size(params: &Value) -> Result<(u32, u32), (i64, String)> {
    let g = |k: &str, d: u32| params.get(k).and_then(Value::as_u64).map(|v| v as u32).unwrap_or(d);
    if !params.is_null() && !params.is_object() {
        return param_err("invalid params: 须为对象");
    }
    let w = g("width", 960).clamp(16, 1920);
    let h = g("height", 540).clamp(16, 1080);
    Ok((w, h))
}

/// viewport.frame:GPU 场景实渲染 + 回读;无设备 → DEV_ENV_DEGRADE 结构化错误(不充绿)。
/// `format` = "rgba8"(默认) | "h264"(F1 wave.4 流腿:Annex B 码流,供纯 web 客户端)。
fn viewport_frame(st: &mut HostState, params: &Value) -> HResult {
    let (w, h) = viewport_size(params)?;
    let format = params.get("format").and_then(Value::as_str).unwrap_or("rgba8");
    let selected = params.get("selectedId").and_then(Value::as_u64);
    let cam = st.camera;
    match crate::viewport::render_scene_frame(st.active(), &cam, selected, w, h) {
        Ok(f) => {
            // 帧通道(F1 wave.2/3):帧源唯一 = render_scene_frame。共享纹理开启时:
            // - 会话已 import(零拷贝档):VK 直渲进共享纹理,仅推进共享 fence;
            // - 未 import(readback 上传档):CPU 拷贝进共享纹理,cpu_uploads 计数。
            let mut frame_path = "no_share";
            if crate::share::is_open() {
                if f.imported {
                    crate::share::signal_frame()
                        .map_err(|e| (-32000, format!("共享 fence 信号失败: {e}")))?;
                    frame_path = "zero_copy";
                } else {
                    crate::share::write_frame(&f.rgba8, f.width, f.height)
                        .map_err(|e| (-32000, format!("共享纹理写入失败: {e}")))?;
                    st.cpu_uploads += 1;
                    frame_path = "readback_upload";
                }
            }
            st.frames += 1;
            st.last_tris = f.draws * 12;
            st.last_nonzero = f.nonzero;
            let frames = st.frames;
            let cpu_uploads = st.cpu_uploads;
            push_event(
                st,
                "viewport.frame",
                json!({ "frames": frames, "draws": f.draws, "nonZeroPixels": f.nonzero, "framePath": frame_path }),
            );
            if format == "h264" {
                let (nal, keyframe) = st
                    .h264
                    .encode_frame(&f.rgba8, f.width, f.height)
                    .map_err(|e| (-32000, format!("H.264 编码失败: {e}")))?;
                return Ok(json!({
                    "width": f.width,
                    "height": f.height,
                    "format": "h264",
                    "nalB64": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &nal),
                    "keyframe": keyframe,
                    "deviceName": f.device_name,
                    "draws": f.draws,
                    "truncated": f.truncated,
                    "frames": frames,
                    "nonZeroPixels": f.nonzero,
                    "framePath": frame_path,
                    "cpuUploads": cpu_uploads,
                }));
            }
            Ok(json!({
                "width": f.width,
                "height": f.height,
                "format": "rgba8",
                "pixelsB64": f.pixels_b64(),
                "deviceName": f.device_name,
                "draws": f.draws,
                "truncated": f.truncated,
                "frames": frames,
                "nonZeroPixels": f.nonzero,
                "framePath": frame_path,
                "cpuUploads": cpu_uploads,
            }))
        }
        Err(e) => Err((-32000, e)),
    }
}

/// viewport.setCamera:子集更新(target/yaw/pitch/dist/fovY),回显全量。
fn viewport_set_camera(st: &mut HostState, params: &Value) -> HResult {
    if !params.is_object() {
        return param_err("invalid params: 须为对象");
    }
    let mut cam = st.camera;
    if let Some(t) = params.get("target") {
        let a = t.as_array().ok_or((-32602, "invalid params: target 须为 [f32;3]".to_string()))?;
        if a.len() != 3 || !a.iter().all(|v| v.is_number()) {
            return param_err("invalid params: target 须为 3 数数组");
        }
        cam.target = [
            a[0].as_f64().unwrap() as f32,
            a[1].as_f64().unwrap() as f32,
            a[2].as_f64().unwrap() as f32,
        ];
    }
    let num = |k: &str| params.get(k).and_then(Value::as_f64).map(|v| v as f32);
    if let Some(v) = num("yaw") {
        cam.yaw_deg = v;
    }
    if let Some(v) = num("pitch") {
        cam.pitch_deg = v.clamp(-89.0, 89.0);
    }
    if let Some(v) = num("dist") {
        cam.dist = v.clamp(0.2, 500.0);
    }
    if let Some(v) = num("fovY") {
        cam.fov_y_deg = v.clamp(10.0, 120.0);
    }
    st.camera = cam;
    Ok(cam.to_json())
}

/// viewport.pick:屏幕像素坐标(左上原点) → 最近命中实体。
fn viewport_pick(st: &mut HostState, params: &Value) -> HResult {
    let (w, h) = viewport_size(params)?;
    let x = params
        .get("x")
        .and_then(Value::as_f64)
        .ok_or((-32602, "invalid params: 缺 x".to_string()))? as f32;
    let y = params
        .get("y")
        .and_then(Value::as_f64)
        .ok_or((-32602, "invalid params: 缺 y".to_string()))? as f32;
    let scene = st.active();
    match crate::viewport::pick_entity(scene, &st.camera, x, y, w, h) {
        Some((id, p)) => {
            let name = scene.entity(id).map(|e| e.name.clone()).unwrap_or_default();
            Ok(json!({ "hit": true, "entityId": id, "name": name, "point": p }))
        }
        None => Ok(json!({ "hit": false })),
    }
}

/// viewport.shareOpen:创建/重建 D3D12 共享纹理,句柄 DuplicateHandle 移交 pid 进程。
/// F1 wave.3 加堆腿:先探 VK import 内存需求,> committed 分配时共享堆 + placed resource。
fn viewport_share_open(params: &Value) -> HResult {
    let pid = params
        .get("pid")
        .and_then(Value::as_u64)
        .ok_or((-32602, "invalid params: 缺 pid".to_string()))? as u32;
    let (w, h) = viewport_size(params)?;
    let min_alloc = crate::viewport::probe_import_min_alloc(w, h).unwrap_or(0);
    crate::share::open(w, h, pid, min_alloc)
        .map(|(tex, fence, w, h, is_heap)| {
            json!({
                "texHandle": tex,
                "fenceHandle": fence,
                "width": w,
                "height": h,
                "format": "rgba8",
                "handleKind": if is_heap { "heap" } else { "resource" },
            })
        })
        .map_err(|e| (-32000, format!("共享纹理打开失败: {e}")))
}

// ---------- entity.* ----------

fn entity_create(st: &mut HostState, params: &Value) -> HResult {
    let name = req_name(params)?;
    let components = parse_components_opt(params)?;
    let transform = merge_transform(Transform::default(), params)?;
    let id = st.active().next_id;
    st.apply_tracked(Op::CreateEntity {
        id,
        name: name.clone(),
        transform,
        components,
    })
    .map_err(|e| (-32000, e))?;
    push_event(st, "entity.created", json!({ "id": id, "name": name }));
    let e = st.active().entity(id).expect("刚创建的实体须存在");
    Ok(json!({ "id": id, "entity": json!(e) }))
}

fn entity_destroy(st: &mut HostState, params: &Value) -> HResult {
    let id = req_id(params)?;
    st.apply_tracked(Op::DestroyEntity { id })
        .map_err(|e| (-32000, e))?;
    push_event(st, "entity.destroyed", json!({ "id": id }));
    Ok(json!({ "destroyed": id }))
}

fn entity_rename(st: &mut HostState, params: &Value) -> HResult {
    let id = req_id(params)?;
    let name = req_name(params)?;
    st.apply_tracked(Op::RenameEntity {
        id,
        name: name.clone(),
    })
    .map_err(|e| (-32000, e))?;
    push_event(st, "entity.renamed", json!({ "id": id, "name": name }));
    Ok(json!({ "id": id, "name": name }))
}

fn entity_get(st: &HostState, params: &Value) -> HResult {
    let id = req_id(params)?;
    let e = st
        .active()
        .entity(id)
        .ok_or((-32000, format!("实体 {id} 不存在")))?;
    Ok(json!(e))
}

fn entity_batch_apply(st: &mut HostState, params: &Value) -> HResult {
    let ops_val = params
        .get("ops")
        .and_then(Value::as_array)
        .ok_or((-32602, "invalid params: ops 须为数组".to_string()))?;
    // 预分配 create id(从当前 next_id 起连续编号,保证批内 transform_set 可引用)。
    let mut next = st.active().next_id;
    let mut ops = Vec::with_capacity(ops_val.len());
    for v in ops_val {
        let mut op = parse_batch_op(v, st.active())?;
        if let Op::CreateEntity { id, .. } = &mut op {
            *id = next;
            next += 1;
        }
        ops.push(op);
    }
    let n = ops.len();
    st.apply_tracked(Op::Batch(ops)).map_err(|e| (-32000, e))?;
    push_event(st, "entity.batchApplied", json!({ "count": n }));
    Ok(json!({ "applied": n }))
}

// ---------- component.* ----------

fn component_add(st: &mut HostState, params: &Value) -> HResult {
    let id = req_id(params)?;
    let c = parse_component(params)?;
    let ctype = c.ctype.clone();
    st.apply_tracked(Op::AddComponent { id, component: c })
        .map_err(|e| (-32000, e))?;
    push_event(st, "component.added", json!({ "id": id, "type": ctype }));
    Ok(json!({ "id": id, "type": ctype }))
}

fn component_remove(st: &mut HostState, params: &Value) -> HResult {
    let id = req_id(params)?;
    let ctype = req_ctype(params)?;
    st.apply_tracked(Op::RemoveComponent {
        id,
        ctype: ctype.clone(),
    })
    .map_err(|e| (-32000, e))?;
    push_event(st, "component.removed", json!({ "id": id, "type": ctype }));
    Ok(json!({ "id": id, "type": ctype }))
}

fn component_set(st: &mut HostState, params: &Value) -> HResult {
    let id = req_id(params)?;
    let ctype = req_ctype(params)?;
    let old = st
        .active()
        .entity(id)
        .ok_or((-32000, format!("实体 {id} 不存在")))?
        .component(&ctype)
        .ok_or((-32000, format!("实体 {id} 无组件 {ctype}")))?
        .clone();
    let mut new = old;
    let mut touched = false;
    if let Some(p) = params.get("props") {
        if !p.is_object() {
            return param_err("invalid params: props 须为对象");
        }
        new.props = p.clone();
        touched = true;
    }
    if let Some(b) = params.get("enabled") {
        new.enabled = b
            .as_bool()
            .ok_or((-32602, "invalid params: enabled 须为布尔".to_string()))?;
        touched = true;
    }
    if !touched {
        return param_err("invalid params: props/enabled 至少给一项");
    }
    forge_scene::validate_component(&new).map_err(|e| (-32602, format!("invalid params: {e}")))?;
    st.apply_tracked(Op::SetComponent { id, component: new })
        .map_err(|e| (-32000, e))?;
    push_event(st, "component.set", json!({ "id": id, "type": ctype }));
    Ok(json!({ "id": id, "type": ctype }))
}

fn component_get(st: &HostState, params: &Value) -> HResult {
    let id = req_id(params)?;
    let ctype = req_ctype(params)?;
    let c = st
        .active()
        .entity(id)
        .ok_or((-32000, format!("实体 {id} 不存在")))?
        .component(&ctype)
        .ok_or((-32000, format!("实体 {id} 无组件 {ctype}")))?;
    Ok(json!(c))
}

// ---------- transform.* ----------

fn transform_set(st: &mut HostState, params: &Value) -> HResult {
    let id = req_id(params)?;
    let old = st
        .active()
        .entity(id)
        .ok_or((-32000, format!("实体 {id} 不存在")))?
        .transform;
    let t = merge_transform(old, params)?;
    st.apply_tracked(Op::SetTransform { id, transform: t })
        .map_err(|e| (-32000, e))?;
    push_event(st, "transform.set", json!({ "id": id }));
    Ok(json!(t))
}

fn transform_get(st: &HostState, params: &Value) -> HResult {
    let id = req_id(params)?;
    let t = st
        .active()
        .entity(id)
        .ok_or((-32000, format!("实体 {id} 不存在")))?
        .transform;
    Ok(json!(t))
}

fn transform_batch_set(st: &mut HostState, params: &Value) -> HResult {
    let items = params
        .get("items")
        .and_then(Value::as_array)
        .ok_or((-32602, "invalid params: items 须为数组".to_string()))?;
    let mut ops = Vec::with_capacity(items.len());
    for v in items {
        let id = v
            .get("id")
            .and_then(Value::as_u64)
            .ok_or((-32602, "invalid params: items[].id 须为非负整数".to_string()))?;
        let old = st
            .active()
            .entity(id)
            .ok_or((-32000, format!("实体 {id} 不存在")))?
            .transform;
        let t = merge_transform(old, v)?;
        ops.push(Op::SetTransform { id, transform: t });
    }
    let n = ops.len();
    st.apply_tracked(Op::Batch(ops)).map_err(|e| (-32000, e))?;
    push_event(st, "transform.batchSet", json!({ "count": n }));
    Ok(json!({ "applied": n }))
}

// ---------- scene.* 存取 / diff / checkpoint ----------

fn scene_save(st: &HostState, params: &Value) -> HResult {
    let path = match params.get("path") {
        None | Some(Value::Null) => default_scene_path(),
        Some(Value::String(s)) => PathBuf::from(s),
        Some(_) => return param_err("invalid params: path 须为字符串"),
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // 保存编辑态场景(唯一真相源;play 态下编辑态冻结,保存它语义最稳)。
    let json = st.scene.to_json().map_err(|e| (-32000, e.to_string()))?;
    let bytes = json.len() + 1;
    st.scene
        .save(&path)
        .map_err(|e| (-32000, e.to_string()))?;
    Ok(json!({ "path": path.to_string_lossy(), "bytes": bytes }))
}

fn scene_load(st: &mut HostState, params: &Value) -> HResult {
    if st.play != PlayState::Edit {
        return domain_err("play 态禁止 scene.load,请先 play.exit");
    }
    let path = match params.get("path") {
        Some(Value::String(s)) => PathBuf::from(s),
        _ => return param_err("invalid params: path 必填且须为字符串"),
    };
    let scene = Scene::load(&path).map_err(|e| (-32000, e.to_string()))?;
    st.apply_tracked(Op::ReplaceScene(scene))
        .map_err(|e| (-32000, e))?;
    push_event(st, "scene.loaded", json!({ "path": path.to_string_lossy() }));
    let s = st.scene.summary();
    Ok(json!({ "name": s.name, "entityCount": s.entity_count }))
}

fn scene_diff(st: &HostState, params: &Value) -> HResult {
    let path = match params.get("path") {
        None | Some(Value::Null) => default_scene_path(),
        Some(Value::String(s)) => PathBuf::from(s),
        Some(_) => return param_err("invalid params: path 须为字符串"),
    };
    let cur = st.scene.to_json().map_err(|e| (-32000, e.to_string()))?;
    let cur_disk = format!("{cur}\n");
    let on_disk = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            return Ok(json!({
                "same": false,
                "summary": format!("磁盘文件缺失:{}", path.display()),
            }));
        }
    };
    if on_disk == cur_disk {
        return Ok(json!({ "same": true, "summary": "与磁盘逐字节一致" }));
    }
    // 摘要不一致点:对比名称与实体数。
    let summary = match Scene::from_json(&on_disk) {
        Ok(disk) => {
            let ds = disk.summary();
            let cs = st.scene.summary();
            format!(
                "内容不同:内存(name={} entityCount={}) vs 磁盘(name={} entityCount={})",
                cs.name, cs.entity_count, ds.name, ds.entity_count
            )
        }
        Err(_) => "内容不同:磁盘文件非合法 .rxscene".to_string(),
    };
    Ok(json!({ "same": false, "summary": summary }))
}

fn scene_checkpoint(st: &mut HostState) -> HResult {
    st.checkpoints.push(st.scene.clone());
    let depth = st.checkpoints.len();
    push_event(st, "scene.checkpoint", json!({ "depth": depth }));
    Ok(json!({ "depth": depth }))
}

fn scene_rollback(st: &mut HostState) -> HResult {
    let snap = st
        .checkpoints
        .pop()
        .ok_or((-32000, "checkpoint 栈为空".to_string()))?;
    // rollback 是恢复操作,不进 undo 栈(与 checkpoint 配对使用)。
    st.scene = snap;
    let depth = st.checkpoints.len();
    push_event(st, "scene.rollback", json!({ "depth": depth }));
    let s = st.scene.summary();
    Ok(json!({ "depth": depth, "name": s.name, "entityCount": s.entity_count }))
}

// ---------- edit.undo / edit.redo ----------

fn edit_undo(st: &mut HostState) -> HResult {
    let inv = st
        .undo
        .pop()
        .ok_or((-32000, "undo 栈为空".to_string()))?;
    let fwd = inv.apply(st.active_mut()).map_err(|e| (-32000, e))?;
    st.redo.push(fwd);
    push_event(st, "edit.undo", json!({}));
    Ok(json!({ "undone": true }))
}

fn edit_redo(st: &mut HostState) -> HResult {
    let fwd = st
        .redo
        .pop()
        .ok_or((-32000, "redo 栈为空".to_string()))?;
    let inv = fwd.apply(st.active_mut()).map_err(|e| (-32000, e))?;
    st.undo.push(inv);
    push_event(st, "edit.redo", json!({}));
    Ok(json!({ "redone": true }))
}

// ---------- play.*(PIE 双态) ----------

fn play_enter(st: &mut HostState) -> HResult {
    if st.play != PlayState::Edit {
        return domain_err(format!("当前状态 {} 禁止 play.enter", st.play.as_str()));
    }
    st.run_scene = Some(st.scene.clone());
    st.play = PlayState::Running;
    // 跨场景切换,命令栈清空避免误作用。
    st.undo.clear();
    st.redo.clear();
    push_event(st, "play.enter", json!({}));
    Ok(json!({ "state": st.play.as_str() }))
}

fn play_pause(st: &mut HostState) -> HResult {
    if st.play != PlayState::Running {
        return domain_err(format!("当前状态 {} 禁止 play.pause", st.play.as_str()));
    }
    st.play = PlayState::Paused;
    push_event(st, "play.pause", json!({}));
    Ok(json!({ "state": st.play.as_str() }))
}

fn play_resume(st: &mut HostState) -> HResult {
    if st.play != PlayState::Paused {
        return domain_err(format!("当前状态 {} 禁止 play.resume", st.play.as_str()));
    }
    st.play = PlayState::Running;
    push_event(st, "play.resume", json!({}));
    Ok(json!({ "state": st.play.as_str() }))
}

fn play_step(st: &mut HostState) -> HResult {
    if st.play != PlayState::Paused {
        return domain_err(format!("当前状态 {} 禁止 play.step", st.play.as_str()));
    }
    if let Some(world) = st.physics.as_mut() {
        match world.step(DT_FIXED) {
            Ok(_) => st.steps += 1,
            Err(_) => st.step_errors += 1,
        }
    }
    push_event(st, "play.step", json!({}));
    Ok(json!({ "state": st.play.as_str(), "steps": st.steps }))
}

fn play_exit(st: &mut HostState) -> HResult {
    if st.play == PlayState::Edit {
        return domain_err("当前状态 edit 禁止 play.exit");
    }
    st.run_scene = None;
    st.play = PlayState::Edit;
    st.undo.clear();
    st.redo.clear();
    push_event(st, "play.exit", json!({}));
    Ok(json!({ "state": st.play.as_str() }))
}
