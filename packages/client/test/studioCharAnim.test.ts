import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { canAccept, canReslice, presetOf, useStudioStore, type StudioNode } from '@/lib/studioStore';

/**
 * 角色动画通道(参考图 → 图生视频 → 截帧图集 → .rxsprite)的诚实面:
 * 无参考图不许悄悄降级成文生视频;截帧失败不能连坐把已出片的 mp4 一起丢掉;
 * 入库要真的把图集 GUID 与逐帧 bbox 交给 sprite_create,而不是只 accept 一张贴图。
 */

interface Call {
  url: string;
  body: Record<string, unknown>;
}

const FAKE_ATLAS = {
  fileRef: '.forge/tmp/gen/gen-1-2-0.png',
  mime: 'image/png',
  dataUrl: 'data:image/png;base64,AAAA',
  width: 20,
  height: 11,
};

function stubFetch(
  handler: (url: string, body: Record<string, unknown>) => { ok: boolean; status: number; json: unknown },
): Call[] {
  const calls: Call[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: unknown, init?: { body?: string }) => {
      const u = String(url);
      const body = init?.body !== undefined ? (JSON.parse(init.body) as Record<string, unknown>) : {};
      calls.push({ url: u, body });
      const r = handler(u, body);
      return { ok: r.ok, status: r.status, json: async () => r.json } as unknown as Response;
    }),
  );
  return calls;
}

function seedNode(extra: Partial<StudioNode> = {}): StudioNode {
  const node: StudioNode = {
    id: 'sa',
    preset: 'charanim',
    name: '角色动画 1',
    pos: [0, 0],
    prompt: '向右行走循环',
    params: {
      aspect: '1:1',
      resolution: '720p',
      durationSec: 5,
      fps: 8,
      maxFrames: 32,
      chromaKey: 'auto',
      crop: 'union',
      clipName: 'walk',
      refAssetPath: 'Concepts/hero.png',
    },
    versions: [],
    currentVersionId: null,
    ...extra,
  };
  useStudioStore.setState({ nodes: [node], edges: [], seq: 10, busyIds: [], lastError: null });
  return node;
}

