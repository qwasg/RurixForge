import { useEffect, useMemo, useState } from 'react';
import { Crosshair, Eraser, Magnet, RotateCcw } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useRigStore } from '@/lib/rigStore';
import { keyformIndex, type RigDoc, type RigParam } from '@/lib/rigTypes';

/**
 * 绑骨编辑器右栏(网格/骨骼/参数/动画模式;F-GAME-5 W5):
 * - 选中项检视(部件绑定摘要 / 选中顶点坐标 / 骨骼静止姿态);
 * - 参数滑杆(每参数一条;吸附按钮 = 该参数在 keyform 组里出现过的关键值 + 默认值);
 * - 参数模式下的「记录 keyform」:选部件 + 1–3 个参数轴 + 每轴关键值,当前参数恰在关键值上时,
 *   把画布上拖过的顶点写成 set_keyform(offsets = 拖后位置 − 当前姿势求值位置,叠加原形态)。
 */

const sectionTitle = 'px-2.5 pb-1 pt-2 text-2xs font-medium text-fg-2';
const numCls =
  'w-full min-w-0 rounded border border-edge bg-shell-input px-1 py-0.5 text-right font-mono text-2xs text-fg outline-none focus:border-fg-4';
const inputCls =
  'w-full min-w-0 rounded border border-edge bg-shell-input px-1.5 py-0.5 font-mono text-2xs text-fg outline-none focus:border-fg-4';
const smallBtn = 'glass-btn h-5 px-1.5 text-2xs disabled:cursor-not-allowed';

function fmt(v: number): string {
  return Number.isInteger(v) ? String(v) : v.toFixed(2);
}

/** 参数的吸附候选:keyform 组里该参数轴的全部关键值 ∪ {default},升序去重。 */
export function paramSnapKeys(doc: RigDoc, p: RigParam): number[] {
  const out = new Set<number>([p.default]);
  for (const g of doc.keyforms) {
    g.params.forEach((pid, ai) => {
      if (pid === p.id) for (const k of g.keys[ai] ?? []) out.add(k);
    });
  }
  return [...out].sort((a, b) => a - b);
}

function NumField({ value, onCommit, testid }: { value: number; onCommit: (v: number) => void; testid?: string }) {
  const [draft, setDraft] = useState(fmt(value));
  useEffect(() => setDraft(fmt(value)), [value]);
  const commit = () => {
    const v = Number(draft);
    if (draft.trim() !== '' && Number.isFinite(v)) onCommit(v);
    else setDraft(fmt(value));
  };
  return (
    <input
      value={draft}
      data-testid={testid}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit();
      }}
      className={numCls}
    />
  );
}

