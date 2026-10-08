import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import StudioComposer from '@/components/studio/StudioComposer';
import { useGenStore, type GenBackendInfo } from '@/lib/genStore';
import { useStudioStore, type StudioNode } from '@/lib/studioStore';
import { mockForgeBackend } from './forgeMock';

const H3: GenBackendInfo = {
  id: 'aliyun-minimax-video', kind: 'remote', configured: true, endpointSet: true,
  capabilities: {
    kinds: ['text2video', 'image2video'], resolutions: ['768p', '2k'],
    defaultResolution: '768p', minDurationSec: 4, maxDurationSec: 15, maxPromptChars: 7000,
  },
};
const GENERIC: GenBackendInfo = {
  id: 'remote-video-compatible', kind: 'remote', configured: true, endpointSet: true,
  capabilities: { kinds: ['text2video', 'image2video'] },
};
const LOCAL_H3: GenBackendInfo = {
  id: 'comfyui-minimax-h3', kind: 'local', configured: true, endpointSet: true,
  capabilities: {
    kinds: ['text2video', 'image2video'], aspects: ['16:9', '9:16', '1:1'],
    resolutions: ['352p', '480p', '768p'], defaultResolution: '352p',
    defaultDurationSec: 2, minDurationSec: 2, maxDurationSec: 15,
  },
};

function LiveComposer() {
  const node = useStudioStore((s) => s.nodes[0]);
  return node ? <StudioComposer node={node} /> : null;
}

function setup(backends: GenBackendInfo[], extra: Partial<StudioNode> = {}) {
  const requests: Record<string, unknown>[] = [];
  const node: StudioNode = {
    id: 'video-test', preset: 'video', name: '视频 1', pos: [0, 0], prompt: '海边日出',
    params: { aspect: '16:9', resolution: '720p', durationSec: 5 },
    versions: [], currentVersionId: null, ...extra,
  };
  useStudioStore.setState({ nodes: [node] });
  vi.stubGlobal('fetch', mockForgeBackend({}, {
    '/api/forge/gen/backends': { backends },
    '/api/forge/workspaces': { workspaces: [] },
    '/api/forge/gen/video': (init?: { body?: string }) => {
      requests.push(JSON.parse(init?.body ?? '{}') as Record<string, unknown>);
      return { backendId: H3.id, artifacts: [] };
    },
  }));
  render(<LiveComposer />);
  return requests;
}

