import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import TitleBar from '@/components/shell/TitleBar';
import AssetsPanel from '@/components/editor/AssetsPanel';
import { ViewportCanvas } from '@/components/editor/ViewportCanvas';
import { bridge, isDesktopBridge, MOCK_FORGE_API, type ForgeAPI } from '@/lib/bridge';
import { useAssetStore } from '@/lib/assetStore';
import { useEditorStore } from '@/lib/editorStore';
import { mockForgeBackend } from './forgeMock';

/**
 * F8 wave.3(G-F8-3 浏览器通路门)client 侧断言:
 * - bridge 浏览器环境(无 window.forgeAPI):MOCK 各面诚实禁用语义,无一处抛未捕获异常;
 * - TitleBar:窗口三钮浏览器隐藏(渲染 null),File>退出 桌面专有;
 * - AssetsPanel:pickImport/showInFolder 菜单禁用 + 「仅桌面端可用」tooltip;
 * - ViewportCanvas 浏览器回退腿(canvas readback):任何帧通道错误诚实上屏降级卡,
 *   禁止黑屏充绿;成功帧照常上屏。
 */

function stubDesktopBridge(close = vi.fn()) {
  const api: ForgeAPI = {
    win: {
      minimize: vi.fn(),
      toggleMaximize: vi.fn(),
      close,
      onMaximizedChanged: vi.fn(() => () => {}),
    },
    platform: 'win32',
  };
  window.forgeAPI = api;
  return api;
}

beforeEach(() => {
  delete window.forgeAPI;
});

afterEach(() => {
  cleanup();
  delete window.forgeAPI;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('bridge() 浏览器环境(无 window.forgeAPI)', () => {
  it('isDesktopBridge()=false;bridge() 回退 MOCK(platform=web)', () => {
    expect(isDesktopBridge()).toBe(false);
    expect(bridge()).toBe(MOCK_FORGE_API);
    expect(bridge().platform).toBe('web');
  });

  it('win.* 全部 no-op 不抛异常 + console.info 诚实说明', () => {
    const info = vi.spyOn(console, 'info').mockImplementation(() => {});
    expect(() => {
      bridge().win.minimize();
      bridge().win.toggleMaximize();
      bridge().win.close();
    }).not.toThrow();
    expect(info).toHaveBeenCalledTimes(3);
    expect(info.mock.calls.every((c) => String(c[0]).includes('仅桌面端'))).toBe(true);
    const off = bridge().win.onMaximizedChanged(() => {});
    expect(typeof off).toBe('function');
    expect(() => off()).not.toThrow();
  });

  it('assets.pickImport=rejected promise 诚实禁用;showInFolder=no-op 不抛异常', async () => {
    const info = vi.spyOn(console, 'info').mockImplementation(() => {});
    await expect(bridge().assets?.pickImport?.()).rejects.toThrow('仅桌面端可用');
    expect(() => bridge().assets?.showInFolder?.('Textures/wood_albedo.png')).not.toThrow();
    expect(info).toHaveBeenCalledTimes(2);
  });

  it('viewport.reportBounds no-op 不抛异常(回退腿照常轮询)', () => {
    expect(() =>
      bridge().viewport?.reportBounds?.({ x: 0, y: 0, w: 16, h: 16, dpr: 1, visible: true }),
    ).not.toThrow();
  });

  it('桌面环境(stub window.forgeAPI):bridge() 直通 + isDesktopBridge()=true', () => {
    const api = stubDesktopBridge();
    expect(isDesktopBridge()).toBe(true);
    expect(bridge()).toBe(api);
    bridge().win.close();
    expect(api.win.close).toHaveBeenCalledTimes(1);
  });
});

describe('<TitleBar /> 浏览器环境', () => {
  it('窗口三钮渲染 null;File 菜单无「退出」且无悬空分隔线', () => {
    render(<TitleBar />);
    expect(screen.queryByLabelText('Minimize')).toBeNull();
    expect(screen.queryByLabelText('Maximize')).toBeNull();
    expect(screen.queryByLabelText('Close')).toBeNull();
    // 菜单与搜索胶囊仍在(壳其余功能不受影响)
    fireEvent.click(screen.getByTestId('menu-file'));
    expect(screen.getByText('新建会话')).toBeInTheDocument();
    expect(screen.getByText('设置')).toBeInTheDocument();
    expect(screen.queryByText('退出')).toBeNull();
    // 恰 3 个 menuitem(新建会话/打开编辑器/设置)——桌面专有行与其前分隔线一并裁掉
    expect(screen.getAllByRole('menuitem')).toHaveLength(3);
  });

  it('桌面环境:三钮渲染可点 + File 菜单含「退出」(回归防退化)', () => {
    const api = stubDesktopBridge();
    render(<TitleBar />);
    fireEvent.click(screen.getByLabelText('Minimize'));
    expect(api.win.minimize).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByLabelText('Close'));
    expect(api.win.close).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId('menu-file'));
    expect(screen.getAllByRole('menuitem')).toHaveLength(4);
    fireEvent.click(screen.getByText('退出'));
    expect(api.win.close).toHaveBeenCalledTimes(2);
  });
});