beforeEach(() => {
  localStorage.clear();
  useStudioStore.setState({ nodes: [], edges: [], seq: 1, busyIds: [], lastError: null, workspaceId: null });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('素材创作 · 角色动画通道', () => {
  it('无参考图 → 如实报错,不静默降级成文生视频', async () => {
    seedNode({ params: { clipName: 'walk', refAssetPath: '' } });
    const calls = stubFetch(() => ({ ok: true, status: 200, json: {} }));
    await useStudioStore.getState().generate('sa');
    expect(useStudioStore.getState().lastError?.code).toBe('NO_REFERENCE_IMAGE');
    expect(calls.some((c) => c.url.includes('/gen/video'))).toBe(false);
  });

  it('参考图 + 提示词纪律 → gen/video 带 imageRef;截帧回填 atlas/boxes', async () => {
    seedNode();
    const calls = stubFetch((url) => {
      if (url.endsWith('/api/forge/gen/video')) {
        return {
          ok: true,
          status: 200,
          json: {
            backendId: 'remote-video-compatible',
            artifacts: [{ fileRef: '.forge/tmp/gen/gen-1-2-0.mp4', mime: 'video/mp4', ext: 'mp4' }],
          },
        };
      }
      return { ok: true, status: 200, json: { atlas: FAKE_ATLAS, boxes: [[2, 2, 7, 7], [11, 2, 7, 7]], fps: 8, frameCount: 2 } };
    });
    await useStudioStore.getState().generate('sa');

    const video = calls.find((c) => c.url.endsWith('/api/forge/gen/video'));
    expect(video?.body.imageRef).toBe('Content/Concepts/hero.png');
    // 参考图路径带 Content/ 前缀;提示词自动补上定镜/纯色底纪律。
    expect(String(video?.body.prompt)).toContain('向右行走循环');
    expect(String(video?.body.prompt)).toContain('镜头完全固定');

    const frames = calls.find((c) => c.url.endsWith('/gen/video/frames'));
    expect(frames?.body).toMatchObject({
      videoFileRef: '.forge/tmp/gen/gen-1-2-0.mp4',
      fps: 8,
      chromaKey: 'auto',
      crop: 'union',
    });

    const node = useStudioStore.getState().nodes[0];
    const v = node.versions[0];
    expect(v.videoFileRef).toBe('.forge/tmp/gen/gen-1-2-0.mp4');
    expect(v.atlas?.fileRef).toBe(FAKE_ATLAS.fileRef);
    expect(v.boxes).toHaveLength(2);
    expect(v.fps).toBe(8);
    expect(canAccept(presetOf('charanim'), v)).toBe(true);
  });

  it('截帧失败(如本机无 ffmpeg)→ 保留 mp4 版本 + 如实报错,不丢视频', async () => {
    seedNode();
    stubFetch((url) => {
      if (url.endsWith('/api/forge/gen/video')) {
        return {
          ok: true,
          status: 200,
          json: {
            backendId: 'remote-video-compatible',
            artifacts: [{ fileRef: '.forge/tmp/gen/gen-1-2-0.mp4', mime: 'video/mp4', ext: 'mp4' }],
          },
        };
      }
      return {
        ok: false,
        status: 501,
        json: { error: { code: 'GEN_TOOL_MISSING', message: '未找到 ffmpeg(视频截帧必需)' } },
      };
    });
    await useStudioStore.getState().generate('sa');

    const v = useStudioStore.getState().nodes[0].versions[0];
    expect(v).toBeDefined();
    expect(v.videoFileRef).toBe('.forge/tmp/gen/gen-1-2-0.mp4');
    expect(v.atlas).toBeUndefined();
    const err = useStudioStore.getState().lastError;
    expect(err?.code).toBe('GEN_TOOL_MISSING');
    expect(err?.message).toContain('视频已生成');
    // 只有 mp4 不算引擎资产,不给入库;但可以换参数重切帧。
    expect(canAccept(presetOf('charanim'), v)).toBe(false);
    expect(canReslice(presetOf('charanim'), v)).toBe(true);
  });

  it('重新截帧只打截帧端点,不重新生成视频', async () => {
    seedNode({
      versions: [
        {
          id: 'v1',
          createdAt: 0,
          backendId: 'remote-video-compatible',
          prompt: '向右行走循环',
          videoFileRef: '.forge/tmp/gen/gen-1-2-0.mp4',
        },
      ],
      currentVersionId: 'v1',
    });
    const calls = stubFetch(() => ({
      ok: true,
      status: 200,
      json: { atlas: FAKE_ATLAS, boxes: [[2, 2, 7, 7]], fps: 12, frameCount: 1 },
    }));
    useStudioStore.getState().setParam('sa', 'fps', 12);
    await useStudioStore.getState().resliceVersion('sa', 'v1');

    expect(calls.every((c) => c.url.endsWith('/gen/video/frames'))).toBe(true);
    expect(calls[0].body.fps).toBe(12);
    expect(useStudioStore.getState().nodes[0].versions[0].fps).toBe(12);
  });

  it('入库 → 图集走 gen_accept(origin=gen-video),再用 GUID + boxes 建 .rxsprite', async () => {
    seedNode({
      versions: [
        {
          id: 'v1',
          createdAt: 0,
          backendId: 'remote-video-compatible',
          prompt: 'walk right',
          seed: 7,
          videoFileRef: '.forge/tmp/gen/gen-1-2-0.mp4',
          atlas: FAKE_ATLAS,
          boxes: [
            [2, 2, 7, 7],
            [11, 2, 7, 7],
          ],
          fps: 8,
        },
      ],
      currentVersionId: 'v1',
    });
    const calls = stubFetch((_url, body) => {
      const tool = String(body.tool);
      const payload =
        tool.endsWith('gen_accept')
          ? { assetPath: 'Textures/walk-right-7.png', guid: 'guid-atlas' }
          : { assetPath: 'Sprites/walk-right-7.rxsprite', frameCount: 2, clipCount: 1 };
      return { ok: true, status: 200, json: { content: [{ type: 'text', text: JSON.stringify(payload) }] } };
    });
    await useStudioStore.getState().acceptVersion('sa', 'v1');
    expect(useStudioStore.getState().lastError).toBeNull();

    const accept = calls.find((c) => String(c.body.tool).endsWith('gen_accept'));
    expect(accept?.body.arguments).toMatchObject({
      imageFileRef: FAKE_ATLAS.fileRef,
      destFolder: 'Textures',
      origin: 'gen-video',
    });

    const create = calls.find((c) => String(c.body.tool).endsWith('sprite_create'));
    const args = create?.body.arguments as Record<string, unknown>;
    expect(args.texture).toBe('guid-atlas');
    expect(args.frames).toEqual({ frame_0: { bbox: [2, 2, 7, 7] }, frame_1: { bbox: [11, 2, 7, 7] } });
    expect(args.clips).toEqual({
      walk: { frames: ['frame_0', 'frame_1'], fps: 8, loop: true, onFinish: 'hold' },
    });

    const v = useStudioStore.getState().nodes[0].versions[0];
    expect(v.guid).toBe('guid-atlas');
    expect(v.spritePath).toBe('Sprites/walk-right-7.rxsprite');
    // 已入库 → 不重复入库,但仍可重切帧换参数
    expect(canAccept(presetOf('charanim'), v)).toBe(false);
  });
});
