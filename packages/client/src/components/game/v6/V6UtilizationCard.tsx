import {formatV6,rateV6,type V6Preview} from '@/lib/sentinelsV6';

/** Space / power / compute utilization from game.session.preview. */
export default function V6UtilizationCard({preview,mode='preview'}:{preview:V6Preview|null|undefined;mode?:'preview'|'inspector'}){
  if(!preview)return null;
  const hasSpace=preview.netArea!==undefined||preview.capacity!==undefined||preview.costPerCapacity!==undefined;
  const hasPower=Number.isFinite(preview.powerBefore)&&Number.isFinite(preview.powerAfter);
  const hasCompute=preview.computeBefore!==undefined||preview.computeAfter!==undefined||preview.computeCapacityBefore!==undefined||preview.computeCapacityAfter!==undefined;
  if(!hasSpace&&!hasPower&&!hasCompute&&mode==='inspector')return null;
  return <div className={`v6-utilization-card ${preview.valid?'':'invalid'}`} aria-label="空间与资源利用率">
    <header><span className="v6-eyebrow">{mode==='preview'?'UTILIZATION PREVIEW':'SPACE BUDGET'}</span><strong>{preview.valid?'利用率':'预览无效'}</strong></header>
    {!preview.valid&&preview.reason&&<p className="v6-muted">{preview.reason}</p>}
    {hasSpace&&<div className="v6-utilization-grid">
      {preview.netArea!==undefined&&<div><span>净面积</span><b>{formatV6(preview.netArea)} 格</b></div>}
      {preview.capacity!==undefined&&<div><span>→ 容量</span><b>{formatV6(preview.capacity)}</b></div>}
      {preview.costPerCapacity!==undefined&&Number.isFinite(preview.costPerCapacity)&&<div><span>每容量造价</span><b>◈ {rateV6(preview.costPerCapacity)}</b></div>}
    </div>}
    {hasPower&&<div className="v6-stat-line"><span>电网负载 / 输出</span><b>{rateV6(preview.demandBefore)} / {rateV6(preview.powerBefore)} → {rateV6(preview.demandAfter)} / {rateV6(preview.powerAfter)}</b></div>}
    {hasCompute&&<div className="v6-stat-line"><span>算力占用 / 容量</span><b>{formatV6(preview.computeBefore)} / {formatV6(preview.computeCapacityBefore)} → {formatV6(preview.computeAfter)} / {formatV6(preview.computeCapacityAfter)}</b></div>}
  </div>;
}
