import { describe, expect, it } from 'vitest';
import {
  CATEGORY_ORDER,
  entityCategory,
  extractAssetRefs,
  inferCategory,
  eventNodeLabel,
} from '@/lib/entityCategory';
import type { EntityData } from '@/lib/editorStore';

const player: EntityData = {
  id: 33,
  name: 'Player',
  category: 'role',
  transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: [
    { type: 'MeshRenderer', enabled: true, props: { mesh: 'cube', material: '' } },
    { type: 'Tag', enabled: true, props: { tag: 'player' } },
    {
      type: 'Script',
      enabled: true,
      props: { module: '', graphRef: 'Content/Graphs/maze_player.rxgraph', props: {} },
    },
  ],
};

describe('entityCategory', () => {
  it('CATEGORY_ORDER 固定三类顺序', () => {
    expect(CATEGORY_ORDER).toEqual(['role', 'map', 'interaction']);
  });

  it('entityCategory 优先用后端 category 字段', () => {
    expect(entityCategory(player)).toBe('role');
  });

  it('inferCategory:无 category 时按组件推断', () => {
    const wall: EntityData = {
      id: 1,
      name: 'Wall',
      transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
      components: [{ type: 'MeshRenderer', enabled: true, props: { mesh: 'cube' } }],
    };
    expect(inferCategory(wall)).toBe('map');
    expect(inferCategory({ ...player, category: undefined })).toBe('role');
  });

  it('extractAssetRefs 抽取 mesh 与 graphRef', () => {
    const refs = extractAssetRefs(player);
    expect(refs.some((r) => r.kind === 'mesh' && r.path === 'cube')).toBe(true);
    expect(refs.some((r) => r.kind === 'graph' && r.path.includes('maze_player'))).toBe(true);
  });

  it('eventNodeLabel 映射中文', () => {
    expect(eventNodeLabel('event.on_start')).toBe('开始时');
    expect(eventNodeLabel('event.on_trigger_enter')).toBe('进入触发区');
  });
});
