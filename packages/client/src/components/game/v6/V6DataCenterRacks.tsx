import {useEffect,useState} from 'react';
import {ChevronDown,Cpu,Wrench} from 'lucide-react';
import {rateV6,type V6Command,type V6Preview,type V6Room} from '@/lib/sentinelsV6';
import {v6Request} from '@/lib/useSentinelsV6';

type Props={room:V6Room;owned:boolean;canOrder:boolean;sessionId?:string;tickSeconds:number;onInstall:()=>void;onOrder:(command:V6Command)=>Promise<unknown>|void};
type Quote={key:string;preview?:V6Preview;error?:string};
const capacityValue=(value:number|null|undefined)=>typeof value==='number'&&Number.isSafeInteger(value)&&value>=0?value:null;

export default function V6DataCenterRacks({room,owned,canOrder,sessionId,tickSeconds,onInstall,onOrder}:Props){
  const budget=capacityValue(room.capacityBudget),potential=capacityValue(room.potentialCapacity);
  const expandable=budget!==null&&potential!==null&&potential>budget;
  const available=room.capacity,blocked=budget===null?0:Math.max(0,budget-available);
  const allowed=owned&&canOrder&&room.progress>=1;
  const key=JSON.stringify([sessionId,room.id,room.kind,budget,potential,available,room.gpus,room.progress]);
  const[quote,setQuote]=useState<Quote|null>(null),[submitting,setSubmitting]=useState(false),[submitError,setSubmitError]=useState('');
  const currentQuote=quote?.key===key?quote:null;
  useEffect(()=>{
    setSubmitError('');
    if(!expandable||!allowed){setQuote(null);return;}
    const controller=new AbortController();
    setQuote(previous=>previous?.key===key?previous:null);
    void v6Request<V6Preview>('preview',{command:{op:'convert-room',id:room.id,kind:room.kind},sessionId},controller.signal)
      .then(preview=>{if(!controller.signal.aborted)setQuote({key,preview});})
      .catch(error=>{if(!controller.signal.aborted)setQuote({key,error:error instanceof Error?error.message:'暂时无法核对改装费用'});});
    return()=>controller.abort();
  },[key,allowed,expandable,room.id,room.kind,sessionId,tickSeconds]);
  const preview=currentQuote?.preview;
  const quoted=preview?.valid===true&&Number.isFinite(preview.cost?.credits)&&preview.cost.credits>=0;
  async function reorganize(){
    if(!allowed||!expandable||!quoted||submitting)return;
    setSubmitting(true);setSubmitError('');
    try{await onOrder({op:'convert-room',id:room.id,kind:room.kind});}
    catch(error){setSubmitError(error instanceof Error?error.message:'重整未提交，请重试');}
    finally{setSubmitting(false);}
  }
  return <section aria-label="数据中心机架" className="v6-facility-status">
    <div className="v6-stat-line"><span>机架 · 已装 / 可用</span><b>{room.gpus.length} / {available}</b></div>
    {budget!==null&&<div className="v6-stat-line"><span>已购机架</span><b>{budget}{potential!==null&&<small> · 空间上限 {potential}</small>}</b></div>}
    <div className="v6-rack">{Array.from({length:Math.min(32,Math.max(0,available))},(_,i)=><button key={i} className={room.gpus[i]?'filled':''} disabled={!owned} aria-label={room.gpus[i]??`空机架${i+1}`} onClick={onInstall}>{room.gpus[i]?<Cpu size={18}/>:<span>＋</span>}<small>{room.gpus[i]?.replace('rtx-','')??'EMPTY'}</small></button>)}</div>
    {expandable&&<p className="v6-muted">合并保留已有的 {budget} 个机架，不会自动添置。当前空间可容纳 {potential} 个，付费重整后可增加机架。</p>}
    {blocked>0&&<p className="v6-muted">空间占用使 {blocked} 个已购机架暂不可用；调整分隔或移除占用后恢复。</p>}
    {expandable&&owned&&<>
      <button className="v6-primary" disabled={!allowed||!quoted||submitting} onClick={()=>void reorganize()}><Wrench size={14}/>{submitting?'正在提交重整':'重整机架'}{quoted&&<> · ◈ {rateV6(preview.cost.credits)}</>}</button>
      <small className="v6-unit-hint">施工 8 秒。请先手动卸载显卡、清空库存，并处理在途运输与关联研究。</small>
      <p className="v6-unit-hint" role="status">{submitError||(!canOrder?'当前无法下达改装指令':room.progress<1?'等待当前施工完成':currentQuote?.error||(!preview?'正在核对改装费用…':!preview.valid?preview.reason:!quoted?'暂时无法核对改装费用':`重整后已购机架增加到 ${potential} 个。`))}</p>
    </>}
    {room.gpus.length>0&&owned&&<details className="v6-room-tools"><summary>卸载与回收显卡<ChevronDown size={13}/></summary><div>{room.gpus.map((model,bay)=><button key={bay} disabled={!canOrder||submitting} onClick={()=>void onOrder({op:'remove-gpu',room:room.id,bay})}>卸载机架 {bay+1} · {model}</button>)}</div></details>}
  </section>;
}
