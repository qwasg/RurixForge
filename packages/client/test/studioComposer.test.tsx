import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import StudioComposer from '@/components/studio/StudioComposer';
import { useGenStore } from '@/lib/genStore';
import { useStudioStore, type StudioNode } from '@/lib/studioStore';
import { mockForgeBackend } from './forgeMock';

/**
 * StudioComposer 后端清单诚实面(F10-RAG 修复):
 * 清单拉取失败 → 如实错误条 + 重试(不冒充「未配置后端」);模型弹层如实标注失败原因;
 * 恢复后重试成功 → 错误条消失、后端可选。
 */

const IMAGE_NODE: StudioNode = {
  id: 'n1',
  preset: 'texture',
  name: '贴图 1',
  pos: [0, 0],
  prompt: '木纹',
  params: {},
  versions: [],
  currentVersionId: null,
};

const BACKENDS = {
  backends: [
    {
      id: 'remote-openai-compatible',
      kind: 'remote',
      configured: true,
      endpointSet: true,
      capabilities: { kinds: ['text2img'] },
    },
  ],
};

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
  useStudioStore.setState({
    lastError: null,
    busyIds: [],
    nodeRuns: {},
    pendingPermission: null,
    readonlyWorkspaceIds: [],
    includeLibrary: false,
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('<StudioComposer /> 后端清单诚实面', () => {
  it('清单拉取失败 → 如实错误条 + 重试;不冒充「未配置后端」;恢复后模型可选', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: unknown) => {
        const u = String(url);
        if (u.includes('/api/forge/gen/backends')) {
          return {
            ok: false,
            status: 502,
            json: async () => ({
              error: { code: 'UPSTREAM_UNREACHABLE', message: 'agentd 不可达: http://127.0.0.1:8103' },
            }),
          } as Response;
        }
        return {
          ok: true,
          status: 200,
          json: async () => ({ workspaces: [] }),
          text: async () => '{"workspaces":[]}',
        } as Response;
      }),
    );

    render(<StudioComposer node={IMAGE_NODE} />);

    // 如实错误条出现(消息透传);「未配置后端」提示不出现。
    const banner = await screen.findByTestId('studio-backends-error');
    expect(banner).toHaveTextContent('生成后端清单拉取失败');
    expect(banner).toHaveTextContent('agentd 不可达');
    expect(screen.queryByText(/未配置.*生成后端/)).toBeNull();

    // 模型弹层:如实标注拉取失败,而非「无支持该能力的后端条目」。
    fireEvent.click(screen.getByTestId('studio-model-btn'));
    const picker = await screen.findByTestId('studio-model-picker');
    expect(picker).toHaveTextContent('后端清单拉取失败');
    expect(picker).not.toHaveTextContent('无支持该能力的后端条目');
    // 关弹层(外部 mousedown)。
    fireEvent.mouseDown(document.body);
    await waitFor(() => expect(screen.queryByTestId('studio-model-picker')).toBeNull());

    // 重试:后端恢复 → 错误条消失,模型弹层列出已配置后端。
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({}, { '/api/forge/gen/backends': BACKENDS, '/api/forge/workspaces': { workspaces: [] } }),
    );
    fireEvent.click(screen.getByTestId('studio-backends-retry'));
    await waitFor(() => expect(screen.queryByTestId('studio-backends-error')).toBeNull());
    fireEvent.click(screen.getByTestId('studio-model-btn'));
    const picker2 = await screen.findByTestId('studio-model-picker');
    expect(picker2).toHaveTextContent('OpenAI 兼容图像');
  });

  it('清单正常但无已配置后端 → 维持「未配置」提示(不冒充失败)', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {},
        {
          '/api/forge/gen/backends': {
            backends: [{ id: 'local-mock', kind: 'local', configured: false, endpointSet: false }],
          },
          '/api/forge/workspaces': { workspaces: [] },
        },
      ),
    );
    render(<StudioComposer node={IMAGE_NODE} />);
    await waitFor(() => expect(screen.getByText(/未配置.*生成后端/)).toBeInTheDocument());
    expect(screen.queryByTestId('studio-backends-error')).toBeNull();
  });
});
