import { User, Map, Zap, Box, FileCode, Package } from 'lucide-react';
import type { EntityData } from './editorStore';

/** 实体三大类(与 forge-scene classify 对齐) */
export type EntityCategory = 'role' | 'map' | 'interaction';

export const CATEGORY_ORDER: EntityCategory[] = ['role', 'map', 'interaction'];

export const CATEGORY_META: Record<
  EntityCategory,
  { label: string; icon: typeof User; chipClass: string; headerClass: string }
> = {
  role: {
    label: '角色',
    icon: User,
    chipClass: 'bg-info/15 text-info border-info/30',
    headerClass: 'text-info',
  },
  map: {
    label: '地图',
    icon: Map,
    chipClass: 'bg-sage/15 text-sage border-sage/30',
    headerClass: 'text-sage',
  },
  interaction: {
    label: '交互',
    icon: Zap,
    chipClass: 'bg-warn/15 text-warn border-warn/30',
    headerClass: 'text-warn',
  },
};

/** event.* 节点 → 中文展示名(Scratch 式交互事件) */
export const EVENT_NODE_LABELS: Record<string, string> = {
  'event.on_start': '开始时',
  'event.on_update': '每帧更新',
  'event.on_input': '收到输入',
  'event.on_contact_begin': '碰撞开始',
  'event.on_contact_end': '碰撞结束',
  'event.on_trigger_enter': '进入触发区',
  'event.on_trigger_exit': '离开触发区',
};

export function eventNodeLabel(type: string): string {
  return EVENT_NODE_LABELS[type] ?? type.replace(/^event\./, '');
}

/** 实体上的素材引用(从组件 props 抽取) */
export interface EntityAssetRef {
  key: string;
  label: string;
  path: string;
  kind: 'mesh' | 'material' | 'script' | 'graph';
}

export function extractAssetRefs(entity: EntityData): EntityAssetRef[] {
  const refs: EntityAssetRef[] = [];
  for (const c of entity.components) {
    if (!c.enabled) continue;
    if (c.type === 'ModelRenderer' && typeof c.props.model === 'string' && c.props.model) {
      refs.push({ key: `model:${c.props.model}`, label: '3D 模型', path: c.props.model, kind: 'mesh' });
    }
    if (c.type === 'MeshRenderer') {
      const mesh = c.props.mesh;
      if (typeof mesh === 'string' && mesh.trim() !== '') {
        refs.push({ key: `mesh:${mesh}`, label: '网格', path: mesh, kind: 'mesh' });
      }
      const material = c.props.material;
      if (typeof material === 'string' && material.trim() !== '') {
        refs.push({ key: `mat:${material}`, label: '材质', path: material, kind: 'material' });
      }
    }
    if (c.type === 'Script') {
      const module = c.props.module;
      if (typeof module === 'string' && module.trim() !== '') {
        refs.push({ key: `mod:${module}`, label: '脚本', path: module, kind: 'script' });
      }
      const graphRef = c.props.graphRef;
      if (typeof graphRef === 'string' && graphRef.trim() !== '') {
        refs.push({ key: `graph:${graphRef}`, label: '节点图', path: graphRef, kind: 'graph' });
      }
    }
  }
  return refs;
}

export function assetRefIcon(kind: EntityAssetRef['kind']) {
  switch (kind) {
    case 'mesh':
      return Box;
    case 'material':
      return Package;
    case 'script':
    case 'graph':
      return FileCode;
    default:
      return Box;
  }
}

/** 客户端兜底推断(后端未返 category 时用) */
export function inferCategory(entity: EntityData): EntityCategory {
  const explicit = entity.components.find((c) => c.type === 'Category' && c.enabled);
  const cat = explicit?.props?.category;
  if (cat === 'role' || cat === 'map' || cat === 'interaction') return cat;

  const tag = entity.components.find((c) => c.type === 'Tag' && c.enabled);
  if (tag?.props?.tag === 'player') return 'role';

  const rb = entity.components.find((c) => c.type === 'RigidBody' && c.enabled);
  const kind = rb?.props?.kind;
  if (kind === 'dynamic' || kind === 'kinematic') return 'role';

  if (entity.components.some((c) => c.type === 'Trigger' && c.enabled)) return 'interaction';

  const script = entity.components.find((c) => c.type === 'Script' && c.enabled);
  if (script) {
    const mod = script.props.module;
    const graph = script.props.graphRef;
    if ((typeof mod === 'string' && mod.trim() !== '') || (typeof graph === 'string' && graph.trim() !== '')) {
      return 'interaction';
    }
  }

  return 'map';
}

export function entityCategory(entity: EntityData): EntityCategory {
  if (entity.category === 'role' || entity.category === 'map' || entity.category === 'interaction') {
    return entity.category;
  }
  return inferCategory(entity);
}
