import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { CSSProperties, PointerEvent as ReactPointerEvent } from 'react';

/**
 * 无限画布视口(画板 / NodeGraph 共用)。
 * 视口 = 定长容器 + 一层 translate/scale 的「世界层」:内容坐标不再受容器尺寸约束,
 * 四向无界(可为负),看得到哪一块只由 {x, y, k} 决定,不再靠 overflow-auto 撑包围盒。
 * 网格画在未变换的容器背景上(CSS 双层渐变),因此随视口无限延伸,不需要造大 SVG。
 * 交互:滚轮平移(shift 横向)、ctrl/⌘+滚轮以指针为锚缩放、空白处或中键拖拽平移。
 * 视口位置按 storageKey 落 localStorage,切页签回来不丢位置。
 */

export interface CanvasView {
  /** 世界原点在容器内的屏幕偏移(px) */
  x: number;
  y: number;
  /** 缩放系数 */
  k: number;
}

export interface WorldRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const ZOOM_MIN = 0.1;
export const ZOOM_MAX = 3;
/** HUD 按钮单步倍率 */
export const ZOOM_STEP = 1.2;

/** 世界坐标下的细网格步长;粗网格 = 5 倍 */
const GRID = 20;
const GRID_MAJOR = GRID * 5;
/** 细网格屏幕间距小于此值就糊成一片,只留粗网格 */
const GRID_MIN_PX = 8;

const IDENTITY: CanvasView = { x: 0, y: 0, k: 1 };

function clampZoom(k: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, k));
}

function loadView(key?: string): CanvasView {
  if (!key) return IDENTITY;
  try {
    const raw = globalThis.localStorage?.getItem(key);
    if (!raw) return IDENTITY;
    const v = JSON.parse(raw) as Partial<CanvasView>;
    if (
      typeof v.x !== 'number' ||
      typeof v.y !== 'number' ||
      typeof v.k !== 'number' ||
      !Number.isFinite(v.x) ||
      !Number.isFinite(v.y) ||
      !Number.isFinite(v.k)
    ) {
      return IDENTITY;
    }
    return { x: v.x, y: v.y, k: clampZoom(v.k) };
  } catch {
    return IDENTITY;
  }
}

export interface CanvasViewportOptions {
  /** 视口位置持久化键(localStorage);不给则不持久化 */
  storageKey?: string;
  /** 命中这些选择器的元素上按下不起平移(留给节点自己的拖拽 / 编辑) */
  panExclude?: string;
}

export interface CanvasViewport {
  /** 挂到视口容器(定长、overflow-hidden)的 ref */
  ref: (node: HTMLDivElement | null) => void;
  view: CanvasView;
  panning: boolean;
  zoomPercent: number;
  /** 容器背景样式:无限延伸的双层网格 */
  gridStyle: CSSProperties;
  /** 世界层样式:translate + scale,原点左上 */
  worldStyle: CSSProperties;
  /** 视口容器的按下处理:空白处左键 / 任意处中键 = 平移 */
  onPointerDown: (e: ReactPointerEvent<HTMLElement>) => void;
  /** client 坐标 → 世界坐标 */
  toWorld: (p: { clientX: number; clientY: number }) => [number, number];
  /** 以指针(不给则视口中心)为锚缩放 */
  zoomBy: (factor: number, anchor?: { clientX: number; clientY: number }) => void;
  resetView: () => void;
  /** 把这些世界矩形整体收进视口(不放大到 100% 以上) */
  fitTo: (rects: WorldRect[], padding?: number) => void;
  /** 目标矩形若在视野外,最小平移把它带回来 */
  reveal: (rect: WorldRect, padding?: number) => void;
  /** 内容整体不在视野内时才 fit(读档回来跑飞了的兜底) */
  ensureContentVisible: (rects: WorldRect[], padding?: number) => void;
}