function SelectionSection() {
  const doc = useRigStore((s) => s.doc);
  const selection = useRigStore((s) => s.selection);
  const selectedVertex = useRigStore((s) => s.selectedVertex);
  const mode = useRigStore((s) => s.mode);
  const st = useRigStore.getState;
  if (!doc || !selection) {
    return (
      <div className="border-b border-edge pb-2">
        <p className={sectionTitle}>选中 Selection</p>
        <p className="px-2.5 text-2xs text-fg-4">在画布或左栏点选部件 / 骨骼</p>
      </div>
    );
  }
  if (selection.kind === 'part') {
    const p = doc.parts.find((x) => x.id === selection.id);
    if (!p) return null;
    const binding = p.weights ? `权重蒙皮(${p.weights.length} 顶点)` : p.bone ? `刚性 → ${p.bone}` : '未绑定';
    const v = selectedVertex !== null ? p.mesh.vertices[selectedVertex] : undefined;
    return (
      <div className="border-b border-edge pb-2" data-testid="rig-selection-part">
        <p className={sectionTitle}>部件 · {p.id}</p>
        <div className="space-y-0.5 px-2.5 text-2xs text-fg-3">
          <p>
            顶点 {p.mesh.vertices.length} · 三角形 {p.mesh.triangles.length} · drawOrder {p.drawOrder}
          </p>
          <p>绑定:{binding}</p>
          {p.deformer && <p>形变器:{p.deformer}</p>}
          {(p.opacity ?? 1) !== 1 && <p>不透明度:{p.opacity}</p>}
        </div>
        {mode === 'mesh' && v && selectedVertex !== null && (
          <div className="mt-1 flex items-center gap-1 px-2.5 text-2xs text-fg-3" data-testid="rig-vertex-fields">
            <span className="shrink-0">#{selectedVertex}</span>
            <NumField value={v[0]} onCommit={(x) => st().moveVertex(p.id, selectedVertex, [x, v[1]])} testid="rig-vertex-x" />
            <NumField value={v[1]} onCommit={(y) => st().moveVertex(p.id, selectedVertex, [v[0], y])} testid="rig-vertex-y" />
          </div>
        )}
      </div>
    );
  }
  if (selection.kind === 'bone') {
    const b = doc.bones.find((x) => x.id === selection.id);
    if (!b) return null;
    const tail = (len: number, rot: number): [number, number] => {
      const r = (rot * Math.PI) / 180;
      return [b.position[0] + Math.cos(r) * len, b.position[1] + Math.sin(r) * len];
    };
    return (
      <div className="border-b border-edge pb-2" data-testid="rig-selection-bone">
        <p className={sectionTitle}>骨骼 · {b.id}</p>
        <div className="grid grid-cols-[48px_1fr_1fr] items-center gap-1 px-2.5 text-2xs text-fg-3">
          <span>父</span>
          <span className="col-span-2 truncate">{b.parent ?? '(根)'}</span>
          <span>位置</span>
          <NumField value={b.position[0]} onCommit={(x) => st().moveBoneHead(b.id, [x, b.position[1]])} />
          <NumField value={b.position[1]} onCommit={(y) => st().moveBoneHead(b.id, [b.position[0], y])} />
          <span>长/角</span>
          <NumField value={b.length} onCommit={(l) => st().setBoneTail(b.id, tail(Math.max(0, l), b.rotation))} />
          <NumField value={b.rotation} onCommit={(r) => st().setBoneTail(b.id, tail(b.length || 1, r))} />
        </div>
      </div>
    );
  }
  const d = doc.deformers.find((x) => x.id === selection.id);
  if (!d) return null;
  return (
    <div className="border-b border-edge pb-2">
      <p className={sectionTitle}>形变器 · {d.id}</p>
      <p className="px-2.5 text-2xs text-fg-3">
        {d.type === 'warp' ? `warp · rect ${d.rect.join(', ')} · grid ${d.grid.join('×')}` : `rotation · origin ${d.origin.join(', ')}`}
      </p>
    </div>
  );
}

