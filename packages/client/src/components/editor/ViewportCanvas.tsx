import { useEffect, useRef, useState } from 'react';
import { callTool, ForgeApiError } from '@/lib/forgeApi';
import { bridge } from '@/lib/bridge';
import { useEditorStore, type ViewportInfo } from '@/lib/editorStore';

/**
 * Viewport 画布(F1 wave.2):GPU 场景实渲染帧(rurix-rt vulkan render_exec → Readback)
 * 经 canvas 呈现;交互 = 单击点选 / Alt+左键环绕 / 滚轮缩放 / F 聚焦 / WER gizmo 拖拽。
 *
 * 诚实档:无 vulkan 设备时 viewport_frame 回 DEV_ENV_DEGRADE,画布区显示降级原因,
 * 绝不显示伪造帧。共享纹理原生呈现(G-F1-9)接入后,本组件退为回退腿。
 */

interface FrameResponse extends ViewportInfo {
  width: number;
  height: number;
  format: string;
  pixelsB64: string;
}

const POLL_MS = 150;
const DRAG_THRESHOLD_PX = 4;

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

type DragMode = 'candidate' | 'orbit' | 'gizmo';

export function ViewportCanvas() {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 960, h: 540, dpr: 1 });

  const selectedId = useEditorStore((s) => s.selectedId);
  const degraded = useEditorStore((s) => s.viewportDegraded);
  const info = useEditorStore((s) => s.viewportInfo);
  const gizmo = useEditorStore((s) => s.gizmo);
  const playState = useEditorStore((s) => s.playState);

  /** downX/downY = 按下原点(阈值判定);x/y = 上次位置(增量);gx/gy = gizmo 累计增量 */
  const drag = useRef<{
    downX: number;
    downY: number;
    x: number;
    y: number;
    mode: DragMode | null;
    gx: number;
    gy: number;
  }>({ downX: 0, downY: 0, x: 0, y: 0, mode: null, gx: 0, gy: 0 });

  // 容器尺寸测量(拖拽过程中去抖,结束后才改会话分辨率——帧协商)
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    let timer: number | undefined;
    const apply = (w: number, h: number) =>
      setSize({
        w: clamp(Math.round(w), 16, 1920),
        h: clamp(Math.round(h), 16, 1080),
        dpr: window.devicePixelRatio || 1,
      });
    const rect = el.getBoundingClientRect();
    apply(rect.width, rect.height);
    const ro = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect;
      if (!r) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => apply(r.width, r.height), 200);
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      window.clearTimeout(timer);
    };
  }, []);

  // G-F1-9:向 desktop 主进程上报视口 bounds(CSS px + dpr),驱动 presenter 子窗口
  // 嵌入与共享纹理尺寸协商;卸载时 visible=false 收回原生呈现层。web/测试环境
  // bridge 无 viewport 段,自然 no-op。
  useEffect(() => {
    const report = bridge().viewport?.reportBounds;
    if (!report) return;
    const el = containerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    report({
      x: rect.left,
      y: rect.top,
      w: size.w,
      h: size.h,
      dpr: size.dpr,
      visible: true,
    });
    return () => {
      report({ x: 0, y: 0, w: 0, h: 0, dpr: 1, visible: false });
    };
  }, [size.w, size.h, size.dpr]);

  // 相机首拉
  useEffect(() => {
    void useEditorStore.getState().loadCamera();
  }, []);

  // 帧轮询(选中态/尺寸变化即重建循环;playState 变化经轮询自然反映)
  useEffect(() => {
    // 轮询分辨率 = 物理像素(CSS × dpr):与 G-F1-9 共享纹理尺寸同源,presenter 1:1 呈现
    const physW = clamp(Math.round(size.w * size.dpr), 16, 3840);
    const physH = clamp(Math.round(size.h * size.dpr), 16, 2160);
    let alive = true;
    let timer: number | undefined;
    const tick = async () => {
      try {
        const f = await callTool<FrameResponse>('viewport_frame', {
          width: physW,
          height: physH,
          ...(selectedId != null ? { selectedId } : {}),
        });
        if (!alive) return;
        const canvas = canvasRef.current;
        const ctx = canvas?.getContext('2d');
        if (canvas && ctx) {
          if (canvas.width !== f.width) canvas.width = f.width;
          if (canvas.height !== f.height) canvas.height = f.height;
          const bin = atob(f.pixelsB64);
          const bytes = new Uint8ClampedArray(bin.length);
          for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
          ctx.putImageData(new ImageData(bytes, f.width, f.height), 0, 0);
        }
        useEditorStore.getState().setViewportStatus(null, {
          deviceName: f.deviceName,
          draws: f.draws,
          frames: f.frames,
          nonZeroPixels: f.nonZeroPixels,
          truncated: f.truncated,
        });
      } catch (err) {
        if (!alive) return;
        const msg = err instanceof ForgeApiError ? err.message : (err as Error).message;
        if (msg.includes('DEV_ENV_DEGRADE')) {
          useEditorStore.getState().setViewportStatus(msg, null);
        }
      }
      if (alive) timer = window.setTimeout(tick, POLL_MS);
    };
    void tick();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [size.w, size.h, size.dpr, selectedId, playState]);

  const st = useEditorStore.getState;

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    e.currentTarget.focus();
    drag.current = {
      downX: e.clientX,
      downY: e.clientY,
      x: e.clientX,
      y: e.clientY,
      mode: e.altKey ? 'orbit' : 'candidate',
      gx: 0,
      gy: 0,
    };
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d.mode) return;
    const dx = e.clientX - d.x;
    const dy = e.clientY - d.y;
    if (d.mode === 'candidate') {
      // 超阈值且有选中 → 升级为 gizmo 拖拽;无选中保持 candidate(位移大也只在抬起时点选)
      if (
        Math.hypot(e.clientX - d.downX, e.clientY - d.downY) >= DRAG_THRESHOLD_PX &&
        st().selectedId != null
      ) {
        d.mode = 'gizmo';
      }
    }
    if (d.mode === 'orbit') {
      void st().orbitCamera(dx, dy);
    } else if (d.mode === 'gizmo') {
      d.gx += dx;
      d.gy += dy;
    }
    d.x = e.clientX;
    d.y = e.clientY;
  };

  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    drag.current.mode = null;
    if (d.mode === 'candidate') {
      const rect = containerRef.current?.getBoundingClientRect();
      if (rect) {
        void st().pickAt(e.clientX - rect.left, e.clientY - rect.top, size.w, size.h);
      }
    } else if (d.mode === 'gizmo' && (d.gx !== 0 || d.gy !== 0)) {
      void st().gizmoDragSelected(d.gx, d.gy, size.h);
    }
  };

  return (
    <div
      ref={containerRef}
      className="relative h-full w-full overflow-hidden bg-ink outline-none"
      tabIndex={0}
      role="application"
      aria-label="Viewport 画布"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onWheel={(e) => void st().zoomCamera(e.deltaY)}
      onKeyDown={(e) => {
        if (e.key === 'f' || e.key === 'F') void st().focusSelected();
        if (e.key === 'w' || e.key === 'W') st().setGizmo('translate');
        if (e.key === 'e' || e.key === 'E') st().setGizmo('rotate');
        if (e.key === 'r' || e.key === 'R') st().setGizmo('scale');
      }}
    >
      <canvas ref={canvasRef} className="h-full w-full" style={{ display: degraded ? 'none' : 'block' }} />
      {degraded && (
        <div className="absolute inset-0 flex items-center justify-center">
          <p className="max-w-md text-center text-xs text-white/40">
            Viewport 帧通道降级(如实上报,非伪造帧)
            <br />
            <span className="text-white/55">{degraded}</span>
          </p>
        </div>
      )}
      {/* 右上:帧统计(viewport_frame 实测) */}
      <div className="absolute right-2 top-2 rounded-md bg-black/40 px-2 py-1 font-mono text-2xs text-white/70">
        {info
          ? `${info.deviceName} · draws ${info.draws} · frames ${info.frames} · px ${info.nonZeroPixels}${info.truncated ? ' · 截断!' : ''}`
          : '帧统计待首帧'}
      </div>
      {/* 左下:交互提示 + gizmo 模式 */}
      <div className="absolute bottom-2 left-2 rounded-md bg-black/40 px-2 py-1 text-2xs text-white/50">
        单击点选 · Alt+左键环绕 · 滚轮缩放 · F 聚焦 · {gizmo === 'translate' ? 'W 平移' : gizmo === 'rotate' ? 'E 旋转' : 'R 缩放'}(选中后拖拽)
      </div>
    </div>
  );
}
