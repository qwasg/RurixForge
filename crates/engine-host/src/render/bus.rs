//! 帧交接(02 §2.5、§4.2-§4.3):通道、等帧请求、输入侧交接箱 SubmitBox(`GB`)与输出侧帧总线 FrameBus(`FB`)。
//! Pipelined 后端(Godot)专用,rurix 不经过这里(I1)。两把锁都是叶子锁:持有时不取任何其他锁,
//! 等帧(FrameRequest 应答 / FrameBus::wait_newer)时调用方不持任何锁(I3、I4)。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::render::backend::ControlMsg;
use crate::render::sink::FrameOrigin;
use crate::render_core::list::RenderList;
use crate::viewport::FramePixels;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Main,
    /// template.preview 专用离屏视口(§4.3)。
    Preview,
}

/// 毒化容忍加锁(与 rpc::lock 同纪律)。
fn plock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// RPC 等帧:min_seq = 自己那份快照的 seq;Main 通道完成条件 frame.seq >= min_seq 且尺寸一致,
/// Preview 通道要求 seq == min_seq(§4.3)。
pub struct FrameRequest {
    pub min_seq: u64,
    pub size: (u32, u32),
    pub deadline: Instant,
    /// 容量 1。
    pub reply: SyncSender<Result<Arc<FrameOut>, String>>,
}

impl FrameRequest {
    /// 建请求与它的应答接收端(容量 1 的同步通道)。
    pub fn new(min_seq: u64, size: (u32, u32), deadline: Instant) -> (Self, Receiver<Result<Arc<FrameOut>, String>>) {
        let (reply, rx) = std::sync::mpsc::sync_channel(1);
        (FrameRequest { min_seq, size, deadline, reply }, rx)
    }

    /// exact = Preview 通道(seq 必须相等)。
    fn accepts(&self, f: &FrameOut, exact: bool) -> bool {
        let seq_ok = if exact { f.seq == self.min_seq } else { f.seq >= self.min_seq };
        seq_ok && (f.pixels.width, f.pixels.height) == self.size
    }
}

/// 输出侧帧总线(`FB`):[gmain] 经 FrameSink 写,[wsr] / 等帧的 [rpc] 读。
#[derive(Default)]
pub struct FrameBus {
    seq: AtomicU64,
    latest: Mutex<Option<Arc<FrameOut>>>,
    cv: Condvar,
}

impl FrameBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 快照序号:从 1 起单调递增(0 表示"还没有帧")。
    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// 发布一帧并唤醒等待者。I5:seq 小于当前 latest 的帧(乱序完成的回读)不替换 latest,返回 false。
    pub fn publish(&self, f: Arc<FrameOut>) -> bool {
        let mut g = plock(&self.latest);
        let newer = g.as_ref().is_none_or(|cur| f.seq >= cur.seq);
        if newer {
            *g = Some(f);
        }
        drop(g);
        self.cv.notify_all();
        newer
    }

    /// 当前最新帧(不等待)。
    pub fn latest(&self) -> Option<Arc<FrameOut>> {
        plock(&self.latest).clone()
    }

    /// 等 seq > after 的帧或超时([wsr] 用;Condvar 等待期间不持 FB)。
    pub fn wait_newer(&self, after: u64, timeout: Duration) -> Option<Arc<FrameOut>> {
        let g = plock(&self.latest);
        let (g, _) = self
            .cv
            .wait_timeout_while(g, timeout, |cur| cur.as_ref().is_none_or(|f| f.seq <= after))
            .unwrap_or_else(|e| e.into_inner());
        g.as_ref().filter(|f| f.seq > after).cloned()
    }
}

/// 一帧输出。pixels.rgba8 已规整为紧凑 RGBA8、首行在上(L2);L1 / NoPixels 时为空。
pub struct FrameOut {
    pub seq: u64,
    pub scene_rev: u64,
    pub pixels: FramePixels,
    pub origin: FrameOrigin,
}


