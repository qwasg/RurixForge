//! F7 wave.1 事件基座(D-F7-A,语义级自研,参考 I:\agent-debug-frontend-backend-copy-20260530
//! gateway-go/backend-rs agent-store/event_bus.rs 语义,不 fork 代码)。
//!
//! wire 格式:`{ id:"evt_*", sessionId, seq, type, ts(RFC3339), source:{domain,actor},
//! correlationId?, channel, payload }`;channel 由 type 前缀派生(session.*/agent.*/tool.*/
//! todo.*/plan.* → 同名 channel,其余 → "logs")。
//!
//! 语义:seq 会话内单调递增;每会话内存环缓冲(默认 4096,env FORGE_AGENTD_EVENT_BUFFER 可调,
//! 供冒烟用小窗口测 gap);append-only JSONL 持久化 data/agent-events/{sessionId}.jsonl
//! (启动时从磁盘重建环缓冲与 seq 计数器);emit()/emit_ephemeral() 二分(ephemeral 只广播
//! 不落盘不回放,但仍消耗 seq 以保持实时流 seq 单调)。
//!
//! 注:持久化为锁内同步 append(wave.1 事件量低;参考仓的独立写线程是性能优化非语义,
//! 后续量化需要时再升级)。revert 截断会重写 JSONL 并重建环缓冲(append-only 的唯一例外,
//! 即「截断语义」本身)。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::sync::broadcast;

/// 环缓冲默认容量(参考仓默认同值)。
pub const DEFAULT_BUFFER_CAP: usize = 4096;
/// 每会话实时广播通道容量(慢消费者 Lagged → SSE 侧合成 stream.gap,不阻塞发布方)。
const BROADCAST_CAP: usize = 1024;

/// type 前缀 → channel(session/agent/tool/todo/plan/goal/codex 同名,其余 logs)。
pub fn channel_for(event_type: &str) -> &'static str {
    let prefix = event_type.split('.').next().unwrap_or("");
    match prefix {
        "session" => "session",
        "agent" => "agent",
        "tool" => "tool",
        "todo" => "todo",
        "plan" => "plan",
        // 目标面独立成频道:GoalBar/GoalTab 只订它,不必在整条 agent 流里过滤。
        "goal" => "goal",
        // Codex 引擎面(账户/额度推送);与 agent 事件分开,状态栏订它就够。
        "codex" => "codex",
        _ => "logs",
    }
}

/// RFC3339(UTC,毫秒)时间戳;无 chrono 依赖,Howard Hinnant civil-from-days 换算。
pub fn now_rfc3339() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// days since 1970-01-01 → (year, month, day)(公历,Howard Hinnant 算法)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `evt_{毫秒}_{随机}`(无 uuid/rand 依赖:RandomState 随机种子哈希 + 进程内原子计数 + 纳秒)。
pub fn new_id(prefix: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    h.write_u128(now.as_nanos());
    h.write_u64(std::process::id() as u64);
    format!("{prefix}_{}_{:08x}", now.as_millis(), h.finish() as u32)
}

/// 事件本体(JSONL 落盘形态;channel 不落盘,to_wire 时由 type 派生)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugEvent {
    pub id: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub seq: i64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub ts: String,
    /// {"domain": "...", "actor": "..."}
    pub source: BTreeMap<String, String>,
    #[serde(rename = "correlationId", skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub payload: Value,
}

impl DebugEvent {
    pub fn channel(&self) -> &'static str {
        channel_for(&self.event_type)
    }

    /// wire JSON(含派生 channel;correlationId 缺省为 null,对齐参考 to_wire)。
    pub fn to_wire(&self) -> Value {
        json!({
            "id": self.id,
            "sessionId": self.session_id,
            "seq": self.seq,
            "type": self.event_type,
            "ts": self.ts,
            "source": self.source,
            "correlationId": self.correlation_id,
            "channel": self.channel(),
            "payload": self.payload,
        })
    }
}

/// 领域代码发事件的草稿(id/seq/ts 由 bus 赋)。
pub struct EventDraft {
    pub session_id: String,
    pub event_type: String,
    pub domain: String,
    pub actor: String,
    pub payload: Value,
    pub correlation_id: Option<String>,
}

