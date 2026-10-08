//! 帧出口(02 §4.2)。FrameSink 是 Godot 后端 [gmain] 把帧交还宿主的唯一入口;rurix 同步腿不经过它(I1),
//! 调用方继续直接调 rpc::feed_share_frame。

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use crate::render::bus::{Channel, FrameBus, FrameOut, SubmitBox};
use crate::viewport::FramePixels;

/// 帧像素的来源。与 FramePixels.imported 的关系是不变量:
/// imported == matches!(origin, SharedZeroCopy | SharedGpuCopy)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOrigin {
    /// 像素在 pixels.rgba8(rurix 回读档 / Godot L2)。
    CpuReadback,
    /// rurix import 档:已直渲进共享 buffer,只需推 fence。
    SharedZeroCopy,
    /// Godot L1:[gmain] 已在 GPU 上把视口拷进共享 buffer 并推进了共享 fence。
    SharedGpuCopy,
    /// want_readback = false(viewport.frame format=none),且没有走 L1。
    NoPixels,
}

impl FrameOrigin {
    /// 逐字节保持现状:imported → SharedZeroCopy,否则 CpuReadback;
    /// V6 的 imported 误报(§1.5)也原样映射(§9.4:单独审批,本阶段不修)。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn from_rurix(f: &FramePixels) -> FrameOrigin {
        if f.imported {
            FrameOrigin::SharedZeroCopy
        } else {
            FrameOrigin::CpuReadback
        }
    }
}

/// L1 目标(01 X1 的 a2 变体):共享 buffer 与共享 fence 由 share::open 在 Godot 的 ID3D12Device 上创建,
/// 这里只把本进程内的 COM 指针交给 [gmain]。
#[derive(Debug, Clone, Copy)]
pub struct SharedTarget {
    /// ID3D12Resource*(已 AddRef;[gmain] 处理 ShareDetach / 新的 ShareAttach 时 Release)。
    pub buffer: usize,
    /// ID3D12Fence*(同上);L1 期间只由 [gmain] 在 Godot 主队列上 Signal。
    pub fence: usize,
    pub width: u32,
    pub height: u32,
    /// shared_layout:ceil(w*4/256)*256。
    pub row_pitch: u32,
    /// row_pitch * height。
    pub size: u64,
    /// 共享 fence 的值计数器(share 模块所有;Producer 与 [gmain] 共用同一序列,保证 fence 值单调、每帧 +1)。
    /// [gmain] 取值:`fetch_add(1) + 1`。
    pub fence_value: &'static AtomicU64,
}

pub trait FrameSink: Send + Sync {
    /// [gmain] 在 L2 回读回调 / L1 命令提交之后调用:只 publish 到 FrameBus 并完成 waiters,
    /// 不取 HS / SH(I3)。L1 的 GPU 拷贝与 Signal 由 [gmain] 在调用前自己录制提交。
    fn deliver(&self, ch: Channel, frame: FrameOut);
}

/// 宿主实现:两条通道各一个 FrameBus + SubmitBox。
#[derive(Default)]
pub struct HostFrameSink {
    pub main: Arc<FrameBus>,
    pub preview: Arc<FrameBus>,
    pub main_box: Arc<SubmitBox>,
    pub preview_box: Arc<SubmitBox>,
}

impl HostFrameSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bus(&self, ch: Channel) -> &FrameBus {
        match ch {
            Channel::Main => &self.main,
            Channel::Preview => &self.preview,
        }
    }

    pub fn submit_box(&self, ch: Channel) -> &SubmitBox {
        match ch {
            Channel::Main => &self.main_box,
            Channel::Preview => &self.preview_box,
        }
    }
}

impl FrameSink for HostFrameSink {
    fn deliver(&self, ch: Channel, frame: FrameOut) {
        let f = Arc::new(frame);
        self.bus(ch).publish(Arc::clone(&f));
        // Preview 通道只认 seq == min_seq(§4.3:每次请求是不同场景,不能被别的清单的帧完成)。
        self.submit_box(ch).complete(&f, ch == Channel::Preview);
    }
}