beforeEach(() => {
  localStorage.clear();
  useGenStore.setState({ backends: [], backendsLoaded: false, backendsError: null });
  useStudioStore.setState({
    nodes: [], edges: [], seq: 1, workspaceId: null, lastError: null, busyIds: [],
    nodeRuns: {}, pendingPermission: null, readonlyWorkspaceIds: [], includeLibrary: false,
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('StudioComposer 视频后端能力', () => {
  it('本地 H3 自动采用 352p/2s 预览并提交所选项目参考图', async () => {
    const requests = setup([LOCAL_H3, H3], {
      params: { aspect: '16:9', resolution: '720p', durationSec: 5, refAssetPath: 'Concepts/hero.png' },
    });
    expect(await screen.findByTestId('studio-res-352p')).toHaveTextContent('352p 预览');
    await waitFor(() => expect(useStudioStore.getState().nodes[0].params.durationSec).toBe(2));
    expect(screen.getByTestId('studio-model-btn')).toHaveTextContent('MiniMax H3 · 本地(自动)');
    expect(screen.queryByTestId('studio-res-2k')).toBeNull();
    fireEvent.click(screen.getByTestId('studio-send'));
    await waitFor(() => expect(requests).toHaveLength(1));
    expect(requests[0]).toMatchObject({ resolution: '352p', durationSec: 2, imageRef: 'Content/Concepts/hero.png' });
    expect(requests[0]).not.toHaveProperty('backend');
  });

  it('本地注册优先级不覆盖显式云端选择,切到本地时重置云端的合法但昂贵参数', async () => {
    const requests = setup([LOCAL_H3, H3], {
      params: { backend: H3.id, aspect: '9:16', resolution: '768p', durationSec: 15 },
    });
    await screen.findByTestId('studio-res-2k');
    expect(screen.queryByTestId('studio-res-352p')).toBeNull();
    expect(screen.getByTestId('studio-dur-15')).toHaveClass('border-acc');
    fireEvent.click(screen.getByTestId('studio-model-btn'));
    expect(screen.getByTestId(`studio-model-opt-${LOCAL_H3.id}`)).toHaveTextContent('MiniMax H3 · 本地');
    fireEvent.click(screen.getByTestId(`studio-model-opt-${LOCAL_H3.id}`));
    await waitFor(() => expect(useStudioStore.getState().nodes[0].params).toMatchObject({
      backend: LOCAL_H3.id, resolution: '352p', durationSec: 2, aspect: '9:16',
    }));
    fireEvent.click(screen.getByTestId('studio-res-480p'));
    fireEvent.click(screen.getByTestId('studio-dur-7'));
    fireEvent.click(screen.getByTestId('studio-send'));
    await waitFor(() => expect(requests).toHaveLength(1));
    expect(requests[0]).toMatchObject({ backend: LOCAL_H3.id, resolution: '480p', durationSec: 7 });
  });

  it('从显式云端切回自动时采用本地默认值,已保存的本地自选参数可以保留', async () => {
    setup([LOCAL_H3, H3], {
      params: { backend: LOCAL_H3.id, aspect: '1:1', resolution: '480p', durationSec: 8 },
    });
    await screen.findByTestId('studio-res-352p');
    expect(screen.getByTestId('studio-res-480p')).toHaveClass('border-acc');
    expect(screen.getByTestId('studio-dur-8')).toHaveClass('border-acc');
    fireEvent.click(screen.getByTestId('studio-model-btn'));
    fireEvent.click(screen.getByTestId(`studio-model-opt-${H3.id}`));
    await screen.findByTestId('studio-res-2k');
    fireEvent.click(screen.getByTestId('studio-dur-15'));
    fireEvent.click(screen.getByTestId('studio-model-btn'));
    fireEvent.click(screen.getByTestId('studio-model-auto'));
    await waitFor(() => expect(useStudioStore.getState().nodes[0].params).toMatchObject({
      backend: '', resolution: '352p', durationSec: 2,
    }));
  });

  it('画板直接生成也加载本地能力,并保留上游已入库参考图的路径与视频产物', async () => {
    const requests: Record<string, unknown>[] = [];
    vi.stubGlobal('fetch', mockForgeBackend({}, {
      '/api/forge/gen/backends': { backends: [LOCAL_H3, H3] },
      '/api/forge/gen/video': (init?: { body?: string }) => {
        requests.push(JSON.parse(init?.body ?? '{}'));
        return { backendId: LOCAL_H3.id, artifacts: [{ fileRef: '.forge/tmp/gen/local-h3.mp4', mime: 'video/mp4' }] };
      },
    }));
    const upstream = useStudioStore.getState().addNode('concept')!;
    const target = useStudioStore.getState().addNode('video')!;
    useStudioStore.setState((s) => ({ nodes: s.nodes.map((n) => n.id === upstream ? {
      ...n, currentVersionId: 'reference-v1', versions: [{ id: 'reference-v1', createdAt: 1,
        backendId: 'image', prompt: '角色', assetPath: 'Concepts/hero.png', fileRef: '.forge/tmp/gen/hero.png' }],
    } : n) }));
    useStudioStore.getState().addEdge(upstream, target);
    useStudioStore.getState().setPrompt(target, '角色向右行走');
    await useStudioStore.getState().generate(target);
    expect(requests).toHaveLength(1);
    expect(requests[0]).toMatchObject({ resolution: '352p', durationSec: 2, imageRef: 'Content/Concepts/hero.png' });
    expect(useStudioStore.getState().lastError).toBeNull();
    expect(useStudioStore.getState().nodes.find((n) => n.id === target)?.versions[0]).toMatchObject({
      backendId: LOCAL_H3.id, fileRef: '.forge/tmp/gen/local-h3.mp4', mime: 'video/mp4',
    });
  });

  it('xzapi H3 normalizes to 2k/15s and forwards a selected reference image', async () => {
    const requests = setup([{
      id: 'xzapi-video', kind: 'remote', configured: true, endpointSet: true,
      capabilities: { kinds: ['text2video', 'image2video'], aspects: ['16:9', '9:16'],
        resolutions: ['2k'], defaultResolution: '2k', minDurationSec: 15, maxDurationSec: 15, maxPromptChars: 5000 },
    }], { params: { aspect: '1:1', resolution: '720p', durationSec: 5, refAssetPath: 'Content/reference.png' } });
    await screen.findByTestId('studio-res-2k');
    await waitFor(() => expect(useStudioStore.getState().nodes[0].params).toMatchObject({
      aspect: '16:9', resolution: '2k', durationSec: 15,
    }));
    expect(screen.queryByTestId('studio-aspect-1:1')).toBeNull();
    expect(screen.queryByTestId('studio-dur-5')).toBeNull();
    fireEvent.click(screen.getByTestId('studio-send'));
    await waitFor(() => expect(requests).toHaveLength(1));
    expect(requests[0]).toMatchObject({ imageRef: 'Content/reference.png', aspect: '16:9', resolution: '2k', durationSec: 15 });
  });

  it('自动模式跳过未配置后端,将历史 720p 草稿与真实请求同步到 H3 的 768p', async () => {
    const requests = setup([{ ...GENERIC, configured: false }, H3]);
    const resolution = await screen.findByTestId('studio-res-768p');
    await waitFor(() => expect(useStudioStore.getState().nodes[0].params.resolution).toBe('768p'));
    expect(resolution).toHaveClass('border-acc');
    expect(screen.queryByTestId('studio-res-720p')).toBeNull();
    expect(screen.queryByTestId('studio-res-1080p')).toBeNull();
    expect(screen.getByTestId('studio-prompt')).toHaveAttribute('maxlength', '7000');
    fireEvent.click(screen.getByTestId('studio-send'));
    await waitFor(() => expect(requests).toHaveLength(1));
    expect(requests[0]).toMatchObject({ prompt: '海边日出', resolution: '768p', durationSec: 5 });
    expect(requests[0]).not.toHaveProperty('backend');
  });

  it('显式 H3 优先于自动候选,允许选择 2k 与 4–15 秒并提交所选值', async () => {
    const requests = setup([GENERIC, H3], {
      params: { backend: H3.id, aspect: '16:9', resolution: '720p', durationSec: 20 },
    });
    await screen.findByTestId('studio-res-768p');
    await waitFor(() => expect(useStudioStore.getState().nodes[0].params.durationSec).toBe(5));
    expect(screen.getByTestId('studio-dur-4')).toBeInTheDocument();
    expect(screen.getByTestId('studio-dur-7')).toBeInTheDocument();
    fireEvent.click(screen.getByTestId('studio-res-2k'));
    fireEvent.click(screen.getByTestId('studio-dur-15'));
    expect(screen.getByTestId('studio-res-2k')).toHaveClass('border-acc');
    expect(screen.getByTestId('studio-dur-15')).toHaveClass('border-acc');
    fireEvent.click(screen.getByTestId('studio-send'));
    await waitFor(() => expect(requests).toHaveLength(1));
    expect(requests[0]).toMatchObject({ backend: H3.id, resolution: '2k', durationSec: 15 });
  });

  it('切回通用视频后端时恢复其参数选项与合法默认值', async () => {
    const requests = setup([H3, GENERIC]);
    await screen.findByTestId('studio-res-768p');
    fireEvent.click(screen.getByTestId('studio-dur-7'));
    fireEvent.click(screen.getByTestId('studio-model-btn'));
    fireEvent.click(screen.getByTestId(`studio-model-opt-${GENERIC.id}`));
    await waitFor(() => expect(useStudioStore.getState().nodes[0].params.resolution).toBe('720p'));
    expect(screen.queryByTestId('studio-res-768p')).toBeNull();
    expect(screen.queryByTestId('studio-dur-7')).toBeNull();
    expect(screen.getByTestId('studio-prompt')).toHaveAttribute('maxlength', '7500');
    fireEvent.click(screen.getByTestId('studio-send'));
    await waitFor(() => expect(requests).toHaveLength(1));
    expect(requests[0]).toMatchObject({ backend: GENERIC.id, resolution: '720p', durationSec: 5 });
  });

  it('保留超长旧草稿并阻止按钮及快捷键发送,缩短后可提交', async () => {
    const prompt = '海'.repeat(7001);
    const requests = setup([H3], { prompt });
    await screen.findByTestId('studio-res-768p');
    expect(screen.getByTestId('studio-prompt')).toHaveValue(prompt);
    expect(screen.getByTestId('studio-send')).toBeDisabled();
    expect(screen.getByText(/请缩短提示词/)).toBeInTheDocument();
    fireEvent.keyDown(screen.getByTestId('studio-prompt'), { key: 'Enter', ctrlKey: true });
    expect(requests).toHaveLength(0);
    fireEvent.change(screen.getByTestId('studio-prompt'), { target: { value: '海'.repeat(7000) } });
    expect(screen.getByTestId('studio-send')).toBeEnabled();
    fireEvent.click(screen.getByTestId('studio-send'));
    await waitFor(() => expect(requests).toHaveLength(1));
    expect(String(requests[0].prompt)).toHaveLength(7000);
  });
});
