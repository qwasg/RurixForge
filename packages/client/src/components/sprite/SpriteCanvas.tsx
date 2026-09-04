import { useEffect, useRef, useState } from 'react';
import { useSpriteStore, resolvePivot } from '@/lib/spriteStore';

/**
 * 精灵图集画布(F-GAME-4,canvas 手绘,照 ViewportCanvas 惯例):
 * - 棋盘格底 + 贴图原图渲染(放大关闭平滑,像素风);
 * - 滚轮缩放(锚点=光标)、空格/中键拖拽平移;
 * - bbox:空白拖拽=画新框(自动命名 frame_<n>)、框内拖拽=移动、四角柄=resize(Shift 等比)、
 *   Delete 删选中帧;
 * - pivot 十字准星:每帧一枚(帧级覆盖带 * 标),可拖拽写帧级 pivot(0..1 钳制 4 位小数)。
 * 交互全部指针事件;贴图 HTMLImageElement 由 SpriteEditorView 解码后传入(预览共用)。
 */

interface View {
  scale: number;
  ox: number;
  oy: number;
}

type DragMode =
  | { kind: 'pan' }
  | { kind: 'move'; name: string; bbox: [number, number, number, number]; tx: number; ty: number }
  | { kind: 'resize'; name: string; ax: number; ay: number; w0: number; h0: number }
  | { kind: 'pivot'; name: string }
  | { kind: 'new'; tx: number; ty: number };

const PIVOT_HIT_PX = 9;
const HANDLE_HIT_PX = 7;

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** 棋盘格(屏幕空间 10px 方格,裁剪进贴图矩形)。 */
function drawChecker(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.fillStyle = '#2c2c34';
  ctx.fillRect(x, y, w, h);
  ctx.fillStyle = '#24242b';
  const cell = 10;
  const x0 = Math.floor(x / cell) * cell;
  const y0 = Math.floor(y / cell) * cell;
  for (let cy = y0; cy < y + h; cy += cell) {
    for (let cx = x0; cx < x + w; cx += cell) {
      if (((cx / cell + cy / cell) & 1) === 0) ctx.fillRect(cx, cy, cell, cell);
    }
  }
  ctx.restore();
}

