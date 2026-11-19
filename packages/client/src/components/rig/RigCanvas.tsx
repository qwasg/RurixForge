import { useEffect, useMemo, useRef, useState } from 'react';
import { useRigStore, displayBones, displayPositions } from '@/lib/rigStore';
import type { RigDoc, RigPart, Vec2 } from '@/lib/rigTypes';
import { bindingToWeights, heatColor, weightOf } from './RigWeightBrush';
import { RigGlRenderer, type GlPart } from './rigGl';

/**
 * 绑骨画布(F-GAME-5 W5):底层 WebGL2 纹理三角形(rigGl.ts;显示顶点 = 静止网格或 rig_evaluate 结果)
 * + 上层 2D 手柄层(网格线框 / 顶点 / 骨骼 / 权重热力图 / 笔刷圈)。
 * - 滚轮缩放(锚点 = 光标)、空格 / 中键拖拽平移(照 SpriteCanvas);
 * - 网格模式:点部件选中,拖顶点改静止网格;
 * - 骨骼模式:拖骨头(默认带子骨骼平移,Alt 单独)/ 拖骨尾(改长度与朝向)/ 空白拖拽新建子骨骼
 *   (父 = 当前选中骨骼);Delete 删骨;
 * - 权重模式:点骨骼(或 Alt+点)切笔刷骨骼,在选中部件上拖拽刷权重;
 * - 参数模式:在求值姿势上拖顶点(暂存为 poseEdits,右栏「记录 keyform」写回);
 * - 动画模式:只读预览(播放头驱动求值)。
 * WebGL2 不可用(jsdom 等)→ 如实提示并降级为线框,编辑交互不受影响。
 */

interface View {
  scale: number;
  ox: number;
  oy: number;
}

type Drag =
  | { kind: 'pan' }
  | { kind: 'vertex'; partId: string; idx: number; pose: boolean }
  | { kind: 'boneHead'; id: string; withChildren: boolean }
  | { kind: 'boneTail'; id: string }
  | { kind: 'newBone'; head: Vec2; parent: string | null; sx: number; sy: number }
  | { kind: 'paint'; partId: string; last: number }
  | { kind: 'click' };

const VERT_HIT_PX = 7;
const BONE_HIT_PX = 7;
const ACC = '#f0a63f';
const CYAN = '#59d0d0';

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

