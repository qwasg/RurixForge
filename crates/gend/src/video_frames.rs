//! video_frames — 视频截帧 → 精灵图集(「参考图 → 图生视频 → 角色帧动画」的中段)。
//!
//! 上游 media::RemoteVideo 出的是 mp4,而引擎的角色动画事实源是 .rxsprite
//! (单张图集 + 逐帧 bbox,08 E-08-002),中间缺的正是这一段:抽帧 → 抠底 → 拼图集。
//!
//! 解码不自造:mp4/h264 解码器在本仓无落点(engine-host 的 openh264 是编码腿,
//! 只把视口帧推给网页),故截帧走外部 ffmpeg 可执行文件。ffmpeg 找不到 →
//! GEN_TOOL_MISSING + 配置指引,绝不退化成「随便给几帧」(I-5:诚实缺省不伪造产物)。
//!
//! 除 extract_frames 一处进程调用外全是纯函数(色键/裁切/拼图),故无 ffmpeg 的
//! 机器也能把这段逻辑测全。

use std::path::{Path, PathBuf};

use assetd::project::ForgeProject;
use serde::Serialize;
use serde_json::json;

use crate::{GenError, Result, GEN_BACKEND_ERROR, GEN_BAD_PARAMS, GEN_TOOL_MISSING};

/// 截帧率上下限(低于 1 出不了动画,高于 30 只是把同一帧抄多份)。
pub const FPS_MIN: f32 = 1.0;
pub const FPS_MAX: f32 = 30.0;
/// 单次截帧上限(与 assetd SliceOptions::max_frames 同档)。
pub const MAX_FRAMES_LIMIT: usize = 256;
/// auto 色键的逐通道容差(视频编码有色度抖动,纯色底出锅时并不真的纯)。
pub const AUTO_KEY_TOLERANCE: u8 = 40;

// ---------- ffmpeg 发现 ----------

/// ffmpeg 可用性(诚实上报:找不到就是找不到,前端据此显示配置指引)。
#[derive(Debug, Clone, Serialize)]
pub struct FfmpegStatus {
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `ffmpeg -version` 首行(找不到则 None)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

fn ffmpeg_exe_name() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// `<path> -version` 首行(跑不起来 → None,即视为不可用)。
fn probe_version(exe: &Path) -> Option<String> {
    let out = std::process::Command::new(exe).arg("-version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(text.lines().next().unwrap_or("").trim().to_string())
}

/// ffmpeg 可执行文件定位。顺序:env `FORGE_FFMPEG`(文件或所在目录)>
/// `<workspace>/data/tools/ffmpeg[.exe]` > PATH。
/// env 指了却指不到文件 = 用户配错,如实返回 None 而不静默回落 PATH。
pub fn ffmpeg_path() -> Option<PathBuf> {
    let exe = ffmpeg_exe_name();
    if let Ok(v) = std::env::var("FORGE_FFMPEG") {
        let v = v.trim();
        if !v.is_empty() {
            let p = PathBuf::from(v);
            let cand = if p.is_dir() { p.join(exe) } else { p };
            return cand.is_file().then_some(cand);
        }
    }
    let bundled = crate::config::workspace_root().join("data").join("tools").join(exe);
    if bundled.is_file() {
        return Some(bundled);
    }
    let bare = PathBuf::from(exe);
    probe_version(&bare).is_some().then_some(bare)
}

pub fn ffmpeg_status() -> FfmpegStatus {
    match ffmpeg_path() {
        Some(p) => FfmpegStatus {
            found: true,
            path: Some(p.display().to_string()),
            version: probe_version(&p),
        },
        None => FfmpegStatus { found: false, path: None, version: None },
    }
}

fn require_ffmpeg() -> Result<PathBuf> {
    ffmpeg_path().ok_or_else(|| {
        GenError::new(
            GEN_TOOL_MISSING,
            "未找到 ffmpeg(视频截帧必需)。装好后置于 PATH,或放到 <workspace>/data/tools/,\
             或用环境变量 FORGE_FFMPEG 指向可执行文件",
        )
    })
}

// ---------- 参数 ----------

/// 背景抠除模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromaKey {
    /// 四角采样取底色,逐通道容差 AUTO_KEY_TOLERANCE,边缘线性软化。
    Auto,
    /// 与视口色键/assetd 自动切帧同一规则(alpha 低或品红族 2g<min(r,b))。
    Magenta,
    /// 黑底发光特效:最大颜色通道转透明度,反预乘 RGB 保留亮度与彩色辉光。
    Black,
    /// 不抠(整帧不透明进图集)。
    None,
}

impl ChromaKey {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(ChromaKey::Auto),
            "magenta" => Some(ChromaKey::Magenta),
            "black" => Some(ChromaKey::Black),
            "none" => Some(ChromaKey::None),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ChromaKey::Auto => "auto",
            ChromaKey::Magenta => "magenta",
            ChromaKey::Black => "black",
            ChromaKey::None => "none",
        }
    }
}

