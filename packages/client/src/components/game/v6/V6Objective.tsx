import {Shield} from 'lucide-react';
import {formatV6,timeV6,type V6Catalog,type V6Resource,type V6Snapshot} from '@/lib/sentinelsV6';

export function v6OutcomeTitle(state:Pick<V6Snapshot,'winner'|'winReason'>,owner:number){
  const won=state.winner===owner;
  if(state.winReason.includes('节点'))return won?'战略压制胜利':'战略压制失利';
  if(state.winReason.includes('核心'))return won?'敌方核心已摧毁':'指挥核心已摧毁';
  return won?'行动胜利':'行动失利';
}
export default function V6Objective({state,owner,coreHp,catalog,onLocate}:{state:V6Snapshot;owner:number;coreHp:number;catalog:V6Catalog;onLocate:(node:V6Resource)=>void}){
  const rule=catalog.rules?.victory as {startsAtSeconds?:number;nodeSeconds?:number;majority?:number;rollbackMultiplier?:number}|undefined;
  const starts=rule?.startsAtSeconds??1080,goal=rule?.nodeSeconds??360,majority=rule?.majority??2,rollback=rule?.rollbackMultiplier??2;
  const nodes=state.resources.filter(n=>n.kind==='node'),active=state.tick>=starts*60;
  const rows=[{id:owner,name:'己方',enemy:false},{id:owner===1?2:1,name:'敌方',enemy:true}].map(side=>{
    const progress=state.players.find(p=>p.owner===side.id)?.dominance??0,count=nodes.filter(n=>n.owner===side.id).length;
    const contested=nodes.some(n=>n.owner===side.id&&n.contested);
    const status=state.winner?(state.winner===side.id&&progress>=goal?'已完成':'已结束'):!active?`${timeV6(starts*60)}开放`:count<majority?progress>0?`回退×${rollback}`:'未占多数':contested?'争夺暂停':'压制中';
    return {...side,progress:Math.max(0,Math.min(goal,progress)),count,contested,status};
  });
  const danger=active&&!rows[1].contested&&rows[1].count>=majority&&rows[1].progress>=goal-60&&!state.winner;
  const title=state.winner?v6OutcomeTitle(state,owner):!active?'夺取矿点，建立前沿补给':danger?'敌方即将完成压制，争夺节点':rows[0].count>=majority?'守住节点，推进压制':'控制两个节点，完成压制';
  return <aside className={`v6-objective ${danger?'danger':''}`}><span>STRATEGIC CONTROL / 资源压制</span><strong>{title}</strong>
    <div className="v6-node-markers">{nodes.map(n=><button key={n.id} aria-label={`定位战略节点${n.id}`} onClick={()=>onLocate(n)} className={n.contested?'contested':n.owner===owner?'owned':n.owner?'enemy':''}><Shield size={16}/>{n.contested?'争夺':n.owner===owner?'控制':n.owner?'敌控':'中立'}</button>)}</div>
    <div className="v6-dominance-tracks">{rows.map(row=><div key={row.id} className={row.enemy?'enemy':'owned'}><div><span>{row.name} · {row.status}</span><b>{Math.floor(row.progress)} / {goal}s</b></div><meter aria-label={`${row.name}压制`} min={0} max={goal} value={row.progress}/></div>)}</div>
    <small>CORE {formatV6(coreHp)} · 争夺暂停，失去多数后双速回退</small>
  </aside>;
}
