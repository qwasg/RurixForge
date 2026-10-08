import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ViewportCanvas } from '@/components/editor/ViewportCanvas';
import { useEditorStore } from '@/lib/editorStore';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import type { ForgeAPI } from '@/lib/bridge';
import type { RenderBackendInfo, RenderCapabilities } from '@/lib/forgeApi';
import { mockForgeBackend } from './forgeMock';

const initialEditor = useEditorStore.getState();

const INFO: RenderBackendInfo = {
  renderBackend: 'godot',
  method: 'mobile',
  driver: 'd3d12',
  source: 'forge.toml',
  ready: true,
  deviceName: '真实 GPU',
  versions: { engineHost: '0.1.0', godot: '4.7.2', gdext: '0.5.5' },
  frameChannels: { l2: 'rd_async', l1Available: true, l1Active: false, l1Reason: '测试降级' },
};

function capabilities(sharedD3d12: boolean): RenderCapabilities {
  return {
    renderBackend: 'godot',
    pipelined: true,
    legs: ['sprite_mesh', 'model'],
    preview: true,
    particles: false,
    frameExits: { cpuRgba8: true, sharedD3d12, zeroCopy: false },
    stats: { nonzero: true, triangles: true, truncated: false, meshFallbacks: false, meshClasses: false },
    maxDraws: { spriteMesh: null, model: null, sentinelsV6: null },
    maxSize: { rpc: [1920, 1080], stream: [1280, 720] },
    coverage: { unsupported: [{ feature: 'Sprite.flip', reason: '当前渲染腿尚未接入' }] },
  };
}

const FRAME = {
  width: 16,
  height: 16,
  format: 'rgba8',
  pixelsB64: btoa(String.fromCharCode(...new Array(16 * 16 * 4).fill(0))),
  deviceName: '真实 GPU',
  draws: 1,
  frames: 1,
  nonZeroPixels: 0,
  truncated: false,
};

class IdleWebSocket {
  static OPEN = 1;
  readyState = 0;
  binaryType = 'blob';
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  send() {}
  close() { this.readyState = 3; }
}

function installDesktopBridge(reportBounds: NonNullable<ForgeAPI['viewport']>['reportBounds']) {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({
    x: 0, y: 0, left: 0, top: 0, right: 960, bottom: 540, width: 960, height: 540,
    toJSON: () => ({}),
  } as DOMRect);
  window.forgeAPI = {
    win: {
      minimize: () => {},
      toggleMaximize: () => {},
      close: () => {},
      onMaximizedChanged: () => () => {},
    },
    viewport: { reportBounds },
    platform: 'win32',
  };
}

function renderBackendMocks(
  caps: RenderCapabilities,
  info: RenderBackendInfo = INFO,
  configContent = '[render]\\nbackend = "godot"\\nmethod = "mobile"\\ndriver = "d3d12"\\n',
) {
  const backend = mockForgeBackend({
    render_backend_info: info,
    render_capabilities: caps,
    viewport_stream_info: { wsUrl: 'ws://127.0.0.1:9/stream?token=test', proto: 1 },
    viewport_get_camera: { target: [0, 0, 0], yaw: 0, pitch: 0, dist: 10, fovY: 50, ortho: true, orthoSize: 5 },
    viewport_frame: FRAME,
  }, {
    '/api/forge/workspace/file': {
      path: 'forge.toml',
      content: configContent,
    },
  });
  vi.stubGlobal('fetch', vi.fn(async (url: unknown, init?: RequestInit) => {
    if (String(url).startsWith('data:')) {
      return { arrayBuffer: async () => new Uint8Array(FRAME.width * FRAME.height * 4).buffer } as Response;
    }
    return backend(url, init as { body?: string });
  }));
}

beforeEach(() => {
  cleanup();
  localStorage.removeItem('forge:activeWorkspace');
  useWorkspaceStore.setState({ activeWorkspaceId: null });
  useEditorStore.setState(initialEditor, true);
  useEditorStore.setState({
    entities: [{
      id: 1,
      name: 'Model',
      transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
      components: [{ type: 'ModelRenderer', enabled: true, props: {} }],
    }],
    camera: { target: [0, 0, 0], yaw: 0, pitch: 0, dist: 10, fovY: 50, ortho: true, orthoSize: 5 },
    sceneMode: '3d',
    renderBackendInfo: INFO,
    renderCapabilities: capabilities(true),
    renderConfigMismatch: null,
    renderStatusError: null,
    renderStatusWorkspaceId: null,
    renderStatusLoaded: true,
    viewportDegraded: null,
  });
  vi.stubGlobal('WebSocket', IdleWebSocket);
  const context = {
    putImageData: vi.fn(),
    scale: vi.fn(),
    clearRect: vi.fn(),
    beginPath: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    stroke: vi.fn(),
    set strokeStyle(_value: string) {},
    set lineWidth(_value: number) {},
  };
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(context as unknown as CanvasRenderingContext2D);
});

