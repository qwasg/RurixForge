import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import EditorView from '@/views/EditorView';
import RightPane from '@/components/shell/RightPane';
import { useEditorStore } from '@/lib/editorStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { mockForgeBackend } from './forgeMock';

/**
 * 响应式波(2026-08-24 用户拍板):A Hierarchy 迁壳右栏,编辑器 tab 激活时接管
 * 「工作区」文件树位。旧行为:七区硬下限 840px(240+320+280),壳主区在 1771px
 * 以下窗口给不到,编辑器右半边被 overflow-x-auto 裁走并顶在工作区树上。
 *
 * 底栏波(同日拍板):Assets 由左列改横向底栏,不再与视口争宽度——宽度分档收放
 * (applyEditorWidth/editorPaneBand)随之退役,只留工具条手动开合 + 持久化。
 */

const CUBE = {
  id: 1,
  name: 'Cube',
  transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: [],
};

const EDITOR_TOOLS = {
  entity_list: { entities: [CUBE] },
  scene_summary: {
    name: 'Demo',
    entityCount: 1,
    playState: 'edit',
    render: { frames: 1, lastTris: 0, lastNonZeroPixels: 0 },
  },
  play_state: { state: 'edit' },
  host_events: [],
  component_list_types: [],
  asset_list: { assets: [] },
  viewport_get_camera: { target: [0, 0, 0], yaw: 0, pitch: 0, dist: 5, fovY: 50 },
};

const initialEditor = useEditorStore.getState();
const initialWorkbench = useWorkbenchStore.getState();

beforeEach(() => {
  useEditorStore.setState(initialEditor, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useEditorStore.setState({ editorPanes: { assets: true } });
  globalThis.localStorage?.clear();
  vi.stubGlobal(
    'fetch',
    mockForgeBackend(EDITOR_TOOLS, {
      '/api/forge/workspace/tree': {
        path: '',
        entries: [
          { name: 'README.md', kind: 'file', relPath: 'README.md', size: 12, modifiedAt: '', hidden: false },
        ],
        total: 1,
        truncated: false,
      },
    }),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

// ---------- Assets 底栏显隐 ----------

describe('toggleEditorPane', () => {
  it('开合即写偏好并持久化', () => {
    useEditorStore.getState().toggleEditorPane('assets');
    expect(useEditorStore.getState().editorPanes.assets).toBe(false);
    expect(JSON.parse(globalThis.localStorage!.getItem('forge:editorPanes') as string)).toEqual({
      assets: false,
    });

    useEditorStore.getState().toggleEditorPane('assets');
    expect(useEditorStore.getState().editorPanes.assets).toBe(true);
    expect(JSON.parse(globalThis.localStorage!.getItem('forge:editorPanes') as string)).toEqual({
      assets: true,
    });
  });
});

// ---------- EditorView 接线 ----------

describe('<EditorView /> Assets 底栏', () => {
  it('默认展开；Inspector 不再装配', async () => {
    render(<EditorView />);
    expect(await screen.findByTestId('editor-pane-assets')).toBeInTheDocument();
    expect(screen.queryByTestId('editor-pane-inspector')).not.toBeInTheDocument();
    expect(screen.queryByTestId('editor-toggle-inspector')).not.toBeInTheDocument();
  });

  it('工具条显隐钮:收起后可再唤回', async () => {
    render(<EditorView />);
    await screen.findByTestId('editor-pane-assets');

    fireEvent.click(screen.getByTestId('editor-toggle-assets'));
    expect(screen.queryByTestId('editor-pane-assets')).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId('editor-toggle-assets'));
    expect(screen.getByTestId('editor-pane-assets')).toBeInTheDocument();
  });
});

// ---------- 右栏接管 ----------

describe('<RightPane /> 工作区 / 层级 / 属性', () => {
  it('默认工作区文件树;开编辑器 tab → 层级接管;关掉回落文件树', async () => {
    render(<RightPane />);
    expect(await screen.findByTestId('inspector')).toBeInTheDocument();
    expect(screen.queryByTestId('hierarchy-panel')).not.toBeInTheDocument();

    act(() => useWorkbenchStore.getState().openEditor());
    expect(await screen.findByTestId('hierarchy-panel')).toBeInTheDocument();
    expect(screen.queryByTestId('inspector')).not.toBeInTheDocument();
    // 层级在编辑器未挂载时自取实体清单
    expect(await screen.findByTestId('hierarchy-row-1')).toHaveTextContent('Cube');

    act(() => useWorkbenchStore.getState().closeTab('editor'));
    expect(await screen.findByTestId('inspector')).toBeInTheDocument();
  });

  it('切到非编辑器 tab 回落文件树;手动切换在下次 tab 变更前保持', async () => {
    render(<RightPane />);
    act(() => useWorkbenchStore.getState().openEditor());
    await screen.findByTestId('hierarchy-panel');

    act(() => useWorkbenchStore.getState().openTab('plan'));
    expect(await screen.findByTestId('inspector')).toBeInTheDocument();

    act(() => useWorkbenchStore.getState().activateTab('editor'));
    expect(await screen.findByTestId('hierarchy-panel')).toBeInTheDocument();

    // 手动切回工作区:停留在编辑器 tab 上也不被抢回
    fireEvent.click(screen.getByTestId('rightpane-tab-files'));
    expect(await screen.findByTestId('inspector')).toBeInTheDocument();
    expect(useWorkbenchStore.getState().activeTabId).toBe('editor');
  });

  it('属性 tab:实体检视迁入右栏', async () => {
    render(<RightPane />);
    act(() => useWorkbenchStore.getState().openEditor());
    await screen.findByTestId('hierarchy-panel');

    fireEvent.click(screen.getByTestId('rightpane-tab-properties'));
    expect(await screen.findByTestId('editor-pane-inspector')).toBeInTheDocument();
    expect(screen.getByText('在视口或层级中选中实体,查看素材与交互事件')).toBeInTheDocument();
  });
});
