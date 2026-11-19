import { cn } from '@/lib/cn';
import { useRigStore } from '@/lib/rigStore';
import { bindingToWeights, type BrushMode } from './RigWeightBrush';

/**
 * 权重模式右栏(F-GAME-5 W5):笔刷骨骼(热力图显示对象)、加/减/平滑、半径(画布像素)与强度;
 * 选中部件的绑定摘要(各骨骼主导顶点数)。笔刷结果写 part.weights(清 part.bone),保存时 diff 成 set_weights。
 */

const sectionTitle = 'px-2.5 pb-1 pt-2 text-2xs font-medium text-fg-2';
const BRUSH_MODES: Array<{ key: BrushMode; label: string }> = [
  { key: 'add', label: '加 Add' },
  { key: 'subtract', label: '减 Sub' },
  { key: 'smooth', label: '平滑 Smooth' },
];

export default function RigWeightPanel() {
  const doc = useRigStore((s) => s.doc);
  const brush = useRigStore((s) => s.brush);
  const selection = useRigStore((s) => s.selection);
  const st = useRigStore.getState;
  if (!doc) return null;
  const part = selection?.kind === 'part' ? doc.parts.find((p) => p.id === selection.id) : undefined;

  const dominant: Record<string, number> = {};
  let unbound = 0;
  if (part) {
    for (const inf of bindingToWeights(part)) {
      if (inf.length === 0) {
        unbound += 1;
        continue;
      }
      const top = inf.reduce((a, b) => (b[1] > a[1] ? b : a));
      dominant[top[0]] = (dominant[top[0]] ?? 0) + 1;
    }
  }

  return (
    <div className="flex flex-col" data-testid="rig-weight-panel">
      <div className="border-b border-edge pb-2">
        <p className={sectionTitle}>笔刷 Brush</p>
        <div className="flex gap-0.5 px-2.5">
          {BRUSH_MODES.map((m) => (
            <button
              key={m.key}
              type="button"
              data-testid={`rig-brush-${m.key}`}
              onClick={() => st().setBrush({ mode: m.key })}
              className={cn('glass-btn h-6 flex-1 justify-center', brush.mode === m.key && 'ring-1 ring-acc text-fg')}
            >
              {m.label}
            </button>
          ))}
        </div>
        <label className="flex items-center gap-1.5 px-2.5 pt-1.5 text-2xs text-fg-3">
          <span className="w-10 shrink-0">半径</span>
          <input
            type="range"
            min={2}
            max={300}
            step={1}
            value={brush.radius}
            data-testid="rig-brush-radius"
            onChange={(e) => st().setBrush({ radius: Number(e.target.value) })}
            className="min-w-0 flex-1"
          />
          <span className="w-8 shrink-0 text-right font-mono">{brush.radius}</span>
        </label>
        <label className="flex items-center gap-1.5 px-2.5 pt-1 text-2xs text-fg-3">
          <span className="w-10 shrink-0">强度</span>
          <input
            type="range"
            min={0.01}
            max={1}
            step={0.01}
            value={brush.strength}
            data-testid="rig-brush-strength"
            onChange={(e) => st().setBrush({ strength: Number(e.target.value) })}
            className="min-w-0 flex-1"
          />
          <span className="w-8 shrink-0 text-right font-mono">{brush.strength.toFixed(2)}</span>
        </label>
      </div>

      <div className="border-b border-edge pb-2">
        <p className={sectionTitle}>笔刷骨骼 Bone({doc.bones.length})</p>
        {doc.bones.length === 0 && <p className="px-2.5 text-2xs text-fg-4">无骨骼:先在骨骼模式里建骨</p>}
        <div className="flex flex-col gap-0.5 px-1.5">
          {doc.bones.map((b) => (
            <button
              key={b.id}
              type="button"
              data-testid={`rig-brush-bone-${b.id}`}
              onClick={() => st().setBrush({ bone: b.id })}
              className={cn(
                'flex items-center justify-between rounded-md border px-1.5 py-0.5 text-left text-2xs',
                brush.bone === b.id ? 'border-acc bg-shell-active text-fg' : 'border-transparent text-fg-3 hover:bg-shell-hover',
              )}
            >
              <span className="truncate font-mono">{b.id}</span>
              {part && dominant[b.id] !== undefined && <span className="text-fg-4">{dominant[b.id]} v</span>}
            </button>
          ))}
        </div>
      </div>

      <div className="pb-2">
        <p className={sectionTitle}>部件 Part</p>
        {!part ? (
          <p className="px-2.5 text-2xs text-fg-4">在画布上点一个部件开始刷权重</p>
        ) : (
          <div className="space-y-0.5 px-2.5 text-2xs text-fg-3" data-testid="rig-weight-part">
            <p className="font-mono text-fg-2">{part.id}</p>
            <p>{part.weights ? '权重蒙皮' : part.bone ? `刚性绑定 → ${part.bone}(落笔即转为权重)` : '未绑定'}</p>
            {unbound > 0 && <p className="text-warn">{unbound} 个顶点无影响</p>}
            <p className="text-fg-4">热力图:蓝 = 0,绿 = 0.5,红 = 1(笔刷骨骼的归一化权重)</p>
          </div>
        )}
      </div>
    </div>
  );
}
