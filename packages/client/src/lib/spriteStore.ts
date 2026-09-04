import { create } from 'zustand';
import { callAssetTool } from './forgeApi';
import { useAssetStore } from './assetStore';
import { detectSpriteBoxes, type SpriteBox } from './spriteAutoDetect';

/**
 * spriteStore(F-GAME-4):精灵编辑器状态——.rxsprite 文档本地可变副本 + 编辑动作 +
 * 保存/撤销 + clip 预览播放态。内建 tab 无 payload 单例,「当前编辑哪个 sprite」放这里。
 * 数据面:mcp__asset-pipeline__ sprite_get/sprite_set/sprite_create/sprite_autoslice +
 * asset_thumbnail(贴图原图 base64 直出);工具级错误 {error,message} 如实进 error 态不吞。
 */

/** 帧定义:图集内紧致 bbox([x,y,w,h] 像素)+ 可选帧级 pivot 覆盖(0..1,y 向下)。 */
export interface SpriteFrame {
  bbox: [number, number, number, number];
  pivot?: [number, number];
}

/** 动画 clip:帧名序列 + 时长(duration 总秒数优先于 fps)+ 循环/收尾行为。 */
export interface SpriteClip {
  frames: string[];
  fps: number;
  duration?: number;
  loop: boolean;
  onFinish: 'hold' | 'first';
}

/** .rxsprite 文档(与 assetd SpriteDoc 序列化形态对齐;animator 以 JSON 文本域编辑,保持宽型)。 */
export interface SpriteDoc {
  version: number;
  texture: string;
  pivot: [number, number];
  frames: Record<string, SpriteFrame>;
  clips: Record<string, SpriteClip>;
  animator?: Record<string, unknown>;
}

/** 单帧时长秒(duration 优先;fps 兜底,钳制防除零;与 SpriteClip::frame_duration 同规则)。 */
export function clipFrameDuration(clip: SpriteClip): number {
  if (clip.duration != null && clip.duration > 0 && clip.frames.length > 0) {
    return clip.duration / clip.frames.length;
  }
  return 1 / Math.max(clip.fps, 0.0001);
}

/** pivot 级联解析:帧级 > 文档级(与 SpriteDoc::resolve_pivot 同规则)。 */
export function resolvePivot(doc: SpriteDoc, frameName: string): [number, number] {
  return doc.frames[frameName]?.pivot ?? doc.pivot;
}

/** bbox 列表 → frames 映射(命名 frame_<i>;>10 个补零到 2 位,与 frames_from_boxes 同规则)。 */
export function framesFromBoxes(boxes: SpriteBox[]): Record<string, SpriteFrame> {
  const pad = boxes.length > 10 ? 2 : 1;
  const out: Record<string, SpriteFrame> = {};
  boxes.forEach((b, i) => {
    out[`frame_${String(i).padStart(pad, '0')}`] = { bbox: [b[0], b[1], b[2], b[3]] };
  });
  return out;
}

/** 键字典序重排(后端 BTreeMap 写出即字典序;本地保持同序,存前存后 UI 顺序稳定)。 */
function sortKeys<T>(rec: Record<string, T>): Record<string, T> {
  const out: Record<string, T> = {};
  for (const k of Object.keys(rec).sort()) out[k] = rec[k];
  return out;
}

function cloneDoc(doc: SpriteDoc): SpriteDoc {
  return JSON.parse(JSON.stringify(doc)) as SpriteDoc;
}

function clamp01(v: number): number {
  return Math.min(1, Math.max(0, v));
}

/** pivot 分量钳制 0..1 + 4 位小数。 */
export function normPivot(p: [number, number]): [number, number] {
  return [Math.round(clamp01(p[0]) * 10000) / 10000, Math.round(clamp01(p[1]) * 10000) / 10000];
}

const UNDO_MAX = 50;

/** 工具级错误信封(asset-pipeline 以 200 + {error,message} 返回,isError 恒 false,须自查)。 */
interface ToolErr {
  error?: string;
  message?: string;
}

