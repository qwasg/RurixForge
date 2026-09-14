import { describe, it, expect } from 'vitest';
import { hierarchyRows } from '../src/components/editor/HierarchyPanel';
import type { EntityData } from '../src/lib/editorStore';

const entity = (id: number, parent?: number): EntityData => ({
  id, name: `Node ${id}`, transform: { translation: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] },
  components: parent === undefined ? [] : [{ type: 'Parent', enabled: true, props: { entity: parent } }],
});
describe('Blender template hierarchy', () => {
  it('orders parents before children and collapses entire subtrees', () => {
    const nodes = [entity(3, 2), entity(4), entity(1), entity(2, 1)];
    expect(hierarchyRows(nodes).map((r) => [r.entity.id, r.depth])).toEqual([[4, 0], [1, 0], [2, 1], [3, 2]]);
    expect(hierarchyRows(nodes, new Set([1])).map((r) => r.entity.id)).toEqual([4, 1]);
  });
  it('keeps orphaned and cyclic nodes selectable without recursive loops', () => {
    const rows = hierarchyRows([entity(1, 2), entity(2, 1), entity(3, 99)]);
    expect(rows).toHaveLength(3);
    expect(new Set(rows.map((r) => r.entity.id)).size).toBe(3);
  });
});
