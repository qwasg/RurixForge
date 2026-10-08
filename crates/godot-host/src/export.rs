//! 帧导出与交付(01 X2 / X3、02 §4.2)。全部在 [gmain]:frame_post_draw 里发起,结果经 FrameSink 交还宿主。
//! L2:Forward+ / Mobile 用 RD `texture_get_data_async`(回调在之后某帧的主线程上触发,C3);
//! Compatibility 没有 RD,同一回调里同步 `texture_2d_get`。结果统一规整成紧凑 RGBA8、A = 255,首行在上。
//! 导出失败只降级 / 丢这一帧,不停帧(X3)。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use godot::classes::image::Format;
use godot::classes::{RenderingDevice, RenderingServer};
use godot::prelude::*;

use engine_host::{normalize_rgba8, Channel, FrameOrigin, FrameOut, FramePixels, FrameSink, SrcFormat};

use crate::backend::Shared;

/// 本帧画了什么:交付 FrameOut 时的元数据(seq / 尺寸 / 统计)。
#[derive(Clone, Debug)]
pub struct FrameMeta {
    pub ch: Channel,
    pub seq: u64,
    pub scene_rev: u64,
    pub width: u32,
    pub height: u32,
    pub want_pixels: bool,
    pub draws: usize,
    pub triangles: usize,
    pub mesh_fallbacks: usize,
    /// 画这一帧的 RS::draw 序号(frame_post_draw 计数);L1 / L2 交付时据此算延迟(帧)。
    pub frame_no: u64,
    /// Mobile 下内容刚变(新材质 / 新网格):这一帧先不导出,再画一帧(ubershader → 专用管线切换,见 host.rs)。
    pub warm: bool,
}

/// 最近写进共享 buffer 的 seq(L1);L2 回调据此把同一帧标成 SharedGpuCopy(pixels.imported = true)。
#[derive(Clone, Default)]
pub struct L1Log(Rc<RefCell<VecDeque<u64>>>);

impl L1Log {
    pub fn push(&self, seq: u64) {
        let mut q = self.0.borrow_mut();
        if q.len() >= 64 {
            q.pop_front();
        }
        q.push_back(seq);
    }

    pub fn contains(&self, seq: u64) -> bool {
        self.0.borrow().contains(&seq)
    }
}

/// 交付一帧。rgba8 为空 = 没有 CPU 像素(L1 独占 / format=none)。
pub fn deliver(shared: &Shared, meta: &FrameMeta, rgba8: Vec<u8>, origin: FrameOrigin, device: &str) {
    let has_pixels = !rgba8.is_empty();
    let pixels = FramePixels {
        width: meta.width,
        height: meta.height,
        rgba8,
        device_name: device.to_string(),
        draws: meta.draws,
        truncated: false,
        nonzero: 0, // 消费线程按需统计(want_stats)
        triangles: meta.triangles,
        mesh_fallbacks: meta.mesh_fallbacks,
        mesh_classes: 0,
        imported: matches!(origin, FrameOrigin::SharedGpuCopy | FrameOrigin::SharedZeroCopy),
    };
    shared.sink.deliver(meta.ch, FrameOut { seq: meta.seq, scene_rev: meta.scene_rev, pixels, origin });
    if meta.ch == Channel::Main {
        shared.set_ready();
        shared.set_device_name(device);
    }
    if has_pixels {
        let lag = shared.frame_no().saturating_sub(meta.frame_no);
        shared.update_channels(|c| {
            c.l2_frames += 1;
            c.l2_lag = Some(lag);
        });
    }
}

fn src_format(len: usize, w: u32, h: u32) -> Result<SrcFormat, String> {
    let px = w as usize * h as usize;
    match len {
        n if n == px * 4 => Ok(SrcFormat::Rgba8),
        n if n == px * 3 => Ok(SrcFormat::Rgb8),
        n => Err(format!("回读字节数 {n} 既不是 {w}x{h} 的 RGBA8 也不是 RGB8")),
    }
}

/// L2(RD):发起异步回读。回调里规整 → 交付;origin 看 L1Log(同一帧已写进共享 buffer 则为 SharedGpuCopy)。
pub fn start_rd_readback(
    rd: &mut Gd<RenderingDevice>,
    rd_tex: Rid,
    meta: FrameMeta,
    shared: Arc<Shared>,
    l1log: L1Log,
    device: String,
) -> Result<(), String> {
    let cb = Callable::from_fn("forge_l2_readback", move |args: &[&Variant]| {
        let data: PackedByteArray = args.first().and_then(|v| v.try_to::<PackedByteArray>().ok()).unwrap_or_default();
        let src = data.as_slice();
        let r = src_format(src.len(), meta.width, meta.height)
            .and_then(|fmt| normalize_rgba8(src, meta.width, meta.height, fmt, 0, false));
        match r {
            Ok(px) => {
                let origin = if l1log.contains(meta.seq) { FrameOrigin::SharedGpuCopy } else { FrameOrigin::CpuReadback };
                deliver(&shared, &meta, px, origin, &device);
            }
            Err(e) => {
                // 降级:丢这一帧(等帧的请求靠 deadline 以 RENDER_TIMEOUT: 失败),不停帧(X3)。
                eprintln!("godot-host: L2 回读丢弃 seq={}:{e}", meta.seq);
            }
        }
    });
    let err = rd.texture_get_data_async(rd_tex, 0, &cb);
    if err != godot::global::Error::OK {
        return Err(format!("texture_get_data_async: {err:?}"));
    }
    Ok(())
}

/// L2(Compatibility / GLES3):没有 RD,同步 `texture_2d_get`(CPU 会等 GPU,01 §3.6)。
pub fn gles_readback(rs: &Gd<RenderingServer>, vp_tex: Rid, meta: &FrameMeta) -> Result<Vec<u8>, String> {
    let img = rs.texture_2d_get(vp_tex).ok_or("texture_2d_get 返回空")?;
    let (w, h) = (img.get_width() as u32, img.get_height() as u32);
    if (w, h) != (meta.width, meta.height) {
        return Err(format!("texture_2d_get 尺寸 {w}x{h} ≠ 视口 {}x{}", meta.width, meta.height));
    }
    let fmt = match img.get_format() {
        Format::RGB8 => SrcFormat::Rgb8,
        Format::RGBA8 => SrcFormat::Rgba8,
        f => return Err(format!("texture_2d_get 格式 {f:?} 不是 RGB8 / RGBA8")),
    };
    let data = img.get_data();
    normalize_rgba8(data.as_slice(), w, h, fmt, 0, false)
}