impl EventDraft {
    pub fn new(session_id: impl Into<String>, event_type: impl Into<String>, domain: impl Into<String>) -> Self {
        EventDraft {
            session_id: session_id.into(),
            event_type: event_type.into(),
            domain: domain.into(),
            actor: "main".to_string(),
            payload: json!({}),
            correlation_id: None,
        }
    }

    pub fn payload(mut self, payload: Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn correlation(mut self, id: Option<String>) -> Self {
        self.correlation_id = id;
        self
    }
}

struct Inner {
    per_session: HashMap<String, VecDeque<DebugEvent>>,
    seq_by_session: HashMap<String, i64>,
}

/// 进程内事件总线:每会话环缓冲 + seq 计数 + 每会话 broadcast 实时扇出 + JSONL 持久化。
pub struct EventBus {
    inner: Mutex<Inner>,
    senders: Mutex<HashMap<String, broadcast::Sender<DebugEvent>>>,
    cap: usize,
    /// data/agent-events 目录。
    dir: PathBuf,
}

impl EventBus {
    /// 构造并立即从磁盘重建(扫描 dir/*.jsonl:环缓冲装尾部 cap 条,seq 计数器 = 最大 seq)。
    pub fn new(dir: PathBuf, cap: usize) -> Self {
        let bus = EventBus {
            inner: Mutex::new(Inner {
                per_session: HashMap::new(),
                seq_by_session: HashMap::new(),
            }),
            senders: Mutex::new(HashMap::new()),
            cap: cap.max(1),
            dir,
        };
        bus.rebuild_from_disk();
        bus
    }

    /// 默认构造:cap 读 env FORGE_AGENTD_EVENT_BUFFER(非法值回落 4096)。
    pub fn from_env(dir: PathBuf) -> Self {
        let cap = std::env::var("FORGE_AGENTD_EVENT_BUFFER")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(DEFAULT_BUFFER_CAP);
        Self::new(dir, cap)
    }

    fn rebuild_from_disk(&self) {
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            return; // 目录不存在 = 无历史
        };
        for ent in rd.flatten() {
            let path = ent.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(sid) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let events = read_jsonl(&path);
            if events.is_empty() {
                continue;
            }
            let max_seq = events.iter().map(|e| e.seq).max().unwrap_or(0);
            let skip = events.len().saturating_sub(self.cap);
            let mut inner = self.inner.lock().unwrap();
            let bucket = inner.per_session.entry(sid.to_string()).or_default();
            for ev in events.into_iter().skip(skip) {
                bucket.push_back(ev);
            }
            inner.seq_by_session.insert(sid.to_string(), max_seq);
        }
    }

    fn file_of(&self, session_id: &str) -> PathBuf {
        self.dir.join(format!("{session_id}.jsonl"))
    }

    /// 订阅会话实时流(每会话一个 broadcast channel,惰性创建)。
    pub fn subscribe(&self, session_id: &str) -> broadcast::Receiver<DebugEvent> {
        let mut senders = self.senders.lock().unwrap();
        senders
            .entry(session_id.to_string())
            .or_insert_with(|| broadcast::channel(BROADCAST_CAP).0)
            .subscribe()
    }

    fn broadcast(&self, event: &DebugEvent) {
        let tx = {
            let mut senders = self.senders.lock().unwrap();
            senders
                .entry(event.session_id.clone())
                .or_insert_with(|| broadcast::channel(BROADCAST_CAP).0)
                .clone()
        };
        // 无订阅者时 send 返回 Err,正常忽略。
        let _ = tx.send(event.clone());
    }

    /// 持久化 + 广播。返回落盘事件(含 id/seq/ts)。
    pub fn emit(&self, draft: EventDraft) -> DebugEvent {
        self.emit_inner(draft, true)
    }

    /// 只广播:不落盘、不进环缓冲(不回放);仍消耗 seq 保持实时流单调。
    pub fn emit_ephemeral(&self, draft: EventDraft) -> DebugEvent {
        self.emit_inner(draft, false)
    }