/// 帧裁切模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropMode {
    /// 全帧不透明包围盒的并集 = 所有帧共用一个矩形。帧等大,脚底锚不抖(D-031 的
    /// 「AI 帧尺寸不一导致漂浮」正是逐帧紧致裁切的副作用)。
    Union,
    /// 逐帧紧致包围盒(帧尺寸不一,靠 pivot 级联兜)。
    Tight,
    /// 不裁,原始帧尺寸。
    None,
}

impl CropMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "union" => Some(CropMode::Union),
            "tight" => Some(CropMode::Tight),
            "none" => Some(CropMode::None),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CropMode::Union => "union",
            CropMode::Tight => "tight",
            CropMode::None => "none",
        }
    }
}

/// 截帧 + 拼图集参数。
#[derive(Debug, Clone)]
pub struct FrameOptions {
    pub fps: f32,
    pub max_frames: usize,
    pub trim_start_sec: Option<f32>,
    pub trim_end_sec: Option<f32>,
    pub chroma_key: ChromaKey,
    pub crop: CropMode,
    /// 格间透明留白(防连通域自动切帧把相邻帧粘成一块)。
    pub padding: u32,
}

impl Default for FrameOptions {
    fn default() -> Self {
        FrameOptions {
            fps: 8.0,
            max_frames: 32,
            trim_start_sec: None,
            trim_end_sec: None,
            chroma_key: ChromaKey::Auto,
            crop: CropMode::Union,
            padding: 2,
        }
    }
}

impl FrameOptions {
    fn validate(&self) -> Result<()> {
        if !(FPS_MIN..=FPS_MAX).contains(&self.fps) || !self.fps.is_finite() {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("fps 须 {FPS_MIN}..={FPS_MAX},实: {}", self.fps),
            ));
        }
        if self.max_frames < 2 || self.max_frames > MAX_FRAMES_LIMIT {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("maxFrames 须 2..={MAX_FRAMES_LIMIT},实: {}", self.max_frames),
            ));
        }
        if self.padding > 32 {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("padding 上限 32,实: {}", self.padding),
            ));
        }
        for (name, v) in
            [("trimStartSec", self.trim_start_sec), ("trimEndSec", self.trim_end_sec)]
        {
            if let Some(v) = v {
                if !v.is_finite() || v < 0.0 {
                    return Err(GenError::new(
                        GEN_BAD_PARAMS,
                        format!("{name} 须为非负秒数,实: {v}"),
                    ));
                }
            }
        }
        Ok(())
    }
}

// ---------- 帧 ----------

/// RGBA8 帧(裸缓冲;pack/色键全在其上做,不牵 image crate 类型进签名)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        let need = (width as usize) * (height as usize) * 4;
        if rgba.len() != need {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("帧缓冲字节数不符: {} ≠ {width}x{height}x4", rgba.len()),
            ));
        }
        Ok(Frame { width, height, rgba })
    }

    fn px(&self, x: u32, y: u32) -> &[u8] {
        let i = ((y as usize) * (self.width as usize) + x as usize) * 4;
        &self.rgba[i..i + 4]
    }
}

// ---------- 截帧(唯一进程调用) ----------

/// ffmpeg 抽帧:`-vf fps=N` 均匀取帧,`-frames:v` 上限截断(免得十秒片子写出几百张 PNG)。
/// 落到进程私有临时目录,读回后即删。
pub fn extract_frames(video_abs: &Path, opts: &FrameOptions) -> Result<Vec<Frame>> {
    opts.validate()?;
    extract_frames_with(&require_ffmpeg()?, video_abs, opts)
}