function pointInTri(p: Vec2, a: Vec2, b: Vec2, c: Vec2): boolean {
  const d1 = (p[0] - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (p[1] - b[1]);
  const d2 = (p[0] - c[0]) * (b[1] - c[1]) - (b[0] - c[0]) * (p[1] - c[1]);
  const d3 = (p[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (p[1] - a[1]);
  const neg = d1 < 0 || d2 < 0 || d3 < 0;
  const pos = d1 > 0 || d2 > 0 || d3 > 0;
  return !(neg && pos);
}

function distToSeg(p: Vec2, a: Vec2, b: Vec2): number {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const l2 = dx * dx + dy * dy;
  const t = l2 > 0 ? clamp(((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2, 0, 1) : 0;
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}

interface DisplayPart {
  part: RigPart;
  positions: Vec2[];
  order: number;
  opacity: number;
}

export default function RigCanvas({ img }: { img: HTMLImageElement | null }) {
  const doc = useRigStore((s) => s.doc);
  const mode = useRigStore((s) => s.mode);
  const selection = useRigStore((s) => s.selection);
  const selectedVertex = useRigStore((s) => s.selectedVertex);
  const evaluated = useRigStore((s) => s.evaluated);
  const poseEdits = useRigStore((s) => s.poseEdits);
  const brush = useRigStore((s) => s.brush);
  const assetPath = useRigStore((s) => s.assetPath);
  const dirty = useRigStore((s) => s.dirty);
  const evalError = useRigStore((s) => s.evalError);
  const texDataUrl = useRigStore((s) => s.texDataUrl);
  const st = useRigStore.getState;

  const containerRef = useRef<HTMLDivElement>(null);
  const glCanvasRef = useRef<HTMLCanvasElement>(null);
  const overlayRef = useRef<HTMLCanvasElement>(null);
  const rendererRef = useRef<RigGlRenderer | null>(null);
  const [glOk, setGlOk] = useState<boolean | null>(null);
  const [size, setSize] = useState({ w: 640, h: 400, dpr: 1 });
  const [view, setView] = useState<View>({ scale: 1, ox: 16, oy: 16 });
  const [spaceHeld, setSpaceHeld] = useState(false);
  const [panning, setPanning] = useState(false);
  const [cursor, setCursor] = useState<Vec2 | null>(null);
  const [bonePreview, setBonePreview] = useState<[Vec2, Vec2] | null>(null);
  const drag = useRef<Drag | null>(null);
  const gestureId = useRef(0);
  const fittedFor = useRef<string | null>(null);

  // 容器尺寸测量(ResizeObserver;测试环境有桩)。
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const apply = (w: number, h: number) =>
      setSize({ w: Math.max(64, Math.round(w)), h: Math.max(64, Math.round(h)), dpr: window.devicePixelRatio || 1 });
    const rect = el.getBoundingClientRect();
    apply(rect.width || 640, rect.height || 400);
    const ro = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect;
      if (r) apply(r.width, r.height);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // WebGL2 初始化(一次);失败 → glOk=false 降级。
  useEffect(() => {
    const c = glCanvasRef.current;
    if (!c) return;
    const r = RigGlRenderer.create(c);
    rendererRef.current = r;
    setGlOk(r !== null);
    return () => {
      r?.dispose();
      rendererRef.current = null;
    };
  }, []);

  useEffect(() => {
    rendererRef.current?.setTexture(img);
  }, [img, glOk]);

  // 初次适配:每资产一次把 rig 画布居中放入视野。
  useEffect(() => {
    if (!doc || fittedFor.current === assetPath) return;
    const [cw, ch] = doc.canvas.size;
    if (!(cw > 0 && ch > 0)) return;
    fittedFor.current = assetPath;
    const scale = clamp(Math.min((size.w - 48) / cw, (size.h - 48) / ch), 0.02, 16);
    setView({ scale, ox: (size.w - cw * scale) / 2, oy: (size.h - ch * scale) / 2 });
  }, [doc, assetPath, size.w, size.h]);

  // 空格 = 平移修饰键(文本控件避让)。
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

  // ---------- 显示数据 ----------
  const display: DisplayPart[] = useMemo(() => {
    if (!doc) return [];
    const evalMode = mode === 'params' || mode === 'animate';
    return doc.parts
      .filter((p) => p.visible !== false)
      .map((p) => {
        const ev = evalMode ? evaluated?.parts.find((e) => e.id === p.id) : undefined;
        return {
          part: p,
          positions: displayPositions(p, mode, evaluated, poseEdits),
          order: ev?.drawOrder ?? p.drawOrder,
          opacity: ev?.opacity ?? p.opacity ?? 1,
        };
      })
      .sort((a, b) => a.order - b.order);
  }, [doc, mode, evaluated, poseEdits]);

  const bones = useMemo(() => (doc ? displayBones(doc, mode, evaluated) : []), [doc, mode, evaluated]);

  const selectedPartId = selection?.kind === 'part' ? selection.id : null;
  const selectedBoneId = selection?.kind === 'bone' ? selection.id : null;

  // ---------- 坐标换算 ----------
  const toScreen = (p: Vec2): Vec2 => [p[0] * view.scale + view.ox, p[1] * view.scale + view.oy];
  const toCanvas = (e: React.PointerEvent | React.WheelEvent): { c: Vec2; sx: number; sy: number } => {
    const rect = containerRef.current?.getBoundingClientRect();
    const sx = e.clientX - (rect?.left ?? 0);
    const sy = e.clientY - (rect?.top ?? 0);
    return { c: [(sx - view.ox) / view.scale, (sy - view.oy) / view.scale], sx, sy };
  };

  // ---------- 命中检测 ----------
  const vertexAt = (partId: string, sx: number, sy: number): number | null => {
    const dp = display.find((d) => d.part.id === partId);
    if (!dp) return null;
    let best: number | null = null;
    let bestD = VERT_HIT_PX;
    dp.positions.forEach((p, i) => {
      const [x, y] = toScreen(p);
      const d = Math.hypot(sx - x, sy - y);
      if (d <= bestD) {
        bestD = d;
        best = i;
      }
    });
    return best;
  };

  const partAt = (c: Vec2): string | null => {
    for (let i = display.length - 1; i >= 0; i -= 1) {
      const { part, positions } = display[i];
      for (const [a, b, d] of part.mesh.triangles) {
        if (positions[a] && positions[b] && positions[d] && pointInTri(c, positions[a], positions[b], positions[d])) {
          return part.id;
        }
      }
    }
    return null;
  };

  const boneAt = (sx: number, sy: number): { id: string; end: 'head' | 'tail' | 'body' } | null => {
    const ordered = selectedBoneId
      ? [...bones.filter((b) => b.id === selectedBoneId), ...bones.filter((b) => b.id !== selectedBoneId)]
      : bones;
    for (const b of ordered) {
      const t = toScreen(b.tail);
      if (Math.hypot(sx - t[0], sy - t[1]) <= BONE_HIT_PX && Math.hypot(b.tail[0] - b.head[0], b.tail[1] - b.head[1]) > 0)
        return { id: b.id, end: 'tail' };
      const h = toScreen(b.head);
      if (Math.hypot(sx - h[0], sy - h[1]) <= BONE_HIT_PX) return { id: b.id, end: 'head' };
    }
    for (const b of ordered) {
      if (distToSeg([sx, sy], toScreen(b.head), toScreen(b.tail)) <= 4) return { id: b.id, end: 'body' };
    }
    return null;
  };

  // ---------- 指针交互 ----------
  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!doc) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.currentTarget.focus();
    const { c, sx, sy } = toCanvas(e);
    gestureId.current += 1;
    const coalesce = `rig-gesture-${gestureId.current}`;

    if (e.button === 1 || (e.button === 0 && spaceHeld)) {
      e.preventDefault();
      drag.current = { kind: 'pan' };
      setPanning(true);
      return;
    }
    if (e.button !== 0) return;

    if (mode === 'mesh' || mode === 'params') {
      if (selectedPartId) {
        const vi = vertexAt(selectedPartId, sx, sy);
        if (vi !== null) {
          st().selectVertex(vi);
          drag.current = { kind: 'vertex', partId: selectedPartId, idx: vi, pose: mode === 'params' };
          return;
        }
      }
      const hit = partAt(c);
      st().select(hit ? { kind: 'part', id: hit } : null);
      drag.current = { kind: 'click' };
      return;
    }

    if (mode === 'bones') {
      const hb = boneAt(sx, sy);
      if (hb) {
        st().select({ kind: 'bone', id: hb.id });
        drag.current =
          hb.end === 'tail' ? { kind: 'boneTail', id: hb.id } : { kind: 'boneHead', id: hb.id, withChildren: !e.altKey };
        return;
      }
      drag.current = { kind: 'newBone', head: c, parent: selectedBoneId, sx, sy };
      setBonePreview([c, c]);
      return;
    }

    if (mode === 'weights') {
      const hb = boneAt(sx, sy);
      if (hb || e.altKey) {
        // 只切笔刷骨骼,不改选中(保持热力图显示的部件)。
        if (hb) st().setBrush({ bone: hb.id });
        drag.current = { kind: 'click' };
        return;
      }
      let partId = selectedPartId;
      if (!partId) {
        partId = partAt(c);
        if (partId) st().select({ kind: 'part', id: partId });
      }
      if (!partId) return;
      st().brushAt(partId, c, coalesce);
      drag.current = { kind: 'paint', partId, last: performance.now() };
      return;
    }

    // animate:只读,点选部件 / 骨骼。
    const hb = boneAt(sx, sy);
    if (hb) st().select({ kind: 'bone', id: hb.id });
    else {
      const hit = partAt(c);
      st().select(hit ? { kind: 'part', id: hit } : null);
    }
    drag.current = { kind: 'click' };
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    const { c } = toCanvas(e);
    if (mode === 'weights') setCursor(c);
    if (!d) return;
    const coalesce = `rig-gesture-${gestureId.current}`;
    switch (d.kind) {
      case 'pan':
        setView((v) => ({ ...v, ox: v.ox + e.movementX, oy: v.oy + e.movementY }));
        break;
      case 'vertex':
        if (d.pose) st().setPoseVertex(d.partId, d.idx, c);
        else st().moveVertex(d.partId, d.idx, c, coalesce);
        break;
      case 'boneHead':
        st().moveBoneHead(d.id, c, { withChildren: d.withChildren, coalesce });
        break;
      case 'boneTail':
        st().setBoneTail(d.id, c, coalesce);
        break;
      case 'newBone':
        setBonePreview([d.head, c]);
        break;
      case 'paint': {
        const now = performance.now();
        if (now - d.last >= 24) {
          st().brushAt(d.partId, c, coalesce);
          d.last = now;
        }
        break;
      }
      default:
        break;
    }
  };

  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    drag.current = null;
    setPanning(false);
    if (d?.kind === 'newBone') {
      setBonePreview(null);
      const { c, sx, sy } = toCanvas(e);
      if (Math.hypot(sx - d.sx, sy - d.sy) < 4) st().select(null);
      else st().addBone(d.head, c, d.parent);
    }
  };

  const onWheel = (e: React.WheelEvent<HTMLDivElement>) => {
    const { sx, sy } = toCanvas(e);
    setView((v) => {
      const scale = clamp(v.scale * Math.exp(-e.deltaY * 0.0015), 0.02, 64);
      return { scale, ox: sx - ((sx - v.ox) / v.scale) * scale, oy: sy - ((sy - v.oy) / v.scale) * scale };
    });
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if ((e.key === 'Delete' || e.key === 'Backspace') && mode === 'bones' && selectedBoneId) {
      e.preventDefault();
      st().removeBone(selectedBoneId);
    } else if (e.key === 'Escape') {
      st().select(null);
    }
  };

  // ---------- WebGL 绘制 ----------
  useEffect(() => {
    const r = rendererRef.current;
    if (!r || !doc) return;
    const parts: GlPart[] = display.map((d) => ({
      positions: d.positions,
      uvs: d.part.mesh.uvs,
      triangles: d.part.mesh.triangles,
      opacity: d.opacity,
      blend: d.part.blend ?? 'alpha',
    }));
    try {
      r.draw(parts, doc.canvas.size, view, size);
    } catch {
      // 绘制失败(上下文丢失等)静默:手柄层仍在。
    }
  }, [display, doc, view, size, img, glOk]);

  // ---------- 2D 手柄层 ----------
  useEffect(() => {
    const canvas = overlayRef.current;
    let ctx: CanvasRenderingContext2D | null = null;
    try {
      ctx = canvas?.getContext('2d') ?? null;
    } catch {
      ctx = null;
    }
    if (!canvas || !ctx || !doc) return;
    canvas.width = Math.round(size.w * size.dpr);
    canvas.height = Math.round(size.h * size.dpr);
    ctx.setTransform(size.dpr, 0, 0, size.dpr, 0, 0);
    ctx.clearRect(0, 0, size.w, size.h);
    drawOverlay(ctx, {
      doc,
      display,
      bones,
      view,
      mode,
      selectedPartId,
      selectedBoneId,
      selectedVertex,
      poseEdits,
      brushBone: brush.bone,
      brushRadius: brush.radius,
      cursor,
      bonePreview,
      glOk: glOk === true,
    });
  }, [
    doc,
    display,
    bones,
    view,
    size,
    mode,
    selectedPartId,
    selectedBoneId,
    selectedVertex,
    poseEdits,
    brush.bone,
    brush.radius,
    cursor,
    bonePreview,
    glOk,
  ]);

  const evalMode = mode === 'params' || mode === 'animate';
  const hint: Record<string, string> = {
    mesh: '点部件选中 · 拖顶点移动静止网格',
    bones: '拖骨头平移(带子骨骼,Alt 单独)· 拖骨尾改长度/朝向 · 空白拖拽新建子骨骼 · Delete 删骨',
    weights: '点骨骼切笔刷骨骼 · 在选中部件上拖拽刷权重',
    params: '右栏拖参数滑杆 · 关键值上拖顶点后「记录 keyform」',
    animate: '时间轴拖播放头 · 播放预览',
  };

  return (
    <div
      ref={containerRef}
      data-testid="rig-canvas"
      role="application"
      aria-label="绑骨画布 / Rig canvas"
      tabIndex={0}
      className="relative h-full min-h-0 w-full overflow-hidden outline-none"
      style={{ cursor: panning ? 'grabbing' : spaceHeld ? 'grab' : mode === 'weights' ? 'none' : 'crosshair' }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerLeave={() => setCursor(null)}
      onWheel={onWheel}
      onKeyDown={onKeyDown}
    >
      <canvas ref={glCanvasRef} className="absolute inset-0 h-full w-full" />
      <canvas ref={overlayRef} className="pointer-events-none absolute inset-0 h-full w-full" />
      <div className="pointer-events-none absolute inset-x-0 top-2 flex flex-col items-center gap-1">
        {glOk === false && (
          <span className="rounded-md bg-black/50 px-2 py-0.5 text-2xs text-white/70" data-testid="rig-canvas-nogl">
            WebGL2 不可用,纹理预览已关闭(线框与编辑交互仍可用)· WebGL2 unavailable
          </span>
        )}
        {!texDataUrl && (
          <span className="rounded-md bg-black/50 px-2 py-0.5 text-2xs text-white/60">
            图集未加载(经 asset_thumbnail 直出;失败原因见顶部错误条)
          </span>
        )}
        {evalMode && dirty && (
          <span className="rounded-md bg-black/50 px-2 py-0.5 text-2xs text-warn" data-testid="rig-canvas-stale">
            求值预览基于已保存版本(有未保存改动,保存后刷新)
          </span>
        )}
        {evalMode && evalError && (
          <span className="rounded-md bg-black/50 px-2 py-0.5 text-2xs text-warn" title={evalError}>
            求值失败:{evalError}
          </span>
        )}
      </div>
      <div className="pointer-events-none absolute bottom-1.5 left-1.5 rounded-md bg-black/40 px-2 py-0.5 text-2xs text-white/50">
        滚轮缩放 · 空格/中键平移 · {hint[mode]}
      </div>
    </div>
  );
}

interface OverlayInput {
  doc: RigDoc;
  display: DisplayPart[];
  bones: Array<{ id: string; head: Vec2; tail: Vec2 }>;
  view: View;
  mode: string;
  selectedPartId: string | null;
  selectedBoneId: string | null;
  selectedVertex: number | null;
  poseEdits: Record<string, Record<number, Vec2>>;
  brushBone: string | null;
  brushRadius: number;
  cursor: Vec2 | null;
  bonePreview: [Vec2, Vec2] | null;
  glOk: boolean;
}

function drawOverlay(ctx: CanvasRenderingContext2D, o: OverlayInput): void {
  const { scale, ox, oy } = o.view;
  const S = (p: Vec2): Vec2 => [p[0] * scale + ox, p[1] * scale + oy];
  const [cw, ch] = o.doc.canvas.size;

  // 画布边框 + 原点十字。
  ctx.setLineDash([4, 3]);
  ctx.strokeStyle = 'rgba(255,255,255,0.25)';
  ctx.lineWidth = 1;
  ctx.strokeRect(ox + 0.5, oy + 0.5, cw * scale, ch * scale);
  ctx.setLineDash([]);
  const org = S(o.doc.canvas.origin);
  ctx.strokeStyle = 'rgba(89,208,208,0.7)';
  ctx.beginPath();
  ctx.moveTo(org[0] - 8, org[1]);
  ctx.lineTo(org[0] + 8, org[1]);
  ctx.moveTo(org[0], org[1] - 8);
  ctx.lineTo(org[0], org[1] + 8);
  ctx.stroke();

  const wire = (dp: DisplayPart, color: string, width = 1) => {
    ctx.strokeStyle = color;
    ctx.lineWidth = width;
    ctx.beginPath();
    for (const [a, b, c] of dp.part.mesh.triangles) {
      const pa = dp.positions[a];
      const pb = dp.positions[b];
      const pc = dp.positions[c];
      if (!pa || !pb || !pc) continue;
      const A = S(pa);
      const B = S(pb);
      const C = S(pc);
      ctx.moveTo(A[0], A[1]);
      ctx.lineTo(B[0], B[1]);
      ctx.lineTo(C[0], C[1]);
      ctx.closePath();
    }
    ctx.stroke();
  };

  // 无 WebGL:所有部件半透明填充 + 线框兜底。
  if (!o.glOk) {
    for (const dp of o.display) {
      ctx.fillStyle = 'rgba(160,165,180,0.18)';
      ctx.beginPath();
      for (const [a, b, c] of dp.part.mesh.triangles) {
        const pa = dp.positions[a];
        const pb = dp.positions[b];
        const pc = dp.positions[c];
        if (!pa || !pb || !pc) continue;
        const A = S(pa);
        const B = S(pb);
        const C = S(pc);
        ctx.moveTo(A[0], A[1]);
        ctx.lineTo(B[0], B[1]);
        ctx.lineTo(C[0], C[1]);
        ctx.closePath();
      }
      ctx.fill();
      wire(dp, 'rgba(255,255,255,0.22)');
    }
  }

  const sel = o.display.find((d) => d.part.id === o.selectedPartId) ?? null;

  if (o.mode === 'mesh') {
    for (const dp of o.display) if (dp !== sel) wire(dp, 'rgba(255,255,255,0.12)');
  }

  if (o.mode === 'weights' && sel) {
    const ws = bindingToWeights(sel.part);
    const bone = o.brushBone;
    for (const [a, b, c] of sel.part.mesh.triangles) {
      const pa = sel.positions[a];
      const pb = sel.positions[b];
      const pc = sel.positions[c];
      if (!pa || !pb || !pc) continue;
      const w = bone ? (weightOf(ws[a], bone) + weightOf(ws[b], bone) + weightOf(ws[c], bone)) / 3 : 0;
      ctx.fillStyle = heatColor(w, 0.55);
      const A = S(pa);
      const B = S(pb);
      const C = S(pc);
      ctx.beginPath();
      ctx.moveTo(A[0], A[1]);
      ctx.lineTo(B[0], B[1]);
      ctx.lineTo(C[0], C[1]);
      ctx.closePath();
      ctx.fill();
    }
    wire(sel, 'rgba(255,255,255,0.25)');
    sel.positions.forEach((p, i) => {
      const [x, y] = S(p);
      ctx.fillStyle = heatColor(bone ? weightOf(ws[i], bone) : 0, 1);
      ctx.fillRect(x - 2, y - 2, 4, 4);
    });
  }

  if (sel && (o.mode === 'mesh' || o.mode === 'params' || o.mode === 'animate' || o.mode === 'bones')) {
    wire(sel, o.mode === 'bones' || o.mode === 'animate' ? 'rgba(240,166,63,0.35)' : 'rgba(240,166,63,0.75)');
  }

  if (sel && (o.mode === 'mesh' || o.mode === 'params')) {
    const edits = o.mode === 'params' ? o.poseEdits[sel.part.id] ?? {} : {};
    sel.positions.forEach((p, i) => {
      const [x, y] = S(p);
      const isSel = i === o.selectedVertex;
      const edited = edits[i] !== undefined;
      ctx.fillStyle = isSel ? '#ffffff' : edited ? CYAN : ACC;
      const r = isSel ? 4 : 3;
      ctx.fillRect(x - r, y - r, r * 2, r * 2);
    });
  }

  // 骨骼(骨骼/权重模式醒目,其余淡)。
  const strong = o.mode === 'bones' || o.mode === 'weights';
  for (const b of o.bones) {
    const h = S(b.head);
    const t = S(b.tail);
    const isSel = b.id === o.selectedBoneId || (o.mode === 'weights' && b.id === o.brushBone);
    const color = isSel ? ACC : strong ? 'rgba(220,225,240,0.85)' : 'rgba(220,225,240,0.35)';
    const len = Math.hypot(t[0] - h[0], t[1] - h[1]);
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.lineWidth = isSel ? 2 : 1.5;
    if (len > 1) {
      // 菱形骨:头部加宽,向尾部收尖。
      const nx = (-(t[1] - h[1]) / len) * Math.min(6, len * 0.15);
      const ny = ((t[0] - h[0]) / len) * Math.min(6, len * 0.15);
      const mx = h[0] + (t[0] - h[0]) * 0.18;
      const my = h[1] + (t[1] - h[1]) * 0.18;
      ctx.beginPath();
      ctx.moveTo(h[0], h[1]);
      ctx.lineTo(mx + nx, my + ny);
      ctx.lineTo(t[0], t[1]);
      ctx.lineTo(mx - nx, my - ny);
      ctx.closePath();
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(t[0], t[1], 2.5, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.beginPath();
    ctx.arc(h[0], h[1], isSel ? 4.5 : 3.5, 0, Math.PI * 2);
    ctx.stroke();
    if (strong || isSel) {
      ctx.font = '10px ui-monospace, monospace';
      ctx.fillText(b.id, h[0] + 6, h[1] - 6);
    }
  }

  if (o.bonePreview) {
    const [a, b] = o.bonePreview;
    const A = S(a);
    const B = S(b);
    ctx.setLineDash([4, 3]);
    ctx.strokeStyle = ACC;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(A[0], A[1]);
    ctx.lineTo(B[0], B[1]);
    ctx.stroke();
    ctx.setLineDash([]);
  }

  if (o.mode === 'weights' && o.cursor) {
    const [x, y] = S(o.cursor);
    ctx.strokeStyle = 'rgba(255,255,255,0.8)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(x, y, o.brushRadius * scale, 0, Math.PI * 2);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(x, y, 1.5, 0, Math.PI * 2);
    ctx.stroke();
  }
}
