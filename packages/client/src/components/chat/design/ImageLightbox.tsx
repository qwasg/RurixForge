import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Minus, Plus, RotateCcw, X } from 'lucide-react';

/**
 * D-045:设计稿 / 截帧的放大查看(滚轮缩放、拖拽平移、Esc 关闭)。
 * 聊天卡片里只有 300–560px 宽,看不清按钮与文字细节,审稿必须能放大。
 */
export default function ImageLightbox({
  src,
  title,
  onClose,
}: {
  src: string;
  title: string;
  onClose: () => void;
}) {
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const zoom = (factor: number) => setScale((s) => Math.min(8, Math.max(0.25, s * factor)));
  const resetView = () => {
    setScale(1);
    setOffset({ x: 0, y: 0 });
  };

  return createPortal(
    <div
      data-testid="design-lightbox"
      role="dialog"
      aria-label={title}
      className="fixed inset-0 z-[100] flex flex-col bg-black/80"
      onClick={onClose}
    >
      <div className="flex h-10 shrink-0 items-center gap-2 px-3 text-[12px] text-white/90" onClick={(e) => e.stopPropagation()}>
        <span className="min-w-0 flex-1 truncate">{title}</span>
        <span className="tabular-nums text-white/60">{Math.round(scale * 100)}%</span>
        <button type="button" aria-label="缩小" className="rounded p-1 hover:bg-white/10" onClick={() => zoom(1 / 1.25)}>
          <Minus size={14} />
        </button>
        <button type="button" aria-label="放大" className="rounded p-1 hover:bg-white/10" onClick={() => zoom(1.25)}>
          <Plus size={14} />
        </button>
        <button type="button" aria-label="复位" className="rounded p-1 hover:bg-white/10" onClick={resetView}>
          <RotateCcw size={14} />
        </button>
        <button
          type="button"
          data-testid="design-lightbox-close"
          aria-label="关闭"
          className="rounded p-1 hover:bg-white/10"
          onClick={onClose}
        >
          <X size={14} />
        </button>
      </div>
      <div
        className="relative min-h-0 flex-1 cursor-grab overflow-hidden active:cursor-grabbing"
        onClick={(e) => e.stopPropagation()}
        onWheel={(e) => zoom(e.deltaY < 0 ? 1.15 : 1 / 1.15)}
        onPointerDown={(e) => {
          drag.current = { x: e.clientX, y: e.clientY, ox: offset.x, oy: offset.y };
          (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
        }}
        onPointerMove={(e) => {
          const d = drag.current;
          if (d) setOffset({ x: d.ox + e.clientX - d.x, y: d.oy + e.clientY - d.y });
        }}
        onPointerUp={() => {
          drag.current = null;
        }}
      >
        <img
          src={src}
          alt={title}
          draggable={false}
          className="absolute left-1/2 top-1/2 max-h-[90%] max-w-[95%] select-none"
          style={{
            transform: `translate(calc(-50% + ${offset.x}px), calc(-50% + ${offset.y}px)) scale(${scale})`,
            imageRendering: scale >= 2 ? 'pixelated' : 'auto',
          }}
        />
      </div>
    </div>,
    document.body,
  );
}
