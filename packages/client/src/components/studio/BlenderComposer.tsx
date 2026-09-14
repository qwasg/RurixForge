import { useEffect, useState } from 'react';
import { ExternalLink, Loader2, RefreshCw } from 'lucide-react';
import { cn } from '@/lib/cn';
import {
  blenderConfigure, blenderCreateJob, blenderGetJob, blenderJobAction, blenderStatus,
  openBlenderInCodex, type BlenderAssetKind, type BlenderJob, type BlenderJobState, type BlenderStatus,
} from '@/lib/blenderApi';
import { useWorkspaceStore } from '@/lib/workspaceStore';
import { useStudioStore, type StudioNode } from '@/lib/studioStore';
import { useAssetStore } from '@/lib/assetStore';
import { callTool } from '@/lib/forgeApi';
import { useEditorStore } from '@/lib/editorStore';

export const BLENDER_STATE_LABELS: Record<BlenderJobState, string> = {
  awaiting_codex: '等待 Codex 领取', claimed: 'Codex 已领取', authoring: '正在 Blender 中制作',
  exporting: '正在导出', validating: '正在验证模型', importing: '正在导入引擎',
  ready: '已同步', failed: '任务失败', cancelled: '已取消',
};
const button = 'rounded border border-edge-strong px-2 py-1 text-2xs text-fg-2 hover:bg-shell-hover disabled:opacity-40';

