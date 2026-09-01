import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HierarchyPanel from '@/components/editor/HierarchyPanel';
import NodeGraphView from '@/components/editor/NodeGraphView';
import EditorView from '@/views/EditorView';
import { useGraphStore, type GraphDoc } from '@/lib/graphStore';
import { useEditorStore, type EntityData } from '@/lib/editorStore';
import { mockForgeBackend } from './forgeMock';

/** door_opener.rxgraph 同构蓝本(tests/fixtures/f4) */
const DOOR: GraphDoc = {
  version: 1,
  id: 'g_door_opener',
  name: 'DoorOpener',
  exposedProps: [{ name: 'openSpeed', kind: 'F32', default: 90 }],
  nodes: [
    { id: 'n1', type: 'event.on_trigger_enter', pos: [40, 80] },
    { id: 'n2', type: 'flow.branch', pos: [240, 80], inputs: { condition: { node: 'n3', pin: 'out' } } },
    {
      id: 'n3',
      type: 'entity.has_tag',
      pos: [60, 200],
      inputs: { entity: { node: 'n1', pin: 'otherEntity' }, tag: { const: 'player' } },
    },
    {
      id: 'n4',
      type: 'transform.rotate_tween',
      pos: [460, 80],
      inputs: { angle: { ref: 'openSpeed' }, duration: { const: 1.2 }, target: { const: '$self' } },
    },
  ],
  edges: [
    { from: ['n1', 'exec'], to: ['n2', 'exec'] },
    { from: ['n2', 'then'], to: ['n4', 'exec'] },
  ],
};

const GRAPH_PATH = 'Content/Graphs/door_opener.rxgraph';

const initialGraph = useGraphStore.getState();
const initialEditor = useEditorStore.getState();

beforeEach(() => {
  globalThis.localStorage.clear(); // 含无限画布视口位置(forge:nodeGraphView),测试间不串
  useGraphStore.setState(initialGraph, true);
  useEditorStore.setState(initialEditor, true);
});

