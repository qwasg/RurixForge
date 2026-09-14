/**
 * CandidatesModal(F5 wave.3):gen_image 候选网格挑拣。
 * 每张卡:dataUrl 缩略图 + seed/backendId 标注 + Accept 钮(gen_accept 入当前 Assets 文件夹,
 * 资产刷新 + 选中);全部候选可逐个 accept(不互斥),已入库卡如实标「已入库」。
 */
import { X } from 'lucide-react';
import { cn } from '@/lib/cn';
import { useGenStore } from '@/lib/genStore';

export default function CandidatesModal() {
  const candidates = useGenStore((s) => s.candidates);
  const destFolder = useGenStore((s) => s.destFolder);
  const acceptedRefs = useGenStore((s) => s.acceptedRefs);
  const acceptBusy = useGenStore((s) => s.acceptBusy);
  const lastError = useGenStore((s) => s.lastError);
  const lastErrorCode = useGenStore((s) => s.lastErrorCode);
  const accept = useGenStore((s) => s.accept);
  const closeCandidates = useGenStore((s) => s.closeCandidates);

  if (!candidates) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/20" data-gen-candidates>
      <div className="flex w-[520px] max-w-[90vw] flex-col rounded-lg border border-edge-strong bg-shell-panel p-3 shadow-md">
        <div className="flex items-center justify-between">
          <span className="text-sm font-medium text-fg">
            候选挑拣({candidates.length} 张 → Content/{destFolder})
          </span>
          <button type="button" title="关闭" data-gen-close className={iconBtn} onClick={closeCandidates}>
            <X size={13} strokeWidth={1.8} />
          </button>
        </div>
        <p className="mt-0.5 text-2xs text-fg-4">
          Accept 入管线(自动 .meta + provenance origin=gen-image);可逐个接受多张,不互斥。
        </p>

        {lastError && (
          <p className="mt-1 text-2xs text-danger" data-gen-error>
            {lastErrorCode ? `${lastErrorCode}:` : ''}
            {lastError}
          </p>
        )}

        <div className="mt-2 grid max-h-[60vh] grid-cols-2 gap-2 overflow-y-auto">
          {candidates.map((c) => {
            const accepted = acceptedRefs.includes(c.imageFileRef);
            const busy = acceptBusy === c.imageFileRef;
            return (
              <div
                key={c.imageFileRef}
                data-gen-candidate
                data-seed={c.seed}
                className="flex flex-col rounded-md border border-edge-strong bg-shell-sunk p-1.5"
              >
                {c.dataUrl ? (
                  <img
                    src={c.dataUrl}
                    alt={`seed ${c.seed}`}
                    className="h-36 w-full rounded-sm object-cover"
                    draggable={false}
                  />
                ) : (
                  <div className="flex h-36 w-full items-center justify-center rounded-sm bg-shell-panel text-2xs text-fg-4">
                    无缩略图(dataUrl 缺失)
                  </div>
                )}
                <div className="mt-1 flex items-center justify-between gap-1">
                  <span className="min-w-0 flex-1 truncate text-2xs text-fg-3" title={`${c.backendId} · seed ${c.seed}`}>
                    seed {c.seed} · {c.backendId}
                  </span>
                  <button
                    type="button"
                    data-gen-accept
                    disabled={busy}
                    onClick={() => void accept(c)}
                    className={cn(
                      'shrink-0 rounded px-2 py-0.5 text-2xs text-fg-inv',
                      busy ? 'cursor-not-allowed bg-fg-4' : accepted ? 'bg-sage' : 'bg-fg',
                    )}
                  >
                    {busy ? '入库中…' : accepted ? '已入库' : 'Accept'}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

const iconBtn =
  'flex h-6 w-6 items-center justify-center rounded-md text-fg-3 transition-colors hover:bg-shell-hover hover:text-fg-soft';