fn extract_frames_with(ffmpeg: &Path, video_abs: &Path, opts: &FrameOptions) -> Result<Vec<Frame>> {
    let dir = std::env::temp_dir().join(format!(
        "gend-frames-{}-{}",
        std::process::id(),
        forge_util::timeutil::unix_millis()
    ));
    std::fs::create_dir_all(&dir)?;
    let result = run_ffmpeg_into(ffmpeg, video_abs, &dir, opts).and_then(|()| read_frames(&dir));
    std::fs::remove_dir_all(&dir).ok();
    result
}

fn run_ffmpeg_into(
    ffmpeg: &Path,
    video_abs: &Path,
    out_dir: &Path,
    opts: &FrameOptions,
) -> Result<()> {
    let mut cmd = std::process::Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
    // -ss 放 -i 之前 = 关键帧快进(截头几秒废帧时省解码)。
    if let Some(s) = opts.trim_start_sec.filter(|v| *v > 0.0) {
        cmd.arg("-ss").arg(format!("{s}"));
    }
    cmd.arg("-i").arg(video_abs);
    if let Some(e) = opts.trim_end_sec.filter(|v| *v > 0.0) {
        let start = opts.trim_start_sec.unwrap_or(0.0);
        if e <= start {
            return Err(GenError::new(
                GEN_BAD_PARAMS,
                format!("trimEndSec({e}) 须大于 trimStartSec({start})"),
            ));
        }
        cmd.arg("-t").arg(format!("{}", e - start));
    }
    // 节奏完全交给 fps 滤镜:它输出的已是等间隔 CFR,所以这里不再加 -fps_mode/-vsync
    // ——前者 ffmpeg 5.1 才有(4.x 会直接报 unrecognized option 而不是忽略),后者正在被
    // 弃用,而两者在此都只是重复 fps 滤镜已经做完的事,不值得为它赌用户装的是哪个大版本。
    cmd.arg("-vf").arg(format!("fps={}", opts.fps));
    cmd.arg("-frames:v").arg(opts.max_frames.to_string());
    cmd.arg(out_dir.join("f_%04d.png"));
    let out = cmd
        .output()
        .map_err(|e| GenError::new(GEN_TOOL_MISSING, format!("ffmpeg 启动失败: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err.lines().rev().take(4).collect::<Vec<_>>().join(" / ");
        return Err(GenError::new(
            GEN_BACKEND_ERROR,
            format!("ffmpeg 截帧失败({}): {tail}", out.status),
        ));
    }
    Ok(())
}

fn read_frames(dir: &Path) -> Result<Vec<Frame>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("png"))
        .collect();
    // f_%04d.png 定宽序号 → 文件名字典序即帧序。
    files.sort();
    if files.is_empty() {
        return Err(GenError::new(
            GEN_BACKEND_ERROR,
            "ffmpeg 未产出任何帧(视频可能为空或时长小于截帧区间)",
        ));
    }
    let mut out = Vec::with_capacity(files.len());
    for f in &files {
        let img = image::open(f)
            .map_err(|e| GenError::new(GEN_BACKEND_ERROR, format!("帧解码失败({}): {e}", f.display())))?
            .to_rgba8();
        let (w, h) = img.dimensions();
        out.push(Frame::new(w, h, img.into_raw())?);
    }
    Ok(out)
}

// ---------- 色键抠底 ----------

/// 四角采样求底色(逐通道取中位数)。四角同时被角色占满的画面本就不适合抠底,
/// 那种情况下 union 裁切会退化成全帧 —— 如实产出,不猜。
fn sample_key_color(frame: &Frame) -> [u8; 3] {
    let block = (frame.width.min(frame.height) / 8).clamp(1, 8);
    let mut r = Vec::new();
    let mut g = Vec::new();
    let mut b = Vec::new();
    let corners = [(0u32, 0u32), (frame.width - block, 0), (0, frame.height - block), (frame.width - block, frame.height - block)];
    for (cx, cy) in corners {
        for y in cy..(cy + block).min(frame.height) {
            for x in cx..(cx + block).min(frame.width) {
                let p = frame.px(x, y);
                r.push(p[0]);
                g.push(p[1]);
                b.push(p[2]);
            }
        }
    }
    let median = |mut v: Vec<u8>| -> u8 {
        v.sort_unstable();
        v[v.len() / 2]
    };
    [median(r), median(g), median(b)]
}

/// 品红/透明背景判定(与 assetd::sprite 的自动切帧、视口 shader 色键同一规则)。
fn is_magenta_bg(px: &[u8]) -> bool {
    let (r, g, b, a) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3]);
    a < 5 || 2 * g < r.min(b)
}

