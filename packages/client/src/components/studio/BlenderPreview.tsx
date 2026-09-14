import { useEffect, useRef, useState } from 'react';
import { blenderPreview } from '@/lib/blenderApi';
import { cn } from '@/lib/cn';

/** Engine-rendered preview. No browser glTF renderer or supplier thumbnail is substituted. */
export default function BlenderPreview({ jobId, revision, workspaceId, clips = [], compact = false, className = '' }: {
  jobId: string; revision: number; workspaceId: string; clips?: string[]; compact?: boolean; className?: string;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [yaw, setYaw] = useState(0);
  const [clip, setClip] = useState('');
  const [time, setTime] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [device, setDevice] = useState('');
  useEffect(() => {
    let live = true;
    setBusy(true);
    setError(null);
    void blenderPreview(jobId, { yaw, clip: clip || undefined, time }, workspaceId).then((frame) => {
      if (!live) return;
      const bytes = Uint8ClampedArray.from(atob(frame.pixelsB64), (c) => c.charCodeAt(0));
      if (bytes.length !== frame.width * frame.height * 4) throw new Error('引擎画面像素长度不正确');
      const el = canvas.current;
      if (!el) return;
      el.width = frame.width;
      el.height = frame.height;
      el.getContext('2d')?.putImageData(new ImageData(bytes, frame.width, frame.height), 0, 0);
      setDevice(frame.deviceName ?? '引擎渲染');
    }).catch((err: unknown) => { if (live) setError((err as Error).message); })
      .finally(() => { if (live) setBusy(false); });
    return () => { live = false; };
  }, [jobId, revision, workspaceId, yaw, clip, time]);
  return (
    <div className={cn('relative flex min-h-24 flex-col overflow-hidden rounded border border-edge bg-shell-panel', className)} data-testid="blender-preview">
      <canvas ref={canvas} aria-label="引擎模型预览" className={cn('min-h-0 w-full flex-1 object-contain', error && 'hidden')} />
      {error && <p role="alert" className="p-2 text-2xs text-danger">{error}</p>}
      {busy && <span className="absolute left-2 top-1 text-[10px] text-fg-3">渲染中…</span>}
      {!compact && <div className="flex flex-wrap items-center gap-2 border-t border-edge px-2 py-1 text-[10px] text-fg-3">
        <button type="button" aria-label="向左旋转模型" onClick={() => setYaw((v) => v - Math.PI / 4)}>↶ 左转</button>
        <button type="button" aria-label="向右旋转模型" onClick={() => setYaw((v) => v + Math.PI / 4)}>右转 ↷</button>
        <select aria-label="预览动画" value={clip} onChange={(e) => { setClip(e.target.value); setTime(0); }} className="max-w-28 bg-shell-panel">
          <option value="">静止姿态</option>
          {clips.map((name) => <option key={name} value={name}>{name}</option>)}
        </select>
        {clip && <label className="flex items-center gap-1">时间 <input aria-label="动画采样时间" type="number" min="0" step="0.1" value={time} onChange={(e) => setTime(Math.max(0, Number(e.target.value) || 0))} className="w-12 bg-shell-panel" />s</label>}
        <span className="ml-auto truncate" title={device}>{device}</span>
      </div>}
    </div>
  );
}
