import { useEffect, useState } from 'react';
import { Bone, RotateCcw } from 'lucide-react';
import { callAssetTool } from '@/lib/forgeApi';
import { useAssetStore } from '@/lib/assetStore';
import { useEditorStore } from '@/lib/editorStore';
import { openRigEditor } from '@/lib/rigStore';
import type { RigParam } from '@/lib/rigTypes';

/**
 * F-GAME-5(D-050):实体检视器里 Rig2D 组件的属性编辑——clip 下拉(取自 rig_get clips)、
 * 播放开关、参数滑杆(写 props.params,编辑态摆姿势;PIE 开始时宿主从空 params 起算,
 * 见契约 D 节)。与 TextPropsEditor 同一写路径:setComponentProps 整份替换 props。
 * 滑杆拖动中只改本地草稿,松手(pointerup / keyup / blur)才提交,避免每帧 component_set。
 */

interface RigMeta {
  params: RigParam[];
  clips: string[];
  assetPath: string | null;
}

export default function RigPropsEditor({ entityId, props }: { entityId: number; props: Record<string, unknown> }) {
  const setComponentProps = useEditorStore((s) => s.setComponentProps);
  const rigGuid = typeof props.rig === 'string' ? props.rig : '';
  const assetPath = useAssetStore((s) => s.items.find((i) => i.guid === rigGuid)?.path ?? null);
  const [meta, setMeta] = useState<RigMeta | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const propParams =
    props.params && typeof props.params === 'object' && !Array.isArray(props.params)
      ? (props.params as Record<string, number>)
      : {};
  const [draft, setDraft] = useState<Record<string, number>>(propParams);
  const paramsKey = JSON.stringify(propParams);
  useEffect(() => setDraft(JSON.parse(paramsKey) as Record<string, number>), [paramsKey]);

  useEffect(() => {
    setMeta(null);
    setErr(null);
    if (rigGuid === '') return;
    let alive = true;
    callAssetTool<{
      assetPath?: string;
      doc?: { parameters?: RigParam[]; clips?: Record<string, unknown> };
      error?: string;
      message?: string;
    }>('rig_get', { guid: rigGuid })
      .then((r) => {
        if (!alive) return;
        if (r.error || !r.doc) {
          setErr(r.message ?? r.error ?? 'rig_get 空响应');
          return;
        }
        setMeta({
          params: r.doc.parameters ?? [],
          clips: Object.keys(r.doc.clips ?? {}),
          assetPath: r.assetPath ?? null,
        });
      })
      .catch((e: Error) => {
        if (alive) setErr(e.message);
      });
    return () => {
      alive = false;
    };
  }, [rigGuid]);

  const commit = (patch: Record<string, unknown>) =>
    void setComponentProps(entityId, 'Rig2D', { ...props, ...patch });
  const commitParams = (next: Record<string, number>) => {
    if (JSON.stringify(next) !== paramsKey) commit({ params: next });
  };

  const clip = typeof props.clip === 'string' ? props.clip : '';
  const playing = props.playing !== false;
  const field = 'h-6 rounded border border-edge bg-shell-panel px-1.5 text-2xs text-fg outline-none focus:border-acc-ring';
  const editorPath = meta?.assetPath ?? assetPath;

  return (
    <div data-testid="rig-props-editor" className="flex flex-col gap-1.5 py-1 text-2xs text-fg-3">
      {rigGuid === '' ? (
        <p className="text-fg-4">未指定 rig 资产(props.rig 为空)</p>
      ) : err ? (
        <p className="text-warn" title={err}>
          rig 读取失败:{err}
        </p>
      ) : !meta ? (
        <p className="text-fg-4">读取 rig…</p>
      ) : (
        <>
          <div className="flex items-center gap-1.5">
            <label className="flex min-w-0 flex-1 items-center gap-1">
              clip
              <select
                data-testid="rig-props-clip"
                value={clip}
                onChange={(e) => commit({ clip: e.target.value, time: 0 })}
                className={`${field} min-w-0 flex-1`}
              >
                <option value="">(无 / animator 默认)</option>
                {clip !== '' && !meta.clips.includes(clip) && <option value={clip}>{clip}(未找到)</option>}
                {meta.clips.map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center gap-1">
              <input
                data-testid="rig-props-playing"
                type="checkbox"
                checked={playing}
                onChange={(e) => commit({ playing: e.target.checked })}
              />
              播放
            </label>
          </div>
          {meta.params.length > 0 && (
            <div className="flex flex-col gap-1" data-testid="rig-props-params">
              <div className="flex items-center justify-between">
                <span className="text-fg-4">编辑态姿势(params)</span>
                <button
                  type="button"
                  title="清空 params(回到各参数默认值)"
                  data-testid="rig-props-reset"
                  onClick={() => commitParams({})}
                  className="flex h-5 items-center gap-0.5 rounded px-1 text-fg-3 hover:bg-shell-hover"
                >
                  <RotateCcw size={10} />
                  重置
                </button>
              </div>
              {meta.params.map((p) => {
                const v = draft[p.id] ?? p.default;
                const step = (p.max - p.min) / 200 || 0.01;
                return (
                  <label key={p.id} className="flex items-center gap-1.5">
                    <span className="w-20 shrink-0 truncate font-mono" title={p.id}>
                      {p.id}
                    </span>
                    <input
                      type="range"
                      data-testid={`rig-props-param-${p.id}`}
                      min={p.min}
                      max={p.max}
                      step={step}
                      value={v}
                      onChange={(e) => setDraft({ ...draft, [p.id]: Number(e.target.value) })}
                      onPointerUp={() => commitParams(draft)}
                      onKeyUp={() => commitParams(draft)}
                      onBlur={() => commitParams(draft)}
                      className="min-w-0 flex-1"
                    />
                    <span className="w-10 shrink-0 text-right font-mono">{v.toFixed(2)}</span>
                  </label>
                );
              })}
            </div>
          )}
        </>
      )}
      {editorPath && (
        <button
          type="button"
          data-testid="rig-props-open-editor"
          onClick={() => openRigEditor(editorPath)}
          className="glass-btn h-6 self-start"
        >
          <Bone size={11} />
          在绑骨编辑器中打开
        </button>
      )}
    </div>
  );
}