/// 就地抠底。auto 用首帧四角采样的底色(整段视频共用一个键色 —— 逐帧各采一次会
/// 让底色随编码抖动漂移,抠出的轮廓跟着一帧胖一帧瘦)。
pub fn apply_chroma_key(frames: &mut [Frame], mode: ChromaKey) {
    match mode {
        ChromaKey::None => {}
        ChromaKey::Magenta => {
            for f in frames.iter_mut() {
                for px in f.rgba.chunks_exact_mut(4) {
                    if is_magenta_bg(px) {
                        px[3] = 0;
                    }
                }
            }
        }
        ChromaKey::Black => {
            for frame in frames.iter_mut() {
                for px in frame.rgba.chunks_exact_mut(4) {
                    let peak = px[0].max(px[1]).max(px[2]) as u32;
                    if peak <= 3 || px[3] == 0 {
                        // Near-black codec noise is not an opaque rectangular backdrop.
                        px.fill(0);
                    } else {
                        let alpha = (px[3] as u32 * peak + 127) / 255;
                        for color in &mut px[..3] {
                            *color = ((*color as u32 * 255 + peak / 2) / peak).min(255) as u8;
                        }
                        px[3] = alpha as u8;
                    }
                }
            }
        }
        ChromaKey::Auto => {
            let Some(key) = frames.first().map(sample_key_color) else { return };
            let tol = AUTO_KEY_TOLERANCE as i32;
            for f in frames.iter_mut() {
                for px in f.rgba.chunks_exact_mut(4) {
                    let d = (0..3)
                        .map(|i| (px[i] as i32 - key[i] as i32).abs())
                        .max()
                        .unwrap_or(0);
                    if d <= tol {
                        px[3] = 0;
                    } else if d < tol * 2 {
                        // 软化带:半透明过渡,免得抠完一圈锯齿硬边。
                        let a = ((d - tol) * 255 / tol).clamp(0, 255) as u8;
                        px[3] = px[3].min(a);
                    }
                }
            }
        }
    }
}

