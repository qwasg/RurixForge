import {Shield} from 'lucide-react';
import {formatV6,type V6ShieldRegion} from '@/lib/sentinelsV6';
export default function V6CudaControl({enabled,regions,onToggle}:{enabled:boolean|undefined;regions:V6ShieldRegion[];onToggle:(enabled:boolean)=>void}){
  const current=regions.reduce((sum,r)=>sum+r.current,0),capacity=regions.reduce((sum,r)=>sum+r.capacity,0);
  return <section className="v6-cuda-control"><header><Shield size={21}/><div><b>CUDA 围合防护</b><small>{regions.length} 个有效区域 · {formatV6(current)} / {formatV6(capacity)} 护盾</small></div></header>
    <p>数据墙和护城河需在本层闭合，内部有运行中的数据中心。门或竖井敞开会破坏封闭。</p>
    <button className="v6-primary" disabled={enabled===undefined} onClick={()=>onToggle(!enabled)}>{enabled===undefined?'状态同步中':enabled?'停止自动充能':'恢复自动充能'}</button>
    <small>{enabled?'持续使用已接入机房的算力填充护盾。':'停止充能会保留当前盾量，为研究和技能留出算力。'}</small>
  </section>;
}