export default function SpriteCanvas({ img }: { img: HTMLImageElement | null }) {
  const doc = useSpriteStore((s) => s.doc);
  const selectedFrame = useSpriteStore((s) => s.selectedFrame);
  const texDataUrl = useSpriteStore((s) => s.texDataUrl);
  const texW = useSpriteStore((s) => s.texW);
  const texH = useSpriteStore((s) => s.texH);
  const assetPath = useSpriteStore((s) => s.assetPath);

  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 640, h: 400, dpr: 1 });
  const [view, setView] = useState<View>({ scale: 1, ox: 16, oy: 16 });
  /** 画新框预览(贴图坐标,浮点;抬起时落帧)。 */
  const [dragRect, setDragRect] = useState<[number, number, number, number] | null>(null);
  const [spaceHeld, setSpaceHeld] = useState(false);
  const [panning, setPanning] = useState(false);

  const drag = useRef<DragMode | null>(null);
  /** 手势序号:同一次拖拽的连续变更共用一个撤销快照(store coalesce 键)。 */
  const gestureId = useRef(0);
  const fittedFor = useRef<string | null>(null);

  // 容器尺寸测量(照 ViewportCanvas;测试环境 ResizeObserver 有桩)。
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const apply = (w: number, h: number) =>
      setSize({
        w: Math.max(64, Math.round(w)),
        h: Math.max(64, Math.round(h)),
        dpr: window.devicePixelRatio || 1,
      });
    const rect = el.getBoundingClientRect();
    apply(rect.width || 640, rect.height || 400);
    const ro = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect;
      if (r) apply(r.width, r.height);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // 初次适配:贴图尺寸已知后居中放入视野(每资产一次;后续用户手势为准)。
  useEffect(() => {
    if (texW <= 0 || texH <= 0 || fittedFor.current === assetPath) return;
    fittedFor.current = assetPath;
    const scale = clamp(Math.min((size.w - 48) / texW, (size.h - 48) / texH), 0.05, 8);
    setView({
      scale,
      ox: (size.w - texW * scale) / 2,
      oy: (size.h - texH * scale) / 2,
    });
  }, [texW, texH, size.w, size.h, assetPath]);

  // 空格 = 平移修饰键(全局监听,文本控件避让)。
  useEffect(() => {
    const isInteractive = (t: EventTarget | null) =>
      t instanceof HTMLElement && t.closest('input, textarea, select, [contenteditable="true"]') != null;
    const down = (e: KeyboardEvent) => {
      if (e.code === 'Space' && !isInteractive(e.target)) setSpaceHeld(true);
    };
    const up = (e: KeyboardEvent) => {
      if (e.code === 'Space') setSpaceHeld(false);
    };
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    return () => {
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
    };
  }, []);

  // ---------- 坐标换算 ----------
  const toScreen = (tx: number, ty: number): [number, number] => [
    tx * view.scale + view.ox,
    ty * view.scale + view.oy,
  ];
  const toTex = (e: React.PointerEvent | React.WheelEvent): { tx: number; ty: number; sx: number; sy: number } => {
    const rect = containerRef.current?.getBoundingClientRect();
    const sx = e.clientX - (rect?.left ?? 0);
    const sy = e.clientY - (rect?.top ?? 0);
    return { tx: (sx - view.ox) / view.scale, ty: (sy - view.oy) / view.scale, sx, sy };
  };

  // ---------- 命中检测 ----------
  /** 帧 pivot 准星命中(优先选中帧,其余按名序)。 */
  const pivotAt = (sx: number, sy: number): string | null => {
    if (!doc) return null;
    const names = Object.keys(doc.frames);
    const ordered = selectedFrame ? [selectedFrame, ...names.filter((n) => n !== selectedFrame)] : names;
    for (const name of ordered) {
      const f = doc.frames[name];
      if (!f) continue;
      const p = resolvePivot(doc, name);
      const [px, py] = toScreen(f.bbox[0] + p[0] * f.bbox[2], f.bbox[1] + p[1] * f.bbox[3]);
      if (Math.hypot(sx - px, sy - py) <= PIVOT_HIT_PX) return name;
    }
    return null;
  };

  /** 选中帧四角柄命中 → 对角锚点(resize 拖拽的固定角)。 */
  const handleAt = (sx: number, sy: number): { ax: number; ay: number } | null => {
    if (!doc || !selectedFrame) return null;
    const f = doc.frames[selectedFrame];
    if (!f) return null;
    const [x, y, w, h] = f.bbox;
    const corners: Array<[number, number, number, number]> = [
      [x, y, x + w, y + h],
      [x + w, y, x, y + h],
      [x, y + h, x + w, y],
      [x + w, y + h, x, y],
    ];
    for (const [cx, cy, ax, ay] of corners) {
      const [px, py] = toScreen(cx, cy);
      if (Math.abs(sx - px) <= HANDLE_HIT_PX && Math.abs(sy - py) <= HANDLE_HIT_PX) {
        return { ax, ay };
      }
    }
    return null;
  };

  /** 命中帧(含点;重叠取面积最小者,便于套嵌小框)。 */
  const frameAt = (tx: number, ty: number): string | null => {
    if (!doc) return null;
    let best: string | null = null;
    let bestArea = Infinity;
    for (const [name, f] of Object.entries(doc.frames)) {
      const [x, y, w, h] = f.bbox;
      if (tx >= x && tx <= x + w && ty >= y && ty <= y + h && w * h < bestArea) {
        best = name;
        bestArea = w * h;
      }
    }
    return best;
  };

  // ---------- 指针交互 ----------
  const st = useSpriteStore.getState;

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!doc) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    e.currentTarget.focus();
    const { tx, ty, sx, sy } = toTex(e);
    gestureId.current += 1;

    if (e.button === 1 || (e.button === 0 && spaceHeld)) {
      e.preventDefault();
      drag.current = { kind: 'pan' };
      setPanning(true);
      return;
    }
    if (e.button !== 0) return;

    const pivotName = pivotAt(sx, sy);
    if (pivotName) {
      st().selectFrame(pivotName);
      drag.current = { kind: 'pivot', name: pivotName };
      return;
    }
    const corner = handleAt(sx, sy);
    if (corner && selectedFrame) {
      const f = doc.frames[selectedFrame];
      drag.current = {
        kind: 'resize',
        name: selectedFrame,
        ax: corner.ax,
        ay: corner.ay,
        w0: f?.bbox[2] ?? 1,
        h0: f?.bbox[3] ?? 1,
      };
      return;
    }
    const hit = frameAt(tx, ty);
    if (hit) {
      st().selectFrame(hit);
      const f = doc.frames[hit];
      drag.current = { kind: 'move', name: hit, bbox: [...f.bbox], tx, ty };
      return;
    }
    // 空白:画新框(抬起时若过小则视为点击 = 取消选中)。
    drag.current = { kind: 'new', tx, ty };
    setDragRect([tx, ty, 0, 0]);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d) return;
    const coalesce = `gesture-${gestureId.current}`;
    if (d.kind === 'pan') {
      setView((v) => ({ ...v, ox: v.ox + e.movementX, oy: v.oy + e.movementY }));
      return;
    }
    const { tx, ty } = toTex(e);
    if (d.kind === 'move') {
      let nx = d.bbox[0] + (tx - d.tx);
      let ny = d.bbox[1] + (ty - d.ty);
      nx = Math.max(0, texW > 0 ? Math.min(nx, texW - d.bbox[2]) : nx);
      ny = Math.max(0, texH > 0 ? Math.min(ny, texH - d.bbox[3]) : ny);
      st().setFrameBbox(d.name, [nx, ny, d.bbox[2], d.bbox[3]], { coalesce });
    } else if (d.kind === 'resize') {
      let cx = texW > 0 ? clamp(tx, 0, texW) : Math.max(0, tx);
      let cy = texH > 0 ? clamp(ty, 0, texH) : Math.max(0, ty);
      if (e.shiftKey && d.w0 > 0 && d.h0 > 0) {
        // 等比:取主导轴比例,另一轴按初始纵横比跟随(方向保持指针侧)。
        const s = Math.max(Math.abs(cx - d.ax) / d.w0, Math.abs(cy - d.ay) / d.h0, 0.01);
        cx = d.ax + Math.sign(cx - d.ax || 1) * d.w0 * s;
        cy = d.ay + Math.sign(cy - d.ay || 1) * d.h0 * s;
      }
      st().setFrameBbox(
        d.name,
        [Math.min(d.ax, cx), Math.min(d.ay, cy), Math.abs(cx - d.ax), Math.abs(cy - d.ay)],
        { coalesce },
      );
    } else if (d.kind === 'pivot') {
      const f = st().doc?.frames[d.name];
      if (f) {
        st().setFramePivot(
          d.name,
          [(tx - f.bbox[0]) / Math.max(1, f.bbox[2]), (ty - f.bbox[1]) / Math.max(1, f.bbox[3])],
          { coalesce },
        );
      }
    } else if (d.kind === 'new') {
      setDragRect([Math.min(d.tx, tx), Math.min(d.ty, ty), Math.abs(tx - d.tx), Math.abs(ty - d.ty)]);
    }
  };

  const onPointerUp = () => {
    const d = drag.current;
    drag.current = null;
    setPanning(false);
    if (d?.kind === 'new') {
      const r = dragRect;
      setDragRect(null);
      if (r && Math.round(r[2]) >= 1 && Math.round(r[3]) >= 1) {
        st().addFrame([
          Math.max(0, Math.round(r[0])),
          Math.max(0, Math.round(r[1])),
          Math.max(1, Math.round(r[2])),
          Math.max(1, Math.round(r[3])),
        ]);
      } else {
        // 过小 = 单击空白:取消选中。
        st().selectFrame(null);
      }
    }
  };

  const onWheel = (e: React.WheelEvent<HTMLDivElement>) => {
    const { sx, sy } = toTex(e);
    setView((v) => {
      const scale = clamp(v.scale * Math.exp(-e.deltaY * 0.0015), 0.05, 64);
      // 锚点=光标:缩放前后光标下的贴图点不动。
      return {
        scale,
        ox: sx - ((sx - v.ox) / v.scale) * scale,
        oy: sy - ((sy - v.oy) / v.scale) * scale,
      };
    });
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Delete' && selectedFrame) {
      e.preventDefault();
      st().deleteFrame(selectedFrame);
    }
  };

  // ---------- 绘制(状态驱动重绘;jsdom 无 2d 上下文时静默跳过) ----------
  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext('2d');
    if (!canvas || !ctx || !doc) return;
    canvas.width = Math.round(size.w * size.dpr);
    canvas.height = Math.round(size.h * size.dpr);
    ctx.setTransform(size.dpr, 0, 0, size.dpr, 0, 0);
    ctx.fillStyle = '#17171c';
    ctx.fillRect(0, 0, size.w, size.h);

    const { scale, ox, oy } = view;
    const sw = texW > 0 ? texW : img?.naturalWidth ?? 0;
    const sh = texH > 0 ? texH : img?.naturalHeight ?? 0;
    if (sw > 0 && sh > 0) drawChecker(ctx, ox, oy, sw * scale, sh * scale);
    if (img) {
      ctx.imageSmoothingEnabled = scale < 1;
      ctx.drawImage(img, ox, oy, img.naturalWidth * scale, img.naturalHeight * scale);
    }

    // bbox 层。
    ctx.font = '10px ui-monospace, monospace';
    for (const [name, f] of Object.entries(doc.frames)) {
      const [x, y, w, h] = f.bbox;
      const sel = name === selectedFrame;
      const rx = x * scale + ox;
      const ry = y * scale + oy;
      ctx.lineWidth = sel ? 1.5 : 1;
      ctx.strokeStyle = sel ? '#f0a63f' : 'rgba(255,255,255,0.45)';
      ctx.strokeRect(rx + 0.5, ry + 0.5, w * scale, h * scale);
      ctx.fillStyle = sel ? '#f0a63f' : 'rgba(255,255,255,0.55)';
      ctx.fillText(name, rx + 1, ry - 3);
      if (sel) {
        // 四角柄。
        ctx.fillStyle = '#f0a63f';
        for (const [cx, cy] of [
          [rx, ry],
          [rx + w * scale, ry],
          [rx, ry + h * scale],
          [rx + w * scale, ry + h * scale],
        ]) {
          ctx.fillRect(cx - 3, cy - 3, 6, 6);
        }
      }
      // pivot 十字准星(帧级覆盖标 *)。
      const p = resolvePivot(doc, name);
      const px = (x + p[0] * w) * scale + ox;
      const py = (y + p[1] * h) * scale + oy;
      ctx.strokeStyle = sel ? '#59d0d0' : 'rgba(89,208,208,0.6)';
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(px - 6, py);
      ctx.lineTo(px + 6, py);
      ctx.moveTo(px, py - 6);
      ctx.lineTo(px, py + 6);
      ctx.stroke();
      if (f.pivot) {
        ctx.fillStyle = '#59d0d0';
        ctx.fillText('*', px + 4, py - 4);
      }
    }

    // 画新框预览(虚线)。
    if (dragRect) {
      ctx.setLineDash([4, 3]);
      ctx.strokeStyle = '#f0a63f';
      ctx.strokeRect(
        dragRect[0] * scale + ox + 0.5,
        dragRect[1] * scale + oy + 0.5,
        dragRect[2] * scale,
        dragRect[3] * scale,
      );
      ctx.setLineDash([]);
    }
  }, [doc, selectedFrame, view, size, img, dragRect, texW, texH]);

  return (
    <div
      ref={containerRef}
      data-testid="sprite-canvas"
      role="application"
      aria-label="精灵图集画布"
      tabIndex={0}
      className="relative h-full min-h-0 w-full overflow-hidden outline-none"
      style={{ cursor: panning ? 'grabbing' : spaceHeld ? 'grab' : 'crosshair' }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onWheel={onWheel}
      onKeyDown={onKeyDown}
    >
      <canvas ref={canvasRef} className="h-full w-full" />
      {!texDataUrl && (
        <div className="pointer-events-none absolute inset-x-0 top-2 text-center text-2xs text-fg-4">
          贴图未加载(原图经 asset_thumbnail 直出;失败原因见顶部错误条)
        </div>
      )}
      <div className="pointer-events-none absolute bottom-1.5 left-1.5 rounded-md bg-black/40 px-2 py-0.5 text-2xs text-white/50">
        滚轮缩放 · 空格/中键平移 · 空白拖拽画框 · 框内拖拽移动 · 角柄 resize(Shift 等比)· 拖准星改 pivot · Delete 删帧
      </div>
    </div>
  );
}