export default function BlenderComposer({ node }: { node: StudioNode }) {
  const setPrompt = useStudioStore((s) => s.setPrompt);
  const setParam = useStudioStore((s) => s.setParam);
  const recordVersion = useStudioStore((s) => s.recordBlenderVersion);
  const workspaceId = useWorkspaceStore((s) => s.activeWorkspaceId) ?? 'default';
  const jobId = typeof node.params.blenderJobId === 'string' ? node.params.blenderJobId : '';
  const kind: BlenderAssetKind = node.params.blenderKind === 'map' || node.params.blenderKind === 'character' ? node.params.blenderKind : 'prop';
  const [status, setStatus] = useState<BlenderStatus | null>(null);
  const [job, setJob] = useState<BlenderJob | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState('');
  const [executable, setExecutable] = useState('');

  useEffect(() => {
    let live = true;
    setStatus(null);
    blenderStatus(workspaceId).then((s) => { if (live) setStatus(s); })
      .catch((e: unknown) => { if (live) setError((e as Error).message); });
    return () => { live = false; };
  }, [workspaceId]);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    setJob(null);
    if (!jobId) return;
    const refresh = async () => {
      try {
        const updated = await blenderGetJob(jobId, workspaceId);
        if (!live) return;
        setJob(updated);
        if (updated.published && updated.revision > 0) {
          recordVersion(node.id, updated);
        }
      } catch (e) { if (live) setError((e as Error).message); }
      finally { if (live) timer = setTimeout(() => void refresh(), 3000); }
    };
    void refresh();
    return () => { live = false; clearTimeout(timer); };
  }, [jobId, workspaceId, node.id, recordVersion]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true); setError(null); setNotice('');
    try { await action(); } catch (e) { setError((e as Error).message); }
    finally { setBusy(false); }
  };
  const create = () => run(async () => {
    const created = await blenderCreateJob(node.name, node.prompt, kind, workspaceId);
    // Save the job before opening another application; rejected deep links remain resumable.
    setParam(node.id, 'blenderJobId', created.id);
    setJob(created);
    await openBlenderInCodex(created);
    setNotice('已打开 Codex。点击一次发送后开始制作。');
  });

  return <div data-testid="blender-composer" className="pointer-events-auto max-h-full w-[560px] max-w-[94%] overflow-y-auto rounded-xl border border-edge-strong bg-shell-float p-3 shadow-composer">
    <div className="mb-2 flex items-center gap-2">
      <strong className="text-xs text-fg">Blender 制作</strong>
      <span className="flex-1 text-[10px] text-fg-3">Codex · 原生 computer-use</span>
      <button type="button" className={button} onClick={() => setParam(node.id, 'creationMethod', 'remote')}>远程生成</button>
    </div>
    <textarea aria-label="Blender 制作需求" value={node.prompt} rows={3} maxLength={7500}
      onChange={(e) => setPrompt(node.id, e.target.value)}
      placeholder="描述模型造型、材质风格，以及地图布局或角色动作…"
      className="w-full resize-none rounded border border-edge bg-shell-sunk p-2 text-xs text-fg outline-none focus:border-acc" />
    <div className="my-2 flex flex-wrap items-center gap-2">
      {([{ id: 'prop', label: '模型素材' }, { id: 'map', label: '地图模板' }, { id: 'character', label: '角色模板' }] as const).map((item) =>
        <button key={item.id} type="button" className={cn(button, kind === item.id && 'border-acc text-acc')}
          aria-pressed={kind === item.id} onClick={() => setParam(node.id, 'blenderKind', item.id)}>{item.label}</button>)}
      <span className="ml-auto text-[10px] text-fg-3">保存后自动同步</span>
    </div>
    {status && !status.blender.found && <div className="mb-2 rounded bg-shell-sunk p-2 text-2xs text-fg-3">
      <p>未找到 Blender，请指定本机程序。</p>
      <div className="mt-1 flex gap-1">
        <input aria-label="Blender 程序路径" className="min-w-0 flex-1 rounded border border-edge bg-shell-panel px-1" value={executable} onChange={(e) => setExecutable(e.target.value)} placeholder="C:\…\blender.exe" />
        <button type="button" disabled={busy || !executable.trim()} className={button} onClick={() => void run(async () => { setStatus(await blenderConfigure(executable, workspaceId)); })}>检测</button>
      </div>
    </div>}
    {job && <div className="mb-2 rounded border border-edge bg-shell-sunk p-2 text-2xs" data-testid="blender-job-status" aria-live="polite">
      <div className="flex items-center gap-2"><strong className={job.state === 'failed' ? 'text-danger' : 'text-fg'}>{BLENDER_STATE_LABELS[job.state]}</strong>
        {job.revision > 0 && <span className="text-fg-4">版本 {job.revision}</span>}
      </div>
      {job.message && <p className="mt-1 text-fg-3">{job.message}</p>}
      {job.error && <p className="mt-1 text-danger">{job.error.message}</p>}
      <p className="mt-1 truncate text-[10px] text-fg-4" title={job.sourceAbsolutePath ?? job.sourcePath}>{job.sourcePath}</p>
      <div className="mt-2 flex flex-wrap gap-1">
        {(job.state === 'awaiting_codex' || job.state === 'failed') && <button className={button} disabled={busy} onClick={() => void run(() => openBlenderInCodex(job))}>在 Codex 中继续</button>}
        <button className={button} disabled={busy} onClick={() => void run(async () => { await navigator.clipboard.writeText(job.handoffPrompt); setNotice('已复制，可粘贴到 Codex 继续。'); })}>复制交接提示</button>
        {job.state === 'failed' && <button className={button} disabled={busy} onClick={() => void run(async () => { setJob(await blenderJobAction(job.id, 'retry', workspaceId)); })}><RefreshCw size={11} className="mr-1 inline" />重试</button>}
        {job.state !== 'cancelled' && <button className={button} disabled={busy} onClick={() => void run(async () => { setJob(await blenderJobAction(job.id, 'cancel', workspaceId)); })}>{job.state === 'ready' ? '停止自动同步' : '取消任务'}</button>}
        {job.published && <button className={cn(button, 'border-acc text-acc')} disabled={busy} onClick={() => void run(async () => {
          await callTool('prefab_instantiate', { prefabRef: job.published!.prefabGuid, translation: [0, 0, 0] });
          await useAssetStore.getState().load();
          await useEditorStore.getState().loadEntities();
          setNotice('模板已加入当前场景。');
        })}>加入场景</button>}
      </div>
    </div>}
    {error && <p role="alert" className="mb-2 text-2xs text-danger">{error}</p>}
    {notice && <p role="status" className="mb-2 text-2xs text-sage">{notice}</p>}
    <div className="flex items-center gap-2">
      <p className="flex-1 text-[10px] leading-4 text-fg-3">在 Codex 点击一次发送后制作。{kind === 'character' ? '包含骨骼、待机／行走和基础移动。' : kind === 'map' ? '保留物体层级、材质和基础碰撞。' : '完成模型、UV 与贴图。'}</p>
      <button type="button" data-testid="blender-create" disabled={busy || !node.prompt.trim() || !status?.blender.found}
        onClick={() => void create()} className="flex shrink-0 items-center gap-1 rounded bg-acc px-3 py-1.5 text-2xs text-fg-inv disabled:opacity-40">
        {busy ? <Loader2 size={12} className="animate-spin" /> : <ExternalLink size={12} />}{job ? '新建制作任务' : '在 Codex 制作'}
      </button>
    </div>
  </div>;
}