afterEach(() => {
  cleanup();
  delete window.forgeAPI;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('<ViewportCanvas /> 渲染后端与 presenter 接缝', () => {
  it('工作区能力尚未刷新时，不使用旧工作区能力打开原生视口', async () => {
    const reportBounds = vi.fn();
    installDesktopBridge(reportBounds);
    renderBackendMocks(capabilities(true));
    useWorkspaceStore.setState({ activeWorkspaceId: 'new-workspace' });
    useEditorStore.setState({
      renderStatusWorkspaceId: 'old-workspace',
      refreshRenderBackend: async () => {},
    });
    render(<ViewportCanvas />);
    await waitFor(() => expect(reportBounds).toHaveBeenCalled());
    expect(reportBounds.mock.calls.every(([bounds]) => bounds.visible === false)).toBe(true);
    expect(reportBounds).toHaveBeenLastCalledWith(expect.objectContaining({ workspaceId: 'new-workspace' }));
  });


  it('共享选择按真实能力决定，不再按 ModelRenderer 硬编码；状态文字在 presenter bounds 外', async () => {
    const reportBounds = vi.fn();
    installDesktopBridge(reportBounds);
    renderBackendMocks(capabilities(true));

    render(<ViewportCanvas />);

    await waitFor(() => expect(reportBounds).toHaveBeenCalled());
    expect(reportBounds).toHaveBeenLastCalledWith(expect.objectContaining({ workspaceId: null }));
    expect(screen.getByTestId('render-backend-label')).toHaveTextContent('godot · mobile · d3d12 · 真实 GPU · 就绪');
    expect(screen.getByTestId('render-capability-summary')).toHaveTextContent('D3D12 共享 支持');
    expect(screen.getByTestId('render-capability-details')).toHaveTextContent('当前渲染腿尚未接入');
    const detailsBody = screen.getByTestId('render-capability-details').querySelector('div');
    expect(detailsBody).not.toHaveClass('absolute');
    expect(detailsBody).not.toHaveClass('fixed');
    expect(screen.getByTestId('viewport-render-status').contains(screen.getByRole('application', { name: 'Viewport 画布' }))).toBe(false);
    expect(screen.getByTestId('viewport-render-status').contains(screen.getByTestId('viewport-frame-stats'))).toBe(false);

    await waitFor(() => expect(useEditorStore.getState().renderConfigMismatch).toBeNull());
  });

  it('共享能力关闭或 2D 网格需要本地叠加时走网页帧，不污染引擎帧', async () => {
    const reportBounds = vi.fn();
    installDesktopBridge(reportBounds);
    renderBackendMocks(capabilities(false));
    useEditorStore.setState({ renderCapabilities: capabilities(false) });

    const { rerender } = render(<ViewportCanvas />);
    expect(reportBounds).toHaveBeenLastCalledWith(expect.objectContaining({ visible: false }));

    renderBackendMocks(capabilities(true));
    useEditorStore.setState({ sceneMode: '2d', renderCapabilities: capabilities(true), camera: { ...useEditorStore.getState().camera!, ortho: true } });
    rerender(<ViewportCanvas />);
    await waitFor(() => expect(screen.getByTestId('viewport-input-hint')).toHaveTextContent('2D'));
    expect(reportBounds).toHaveBeenLastCalledWith(expect.objectContaining({ visible: false }));
    expect(screen.getByRole('application', { name: 'Viewport 画布' })).toBeInTheDocument();
  });

  it('配置不一致仅显示重启提示；保留运行中的后端身份', async () => {
    const running: RenderBackendInfo = { ...INFO, renderBackend: 'rurix', method: null, driver: null, source: 'default' };
    useEditorStore.setState({
      renderConfigMismatch: null,
      renderBackendInfo: running,
      renderCapabilities: capabilities(true),
    });
    const reportBounds = vi.fn();
    installDesktopBridge(reportBounds);
    renderBackendMocks(capabilities(true), running, '[render]\\nbackend = "godot"\\nmethod = "forward_plus"\\ndriver = "d3d12"\\n');
    render(<ViewportCanvas />);

    expect(await screen.findByTestId('render-config-mismatch')).toHaveTextContent('未自动切换');
    expect(screen.getByTestId('render-backend-label')).toHaveTextContent('rurix');
  });
});