interface SpriteState {
  /** 当前编辑资产(null = 未打开)。 */
  assetPath: string | null;
  guid: string | null;
  /** 本地可变副本(sprite_get 规范形态;save 前的编辑真相)。 */
  doc: SpriteDoc | null;
  /** 上次保存/打开时的序列化快照(dirty 判定基线)。 */
  savedJson: string | null;
  dirty: boolean;
  loading: boolean;
  saving: boolean;
  /** 打开/保存/切帧错误(message 如实展示不吞)。 */
  error: string | null;

  /** 贴图显示与像素面(asset_thumbnail 原图直出;像素由画布解码后回填,本地检测用)。 */
  texPath: string | null;
  texDataUrl: string | null;
  texW: number;
  texH: number;
  texPixels: Uint8ClampedArray | null;

  selectedFrame: string | null;
  selectedClip: string | null;

  /** 撤销栈:doc JSON 快照(上限 50;连续手势/同域输入按 coalesce 键合并)。 */
  undoStack: string[];
  undoCoalesce: string | null;

  /** animator JSON 文本域草稿(实时 parse 校验;坏 JSON 禁存)。 */
  animatorText: string;
  animatorError: string | null;

  /** 预览播放态(rAF 驱动;duration/帧数 优先,否则 1/fps)。 */
  playing: boolean;
  frameIdx: number;
  elapsed: number;

  openSprite: (assetPath: string) => Promise<void>;
  openFromTexture: (texturePath: string, textureGuid: string) => Promise<void>;
  /** 画布解码贴图后回填像素(jsdom 无解码面时不回填,本地检测按钮如实禁用)。 */
  setTextureBitmap: (w: number, h: number, pixels: Uint8ClampedArray) => void;

  /** 前端本地 flood fill 即时切帧(色键同规则;贴图像素未就绪时如实报错)。 */
  autoDetect: (minArea?: number) => void;
  /** 服务端切帧(sprite_autoslice;boxes 重建 frames)。 */
  serverAutoslice: (minArea?: number) => Promise<void>;

  addFrame: (bbox: [number, number, number, number]) => string | null;
  renameFrame: (oldName: string, newName: string) => void;
  setFrameBbox: (
    name: string,
    bbox: [number, number, number, number],
    opts?: { coalesce?: string },
  ) => void;
  setFramePivot: (
    name: string,
    pivot: [number, number] | null,
    opts?: { coalesce?: string },
  ) => void;
  deleteFrame: (name: string) => void;
  setDocPivot: (pivot: [number, number], opts?: { coalesce?: string }) => void;
  selectFrame: (name: string | null) => void;

  addClip: (name: string) => void;
  renameClip: (oldName: string, newName: string) => void;
  deleteClip: (name: string) => void;
  clipAddFrame: (clipName: string, frameName: string) => void;
  clipRemoveFrame: (clipName: string, idx: number) => void;
  clipMoveFrame: (clipName: string, idx: number, dir: -1 | 1) => void;
  setClipMeta: (
    clipName: string,
    patch: { fps?: number; duration?: number | null; loop?: boolean; onFinish?: 'hold' | 'first' },
  ) => void;
  selectClip: (name: string | null) => void;

  setAnimatorText: (text: string) => void;

  save: () => Promise<void>;
  undo: () => void;

  play: () => void;
  pause: () => void;
  tickPreview: (dtSec: number) => void;
  seekPreview: (idx: number) => void;

  reset: () => void;
}

/** 打开新资产前的干净基态(编辑区字段;不含动作)。 */
const FRESH = {
  assetPath: null as string | null,
  guid: null as string | null,
  doc: null as SpriteDoc | null,
  savedJson: null as string | null,
  dirty: false,
  loading: false,
  saving: false,
  error: null as string | null,
  texPath: null as string | null,
  texDataUrl: null as string | null,
  texW: 0,
  texH: 0,
  texPixels: null as Uint8ClampedArray | null,
  selectedFrame: null as string | null,
  selectedClip: null as string | null,
  undoStack: [] as string[],
  undoCoalesce: null as string | null,
  animatorText: '',
  animatorError: null as string | null,
  playing: false,
  frameIdx: 0,
  elapsed: 0,
};

