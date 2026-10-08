import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useGraphStore, type GraphDoc } from '@/lib/graphStore';
import { useEditorStore, type EntityData } from '@/lib/editorStore';
import { mockForgeBackend } from './forgeMock';

/** door_opener.rxgraph 同构蓝本(tests/fixtures/f4;4 节点 2 exec 边 2 数据边) */
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

const DOOR_ENTITY: EntityData = {
  id: 1,
  name: 'Door',
  transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: [
    { type: 'Script', enabled: true, props: { module: '', graphRef: GRAPH_PATH, props: {} } },
  ],
};

const initialGraph = useGraphStore.getState();
const initialEditor = useEditorStore.getState();

let fetchMock: ReturnType<typeof mockForgeBackend>;

beforeEach(() => {
  useGraphStore.setState(initialGraph, true);
  useEditorStore.setState(initialEditor, true);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** 全部 fetch 调用请求体(tool 含完整前缀) */
function toolCalls(): Array<{ tool: string; arguments: Record<string, unknown> }> {
  return fetchMock.mock.calls.map((c) =>
    JSON.parse((c[1] as { body: string }).body) as { tool: string; arguments: Record<string, unknown> },
  );
}

describe('graphStore', () => {
  it('loadByPath:graph_get 载图,graphPath/dirty/errors 就位', async () => {
    fetchMock = mockForgeBackend({ graph_get: { graph: DOOR } });
    vi.stubGlobal('fetch', fetchMock);

    await useGraphStore.getState().loadByPath(GRAPH_PATH);
    const s = useGraphStore.getState();
    expect(s.graph?.name).toBe('DoorOpener');
    expect(s.graph?.nodes).toHaveLength(4);
    expect(s.graphPath).toBe(GRAPH_PATH);
    expect(s.dirty).toBe(false);
    expect(s.errors).toEqual([]);
    expect(s.lastError).toBeNull();
    expect(toolCalls()[0].tool).toBe('mcp__code-forge__graph_get');
    expect(toolCalls()[0].arguments).toEqual({ path: GRAPH_PATH });
  });

  it('loadByPath 失败:lastError 如实,不伪造成空图', async () => {
    fetchMock = mockForgeBackend({}); // graph_get 未 mock → 请求抛错
    vi.stubGlobal('fetch', fetchMock);

    await useGraphStore.getState().loadByPath('Content/Graphs/ghost.rxgraph');
    const s = useGraphStore.getState();
    expect(s.graph).toBeNull();
    expect(s.lastError).toBeTruthy();
    expect(s.loading).toBe(false);
  });

  it('loadForSelectedEntity:选中实体有 Script.graphRef → 载图;无 → 空态', async () => {
    fetchMock = mockForgeBackend({ graph_get: { graph: DOOR } });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ entities: [DOOR_ENTITY], selectedId: 1 });

    await useGraphStore.getState().loadForSelectedEntity();
    expect(useGraphStore.getState().graph?.id).toBe('g_door_opener');
    expect(toolCalls()[0].arguments).toEqual({ path: GRAPH_PATH });

    // Switching selection must preserve the unsaved shared graph draft.
    useGraphStore.setState({ dirty: true });
    useEditorStore.setState({
      entities: [{ ...DOOR_ENTITY, components: [] }],
      selectedId: 1,
    });
    await useGraphStore.getState().loadForSelectedEntity();
    const s = useGraphStore.getState();
    expect(s.graph?.id).toBe('g_door_opener');
    expect(s.graphPath).toBe(GRAPH_PATH);
    expect(s.dirty).toBe(true);
    useGraphStore.setState({ dirty: false });
    await useGraphStore.getState().loadForSelectedEntity();
    expect(useGraphStore.getState().graph).toBeNull();
  });

  it('editConst:本地改 inputs[pin].const 并置 dirty(不打后端)', () => {
    useGraphStore.setState({ graph: DOOR, graphPath: GRAPH_PATH });
    useGraphStore.getState().editConst('n4', 'duration', 2.5);

    const s = useGraphStore.getState();
    expect(s.dirty).toBe(true);
    const n4 = s.graph?.nodes.find((n) => n.id === 'n4');
    expect(n4?.inputs?.duration).toEqual({ const: 2.5 });
    // 其余输入不动
    expect(n4?.inputs?.angle).toEqual({ ref: 'openSpeed' });
  });

  it('editExposedDefault:本地改暴露属性默认值并置 dirty', () => {
    useGraphStore.setState({ graph: DOOR, graphPath: GRAPH_PATH });
    useGraphStore.getState().editExposedDefault('openSpeed', 120);

    const s = useGraphStore.getState();
    expect(s.dirty).toBe(true);
    expect(s.graph?.exposedProps[0].default).toBe(120);
  });

  it('save 成功:validate ok → create 覆盖写回(name 取自路径 basename),dirty=false', async () => {
    fetchMock = mockForgeBackend({
      graph_get: { graph: DOOR },
      graph_validate: { ok: true, errors: [] },
      graph_create: { ok: true, path: GRAPH_PATH },
    });
    vi.stubGlobal('fetch', fetchMock);
    await useGraphStore.getState().loadByPath(GRAPH_PATH);
    useGraphStore.getState().editConst('n4', 'duration', 2.5);

    await useGraphStore.getState().save();
    const s = useGraphStore.getState();
    expect(s.dirty).toBe(false);
    expect(s.errors).toEqual([]);
    expect(s.lastSaved).toBe(GRAPH_PATH);

    const create = toolCalls().find((c) => c.tool === 'mcp__code-forge__graph_create');
    expect(create).toBeTruthy();
    expect(create?.arguments.name).toBe('door_opener');
    // 保存载荷携带本地常量修改(诚实:改什么存什么)
    const sent = create?.arguments.graph as GraphDoc;
    expect(sent.nodes.find((n) => n.id === 'n4')?.inputs?.duration).toEqual({ const: 2.5 });
  });

  it('save 校验失败:不写盘,errors 进 state,dirty 保持', async () => {
    fetchMock = mockForgeBackend({
      graph_validate: {
        ok: false,
        errors: [{ code: 'GRAPH_DANGLING_INPUT', message: 'flow.branch.condition 悬空', nodeId: 'n2' }],
      },
      graph_create: { ok: true, path: GRAPH_PATH }, // 在场但不应被调
    });
    vi.stubGlobal('fetch', fetchMock);
    useGraphStore.setState({ graph: DOOR, graphPath: GRAPH_PATH, dirty: true });

    await useGraphStore.getState().save();
    const s = useGraphStore.getState();
    expect(s.errors).toHaveLength(1);
    expect(s.errors[0].code).toBe('GRAPH_DANGLING_INPUT');
    expect(s.errors[0].nodeId).toBe('n2');
    expect(s.dirty).toBe(true);
    expect(toolCalls().filter((c) => c.tool === 'mcp__code-forge__graph_create')).toHaveLength(0);
  });
});
