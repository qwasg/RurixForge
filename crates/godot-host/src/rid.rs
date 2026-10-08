//! RS 资源的 RAII 句柄(01 §4.3:RID 用 RAII 管理)。drop 时 `free_rid`;只在 [gmain] 创建与释放
//! (所有持有者都挂在 host.rs 的 Runtime 上,Runtime::shutdown 在 SceneTree::finalize 里、RS 还活着时逐个 drop)。
//! 共享资源(网格、材质、贴图)放进 `Rc<Owned>`,最后一个持有者 drop 时释放。

use std::sync::atomic::{AtomicI64, Ordering};

use godot::classes::RenderingServer;
use godot::prelude::*;

/// 存活的 RAII 句柄数(g4_mesh 的泄漏测试经 asset.reload 后的日志行读它)。
static LIVE: AtomicI64 = AtomicI64::new(0);

pub fn live() -> i64 {
    LIVE.load(Ordering::SeqCst)
}

pub struct Owned(Rid);

impl Owned {
    pub fn new(rid: Rid) -> Owned {
        LIVE.fetch_add(1, Ordering::SeqCst);
        Owned(rid)
    }

    pub fn rid(&self) -> Rid {
        self.0
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::SeqCst);
        if self.0.is_valid() {
            RenderingServer::singleton().free_rid(self.0);
        }
    }
}

impl std::fmt::Debug for Owned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Owned({:?})", self.0)
    }
}
