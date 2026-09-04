import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useAssetStore } from '@/lib/assetStore';
import {
  clipFrameDuration,
  framesFromBoxes,
  useSpriteStore,
  type SpriteDoc,
} from '@/lib/spriteStore';
import { forgeMock } from './forgeMock';

/**
 * F-GAME-4 spriteStore:openSprite/openFromTexture 流(forgeMock mock sprite_* 工具)、
 * bbox/clip 编辑 reducer、save 成功与 SPRITE_INVALID 如实进 error 态、undo 回滚、
 * 预览播放推进(loop/hold/first 与后端语义一致)。
 */

const TEX = { path: 'Textures/hero.png', guid: 'tex-1', type: 'texture', size: 10 };
const SPRITE_ITEM = { path: 'Sprites/hero.rxsprite', guid: 'sp-1', type: 'sprite', size: 5 };

function baseDoc(): SpriteDoc {
  return {
    version: 1,
    texture: 'tex-1',
    pivot: [0.5, 1],
    frames: {
      frame_0: { bbox: [0, 0, 16, 16] },
      frame_1: { bbox: [16, 0, 16, 16], pivot: [0.4, 1] },
    },
    clips: {
      walk: { frames: ['frame_0', 'frame_1'], fps: 8, loop: true, onFinish: 'hold' },
    },
  };
}

const initialSprite = useSpriteStore.getState();
const initialAssets = useAssetStore.getState();

/** 常用打开流:mock sprite_get/asset_thumbnail/asset_list 后 openSprite。 */
async function openWithDoc(doc: SpriteDoc = baseDoc()) {
  forgeMock.setAssets([TEX, SPRITE_ITEM]);
  forgeMock.setBuildStatus([]);
  forgeMock.setDefault('sprite_get', { guid: 'sp-1', doc });
  forgeMock.setDefault('asset_thumbnail', { dataUrl: 'data:image/png;base64,AAA=' });
  await useSpriteStore.getState().openSprite('Sprites/hero.rxsprite');
}

