import { useEffect, useRef, useState } from 'react';
import { callTool, ForgeApiError } from '@/lib/forgeApi';
import { bridge } from '@/lib/bridge';
import { useEditorStore, type ViewportInfo } from '@/lib/editorStore';
import { useAssetStore } from '@/lib/assetStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import {
  mapKeyToInput,
  openViewportStream,
  type StreamFrame,
  type ViewportStreamHandle,
} from '@/lib/viewportStream';

/**
 * Viewport 画布(F1 wave.2 → 直连推流波):
 * - 首选通道 = engine-host WS 直连推流(二进制 RGBA 帧引擎侧主动推送,rAF 绘制,
 *   实测 30-60fps);键盘/相机/选中经同一连接实时回传,play 态游戏真实可玩。
 * - 回退通道 = 既有 viewport_frame MCP 轮询腿(WS 三连败自动切换,恢复即切回);
 *   诚实档不变:无 vulkan 设备时降级原因如实上屏,绝不显示伪造帧。
 * 交互:编辑态 单击点选 / Alt+左键环绕 / 滚轮缩放 / F 聚焦 / WER gizmo 拖拽;
 * play 运行态 方向键/WASD/空格 → 游戏输入,单击 → 带归一化坐标的 pointer 输入
 * (引擎反投影为世界坐标派发 click_x/click_y/click_z + click;PvZ 点格种植/点阳光等)。
 * 工作区切换时流通道按新工作区的 engine-host 重连(帧与输入都落到当前项目)。
 */

interface FrameResponse extends ViewportInfo {
  width: number;
  height: number;
  format: string;
  pixelsB64: string;
}

const POLL_MS = 100;
const DRAG_THRESHOLD_PX = 4;

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** 事件目标是否属于文本输入/交互控件(play 态全局键盘监听须避让) */
function isInteractiveTarget(t: EventTarget | null): boolean {
  return (
    t instanceof HTMLElement &&
    t.closest('button, input, textarea, select, [contenteditable="true"]') != null
  );
}

type DragMode = 'candidate' | 'orbit' | 'gizmo' | 'pan';

