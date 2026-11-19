import { useEffect, useState } from 'react';
import { Bone } from 'lucide-react';
import { callAssetTool } from '@/lib/forgeApi';
import { openRigEditor } from '@/lib/rigStore';
import type { RigStats } from '@/lib/rigTypes';

/**
 * F-GAME-5(D-050):资产检视器 rig 摘要(rig_get 不带网格,只拿 stats + clip 名 + 贴图 GUID)
 * + 「打开绑骨编辑器」按钮。照 AssetInspectorPanel SpriteSection 范式。
 */

interface RigSummary {
  stats: RigStats | null;
  clipNames: string[];
  texture: string;
}

export default function RigAssetSection({ assetPath }: { assetPath: string }) {
  const [summary, setSummary] = useState<RigSummary | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    setSummary(null);
    setErr(null);
    let alive = true;
    callAssetTool<{
      stats?: RigStats;
      doc?: { texture?: string; clips?: Record<string, unknown> };
      error?: string;
      message?: string;
    }>('rig_get', { assetPath })
      .then((r) => {
        if (!alive) return;
        if (r.error || (!r.doc && !r.stats)) {
          setErr(r.message ?? r.error ?? 'rig_get 空响应');
          return;
        }
        setSummary({
          stats: r.stats ?? null,
          clipNames: Object.keys(r.doc?.clips ?? {}),
          texture: r.doc?.texture ?? '',
        });
      })
      .catch((e: Error) => {
        if (alive) setErr(e.message);
      });
    return () => {
      alive = false;
    };
  }, [assetPath]);

  const s = summary?.stats;
  return (
    <div className="border-t border-edge px-2.5 py-2" data-testid="asset-rig-summary">
      <p className="pb-1 text-2xs font-medium text-fg-2">2D 绑骨 · Rig</p>
      {err ? (
        <p className="text-2xs text-warn" title={err}>
          读取失败:{err}
        </p>
      ) : !summary ? (
        <p className="text-2xs text-fg-4">读取中…</p>
      ) : (
        <div className="space-y-0.5 text-2xs text-fg-3">
          {s && (
            <>
              <p>
                部件 {s.parts} · 顶点 {s.vertices} · 三角形 {s.triangles}
              </p>
              <p>
                骨骼 {s.bones} · 形变器 {s.deformers} · 参数 {s.parameters}
              </p>
              <p>
                keyform 组 {s.keyformGroups} · 物理 {s.physics} · animator {s.hasAnimator ? '有' : '无'}
              </p>
            </>
          )}
          <p>
            clip:
            {summary.clipNames.length === 0 ? '(无)' : summary.clipNames.join('、')}
          </p>
          {summary.texture && (
            <p className="truncate font-mono" title={summary.texture}>
              图集 GUID:{summary.texture}
            </p>
          )}
        </div>
      )}
      <button
        type="button"
        data-testid="asset-rig-open"
        onClick={() => openRigEditor(assetPath)}
        className="mt-1.5 flex items-center gap-1 rounded-md bg-acc px-2 py-1 text-2xs text-white"
      >
        <Bone size={11} />
        打开绑骨编辑器
      </button>
    </div>
  );
}
