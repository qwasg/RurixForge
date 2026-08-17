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

  it('Composer 五模式切换 + multitask 分片卡片渲染(F3 wave.4 G-F3-4)', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend(
        {
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
          host_events: [],
          component_list_types: [],
          viewport_get_camera: { target: [0, 0.5, 0], yaw: 35, pitch: 28, dist: 9, fovY: 50 },
        },
        {
          '/api/forge/swarm/execute': {
            shardType: 'scene-partition',
            shards: [
              { shardId: 'shard-1', status: 'done', okCount: 1, errorCount: 0 },
              { shardId: 'shard-2', status: 'done', okCount: 0, errorCount: 0 },
            ],
            aggregate: { totalItems: 1, succeeded: 1, failed: 0, disjoint: true, consistent: true },
          },
        },
      ),
    );

    render(<EditorView />);
    // 展开 Chat dock
    fireEvent.click(screen.getByTitle('Toggle Chat'));
    const modes = await screen.findByTestId('composer-modes');
    // 五模式齐全
    expect(modes.querySelectorAll('button')).toHaveLength(5);

    // 切 multitask:选中态 + placeholder 随模式变化
    const mtBtn = modes.querySelector('[data-mode="multitask"]') as HTMLButtonElement;
    fireEvent.click(mtBtn);
    expect(mtBtn.className).toContain('bg-ink');
    expect(
      screen.getByPlaceholderText('批量任务:如「给全部关卡块生成碰撞体」'),
    ).toBeInTheDocument();

    // 发送 → 分片卡片渲染分片进度与聚合结论(数据来自 /swarm/execute 响应)
    fireEvent.change(screen.getByPlaceholderText('批量任务:如「给全部关卡块生成碰撞体」'), {
      target: { value: '给全部关卡块生成碰撞体' },
    });
    fireEvent.click(screen.getByTitle('Send'));

    const card = await screen.findByTestId('swarm-card');
    expect(card).toHaveTextContent('shard-1');
    expect(card).toHaveTextContent('done ok=1 err=0');
    expect(card).toHaveTextContent('聚合 1/1 成功 · 0 失败 · disjoint=true');
    // user 消息带模式徽标(切换器按钮 + 徽标 = 至少 2 处 multitask 文本)
    expect(screen.getAllByText('multitask').length).toBeGreaterThanOrEqual(2);
  });
});