export function useCanvasViewport(opts: CanvasViewportOptions = {}): CanvasViewport {
  const { storageKey, panExclude } = opts;
  const [el, setEl] = useState<HTMLDivElement | null>(null);
  const elRef = useRef<HTMLDivElement | null>(null);
  const ref = useCallback((node: HTMLDivElement | null) => {
    elRef.current = node;
    setEl(node);
  }, []);

  const [view, setView] = useState<CanvasView>(() => loadView(storageKey));
  const [panning, setPanning] = useState(false);
  // 原生 wheel / window 拖拽监听里要读最新视口,状态闭包会过期
  const viewRef = useRef(view);
  viewRef.current = view;

  const box = (): DOMRect | null => elRef.current?.getBoundingClientRect() ?? null;

  const toWorld = useCallback((p: { clientX: number; clientY: number }): [number, number] => {
    const r = box();
    const v = viewRef.current;
    return [(p.clientX - (r?.left ?? 0) - v.x) / v.k, (p.clientY - (r?.top ?? 0) - v.y) / v.k];
  }, []);

  /** 锚点用容器内屏幕坐标给:缩放前后该点下的世界坐标不变 */
  const zoomAtScreen = useCallback((nextK: number, sx: number, sy: number) => {
    setView((v) => {
      const k = clampZoom(nextK);
      if (k === v.k) return v;
      return { k, x: sx - ((sx - v.x) / v.k) * k, y: sy - ((sy - v.y) / v.k) * k };
    });
  }, []);

  const zoomBy = useCallback(
    (factor: number, anchor?: { clientX: number; clientY: number }) => {
      const r = box();
      const sx = anchor ? anchor.clientX - (r?.left ?? 0) : (r?.width ?? 0) / 2;
      const sy = anchor ? anchor.clientY - (r?.top ?? 0) : (r?.height ?? 0) / 2;
      zoomAtScreen(viewRef.current.k * factor, sx, sy);
    },
    [zoomAtScreen],
  );

  const resetView = useCallback(() => setView(IDENTITY), []);

  const fitTo = useCallback((rects: WorldRect[], padding = 48) => {
    const r = box();
    if (!r || r.width === 0 || r.height === 0 || rects.length === 0) return; // 无布局面(jsdom / 未挂载)不猜
    const minX = Math.min(...rects.map((b) => b.x));
    const minY = Math.min(...rects.map((b) => b.y));
    const maxX = Math.max(...rects.map((b) => b.x + b.w));
    const maxY = Math.max(...rects.map((b) => b.y + b.h));
    const w = Math.max(1, maxX - minX);
    const h = Math.max(1, maxY - minY);
    const k = clampZoom(
      Math.min(1, (r.width - padding * 2) / w, (r.height - padding * 2) / h),
    );
    setView({ k, x: (r.width - w * k) / 2 - minX * k, y: (r.height - h * k) / 2 - minY * k });
  }, []);

  const reveal = useCallback((b: WorldRect, padding = 32) => {
    const r = box();
    if (!r || r.width === 0 || r.height === 0) return;
    setView((v) => {
      const left = b.x * v.k + v.x;
      const top = b.y * v.k + v.y;
      const right = (b.x + b.w) * v.k + v.x;
      const bottom = (b.y + b.h) * v.k + v.y;
      let { x, y } = v;
      if (left < padding) x += padding - left;
      else if (right > r.width - padding) x -= right - (r.width - padding);
      if (top < padding) y += padding - top;
      else if (bottom > r.height - padding) y -= bottom - (r.height - padding);
      return x === v.x && y === v.y ? v : { ...v, x, y };
    });
  }, []);

  const ensureContentVisible = useCallback(
    (rects: WorldRect[], padding = 24) => {
      const r = box();
      if (!r || r.width === 0 || r.height === 0 || rects.length === 0) return;
      const v = viewRef.current;
      const seen = rects.some((b) => {
        const left = b.x * v.k + v.x;
        const top = b.y * v.k + v.y;
        return (
          left < r.width - padding &&
          (b.x + b.w) * v.k + v.x > padding &&
          top < r.height - padding &&
          (b.y + b.h) * v.k + v.y > padding
        );
      });
      if (!seen) fitTo(rects);
    },
    [fitTo],
  );

  // ---- 平移:按下起 window 级跟手,指针跑出容器也不断 ----

  const endPanRef = useRef<(() => void) | null>(null);

  const startPan = useCallback((clientX: number, clientY: number) => {
    endPanRef.current?.();
    const base = { sx: clientX, sy: clientY, x: viewRef.current.x, y: viewRef.current.y };
    const onMove = (ev: MouseEvent) => {
      setView((v) => ({ ...v, x: base.x + (ev.clientX - base.sx), y: base.y + (ev.clientY - base.sy) }));
    };
    const stop = () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', stop);
      window.removeEventListener('pointercancel', stop);
      endPanRef.current = null;
      setPanning(false);
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', stop);
    window.addEventListener('pointercancel', stop);
    endPanRef.current = stop;
    setPanning(true);
  }, []);

  useEffect(() => () => endPanRef.current?.(), []);

  const onPointerDown = useCallback(
    (e: ReactPointerEvent<HTMLElement>) => {
      const middle = e.button === 1;
      if (!middle && e.button !== 0) return;
      if (!middle && panExclude) {
        const t = e.target as HTMLElement | null;
        if (t?.closest?.(panExclude)) return; // 节点自己的拖拽 / 内联编辑优先
      }
      startPan(e.clientX, e.clientY);
    },
    [panExclude, startPan],
  );

  // ---- 滚轮:React 的 onWheel 是被动监听,preventDefault 无效,只能挂原生 ----

  useEffect(() => {
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (e.ctrlKey || e.metaKey) {
        const r = el.getBoundingClientRect();
        // 触控板捏合也走 ctrl+wheel:指数映射让快慢两端手感一致
        zoomAtScreen(
          viewRef.current.k * Math.exp(-e.deltaY / 320),
          e.clientX - r.left,
          e.clientY - r.top,
        );
        return;
      }
      const dx = e.shiftKey ? e.deltaY : e.deltaX;
      const dy = e.shiftKey ? 0 : e.deltaY;
      setView((v) => ({ ...v, x: v.x - dx, y: v.y - dy }));
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [el, zoomAtScreen]);

  // ---- 持久化(拖拽中高频改视口,收敛 400ms 再写) ----

  useEffect(() => {
    if (!storageKey) return;
    const t = setTimeout(() => {
      try {
        globalThis.localStorage?.setItem(storageKey, JSON.stringify(view));
      } catch {
        // 写不进静默
      }
    }, 400);
    return () => clearTimeout(t);
  }, [storageKey, view]);

  const gridStyle = useMemo<CSSProperties>(() => {
    const minor = GRID * view.k;
    const major = GRID_MAJOR * view.k;
    const images: string[] = [];
    const sizes: string[] = [];
    if (minor >= GRID_MIN_PX) {
      images.push(
        'linear-gradient(to right, var(--line) 1px, transparent 1px)',
        'linear-gradient(to bottom, var(--line) 1px, transparent 1px)',
      );
      sizes.push(`${minor}px ${minor}px`, `${minor}px ${minor}px`);
    }
    images.push(
      'linear-gradient(to right, var(--line-strong) 1px, transparent 1px)',
      'linear-gradient(to bottom, var(--line-strong) 1px, transparent 1px)',
    );
    sizes.push(`${major}px ${major}px`, `${major}px ${major}px`);
    return {
      backgroundImage: images.join(', '),
      backgroundSize: sizes.join(', '),
      backgroundPosition: `${view.x}px ${view.y}px`,
    };
  }, [view]);

  const worldStyle = useMemo<CSSProperties>(
    () => ({
      transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})`,
      transformOrigin: '0 0',
    }),
    [view],
  );

  return {
    ref,
    view,
    panning,
    zoomPercent: Math.round(view.k * 100),
    gridStyle,
    worldStyle,
    onPointerDown,
    toWorld,
    zoomBy,
    resetView,
    fitTo,
    reveal,
    ensureContentVisible,
  };
}
