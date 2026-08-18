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

  it('prefillChat:预填文案并打开 Chat,clearChatPrefill 清除(F2 wave.3 seam)', () => {
    useEditorStore.getState().prefillChat('基于资产 Meshes/cube.gltf 生成变体:');
    let s = useEditorStore.getState();
    expect(s.chatPrefill).toBe('基于资产 Meshes/cube.gltf 生成变体:');
    expect(s.chatOpen).toBe(true);
    useEditorStore.getState().clearChatPrefill();
    s = useEditorStore.getState();
    expect(s.chatPrefill).toBeNull();
    expect(s.chatOpen).toBe(true); // 清除预填不回收面板
  });

  // ---- F3 wave.4:Composer 五模式载荷 + multitask 分片卡片 ----

  it('sendChat:选中模式随发送载荷(user 消息记录 mode)', async () => {
    // RD-F1-002:非 multitask 四模式走 /api/forge/llm/chat(REST 面),不再打 scene_summary 工具。
    const chatCalls: Array<Record<string, unknown>> = [];
    fetchMock = mockForgeBackend({}, {
      '/api/forge/llm/chat': (init?: { body?: string }) => {
        chatCalls.push(JSON.parse(init?.body ?? '{}') as Record<string, unknown>);
        return { provider: 'mock', text: 'mock:已收到', toolCalls: [], iters: 0 };
      },
    });
    vi.stubGlobal('fetch', fetchMock);

    for (const mode of ['build', 'plan', 'debug', 'ask'] as const) {
      await useEditorStore.getState().sendChat(`hello ${mode}`, mode);
    }
    const users = useEditorStore.getState().chatMessages.filter((m) => m.role === 'user');
    expect(users.map((m) => m.mode)).toEqual(['build', 'plan', 'debug', 'ask']);
    // 载荷:{text, mode} 逐项打到 llm/chat
    expect(chatCalls).toHaveLength(4);
    expect(chatCalls.map((c) => c.mode)).toEqual(['build', 'plan', 'debug', 'ask']);
    // mock provider 如实标注
    const assistants = useEditorStore.getState().chatMessages.filter((m) => m.role === 'assistant');
    expect(assistants.every((m) => m.text.startsWith('[mock]'))).toBe(true);
  });

  it('sendChat deepseek:工具循环响应渲染工具摘要 + 成功后刷新实体与统计', async () => {
    fetchMock = mockForgeBackend(
      {
        entity_list: { entities: [CUBE] },
        scene_summary: {
          name: 'Demo',
          entityCount: 1,
          playState: 'edit',
          render: { frames: 9, lastTris: 3, lastNonZeroPixels: 7 },
        },
      },
      {
        '/api/forge/llm/chat': () => ({
          provider: 'deepseek',
          text: '已创建 1 个立方体',
          toolCalls: [
            { name: 'mcp__engine-scene__entity_create', ok: true, summary: '{"id":1}' },
            { name: 'mcp__engine-scene__component_add', ok: false, summary: '不支持' },
          ],
          iters: 3,
        }),
      },
    );
    vi.stubGlobal('fetch', fetchMock);

    await useEditorStore.getState().sendChat('创建一个立方体', 'build');
    const s = useEditorStore.getState();
    const assistant = s.chatMessages.find((m) => m.role === 'assistant');
    expect(assistant).toBeDefined();
    expect(assistant!.text).toContain('已创建 1 个立方体');
    expect(assistant!.text).toContain('工具调用 2 次(成功 1)');
    expect(assistant!.text).toContain('✓ mcp__engine-scene__entity_create');
    expect(assistant!.text).toContain('✗ mcp__engine-scene__component_add');
    // 有成功工具调用 → reload(entity_list)+ scene_summary 刷新
    expect(s.entities).toEqual([CUBE]);
    expect(s.stats).toEqual({ frames: 9, lastTris: 3, lastNonZeroPixels: 7 });
  });

  it('sendChat deepseek:无工具调用不刷新实体(纯文本答复)', async () => {
    fetchMock = mockForgeBackend(
      {},
      {
        '/api/forge/llm/chat': () => ({
          provider: 'deepseek',
          text: '这是一个解释',
          toolCalls: [],
          iters: 1,
        }),
      },
    );
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ entities: [] });

    await useEditorStore.getState().sendChat('解释一下场景', 'ask');
    const s = useEditorStore.getState();
    const assistant = s.chatMessages.find((m) => m.role === 'assistant');
    expect(assistant!.text).toBe('这是一个解释');
    // 无 toolCalls → 不打 entity_list/scene_summary(仅一次 llm/chat 请求)
    expect(fetchMock.mock.calls).toHaveLength(1);
  });

  it('sendChat multitask:碰撞模板 → /swarm/execute 载荷 + 分片卡片消息', async () => {
    const executeCalls: Array<Record<string, unknown>> = [];
    fetchMock = mockForgeBackend(
      { entity_list: { entities: [CUBE] } },
      {
        '/api/forge/swarm/execute': (init?: { body?: string }) => {
          executeCalls.push(JSON.parse(init?.body ?? '{}') as Record<string, unknown>);
          return {
            shardType: 'scene-partition',
            shards: [{ shardId: 'shard-1', status: 'done', okCount: 1, errorCount: 0 }],
            aggregate: { totalItems: 1, succeeded: 1, failed: 0, disjoint: true, consistent: true },
          };
        },
      },
    );
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ entities: [CUBE] });

    await useEditorStore.getState().sendChat('给全部关卡块生成碰撞体', 'multitask');

    // /swarm/execute 载荷:scene-partition + 全部实体 id + add_component RigidBody
    expect(executeCalls).toHaveLength(1);
    expect(executeCalls[0].shardType).toBe('scene-partition');
    expect(executeCalls[0].items).toEqual([CUBE.id]);
    expect(executeCalls[0].shardCount).toBe(4);
    expect((executeCalls[0].operation as { type: string }).type).toBe('RigidBody');

    // 分片卡片消息:assistant 带 swarm 报告;multitask 后 reload 拉实体
    const msgs = useEditorStore.getState().chatMessages;
    const card = msgs.find((m) => m.swarm);
    expect(card).toBeDefined();
    expect(card!.swarm!.aggregate).toMatchObject({ totalItems: 1, succeeded: 1, failed: 0 });
    expect(card!.text).toContain('1/1');
    const tools = fetchMock.mock.calls.map((c) => {
      try {
        return (JSON.parse((c[1] as { body: string }).body) as { tool?: string }).tool;
      } catch {
        return undefined;
      }
    });
    expect(tools).toContain('mcp__engine-scene__entity_list');
  });

  it('sendChat multitask:模板未命中 → error 消息如实,不伪造执行', async () => {
    fetchMock = mockForgeBackend({});
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ entities: [CUBE] });

    await useEditorStore.getState().sendChat('给我讲个故事', 'multitask');
    const msgs = useEditorStore.getState().chatMessages;
    const err = msgs[msgs.length - 1];
    expect(err.role).toBe('error');
    expect(err.text).toContain('multitask 模板未命中');
    expect(msgs.some((m) => m.swarm)).toBe(false);
  });

  // ---- F1 wave.2:viewport 相机 / 点选 / gizmo ----

  const CAM = { target: [0, 0.5, 0], yaw: 35, pitch: 28, dist: 9, fovY: 50 };

  it('pickAt:命中设置 selectedId,未命中清空', async () => {
    fetchMock = mockForgeBackend({
      viewport_pick: { hit: true, entityId: 7, name: 'Cube7', point: [0, 0.5, 0] },
    });
    vi.stubGlobal('fetch', fetchMock);

    await useEditorStore.getState().pickAt(64, 48, 128, 96);
    expect(useEditorStore.getState().selectedId).toBe(7);
    expect(callBody(0).tool).toBe('mcp__engine-scene__viewport_pick');
    expect(callBody(0).arguments).toEqual({ x: 64, y: 48, width: 128, height: 96 });

    fetchMock = mockForgeBackend({ viewport_pick: { hit: false } });
    vi.stubGlobal('fetch', fetchMock);
    await useEditorStore.getState().pickAt(2, 2, 128, 96);
    expect(useEditorStore.getState().selectedId).toBeNull();
  });

  it('orbitCamera / zoomCamera / focusSelected:相机子集更新回显全量', async () => {
    const after = { ...CAM, yaw: 0, pitch: 33.5 };
    fetchMock = mockForgeBackend({ viewport_set_camera: after });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ camera: CAM, entities: [CUBE], selectedId: 1 });

    // orbit dx=100, dy=10 → yaw 35-35=0,pitch 28+3.5=31.5(断言以服务端回显为准)
    await useEditorStore.getState().orbitCamera(100, 10);
    expect(callBody(0).tool).toBe('mcp__engine-scene__viewport_set_camera');
    expect(callBody(0).arguments).toEqual({ yaw: 0, pitch: 31.5 });
    expect(useEditorStore.getState().camera).toEqual(after);

    // 聚焦:target = 选中实体 translation
    const focused = { ...CAM, target: [0, 0, 0] };
    fetchMock = mockForgeBackend({ viewport_set_camera: focused });
    vi.stubGlobal('fetch', fetchMock);
    await useEditorStore.getState().focusSelected();
    expect(callBody(0).arguments).toEqual({ target: [0, 0, 0] });
  });

  it('gizmoDragSelected translate:相机平面位移提交 transform_set(单次可 undo)', async () => {
    const next = { translation: [0.1, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] };
    fetchMock = mockForgeBackend({ transform_set: next });
    vi.stubGlobal('fetch', fetchMock);
    // 相机 yaw=90,pitch=0 → 眼在 +x 看向 -x;right = cross(f,up) 归一 = [0,0,-1]
    useEditorStore.setState({
      camera: { target: [0, 0, 0], yaw: 90, pitch: 0, dist: 10, fovY: 90 },
      entities: [CUBE],
      selectedId: 1,
      gizmo: 'translate',
    });

    await useEditorStore.getState().gizmoDragSelected(100, 0, 500);
    const args = callBody(0).arguments;
    expect(callBody(0).tool).toBe('mcp__engine-scene__transform_set');
    expect(args.id).toBe(1);
    const t = args.translation as number[];
    // wpp = 2·10·tan(45°)/500 = 0.04;dx=100 → 沿 right=[0,0,-1] 移 4.0
    expect(t[0]).toBeCloseTo(0, 5);
    expect(t[1]).toBeCloseTo(0, 5);
    expect(t[2]).toBeCloseTo(-4.0, 5);
  });

  it('gizmoDragSelected rotate:绕世界 Y 轴四元数左乘', async () => {
    const next = { translation: [0, 0, 0], rotation: [0, 0.7071, 0, 0.7071], scale: [1, 1, 1] };
    fetchMock = mockForgeBackend({ transform_set: next });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({
      camera: CAM,
      entities: [CUBE],
      selectedId: 1,
      gizmo: 'rotate',
    });

    // dx=180 → 0.5°/px → 90° → qYaw=[0,sin45°,0,cos45°]
    await useEditorStore.getState().gizmoDragSelected(180, 0, 500);
    const rot = (callBody(0).arguments.rotation as number[]).map((v) => v as number);
    expect(rot[0]).toBeCloseTo(0, 5);
    expect(rot[1]).toBeCloseTo(Math.SQRT1_2, 3);
    expect(rot[2]).toBeCloseTo(0, 5);
    expect(rot[3]).toBeCloseTo(Math.SQRT1_2, 3);
  });
});
