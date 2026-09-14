import { useEffect, useRef, useState } from 'react';
import { Pause, Play } from 'lucide-react';
import { cn } from '@/lib/cn';
import { clipFrameDuration, useSpriteStore, type SpriteFrame } from '@/lib/spriteStore';

/**
 * clip 预览播放器(F-GAME-4):rAF 按逐帧时长推进(duration/帧数 优先,否则 1/fps),
 * 小窗按当前帧 bbox 从贴图裁绘(棋盘格底);时间轴 rail 逐帧缩略图 + 进度指针,
 * 点缩略图 = 暂停 seek 并同步选中该帧 bbox。循环/非循环行为与后端一致(hold 停末帧)。
 */

const THUMB = 40;

/** 棋盘格底(小窗/缩略图共用)。 */
function checker(ctx: CanvasRenderingContext2D, w: number, h: number, cell = 6) {
  ctx.fillStyle = '#2c2c34';
  ctx.fillRect(0, 0, w, h);
  ctx.fillStyle = '#24242b';
  for (let y = 0; y < h; y += cell) {
    for (let x = 0; x < w; x += cell) {
      if (((x / cell + y / cell) & 1) === 0) ctx.fillRect(x, y, cell, cell);
    }
  }
}

/** 把帧 bbox 从贴图居中等比裁绘进 w×h 画布(jsdom 无 2d 上下文时静默跳过)。 */
function drawFrameInto(
  canvas: HTMLCanvasElement | null,
  img: HTMLImageElement | null,
  frame: SpriteFrame | undefined,
  w: number,
  h: number,
) {
  const ctx = canvas?.getContext('2d');
  if (!canvas || !ctx) return;
  canvas.width = w;
  canvas.height = h;
  checker(ctx, w, h);
  if (!img || !frame) return;
  const [x, y, fw, fh] = frame.bbox;
  const s = Math.min((w - 4) / fw, (h - 4) / fh);
  const dw = fw * s;
  const dh = fh * s;
  ctx.imageSmoothingEnabled = s < 1;
  ctx.drawImage(img, x, y, fw, fh, (w - dw) / 2, (h - dh) / 2, dw, dh);
}

/** 时间轴单帧缩略图(点击 = 暂停 seek + 选中该帧)。 */
function FrameThumb({
  img,
  frame,
  frameName,
  idx,
  active,
}: {
  img: HTMLImageElement | null;
  frame: SpriteFrame | undefined;
  frameName: string;
  idx: number;
  active: boolean;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    drawFrameInto(ref.current, img, frame, THUMB, THUMB);
  }, [img, frame]);
  return (
    <button
      type="button"
      title={`${idx}: ${frameName}(点击暂停并定位)`}
      data-testid={`sprite-timeline-${idx}`}
      onClick={() => {
        const st = useSpriteStore.getState();
        st.seekPreview(idx);
        st.selectFrame(frameName);
      }}
      className={cn(
        'shrink-0 overflow-hidden rounded border',
        active ? 'border-acc' : 'border-edge hover:border-fg-4',
      )}
      style={{ width: THUMB, height: THUMB }}
    >
      <canvas ref={ref} width={THUMB} height={THUMB} />
    </button>
  );
}

