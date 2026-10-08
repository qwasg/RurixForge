import {Cpu,FlaskConical,LockKeyhole,Plus,Warehouse,Zap} from 'lucide-react';
import {V6_BRANCHES,rateV6,type V6CatalogItem,type V6Player} from '@/lib/sentinelsV6';
export const V6_ART='/games/code-sentinels/ui-v6/';
const gpuImages:Record<string,string>={
  'rtx-5060':'rtx-5060-msi-ventus2x.png','rtx-5070':'rtx-5070-fe.png','rtx-5080':'rtx-5080-fe.jpg','rtx-5090':'rtx-5090-fe.jpg','rtx-pro-6000':'rtx-pro6000-blackwell-workstation-card.jpg','a100':'a100-80gb-pcie.png','h200':'h200-nvl-pcie.png',
};
export function v6CardImage(item:V6CatalogItem){
  if(item.image)return item.image;
  if(item.category==='gpus')return '/games/code-sentinels/gpus/'+gpuImages[item.id];
  if(['deepseek','gpt','gemini','claude','kimi','minimax','glm'].includes(item.id))return V6_ART+'operators/'+item.id+'.png';
  return V6_ART+(item.category==='units'?'units/':item.category==='plugins'?'plugins/':'facilities/')+item.id+'.png';
}
export function v6CardLock(item:V6CatalogItem,player?:V6Player){
  if(!player)return '';
  const tier=item.branch?player.branches[item.branch]??0:Math.max(1,...Object.values(player.branches));
  if(item.tier>tier)return `${V6_BRANCHES.find(b=>b.id===item.branch)?.name??'科技'} T${item.tier}`;
  return player.credits<item.cost?'经费不足':'';
}
export default function V6Card({item,player,selected,onClick,index,prerequisite}:{item:V6CatalogItem;player?:V6Player;selected?:boolean;onClick:()=>void;index?:number;prerequisite?:string}){
  const branch=V6_BRANCHES.find(b=>b.id===item.branch),lock=prerequisite||v6CardLock(item,player);
  const Icon=item.category==='gpus'?Cpu:item.category==='rooms'?Warehouse:item.category==='plugins'?FlaskConical:Zap;
  return <button className={`v6-card ${selected?'selected':''} ${lock?'locked':''} ${item.role==='ai'?'operator':''}`} style={{'--branch':branch?.color??'#d5bf80'} as React.CSSProperties} onClick={onClick} aria-label={`${item.name}，${rateV6(item.cost)}经费${lock?'，'+lock:''}`} aria-pressed={selected}>
    <div className="v6-card-image"><img src={v6CardImage(item)} alt="" onLoad={e=>{e.currentTarget.style.visibility='visible';}} onError={e=>{e.currentTarget.style.visibility='hidden';}}/><Icon className="v6-card-emblem" size={28}/></div>
    <div className="v6-card-top"><span>{branch?.name??'通用'} / T{item.tier}</span>{index!==undefined&&index<9&&<kbd>{index+1}</kbd>}</div>
    <div className="v6-card-copy"><strong>{item.name}</strong><small>{item.description}</small>{item.category==='plugins'&&(item.computeCost??0)>0&&<small>安装另需 {rateV6(item.computeCost)} 算力</small>}<footer><b>◈ {rateV6(item.cost)}</b>{lock?<span><LockKeyhole size={11}/>{lock}</span>:<Plus size={15}/>}</footer></div>
  </button>;
}