/// 共享 buffer 喂帧,由消费线程([wsr] / 等帧的 [rpc])调用,不在 [gmain]。
/// SharedGpuCopy → ("zero_copy", false),不再推 fence([gmain] 已推进,再推一次会让 presenter 的帧计数跳号);
/// 其余 origin = 现有函数 feed_share_frame(&out.pixels),两后端的 framePath / cpuUploads 语义相同。
pub fn feed_share(out: &FrameOut) -> Result<(&'static str, bool), String> {
    match out.origin {
        FrameOrigin::SharedGpuCopy => Ok(("zero_copy", false)),
        _ => crate::rpc::feed_share_frame(&out.pixels),
    }
}


/// L2 源像素格式。RD 视口纹理是 R8G8B8A8_UNORM、存 sRGB 编码值(01 §3.1);Rgb8 兜底 Image 路径(00 §2 第 4 条)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrcFormat {
    Rgb8,
    Rgba8,
}

/// L2 规整(01 X2):输出紧凑 RGBA8、A 强制 255;src_pitch 是源行距(可含行尾填充,0 = 紧凑)。
/// 行序:Image / RD 回读顶行在前,L2 传 flip_y = false(01 §2.3 C1)。
/// 与 02 §4.2 的差异:源字节不足时返回 Err(不 panic、不截断),由调用方降级。
pub fn normalize_rgba8(src: &[u8], w: u32, h: u32, fmt: SrcFormat, src_pitch: usize, flip_y: bool) -> Result<Vec<u8>, String> {
    let (w, h) = (w as usize, h as usize);
    let bpp = match fmt {
        SrcFormat::Rgb8 => 3,
        SrcFormat::Rgba8 => 4,
    };
    let pitch = if src_pitch == 0 { w * bpp } else { src_pitch };
    if pitch < w * bpp {
        return Err(format!("normalize_rgba8: 行距 {pitch} < 行宽 {}", w * bpp));
    }
    let need = if h == 0 { 0 } else { pitch * (h - 1) + w * bpp };
    if src.len() < need {
        return Err(format!("normalize_rgba8: 源 {}B < 需要 {need}B({w}x{h} {fmt:?} pitch {pitch})", src.len()));
    }
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let sy = if flip_y { h - 1 - y } else { y };
        let row = &src[sy * pitch..sy * pitch + w * bpp];
        match fmt {
            SrcFormat::Rgba8 => {
                for p in row.chunks_exact(4) {
                    out.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
            }
            SrcFormat::Rgb8 => {
                for p in row.chunks_exact(3) {
                    out.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_forces_alpha_and_drops_row_padding() {
        // 2x2 RGBA8,行距 12(每行 4B 填充),alpha 各异。
        let src = [1, 2, 3, 0, 4, 5, 6, 7, 9, 9, 9, 9, 8, 7, 6, 5, 4, 3, 2, 1, 9, 9, 9, 9];
        let out = normalize_rgba8(&src, 2, 2, SrcFormat::Rgba8, 12, false).unwrap();
        assert_eq!(out, [1, 2, 3, 255, 4, 5, 6, 255, 8, 7, 6, 255, 4, 3, 2, 255]);
        let flipped = normalize_rgba8(&src, 2, 2, SrcFormat::Rgba8, 12, true).unwrap();
        assert_eq!(flipped, [8, 7, 6, 255, 4, 3, 2, 255, 1, 2, 3, 255, 4, 5, 6, 255], "flip_y 只换行序");
        let rgb = normalize_rgba8(&[10, 20, 30, 40, 50, 60], 2, 1, SrcFormat::Rgb8, 0, false).unwrap();
        assert_eq!(rgb, [10, 20, 30, 255, 40, 50, 60, 255]);
    }

    #[test]
    fn normalize_rejects_short_input() {
        assert!(normalize_rgba8(&[0; 15], 2, 2, SrcFormat::Rgba8, 0, false).is_err());
        assert!(normalize_rgba8(&[0; 16], 2, 2, SrcFormat::Rgba8, 4, false).is_err(), "行距小于行宽");
        assert_eq!(normalize_rgba8(&[], 0, 0, SrcFormat::Rgb8, 0, false).unwrap(), Vec::<u8>::new());
    }
}
