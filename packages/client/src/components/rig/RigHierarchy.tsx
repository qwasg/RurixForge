import { ChevronDown, ChevronUp, Eye, EyeOff, Trash2 } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useRigStore } from '@/lib/rigStore';
import type { RigBone } from '@/lib/rigTypes';

/**
 * 绑骨编辑器左栏层级(F-GAME-5 W5):部件(按 drawOrder 自上而下 = 后画在上;显隐 / 上移下移)、
 * 骨骼树(按 parent 缩进;删除)、形变器列表。点击行 = 选中(画布同步高亮)。
 */

const iconBtn =
  'flex h-5 w-5 shrink-0 items-center justify-center rounded text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-30';
const sectionTitle = 'px-2.5 pb-1 pt-2 text-2xs font-medium text-fg-2';
const rowCls = 'flex items-center gap-1 rounded-md border px-1 py-0.5 text-2xs';

function boneTree(bones: RigBone[]): Array<{ bone: RigBone; depth: number }> {
  const ids = new Set(bones.map((b) => b.id));
  const children = new Map<string | null, RigBone[]>();
  for (const b of bones) {
    const p = b.parent && ids.has(b.parent) ? b.parent : null;
    const list = children.get(p) ?? [];
    list.push(b);
    children.set(p, list);
  }
  const out: Array<{ bone: RigBone; depth: number }> = [];
  const seen = new Set<string>();
  const walk = (parent: string | null, depth: number) => {
    for (const b of children.get(parent) ?? []) {
      if (seen.has(b.id)) continue;
      seen.add(b.id);
      out.push({ bone: b, depth });
      walk(b.id, depth + 1);
    }
  };
  walk(null, 0);
  // 成环等异常数据:剩余骨骼平铺,不丢。
  for (const b of bones) if (!seen.has(b.id)) out.push({ bone: b, depth: 0 });
  return out;
}

export default function RigHierarchy() {
  const doc = useRigStore((s) => s.doc);
  const selection = useRigStore((s) => s.selection);
  const st = useRigStore.getState;
  if (!doc) return null;

  const parts = [...doc.parts].sort((a, b) => b.drawOrder - a.drawOrder);
  const isSel = (kind: string, id: string) => selection?.kind === kind && selection.id === id;

  return (
    <div
      className="flex w-[200px] shrink-0 flex-col overflow-y-auto border-r border-edge bg-shell-sidebar pb-2"
      data-testid="rig-hierarchy"
    >
      <p className={sectionTitle}>部件 Parts({parts.length})</p>
      {parts.length === 0 && <p className="px-2.5 text-2xs text-fg-4">无部件(用 PSD 导入或 rig_parts_from_sheet 生成)</p>}
      <div className="flex flex-col gap-0.5 px-1.5">
        {parts.map((p, i) => {
          const hidden = p.visible === false;
          return (
            <div
              key={p.id}
              data-testid={`rig-part-row-${p.id}`}
              onClick={() => st().select({ kind: 'part', id: p.id })}
              className={cn(
                rowCls,
                isSel('part', p.id) ? 'border-acc bg-shell-active' : 'border-transparent hover:bg-shell-hover',
              )}
            >
              <button
                type="button"
                className={iconBtn}
                title={hidden ? '显示 / Show' : '隐藏 / Hide'}
                data-testid={`rig-part-vis-${p.id}`}
                onClick={(e) => {
                  e.stopPropagation();
                  st().setPartVisible(p.id, hidden);
                }}
              >
                {hidden ? <EyeOff size={11} /> : <Eye size={11} />}
              </button>
              <span className={cn('min-w-0 flex-1 truncate', hidden ? 'text-fg-4' : 'text-fg-2')} title={p.id}>
                {p.id}
              </span>
              <span className="shrink-0 font-mono text-fg-4" title="drawOrder">
                {p.drawOrder}
              </span>
              <button
                type="button"
                className={iconBtn}
                title="上移(后画,更靠前)"
                disabled={i === 0}
                data-testid={`rig-part-up-${p.id}`}
                onClick={(e) => {
                  e.stopPropagation();
                  st().moveDrawOrder(p.id, 1);
                }}
              >
                <ChevronUp size={11} />
              </button>
              <button
                type="button"
                className={iconBtn}
                title="下移(先画,更靠后)"
                disabled={i === parts.length - 1}
                data-testid={`rig-part-down-${p.id}`}
                onClick={(e) => {
                  e.stopPropagation();
                  st().moveDrawOrder(p.id, -1);
                }}
              >
                <ChevronDown size={11} />
              </button>
            </div>
          );
        })}
      </div>

      <p className={sectionTitle}>骨骼 Bones({doc.bones.length})</p>
      {doc.bones.length === 0 && (
        <p className="px-2.5 text-2xs text-fg-4">无骨骼:骨骼模式下在画布空白处拖拽新建</p>
      )}
      <div className="flex flex-col gap-0.5 px-1.5">
        {boneTree(doc.bones).map(({ bone, depth }) => (
          <div
            key={bone.id}
            data-testid={`rig-bone-row-${bone.id}`}
            onClick={() => st().select({ kind: 'bone', id: bone.id })}
            className={cn(
              rowCls,
              isSel('bone', bone.id) ? 'border-acc bg-shell-active' : 'border-transparent hover:bg-shell-hover',
            )}
            style={{ paddingLeft: 4 + depth * 10 }}
          >
            <span className="min-w-0 flex-1 truncate text-fg-2" title={bone.id}>
              {bone.id}
            </span>
            <span className="shrink-0 font-mono text-fg-4" title="length / rotation">
              {Math.round(bone.length)}·{Math.round(bone.rotation)}°
            </span>
            <button
              type="button"
              className={iconBtn}
              title="删除骨骼(子骨骼挂到父上,权重剔除)"
              data-testid={`rig-bone-del-${bone.id}`}
              onClick={(e) => {
                e.stopPropagation();
                st().removeBone(bone.id);
              }}
            >
              <Trash2 size={11} />
            </button>
          </div>
        ))}
      </div>

      <p className={sectionTitle}>形变器 Deformers({doc.deformers.length})</p>
      {doc.deformers.length === 0 && <p className="px-2.5 text-2xs text-fg-4">无</p>}
      <div className="flex flex-col gap-0.5 px-1.5">
        {doc.deformers.map((d) => (
          <div
            key={d.id}
            data-testid={`rig-deformer-row-${d.id}`}
            onClick={() => st().select({ kind: 'deformer', id: d.id })}
            className={cn(
              rowCls,
              isSel('deformer', d.id) ? 'border-acc bg-shell-active' : 'border-transparent hover:bg-shell-hover',
            )}
          >
            <span className="min-w-0 flex-1 truncate text-fg-2" title={d.id}>
              {d.id}
            </span>
            <span className="shrink-0 text-fg-4">
              {d.type === 'warp' ? `warp ${d.grid[0]}×${d.grid[1]}` : 'rotation'}
              {d.parent ? ` ← ${d.parent}` : ''}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
