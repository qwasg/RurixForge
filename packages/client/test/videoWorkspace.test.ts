import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { apiGenVideo, apiGenVideoFrames } from '@/lib/forgeApi';
import { useGenStore } from '@/lib/genStore';
import { loadStudio, useStudioStore } from '@/lib/studioStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';

beforeEach(() => {
  localStorage.clear();
  useGenStore.setState({ backends: [], backendsLoaded: false, backendsError: null });
  useWorkspaceStore.setState({ activeWorkspaceId: null });
  useStudioStore.setState({ nodes: [], edges: [], workspaceId: null, seq: 1, busyIds: [], lastError: null });
});

function delayedVideos() {
  const pending: Array<{ body: Record<string, unknown>; finish: (file: string, fail?: boolean) => void }> = [];
  useGenStore.setState({ backends: [], backendsLoaded: true, backendsError: null });
  vi.stubGlobal('fetch', vi.fn((url: unknown, init?: RequestInit) => {
    if (String(url).endsWith('/gen/video/frames')) {
      return Promise.resolve({ ok: true, status: 200, json: async () => ({
        atlas: { fileRef: '.forge/tmp/gen/atlas.png', width: 32, height: 32 }, boxes: [[0, 0, 32, 32]], fps: 8,
      }) } as Response);
    }
    return new Promise<Response>((resolve) => pending.push({
      body: JSON.parse(String(init?.body)),
      finish: (file, fail = false) => resolve({ ok: !fail, status: fail ? 500 : 200,
        json: async () => fail ? { error: { code: 'GEN_BACKEND_ERROR', message: file } }
          : { backendId: 'comfyui-minimax-h3', artifacts: [{ fileRef: file, mime: 'video/mp4', ext: 'mp4' }] },
      } as Response),
    }));
  }));
  return pending;
}

function createVideoBoard(workspaceId: string, preset: 'video' | 'charanim' = 'video') {
  useStudioStore.getState().bindWorkspace(workspaceId);
  const id = useStudioStore.getState().addNode(preset)!;
  useStudioStore.getState().setPrompt(id, `${workspaceId} 的角色行走`);
  useStudioStore.getState().setParam(id, 'refAssetPath', 'Concepts/hero.png');
  return id;
}

describe('视频异步完成的画板隔离', () => {
  it.each(['video', 'charanim'] as const)('%s 在后台完成仅写回原画板,不碰另一画板同 id 节点的版本或 busy', async (preset) => {
    const pending = delayedVideos();
    const idA = createVideoBoard('A', preset);
    const runA = useStudioStore.getState().generate(idA);
    const idB = createVideoBoard('B', preset);
    expect(idB).toBe(idA);
    const runB = useStudioStore.getState().generate(idB);
    const beforeB = useStudioStore.getState();
    pending[0].finish('.forge/tmp/gen/A.mp4');
    await runA;
    expect(useStudioStore.getState()).toBe(beforeB);
    expect(loadStudio('A').nodes[0].versions[0].fileRef).toBe('.forge/tmp/gen/A.mp4');
    useStudioStore.getState().bindWorkspace('A');
    expect(useStudioStore.getState().nodes[0].versions[0].fileRef).toBe('.forge/tmp/gen/A.mp4');
    expect(useStudioStore.getState().busyIds).toEqual([]);
    const beforeA = useStudioStore.getState();
    pending[1].finish('.forge/tmp/gen/B.mp4');
    await runB;
    expect(useStudioStore.getState()).toBe(beforeA);
    useStudioStore.getState().bindWorkspace('B');
    expect(useStudioStore.getState().nodes[0].versions[0].fileRef).toBe('.forge/tmp/gen/B.mp4');
    expect(useStudioStore.getState().busyIds).toEqual([]);
  });

  it('后台失败不会覆盖当前画板错误或 busy,返回原画板可见失败信息', async () => {
    const pending = delayedVideos();
    const idA = createVideoBoard('error-A');
    const runA = useStudioStore.getState().generate(idA);
    const idB = createVideoBoard('error-B');
    const runB = useStudioStore.getState().generate(idB);
    useStudioStore.setState({ lastError: { nodeId: idB, code: 'B_ERROR', message: 'B 自己的错误' } });
    const beforeB = useStudioStore.getState();
    pending[0].finish('A GPU failed', true);
    await runA;
    expect(useStudioStore.getState()).toBe(beforeB);
    useStudioStore.getState().bindWorkspace('error-A');
    expect(useStudioStore.getState().lastError).toMatchObject({ nodeId: idA, message: 'A GPU failed' });
    expect(useStudioStore.getState().busyIds).toEqual([]);
    pending[1].finish('.forge/tmp/gen/B.mp4');
    await runB;
  });

  it('切走再切回恢复 busy;取消后新请求不会被旧请求完成清掉,产物版本 id 唯一', async () => {
    const pending = delayedVideos();
    const id = createVideoBoard('restart-A');
    const first = useStudioStore.getState().generate(id);
    useStudioStore.getState().bindWorkspace('restart-B');
    useStudioStore.getState().bindWorkspace('restart-A');
    expect(useStudioStore.getState().busyIds).toEqual([id]);
    await useStudioStore.getState().cancelGenerate(id);
    const second = useStudioStore.getState().generate(id);
    pending[0].finish('.forge/tmp/gen/first.mp4');
    await first;
    expect(useStudioStore.getState().busyIds).toEqual([id]);
    pending[1].finish('.forge/tmp/gen/second.mp4');
    await second;
    expect(useStudioStore.getState().busyIds).toEqual([]);
    const versions = useStudioStore.getState().nodes[0].versions;
    expect(versions.map((v) => v.fileRef)).toEqual(['.forge/tmp/gen/first.mp4', '.forge/tmp/gen/second.mp4']);
    expect(new Set(versions.map((v) => v.id)).size).toBe(2);
  });
});