    fn emit_inner(&self, draft: EventDraft, persist: bool) -> DebugEvent {
        let mut source = BTreeMap::new();
        source.insert("domain".to_string(), draft.domain);
        source.insert("actor".to_string(), draft.actor);
        let event = {
            let mut inner = self.inner.lock().unwrap();
            let seq = {
                let entry = inner.seq_by_session.entry(draft.session_id.clone()).or_insert(0);
                *entry += 1;
                *entry
            };
            let event = DebugEvent {
                id: new_id("evt"),
                session_id: draft.session_id.clone(),
                seq,
                event_type: draft.event_type,
                ts: now_rfc3339(),
                source,
                correlation_id: draft.correlation_id,
                payload: draft.payload,
            };
            if persist {
                let bucket = inner.per_session.entry(draft.session_id.clone()).or_default();
                bucket.push_back(event.clone());
                while bucket.len() > self.cap {
                    bucket.pop_front();
                }
                // 锁内同步 append:保证文件行序 == seq 序(wave.1 量低,如实注释)。
                if let Err(e) = append_jsonl(&self.dir, &self.file_of(&draft.session_id), &event) {
                    eprintln!("event jsonl append 失败({}): {e}", draft.session_id);
                }
            }
            event
        };
        self.broadcast(&event);
        event
    }

    /// from_seq(不含)起回放环缓冲;gap = from_seq 早于环缓冲窗口。
    pub fn replay_since(&self, session_id: &str, from_seq: i64) -> (Vec<DebugEvent>, bool) {
        let inner = self.inner.lock().unwrap();
        let Some(bucket) = inner.per_session.get(session_id) else {
            return (Vec::new(), false);
        };
        if bucket.is_empty() {
            return (Vec::new(), false);
        }
        let oldest = bucket.front().map(|e| e.seq).unwrap_or(0);
        let gap = from_seq + 1 < oldest;
        let out: Vec<DebugEvent> = bucket.iter().filter(|e| e.seq > from_seq).cloned().collect();
        (out, gap)
    }

    /// 环缓冲全量(内存窗口内)。
    pub fn snapshot(&self, session_id: &str) -> Vec<DebugEvent> {
        let inner = self.inner.lock().unwrap();
        inner
            .per_session
            .get(session_id)
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn latest_seq(&self, session_id: &str) -> i64 {
        let inner = self.inner.lock().unwrap();
        *inner.seq_by_session.get(session_id).unwrap_or(&0)
    }

    /// 会话磁盘全量(不经环缓冲窗口;fork/revert 用,保真全文)。
    pub fn persisted(&self, session_id: &str) -> Vec<DebugEvent> {
        read_jsonl(&self.file_of(session_id))
    }

    /// 删除会话:内存(环缓冲/seq/广播 channel)+ 事件文件。
    pub fn purge_session(&self, session_id: &str) {
        {
            let mut inner = self.inner.lock().unwrap();
            inner.per_session.remove(session_id);
            inner.seq_by_session.remove(session_id);
        }
        self.senders.lock().unwrap().remove(session_id);
        std::fs::remove_file(self.file_of(session_id)).ok();
    }

    /// 克隆事件流:逐条复制(sessionId 换新,seq/id/ts/payload 保持),重写目标 JSONL
    /// 并重建目标环缓冲与 seq 计数器。不发实时广播(克隆的是历史)。
    pub fn fork_events(&self, old_session_id: &str, new_session_id: &str) {
        let mut events = self.persisted(old_session_id);
        for ev in &mut events {
            ev.session_id = new_session_id.to_string();
        }
        self.replace_session_log(new_session_id, &events);
    }

    /// 截断到 seq <= max_seq:重写 JSONL + 重建环缓冲 + 回卷 seq 计数器。
    pub fn truncate_to_seq(&self, session_id: &str, max_seq: i64) {
        let events: Vec<DebugEvent> = self
            .persisted(session_id)
            .into_iter()
            .filter(|e| e.seq <= max_seq)
            .collect();
        self.replace_session_log(session_id, &events);
    }

    /// 按事件 id 查 seq(磁盘全文;不存在 → None)。
    pub fn seq_of_event(&self, session_id: &str, event_id: &str) -> Option<i64> {
        self.persisted(session_id)
            .into_iter()
            .find(|e| e.id == event_id)
            .map(|e| e.seq)
    }

    /// 重写会话 JSONL 为给定事件集(原子:tmp + rename),并重建内存态。
    fn replace_session_log(&self, session_id: &str, events: &[DebugEvent]) {
        if let Err(e) = write_jsonl_atomic(&self.dir, &self.file_of(session_id), events) {
            eprintln!("event jsonl rewrite 失败({session_id}): {e}");
        }
        let max_seq = events.iter().map(|e| e.seq).max().unwrap_or(0);
        let skip = events.len().saturating_sub(self.cap);
        let mut inner = self.inner.lock().unwrap();
        let bucket = inner.per_session.entry(session_id.to_string()).or_default();
        *bucket = events.iter().skip(skip).cloned().collect();
        inner.seq_by_session.insert(session_id.to_string(), max_seq);
    }
}

fn read_jsonl(path: &Path) -> Vec<DebugEvent> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| {
            serde_json::from_str::<DebugEvent>(l)
                .map_err(|e| eprintln!("event jsonl 行解析失败({}): {e}", path.display()))
                .ok()
        })
        .collect()
}

