import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import AssetsPanel from '@/components/editor/AssetsPanel';
import { useAssetStore } from '@/lib/assetStore';
import { useGenStore } from '@/lib/genStore';
import { mockForgeBackend } from './forgeMock';

/**
 * F5 wave.3 Assets 生成链:右键「Generate...」→ GenerateDialog → prompt 提交 →
 * CandidatesModal 4 卡 → Accept → gen_accept 调用断言 + 资产列表刷新。
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

const CANDIDATES = [0, 1, 2, 3].map((i) => ({
  imageFileRef: `.forge/tmp/gen/gen-9-5${i}-${i}.png`,
  seed: 50 + i,
  backendId: 'local-mock',
  dataUrl: 'data:image/png;base64,AAAA',
}));

const ITEM = {
  path: 'Textures/wood_albedo.png',
  guid: 'g-wood',
  type: 'texture',
  size: 10,
  description: '橡木木纹贴图,暖色调',
  tags: ['木纹'],
};

function setupFetch() {
  const state = { accepted: false };
  const fetchMock = mockForgeBackend(
    {
      asset_list: { assets: [ITEM] },
      asset_build_status: { items: [] },
      gen_image: { candidates: CANDIDATES },
      gen_accept: { assetPath: 'Textures/wood-50.png', guid: 'g-new' },
    },
    { '/api/forge/gen/backends': BACKENDS },
  );
  return { fetchMock, state };
}

function toolCalls(fetchMock: ReturnType<typeof mockForgeBackend>) {
  return fetchMock.mock.calls
    .filter((c) => (c[0] as string) === '/api/forge/mcp/call')
    .map((c) => {
      const init = c[1] as { body: string };
      return JSON.parse(init.body) as { tool: string; arguments: Record<string, unknown> };
    });
}

beforeEach(() => {
  useAssetStore.setState({ items: [], status: {}, selectedGuid: null, currentFolder: '' });
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
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<AssetsPanel /> F5 生成链', () => {
  it('右键 Generate → dialog 开;prompt 提交 → modal 4 卡;Accept → gen_accept 断言', async () => {
    const { fetchMock } = setupFetch();
    vi.stubGlobal('fetch', fetchMock);

    render(<AssetsPanel />);

    // 资产条目渲染后右键开菜单。
    const item = await screen.findByText('wood_albedo.png');
    const card = item.closest('[data-asset-guid]')!;
    fireEvent.contextMenu(card);
    const genItem = await screen.findByText('Generate...');
    fireEvent.click(genItem);

    // GenerateDialog 开;后端清单加载后 submit 可用。
    const dialog = await screen.findByText('生成图像候选');
    expect(dialog).toBeInTheDocument();
    expect(document.querySelector('[data-gen-dialog]')).not.toBeNull();
    await waitFor(() =>
      expect(
        (document.querySelector('[data-gen-backend]') as HTMLSelectElement).options.length,
      ).toBe(1),
    );

    // F10-RAG:右键资产 → 对话框显示绑定行(有简介 → 并入提示词)。
    const bindRow = document.querySelector('[data-gen-bind-asset]');
    expect(bindRow).not.toBeNull();
    expect(bindRow!.textContent).toContain('wood_albedo.png');
    expect(bindRow!.textContent).toContain('简介+标签将并入提示词');

    // 填 prompt 提交。
    fireEvent.change(document.querySelector('[data-gen-prompt]')!, {
      target: { value: 'wood 木纹' },
    });
    const submit = document.querySelector('[data-gen-submit]') as HTMLButtonElement;
    await waitFor(() => expect(submit.disabled).toBe(false));
    fireEvent.click(submit);

    // gen_image 调用断言(n=4 默认,prompt 透传,assetPath 绑定右键资产)。
    await waitFor(() =>
      expect(toolCalls(fetchMock).some((c) => c.tool === 'mcp__gen-image__gen_image')).toBe(true),
    );
    const genCall = toolCalls(fetchMock).find((c) => c.tool === 'mcp__gen-image__gen_image')!;
    expect(genCall.arguments.prompt).toBe('wood 木纹');
    expect(genCall.arguments.n).toBe(4);
    expect(genCall.arguments.backend).toBe('local-mock');
    expect(genCall.arguments.assetPath).toBe('Textures/wood_albedo.png');

    // CandidatesModal 4 卡(dataUrl img + seed 标注)。
    await waitFor(() =>
      expect(document.querySelectorAll('[data-gen-candidate]')).toHaveLength(4),
    );
    expect(screen.getByText(/seed 50 · local-mock/)).toBeInTheDocument();

    // 点第一张 Accept → gen_accept(destFolder=Textures,name=slug-seed)。
    const acceptBtns = document.querySelectorAll('[data-gen-accept]');
    fireEvent.click(acceptBtns[0]);
    await waitFor(() =>
      expect(toolCalls(fetchMock).some((c) => c.tool === 'mcp__gen-image__gen_accept')).toBe(true),
    );
    const acceptCall = toolCalls(fetchMock).find((c) => c.tool === 'mcp__gen-image__gen_accept')!;
    expect(acceptCall.arguments).toEqual({
      imageFileRef: CANDIDATES[0].imageFileRef,
      destFolder: 'Textures',
      name: 'wood-50',
    });
    // 资产列表刷新 + 新资产选中;卡片标「已入库」。
    await waitFor(() => expect(screen.getByText('已入库')).toBeInTheDocument());
    expect(useAssetStore.getState().selectedGuid).toBe('g-new');
    // 其余候选仍可 accept(不互斥)。
    expect(document.querySelectorAll('[data-gen-candidate]')).toHaveLength(4);
  });

  it('无已配置后端 → dialog 如实错误条(GEN_BACKEND_NOT_CONFIGURED + 设置页指引)', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        { asset_list: { assets: [ITEM] }, asset_build_status: { items: [] } },
        {
          '/api/forge/gen/backends': {
            backends: [
              { id: 'local-mock', kind: 'local', configured: false, endpointSet: false },
              { id: 'remote-openai-compatible', kind: 'remote', configured: false, endpointSet: false },
            ],
          },
        },
      ),
    );
    render(<AssetsPanel />);
    const item = await screen.findByText('wood_albedo.png');
    fireEvent.contextMenu(item.closest('[data-asset-guid]')!);
    fireEvent.click(await screen.findByText('Generate...'));

    await waitFor(() => expect(document.querySelector('[data-gen-error]')).not.toBeNull());
    expect(document.querySelector('[data-gen-error]')!.textContent).toContain(
      'GEN_BACKEND_NOT_CONFIGURED',
    );
    expect(document.querySelector('[data-gen-error]')!.textContent).toContain('Generation');
    expect((document.querySelector('[data-gen-submit]') as HTMLButtonElement).disabled).toBe(true);
  });
});