/** jsdom 无 PointerEvent 构造器:用同名类型的 MouseEvent 承载 */
function pointer(type: string, init: MouseEventInit = {}): MouseEvent {
  return new MouseEvent(type, { bubbles: true, cancelable: true, ...init });
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

/** 预置已载图状态(绕过 graph_get,直测渲染/编辑) */
function seedLoaded(patch: Partial<ReturnType<typeof useGraphStore.getState>> = {}) {
  useGraphStore.setState({
    graph: DOOR,
    graphPath: GRAPH_PATH,
    errors: [],
    dirty: false,
    loading: false,
    lastError: null,
    lastSaved: null,
    ...patch,
  });
}

describe('<NodeGraphView />', () => {
  it('空态:未加载图提示 + 路径输入框 + agent 定位文案', () => {
    render(<NodeGraphView />);
    expect(screen.getByText(/未加载图/)).toBeInTheDocument();
    expect(screen.getByText(/logic-blueprint-gen/)).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Content/Graphs/xxx.rxgraph')).toBeInTheDocument();
  });

  it('渲染 door fixture:4 节点卡片 + 2 exec 实线边 + 2 数据虚线边 + 暴露属性表', () => {
    seedLoaded();
    const { container } = render(<NodeGraphView />);

    expect(container.querySelectorAll('[data-graph-node]')).toHaveLength(4);
    expect(container.querySelectorAll('[data-graph-edge="exec"]')).toHaveLength(2);
    expect(container.querySelectorAll('[data-graph-edge="data"]')).toHaveLength(2);
    // 节点标题 = type;exposed default 90 就位
    expect(screen.getByText('event.on_trigger_enter')).toBeInTheDocument();
    expect(screen.getByText('transform.rotate_tween')).toBeInTheDocument();
    expect((screen.getByTestId('exposed-openSpeed') as HTMLInputElement).value).toBe('90');
  });

  it('无限画布:空白处拖拽平移世界层,HUD 倍率随缩放更新', () => {
    seedLoaded();
    render(<NodeGraphView />);
    const world = screen.getByTestId('graph-world');
    expect(world.style.transform).toBe('translate(0px, 0px) scale(1)');

    fireEvent(screen.getByTestId('graph-canvas'), pointer('pointerdown', { button: 0, clientX: 10, clientY: 10 }));
    fireEvent(window, pointer('pointermove', { clientX: 50, clientY: 35 }));
    fireEvent(window, pointer('pointerup'));
    expect(world.style.transform).toBe('translate(40px, 25px) scale(1)');

    fireEvent.click(screen.getByTestId('graph-zoom-out'));
    expect(screen.getByTestId('graph-zoom-reset')).toHaveTextContent('83%');
    fireEvent.click(screen.getByTestId('graph-zoom-reset'));
    expect(world.style.transform).toBe('translate(0px, 0px) scale(1)');
  });

  it('常量内联编辑:点击 const → input,回车提交 editConst 并置 dirty', () => {
    seedLoaded();
    render(<NodeGraphView />);

    fireEvent.click(screen.getByTestId('const-n4-duration'));
    const input = screen.getByTestId('const-input-n4-duration') as HTMLInputElement;
    expect(input.value).toBe('1.2');
    fireEvent.change(input, { target: { value: '2.5' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    const s = useGraphStore.getState();
    expect(s.dirty).toBe(true);
    expect(s.graph?.nodes.find((n) => n.id === 'n4')?.inputs?.duration).toEqual({ const: 2.5 });
  });

  it('暴露属性默认值编辑:失焦提交 editExposedDefault 并置 dirty', () => {
    seedLoaded();
    render(<NodeGraphView />);

    const input = screen.getByTestId('exposed-openSpeed') as HTMLInputElement;
    fireEvent.change(input, { target: { value: '120' } });
    fireEvent.blur(input);

    const s = useGraphStore.getState();
    expect(s.dirty).toBe(true);
    expect(s.graph?.exposedProps[0].default).toBe(120);
  });

  it('保存成功:validate ok + create → dirty=false + 状态行「已保存」', async () => {
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({
        graph_validate: { ok: true, errors: [] },
        graph_create: { ok: true, path: GRAPH_PATH },
      }),
    );
    seedLoaded({ dirty: true });
    render(<NodeGraphView />);

    fireEvent.click(screen.getByTitle('保存'));
    expect(await screen.findByText(/已保存 Content\/Graphs\/door_opener\.rxgraph/)).toBeInTheDocument();
    expect(useGraphStore.getState().dirty).toBe(false);
  });

  it('保存校验失败:错误条列 code+nodeId,错误节点红框,不写盘', async () => {
    const fetchMock = mockForgeBackend({
      graph_validate: {
        ok: false,
        errors: [{ code: 'GRAPH_DANGLING_INPUT', message: 'flow.branch.condition 悬空', nodeId: 'n2' }],
      },
      graph_create: { ok: true, path: GRAPH_PATH },
    });
    vi.stubGlobal('fetch', fetchMock);
    seedLoaded({ dirty: true });
    const { container } = render(<NodeGraphView />);

    fireEvent.click(screen.getByTitle('保存'));
    const strip = await screen.findByTestId('graph-errors');
    expect(strip).toHaveTextContent('GRAPH_DANGLING_INPUT');
    expect(strip).toHaveTextContent('n2');
    expect(strip).toHaveTextContent('flow.branch.condition 悬空');
    // 错误节点红框
    expect(container.querySelector('[data-graph-node="n2"]')?.className).toContain('border-danger');
    expect(container.querySelector('[data-graph-node="n1"]')?.className).not.toContain('border-danger');
    // 校验不过不落盘:graph_create 零调用,dirty 保持
    const tools = fetchMock.mock.calls.map((c) =>
      (JSON.parse((c[1] as { body: string }).body) as { tool: string }).tool,
    );
    expect(tools.filter((t) => t === 'mcp__code-forge__graph_create')).toHaveLength(0);
    expect(useGraphStore.getState().dirty).toBe(true);
  });

  it('EditorView 集成:选中带 Script 的实体 → 切 NodeGraph 页签 → graph_get 载图渲染', async () => {
    const doorEntity: EntityData = {
      id: 1,
      name: 'Door',
      transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
      components: [
        { type: 'Script', enabled: true, props: { module: '', graphRef: GRAPH_PATH, props: {} } },
      ],
    };
    vi.stubGlobal(
      'fetch',
      mockForgeBackend({
        entity_list: { entities: [doorEntity] },
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
        graph_get: { graph: DOOR },
      }),
    );

    // 层级已迁壳右栏(RightPane),这里与之并排渲染以走真实点选路径
    const { container } = render(
      <>
        <HierarchyPanel />
        <EditorView />
      </>,
    );
    // 选中 Door(带 Script.graphRef)
    fireEvent.click(await screen.findByText('Door'));
    // 切 NodeGraph 页签 → loadForSelectedEntity → graph_get
    fireEvent.click(screen.getByRole('button', { name: 'NodeGraph' }));

    expect(await screen.findByText('DoorOpener')).toBeInTheDocument();
    expect(container.querySelectorAll('[data-graph-node]')).toHaveLength(4);
    expect(container.querySelectorAll('[data-graph-edge="exec"]')).toHaveLength(2);
  });
});
