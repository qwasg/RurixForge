import {ArrowUp,ChevronRight,FlaskConical} from 'lucide-react';
import {containsV6} from '@/lib/sentinelsV6Geometry';
import {V6_BRANCHES,formatV6,rateV6,type V6Catalog,type V6Player,type V6Room,type V6Snapshot} from '@/lib/sentinelsV6';

export function v6ResearchQuote(catalog:V6Catalog,tier:number,branch?:string,branches?:Record<string,number>){
  const rules=catalog.rules?.research as Record<string,unknown>|undefined;
  const read=(key:string)=>Array.isArray(rules?.[key])?Number((rules![key] as unknown[])[tier-1]):NaN;
  const values={credits:read('credits'),compute:read('compute'),science:read('scienceData'),seconds:read('seconds'),upkeep:read('labComputeUpkeep')};
  if(!Object.values(values).every(v=>Number.isFinite(v)&&v>=0))return null;
  const otherTiers=branch&&branches?Object.entries(branches).filter(([id])=>id!==branch).map(([,t])=>t):[];
  const factor=Number(rules?.additionalBranchFactor),fromTier=Number(rules?.additionalBranchFromTier);
  if(otherTiers.some(t=>t>1)&&(!Number.isFinite(factor)||factor<0||!Number.isInteger(fromTier)||fromTier<2))return null;
  const multiplier=1+otherTiers.filter(t=>t>=fromTier).length*(Number.isFinite(factor)?factor:0);
  return {...values,credits:values.credits*multiplier,science:values.science*multiplier,seconds:values.seconds*multiplier,multiplier};
}
export default function V6TechnologyPanel({player,rooms,state,catalog,classic,onResearch,onLocate}:{player?:V6Player;rooms:V6Room[];state:V6Snapshot|null;catalog:V6Catalog;classic?:boolean;onResearch:(lab:V6Room|null,branch:string)=>void;onLocate:(lab:V6Room)=>void}){
  return <div className="v6-tech-tree">{V6_BRANCHES.map(b=>{
    const tier=player?.branches[b.id]??0,labs=rooms.filter(q=>q.kind==='research-lab'&&q.branch===b.id&&q.hp>0);
    const lab=labs.find(q=>q.progress>=1&&q.powered&&q.connected&&q.online!==false)??labs.find(q=>q.progress>=1)??labs[0];
    const research=(player?.researches??(player?.research?[player.research]:[])).find(q=>q.branch===b.id),quote=v6ResearchQuote(catalog,tier+1,b.id,player?.branches);
    const network=lab?state?.networkStores?.find(s=>s.owner===lab.owner&&s.cells.some(p=>containsV6(lab.rect,p))):undefined;
    const running=research?rooms.find(q=>q.id===research.lab):undefined;
    // Classic researches at the command core and only pays credits.
    const stopped=!classic&&!!research&&(!running||!running.powered||!running.connected||running.online===false);
    const lock=classic
      ?(!quote?'科技数据尚未就绪':(player?.credits??0)<quote.credits?'经费不足':'')
      :!lab?'先建此方向的研究所':lab.progress<1?'等待研究所施工完成':!lab.powered?'研究所缺电':!lab.connected?'研究所未接算力网络':lab.online===false?'研究所维护算力不足':!quote?'科技数据尚未就绪':(player?.credits??0)<quote.credits?'经费不足':(player?.science??0)<quote.science?'科研数据不足':(network?.compute??0)<quote.compute?'接入网络算力不足':'';
    return <article key={b.id} style={{'--branch':b.color} as React.CSSProperties}><header><FlaskConical/><h3>{b.name}</h3><b>T{tier}</b></header><p>{b.description}</p>
      <div className="v6-tech-track">{[1,2,3,4,5].map(t=><div key={t} className={tier>=t?'unlocked':research?.target===t?'researching':''}><span>T{t}</span><small>{catalog.items.filter(c=>c.branch===b.id&&c.tier===t&&c.category==='units').map(c=>c.name).join(' · ')||'基础设施与装备改进'}</small></div>)}</div>
      {research?<div className={`v6-research-progress ${stopped?'stalled':''}`}><i style={{width:`${research.progress*100}%`}}/><span>{stopped?'研究暂停':'研究中'} T{research.target} · {Math.floor(research.progress*research.duration)}/{rateV6(research.duration)}s</span></div>:tier<5&&quote&&<div className="v6-research-quote"><b>下一阶 T{tier+1}</b>{classic?<><span>◈ {formatV6(quote.credits)} · {rateV6(quote.seconds)}s</span><small>核心研究 · 只消耗经费</small></>:<><span>◈ {formatV6(quote.credits)} · 算力 {formatV6(quote.compute)}</span><span>科研数据 {formatV6(quote.science)} · {rateV6(quote.seconds)}s</span></>}{quote.multiplier>1&&<small>兼修投入 ×{rateV6(quote.multiplier)} · 经费{classic?'':'、数据'}和时间已计入</small>}{!classic&&<small>研究额外占用 {rateV6(quote.upkeep)} 算力/s</small>}</div>}
      <button className="v6-primary" disabled={tier>=5||!!research||!!lock} onClick={()=>{if(classic)onResearch(null,b.id);else if(lab)onResearch(lab,b.id);}}>{tier>=5?'最高等级':research?stopped?'恢复供给后继续':'研究进行中':lock||`研究 T${tier+1}`}<ArrowUp size={15}/></button>
      {!classic&&lab&&<button className="v6-text-action" onClick={()=>onLocate(lab)}>定位研究室 · {lab.rect.x},{lab.rect.y}<ChevronRight size={14}/></button>}
    </article>;
  })}</div>;
}
