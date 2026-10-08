import { useState } from 'react';
import { callTool } from '@/lib/forgeApi';
import { useEditorStore, type EntityData } from '@/lib/editorStore';

export default function TemplateControls({ entity }: { entity: EntityData }) {
  const entities = useEditorStore((s) => s.entities);
  const prefab = entity.components.find((c) => c.type === 'PrefabInstance');
  const rootId = typeof prefab?.props.rootId === 'number' ? prefab.props.rootId : entity.id;
  const root = entities.find((e) => e.id === rootId) ?? entity;
  const animator = root.components.find((c) => c.type === 'Animator' && c.enabled);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const action = async (tool: string, args: Record<string, unknown>) => {
    setBusy(true); setError('');
    try { await callTool(tool, args); await useEditorStore.getState().loadEntities(); }
    catch (e) { setError((e as Error).message); }
    finally { setBusy(false); }
  };
  const button = 'rounded border border-edge px-2 py-0.5 text-2xs text-fg-2 hover:bg-shell-hover disabled:opacity-40';
  if (!prefab && !animator) return null;
  const clips = [...new Set([animator?.props.idleClip, animator?.props.walkClip, animator?.props.clip].filter((v): v is string => typeof v === 'string' && v !== ''))];
  return <div className="space-y-2 border-b border-edge px-2 py-2" data-testid="template-controls">
    {prefab && <div>
      <p className="mb-1 text-2xs text-fg-3">模板实例 · 版本 {String(prefab.props.revision ?? '')}</p>
      <button type="button" className={button} disabled={busy} onClick={() => void action('prefab_revert', { id: entity.id })}>恢复整个实例的模板默认值</button>
    </div>}
    {animator && <div className="flex flex-wrap items-center gap-1">
      <span className="text-2xs text-fg-3">动画</span>
      <select aria-label="角色动画" value={String(animator.props.clip ?? '')} disabled={busy} className="min-w-0 max-w-28 bg-shell-input text-2xs text-fg"
        onChange={(e) => void action('animation_control', { id: rootId, action: 'play', clip: e.target.value, loop: true })}>
        {clips.map((clip) => <option key={clip} value={clip}>{clip}</option>)}
      </select>
      <button type="button" className={button} disabled={busy} onClick={() => void action('animation_control', { id: rootId, action: animator.props.playing ? 'pause' : 'play' })}>{animator.props.playing ? '暂停' : '播放'}</button>
      <button type="button" className={button} disabled={busy} onClick={() => void action('animation_control', { id: rootId, action: 'stop' })}>重置</button>
      {animator.props.manualControl === true && <button type="button" className={button} disabled={busy}
        onClick={() => void action('component_set', { id: rootId, type: 'Animator', props: { ...animator.props, manualControl: false, playing: true } })}>随移动切换</button>}
    </div>}
    {error && <p role="alert" className="text-2xs text-danger">{error}</p>}
  </div>;
}
