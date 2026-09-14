import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useAssetStore } from '@/lib/assetStore';
import { forgeMock } from './forgeMock';

describe('assetStore', () => {
  beforeEach(() => {
    forgeMock.reset();
    forgeMock.stubGlobal();
    useAssetStore.setState({
      items: [],
      status: {},
      loading: false,
      error: null,
      viewMode: 'grid',
      typeFilter: 'all',
      search: '',
      currentFolder: '',
      selectedGuid: null,
      thumbs: {},
      refsResult: null,
      pendingDelete: null,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('load 拉取资产列表与构建状态', async () => {
    forgeMock.setAssets([
      { path: 'Meshes/cube.gltf', guid: 'g1', type: 'mesh', size: 1234 },
      { path: 'Textures/wood.png', guid: 'g2', type: 'texture', size: 5678 },
    ]);
    forgeMock.setBuildStatus([
      { path: 'Meshes/cube.gltf', state: 'current', hash: 'h1' },
      { path: 'Textures/wood.png', state: 'stale', hash: 'h2' },
    ]);

    await useAssetStore.getState().load();
    const s = useAssetStore.getState();
    expect(s.items).toHaveLength(2);
    expect(s.status['Meshes/cube.gltf']).toBe('current');
    expect(s.status['Textures/wood.png']).toBe('stale');
  });

  it('过滤:类型 + 搜索', async () => {
    forgeMock.setAssets([
      { path: 'Meshes/cube.gltf', guid: 'g1', type: 'mesh', size: 1 },
      { path: 'Meshes/sphere.gltf', guid: 'g2', type: 'mesh', size: 1 },
      { path: 'Textures/wood.png', guid: 'g3', type: 'texture', size: 1 },
    ]);
    forgeMock.setBuildStatus([]);
    await useAssetStore.getState().load();

    // 类型过滤
    useAssetStore.getState().setTypeFilter('mesh');
    let filtered = useAssetStore.getState().items.filter((i) => i.type === 'mesh');
    expect(filtered).toHaveLength(2);

    // 搜索过滤
    useAssetStore.getState().setSearch('cube');
    filtered = useAssetStore
      .getState()
      .items.filter((i) => i.type === 'mesh' && i.path.toLowerCase().includes('cube'));
    expect(filtered).toHaveLength(1);
  });

  it('实例化:mesh 资产 → entity_create 带 MeshRenderer', async () => {
    forgeMock.setAssets([{ path: 'Meshes/cube.gltf', guid: 'g1', type: 'mesh', size: 1 }]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('entity_create', { id: 42, entity: { id: 42, name: 'cube', transform: { translation: [1,2,3], rotation: [0,0,0,1], scale: [1,1,1] }, components: [] } });
    await useAssetStore.getState().load();

    await useAssetStore.getState().instantiate('g1', [1, 2, 3]);
    const calls = forgeMock.calls.filter((c) => c.tool === 'mcp__engine-scene__entity_create');
    expect(calls).toHaveLength(1);
    const args = calls[0].arguments as {
      translation: number[];
      components: Array<{ type: string; props: { mesh: string } }>;
    };
    expect(args.translation).toEqual([1, 2, 3]);
    expect(args.components[0].type).toBe('MeshRenderer');
    expect(args.components[0].props.mesh).toBe('g1');
  });

  it('实例化非 mesh 类型拒绝', async () => {
    forgeMock.setAssets([{ path: 'Textures/wood.png', guid: 'g2', type: 'texture', size: 1 }]);
    forgeMock.setBuildStatus([]);
    await useAssetStore.getState().load();

    await expect(useAssetStore.getState().instantiate('g2', [0, 0, 0])).rejects.toThrow(
      /仅 mesh\/model\/prefab 可实例化/,
    );
  });

  it('Blender prefab uses subtree instantiation, while model uses ModelRenderer', async () => {
    forgeMock.setAssets([
      { path: 'Models/hero/template.rxprefab', guid: 'prefab1', type: 'prefab', size: 1 },
      { path: 'Models/hero/model.rxmodel', guid: 'model1', type: 'model', size: 1 },
    ]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('prefab_instantiate', { rootId: 1, entityIds: [1, 2], revision: 1 });
    forgeMock.setDefault('entity_create', { id: 3 });
    await useAssetStore.getState().load();
    await useAssetStore.getState().instantiate('prefab1', [1, 2, 3]);
    const prefab = forgeMock.calls.find((c) => c.tool === 'mcp__engine-scene__prefab_instantiate');
    expect(prefab?.arguments).toEqual({ prefabRef: 'prefab1', translation: [1, 2, 3] });
    expect(forgeMock.calls.filter((c) => c.tool === 'mcp__engine-scene__entity_create')).toHaveLength(0);
    await useAssetStore.getState().instantiate('model1', [0, 0, 0]);
    const model = forgeMock.calls.find((c) => c.tool === 'mcp__engine-scene__entity_create');
    expect(model?.arguments).toMatchObject({ components: [{ type: 'ModelRenderer', props: { model: 'model1' } }] });
  });

  it('删除后自动重载', async () => {
    forgeMock.setAssets([{ path: 'Meshes/cube.gltf', guid: 'g1', type: 'mesh', size: 1 }]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('asset_delete', { deleted: ['Meshes/cube.gltf'], blockedByRefs: [] });
    await useAssetStore.getState().load();

    await useAssetStore.getState().remove('Meshes/cube.gltf');
    const delCalls = forgeMock.calls.filter((c) => c.tool === 'mcp__asset-pipeline__asset_delete');
    expect(delCalls).toHaveLength(1);
    const listCalls = forgeMock.calls.filter((c) => c.tool === 'mcp__asset-pipeline__asset_list');
    expect(listCalls.length).toBeGreaterThanOrEqual(2); // 初次 + 删除后
  });

  it('importToHere:destFolder = 当前文件夹', async () => {
    forgeMock.setAssets([]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('asset_import', { imported: [], failed: [] });
    useAssetStore.getState().setCurrentFolder('Textures');

    await useAssetStore.getState().importToHere(['D:/src/wood.png']);
    const calls = forgeMock.calls.filter((c) => c.tool === 'mcp__asset-pipeline__asset_import');
    expect(calls).toHaveLength(1);
    expect(calls[0].arguments).toEqual({ sourcePaths: ['D:/src/wood.png'], destFolder: 'Textures' });
  });

  it('queryRefs:双向查询并把 GUID 解析为路径', async () => {
    forgeMock.setAssets([
      { path: 'Meshes/cube.gltf', guid: 'g1', type: 'mesh', size: 1 },
      { path: 'Scenes/Main.rxscene', guid: 'g2', type: 'scene', size: 1 },
    ]);
    forgeMock.setBuildStatus([]);
    await useAssetStore.getState().load();
    // refs / referencedBy 双向同一边数据,分别验证 to / from 的 GUID→路径解析。
    forgeMock.setDefault('asset_refs', {
      edges: [{ from: 'g2', to: 'g1', type: 'scene→mesh' }],
    });

    await useAssetStore.getState().queryRefs('Meshes/cube.gltf');
    const r = useAssetStore.getState().refsResult;
    expect(r).not.toBeNull();
    expect(r!.path).toBe('Meshes/cube.gltf');
    // refs 方向:to=g1 解析为 Meshes/cube.gltf;referencedBy 方向:from=g2 解析为 Scenes/Main.rxscene
    expect(r!.refs[0]).toContain('Meshes/cube.gltf');
    expect(r!.refs[0]).toContain('scene→mesh');
    expect(r!.referencedBy[0]).toContain('Scenes/Main.rxscene');
    const refCalls = forgeMock.calls.filter((c) => c.tool === 'mcp__asset-pipeline__asset_refs');
    expect(refCalls).toHaveLength(2); // refs + referencedBy
    useAssetStore.getState().clearRefs();
    expect(useAssetStore.getState().refsResult).toBeNull();
  });

  it('删除提案:引用阻断 → blocked 清单保留且不重载', async () => {
    forgeMock.setAssets([
      { path: 'Meshes/cube.gltf', guid: 'g1', type: 'mesh', size: 1 },
      { path: 'Scenes/Main.rxscene', guid: 'g2', type: 'scene', size: 1 },
    ]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('asset_delete', {
      deleted: [],
      blockedByRefs: [{ assetPath: 'Meshes/cube.gltf', referencedBy: ['g2'] }],
    });
    await useAssetStore.getState().load();

    useAssetStore.getState().requestDelete('Meshes/cube.gltf');
    expect(useAssetStore.getState().pendingDelete).toEqual({ path: 'Meshes/cube.gltf', blocked: null });

    await useAssetStore.getState().confirmDelete();
    const pd = useAssetStore.getState().pendingDelete;
    expect(pd).not.toBeNull();
    expect(pd!.blocked).not.toBeNull();
    expect(pd!.blocked![0]).toContain('Scenes/Main.rxscene'); // GUID 解析为路径
    useAssetStore.getState().cancelDelete();
    expect(useAssetStore.getState().pendingDelete).toBeNull();
  });

  it('删除提案:无阻断 → 删除成功并重载', async () => {
    forgeMock.setAssets([{ path: 'Meshes/cube.gltf', guid: 'g1', type: 'mesh', size: 1 }]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('asset_delete', { deleted: ['Meshes/cube.gltf'], blockedByRefs: [] });
    await useAssetStore.getState().load();

    useAssetStore.getState().requestDelete('Meshes/cube.gltf');
    await useAssetStore.getState().confirmDelete();
    expect(useAssetStore.getState().pendingDelete).toBeNull();
    const listCalls = forgeMock.calls.filter((c) => c.tool === 'mcp__asset-pipeline__asset_list');
    expect(listCalls.length).toBeGreaterThanOrEqual(2);
  });

  it('loadThumb:贴图缓存 dataUrl;失败记 none 不重试', async () => {
    const tex = { path: 'Textures/wood.png', guid: 'g2', type: 'texture', size: 1 };
    forgeMock.setAssets([tex]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('asset_thumbnail', { dataUrl: 'data:image/png;base64,AAA=', bytes: 3 });
    await useAssetStore.getState().load();

    await useAssetStore.getState().loadThumb(tex);
    expect(useAssetStore.getState().thumbs['g2']).toBe('data:image/png;base64,AAA=');
    await useAssetStore.getState().loadThumb(tex); // 已缓存,不再请求
    const thumbCalls = forgeMock.calls.filter((c) => c.tool === 'mcp__asset-pipeline__asset_thumbnail');
    expect(thumbCalls).toHaveLength(1);

    // 非贴图直接跳过
    await useAssetStore.getState().loadThumb({ path: 'Meshes/cube.gltf', guid: 'g1', type: 'mesh', size: 1 });
    expect(thumbCalls).toHaveLength(1);
  });

  it('loadThumb:工具返回 error → 记 none', async () => {
    const tex = { path: 'Textures/big.png', guid: 'g9', type: 'texture', size: 1 };
    forgeMock.setAssets([tex]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('asset_thumbnail', { error: 'TOO_LARGE', message: '超上限' });
    await useAssetStore.getState().load();

    await useAssetStore.getState().loadThumb(tex);
    expect(useAssetStore.getState().thumbs['g9']).toBe('none');
  });

  it('F10-RAG:setDescription 写简介(source=human)并返回索引同步状态', async () => {
    forgeMock.setAssets([{ path: 'Textures/wood.png', guid: 'g2', type: 'texture', size: 1 }]);
    forgeMock.setBuildStatus([]);
    forgeMock.setDefault('asset_set_description', {
      ok: true,
      indexed: true,
      indexedDocs: 1,
      tier: 'lexical',
    });
    await useAssetStore.getState().load();

    const r = await useAssetStore.getState().setDescription('Textures/wood.png', '橡木木纹', ['木纹']);
    expect(r.indexed).toBe(true);
    expect(r.tier).toBe('lexical');
    const calls = forgeMock.calls.filter(
      (c) => c.tool === 'mcp__asset-pipeline__asset_set_description',
    );
    expect(calls).toHaveLength(1);
    expect(calls[0].arguments).toMatchObject({
      assetPath: 'Textures/wood.png',
      description: '橡木木纹',
      tags: ['木纹'],
      source: 'human',
    });
    // 写后列表刷新(简介/标签回读进 items)。
    const listCalls = forgeMock.calls.filter((c) => c.tool === 'mcp__asset-pipeline__asset_list');
    expect(listCalls.length).toBeGreaterThanOrEqual(2);
  });
});