/// 不透明像素包围盒 [x, y, w, h];整帧透明 → None。
pub fn alpha_bbox(frame: &Frame) -> Option<[u32; 4]> {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..frame.height {
        for x in 0..frame.width {
            if frame.px(x, y)[3] >= 5 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    (min_x != u32::MAX).then(|| [min_x, min_y, max_x - min_x + 1, max_y - min_y + 1])
}

// ---------- 拼图集 ----------

/// 图集(单张 RGBA + 逐帧 bbox,直接对得上 .rxsprite 的 texture + frames)。
#[derive(Debug, Clone)]
pub struct Atlas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// 与输入帧同序的 [x, y, w, h]。
    pub boxes: Vec<[u32; 4]>,
}

/// 帧序列 → 单张图集。网格近正方(cols = ceil(sqrt(n))),格间 padding 全透明。
pub fn pack_atlas(frames: &[Frame], crop: CropMode, padding: u32) -> Result<Atlas> {
    if frames.is_empty() {
        return Err(GenError::new(GEN_BAD_PARAMS, "帧序列为空,无法拼图集"));
    }
    // 逐帧取内容矩形。
    let rects: Vec<[u32; 4]> = match crop {
        CropMode::None => frames.iter().map(|f| [0, 0, f.width, f.height]).collect(),
        CropMode::Tight => {
            let mut out = Vec::with_capacity(frames.len());
            for (i, f) in frames.iter().enumerate() {
                out.push(alpha_bbox(f).ok_or_else(|| empty_frame_err(i))?);
            }
            out
        }
        CropMode::Union => {
            let mut acc: Option<[u32; 4]> = None;
            for (i, f) in frames.iter().enumerate() {
                let b = alpha_bbox(f).ok_or_else(|| empty_frame_err(i))?;
                acc = Some(match acc {
                    None => b,
                    Some(a) => {
                        let x0 = a[0].min(b[0]);
                        let y0 = a[1].min(b[1]);
                        let x1 = (a[0] + a[2]).max(b[0] + b[2]);
                        let y1 = (a[1] + a[3]).max(b[1] + b[3]);
                        [x0, y0, x1 - x0, y1 - y0]
                    }
                });
            }
            let u = acc.expect("frames 非空则并集必有值");
            frames.iter().map(|_| u).collect()
        }
    };

    let cell_w = rects.iter().map(|r| r[2]).max().unwrap_or(1).max(1);
    let cell_h = rects.iter().map(|r| r[3]).max().unwrap_or(1).max(1);
    let n = frames.len();
    let cols = (n as f64).sqrt().ceil() as u32;
    let cols = cols.max(1);
    let rows = ((n as u32) + cols - 1) / cols;
    let width = cols * cell_w + (cols + 1) * padding;
    let height = rows * cell_h + (rows + 1) * padding;
    let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
    let mut boxes = Vec::with_capacity(n);

    for (i, (frame, rect)) in frames.iter().zip(&rects).enumerate() {
        let col = (i as u32) % cols;
        let row = (i as u32) / cols;
        let dx = padding + col * (cell_w + padding);
        let dy = padding + row * (cell_h + padding);
        // 源矩形按帧尺寸夹紧(union 的并集矩形对个别帧可能越界)。
        let sx = rect[0].min(frame.width.saturating_sub(1));
        let sy = rect[1].min(frame.height.saturating_sub(1));
        let sw = rect[2].min(frame.width - sx);
        let sh = rect[3].min(frame.height - sy);
        for y in 0..sh {
            let src = (((sy + y) as usize) * (frame.width as usize) + sx as usize) * 4;
            let dst = (((dy + y) as usize) * (width as usize) + dx as usize) * 4;
            let len = (sw as usize) * 4;
            rgba[dst..dst + len].copy_from_slice(&frame.rgba[src..src + len]);
        }
        boxes.push([dx, dy, rect[2], rect[3]]);
    }

    Ok(Atlas { width, height, rgba, boxes })
}

fn empty_frame_err(idx: usize) -> GenError {
    GenError::new(
        GEN_BAD_PARAMS,
        format!(
            "第 {idx} 帧抠底后整帧透明:色键把角色一起抠掉了。\
             换 chromaKey=none 看原帧,或让视频用纯色背景重生成"
        ),
    )
}

// ---------- 总入口 ----------

/// 截帧管线产出(fileRef 落 .forge/tmp/gen/,待 gen_accept 入 Content/)。
#[derive(Debug, Clone)]
pub struct AtlasOutcome {
    pub atlas_ref: String,
    pub width: u32,
    pub height: u32,
    pub boxes: Vec<[u32; 4]>,
    /// 实际截帧率(= 请求值;clip 时长据此换算)。
    pub fps: f32,
    pub frame_count: usize,
    /// 图集 PNG 字节(调用方按需转 dataUrl 内联给前端)。
    pub atlas_png: Vec<u8>,
}

/// mp4(.forge/tmp/gen/ 内)→ 图集 PNG + 逐帧 bbox,产物落同目录带 sidecar。
pub fn video_to_atlas(
    project: &ForgeProject,
    video_file_ref: &str,
    opts: &FrameOptions,
) -> Result<AtlasOutcome> {
    opts.validate()?;
    // ffmpeg 先于 fileRef 校验:本机没这把工具时,「产物在哪」根本不是用户要解的问题。
    let ffmpeg = require_ffmpeg()?;
    let abs = crate::tmpstore::resolve_video_ref(project, video_file_ref)?;
    let mut frames = extract_frames_with(&ffmpeg, &abs, opts)?;
    apply_chroma_key(&mut frames, opts.chroma_key);
    let atlas = pack_atlas(&frames, opts.crop, opts.padding)?;
    let png = crate::mock::encode_png_rgba8(&atlas.rgba, atlas.width, atlas.height)?;
    let sidecar = json!({
        "kind": "video-frames",
        "sourceVideo": video_file_ref,
        "fps": opts.fps,
        "frameCount": frames.len(),
        "chromaKey": opts.chroma_key.as_str(),
        "crop": opts.crop.as_str(),
        "generatedAt": forge_util::timeutil::utc_now_iso8601(),
    });
    let seed = crate::fnv1a64(video_file_ref.as_bytes());
    let atlas_ref = crate::tmpstore::save_artifact(project, &png, "png", seed, 0, &sidecar)?;
    Ok(AtlasOutcome {
        atlas_ref,
        width: atlas.width,
        height: atlas.height,
        boxes: atlas.boxes,
        fps: opts.fps,
        frame_count: frames.len(),
        atlas_png: png,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TEST_ENV_LOCK as ENV_LOCK;

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 纯色底 + 中间一块实心方块的合成帧(方块位置可挪,模拟角色位移)。
    fn synth(w: u32, h: u32, bg: [u8; 4], obj: [u8; 4], rect: [u32; 4]) -> Frame {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let inside = x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3];
                rgba.extend_from_slice(if inside { &obj } else { &bg });
            }
        }
        Frame::new(w, h, rgba).unwrap()
    }

    #[test]
    fn ffmpeg_status_honest_when_env_points_nowhere() {
        let _g = env_lock();
        let prev = std::env::var("FORGE_FFMPEG").ok();
        std::env::set_var("FORGE_FFMPEG", "/definitely/not/here/ffmpeg-nope");
        let st = ffmpeg_status();
        assert!(!st.found, "env 指到不存在的路径须如实 found=false");
        assert!(st.path.is_none() && st.version.is_none());
        // 截帧调用侧同步给出 GEN_TOOL_MISSING 而非空帧。
        let err = extract_frames(Path::new("nope.mp4"), &FrameOptions::default()).unwrap_err();
        assert_eq!(err.code, GEN_TOOL_MISSING);
        assert!(err.message.contains("FORGE_FFMPEG"), "错误须带配置指引: {}", err.message);
        match prev {
            Some(v) => std::env::set_var("FORGE_FFMPEG", v),
            None => std::env::remove_var("FORGE_FFMPEG"),
        }
    }

    #[test]
    fn auto_key_removes_sampled_background() {
        // 底色带轻微抖动(容差内),角色纯绿。
        let mut frames = vec![
            synth(16, 16, [30, 90, 200, 255], [20, 200, 20, 255], [4, 4, 8, 8]),
            synth(16, 16, [45, 105, 215, 255], [20, 200, 20, 255], [5, 4, 8, 8]),
        ];
        apply_chroma_key(&mut frames, ChromaKey::Auto);
        assert_eq!(frames[0].px(0, 0)[3], 0, "四角底色须抠成全透明");
        assert_eq!(frames[1].px(0, 0)[3], 0, "抖动后的底色仍在容差内");
        assert_eq!(frames[0].px(8, 8)[3], 255, "角色像素须保留");
        assert_eq!(alpha_bbox(&frames[0]), Some([4, 4, 8, 8]));
        assert_eq!(alpha_bbox(&frames[1]), Some([5, 4, 8, 8]));
    }

    #[test]
    fn magenta_key_matches_assetd_rule() {
        let mut frames = vec![synth(8, 8, [255, 0, 255, 255], [220, 60, 40, 255], [2, 2, 4, 4])];
        apply_chroma_key(&mut frames, ChromaKey::Magenta);
        assert_eq!(frames[0].px(0, 0)[3], 0, "纯品红底");
        assert_eq!(frames[0].px(4, 4)[3], 255, "红屋不该被当品红");
        // none 模式一个像素都不动。
        let mut untouched = vec![synth(8, 8, [255, 0, 255, 255], [220, 60, 40, 255], [2, 2, 4, 4])];
        apply_chroma_key(&mut untouched, ChromaKey::None);
        assert_eq!(untouched[0].px(0, 0)[3], 255);
    }

    #[test]
    fn union_crop_gives_equal_cells_tight_does_not() {
        let mk = |rect: [u32; 4]| {
            let mut f = vec![synth(16, 16, [0, 0, 0, 0], [20, 200, 20, 255], rect)];
            apply_chroma_key(&mut f, ChromaKey::None);
            f.pop().unwrap()
        };
        // 一帧 4x4、一帧 6x6 且偏移 → union = [3,3,7,7]。
        let frames = vec![mk([3, 3, 4, 4]), mk([4, 4, 6, 6])];
        let u = pack_atlas(&frames, CropMode::Union, 2).unwrap();
        assert_eq!(u.boxes.len(), 2);
        assert_eq!(u.boxes[0][2..], [7, 7], "union 下所有帧等大");
        assert_eq!(u.boxes[1][2..], [7, 7]);
        // 2 帧 → cols=2, rows=1;宽 = 2*7 + 3*2 = 20,高 = 7 + 2*2 = 11。
        assert_eq!((u.width, u.height), (20, 11));
        assert_eq!(u.boxes[0][..2], [2, 2]);
        assert_eq!(u.boxes[1][..2], [11, 2]);
        // 格间 padding 须全透明,否则连通域切帧会把相邻帧粘一块。
        let seam = ((2usize) * (u.width as usize) + 9) * 4 + 3;
        assert_eq!(u.rgba[seam], 0);

        let t = pack_atlas(&frames, CropMode::Tight, 0).unwrap();
        assert_eq!(t.boxes[0][2..], [4, 4], "tight 下逐帧紧致");
        assert_eq!(t.boxes[1][2..], [6, 6]);
    }

    #[test]
    fn black_key_preserves_emissive_colors_after_compositing_and_clears_black() {
        let originals = [[0, 0, 0, 255], [3, 2, 1, 255], [12, 48, 120, 255],
            [100, 10, 150, 255], [20, 130, 40, 128], [255, 255, 255, 255]];
        let mut frame = Frame::new(6, 1, originals.into_iter().flatten().collect()).unwrap();
        apply_chroma_key(std::slice::from_mut(&mut frame), ChromaKey::Black);
        assert_eq!(frame.px(0, 0), &[0, 0, 0, 0]);
        assert_eq!(frame.px(1, 0), &[0, 0, 0, 0]);
        for (index, original) in originals.iter().enumerate().skip(2) {
            let actual = frame.px(index as u32, 0);
            for channel in 0..3 {
                let expected = original[channel] as i32 * original[3] as i32 / 255;
                let composited = actual[channel] as i32 * actual[3] as i32 / 255;
                assert!((expected - composited).abs() <= 1, "emissive color changed at {index}/{channel}");
            }
        }
        assert_eq!(ChromaKey::parse("black"), Some(ChromaKey::Black));
        assert_eq!(ChromaKey::Black.as_str(), "black");
    }

    #[test]
    fn pack_grid_is_near_square_and_pixels_land_in_cells() {
        let frames: Vec<Frame> = (0..5)
            .map(|i| synth(4, 4, [0, 0, 0, 0], [10 + i * 10, 200, 20, 255], [1, 1, 2, 2]))
            .collect();
        // 5 帧 → cols=3, rows=2;格 2x2(union of [1,1,2,2]),padding=1。
        let a = pack_atlas(&frames, CropMode::Union, 1).unwrap();
        assert_eq!(a.boxes.len(), 5);
        assert_eq!((a.width, a.height), (3 * 2 + 4, 2 * 2 + 3));
        // 第 4 帧(index 3)落第二行第一列。
        assert_eq!(a.boxes[3], [1, 4, 2, 2]);
        let p = ((4usize) * (a.width as usize) + 1) * 4;
        assert_eq!(a.rgba[p], 40, "像素须搬到对应格位");
    }

    #[test]
    fn fully_keyed_frame_fails_honestly() {
        // 全帧同色 → auto 抠成空 → 报错带排查指引,不产出空图集。
        let mut frames = vec![synth(8, 8, [7, 7, 7, 255], [7, 7, 7, 255], [0, 0, 8, 8])];
        apply_chroma_key(&mut frames, ChromaKey::Auto);
        let err = pack_atlas(&frames, CropMode::Union, 2).unwrap_err();
        assert_eq!(err.code, GEN_BAD_PARAMS);
        assert!(err.message.contains("chromaKey=none"), "{}", err.message);
    }

    #[test]
    fn bad_params_rejected() {
        let bad = |f: FrameOptions| f.validate().unwrap_err().code;
        assert_eq!(bad(FrameOptions { fps: 0.0, ..Default::default() }), GEN_BAD_PARAMS);
        assert_eq!(bad(FrameOptions { fps: 99.0, ..Default::default() }), GEN_BAD_PARAMS);
        assert_eq!(bad(FrameOptions { max_frames: 1, ..Default::default() }), GEN_BAD_PARAMS);
        assert_eq!(bad(FrameOptions { max_frames: 999, ..Default::default() }), GEN_BAD_PARAMS);
        assert_eq!(bad(FrameOptions { padding: 99, ..Default::default() }), GEN_BAD_PARAMS);
        assert_eq!(
            bad(FrameOptions { trim_start_sec: Some(-1.0), ..Default::default() }),
            GEN_BAD_PARAMS
        );
        assert!(FrameOptions::default().validate().is_ok());
        assert_eq!(ChromaKey::parse("auto"), Some(ChromaKey::Auto));
        assert_eq!(ChromaKey::parse("nope"), None);
        assert_eq!(CropMode::parse("union"), Some(CropMode::Union));
        assert_eq!(CropMode::parse("nope"), None);
    }
}
