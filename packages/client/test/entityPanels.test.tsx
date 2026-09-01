import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import HierarchyPanel from '@/components/editor/HierarchyPanel';
import EntityInspectorPanel from '@/components/editor/EntityInspectorPanel';
import { useEditorStore, type EntityData } from '@/lib/editorStore';
import { useGraphStore } from '@/lib/graphStore';
import { mockForgeBackend } from './forgeMock';

const PLAYER: EntityData = {
  id: 33,
  name: 'Player',
  category: 'role',
  transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: [
    { type: 'MeshRenderer', enabled: true, props: { mesh: 'cube', material: '' } },
    {
      type: 'Script',
      enabled: true,
      props: { module: '', graphRef: 'Content/Graphs/maze_player.rxgraph', props: {} },
    },
  ],
};

const KEY: EntityData = {
  id: 34,
  name: 'Key',
  category: 'interaction',
  transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: [
    { type: 'Trigger', enabled: true, props: { kind: 'box', extents: [1, 1, 1] } },
    {
      type: 'Script',
      enabled: true,
      props: { module: '', graphRef: 'Content/Graphs/maze_key.rxgraph', props: {} },
    },
  ],
};

const WALL: EntityData = {
  id: 1,
  name: 'Wall_0_0',
  category: 'map',
  transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: [{ type: 'MeshRenderer', enabled: true, props: { mesh: 'cube' } }],
};

const initial = useEditorStore.getState();
const initialGraph = useGraphStore.getState();

beforeEach(() => {
  useEditorStore.setState(initial, true);
  useGraphStore.setState(initialGraph, true);
  vi.stubGlobal(
    'fetch',
    mockForgeBackend({
      component_list_types: [{ name: 'Category', fields: [{ name: 'category', type: 'enum:role|map|interaction' }] }],
      asset_list: { assets: [] },
      graph_get: {
        graph: {
          version: 1,
          id: 'g',
          name: 'player',
          exposedProps: [],
          nodes: [{ id: 's', type: 'event.on_start', pos: [0, 0] }],
          edges: [],
        },
      },
    }),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('HierarchyPanel 三分类', () => {
  it('按 角色/地图/交互 分组并显示计数', () => {
    useEditorStore.setState({ entities: [PLAYER, KEY, WALL] });
    render(<HierarchyPanel />);

    expect(screen.getByTestId('hierarchy-group-role')).toHaveTextContent('1');
    expect(screen.getByTestId('hierarchy-group-map')).toHaveTextContent('1');
    expect(screen.getByTestId('hierarchy-group-interaction')).toHaveTextContent('1');
    expect(screen.getByTestId('hierarchy-row-33')).toHaveTextContent('Player');
  });
});

describe('EntityInspectorPanel 实体卡片', () => {
  it('选中 Player 展示分类 chip、素材与交互事件', async () => {
    useEditorStore.setState({ entities: [PLAYER], selectedId: 33 });
    render(<EntityInspectorPanel />);

    expect(screen.getByTestId('entity-category-chip')).toHaveTextContent('角色');
    expect(screen.getByTestId('entity-asset-mesh')).toBeInTheDocument();
    expect(screen.getByTestId('entity-asset-graph')).toBeInTheDocument();
    expect(await screen.findByTestId('entity-event-s')).toHaveTextContent('开始时');
  });

  it('选中 Key 展示触发区信息', () => {
    useEditorStore.setState({ entities: [KEY], selectedId: 34 });
    render(<EntityInspectorPanel />);

    expect(screen.getByTestId('entity-category-chip')).toHaveTextContent('交互');
    expect(screen.getByTestId('entity-trigger-info')).toBeInTheDocument();
  });
});
