import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { configuredBackends, slugifyName, useGenStore, type GenCandidate } from '@/lib/genStore';
import { useAssetStore } from '@/lib/assetStore';
import { mockForgeBackend } from './forgeMock';

/**
 * F5 wave.3 genStore:backends 加载/configured 过滤;gen_image → candidates 进 modal;
 * accept → gen_accept 参数(destFolder/name slug)+ assetStore 刷新触发;错误码如实保留。
 */

const BACKENDS = {
  backends: [
    { id: 'local-mock', kind: 'local', configured: true, endpointSet: false, capabilities: {} },
    {
      id: 'remote-openai-compatible',
      kind: 'remote',
      configured: false,
      endpointSet: false,
      capabilities: {},
    },
  ],
};

const CANDIDATES: GenCandidate[] = [0, 1, 2, 3].map((i) => ({
  imageFileRef: `.forge/tmp/gen/gen-1-4${i}-${i}.png`,
  seed: 40 + i,
  backendId: 'local-mock',
  dataUrl: 'data:image/png;base64,AAAA',
}));

const ASSETS = {
  assets: [{ path: 'Textures/wood_albedo.png', guid: 'g-wood', type: 'texture', size: 10 }],
};

function toolMap() {
  return {
    gen_image: { candidates: CANDIDATES },
    gen_accept: { assetPath: 'Textures/wood-40.png', guid: 'g-new' },
    asset_list: ASSETS,
    asset_build_status: { items: [] },
  };
}

beforeEach(() => {
  useGenStore.setState({
    backends: [],
    backendsLoaded: false,
    backendsError: null,
    dialogOpen: false,
    destFolder: 'Textures',
    bindAssetPath: null,
    candidates: null,
    lastPrompt: '',
    acceptedRefs: [],
    busy: false,
    acceptBusy: null,
    lastError: null,
    lastErrorCode: null,
  });
  useAssetStore.setState({ items: [], selectedGuid: null });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('genStore backends', () => {
  it('loadBackends 拉取 + configuredBackends 过滤(仅 configured=true)', async () => {
    vi.stubGlobal('fetch', mockForgeBackend({}, { '/api/forge/gen/backends': BACKENDS }));
    await useGenStore.getState().loadBackends();
    const s = useGenStore.getState();
    expect(s.backends).toHaveLength(2);
    expect(s.backendsLoaded).toBe(true);
    const configured = configuredBackends(s.backends);
    expect(configured.map((b) => b.id)).toEqual(['local-mock']);
  });
});

describe('genStore generate', () => {
  it('gen_image 成功 → candidates 进 modal,dialog 关闭,lastPrompt 记录', async () => {
    vi.stubGlobal('fetch', mockForgeBackend(toolMap(), { '/api/forge/gen/backends': BACKENDS }));
    useGenStore.getState().openDialog('Textures');
    await useGenStore.getState().generate({ prompt: 'wood 木纹', size: 256, n: 4 });
    const s = useGenStore.getState();
    expect(s.candidates).toHaveLength(4);
    expect(s.dialogOpen).toBe(false);
    expect(s.lastPrompt).toBe('wood 木纹');
    expect(s.lastError).toBeNull();
  });

  it('F10-RAG:openDialog 带 assetPath → generate 绑定资产路径进 gen_image 参数', async () => {
    const fetchMock = mockForgeBackend(toolMap(), { '/api/forge/gen/backends': BACKENDS });
    vi.stubGlobal('fetch', fetchMock);
    useGenStore.getState().openDialog('Textures', 'Textures/chair.png');
    expect(useGenStore.getState().bindAssetPath).toBe('Textures/chair.png');
    await useGenStore.getState().generate({
      prompt: '一把椅子',
      size: 256,
      n: 1,
      assetPath: useGenStore.getState().bindAssetPath ?? undefined,
    });
    const calls = fetchMock.mock.calls
      .filter((c) => (c[0] as string) === '/api/forge/mcp/call')
      .map((c) => {
        const init = c[1] as { body: string };
        return JSON.parse(init.body) as { tool: string; arguments: Record<string, unknown> };
      });
    const genCall = calls.find((c) => c.tool === 'mcp__gen-image__gen_image');
    expect(genCall).toBeDefined();
    expect(genCall!.arguments.prompt).toBe('一把椅子');
    expect(genCall!.arguments.assetPath).toBe('Textures/chair.png');
  });

  it('gen_image 工具级错误 → lastErrorCode 如实保留 GEN_BACKEND_NOT_CONFIGURED', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({
        ok: true,
        status: 200,
        json: async () => ({
          content: [
            {
              type: 'text',
              text: JSON.stringify({
                error: 'GEN_BACKEND_NOT_CONFIGURED',
                message: '无已配置生成后端(data/gen-backends.json 缺 enabled 条目)',
              }),
            },
          ],
          isError: true,
        }),
      })),
    );
    await useGenStore.getState().generate({ prompt: 'wood', size: 512, n: 4 });
    const s = useGenStore.getState();
    expect(s.candidates).toBeNull();
    expect(s.lastErrorCode).toBe('GEN_BACKEND_NOT_CONFIGURED');
    expect(s.lastError).toContain('无已配置生成后端');
  });
});

describe('genStore accept', () => {
  it('gen_accept 参数(destFolder/name slug)+ assetStore 刷新 + 选中新资产', async () => {
    const fetchMock = mockForgeBackend(toolMap(), { '/api/forge/gen/backends': BACKENDS });
    vi.stubGlobal('fetch', fetchMock);
    useGenStore.setState({ candidates: CANDIDATES, lastPrompt: 'wood 木纹', destFolder: 'Textures' });

    await useGenStore.getState().accept(CANDIDATES[0]);

    // gen_accept 调用断言:imageFileRef/destFolder/name=slug-seed。
    const calls = fetchMock.mock.calls.map((c) => {
      const init = c[1] as { body: string };
      return JSON.parse(init.body) as { tool: string; arguments: Record<string, unknown> };
    });
    const acceptCall = calls.find((c) => c.tool === 'mcp__gen-image__gen_accept');
    expect(acceptCall).toBeDefined();
    expect(acceptCall!.arguments).toEqual({
      imageFileRef: CANDIDATES[0].imageFileRef,
      destFolder: 'Textures',
      name: 'wood-40',
    });
    // assetStore 刷新(asset_list 被再调)+ 选中新资产 guid。
    expect(calls.filter((c) => c.tool === 'mcp__asset-pipeline__asset_list').length).toBeGreaterThan(0);
    expect(useAssetStore.getState().items).toHaveLength(1);
    expect(useAssetStore.getState().selectedGuid).toBe('g-new');
    // 已入库记录(不互斥,可再 accept 其他候选)。
    expect(useGenStore.getState().acceptedRefs).toContain(CANDIDATES[0].imageFileRef);
  });

  it('slugifyName:ascii slug + seed(中文剥离,空 → gen)', () => {
    expect(slugifyName('wood 木纹', 42)).toBe('wood-42');
    expect(slugifyName('Oak Floor!', 7)).toBe('oak-floor-7');
    expect(slugifyName('木纹', 1)).toBe('gen-1');
  });
});