export function ViewportCanvas() {
  const containerRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const gridRef = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 960, h: 540, dpr: 1 });
  /** 帧通道:直连流(首选)/ 轮询回退。初始 poll——轮询先行保证冷启动即出帧,
   *  WS 建立(onChannel true)后接管;断流(三连败)自动切回,恢复再接管。 */
  const [channel, setChannel] = useState<'stream' | 'poll'>('poll');

  const selectedId = useEditorStore((s) => s.selectedId);
  const degraded = useEditorStore((s) => s.viewportDegraded);
  const info = useEditorStore((s) => s.viewportInfo);
  const gizmo = useEditorStore((s) => s.gizmo);
  const playState = useEditorStore((s) => s.playState);
  const modelReadback = useEditorStore((s) => s.entities.some((e) => e.components.some((c) => c.type === 'ModelRenderer' && c.enabled)));
  // F-GAME-3:2D 场景模式 → 视口手势切换(平移/正交缩放/网格;禁用环绕)
  const sceneMode = useEditorStore((s) => s.sceneMode);
  const camera = useEditorStore((s) => s.camera);

  // 工作区 = 视口所连 engine-host 的项目作用域;切换即重建流通道(viewport_stream_info 按新根解析)。
  const activeWorkspaceId = useWorkspaceStore((s) => s.activeWorkspaceId);

  const streamRef = useRef<ViewportStreamHandle | null>(null);
  /** 当前流通道所属工作区(与 activeWorkspaceId 不一致 → 关旧连新) */
  const streamWorkspaceRef = useRef<string | null>(null);
  /** 最新推流帧(latest-wins;rAF 每显示刷新至多绘一次) */
  const frameRef = useRef<StreamFrame | null>(null);
  const drawnFrameId = useRef(-1);
  /** 相机脏标(编辑态环绕/缩放本地先行,rAF 合并成至多每帧一条 WS camera 消息) */
  const camDirty = useRef(false);
  /** 服务端帧级 play 标志(输入注入以引擎真相为准,不依赖 store 轮询时差) */
  const playingRef = useRef(false);

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

  /** F8 wave.3:最近一次帧通道错误原因(同因去重,轮询不重复 setState 刷屏) */
  const lastFrameErr = useRef<string | null>(null);

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
      // Standard model rendering currently delivers GPU readback through the stream.
      // Keep the native shared-texture child from covering that canvas with a stale frame.
      visible: !modelReadback,
    });
    return () => {
      report({ x: 0, y: 0, w: 0, h: 0, dpr: 1, visible: false });
    };
  }, [size.w, size.h, size.dpr, modelReadback]);

  // 相机首拉
  useEffect(() => {
    void useEditorStore.getState().loadCamera();
  }, []);

  // 流分辨率:CSS 尺寸封顶 1280×720(不乘 dpr)——面板内视觉无损,canvas 拉伸呈现;
  // 与服务端推流上限一致(遗留 MCP 腿按流尺寸出帧时 base64 响应也稳在 8MB 帧限内)。
  const scale = Math.min(1, 1280 / Math.max(1, size.w));
  const physW = clamp(Math.round(size.w * scale), 16, 1280);
  const physH = clamp(Math.round(size.h * scale), 16, 720);

  // ── 直连流通道:挂载建连,尺寸变化走 resize 消息(服务端有重建防抖);
  //    工作区切换 → 关旧连新(新工作区的 engine-host 是另一个进程/另一条 WS) ──
  useEffect(() => {
    const existing = streamRef.current;
    if (existing && streamWorkspaceRef.current === activeWorkspaceId) {
      existing.resize(physW, physH);
      return;
    }
    if (existing) {
      existing.close();
      streamRef.current = null;
      frameRef.current = null;
      drawnFrameId.current = -1;
      setChannel('poll');
    }
    streamWorkspaceRef.current = activeWorkspaceId;
    streamRef.current = openViewportStream({
      width: physW,
      height: physH,
      maxFps: 60,
      onFrame: (f) => {
        frameRef.current = f;
        playingRef.current = f.playing;
        if (lastFrameErr.current != null) {
          lastFrameErr.current = null;
          useEditorStore.getState().setViewportStatus(null, null);
        }
      },
      onStatus: (s) => {
        useEditorStore.getState().setViewportStatus(null, {
          deviceName: s.deviceName,
          draws: s.draws,
          truncated: s.truncated,
          fps: s.fps,
          channel: 'stream',
        });
      },
      onError: (msg) => {
        // 服务端渲染降级(DEV_ENV_DEGRADE 等):如实上屏,禁止黑屏充绿(G-F8-3)。
        if (msg !== lastFrameErr.current) {
          lastFrameErr.current = msg;
          useEditorStore.getState().setViewportStatus(msg, null);
        }
      },
      onChannel: (isUp) => {
        setChannel(isUp ? 'stream' : 'poll');
      },
    });
    // 卸载清理在独立 effect(本 effect 随尺寸/工作区重跑,不能顺手关连接)。
  }, [physW, physH, activeWorkspaceId]);

  useEffect(
    () => () => {
      streamRef.current?.close();
      streamRef.current = null;
    },
    [],
  );

  // 选中高亮跟随(流腿由服务端渲染选中描色;轮询腿经参数传递)
  useEffect(() => {
    streamRef.current?.setSelected(selectedId);
  }, [selectedId]);

  // ── rAF 绘制环:有新帧才 putImageData(每显示刷新至多一次);顺带合并发送相机 ──
  useEffect(() => {
    let alive = true;
    let raf = 0;
    const draw = () => {
      if (!alive) return;
      const f = frameRef.current;
      if (f && f.frameId !== drawnFrameId.current) {
        drawnFrameId.current = f.frameId;
        const canvas = canvasRef.current;
        const ctx = canvas?.getContext('2d');
        if (canvas && ctx) {
          if (canvas.width !== f.width) canvas.width = f.width;
          if (canvas.height !== f.height) canvas.height = f.height;
          ctx.putImageData(new ImageData(f.rgba, f.width, f.height), 0, 0);
        }
      }
      const h = streamRef.current;
      if (camDirty.current && h?.up) {
        camDirty.current = false;
        const c = useEditorStore.getState().camera;
        if (c) {
          // F-GAME-3:ortho/orthoSize 随相机下发(服务端 viewport.setCamera 同字段接收)
          h.sendCamera({
            target: c.target,
            yaw: c.yaw,
            pitch: c.pitch,
            dist: c.dist,
            fovY: c.fovY,
            ortho: c.ortho,
            orthoSize: c.orthoSize,
          });
        }
      }
      raf = window.requestAnimationFrame(draw);
    };
    raf = window.requestAnimationFrame(draw);
    return () => {
      alive = false;
      window.cancelAnimationFrame(raf);
    };
  }, []);

  // ── play 运行态全局键盘 → 游戏输入(方向键/WASD/空格;keyup 发 value 0) ──
  // window 级监听:游玩无须先点视口聚焦;文本框/按钮等交互控件避让。
  // WS 直连优先(毫秒级);流断走 HTTP 注入兜底(丢 auto-repeat 限流,可玩性降档不失能)。
  useEffect(() => {
    if (playState !== 'play_running') return;
    const held = new Map<string, string>();
    const send = (action: string, value: number, repeat: boolean) => {
      const h = streamRef.current;
      if (h?.up) {
        h.sendInput(action, value);
      } else if (!repeat) {
        void callTool('logic_inject_input', { action, value }).catch(() => {
          // play 已退出等时序竞态:输入丢弃即可,不上屏刷错
        });
      }
    };
    const down = (e: KeyboardEvent) => {
      if (isInteractiveTarget(e.target)) return;
      const m = mapKeyToInput(e.key);
      if (!m) return;
      e.preventDefault();
      held.set(e.code || e.key, m.action);
      send(m.action, m.value, e.repeat);
    };
    const up = (e: KeyboardEvent) => {
      const m = mapKeyToInput(e.key);
      if (!m) return;
      const key = e.code || e.key;
      if (!held.has(key) && isInteractiveTarget(e.target)) return;
      held.delete(key);
      // W and ArrowUp can both hold the same action; releasing one must not stop the other.
      if ([...held.values()].includes(m.action)) return;
      e.preventDefault();
      send(m.action, 0, false);
    };
    const release = () => {
      for (const action of new Set(held.values())) send(action, 0, false);
      held.clear();
    };
    const visibility = () => { if (document.hidden) release(); };
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    window.addEventListener('blur', release);
    document.addEventListener('visibilitychange', visibility);
    return () => {
      release();
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
      window.removeEventListener('blur', release);
      document.removeEventListener('visibilitychange', visibility);
    };
  }, [playState]);

  // ── 轮询回退腿(流通道断时的既有 100ms viewport_frame 路径,原样保留) ──
  useEffect(() => {
    if (channel !== 'poll') return;
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
          // base64 → 字节走 data-URL fetch(浏览器原生解码,比 JS charCodeAt 循环
          // 快一个量级;大帧主线程不再长阻塞)。
          const blob = await (
            await fetch(`data:application/octet-stream;base64,${f.pixelsB64}`)
          ).arrayBuffer();
          ctx.putImageData(
            new ImageData(new Uint8ClampedArray(blob), f.width, f.height),
            0,
            0,
          );
        }
        lastFrameErr.current = null;
        useEditorStore.getState().setViewportStatus(null, {
          deviceName: f.deviceName,
          draws: f.draws,
          frames: f.frames,
          nonZeroPixels: f.nonZeroPixels,
          truncated: f.truncated,
          channel: 'poll',
        });
      } catch (err) {
        if (!alive) return;
        // F8 wave.3 浏览器回退腿诚实化:任何帧通道错误(DEV_ENV_DEGRADE / NETWORK /
        // UPSTREAM_UNREACHABLE / TOOL_ERROR …)都必须上屏降级原因——禁止黑屏充绿(G-F8-3)。
        const msg =
          err instanceof ForgeApiError
            ? err.message
            : err instanceof Error
              ? err.message
              : String(err);
        if (msg !== lastFrameErr.current) {
          lastFrameErr.current = msg;
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
  }, [channel, physW, physH, selectedId, playState]);

  const st = useEditorStore.getState;
  const isPlaying = playState === 'play_running';

  // ── F-GAME-3:2D 网格叠加层(编辑态 + 2D 场景 + 正交相机;客户端 canvas 叠加,
  // 不动引擎 pass 图)。世界→屏幕:yaw0/pitch0 正交下 ndc=(w-target)/half,屏外 y 翻转。 ──
  const showGrid = sceneMode === '2d' && !isPlaying && !degraded && camera?.ortho === true;
  useEffect(() => {
    const canvas = gridRef.current;
    if (!canvas || !showGrid || !camera) return;
    const w = size.w;
    const h = size.h;
    const dpr = size.dpr;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, w, h);
    const halfH = Math.max(camera.orthoSize, 1e-4);
    const halfW = halfH * (w / Math.max(h, 1));
    // 自适应步长:线间距 ≥ 40 CSS px(0.1/0.25/0.5/1/2/5… 档位)。
    const wpp = (2 * halfH) / h;
    const steps = [0.1, 0.25, 0.5, 1, 2, 5, 10, 25, 50, 100];
    const step = steps.find((s) => s / wpp >= 40) ?? 100;
    const toSx = (wx: number) => (((wx - camera.target[0]) / halfW) * 0.5 + 0.5) * w;
    const toSy = (wy: number) => (1 - ((wy - camera.target[1]) / halfH) * 0.5 - 0.5) * h;
    const x0 = camera.target[0] - halfW;
    const x1 = camera.target[0] + halfW;
    const y0 = camera.target[1] - halfH;
    const y1 = camera.target[1] + halfH;
    ctx.lineWidth = 1;
    for (let gx = Math.ceil(x0 / step) * step; gx <= x1 + 1e-9; gx += step) {
      const sx = Math.round(toSx(gx)) + 0.5;
      ctx.strokeStyle = Math.abs(gx) < 1e-6 ? 'rgba(255,120,120,0.55)' : 'rgba(255,255,255,0.08)';
      ctx.beginPath();
      ctx.moveTo(sx, 0);
      ctx.lineTo(sx, h);
      ctx.stroke();
    }
    for (let gy = Math.ceil(y0 / step) * step; gy <= y1 + 1e-9; gy += step) {
      const sy = Math.round(toSy(gy)) + 0.5;
      ctx.strokeStyle = Math.abs(gy) < 1e-6 ? 'rgba(120,255,120,0.55)' : 'rgba(255,255,255,0.08)';
      ctx.beginPath();
      ctx.moveTo(0, sy);
      ctx.lineTo(w, sy);
      ctx.stroke();
    }
  }, [showGrid, camera, size.w, size.h, size.dpr]);

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    const is2d = sceneMode === '2d';
    // 中键 = 平移(两种模式通用;2D 编辑器惯例)。
    if (e.button === 1 && !isPlaying) {
      e.preventDefault();
      e.currentTarget.setPointerCapture(e.pointerId);
      e.currentTarget.focus();
      drag.current = { downX: e.clientX, downY: e.clientY, x: e.clientX, y: e.clientY, mode: 'pan', gx: 0, gy: 0 };
      return;
    }
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    e.currentTarget.focus();
    // F-GAME-3:2D 模式 Alt+左键 = 平移(不再环绕);3D 模式 Alt+左键 = 环绕(既有)。
    const mode: DragMode =
      !isPlaying && e.altKey ? (is2d ? 'pan' : 'orbit') : 'candidate';
    drag.current = {
      downX: e.clientX,
      downY: e.clientY,
      x: e.clientX,
      y: e.clientY,
      mode,
      gx: 0,
      gy: 0,
    };
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d.mode) return;
    const dx = e.clientX - d.x;
    const dy = e.clientY - d.y;
    if (d.mode === 'candidate' && !isPlaying) {
      // 超阈值且有选中 → 升级为 gizmo 拖拽;2D 模式无选中拖拽空白 → 平移视野;
      // 3D 无选中保持 candidate(位移大也只在抬起时点选)
      const over = Math.hypot(e.clientX - d.downX, e.clientY - d.downY) >= DRAG_THRESHOLD_PX;
      if (over && st().selectedId != null) {
        d.mode = 'gizmo';
      } else if (over && sceneMode === '2d') {
        d.mode = 'pan';
      }
    }
    if (d.mode === 'orbit') {
      // 直连流:本地先行 + rAF 合并 WS 发送(替代逐 pointermove 一次 HTTP 往返);
      // 流断:回退既有 HTTP 路径。
      const h = streamRef.current;
      const c = st().camera;
      if (h?.up && c) {
        st().setCameraLocal({ yaw: c.yaw - dx * 0.35, pitch: c.pitch + dy * 0.35 });
        camDirty.current = true;
      } else {
        void st().orbitCamera(dx, dy);
      }
    } else if (d.mode === 'pan') {
      // F-GAME-3:平移(2D 主手势;本地先行/HTTP 回退与 orbit 同构)
      const h = streamRef.current;
      if (h?.up) {
        st().panCameraLocal(dx, dy, size.h);
        camDirty.current = true;
      } else {
        void st().panCamera(dx, dy, size.h);
      }
    } else if (d.mode === 'gizmo') {
      d.gx += dx;
      d.gy += dy;
    }
    d.x = e.clientX;
    d.y = e.clientY;
  };

  const onPointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    // 先取再清:d 与 drag.current 同引用,先置 null 会把所有抬起分支判死
    // (历史别名 bug——鼠标点选/click 输入此前从未走到过)。
    const mode = d.mode;
    drag.current.mode = null;
    if (mode === 'candidate') {
      const rect = containerRef.current?.getBoundingClientRect();
      // play 运行态:单击 = 带归一化坐标的 pointer 输入。引擎按游戏相机反投影到游戏平面,
      // 依次派发 click_x/click_y/click_z(世界坐标)+ click(=1):点格种植、点阳光收集等
      // 位置语义在图侧成立;只认 click 正值的旧图行为不变。
      // 引擎真相 playingRef 与 store 任一为运行即注入(agent 拉起 play 时 store 有时差)。
      if (isPlaying || playingRef.current) {
        const nx = rect ? clamp((e.clientX - rect.left) / Math.max(rect.width, 1), 0, 1) : 0.5;
        const ny = rect ? clamp((e.clientY - rect.top) / Math.max(rect.height, 1), 0, 1) : 0.5;
        const h = streamRef.current;
        if (h?.up) h.sendPointer('click', nx, ny);
        else
          void callTool('logic_inject_pointer', {
            action: 'click',
            x: nx,
            y: ny,
            width: physW,
            height: physH,
          }).catch(() => {});
        return;
      }
      if (rect) {
        void st().pickAt(e.clientX - rect.left, e.clientY - rect.top, size.w, size.h);
      }
    } else if (mode === 'gizmo' && (d.gx !== 0 || d.gy !== 0)) {
      // F-GAME-3:Ctrl 按住 = 临时禁用 2D 网格吸附(07 §2 规范)
      void st().gizmoDragSelected(d.gx, d.gy, size.h, !e.ctrlKey);
    }
  };

  const onWheel = (e: React.WheelEvent<HTMLDivElement>) => {
    if (isPlaying) return; // play 态滚轮不动编辑器相机(PIE 视角属于场景相机)
    const h = streamRef.current;
    const c = st().camera;
    if (h?.up && c) {
      // F-GAME-3:正交缩放调 orthoSize;透视沿视轴 dolly
      st().setCameraLocal(
        c.ortho
          ? { orthoSize: c.orthoSize * Math.pow(1.0015, e.deltaY) }
          : { dist: c.dist * Math.pow(1.0015, e.deltaY) },
      );
      camDirty.current = true;
    } else {
      void st().zoomCamera(e.deltaY);
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
      onWheel={onWheel}
      onDragOver={(e) => {
        e.preventDefault(); // 允许 drop
        e.dataTransfer.dropEffect = 'copy';
      }}
      onDrop={(e) => {
        e.preventDefault();
        const guid = e.dataTransfer.getData('forge/asset-guid');
        const atype = e.dataTransfer.getData('forge/asset-type');
        if (!guid || (atype !== 'mesh' && atype !== 'model' && atype !== 'prefab')) return;
        // 实例化到相机目标点前方 2m(或原点)。
        const cam = st().camera;
        const pos: [number, number, number] = cam
          ? [cam.target[0], cam.target[1], cam.target[2]]
          : [0, 0, 0];
        void useAssetStore.getState().instantiate(guid, pos).then(() => st().loadEntities());
      }}
      onKeyDown={(e) => {
        if (isPlaying) return; // play 运行态键盘归游戏输入(window 级监听),编辑器快捷键避让
        if (e.key === 'f' || e.key === 'F') void st().focusSelected();
        if (e.key === 'w' || e.key === 'W') st().setGizmo('translate');
        if (e.key === 'e' || e.key === 'E') st().setGizmo('rotate');
        if (e.key === 'r' || e.key === 'R') st().setGizmo('scale');
      }}
    >
      <canvas ref={canvasRef} className="h-full w-full" style={{ display: degraded ? 'none' : 'block' }} />
      {/* F-GAME-3:2D 网格叠加(编辑态 2D 场景;不拦截指针) */}
      {showGrid && (
        <canvas
          ref={gridRef}
          className="pointer-events-none absolute inset-0 h-full w-full"
          aria-hidden
        />
      )}
      {degraded && (
        <div className="absolute inset-0 flex items-center justify-center">
          <p className="max-w-md text-center text-xs text-white/40">
            Viewport 帧通道降级(如实上报,非伪造帧)
            <br />
            <span className="text-white/55">{degraded}</span>
          </p>
        </div>
      )}
      {/* 右上:帧统计(直连流 = 服务端 1Hz status;轮询回退 = viewport_frame 实测) */}
      <div className="absolute right-2 top-2 rounded-md bg-black/40 px-2 py-1 font-mono text-2xs text-white/70">
        {info
          ? `${info.deviceName} · draws ${info.draws}${info.fps != null ? ` · ${info.fps}fps` : ''}${info.nonZeroPixels != null ? ` · px ${info.nonZeroPixels}` : ''}${info.truncated ? ' · 截断!' : ''} · ${(info.channel ?? channel) === 'stream' ? '直连流' : '轮询回退'}`
          : '帧统计待首帧'}
      </div>
      {/* 左下:交互提示(编辑 = gizmo 操作;play 运行 = 游戏输入契约;2D/3D 手势分叉) */}
      <div className="absolute bottom-2 left-2 rounded-md bg-black/40 px-2 py-1 text-2xs text-white/50">
        {isPlaying
          ? '方向键/WASD 移动 · 空格 动作 · 单击 = 带坐标的 click 输入'
          : sceneMode === '2d'
            ? `2D · 单击点选 · 左键/中键拖拽空白平移 · 滚轮缩放 · F 聚焦 · ${gizmo === 'translate' ? 'W 平移(0.5 吸附,Ctrl 禁用)' : gizmo === 'rotate' ? 'E 旋转(绕 Z)' : 'R 缩放'}`
            : `单击点选 · Alt+左键环绕 · 中键平移 · 滚轮缩放 · F 聚焦 · ${gizmo === 'translate' ? 'W 平移' : gizmo === 'rotate' ? 'E 旋转' : 'R 缩放'}(选中后拖拽)`}
      </div>
    </div>
  );
}
