import {Cable,Link2} from 'lucide-react';
import {containsV6} from '@/lib/sentinelsV6Geometry';
import {formatV6,rateV6,type V6Building,type V6Room,type V6Snapshot,type V6CatalogItem} from '@/lib/sentinelsV6';

export function facilityV6Network(entity:V6Building|V6Room,state:V6Snapshot,items:V6CatalogItem[]){
  const item=items.find(c=>c.id===entity.kind),room='shell'in entity;
  const generation=room?0:entity.power;
  const demand=room?(item?.power??0)*(entity.equipmentShare??1)+entity.gpus.reduce((sum,id)=>sum+(items.find(c=>c.id===id)?.power??0),0):entity.demand;
  const grid=state.powerGrids?.find(g=>g.owner===entity.owner&&g.cells.some(p=>containsV6(entity.rect,p)));
  const network=state.networkStores?.find(g=>g.owner===entity.owner&&g.cells.some(p=>containsV6(entity.rect,p)));
  let reason='',tone:'good'|'bad'|'muted'='good';
  if(entity.progress<1){reason=`施工中 · ${Math.floor(entity.progress*100)}%`;tone='muted';}
  else if(!room&&entity.jam>0){reason='设备受干扰，运行暂停';tone='bad';}
  else if(!room&&['coal-power','nuclear-power'].includes(entity.kind)&&!entity.powered){reason='经费不足，电站停机';tone='bad';}
  else if(demand>0&&!entity.powered){
    reason=!grid?'未接电力线路 · L 连接发电站':grid.output<=0?'当前线路没有运行中的发电设施':grid.output<grid.load?`当前电网过载 · 缺 ${rateV6(grid.load-grid.output)} 电力`:'设备暂未通电';tone='bad';
  }else if(room&&entity.kind==='data-center'&&entity.gpus.length===0){reason='机房已就绪 · I 安装显卡';tone='muted';}
  else if(room&&(entity.maintenance??item?.upkeepCompute??0)>0&&!entity.connected){reason='未接算力网络 · C 连接数据中心';tone='bad';}
  else if(room&&entity.online===false&&(entity.maintenance??0)>0){reason='接入网络算力不足，设备运行暂停';tone='bad';}
  else if(generation>0){reason=grid?'正在向本地电网供电':'发电就绪 · L 拉线接入用电设施';}
  else if(entity.kind==='mobile-relay'){reason=entity.connected?'回传在线 · 提供无线覆盖':'未接通机房回传，请布线或靠近运行机房';tone=entity.connected?'good':'bad';}
  else if(entity.kind==='repair-bay'){reason=entity.powered&&(!('online'in entity)||entity.online!==false)?'维修光环生效 · 附近友军回复中':'通电在线后为附近友军提供被动回复';tone=entity.powered&&(!('online'in entity)||entity.online!==false)?'good':'muted';}
  else {reason='设备运行正常';}
  return {reason,tone,grid,network,demand,generation};
}

export default function V6FacilityStatus({entity,state,items,owned,classic,onWire}:{entity:V6Building|V6Room;state:V6Snapshot;items:V6CatalogItem[];owned:boolean;classic?:boolean;onWire:(kind:'power'|'compute')=>void}){
  if(entity.kind==='shell'||!owned)return null;
  const status=facilityV6Network(entity,state,items);
  // Classic has no grids: a finished structure is simply running.
  if(classic)return <div className="v6-facility-status"><p className={entity.progress<1?'muted':'good'}>{entity.progress<1?`施工中 · ${Math.floor(entity.progress*100)}%`:'设施运行正常'}</p></div>;
  return <div className="v6-facility-status"><p className={status.tone}>{status.reason}</p>
    {'shell'in entity&&entity.equipmentShare!==undefined&&<div className="v6-stat-line"><span>功能设备配额</span><b>{rateV6(entity.equipmentShare*100)}%</b></div>}
    {'shell'in entity&&(entity.potentialCapacity!==undefined||entity.capacity>0)&&<div className="v6-stat-line"><span>净面积容量</span><b>{formatV6(entity.capacity)}{entity.potentialCapacity!=null?` / 几何上限 ${formatV6(entity.potentialCapacity)}`:''}</b></div>}
    {status.demand>0&&<div className="v6-stat-line"><span>此设施用电</span><b>{rateV6(status.demand)}</b></div>}
    {status.grid&&<div className="v6-stat-line"><span>接入电网 · 发电 / 负载</span><b className={status.grid.output<status.grid.load?'bad':'good'}>{rateV6(status.grid.output)} / {rateV6(status.grid.load)}</b></div>}
    {status.network&&<div className="v6-stat-line"><span>接入网络 · 算力余量</span><b>{formatV6(status.network.compute)} / {formatV6(status.network.capacity)}</b></div>}
    <div className="v6-facility-ports"><button onClick={()=>onWire('power')}><Cable size={14}/>从此处拉电线 <kbd>L</kbd></button><button onClick={()=>onWire('compute')}><Link2 size={14}/>从此处拉算力线 <kbd>C</kbd></button></div>
  </div>;
}