afterEach(() => {
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe('视频 REST 工作区作用域', () => {
  it.each(['video', 'frames'] as const)('%s 默认传当前工作区,显式工作区与 null 不受当前选项覆盖', async (kind) => {
    const posts: Record<string, unknown>[] = [];
    vi.stubGlobal('fetch', vi.fn(async (_url: unknown, init?: RequestInit) => {
      posts.push(JSON.parse(String(init?.body)));
      return { ok: true, status: 200, json: async () => ({ artifacts: [] }) } as Response;
    }));
    localStorage.setItem('forge:activeWorkspace', 'selected-project');
    const call = (scope: { workspaceId?: string | null }) => kind === 'video'
      ? apiGenVideo({ prompt: 'animate', imageRef: 'Content/reference.png', ...scope })
      : apiGenVideoFrames({ videoFileRef: '.forge/tmp/gen/video.mp4', ...scope });
    await call({});
    await call({ workspaceId: 'non-active-project' });
    await call({ workspaceId: null });
    expect(posts.map((p) => p.workspaceId)).toEqual(['selected-project', 'non-active-project', null]);
  });

  it.each([
    { preset: 'video' as const, boardId: 'board-project' },
    { preset: 'charanim' as const, boardId: 'board-project' },
    { preset: 'video' as const, boardId: null },
    { preset: 'charanim' as const, boardId: null },
  ])('$preset 在等待后端清单与视频产出前固定工作区($boardId),截帧沿用同一项目', async ({ preset, boardId }) => {
    const posts: { url: string; body: Record<string, unknown> }[] = [];
    localStorage.setItem('forge:activeWorkspace', 'initial-selected');
    useWorkspaceStore.setState({ activeWorkspaceId: 'initial-selected' });
    useStudioStore.setState({ workspaceId: boardId });
    const id = useStudioStore.getState().addNode(preset)!;
    useStudioStore.getState().setPrompt(id, '角色行走');
    useStudioStore.getState().setParam(id, 'refAssetPath', 'Concepts/hero.png');
    vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
      const path = String(url);
      if (path.endsWith('/gen/backends')) {
        // 清单异步返回期间切换活动项目,本轮仍须读取最初项目的 reference。
        localStorage.setItem('forge:activeWorkspace', 'later-selected');
        useWorkspaceStore.setState({ activeWorkspaceId: 'later-selected' });
        return { ok: true, status: 200, json: async () => ({ backends: [] }) } as Response;
      }
      posts.push({ url: path, body: JSON.parse(String(init?.body)) });
      if (path.endsWith('/gen/video')) {
        return { ok: true, status: 200, json: async () => ({ backendId: 'comfyui-minimax-h3',
          artifacts: [{ fileRef: '.forge/tmp/gen/local.mp4', mime: 'video/mp4', ext: 'mp4' }] }) } as Response;
      }
      return { ok: true, status: 200, json: async () => ({
        atlas: { fileRef: '.forge/tmp/gen/atlas.png', width: 32, height: 32 }, boxes: [[0, 0, 32, 32]], fps: 8,
      }) } as Response;
    }));
    await useStudioStore.getState().generate(id);
    expect(useStudioStore.getState().lastError).toBeNull();
    expect(posts[0].body).toMatchObject({ workspaceId: boardId ?? 'initial-selected', imageRef: 'Content/Concepts/hero.png' });
    expect(posts).toHaveLength(preset === 'charanim' ? 2 : 1);
    expect(posts.every((p) => p.body.workspaceId === (boardId ?? 'initial-selected'))).toBe(true);
    if (preset === 'charanim') expect(posts[1].url).toBe('/api/forge/gen/video/frames');
  });
});