describe('<AssetsPanel /> 浏览器禁用态接线', () => {
  const ITEM = { path: 'Textures/wood_albedo.png', guid: 'g-wood', type: 'texture', size: 10 };

  beforeEach(() => {
    useAssetStore.setState({ items: [], status: {}, selectedGuid: null, currentFolder: '' });
  });

  it('Import to here / Show in folder 禁用 + 「仅桌面端可用」tooltip;其余项不受影响', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({
        asset_list: { assets: [ITEM] },
        asset_build_status: { items: [] },
        asset_thumbnail: { error: 'none' },
      }),
    );
    render(<AssetsPanel />);
    const item = await screen.findByText('wood_albedo.png');
    fireEvent.contextMenu(item.closest('[data-asset-guid]')!);

    const importBtn = (await screen.findByText('Import to here')).closest('button')!;
    expect(importBtn).toBeDisabled();
    expect(importBtn).toHaveAttribute('title', '仅桌面端可用(系统文件对话框)');
    const showBtn = screen.getByText('Show in folder').closest('button')!;
    expect(showBtn).toBeDisabled();
    expect(showBtn).toHaveAttribute('title', '仅桌面端可用(系统文件管理器)');
    // 非桌面专有项保持可用
    expect(screen.getByText('Reimport').closest('button')!).not.toBeDisabled();
    expect(screen.getByText('Find refs').closest('button')!).not.toBeDisabled();
  });
});

describe('<ViewportCanvas /> 浏览器回退腿(canvas readback)', () => {
  const initialEditor = useEditorStore.getState();
  const CAMERA = { target: [0, 0.5, 0], yaw: 35, pitch: 28, dist: 9, fovY: 50 };

  beforeEach(() => {
    useEditorStore.setState(initialEditor, true);
  });

  it('帧通道网络错误(agentd 不可达)→ 诚实降级卡上屏,禁止黑屏充绿', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new Error('fetch failed');
      }),
    );
    render(<ViewportCanvas />);
    expect(await screen.findByText(/Viewport 帧通道降级/)).toBeInTheDocument();
    // ForgeApiError NETWORK 消息原样上屏(如实上报,非伪造帧)
    expect(await screen.findByText(/请求失败: fetch failed/)).toBeInTheDocument();
    const canvas = document.querySelector('canvas')!;
    expect(canvas.style.display).toBe('none');
    expect(useEditorStore.getState().viewportDegraded).toContain('请求失败');
  });

  it('viewport_frame DEV_ENV_DEGRADE 信封 → 降级原因上屏', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: unknown, init?: { body?: string }) => {
        const { tool } = JSON.parse(init?.body ?? '{}') as { tool: string };
        if (tool.endsWith('viewport_get_camera')) {
          return {
            ok: true,
            status: 200,
            json: async () => ({
              content: [{ type: 'text', text: JSON.stringify(CAMERA) }],
            }),
          } as Response;
        }
        return {
          ok: true,
          status: 200,
          json: async () => ({
            content: [
              {
                type: 'text',
                text: JSON.stringify({
                  message: 'DEV_ENV_DEGRADE: no vulkan device',
                  error: 'DEV_ENV_DEGRADE',
                }),
              },
            ],
            isError: true,
          }),
        } as Response;
      }),
    );
    render(<ViewportCanvas />);
    expect(await screen.findByText(/DEV_ENV_DEGRADE: no vulkan device/)).toBeInTheDocument();
    const canvas = document.querySelector('canvas')!;
    expect(canvas.style.display).toBe('none');
  });

  it('成功帧 → 帧统计上屏且无降级卡(浏览器 canvas readback 腿真实可用)', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({
        viewport_get_camera: CAMERA,
        viewport_frame: {
          width: 16,
          height: 16,
          format: 'rgba8',
          pixelsB64: btoa(String.fromCharCode(...new Array(16 * 16 * 4).fill(7))),
          deviceName: 'mock-gpu',
          draws: 1,
          frames: 1,
          nonZeroPixels: 5,
          truncated: false,
        },
      }),
    );
    render(<ViewportCanvas />);
    expect(
      await screen.findByText((_, el) => el?.textContent === 'mock-gpu · draws 1 · px 5 · 轮询回退'),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Viewport 帧通道降级/)).toBeNull();
    expect(useEditorStore.getState().viewportDegraded).toBeNull();
  });
});
