import { useMemo } from 'react';
import { useAssetStore } from './assetStore';
import { useEditorStore } from './editorStore';

/**
 * UI 融合波 C1:Composer 上下文 chip(07 §5「上下文注入:当前选中实体/资产
 * 作为引用 chip 随消息发送」——「把这个」语义落地)。
 * chip 三源:当前场景(sceneName)/ 选中实体(selectedId)/ 选中资产(selectedGuid)。
 * 注入为消息文本前缀(纯文本 seam,不改后端 ask:execute 契约):
 *   【上下文】场景:X | 实体:#42 Crate | 资产:Content/T.png
 *   <空行>
 *   <用户正文>
 */

export interface ContextChip {
  /** 稳定 key(scene / entity:{id} / asset:{guid});排除态按 key 记。 */
  key: string;
  kind: 'scene' | 'entity' | 'asset';
  /** chip 显示文(短)。 */
  label: string;
  /** 注入前缀片段(完整可解析)。 */
  refText: string;
}

function baseName(p: string): string {
  const i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
  return i >= 0 ? p.slice(i + 1) : p;
}

/** 订阅三源装配 chip 列表(选择变化即重算)。 */
export function useContextChips(): ContextChip[] {
  const sceneName = useEditorStore((s) => s.sceneName);
  const selectedId = useEditorStore((s) => s.selectedId);
  const entities = useEditorStore((s) => s.entities);
  const selectedGuid = useAssetStore((s) => s.selectedGuid);
  const items = useAssetStore((s) => s.items);

  return useMemo(() => {
    const chips: ContextChip[] = [];
    if (sceneName !== '') {
      chips.push({
        key: 'scene',
        kind: 'scene',
        label: `@${sceneName}`,
        refText: `场景:${sceneName}`,
      });
    }
    if (selectedId !== null) {
      const e = entities.find((x) => x.id === selectedId);
      if (e) {
        chips.push({
          key: `entity:${e.id}`,
          kind: 'entity',
          label: `#${e.id} ${e.name}`,
          refText: `实体:#${e.id} ${e.name}`,
        });
      }
    }
    if (selectedGuid !== null) {
      const a = items.find((x) => x.guid === selectedGuid);
      if (a) {
        // F10:简介注入(截断 60 字控 token;contextUsage 环如实计量)。
        const desc = (a.description ?? '').trim();
        const brief = desc === '' ? '' : `(${desc.length > 60 ? `${desc.slice(0, 60)}…` : desc})`;
        chips.push({
          key: `asset:${a.guid}`,
          kind: 'asset',
          label: baseName(a.path),
          refText: `资产:${a.path}${brief}`,
        });
      }
    }
    return chips;
  }, [sceneName, selectedId, entities, selectedGuid, items]);
}

/** 注入前缀(无 chip 返空串,正文原样)。 */
export function chipsPrefix(chips: ContextChip[]): string {
  if (chips.length === 0) return '';
  return `【上下文】${chips.map((c) => c.refText).join(' | ')}\n\n`;
}
