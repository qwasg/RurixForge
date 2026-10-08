import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useEditorStore, type EntityData } from '@/lib/editorStore';
import { useWorkbenchStore } from '@/lib/workbenchStore';
import { useToastStore } from '@/lib/toastStore';
import { mockForgeBackend } from './forgeMock';

const CUBE: EntityData = {
  id: 1,
  name: 'Cube',
  category: 'map',
  transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: [{ type: 'MeshRenderer', enabled: true, props: { mesh: 'cube' } }],
};

const initialWorkbench = useWorkbenchStore.getState();

let fetchMock: ReturnType<typeof mockForgeBackend>;

const initialState = useEditorStore.getState();
beforeEach(() => {
  useEditorStore.setState(initialState, true);
  useWorkbenchStore.setState(initialWorkbench, true);
  useToastStore.getState().clear();
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
  it('refreshRenderBackend:能力来自真实 RPC，只提示 forge.toml 不一致，不执行切换/写配置', async () => {
    const capabilities = {
      renderBackend: 'godot',
      pipelined: true,
      legs: ['sprite_mesh', 'model'],
      preview: true,
      particles: false,
      frameExits: { cpuRgba8: true, sharedD3d12: true, zeroCopy: false },
      stats: { nonzero: true, triangles: true, truncated: false, meshFallbacks: false, meshClasses: false },
      maxDraws: { spriteMesh: null, model: null, sentinelsV6: null },
      maxSize: { rpc: [1920, 1080] as [number, number], stream: [1280, 720] as [number, number] },
      coverage: { unsupported: [{ feature: 'Sprite.flip', reason: '配置未接入' }] },
    };
    fetchMock = mockForgeBackend({
      render_backend_info: {
        renderBackend: 'godot', method: 'forward_plus', driver: 'd3d12', source: 'forge.toml',
        ready: true, deviceName: '真实 GPU', versions: { engineHost: '0.1.0', godot: '4.7.2', gdext: '0.5.5' },
      },
      render_capabilities: capabilities,
    }, {
      '/api/forge/workspace/file': {
        path: 'forge.toml',
        content: '[render]\\nbackend = "godot"\\nmethod = "mobile"\\ndriver = "d3d12"\\n',
      },
    });
    vi.stubGlobal('fetch', fetchMock);
    localStorage.removeItem('forge:activeWorkspace');

    await useEditorStore.getState().refreshRenderBackend(null);
    const state = useEditorStore.getState();
    expect(state.renderBackendInfo?.deviceName).toBe('真实 GPU');
    expect(state.renderCapabilities).toEqual(capabilities);
    expect(state.renderConfigMismatch).toContain('forge.toml 当前配置为 godot / mobile / d3d12');
    expect(state.renderConfigMismatch).toContain('未自动切换');
    expect(state.renderStatusError).toBeNull();
    expect(fetchMock.mock.calls.every(([url]) => String(url) === '/api/forge/mcp/call' || String(url).startsWith('/api/forge/workspace/file'))).toBe(true);
  });

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

  it('F9(D1) refreshSummary:entityCount 漂移(MCP 侧外部新建)时真实重拉 entity_list', async () => {
    const JOURNEY_BOX: EntityData = { ...CUBE, id: 37, name: 'JourneyBox' };
    fetchMock = mockForgeBackend({
      scene_summary: {
        name: 'Demo',
        entityCount: 2, // 后端已 2 实体,本地仅 1 → 漂移
        playState: 'edit',
        render: { frames: 7, lastTris: 1, lastNonZeroPixels: 42 },
      },
      entity_list: { entities: [CUBE, JOURNEY_BOX] },
    });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ entities: [CUBE] });

    await useEditorStore.getState().refreshSummary();
    const s = useEditorStore.getState();
    // 面板数据来自 entity_list 实返(非伪造拼接)
    expect(s.entities).toEqual([CUBE, JOURNEY_BOX]);
    expect(callBody(0).tool).toBe('mcp__engine-scene__scene_summary');
    expect(callBody(1).tool).toBe('mcp__engine-scene__entity_list');
  });

  it('F9(D1) refreshSummary:entityCount 与本地一致时不重拉(不多打 entity_list)', async () => {
    fetchMock = mockForgeBackend({
      scene_summary: {
        name: 'Demo',
        entityCount: 1,
        playState: 'edit',
        render: { frames: 7, lastTris: 1, lastNonZeroPixels: 42 },
      },
    });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ entities: [CUBE] });

    await useEditorStore.getState().refreshSummary();
    const s = useEditorStore.getState();
    expect(s.entities).toEqual([CUBE]);
    expect(fetchMock).toHaveBeenCalledTimes(1); // 仅 scene_summary
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
    fetchMock = vi.fn(async () => ({ ok: false, status: 409, json: async () => ({ error: { code: 'HISTORY_CONFLICT', message: '历史栈顶已改变' } }) }) as Response);
    // 协调撤销拒绝后，冲突原因必须展示而不能绕过后端直接 edit_undo。
    vi.stubGlobal('fetch', fetchMock);
    await useEditorStore.getState().undo();
    expect(useEditorStore.getState().lastError).toContain('历史栈顶已改变');
  });

  it('runPlaytest:汇总行 + 失败用例走 toast(底栏波:Console 退役后的唯一展示位)', async () => {
    const report = {
      scene: 'projects/demo/Content/Scenes/maze.rxscene',
      ok: false,
      passed: 5,
      failed: 1,
      durationMs: 270,
      cases: [
        { name: '实体计数=36', kind: 'entity_count', pass: true, actual: 36, expected: 36, detail: '' },
        {
          name: '玩家抵达终点',
          kind: 'transform_near',
          pass: false,
          actual: [2, 0.4, 4],
          expected: [10, 0.4, 10],
          detail: 'maxDeviation=4.0, tolerance=0.3',
        },
      ],
    };
    fetchMock = mockForgeBackend({}, { '/api/forge/playtest/run': report });
    vi.stubGlobal('fetch', fetchMock);

    await useEditorStore.getState().runPlaytest('tests/maze/matrix.json');
    const items = useToastStore.getState().items;
    // 汇总 1 条 + 失败用例 1 条;通过的用例不刷屏
    expect(items).toHaveLength(2);
    expect(items[0].kind).toBe('error');
    expect(items[0].title).toContain('FAIL 5/6');
    expect(items[0].title).toContain('270ms');
    expect(items[1].title).toContain('玩家抵达终点');
    expect(items[1].title).toContain('maxDeviation=4.0');
  });

  it('runPlaytest:全绿 → 单条 success toast', async () => {
    fetchMock = mockForgeBackend(
      {},
      {
        '/api/forge/playtest/run': {
          scene: 's',
          ok: true,
          passed: 6,
          failed: 0,
          durationMs: 120,
          cases: [{ name: 'a', kind: 'entity_count', pass: true, actual: 1, expected: 1, detail: '' }],
        },
      },
    );
    vi.stubGlobal('fetch', fetchMock);

    await useEditorStore.getState().runPlaytest('tests/maze/matrix.json');
    const items = useToastStore.getState().items;
    expect(items).toHaveLength(1);
    expect(items[0].kind).toBe('success');
    expect(items[0].title).toContain('PASS 6/6');
  });

  it('prefillChat:预填文案写入 chatPrefill,clearChatPrefill 清除(F2 wave.3 seam;F7 wave.3 保留)', () => {
    useEditorStore.getState().prefillChat('基于资产 Meshes/cube.gltf 生成变体:');
    let s = useEditorStore.getState();
    expect(s.chatPrefill).toBe('基于资产 Meshes/cube.gltf 生成变体:');
    useEditorStore.getState().clearChatPrefill();
    s = useEditorStore.getState();
    expect(s.chatPrefill).toBeNull();
  });

  // F7 wave.3(D-F7-B):sendChat/multitask/ChatDock 用例随 chat 子集退役删除;
  // agent 对话统一由壳内对话列承接(wave.4 chatStore 测试面)。

  // ---- F1 wave.2:viewport 相机 / 点选 / gizmo ----

  const CAM = { target: [0, 0.5, 0], yaw: 35, pitch: 28, dist: 9, fovY: 50, ortho: false, orthoSize: 5 };
  /** F-GAME-3:2D 正交相机(yaw0/pitch0 正对 XY 平面,orthoSize=5) */
  const CAM2D = { target: [0, 0, 0], yaw: 0, pitch: 0, dist: 10, fovY: 50, ortho: true, orthoSize: 5 };

  it('pickAt:命中设置 selectedId 并切右栏到属性,未命中清空', async () => {
    fetchMock = mockForgeBackend({
      viewport_pick: { hit: true, entityId: 7, name: 'Cube7', point: [0, 0.5, 0] },
    });
    vi.stubGlobal('fetch', fetchMock);

    await useEditorStore.getState().pickAt(64, 48, 128, 96);
    expect(useEditorStore.getState().selectedId).toBe(7);
    expect(useWorkbenchStore.getState().rightTab).toBe('properties');
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
      camera: { target: [0, 0, 0], yaw: 90, pitch: 0, dist: 10, fovY: 90, ortho: false, orthoSize: 5 },
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

  // ---- F-GAME-3:2D 正交视口 ----

  it('orbitCamera:正交(2D)模式下置空,不发请求', async () => {
    useEditorStore.setState({ camera: CAM2D, sceneMode: '2d' });
    await useEditorStore.getState().orbitCamera(100, 10);
    expect(fetchMock.mock.calls.length).toBe(0);
  });

  it('zoomCamera:正交分支调 orthoSize(不动 dist)', async () => {
    const after = { ...CAM2D, orthoSize: 2.5 };
    fetchMock = mockForgeBackend({ viewport_set_camera: after });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ camera: CAM2D, sceneMode: '2d' });
    await useEditorStore.getState().zoomCamera(-800);
    const args = callBody(0).arguments;
    // 正交缩放:orthoSize = 5·1.0015^-800 ≈ 1.506;dist 不动
    expect(args.orthoSize).toBeCloseTo(5 * Math.pow(1.0015, -800), 5);
    expect(args.dist).toBeUndefined();
  });

  it('panCamera:2D 正交平移 target(wpp = 2·orthoSize/viewH)', async () => {
    const after = { ...CAM2D, target: [4, 2, 0] };
    fetchMock = mockForgeBackend({ viewport_set_camera: after });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({ camera: CAM2D, sceneMode: '2d' });
    // wpp = 2·5/500 = 0.02;dx=100,dy=-50 → target.x -= 100·0.02=2(yaw0 → right=[1,0,0])
    // target.y += dy·0.02 = -1(dy 向下为正 → 视野上移);即 target=[-2,-1,0]
    await useEditorStore.getState().panCamera(100, -50, 500);
    const args = callBody(0).arguments;
    const t = args.target as number[];
    expect(t[0]).toBeCloseTo(-2, 5);
    expect(t[1]).toBeCloseTo(-1, 5);
    expect(t[2]).toBeCloseTo(0, 5);
  });

  it('panCameraLocal:本地先行平移(只改本地相机态,不发请求)', () => {
    useEditorStore.setState({ camera: CAM2D, sceneMode: '2d' });
    useEditorStore.getState().panCameraLocal(100, 0, 500);
    const c = useEditorStore.getState().camera;
    expect(c?.target[0]).toBeCloseTo(-2, 5);
    expect(fetchMock.mock.calls.length).toBe(0);
  });

  it('gizmoDragSelected 2D:平移吸附 0.5 网格;snap=false 不吸附', async () => {
    const next = { translation: [0.5, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] };
    fetchMock = mockForgeBackend({ transform_set: next });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({
      camera: CAM2D,
      entities: [CUBE],
      selectedId: 1,
      gizmo: 'translate',
      sceneMode: '2d',
    });
    // wpp=0.02;dx=13 → 0.26 → 吸附 0.5;dy=0
    await useEditorStore.getState().gizmoDragSelected(13, 0, 500);
    let t = callBody(0).arguments.translation as number[];
    expect(t[0]).toBeCloseTo(0.5, 5);
    // Ctrl(snap=false):从原点再拖 0.26 原样(先重置实体位置,排除上次提交影响)
    useEditorStore.setState({ entities: [CUBE] });
    await useEditorStore.getState().gizmoDragSelected(13, 0, 500, false);
    t = callBody(1).arguments.translation as number[];
    expect(t[0]).toBeCloseTo(0.26, 5);
  });

  it('gizmoDragSelected 2D:旋转绕 Z 轴(XY 平面内)', async () => {
    const next = { translation: [0, 0, 0], rotation: [0, 0, 0.7071, 0.7071], scale: [1, 1, 1] };
    fetchMock = mockForgeBackend({ transform_set: next });
    vi.stubGlobal('fetch', fetchMock);
    useEditorStore.setState({
      camera: CAM2D,
      entities: [CUBE],
      selectedId: 1,
      gizmo: 'rotate',
      sceneMode: '2d',
    });
    await useEditorStore.getState().gizmoDragSelected(180, 0, 500);
    const rot = callBody(0).arguments.rotation as number[];
    expect(rot[0]).toBeCloseTo(0, 5);
    expect(rot[1]).toBeCloseTo(0, 5);
    expect(rot[2]).toBeCloseTo(Math.SQRT1_2, 3);
    expect(rot[3]).toBeCloseTo(Math.SQRT1_2, 3);
  });
});