function ParamsSection() {
  const doc = useRigStore((s) => s.doc);
  const params = useRigStore((s) => s.params);
  const mode = useRigStore((s) => s.mode);
  const evalBusy = useRigStore((s) => s.evalBusy);
  const st = useRigStore.getState;
  if (!doc) return null;
  const affects = mode === 'params' || mode === 'animate';
  return (
    <div className="border-b border-edge pb-2" data-testid="rig-params">
      <div className="flex items-center justify-between pr-2">
        <p className={sectionTitle}>
          参数 Parameters({doc.parameters.length}){evalBusy && <span className="ml-1 text-fg-4">求值中…</span>}
        </p>
        <button
          type="button"
          className={smallBtn}
          title="全部参数回到默认值"
          data-testid="rig-params-reset"
          onClick={() => st().resetParams()}
        >
          <RotateCcw size={10} />
          重置
        </button>
      </div>
      {!affects && <p className="px-2.5 pb-1 text-2xs text-fg-4">当前模式显示静止姿态;参数在「参数 / 动画」模式下驱动画面。</p>}
      {doc.parameters.length === 0 && <p className="px-2.5 text-2xs text-fg-4">无参数(可用 rig_patch merge_scaffold 合并模板参数)</p>}
      <div className="flex flex-col gap-1.5 px-2.5">
        {doc.parameters.map((p) => {
          const v = params[p.id] ?? p.default;
          const keys = paramSnapKeys(doc, p);
          const step = (p.max - p.min) / 200 || 0.01;
          return (
            <div key={p.id} className="flex flex-col gap-0.5" data-testid={`rig-param-${p.id}`}>
              <div className="flex items-center gap-1 text-2xs">
                <span className="min-w-0 flex-1 truncate font-mono text-fg-2" title={p.id}>
                  {p.id}
                </span>
                <span className="w-14 shrink-0">
                  <NumField value={Math.round(v * 1000) / 1000} onCommit={(x) => st().setParam(p.id, x)} />
                </span>
              </div>
              <input
                type="range"
                min={p.min}
                max={p.max}
                step={step}
                value={v}
                aria-label={p.id}
                data-testid={`rig-param-slider-${p.id}`}
                onChange={(e) => st().setParam(p.id, Number(e.target.value))}
                className="w-full"
              />
              <div className="flex flex-wrap items-center gap-0.5">
                <Magnet size={9} className="text-fg-4" />
                {keys.map((k) => (
                  <button
                    key={k}
                    type="button"
                    title={`吸附到关键值 ${k}`}
                    data-testid={`rig-param-snap-${p.id}-${k}`}
                    onClick={() => st().setParam(p.id, k)}
                    className={cn(
                      'rounded px-1 font-mono text-2xs',
                      Math.abs(v - k) < 1e-3 ? 'bg-acc text-white' : 'bg-shell-panel text-fg-3 hover:text-fg-2',
                    )}
                  >
                    {fmt(k)}
                  </button>
                ))}
                <span className="ml-auto font-mono text-2xs text-fg-4">
                  [{fmt(p.min)}, {fmt(p.max)}]
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function parseKeys(text: string): number[] | null {
  const vals = text
    .split(/[,，\s]+/)
    .filter(Boolean)
    .map(Number);
  if (vals.length === 0 || vals.some((v) => !Number.isFinite(v))) return null;
  return [...new Set(vals)].sort((a, b) => a - b);
}

function KeyformSection() {
  const doc = useRigStore((s) => s.doc);
  const selection = useRigStore((s) => s.selection);
  const params = useRigStore((s) => s.params);
  const poseEdits = useRigStore((s) => s.poseEdits);
  const dirty = useRigStore((s) => s.dirty);
  const st = useRigStore.getState;
  const partId = selection?.kind === 'part' ? selection.id : null;
  const groups = useMemo(
    () => (doc && partId ? doc.keyforms.filter((g) => g.target === `part:${partId}`) : []),
    [doc, partId],
  );
  const [axes, setAxes] = useState<string[]>([]);
  const [keysText, setKeysText] = useState<Record<string, string>>({});

  // 换部件:默认取该部件第一个已有组的参数轴。
  useEffect(() => {
    setAxes(groups[0]?.params ?? []);
    setKeysText({});
  }, [partId]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!doc) return null;
  if (!partId) {
    return (
      <div className="pb-2">
        <p className={sectionTitle}>记录 keyform</p>
        <p className="px-2.5 text-2xs text-fg-4">先选中一个部件,把参数吸附到关键值,再在画布上拖动顶点。</p>
      </div>
    );
  }
  const existing = groups.find((g) => JSON.stringify(g.params) === JSON.stringify(axes));
  const keysFor = (pid: string): number[] | null => {
    const ai = axes.indexOf(pid);
    if (existing) return existing.keys[ai] ?? null;
    const p = doc.parameters.find((x) => x.id === pid);
    const def = p ? [...new Set([p.min, p.default, p.max])].sort((a, b) => a - b) : [];
    const t = keysText[pid];
    return t === undefined ? def : parseKeys(t);
  };
  const keys = axes.map((a) => keysFor(a));
  const keysOk = axes.length > 0 && keys.every((k) => k !== null && k.length > 0);
  const atIdx = keysOk ? keyformIndex({ params: axes, keys: keys as number[][] }, params) : null;
  const editCount = Object.keys(poseEdits[partId] ?? {}).length;
  const toggleAxis = (pid: string) => {
    setAxes((cur) => (cur.includes(pid) ? cur.filter((x) => x !== pid) : cur.length >= 3 ? cur : [...cur, pid]));
  };

  return (
    <div className="pb-2" data-testid="rig-keyform">
      <p className={sectionTitle}>记录 keyform · part:{partId}</p>
      {groups.length > 0 && (
        <div className="flex flex-wrap gap-1 px-2.5 pb-1">
          {groups.map((g) => (
            <button
              key={g.params.join(',')}
              type="button"
              className={cn(
                'rounded px-1.5 py-px font-mono text-2xs',
                JSON.stringify(g.params) === JSON.stringify(axes) ? 'bg-acc text-white' : 'bg-shell-panel text-fg-3',
              )}
              title={`已有组 keys: ${JSON.stringify(g.keys)}`}
              onClick={() => setAxes([...g.params])}
            >
              {g.params.join(' × ')}
            </button>
          ))}
        </div>
      )}
      <p className="px-2.5 text-2xs text-fg-4">参数轴(1–3,按勾选顺序):</p>
      <div className="flex flex-col gap-0.5 px-2.5">
        {doc.parameters.map((p) => {
          const on = axes.includes(p.id);
          const k = on ? keysFor(p.id) : null;
          return (
            <div key={p.id} className="flex items-center gap-1 text-2xs">
              <label className="flex min-w-0 flex-1 items-center gap-1 text-fg-3">
                <input
                  type="checkbox"
                  checked={on}
                  data-testid={`rig-keyform-axis-${p.id}`}
                  onChange={() => toggleAxis(p.id)}
                />
                <span className="truncate font-mono">{p.id}</span>
              </label>
              {on &&
                (existing ? (
                  <span className="shrink-0 font-mono text-fg-4" title="已有组关键值(改关键值请用 agent / set_keyform_group)">
                    {(k ?? []).map(fmt).join(', ')}
                  </span>
                ) : (
                  <input
                    className={cn(inputCls, 'w-24 shrink-0', k === null && 'border-warn')}
                    value={keysText[p.id] ?? (k ?? []).map(fmt).join(', ')}
                    title="该轴关键值(逗号分隔,升序)"
                    onChange={(e) => setKeysText({ ...keysText, [p.id]: e.target.value })}
                  />
                ))}
            </div>
          );
        })}
      </div>
      <div className="space-y-0.5 px-2.5 pt-1 text-2xs">
        <p className={atIdx !== null ? 'text-fg-3' : 'text-warn'}>
          {axes.length === 0
            ? '未选参数轴'
            : atIdx !== null
              ? `当前姿势在关键值上(形态 #${atIdx}${existing ? '' : ',将新建组'})`
              : '当前参数不在关键值上:用上方吸附按钮对齐'}
        </p>
        <p className="text-fg-3">已拖动顶点:{editCount}</p>
        {dirty && <p className="text-warn">有未保存改动:求值基于已保存文档,请先保存</p>}
      </div>
      <div className="flex gap-1 px-2.5 pt-1">
        <button
          type="button"
          className={smallBtn}
          data-testid="rig-keyform-record"
          disabled={!keysOk || atIdx === null || editCount === 0 || dirty}
          onClick={() => {
            if (keysOk) st().recordKeyform({ partId, axes, keys: keys as number[][] });
          }}
          title="写 set_keyform(offsets = 拖后位置 − 当前姿势求值位置);保存后生效"
        >
          <Crosshair size={10} />
          记录 keyform
        </button>
        <button
          type="button"
          className={smallBtn}
          disabled={editCount === 0}
          onClick={() => st().clearPoseEdits(partId)}
          title="丢弃当前姿势下拖过的顶点"
        >
          <Eraser size={10} />
          清除拖动
        </button>
      </div>
    </div>
  );
}

export default function RigParamsPanel() {
  const mode = useRigStore((s) => s.mode);
  return (
    <div className="flex flex-col" data-testid="rig-params-panel">
      <SelectionSection />
      <ParamsSection />
      {mode === 'params' && <KeyformSection />}
    </div>
  );
}
