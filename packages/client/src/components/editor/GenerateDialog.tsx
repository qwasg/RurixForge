/**
 * GenerateDialog(F5 wave.3):Assets 右键「Generate...」直开的文生图对话框。
 * 提交 → gen_image → 候选进 CandidatesModal;未配置后端如实错误条
 * (GEN_BACKEND_NOT_CONFIGURED + 设置页 generation tab 指引),不伪造生成。
 */
import { useEffect, useState } from 'react';
import { X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useAssetStore } from '@/lib/assetStore';
import { configuredBackends, useGenStore } from '@/lib/genStore';

const SIZES = [256, 512, 1024] as const;

export default function GenerateDialog() {
  const open = useGenStore((s) => s.dialogOpen);
  const destFolder = useGenStore((s) => s.destFolder);
  const bindAssetPath = useGenStore((s) => s.bindAssetPath);
  const backends = useGenStore((s) => s.backends);
  const backendsLoaded = useGenStore((s) => s.backendsLoaded);
  const backendsError = useGenStore((s) => s.backendsError);
  const busy = useGenStore((s) => s.busy);
  const lastError = useGenStore((s) => s.lastError);
  const lastErrorCode = useGenStore((s) => s.lastErrorCode);
  const closeDialog = useGenStore((s) => s.closeDialog);
  const generate = useGenStore((s) => s.generate);
  const loadBackends = useGenStore((s) => s.loadBackends);
  // F10-RAG:绑定资产的简介(有则并入提示词;无则如实提示按原 prompt 生成)。
  const bindDesc = useAssetStore((s) =>
    bindAssetPath ? (s.items.find((i) => i.path === bindAssetPath)?.description ?? '') : '',
  );

  const [prompt, setPrompt] = useState('');
  const [negativePrompt, setNegativePrompt] = useState('');
  const [size, setSize] = useState<(typeof SIZES)[number]>(512);
  const [n, setN] = useState(4);
  const [backend, setBackend] = useState('');

  useEffect(() => {
    if (open) void loadBackends();
  }, [open, loadBackends]);

  if (!open) return null;

  const configured = configuredBackends(backends);
  const backendValue = backend || configured[0]?.id || '';
  const noBackend = backendsLoaded && configured.length === 0;

  const submit = () => {
    if (!prompt.trim() || busy || noBackend) return;
    void generate({
      prompt: prompt.trim(),
      negativePrompt,
      size,
      n,
      backend: backendValue || undefined,
      assetPath: bindAssetPath ?? undefined,
    });
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/20" data-gen-dialog>
      <div className="flex w-[360px] flex-col rounded-lg border border-edge-strong bg-shell-panel p-3 shadow-md">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-fg">生成图像候选</span>
          <button type="button" title="关闭" className={iconBtn} onClick={closeDialog}>
            <X size={13} strokeWidth={1.8} />
          </button>
        </div>
        <p className="mt-0.5 text-2xs text-fg-4">
          目标文件夹:Content/{destFolder}(Accept 后入管线,provenance 自动记录)
        </p>
        {bindAssetPath && (
          <p className="mt-0.5 text-2xs text-fg-3" data-gen-bind-asset title={bindAssetPath}>
            绑定资产:{bindAssetPath.split('/').pop()}
            {bindDesc
              ? ' — 简介+标签将并入提示词'
              : ' — 该资产尚无简介,按原 prompt 生成(检视器「资产」页签可写)'}
          </p>
        )}

        <label className="mt-2 block text-2xs text-fg-3">Prompt(必填)</label>
        <textarea
          data-gen-prompt
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          rows={2}
          placeholder="wood 木纹"
          className="mt-0.5 w-full resize-none rounded-md border border-edge-strong bg-shell-panel px-2 py-1 text-xs text-fg outline-none placeholder:text-fg-4 focus:border-fg-4"
        />

        <label className="mt-1.5 block text-2xs text-fg-3">Negative prompt(可选)</label>
        <input
          data-gen-negative
          value={negativePrompt}
          onChange={(e) => setNegativePrompt(e.target.value)}
          className="mt-0.5 w-full rounded-md border border-edge-strong bg-shell-panel px-2 py-1 text-xs text-fg outline-none focus:border-fg-4"
        />

        <div className="mt-1.5 flex gap-2">
          <div className="flex-1">
            <label className="block text-2xs text-fg-3">尺寸</label>
            <select
              data-gen-size
              value={size}
              onChange={(e) => setSize(Number(e.target.value) as (typeof SIZES)[number])}
              className="mt-0.5 w-full rounded-md border border-edge-strong bg-shell-panel px-1.5 py-1 text-xs text-fg outline-none"
            >
              {SIZES.map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
            </select>
          </div>
          <div className="flex-1">
            <label className="block text-2xs text-fg-3">候选数(1-4)</label>
            <select
              data-gen-n
              value={n}
              onChange={(e) => setN(Number(e.target.value))}
              className="mt-0.5 w-full rounded-md border border-edge-strong bg-shell-panel px-1.5 py-1 text-xs text-fg outline-none"
            >
              {[1, 2, 3, 4].map((v) => (
                <option key={v} value={v}>
                  {v}
                </option>
              ))}
            </select>
          </div>
        </div>

        <label className="mt-1.5 block text-2xs text-fg-3">后端(仅列已配置)</label>
        <select
          data-gen-backend
          value={backendValue}
          onChange={(e) => setBackend(e.target.value)}
          disabled={configured.length === 0}
          className="mt-0.5 w-full rounded-md border border-edge-strong bg-shell-panel px-1.5 py-1 text-xs text-fg outline-none disabled:text-fg-4"
        >
          {configured.map((b) => (
            <option key={b.id} value={b.id}>
              {b.id}({b.kind})
            </option>
          ))}
          {configured.length === 0 && <option value="">无已配置后端</option>}
        </select>

        {backendsError && (
          <p className="mt-1.5 text-2xs text-danger" data-gen-error>
            后端清单拉取失败:{backendsError}
          </p>
        )}
        {noBackend && (
          <p className="mt-1.5 text-2xs text-info" data-gen-error>
            GEN_BACKEND_NOT_CONFIGURED:无已配置生成后端。请到 设置 → Generation 配置
            (data/gen-backends.json),配置后重试。
          </p>
        )}
        {lastError && (
          <p className="mt-1.5 text-2xs text-danger" data-gen-error>
            {lastErrorCode ? `${lastErrorCode}:` : ''}
            {lastError}
            {lastErrorCode === 'GEN_BACKEND_NOT_CONFIGURED' &&
              '(请到 设置 → Generation 配置后端)'}
          </p>
        )}

        <div className="mt-2.5 flex justify-end gap-1.5">
          <button
            type="button"
            className="rounded-md px-3 py-1 text-xs text-fg-3 hover:text-fg-2"
            onClick={closeDialog}
          >
            取消
          </button>
          <button
            type="button"
            data-gen-submit
            disabled={!prompt.trim() || busy || noBackend}
            onClick={submit}
            className={cn(
              'rounded-md px-3 py-1 text-xs text-fg-inv',
              !prompt.trim() || busy || noBackend ? 'cursor-not-allowed bg-fg-4' : 'bg-fg',
            )}
          >
            {busy ? '生成中…' : '生成候选'}
          </button>
        </div>
      </div>
    </div>
  );
}

const iconBtn =
  'flex h-6 w-6 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-2';
