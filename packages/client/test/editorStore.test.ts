import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useEditorStore, type EntityData } from '@/lib/editorStore';
import { mockForgeBackend } from './forgeMock';

const CUBE: EntityData = {
  id: 1,
  name: 'Cube',
  transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: [{ type: 'MeshRenderer', enabled: true, props: { mesh: 'cube' } }],
};

let fetchMock: ReturnType<typeof mockForgeBackend>;

const initialState = useEditorStore.getState();
beforeEach(() => {
  useEditorStore.setState(initialState, true);
  fetchMock = mockForgeBackend({});
  vi.stubGlobal('fetch', fetchMock);
});
afterEach(() => {
  vi.unstubAllGlobals();
});

/** 取第 n 次 fetch 调用的请求体 */
function callBody(n = 0): { tool: string; arguments: Record<string, unknown> } {
  const init = fetchMock.mock.calls[n]?.[1] as { body: string };
  return JSON.parse(init.body) as { tool: string; arguments: Record<string, unknown> };
}

describe('editorStore', () => {
  it('loadEntities:打 entity_list 并填充实体表', async () => {
    fetchMock = mockForgeBackend({ entity_list: { entities: [CUBE] } });
    vi.stubGlobal('fetch', fetchMock);

    await useEditorStore.getState().loadEntities();
    expect(useEditorStore.getState().entities).toEqual([CUBE]);
    expect(callBody(0).tool).toBe('mcp__engine-scene__entity_list');
  });

  it('selectEntity:更新选中 id,可清空', () => {
    const s = useEditorStore.getState();
    s.selectEntity(1);
    expect(useEditorStore.getState().selectedId).toBe(1);
    useEditorStore.getState().selectEntity(null);
    expect(useEditorStore.getState().selectedId).toBeNull();
  });

  it('createEntity:默认命名、追加并选中新实体', async () => {
    const created: EntityData = { ...CUBE, id: 2, name: 'Entity 2' };
    fetchMock = mockForgeBackend({ entity_create: { id: 2, entity: created } });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ entities: [CUBE] });

    await useEditorStore.getState().createEntity();
    const s = useEditorStore.getState();
    expect(s.entities).toHaveLength(2);
    expect(s.entities[1]).toEqual(created);
    expect(s.selectedId).toBe(2);
    expect(callBody(0).arguments).toEqual({ name: 'Entity 2' });
  });

  it('setTransform:提交 translation 数组并以返回的全量 Transform 更新本地', async () => {
    const next = { translation: [1, 2, 3], rotation: [0, 0, 0, 1], scale: [1, 1, 1] };
    fetchMock = mockForgeBackend({ transform_set: next });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ entities: [CUBE] });

    await useEditorStore.getState().setTransform(1, { translation: [1, 2, 3] });
    expect(useEditorStore.getState().entities[0].transform).toEqual(next);
    expect(callBody(0).tool).toBe('mcp__engine-scene__transform_set');
    expect(callBody(0).arguments).toEqual({ id: 1, translation: [1, 2, 3] });
  });

  it('playEnter:状态迁移到 play_running 并重新拉实体列表', async () => {
    fetchMock = mockForgeBackend({
      play_enter: { state: 'play_running' },
      entity_list: { entities: [CUBE] },
    });
    vi.stubGlobal('fetch', fetchMock);

    await useEditorStore.getState().playEnter();
    const s = useEditorStore.getState();
    expect(s.playState).toBe('play_running');
    expect(s.entities).toEqual([CUBE]);
    expect(callBody(0).tool).toBe('mcp__engine-scene__play_enter');
    expect(callBody(1).tool).toBe('mcp__engine-scene__entity_list');
  });

  it('refreshSummary:取 render 块帧统计与 playState', async () => {
    fetchMock = mockForgeBackend({
      scene_summary: {
        name: 'Demo',
        entityCount: 1,
        playState: 'edit',
        render: { frames: 7, lastTris: 1, lastNonZeroPixels: 42 },
      },
    });
    vi.stubGlobal('fetch', fetchMock);

    await useEditorStore.getState().refreshSummary();
    const s = useEditorStore.getState();
    expect(s.sceneName).toBe('Demo');
    expect(s.stats).toEqual({ frames: 7, lastTris: 1, lastNonZeroPixels: 42 });
    expect(s.playState).toBe('edit');
  });

  it('destroyEntity:移除实体并清空其选中态', async () => {
    fetchMock = mockForgeBackend({ entity_destroy: { destroyed: 1 } });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ entities: [CUBE], selectedId: 1 });

    await useEditorStore.getState().destroyEntity(1);
    const s = useEditorStore.getState();
    expect(s.entities).toEqual([]);
    expect(s.selectedId).toBeNull();
    expect(callBody(0).arguments).toEqual({ id: 1 });
  });

  it('错误分支:工具调用失败时写入 lastError', async () => {
    fetchMock = mockForgeBackend({
      entity_list: { entities: [CUBE] },
    });
    // edit_undo 未 mock → 抛错 → lastError
    vi.stubGlobal('fetch', fetchMock);
    await useEditorStore.getState().undo();
    expect(useEditorStore.getState().lastError).toContain('未 mock 的工具');
  });
});