export default function SpritePreview({ img }: { img: HTMLImageElement | null }) {
  const doc = useSpriteStore((s) => s.doc);
  const selectedClip = useSpriteStore((s) => s.selectedClip);
  const playing = useSpriteStore((s) => s.playing);
  const frameIdx = useSpriteStore((s) => s.frameIdx);
  const elapsed = useSpriteStore((s) => s.elapsed);

  const winRef = useRef<HTMLCanvasElement>(null);
  const [winSize, setWinSize] = useState({ w: 148, h: 120 });
  const winBoxRef = useRef<HTMLDivElement>(null);

  const clip = selectedClip ? doc?.clips[selectedClip] : undefined;
  const frameName = clip?.frames[Math.min(frameIdx, Math.max(0, (clip?.frames.length ?? 1) - 1))];
  const frame = frameName ? doc?.frames[frameName] : undefined;

  // 播放小窗尺寸随容器。
  useEffect(() => {
    const el = winBoxRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect;
      if (r) setWinSize({ w: Math.max(48, Math.round(r.width)), h: Math.max(48, Math.round(r.height)) });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // rAF 播放环:store.tickPreview 按真实 dt 推进(收尾/循环语义在 store)。
  useEffect(() => {
    if (!playing) return;
    let raf = 0;
    let last = performance.now();
    let alive = true;
    const loop = (now: number) => {
      if (!alive) return;
      useSpriteStore.getState().tickPreview((now - last) / 1000);
      last = now;
      raf = window.requestAnimationFrame(loop);
    };
    raf = window.requestAnimationFrame(loop);
    return () => {
      alive = false;
      window.cancelAnimationFrame(raf);
    };
  }, [playing]);

  // 小窗重绘(当前帧变化/贴图就绪/尺寸变化)。
  useEffect(() => {
    drawFrameInto(winRef.current, img, frame, winSize.w, winSize.h);
  }, [img, frame, winSize]);

  if (!doc) return null;

  const frames = clip?.frames ?? [];
  const dur = clip ? clipFrameDuration(clip) : 0;
  // 进度指针:已播帧数 + 当前帧内进度,占总帧数比例。
  const progress =
    frames.length > 0 ? Math.min(1, (frameIdx + (dur > 0 ? Math.min(elapsed / dur, 1) : 0)) / frames.length) : 0;

  return (
    <div className="flex h-[168px] shrink-0 border-t border-edge" data-testid="sprite-preview">
      {/* 播放小窗 */}
      <div ref={winBoxRef} className="w-[168px] shrink-0 overflow-hidden border-r border-edge">
        <canvas ref={winRef} />
      </div>

      <div className="flex min-w-0 flex-1 flex-col p-2">
        {!clip ? (
          <p className="text-2xs text-fg-4" data-testid="sprite-preview-empty">
            在右栏选中一个 clip 后可播放预览
          </p>
        ) : (
          <>
            <div className="flex items-center gap-2 pb-1.5">
              <button
                type="button"
                data-testid="sprite-preview-play"
                title={playing ? '暂停' : '播放'}
                disabled={frames.length === 0}
                onClick={() => {
                  const st = useSpriteStore.getState();
                  if (playing) st.pause();
                  else st.play();
                }}
                className="flex h-6 w-6 items-center justify-center rounded-md border border-edge text-fg-2 transition-colors hover:bg-shell-hover disabled:cursor-not-allowed disabled:opacity-30"
              >
                {playing ? <Pause size={12} /> : <Play size={12} />}
              </button>
              <span className="truncate text-2xs text-fg-2">{selectedClip}</span>
              <span className="font-mono text-2xs text-fg-4" data-testid="sprite-preview-pos">
                {frames.length === 0 ? '0/0' : `${Math.min(frameIdx, frames.length - 1) + 1}/${frames.length}`}
              </span>
              <span className="text-2xs text-fg-4">
                {clip.duration != null
                  ? `总 ${clip.duration}s(优先)`
                  : `${clip.fps}fps`}
                {' · '}
                {clip.loop ? '循环' : `单次(${clip.onFinish === 'first' ? '回首帧' : '停末帧'})`}
              </span>
            </div>

            {/* 时间轴 rail:逐帧缩略图 + 进度指针 */}
            <div className="min-h-0 flex-1 overflow-x-auto" data-testid="sprite-timeline">
              <div className="flex gap-1">
                {frames.map((fn, i) => (
                  <FrameThumb
                    key={`${fn}-${i}`}
                    img={img}
                    frame={doc.frames[fn]}
                    frameName={fn}
                    idx={i}
                    active={i === Math.min(frameIdx, frames.length - 1)}
                  />
                ))}
                {frames.length === 0 && <p className="text-2xs text-warn">clip 帧序列为空</p>}
              </div>
              {frames.length > 0 && (
                <div className="relative mt-1 h-1 rounded bg-shell-sunk" style={{ width: frames.length * (THUMB + 4) - 4 }}>
                  <div
                    className="absolute inset-y-0 left-0 rounded bg-acc"
                    data-testid="sprite-progress"
                    style={{ width: `${progress * 100}%` }}
                  />
                </div>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