beforeEach(() => {
  forgeMock.reset();
  forgeMock.stubGlobal();
  useSpriteStore.setState(initialSprite, true);
  useAssetStore.setState(initialAssets, true);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('openSprite / openFromTexture', () => {
  it('openSprite:sprite_get + 贴图 GUID→路径解析 + asset_thumbnail,基线快照就位', async () => {
    await openWithDoc();
    const s = useSpriteStore.getState();
    expect(s.error).toBeNull();
    expect(s.loading).toBe(false);
    expect(s.guid).toBe('sp-1');
    expect(Object.keys(s.doc!.frames)).toEqual(['frame_0', 'frame_1']);
    expect(s.texPath).toBe('Textures/hero.png');
    expect(s.texDataUrl).toBe('data:image/png;base64,AAA=');
    expect(s.dirty).toBe(false);
    expect(s.selectedFrame).toBe('frame_0');
    expect(s.selectedClip).toBe('walk');
    // thumbnail 调用落到贴图路径而非 sprite 路径。
    const thumbCall = forgeMock.calls.find((c) => c.tool === 'mcp__asset-pipeline__asset_thumbnail');
    expect(thumbCall?.arguments).toEqual({ assetPath: 'Textures/hero.png' });
  });

  it('openSprite:工具级 {error,message} 如实进 error 态,不吞', async () => {
    forgeMock.setAssets([]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('sprite_get', { error: 'NO_META', message: '缺 .meta: Sprites/x.rxsprite' });
    await useSpriteStore.getState().openSprite('Sprites/x.rxsprite');
    const s = useSpriteStore.getState();
    expect(s.doc).toBeNull();
    expect(s.error).toBe('缺 .meta: Sprites/x.rxsprite');
  });

  it('openFromTexture:name=文件名去扩展名,autoslice=false 建空精灵后打开', async () => {
    forgeMock.setAssets([TEX, SPRITE_ITEM]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('sprite_create', {
      assetPath: 'Sprites/hero.rxsprite',
      guid: 'sp-1',
      textureGuid: 'tex-1',
      frameCount: 0,
      clipCount: 0,
    });
    forgeMock.setDefault('sprite_get', {
      guid: 'sp-1',
      doc: { version: 1, texture: 'tex-1', pivot: [0.5, 1], frames: {}, clips: {} },
    });
    forgeMock.setDefault('asset_thumbnail', { dataUrl: 'data:image/png;base64,AAA=' });

    await useSpriteStore.getState().openFromTexture('Textures/hero.png', 'tex-1');
    const create = forgeMock.calls.find((c) => c.tool === 'mcp__asset-pipeline__sprite_create');
    expect(create?.arguments).toEqual({ name: 'hero', texture: 'tex-1', autoslice: false });
    const s = useSpriteStore.getState();
    expect(s.assetPath).toBe('Sprites/hero.rxsprite');
    expect(s.doc).not.toBeNull();
    expect(s.error).toBeNull();
  });

  it('openFromTexture:sprite_create 错误如实上屏', async () => {
    forgeMock.setDefault('sprite_create', { error: 'UNKNOWN_GUID', message: 'texture GUID 不存在: bad' });
    await useSpriteStore.getState().openFromTexture('Textures/bad.png', 'bad');
    expect(useSpriteStore.getState().error).toBe('texture GUID 不存在: bad');
  });
});

describe('帧编辑 reducer', () => {
  it('addFrame 自动命名不重名并选中;bbox 落库钳制取整(x,y≥0;w,h≥1)', async () => {
    await openWithDoc();
    const st = useSpriteStore.getState();
    const name = st.addFrame([4, 4, 8, 8]);
    expect(name).toBe('frame_2');
    expect(useSpriteStore.getState().selectedFrame).toBe('frame_2');

    st.setFrameBbox('frame_2', [-2, 3.6, 0.2, 9.4]);
    expect(useSpriteStore.getState().doc!.frames['frame_2'].bbox).toEqual([0, 4, 1, 9]);
    expect(useSpriteStore.getState().dirty).toBe(true);
  });

  it('renameFrame 同步 clip 引用;重名拒绝并报错', async () => {
    await openWithDoc();
    useSpriteStore.getState().renameFrame('frame_0', 'walk_a');
    let s = useSpriteStore.getState();
    expect(s.doc!.frames['walk_a']).toBeDefined();
    expect(s.doc!.frames['frame_0']).toBeUndefined();
    expect(s.doc!.clips['walk'].frames).toEqual(['walk_a', 'frame_1']);
    expect(s.selectedFrame).toBe('walk_a');

    useSpriteStore.getState().renameFrame('frame_1', 'walk_a');
    s = useSpriteStore.getState();
    expect(s.error).toContain('帧名已存在');
    expect(s.doc!.frames['frame_1']).toBeDefined();
  });

  it('deleteFrame 连带移出 clip 帧序列', async () => {
    await openWithDoc();
    useSpriteStore.getState().deleteFrame('frame_1');
    const s = useSpriteStore.getState();
    expect(s.doc!.frames['frame_1']).toBeUndefined();
    expect(s.doc!.clips['walk'].frames).toEqual(['frame_0']);
  });

  it('pivot:帧级写入钳制 0..1 四位小数;null 清除覆盖;文档级同规则', async () => {
    await openWithDoc();
    const st = useSpriteStore.getState();
    st.setFramePivot('frame_0', [1.2, -0.5]);
    expect(useSpriteStore.getState().doc!.frames['frame_0'].pivot).toEqual([1, 0]);
    st.setFramePivot('frame_0', null);
    expect(useSpriteStore.getState().doc!.frames['frame_0'].pivot).toBeUndefined();
    st.setDocPivot([0.33333333, 1]);
    expect(useSpriteStore.getState().doc!.pivot).toEqual([0.3333, 1]);
  });
});

describe('clip 编辑 reducer', () => {
  it('addClip 带上选中帧;重名拒绝;帧序列增/移/删;meta 表单', async () => {
    await openWithDoc();
    const st = useSpriteStore.getState();
    st.selectFrame('frame_1');
    st.addClip('idle');
    let s = useSpriteStore.getState();
    expect(s.doc!.clips['idle']).toEqual({ frames: ['frame_1'], fps: 10, loop: true, onFinish: 'hold' });
    expect(s.selectedClip).toBe('idle');

    st.addClip('idle');
    expect(useSpriteStore.getState().error).toContain('clip 已存在');

    st.clipAddFrame('idle', 'frame_0');
    expect(useSpriteStore.getState().doc!.clips['idle'].frames).toEqual(['frame_1', 'frame_0']);
    st.clipMoveFrame('idle', 1, -1);
    expect(useSpriteStore.getState().doc!.clips['idle'].frames).toEqual(['frame_0', 'frame_1']);
    st.clipRemoveFrame('idle', 0);
    expect(useSpriteStore.getState().doc!.clips['idle'].frames).toEqual(['frame_1']);

    st.setClipMeta('idle', { fps: 12, duration: 1.5, loop: false, onFinish: 'first' });
    const c = useSpriteStore.getState().doc!.clips['idle'];
    expect(c).toEqual({ frames: ['frame_1'], fps: 12, duration: 1.5, loop: false, onFinish: 'first' });
    st.setClipMeta('idle', { duration: null });
    expect(useSpriteStore.getState().doc!.clips['idle'].duration).toBeUndefined();

    st.deleteClip('idle');
    expect(useSpriteStore.getState().doc!.clips['idle']).toBeUndefined();
    expect(useSpriteStore.getState().selectedClip).toBeNull();
  });

  it('renameClip 同步 animator 状态引用', async () => {
    const doc = baseDoc();
    doc.animator = {
      defaultState: 'w',
      states: { w: { clip: 'walk' } },
      transitions: [],
    };
    await openWithDoc(doc);
    useSpriteStore.getState().renameClip('walk', 'run');
    const s = useSpriteStore.getState();
    expect(s.doc!.clips['run']).toBeDefined();
    expect(s.doc!.clips['walk']).toBeUndefined();
    expect((s.doc!.animator as { states: Record<string, { clip: string }> }).states['w'].clip).toBe('run');
    expect(s.selectedClip).toBe('run');
  });
});

describe('save / undo / animator', () => {
  it('save 成功:sprite_set 带整文档,dirty 归零', async () => {
    await openWithDoc();
    useSpriteStore.getState().setDocPivot([0.5, 0.5]);
    expect(useSpriteStore.getState().dirty).toBe(true);

    forgeMock.setDefault('sprite_set', { ok: true, frameCount: 2, clipCount: 1 });
    await useSpriteStore.getState().save();
    const s = useSpriteStore.getState();
    expect(s.error).toBeNull();
    expect(s.dirty).toBe(false);
    const call = forgeMock.calls.find((c) => c.tool === 'mcp__asset-pipeline__sprite_set');
    expect(call?.arguments.assetPath).toBe('Sprites/hero.rxsprite');
    expect((call?.arguments.doc as SpriteDoc).pivot).toEqual([0.5, 0.5]);
  });

  it('save 失败:SPRITE_INVALID 的 message 如实进 error 态,dirty 保持', async () => {
    await openWithDoc();
    useSpriteStore.getState().setDocPivot([0.5, 0.5]);
    forgeMock.setDefault('sprite_set', {
      error: 'SPRITE_INVALID',
      message: 'clip walk 引用不存在的帧: nosuch',
    });
    await useSpriteStore.getState().save();
    const s = useSpriteStore.getState();
    expect(s.error).toBe('clip walk 引用不存在的帧: nosuch');
    expect(s.dirty).toBe(true);
  });

  it('undo 回滚一步;同手势 coalesce 合并为一档;回到基线 dirty 归零', async () => {
    await openWithDoc();
    const st = useSpriteStore.getState();
    // 同一拖拽手势多次变更 → 一条撤销记录。
    st.setFrameBbox('frame_0', [1, 1, 16, 16], { coalesce: 'gesture-1' });
    st.setFrameBbox('frame_0', [2, 2, 16, 16], { coalesce: 'gesture-1' });
    st.setFrameBbox('frame_0', [3, 3, 16, 16], { coalesce: 'gesture-1' });
    expect(useSpriteStore.getState().undoStack).toHaveLength(1);
    expect(useSpriteStore.getState().doc!.frames['frame_0'].bbox).toEqual([3, 3, 16, 16]);

    useSpriteStore.getState().undo();
    const s = useSpriteStore.getState();
    expect(s.doc!.frames['frame_0'].bbox).toEqual([0, 0, 16, 16]);
    expect(s.dirty).toBe(false);
    expect(s.undoStack).toHaveLength(0);
    // 空栈 undo 无害。
    useSpriteStore.getState().undo();
    expect(useSpriteStore.getState().doc!.frames['frame_0'].bbox).toEqual([0, 0, 16, 16]);
  });

  it('animator 文本:坏 JSON 报错并禁存(不发 sprite_set);好 JSON 应用进 doc;清空移除', async () => {
    await openWithDoc();
    const st = useSpriteStore.getState();
    st.setAnimatorText('{ bad json');
    let s = useSpriteStore.getState();
    expect(s.animatorError).not.toBeNull();

    forgeMock.setDefault('sprite_set', { ok: true });
    await useSpriteStore.getState().save();
    s = useSpriteStore.getState();
    expect(s.error).toContain('禁止保存');
    expect(forgeMock.calls.some((c) => c.tool === 'mcp__asset-pipeline__sprite_set')).toBe(false);

    const good = '{ "defaultState": "w", "states": { "w": { "clip": "walk" } } }';
    st.setAnimatorText(good);
    s = useSpriteStore.getState();
    expect(s.animatorError).toBeNull();
    expect((s.doc!.animator as { defaultState: string }).defaultState).toBe('w');

    st.setAnimatorText('');
    expect(useSpriteStore.getState().doc!.animator).toBeUndefined();
  });
});

describe('自动切帧', () => {
  it('serverAutoslice:boxes 重建 frames(frame_<i>)、清 clip 悬空引用、回填贴图尺寸', async () => {
    const doc = baseDoc();
    doc.frames = { hero_a: { bbox: [0, 0, 8, 8] }, frame_0: { bbox: [8, 0, 8, 8] } };
    doc.clips = { walk: { frames: ['hero_a', 'frame_0'], fps: 8, loop: true, onFinish: 'hold' } };
    await openWithDoc(doc);
    forgeMock.setDefault('sprite_autoslice', {
      width: 64,
      height: 32,
      boxes: [
        [0, 0, 16, 16],
        [16, 0, 16, 16],
        [32, 0, 16, 16],
      ],
    });
    await useSpriteStore.getState().serverAutoslice();
    const s = useSpriteStore.getState();
    expect(Object.keys(s.doc!.frames)).toEqual(['frame_0', 'frame_1', 'frame_2']);
    expect(s.doc!.frames['frame_1'].bbox).toEqual([16, 0, 16, 16]);
    // hero_a 悬空引用被清;重建后同名 frame_0 保留。
    expect(s.doc!.clips['walk'].frames).toEqual(['frame_0']);
    expect(s.texW).toBe(64);
    expect(s.texH).toBe(32);
    const call = forgeMock.calls.find((c) => c.tool === 'mcp__asset-pipeline__sprite_autoslice');
    expect(call?.arguments).toEqual({ assetPath: 'Textures/hero.png', minArea: 16 });
  });

  it('autoDetect:像素未就绪如实报错;就绪后本地重建 frames(与服务端同规则)', async () => {
    await openWithDoc();
    useSpriteStore.getState().autoDetect();
    expect(useSpriteStore.getState().error).toContain('贴图像素未就绪');

    // 构造 12×6 透明底,两个 4×5 白块。
    const w = 12;
    const h = 6;
    const px = new Uint8ClampedArray(w * h * 4);
    const blob = (x0: number) => {
      for (let y = 0; y < 5; y++) {
        for (let x = x0; x < x0 + 4; x++) px.set([255, 255, 255, 255], (y * w + x) * 4);
      }
    };
    blob(0);
    blob(6);
    useSpriteStore.getState().setTextureBitmap(w, h, px);
    useSpriteStore.getState().autoDetect(8);
    const s = useSpriteStore.getState();
    expect(s.error).toBeNull();
    expect(Object.keys(s.doc!.frames)).toEqual(['frame_0', 'frame_1']);
    expect(s.doc!.frames['frame_0'].bbox).toEqual([0, 0, 4, 5]);
    expect(s.doc!.frames['frame_1'].bbox).toEqual([6, 0, 4, 5]);
  });

  it('framesFromBoxes:>10 个补零到 2 位(与服务端 frames_from_boxes 同规则)', () => {
    const boxes = Array.from({ length: 11 }, (_, i) => [i, 0, 1, 1] as [number, number, number, number]);
    const names = Object.keys(framesFromBoxes(boxes));
    expect(names[0]).toBe('frame_00');
    expect(names[10]).toBe('frame_10');
    expect(Object.keys(framesFromBoxes(boxes.slice(0, 3)))).toEqual(['frame_0', 'frame_1', 'frame_2']);
  });
});

describe('预览播放', () => {
  it('clipFrameDuration:duration/帧数 优先,否则 1/fps', () => {
    expect(
      clipFrameDuration({ frames: ['a', 'b'], fps: 8, duration: 1, loop: true, onFinish: 'hold' }),
    ).toBe(0.5);
    expect(clipFrameDuration({ frames: ['a', 'b'], fps: 8, loop: true, onFinish: 'hold' })).toBe(1 / 8);
  });

  it('loop:tick 跨帧推进并回卷', async () => {
    await openWithDoc(); // walk: 2 帧 fps 8 → 每帧 0.125s
    const st = useSpriteStore.getState();
    st.selectClip('walk');
    st.play();
    st.tickPreview(0.125);
    expect(useSpriteStore.getState().frameIdx).toBe(1);
    st.tickPreview(0.125);
    const s = useSpriteStore.getState();
    expect(s.frameIdx).toBe(0); // 回卷
    expect(s.playing).toBe(true);
  });

  it('非循环 hold:停末帧;first:回首帧;play 重播;seek 暂停定位', async () => {
    const doc = baseDoc();
    doc.clips['walk'].loop = false;
    await openWithDoc(doc);
    const st = useSpriteStore.getState();
    st.selectClip('walk');
    st.play();
    st.tickPreview(0.125); // → 帧 1(末帧)
    st.tickPreview(0.125); // 播完:hold 停末帧
    let s = useSpriteStore.getState();
    expect(s.playing).toBe(false);
    expect(s.frameIdx).toBe(1);

    // 停在末帧后再播 = 从头重播。
    st.play();
    s = useSpriteStore.getState();
    expect(s.playing).toBe(true);
    expect(s.frameIdx).toBe(0);
    st.pause();

    // first:播完回首帧。
    st.setClipMeta('walk', { onFinish: 'first' });
    st.play();
    st.tickPreview(0.3); // 一口气播完(0.3 > 2×0.125)
    s = useSpriteStore.getState();
    expect(s.playing).toBe(false);
    expect(s.frameIdx).toBe(0);

    st.seekPreview(1);
    s = useSpriteStore.getState();
    expect(s.frameIdx).toBe(1);
    expect(s.playing).toBe(false);
  });
});