export const useSpriteStore = create<SpriteState>((set, get) => {
  /**
   * 变更文档:先推撤销快照(coalesce 键与上次相同时合并,拖拽手势/连续输入一档一条),
   * 再应用变更并重算 dirty。选中帧/clip 失效时自动回落。
   */
  const mutate = (fn: (doc: SpriteDoc) => void, opts: { coalesce?: string } = {}) => {
    const { doc, undoStack, undoCoalesce, savedJson, selectedFrame, selectedClip } = get();
    if (!doc) return;
    const key = opts.coalesce ?? null;
    const stack =
      key !== null && key === undoCoalesce
        ? undoStack
        : [...undoStack, JSON.stringify(doc)].slice(-UNDO_MAX);
    const next = cloneDoc(doc);
    fn(next);
    const json = JSON.stringify(next);
    set({
      doc: next,
      undoStack: stack,
      undoCoalesce: key,
      dirty: json !== savedJson,
      // 选中项仅在失效时回落(帧回落到首帧,clip 回落到未选);主动未选保持未选。
      selectedFrame:
        selectedFrame === null
          ? null
          : next.frames[selectedFrame]
            ? selectedFrame
            : (Object.keys(next.frames)[0] ?? null),
      selectedClip: selectedClip && next.clips[selectedClip] ? selectedClip : null,
    });
  };

  /** boxes 重建 frames(命名 frame_<i>)+ 清掉 clip 里悬空的帧引用(clip 本身保留,如实可见)。 */
  const applyBoxes = (boxes: SpriteBox[]) => {
    mutate((doc) => {
      doc.frames = framesFromBoxes(boxes);
      for (const clip of Object.values(doc.clips)) {
        clip.frames = clip.frames.filter((f) => doc.frames[f] !== undefined);
      }
    });
    set({ frameIdx: 0, elapsed: 0, playing: false });
  };

  /** 贴图 GUID → 资产路径(经 assetStore 列表;未加载先拉)。 */
  const resolveTexPath = async (textureGuid: string): Promise<string | null> => {
    const assets = useAssetStore.getState();
    if (assets.items.length === 0) await assets.load();
    return useAssetStore.getState().items.find((i) => i.guid === textureGuid)?.path ?? null;
  };

  return {
    ...FRESH,

    openSprite: async (assetPath) => {
      set({ ...FRESH, assetPath, loading: true });
      try {
        const r = await callAssetTool<ToolErr & { guid?: string; doc?: SpriteDoc }>('sprite_get', {
          assetPath,
        });
        if (r.error || !r.doc) {
          set({ loading: false, error: r.message ?? r.error ?? 'sprite_get 空响应' });
          return;
        }
        const doc = cloneDoc(r.doc);
        doc.frames = sortKeys(doc.frames ?? {});
        doc.clips = sortKeys(doc.clips ?? {});
        const json = JSON.stringify(doc);
        set({
          guid: r.guid ?? null,
          doc,
          savedJson: json,
          dirty: false,
          selectedFrame: Object.keys(doc.frames)[0] ?? null,
          selectedClip: Object.keys(doc.clips)[0] ?? null,
          animatorText: doc.animator ? JSON.stringify(doc.animator, null, 2) : '',
          animatorError: null,
        });
        // 贴图 GUID → 路径 → 原图 dataUrl(解析不到/取图失败 = 降级可编辑,错误如实上屏)。
        const texPath = await resolveTexPath(doc.texture);
        if (texPath === null) {
          set({ loading: false, error: `贴图 GUID 未在资产列表中解析到路径:${doc.texture}` });
          return;
        }
        set({ texPath });
        try {
          const t = await callAssetTool<ToolErr & { dataUrl?: string }>('asset_thumbnail', {
            assetPath: texPath,
          });
          if (t.error || !t.dataUrl) {
            set({ loading: false, error: `贴图加载失败:${t.message ?? t.error ?? '空响应'}` });
            return;
          }
          set({ texDataUrl: t.dataUrl, loading: false });
        } catch (err) {
          set({ loading: false, error: `贴图加载失败:${(err as Error).message}` });
        }
      } catch (err) {
        set({ loading: false, error: (err as Error).message });
      }
    },

    openFromTexture: async (texturePath, textureGuid) => {
      set({ ...FRESH, loading: true });
      // name = 贴图文件名去扩展名;重名时后端复用 GUID(reimport 语义)属预期。
      const base = texturePath.replace(/\\/g, '/').split('/').pop() ?? texturePath;
      const name = base.replace(/\.[^.]+$/, '');
      try {
        const r = await callAssetTool<ToolErr & { assetPath?: string }>('sprite_create', {
          name,
          texture: textureGuid,
          autoslice: false,
        });
        if (r.error || !r.assetPath) {
          set({ loading: false, error: r.message ?? r.error ?? 'sprite_create 空响应' });
          return;
        }
        // 新资产进列表(GUID→路径解析、资产面板同步)后再打开。
        await useAssetStore.getState().load();
        await get().openSprite(r.assetPath);
      } catch (err) {
        set({ loading: false, error: (err as Error).message });
      }
    },

    setTextureBitmap: (w, h, pixels) => {
      set({ texW: w, texH: h, texPixels: pixels });
    },

    autoDetect: (minArea = 16) => {
      const { texPixels, texW, texH } = get();
      if (!texPixels || texW === 0 || texH === 0) {
        set({ error: '贴图像素未就绪(等待解码或环境不支持),本地检测不可用;可用「服务端切帧」' });
        return;
      }
      try {
        applyBoxes(detectSpriteBoxes(texW, texH, texPixels, { minArea }));
        set({ error: null });
      } catch (err) {
        set({ error: (err as Error).message });
      }
    },

    serverAutoslice: async (minArea = 16) => {
      const { texPath } = get();
      if (!texPath) {
        set({ error: '贴图路径未解析,无法调用服务端切帧' });
        return;
      }
      try {
        const r = await callAssetTool<ToolErr & { width?: number; height?: number; boxes?: SpriteBox[] }>(
          'sprite_autoslice',
          { assetPath: texPath, minArea },
        );
        if (r.error || !r.boxes) {
          set({ error: r.message ?? r.error ?? 'sprite_autoslice 空响应' });
          return;
        }
        // 顺带补贴图尺寸(jsdom/解码失败场景也能拿到画布坐标系)。
        if (r.width && r.height && get().texW === 0) set({ texW: r.width, texH: r.height });
        applyBoxes(r.boxes);
        set({ error: null });
      } catch (err) {
        set({ error: (err as Error).message });
      }
    },

    addFrame: (bbox) => {
      const { doc } = get();
      if (!doc) return null;
      let n = 0;
      while (doc.frames[`frame_${n}`] !== undefined) n += 1;
      const name = `frame_${n}`;
      mutate((d) => {
        d.frames = sortKeys({ ...d.frames, [name]: { bbox } });
      });
      set({ selectedFrame: name });
      return name;
    },

    renameFrame: (oldName, newName) => {
      const { doc } = get();
      const trimmed = newName.trim();
      if (!doc || trimmed === '' || trimmed === oldName) return;
      if (doc.frames[trimmed] !== undefined) {
        set({ error: `帧名已存在:${trimmed}` });
        return;
      }
      mutate((d) => {
        const f = d.frames[oldName];
        if (!f) return;
        delete d.frames[oldName];
        d.frames = sortKeys({ ...d.frames, [trimmed]: f });
        // clip 引用同步改名。
        for (const clip of Object.values(d.clips)) {
          clip.frames = clip.frames.map((fn) => (fn === oldName ? trimmed : fn));
        }
      });
      set({ selectedFrame: trimmed, error: null });
    },

    setFrameBbox: (name, bbox, opts) => {
      mutate((d) => {
        const f = d.frames[name];
        if (!f) return;
        f.bbox = [
          Math.max(0, Math.round(bbox[0])),
          Math.max(0, Math.round(bbox[1])),
          Math.max(1, Math.round(bbox[2])),
          Math.max(1, Math.round(bbox[3])),
        ];
      }, opts);
    },

    setFramePivot: (name, pivot, opts) => {
      mutate((d) => {
        const f = d.frames[name];
        if (!f) return;
        if (pivot === null) delete f.pivot;
        else f.pivot = normPivot(pivot);
      }, opts);
    },

    deleteFrame: (name) => {
      mutate((d) => {
        delete d.frames[name];
        for (const clip of Object.values(d.clips)) {
          clip.frames = clip.frames.filter((f) => f !== name);
        }
      });
      set({ frameIdx: 0, elapsed: 0, playing: false });
    },

    setDocPivot: (pivot, opts) => {
      mutate((d) => {
        d.pivot = normPivot(pivot);
      }, opts);
    },

    selectFrame: (name) => set({ selectedFrame: name }),

    addClip: (name) => {
      const { doc, selectedFrame } = get();
      const trimmed = name.trim();
      if (!doc || trimmed === '') return;
      if (doc.clips[trimmed] !== undefined) {
        set({ error: `clip 已存在:${trimmed}` });
        return;
      }
      // 有选中帧则带上(后端拒空帧 clip;空建也允许,保存时错误如实上屏)。
      const frames = selectedFrame ? [selectedFrame] : [];
      mutate((d) => {
        d.clips = sortKeys({ ...d.clips, [trimmed]: { frames, fps: 10, loop: true, onFinish: 'hold' } });
      });
      set({ selectedClip: trimmed, frameIdx: 0, elapsed: 0, playing: false, error: null });
    },

    renameClip: (oldName, newName) => {
      const { doc } = get();
      const trimmed = newName.trim();
      if (!doc || trimmed === '' || trimmed === oldName) return;
      if (doc.clips[trimmed] !== undefined) {
        set({ error: `clip 名已存在:${trimmed}` });
        return;
      }
      mutate((d) => {
        const c = d.clips[oldName];
        if (!c) return;
        delete d.clips[oldName];
        d.clips = sortKeys({ ...d.clips, [trimmed]: c });
        // animator 状态引用同步改名(宽型 JSON,尽力而为;结构不符留给保存校验)。
        const states = (d.animator as { states?: Record<string, { clip?: unknown }> } | undefined)
          ?.states;
        if (states) {
          for (const st of Object.values(states)) {
            if (st && st.clip === oldName) st.clip = trimmed;
          }
        }
      });
      set({
        selectedClip: trimmed,
        error: null,
        animatorText: get().doc?.animator ? JSON.stringify(get().doc?.animator, null, 2) : '',
      });
    },

    deleteClip: (name) => {
      mutate((d) => {
        delete d.clips[name];
      });
      set({ frameIdx: 0, elapsed: 0, playing: false });
    },

    clipAddFrame: (clipName, frameName) => {
      mutate((d) => {
        const c = d.clips[clipName];
        if (!c || d.frames[frameName] === undefined) return;
        c.frames = [...c.frames, frameName];
      });
    },

    clipRemoveFrame: (clipName, idx) => {
      mutate((d) => {
        const c = d.clips[clipName];
        if (!c) return;
        c.frames = c.frames.filter((_, i) => i !== idx);
      });
      set({ frameIdx: 0, elapsed: 0, playing: false });
    },

    clipMoveFrame: (clipName, idx, dir) => {
      const j = idx + dir;
      mutate((d) => {
        const c = d.clips[clipName];
        if (!c || idx < 0 || idx >= c.frames.length || j < 0 || j >= c.frames.length) return;
        const next = [...c.frames];
        [next[idx], next[j]] = [next[j], next[idx]];
        c.frames = next;
      });
    },

    setClipMeta: (clipName, patch) => {
      mutate(
        (d) => {
          const c = d.clips[clipName];
          if (!c) return;
          if (patch.fps !== undefined) c.fps = Math.max(0.01, patch.fps);
          if (patch.duration !== undefined) {
            if (patch.duration === null || !(patch.duration > 0)) delete c.duration;
            else c.duration = patch.duration;
          }
          if (patch.loop !== undefined) c.loop = patch.loop;
          if (patch.onFinish !== undefined) c.onFinish = patch.onFinish;
        },
        { coalesce: `clipmeta:${clipName}:${Object.keys(patch).join(',')}` },
      );
    },

    selectClip: (name) => set({ selectedClip: name, frameIdx: 0, elapsed: 0, playing: false }),

    setAnimatorText: (text) => {
      const trimmed = text.trim();
      if (trimmed === '') {
        // 空 = 移除状态机(合法形态)。
        mutate(
          (d) => {
            delete d.animator;
          },
          { coalesce: 'animator' },
        );
        set({ animatorText: text, animatorError: null });
        return;
      }
      try {
        const v: unknown = JSON.parse(trimmed);
        if (typeof v !== 'object' || v === null || Array.isArray(v)) {
          throw new Error('animator 须为 JSON 对象');
        }
        mutate(
          (d) => {
            d.animator = v as Record<string, unknown>;
          },
          { coalesce: 'animator' },
        );
        set({ animatorText: text, animatorError: null });
      } catch (err) {
        // 坏 JSON:文档保持上个合法值,错误如实显示,save 阻断。
        set({ animatorText: text, animatorError: (err as Error).message });
      }
    },

    save: async () => {
      const { assetPath, doc, animatorError, saving } = get();
      if (!assetPath || !doc || saving) return;
      if (animatorError) {
        set({ error: `animator JSON 无效,禁止保存:${animatorError}` });
        return;
      }
      set({ saving: true, error: null });
      try {
        const r = await callAssetTool<ToolErr & { ok?: boolean }>('sprite_set', {
          assetPath,
          doc: doc as unknown as Record<string, unknown>,
        });
        if (r.error) {
          set({ saving: false, error: r.message ?? r.error });
          return;
        }
        set({ saving: false, dirty: false, savedJson: JSON.stringify(get().doc) });
      } catch (err) {
        set({ saving: false, error: (err as Error).message });
      }
    },

    undo: () => {
      const { undoStack, savedJson, selectedFrame, selectedClip } = get();
      const last = undoStack[undoStack.length - 1];
      if (last === undefined) return;
      const doc = JSON.parse(last) as SpriteDoc;
      set({
        doc,
        undoStack: undoStack.slice(0, -1),
        undoCoalesce: null,
        dirty: JSON.stringify(doc) !== savedJson,
        selectedFrame:
          selectedFrame && doc.frames[selectedFrame] ? selectedFrame : (Object.keys(doc.frames)[0] ?? null),
        selectedClip: selectedClip && doc.clips[selectedClip] ? selectedClip : null,
        animatorText: doc.animator ? JSON.stringify(doc.animator, null, 2) : '',
        animatorError: null,
        frameIdx: 0,
        elapsed: 0,
        playing: false,
      });
    },

    play: () => {
      const { doc, selectedClip, frameIdx } = get();
      const clip = selectedClip ? doc?.clips[selectedClip] : undefined;
      if (!clip || clip.frames.length === 0) return;
      // 非循环停在末帧后再按播放 = 从头重播。
      const restart = !clip.loop && frameIdx >= clip.frames.length - 1;
      set({ playing: true, ...(restart ? { frameIdx: 0, elapsed: 0 } : {}) });
    },

    pause: () => set({ playing: false }),

    tickPreview: (dtSec) => {
      const { doc, selectedClip, playing, frameIdx, elapsed } = get();
      if (!playing || !doc || !selectedClip) return;
      const clip = doc.clips[selectedClip];
      if (!clip || clip.frames.length === 0) {
        set({ playing: false });
        return;
      }
      const dur = clipFrameDuration(clip);
      let e = elapsed + dtSec;
      let idx = Math.min(frameIdx, clip.frames.length - 1);
      let still = true;
      while (e >= dur) {
        e -= dur;
        if (idx + 1 < clip.frames.length) {
          idx += 1;
        } else if (clip.loop) {
          idx = 0;
        } else {
          // 收尾:hold 停末帧 / first 回首帧(与宿主运行时一致)。
          still = false;
          if (clip.onFinish === 'first') idx = 0;
          e = 0;
          break;
        }
      }
      set({ frameIdx: idx, elapsed: e, playing: still });
    },

    seekPreview: (idx) => {
      const { doc, selectedClip } = get();
      const clip = selectedClip ? doc?.clips[selectedClip] : undefined;
      if (!clip) return;
      set({
        playing: false,
        frameIdx: Math.min(Math.max(0, idx), Math.max(0, clip.frames.length - 1)),
        elapsed: 0,
      });
    },

    reset: () => set({ ...FRESH }),
  };
});