fn append_jsonl(dir: &Path, path: &Path, event: &DebugEvent) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())
}

/// 原子重写:tmp 写全量 + rename(Windows std rename 覆盖已存在目标)。
fn write_jsonl_atomic(dir: &Path, path: &Path, events: &[DebugEvent]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut text = String::new();
    for ev in events {
        text.push_str(&serde_json::to_string(ev).map_err(std::io::Error::other)?);
        text.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentd-events-{tag}-{}-{}",
            std::process::id(),
            new_id("t")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn draft(sid: &str, etype: &str) -> EventDraft {
        EventDraft::new(sid, etype, "agent").payload(json!({ "n": etype }))
    }

    fn seqs(events: &[DebugEvent]) -> Vec<i64> {
        events.iter().map(|e| e.seq).collect()
    }

    #[test]
    fn channel_derivation() {
        assert_eq!(channel_for("session.created"), "session");
        assert_eq!(channel_for("agent.started"), "agent");
        assert_eq!(channel_for("tool.invoked"), "tool");
        assert_eq!(channel_for("todo.updated"), "todo");
        assert_eq!(channel_for("plan.generated"), "plan");
        assert_eq!(channel_for("stream.gap"), "logs");
        assert_eq!(channel_for("composer.user.message"), "logs");
    }

    #[test]
    fn seq_monotonic_and_wire_shape() {
        let dir = temp_dir("seq");
        let bus = EventBus::new(dir.clone(), 16);
        let e1 = bus.emit(draft("s1", "agent.started"));
        let e2 = bus.emit(draft("s1", "agent.message"));
        let e3 = bus.emit(draft("s2", "agent.started"));
        assert_eq!((e1.seq, e2.seq), (1, 2));
        assert_eq!(e3.seq, 1, "seq 会话内独立");
        let wire = e1.to_wire();
        assert!(wire["id"].as_str().unwrap().starts_with("evt_"));
        assert_eq!(wire["sessionId"], "s1");
        assert_eq!(wire["type"], "agent.started");
        assert_eq!(wire["channel"], "agent");
        assert_eq!(wire["source"]["domain"], "agent");
        assert_eq!(wire["source"]["actor"], "main");
        assert!(wire["ts"].as_str().unwrap().ends_with('Z'));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn replay_no_dup_no_loss() {
        let dir = temp_dir("replay");
        let bus = EventBus::new(dir.clone(), 64);
        for i in 1..=10 {
            bus.emit(draft("s", &format!("e{i}")));
        }
        let (all, gap) = bus.replay_since("s", 0);
        assert!(!gap);
        assert_eq!(seqs(&all), (1..=10).collect::<Vec<_>>(), "全量回放无重无漏");
        let (tail, gap2) = bus.replay_since("s", 4);
        assert!(!gap2);
        assert_eq!(seqs(&tail), vec![5, 6, 7, 8, 9, 10], "续传从 from_seq+1 起");
        let (none, gap3) = bus.replay_since("s", 10);
        assert!(!gap3 && none.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ring_buffer_window_gap_semantics() {
        let dir = temp_dir("ring");
        let bus = EventBus::new(dir.clone(), 4);
        for i in 1..=6 {
            bus.emit(draft("s", &format!("e{i}")));
        }
        // from_seq 早于窗口(oldest=3):gap + 仅窗口内 4 条。
        let (events, gap) = bus.replay_since("s", 1);
        assert!(gap, "from_seq=1 早于窗口须 gap");
        assert_eq!(seqs(&events), vec![3, 4, 5, 6]);
        // 边界:from_seq=2,窗口 oldest=3,2+1<3 不成立 → 无 gap 且无漏(2 已见)。
        let (events2, gap2) = bus.replay_since("s", 2);
        assert!(!gap2);
        assert_eq!(seqs(&events2), vec![3, 4, 5, 6]);
        // 最新处:空且无 gap。
        let (events3, gap3) = bus.replay_since("s", 6);
        assert!(!gap3 && events3.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn ephemeral_broadcast_only_not_persisted_not_replayed() {
        let dir = temp_dir("eph");
        let bus = EventBus::new(dir.clone(), 16);
        let mut rx = bus.subscribe("s");
        let e1 = bus.emit(draft("s", "session.created"));
        let eph = bus.emit_ephemeral(draft("s", "agent.delta"));
        let e2 = bus.emit(draft("s", "session.updated"));
        // 实时广播三者皆收(ephemeral 消耗 seq 保单调)。
        assert_eq!(rx.recv().await.unwrap().seq, e1.seq);
        assert_eq!(rx.recv().await.unwrap().id, eph.id);
        assert_eq!(rx.recv().await.unwrap().seq, e2.seq);
        assert_eq!((e1.seq, eph.seq, e2.seq), (1, 2, 3));
        // 回放:无 ephemeral。
        let (events, _) = bus.replay_since("s", 0);
        assert_eq!(seqs(&events), vec![1, 3]);
        // 落盘:仅 2 行且无 agent.delta。
        let text = std::fs::read_to_string(dir.join("s.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(!text.contains("agent.delta"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn persistence_rebuild_replays_identically_and_continues_seq() {
        let dir = temp_dir("rebuild");
        {
            let bus = EventBus::new(dir.clone(), 64);
            for i in 1..=5 {
                bus.emit(draft("s", &format!("e{i}")));
            }
        } // drop = 进程重启语义
        let bus = EventBus::new(dir.clone(), 64);
        let (events, gap) = bus.replay_since("s", 0);
        assert!(!gap);
        assert_eq!(seqs(&events), vec![1, 2, 3, 4, 5], "重建后回放一致");
        assert_eq!(events[0].event_type, "e1");
        let next = bus.emit(draft("s", "after-restart"));
        assert_eq!(next.seq, 6, "seq 计数器重建后续接");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fork_clones_stream_with_new_session_id_same_seq() {
        let dir = temp_dir("fork");
        let bus = EventBus::new(dir.clone(), 64);
        for i in 1..=3 {
            bus.emit(draft("old", &format!("e{i}")));
        }
        bus.fork_events("old", "new");
        let old = bus.snapshot("old");
        let new = bus.snapshot("new");
        assert_eq!(seqs(&old), seqs(&new), "seq 保持");
        assert!(new.iter().all(|e| e.session_id == "new"), "sessionId 换新");
        assert_eq!(
            old.iter().map(|e| &e.event_type).collect::<Vec<_>>(),
            new.iter().map(|e| &e.event_type).collect::<Vec<_>>()
        );
        // 新会话 JSONL 独立落盘且可重建。
        let bus2 = EventBus::new(dir.clone(), 64);
        assert_eq!(bus2.snapshot("new").len(), 3);
        assert_eq!(bus2.latest_seq("new"), 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn truncate_rewrites_log_and_rewinds_seq() {
        let dir = temp_dir("trunc");
        let bus = EventBus::new(dir.clone(), 64);
        for i in 1..=5 {
            bus.emit(draft("s", &format!("e{i}")));
        }
        bus.truncate_to_seq("s", 3);
        assert_eq!(seqs(&bus.snapshot("s")), vec![1, 2, 3]);
        assert_eq!(bus.latest_seq("s"), 3);
        let text = std::fs::read_to_string(dir.join("s.jsonl")).unwrap();
        assert_eq!(text.lines().count(), 3, "JSONL 已重写");
        // 回卷后新事件 seq 复接。
        assert_eq!(bus.emit(draft("s", "e-new")).seq, 4);
        // 重建验证截断持久。
        let bus2 = EventBus::new(dir.clone(), 64);
        assert_eq!(seqs(&bus2.snapshot("s")), vec![1, 2, 3, 4]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn purge_removes_memory_and_file() {
        let dir = temp_dir("purge");
        let bus = EventBus::new(dir.clone(), 64);
        bus.emit(draft("s", "e1"));
        assert!(dir.join("s.jsonl").exists());
        bus.purge_session("s");
        assert!(!dir.join("s.jsonl").exists());
        assert!(bus.snapshot("s").is_empty());
        assert_eq!(bus.latest_seq("s"), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
