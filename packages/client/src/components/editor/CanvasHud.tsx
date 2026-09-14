import { Maximize2, ZoomIn, ZoomOut } from 'lucide-react';
import { cn } from '@/lib/cn';
import { ZOOM_MAX, ZOOM_MIN, ZOOM_STEP, type CanvasViewport } from '@/lib/useCanvasViewport';

/**
 * 无限画布右下角悬浮控件:缩小 / 当前倍率(点击回 100%)/ 放大 / 适应内容。
 * 无限画布没有滚动条兜底,倍率与「把内容找回来」的入口必须常驻可见。
 */

const hudBtn =
  'flex h-6 w-6 items-center justify-center rounded text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:opacity-40 disabled:hover:bg-transparent';

export interface CanvasHudProps {
  vp: CanvasViewport;
  /** 适应内容(由各画布按自己的节点包围盒实现);内容为空时给 undefined 置灰 */
  onFit?: () => void;
  /** data-testid 前缀,如 board / graph */
  prefix: string;
}

export default function CanvasHud({ vp, onFit, prefix }: CanvasHudProps) {
  return (
    <div
      data-testid={`${prefix}-canvas-hud`}
      className="absolute bottom-2 right-2 z-20 flex items-center gap-0.5 rounded-md border border-edge-strong bg-shell-panel p-0.5 shadow-composer"
    >
      <button
        type="button"
        data-testid={`${prefix}-zoom-out`}
        title="缩小(Ctrl + 滚轮)"
        disabled={vp.view.k <= ZOOM_MIN}
        onClick={() => vp.zoomBy(1 / ZOOM_STEP)}
        className={hudBtn}
      >
        <ZoomOut size={12} strokeWidth={1.8} />
      </button>
      <button
        type="button"
        data-testid={`${prefix}-zoom-reset`}
        title="回到 100%"
        onClick={vp.resetView}
        className={cn(hudBtn, 'w-11 font-mono text-2xs')}
      >
        {vp.zoomPercent}%
      </button>
      <button
        type="button"
        data-testid={`${prefix}-zoom-in`}
        title="放大(Ctrl + 滚轮)"
        disabled={vp.view.k >= ZOOM_MAX}
        onClick={() => vp.zoomBy(ZOOM_STEP)}
        className={hudBtn}
      >
        <ZoomIn size={12} strokeWidth={1.8} />
      </button>
      <span className="mx-0.5 h-4 w-px bg-edge-strong" />
      <button
        type="button"
        data-testid={`${prefix}-fit`}
        title="适应内容:把全部节点收进视野"
        disabled={!onFit}
        onClick={onFit}
        className={hudBtn}
      >
        <Maximize2 size={12} strokeWidth={1.8} />
      </button>
    </div>
  );
}
