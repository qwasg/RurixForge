import {ChevronRight,PlaneLanding,Square,Move} from 'lucide-react';
import {rateV6,v6UnitCosts,V6_SKILL_NAMES,type V6CatalogItem,type V6Unit} from '@/lib/sentinelsV6';

const FLIGHT:Record<string,string>={'taking-off':'起飞爬升 · 暂不攻击',cruising:'巡航作战',returning:'返航补给',landing:'下降着陆 · 暂不攻击',landed:'已着陆 · 等待补给',emergency:'燃料耗尽 · 紧急迫降'};
const SHAPES:Record<string,string>={self:'自身增益',target:'单体指令',direction:'定向贯穿',line:'直线施法',cone:'扇形施法',area:'范围施法','target-friendly':'友方支援','circle-ally':'区域维修与强化','circle-mixed':'区域减疗与支援','target-ally':'单体友方增益',circle:'范围轰击'};
const STATUSES:Record<string,string>={silence:'技能受干扰',dash:'突进',haste:'超频',fortify:'强化护甲',precision:'精确锁定','charged-shot':'下一轮蓄能攻击','anti-heal':'维修 / 治疗受抑制','support-boost':'火力支援','compute-efficiency':'算力优化',marked:'已被标记',slow:'减速','gemini-ready':'三轮协同 · 下一技能九折','passive-guardian-intercept-cooldown':'守护拦截冷却','passive-maintenance-daemon-cooldown':'维护守护冷却'};

function Reserve({name,value,max,empty}:{name:string;value:number;max:number;empty:string}){
  if(max<=0)return null;
  const percent=Math.max(0,Math.min(100,value/max*100));
  return <div className={`v6-unit-reserve ${percent<20?'low':''}`}><div><span>{name}</span><b>{rateV6(value)} <small>/ {rateV6(max)}</small></b></div><meter aria-label={name} min={0} max={max} value={Math.max(0,value)}/>{value<=0&&<small>{empty}</small>}</div>;
}

export default function V6UnitStatus({unit,item,owned,catalog,onSkill,onPlugins,onMove,onStop,onReturn}:{unit:V6Unit;item?:V6CatalogItem;owned:boolean;catalog:V6CatalogItem[];onSkill:()=>void;onPlugins:()=>void;onMove:()=>void;onStop:()=>void;onReturn:()=>void}){
  const ai=item?.role==='ai',air=item?.role==='air',hasCompute=unit.batteryMax>0||(item?.skillCost??0)>0;
  const costs=item?v6UnitCosts(unit,item,catalog):undefined;
  const hasBranch=!!(unit.branch||item?.branch);
  const statuses=Object.entries(unit.statuses).filter(([,duration])=>duration>0);
  return <div className="v6-unit-status">
    {hasCompute&&<div className="v6-stat-line"><span>算力接入</span><b>{unit.wired?'有线驻守':unit.covered?'网络已接通':unit.batteryMax>0?'离网 · 使用随身算力':'未接指挥算力 · 主动能力需联网'}</b></div>}
    {air&&<p className={`v6-unit-flight ${unit.flightState==='emergency'?'bad':''}`}>{FLIGHT[unit.flightState??'']??'飞行状态同步中'}</p>}
    {(unit.transitProgress??0)>0&&<p className="v6-unit-hint">跨层通行 · {Math.floor(unit.transitProgress!*100)}% · 入口中断时暂停</p>}
    {owned&&!!unit.queuedGoals?.length&&<p className="v6-unit-hint">已排队 {unit.queuedGoals.length} 个后续移动点 · S 清空</p>}
    {owned&&<>
      <Reserve name="随身算力" value={unit.battery} max={unit.batteryMax} empty={unit.covered?'缓存已空，攻击依赖当前接入网络余量':'离网缓存已耗尽，请返回基站覆盖'}/>
      <Reserve name="弹药" value={unit.ammo} max={unit.ammoMax??0} empty="弹药耗尽，等待实体补给后恢复射击"/>
      <Reserve name="燃料" value={unit.fuel??0} max={unit.fuelMax??0} empty={air?'燃料耗尽，无法继续正常飞行':'燃料耗尽，等待补给后恢复移动'}/>
      <Reserve name="武器蓄能" value={unit.energy??0} max={unit.energyMax??0} empty="蓄能耗尽，需接电或停靠供电设施充能"/>
      {(unit.energyMax??0)>0&&<p className="v6-unit-hint">接入有余量的电网，或停靠已供电的工厂、仓库、机场及维修间附近充能。</p>}
      {costs&&<div className="v6-stat-line"><span>当前攻击消耗</span><b>{[costs.ammo>0?`${rateV6(costs.ammo)} 弹药`:'',costs.energy>0?`${rateV6(costs.energy)} 蓄能`:'',costs.compute>0?`${rateV6(costs.compute)} 算力`:''].filter(Boolean).join(' / ')||'无基础资源消耗'}</b></div>}
    </>}
    {item?.passive&&<div className="v6-unit-passive"><strong>{item.passive.name} · 被动</strong><p>{item.passive.description}</p></div>}
    {statuses.length>0&&<div className="v6-status-chips">{statuses.map(([name,duration])=><span key={name} className={['silence','anti-heal','marked','slow'].includes(name)?'negative':''}>{name.startsWith('target-lock:')?'锁定目标 #'+name.slice(12):STATUSES[name]??name} · {Math.ceil(duration)}s</span>)}</div>}
    {owned&&<>
      {item?.skill&&<><div className="v6-stat-line"><span>{SHAPES[item.skillShape??'']??'主动能力'}</span><b>{rateV6(costs?.skill)} 算力 · {unit.skillCooldown>0?`${unit.skillCooldown.toFixed(1)}s`:'就绪'}</b></div><button className="v6-primary" disabled={unit.skillCooldown>0||!!unit.statuses.silence||unit.hp<=0} onClick={onSkill}>发动主动能力 · {V6_SKILL_NAMES[item.skill]??item.skill} <kbd>Q</kbd></button></>}
      {hasBranch&&<><div className="v6-installed-plugins">{unit.plugins.map(id=><span key={id}>{catalog.find(c=>c.category==='plugins'&&c.id===id)?.name??id}</span>)}</div>{(unit.pluginDiscount??0)>0&&<p className="v6-unit-hint">模块工坊已接入 · 安装经费优惠 {Math.round(unit.pluginDiscount!*100)}%，算力费用不变。</p>}<button className="v6-text-action" onClick={onPlugins}>{ai?'插件整备':'武器模块'}<ChevronRight size={14}/></button></>}
      <div className="v6-unit-orders">{(item?.speed??0)>0&&<button onClick={onMove} disabled={unit.wired}><Move size={13}/>{unit.wired?'有线驻守':'移动'}</button>}<button onClick={onStop}><Square size={12}/>停止指令</button>{air&&<button onClick={onReturn}><PlaneLanding size={14}/>返回机场</button>}</div>
    </>}
  </div>;
}
