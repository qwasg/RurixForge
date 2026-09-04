import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HierarchyPanel from '@/components/editor/HierarchyPanel';
import EditorView from '@/views/EditorView';
import { useEditorStore } from '@/lib/editorStore';
import { mockForgeBackend } from './forgeMock';

/** 骨架冒烟:fetch mock 下各区容器元素均就位 */
describe('<EditorView />', () => {
  const initialState = useEditorStore.getState();

  beforeEach(() => {
    useEditorStore.setState(initialState, true);
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({
        entity_list: {
          entities: [
            {
              id: 1,
              name: 'Cube',
              transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
              components: [],
            },
          ],
        },
        scene_summary: {
          name: 'Demo',
          entityCount: 1,
          playState: 'edit',
          render: { frames: 3, lastTris: 1, lastNonZeroPixels: 42 },
        },
        play_state: { state: 'edit' },
        host_events: [{ ts: '2026-08-17T00:00:00Z', event: 'host.started', backend: 'none' }],
        component_list_types: [{ name: 'Light', fields: [] }],
        viewport_get_camera: { target: [0, 0.5, 0], yaw: 35, pitch: 28, dist: 9, fovY: 50 },
        viewport_frame: {
          width: 16,
          height: 16,
          format: 'rgba8',
          pixelsB64: btoa(String.fromCharCode(...new Array(16 * 16 * 4).fill(0))),
          deviceName: 'mock-gpu',
          draws: 1,
          frames: 1,
          nonZeroPixels: 0,
          truncated: false,
        },
      }),
    );
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('骨架:Viewport 工具条 + Assets 底栏(层级/属性已迁壳右栏,Workbench 已退役)', async () => {
    render(<EditorView />);

    // A Hierarchy 迁至 RightPane,本视图不再自带层级列
    expect(screen.queryByTestId('hierarchy-panel')).not.toBeInTheDocument();
    expect(screen.queryByText('Hierarchy')).not.toBeInTheDocument();

    // C Viewport:页签 + gizmo + Play/Pause/Step + NodeGraph 同位页签
    expect(screen.getByRole('button', { name: 'Viewport' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'NodeGraph' })).toBeInTheDocument();
    expect(screen.getByTitle('Move (W)')).toBeInTheDocument();
    expect(screen.getByTitle('Rotate (E)')).toBeInTheDocument();
    expect(screen.getByTitle('Scale (R)')).toBeInTheDocument();
    expect(screen.getByTitle('Play')).toBeInTheDocument();
    expect(screen.getByTitle('Pause')).toBeInTheDocument();
    expect(screen.getByTitle('Step')).toBeInTheDocument();

    // 底栏波:场景级操作从退役的 Workbench tab 条并入视口工具条
    expect(screen.getByTitle('Undo')).toBeInTheDocument();
    expect(screen.getByTitle('Redo')).toBeInTheDocument();
    expect(screen.getByTitle('Save Scene')).toBeInTheDocument();
    expect(screen.getByTitle('Load Scene')).toBeInTheDocument();

    // B Assets 迁底栏(占旧 Workbench 位)
    expect(await screen.findByTestId('editor-pane-assets')).toBeInTheDocument();
    expect(screen.getByText('Assets')).toBeInTheDocument();
    expect(screen.getByTitle('隐藏 Assets 底栏')).toBeInTheDocument();

    // D Inspector 已从编辑器布局移除
    expect(screen.queryByTestId('editor-pane-inspector')).not.toBeInTheDocument();
    expect(screen.queryByTestId('editor-toggle-inspector')).not.toBeInTheDocument();

    // E Workbench 整块退役:Console/Problems/Metrics 页签与 host 事件流不复存在
    expect(screen.queryByRole('button', { name: 'console' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'problems' })).not.toBeInTheDocument();
    expect(screen.queryByText('host.started')).not.toBeInTheDocument();

    // F7 wave.3(D-F7-B):F Chat dock 已移除(对话入壳对话列)——Toggle Chat 钮不复存在
    expect(screen.queryByTitle('Toggle Chat')).not.toBeInTheDocument();
  });

  it('PIE 状态条与视口帧统计来自 play_state/viewport_frame 实测(wave.2)', async () => {
    render(<EditorView />);
    expect(await screen.findByText('edit')).toBeInTheDocument();
    // 帧统计:mock-gpu · draws 1 · px 0 · 轮询回退(viewport_frame 轮询首帧后上屏;
    // 测试环境无 WS 流服务器,通道如实标注轮询回退)
    expect(
      await screen.findByText((_, el) => el?.textContent === 'mock-gpu · draws 1 · px 0 · 轮询回退'),
    ).toBeInTheDocument();
  });

  it('F9(D1):MCP 侧新建实体后,周期 scene_summary 轮询驱动 Hierarchy 同步刷新', async () => {
    vi.useFakeTimers();
    render(
      <>
        <HierarchyPanel />
        <EditorView />
      </>,
    );
    // mount 首屏(beforeEach mock:后端 1 实体 Cube)
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(screen.getByText('Cube')).toBeInTheDocument();
    expect(screen.queryByText('JourneyBox')).not.toBeInTheDocument();

    // 模拟 MCP 侧(agent 聊天)entity_create:后端变 2 实体(面板数据源不感知,待轮询发现)
    const cube = {
      id: 1,
      name: 'Cube',
      transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
      components: [],
    };
    const journeyBox = {
      id: 37,
      name: 'JourneyBox',
      transform: { translation: [2, 1, 2], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
      components: [],
    };
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({
        entity_list: { entities: [cube, journeyBox] },
        scene_summary: {
          name: 'Demo',
          entityCount: 2,
          playState: 'edit',
          render: { frames: 4, lastTris: 1, lastNonZeroPixels: 42 },
        },
        play_state: { state: 'edit' },
        host_events: [],
        viewport_frame: {
          width: 16,
          height: 16,
          format: 'rgba8',
          pixelsB64: btoa(String.fromCharCode(...new Array(16 * 16 * 4).fill(0))),
          deviceName: 'mock-gpu',
          draws: 2,
          frames: 2,
          nonZeroPixels: 0,
          truncated: false,
        },
      }),
    );

    // 推进 1s → 轮询 tick → entityCount 漂移(2≠1)→ 真实重拉 entity_list → Hierarchy 出现新实体
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(screen.getByText('JourneyBox')).toBeInTheDocument();
    expect(screen.getByText('Cube')).toBeInTheDocument();
  });
});
