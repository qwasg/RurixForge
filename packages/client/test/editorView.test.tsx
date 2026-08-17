import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import EditorView from '@/views/EditorView';
import { useEditorStore } from '@/lib/editorStore';
import { mockForgeBackend } from './forgeMock';

/** 七区骨架冒烟:fetch mock 下各区容器元素均就位 */
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
    vi.unstubAllGlobals();
  });

  it('七区骨架:Hierarchy/Assets/Viewport 工具条/Inspector/Workbench/Chat 折叠钮', async () => {
    render(<EditorView />);

    // A Hierarchy(实体行异步就位)
    expect(screen.getByText('Hierarchy')).toBeInTheDocument();
    expect(await screen.findByText('Cube')).toBeInTheDocument();
    expect(screen.getByTitle('Create entity')).toBeInTheDocument();

    // B Assets 占位
    expect(screen.getByText('Assets')).toBeInTheDocument();

    // C Viewport:页签 + gizmo + Play/Pause/Step + NodeGraph 同位页签
    expect(screen.getByRole('button', { name: 'Viewport' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'NodeGraph' })).toBeInTheDocument();
    expect(screen.getByTitle('Move (W)')).toBeInTheDocument();
    expect(screen.getByTitle('Rotate (E)')).toBeInTheDocument();
    expect(screen.getByTitle('Scale (R)')).toBeInTheDocument();
    expect(screen.getByTitle('Play')).toBeInTheDocument();
    expect(screen.getByTitle('Pause')).toBeInTheDocument();
    expect(screen.getByTitle('Step')).toBeInTheDocument();

    // D Inspector
    expect(screen.getByText('Inspector')).toBeInTheDocument();

    // E Workbench:Console 页签 + Undo/Redo/Save/Load
    expect(screen.getByRole('button', { name: 'console' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'problems' })).toBeInTheDocument();
    expect(screen.getByTitle('Undo')).toBeInTheDocument();
    expect(screen.getByTitle('Save Scene')).toBeInTheDocument();
    // Console 事件流(host_events 实测)
    expect(await screen.findByText('host.started')).toBeInTheDocument();

    // F Chat:折叠钮默认收起,点击展开 dock
    fireEvent.click(screen.getByTitle('Toggle Chat'));
    expect(await screen.findByText('Chat')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Ask the engine...')).toBeInTheDocument();
    expect(screen.getByTitle('Collapse Chat')).toBeInTheDocument();
  });

  it('PIE 状态条与视口帧统计来自 play_state/viewport_frame 实测(wave.2)', async () => {
    render(<EditorView />);
    expect(await screen.findByText('edit')).toBeInTheDocument();
    // 帧统计:mock-gpu · draws 1 · frames 1 · px 0(viewport_frame 轮询首帧后上屏)
    expect(
      await screen.findByText((_, el) => el?.textContent === 'mock-gpu · draws 1 · frames 1 · px 0'),
    ).toBeInTheDocument();
  });
});