/// 输入侧交接箱(`GB`):latest-wins 的 RenderList + 等帧请求 + 控制消息队列(§4.2 补的 control)。
/// [rpc] / [wsr] 投递,[gmain] 每次 process() 用 take() 取走。
#[derive(Default)]
pub struct SubmitBox {
    inner: Mutex<SubmitState>,
}

#[derive(Default)]
struct SubmitState {
    latest: Option<Arc<RenderList>>,
    waiters: Vec<FrameRequest>,
    control: VecDeque<ControlMsg>,
}

impl SubmitBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// 覆盖尚未被取走的旧清单(latest-wins);req 进 waiters,不会被后来的清单覆盖。
    pub fn put(&self, list: Arc<RenderList>, req: Option<FrameRequest>) {
        let mut g = plock(&self.inner);
        g.latest = Some(list);
        if let Some(r) = req {
            g.waiters.push(r);
        }
    }

    /// 投递控制消息(按到达顺序处理)。
    pub fn control(&self, msg: ControlMsg) {
        plock(&self.inner).control.push_back(msg);
    }

    /// [gmain]:取走全部控制消息与最新清单;控制消息先于清单处理(ControlMsg 的约定)。
    pub fn take(&self) -> (Vec<ControlMsg>, Option<Arc<RenderList>>) {
        let mut g = plock(&self.inner);
        (g.control.drain(..).collect(), g.latest.take())
    }

    /// 用这一帧完成所有接受它的 waiter;exact = Preview 通道。返回完成的个数。
    pub fn complete(&self, f: &Arc<FrameOut>, exact: bool) -> usize {
        let mut g = plock(&self.inner);
        let before = g.waiters.len();
        g.waiters.retain(|r| {
            if r.accepts(f, exact) {
                let _ = r.reply.try_send(Ok(Arc::clone(f)));
                false
            } else {
                true
            }
        });
        before - g.waiters.len()
    }

    /// 超过 deadline 的 waiter 以 `RENDER_TIMEOUT:` 失败;返回失败的个数。
    pub fn expire(&self, now: Instant) -> usize {
        let mut g = plock(&self.inner);
        let before = g.waiters.len();
        g.waiters.retain(|r| {
            if now >= r.deadline {
                let msg = format!("RENDER_TIMEOUT: 等帧超时(min_seq={} {}x{})", r.min_seq, r.size.0, r.size.1);
                let _ = r.reply.try_send(Err(msg));
                false
            } else {
                true
            }
        });
        before - g.waiters.len()
    }

    /// 全部 waiter 以给定错误失败(后端关停 / 通道失效);返回失败的个数。
    pub fn fail_all(&self, message: &str) -> usize {
        let mut g = plock(&self.inner);
        let n = g.waiters.len();
        for r in g.waiters.drain(..) {
            let _ = r.reply.try_send(Err(message.to_string()));
        }
        n
    }

    /// 仍在等帧的请求数。
    pub fn waiting(&self) -> usize {
        plock(&self.inner).waiters.len()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn frame(seq: u64, w: u32, h: u32) -> Arc<FrameOut> {
        let pixels = FramePixels {
            width: w, height: h, rgba8: vec![0; (w * h * 4) as usize], device_name: String::new(), draws: 0,
            truncated: false, nonzero: 0, triangles: 0, mesh_fallbacks: 0, mesh_classes: 0, imported: false,
        };
        Arc::new(FrameOut { seq, scene_rev: 0, pixels, origin: FrameOrigin::CpuReadback })
    }

    fn list(seq: u64) -> Arc<RenderList> {
        let st = crate::rpc::HostState::new();
        let snap = crate::render::snapshot(&st, crate::render::SnapshotParams { scene_camera: false,
            width: 4, height: 2, selected: None, want_readback: true, want_stats: false,
            requester: crate::render::FrameRequester::Stream,
        }, seq);
        crate::render_core::list::extract_arc(&snap).unwrap()
    }

    /// I5:乱序完成的回读不得让 latest 回退;wait_newer 只返回 seq > after 的帧。
    #[test]
    fn frame_bus_is_monotonic_and_wait_newer_filters() {
        let bus = FrameBus::new();
        assert_eq!((bus.next_seq(), bus.next_seq()), (1, 2));
        assert!(bus.wait_newer(0, Duration::from_millis(5)).is_none(), "空总线超时");
        assert!(bus.publish(frame(5, 4, 2)));
        assert!(!bus.publish(frame(3, 4, 2)), "旧帧不替换");
        assert_eq!(bus.latest().unwrap().seq, 5);
        assert_eq!(bus.wait_newer(4, Duration::from_millis(5)).unwrap().seq, 5);
        assert!(bus.wait_newer(5, Duration::from_millis(5)).is_none());
        let bus = Arc::new(bus);
        let b2 = Arc::clone(&bus);
        let t = std::thread::spawn(move || b2.wait_newer(5, Duration::from_secs(5)).map(|f| f.seq));
        std::thread::sleep(Duration::from_millis(20));
        bus.publish(frame(6, 4, 2));
        assert_eq!(t.join().unwrap(), Some(6), "Condvar 唤醒");
    }

    #[test]
    fn submit_box_latest_wins_and_completes_matching_waiters() {
        let b = SubmitBox::new();
        let deadline = Instant::now() + Duration::from_secs(60);
        let (r1, rx1) = FrameRequest::new(2, (4, 2), deadline);
        let (r2, rx2) = FrameRequest::new(3, (8, 8), deadline);
        b.put(list(1), Some(r1));
        b.put(list(2), Some(r2));
        b.control(ControlMsg::ShareDetach);
        let (ctrl, latest) = b.take();
        assert_eq!((ctrl.len(), latest.unwrap().seq), (1, 2), "控制消息 + 只剩最新清单");
        assert!(b.take().1.is_none(), "取走即空");
        assert_eq!(b.complete(&frame(1, 4, 2), false), 0, "seq 太旧");
        assert_eq!(b.complete(&frame(4, 4, 2), true), 0, "Preview 要求 seq 相等");
        assert_eq!(b.complete(&frame(4, 4, 2), false), 1, "Main:seq ≥ min_seq 且尺寸一致");
        assert_eq!(rx1.try_recv().unwrap().unwrap().seq, 4);
        assert_eq!(b.waiting(), 1, "尺寸不符的请求仍在等");
        assert_eq!(b.expire(Instant::now()), 0);
        assert_eq!(b.expire(deadline), 1);
        let e = rx2.try_recv().unwrap().err().unwrap();
        assert!(e.starts_with("RENDER_TIMEOUT:"), "{e}");
        let (r3, rx3) = FrameRequest::new(9, (1, 1), deadline);
        b.put(list(3), Some(r3));
        assert_eq!(b.fail_all("RENDER_NOT_READY: x"), 1);
        assert!(rx3.try_recv().unwrap().is_err());
    }

    #[test]
    fn host_frame_sink_publishes_and_completes_per_channel() {
        use crate::render::sink::{FrameSink, HostFrameSink};
        let s = HostFrameSink::new();
        let deadline = Instant::now() + Duration::from_secs(60);
        let (req, rx) = FrameRequest::new(7, (4, 2), deadline);
        s.preview_box.put(list(7), Some(req));
        let f = |seq| FrameOut { seq, scene_rev: 1, pixels: Arc::try_unwrap(frame(seq, 4, 2)).ok().unwrap().pixels, origin: FrameOrigin::CpuReadback };
        s.deliver(Channel::Preview, f(8));
        assert!(rx.try_recv().is_err(), "Preview 通道 seq 8 ≠ 7,不完成");
        s.deliver(Channel::Preview, f(7));
        assert_eq!(rx.try_recv().unwrap().unwrap().seq, 7);
        assert!(s.main.latest().is_none(), "两条通道互不影响");
        assert_eq!(s.preview.latest().unwrap().seq, 8, "latest 不回退");
    }
}
