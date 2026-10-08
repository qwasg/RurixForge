import type { V6Point,V6Rect } from './sentinelsV6Geometry';
export const V6_PROTOCOL='code-sentinels-pvp/6.1';
export type V6Branch='speed'|'security'|'algorithm'|'science'|'lightweight';
export const V6_BRANCHES: {id:V6Branch;name:string;color:string;description:string}[]=[
  {id:'speed',name:'速度',color:'#f4bd6c',description:'机动突击 · 超频 · 快速整备'},
  {id:'security',name:'网安',color:'#76d6bf',description:'拦截防护 · 抗干扰 · 战地修复'},
  {id:'algorithm',name:'算法',color:'#79b9f4',description:'预测瞄准 · 定向贯穿 · 精确打击'},
  {id:'science',name:'科研',color:'#b7a0ed',description:'粒子武器 · 结构破坏 · 轨道打击'},
  {id:'lightweight',name:'轻量',color:'#c4d487',description:'分散部署 · 低维护 · 模块化支援'},
];
export interface V6Research{lab:number;branch:string;target:number;progress:number;duration:number}
export interface V6Player{owner:number;credits:number;compute:number;computeCapacity:number;power:number;demand:number;income:number;production:number;branches:Record<string,number>;research:V6Research|null;researches?:V6Research[];dominance:number;ai:boolean;lostValue:number;science?:number}
export interface V6Building{id:number;owner:number;rect:V6Rect;kind:string;tier:number;hp:number;maxHp:number;progress:number;buildTime:number;powered:boolean;connected:boolean;power:number;demand:number;capacity:number;branch:string|null;inventory:number;invested:number;jam:number;shield:number;born:number}
export interface V6Room{id:number;shell:number;owner:number;rect:V6Rect;kind:string;branch:string|null;tier:number;hp:number;maxHp:number;powered:boolean;connected:boolean;capacity:number;capacityBudget?:number|null;potentialCapacity?:number|null;gpus:string[];inventory:number;progress:number;buildTime:number;cooldown:number;online?:boolean;maintenance?:number;equipmentShare?:number}
export interface V6Unit{id:number;owner:number;kind:string;pos:V6Point;x:number;y:number;z:number;tier:number;hp:number;maxHp:number;battery:number;batteryMax:number;covered:boolean;wired:boolean;fuel?:number;fuelMax?:number;energy?:number;energyMax?:number;altitude?:number;flightState?:string;sourceFacility?:number;route:V6Point[];target:number|null;cooldown:number;skillCooldown:number;plugins:string[];statuses:Record<string,number>;invested:number;moving:boolean;attackCount:number;branch?:string;goal?:V6Point|null;queuedGoals?:V6Point[];pluginDiscount?:number}
export interface V6Link{id:number;owner:number;kind:string;path:V6Point[];hp:number;active:boolean;unitEndpoints?:number[]}
export interface V6Wall{id:number;owner:number;pos:V6Point;kind:string;hp:number;maxHp:number;shield:number}
export interface V6Resource{id:number;pos:V6Point;kind:string;remaining:number;owner:number;capture:number;contested:boolean;capturer?:number}
export interface V6Projectile{id:number;owner:number;source:number;target:number|null;origin:V6Point;destination:V6Point;x:number;y:number;z:number;age:number;duration:number;damage:number;radius:number;kind:string;damageType:string;penetration:number}
export interface V6Event{id:number;tick:number;kind:string;pos:V6Point;owner:number;magnitude:number;subject:number}
export interface V6Job{id:number;owner:number;target:number;rect:V6Rect;kind:string;worker:V6Point;route:V6Point[];progress:number;duration:number;invested:number;blocked:boolean}
export interface V6Rubble{id:number;owner:number;rect:V6Rect;salvage:number}
export interface V6NetworkStore{owner:number;anchor:V6Point;cells:V6Point[];compute:number;capacity:number;production:number}
export interface V6PowerGrid{owner:number;cells:V6Point[];output:number;load:number}
export interface V6ShieldRegion{owner:number;anchor:V6Point;cells:V6Point[];current:number;capacity:number;network:V6Point}
export type V6Ruleset='full'|'classic';
export interface V6Snapshot{version:6;revision:number;tick:number;seed:number;width:number;height:number;minLevel:number;maxLevel:number;players:V6Player[];terrain:number[];buildings:V6Building[];rooms:V6Room[];units:V6Unit[];links:V6Link[];walls:V6Wall[];resources:V6Resource[];projectiles:V6Projectile[];events:V6Event[];winner:number|null;winReason:string;explored:V6Point[][];visible:V6Point[][];jobs?:V6Job[];rubble?:V6Rubble[];networkStores?:V6NetworkStore[];powerGrids?:V6PowerGrid[];shieldAuto?:boolean[];shieldRegions?:V6ShieldRegion[];theme?:string;ruleset?:V6Ruleset;playback?:{paused:boolean;speed:number;currentTick:number;totalTicks:number}|null}
export type V6Command=
  |{op:'shell';rect:V6Rect}
  |{op:'room';shell:number;rect:V6Rect;kind:string;branch?:string}
  |{op:'build';pos:V6Point;kind:string}
  |{op:'wire';kind:string;path:V6Point[];unitEndpoints?:number[]}
  |{op:'wall';kind:string;path:V6Point[]}
  |{op:'install-gpu';room:number;model:string}
  |{op:'remove-gpu';room:number;bay:number}
  |{op:'research';room:number;branch:string}
  |{op:'deploy';room:number;kind:string;pos:V6Point}
  |{op:'move';ids:number[];pos:V6Point}
  |{op:'queue-move';ids:number[];pos:V6Point}
  |{op:'attack';ids:number[];target:number}
  |{op:'stop';ids:number[]}
  |{op:'skill';id:number;pos:V6Point;direction?:V6Point}
  |{op:'plugin';id:number;plugin:string}
  |{op:'upgrade'|'repair'|'recycle';id:number}
  |{op:'shield';enabled:boolean}
  |{op:'expand-shell';id:number;rect:V6Rect}
  |{op:'split-room';id:number;axis:'x'|'y';offset:number}
  |{op:'merge-rooms';ids:number[]}
  |{op:'convert-room';id:number;kind:string;branch?:string}
  |{op:'cancel'|'clear-rubble';id:number};
export interface V6Receipt{accepted:boolean;sequence:number;tick:number;reason:string}
export interface V6Preview{
  valid:boolean;reason:string;
  cost:{credits:number;compute:number;science:number};
  powerBefore:number;powerAfter:number;demandBefore:number;demandAfter:number;
  targetPowered?:boolean;targetConnected?:boolean;completionProjection?:boolean;
  netArea?:number;capacity?:number;costPerCapacity?:number;
  computeBefore?:number;computeAfter?:number;computeCapacityBefore?:number;computeCapacityAfter?:number;
}
export interface V6Session{mode:'solo'|'host'|'join';status:'lobby'|'battle'|'finished';roomId:string;code:string;address:string;ruleset?:V6Ruleset;players:{owner:number;nickname:string;ready:boolean;connected:boolean}[];lastSequence:number;paused?:boolean;replay?:boolean;replayState?:{paused:boolean;speed:number;tick:number;durationTicks:number};connected?:boolean;error?:string|null;replicaSynced?:boolean}
export interface V6Status{protocol:string;playerId:number|null;session:V6Session|null;snapshot:V6Snapshot|null}
export interface V6SessionOptions{mode:'solo'|'host'|'join';theme:string;seed:number;ruleset?:V6Ruleset;address?:string;code?:string;nickname?:string;port?:number}
export const V6_RULESETS:{id:V6Ruleset;name:string;description:string}[]=[
  {id:'classic',name:'单层塔防部署',description:'只用经费在核心与己方设施12格内部署炮台，无毛坯、房间、接线与显卡'},
  {id:'full',name:'完整基地建造',description:'毛坯楼体、功能房间、电力与算力网络、显卡与研究所的完整流程'},
];
/** Classic hides the whole base-building layer; the snapshot is authoritative. */
export function v6IsClassic(snapshot?:V6Snapshot|null,session?:V6Session|null){
  return (snapshot?.ruleset??session?.ruleset??'full')==='classic';
}
export type V6Category='rooms'|'buildings'|'units'|'gpus'|'plugins';
export interface V6CatalogItem{id:string;kind:string;name:string;category:V6Category;role?:string;description:string;branch?:string|null;tier:number;cost:number;power?:number;rate?:number;capacity?:number;image?:string;producer?:string;damage?:number;range?:number;skill?:string;skillCost?:number;cooldown?:number;slot?:string;chassis?:string;skillShape?:string;skillRadius?:number;skillWidth?:number;skillAngle?:number;skillRange?:number;width?:number;height?:number;minArea?:number;computeCost?:number;ammoPerShot?:number;computePerAttack?:number;energyPerAttack?:number;speed?:number;upkeepCompute?:number;modifier?:string;magnitude?:number;passive?:{id:string;name:string;description:string}|null}
export interface V6ClassicRules{buildings:string[];unitCategories:string[];deployRadius:number}
export interface V6Catalog{items:V6CatalogItem[];rules?:Record<string,unknown>;classic?:V6ClassicRules}
export type V6Selection={kind:'building'|'room'|'unit'|'resource'|'wall'|'link'|'rubble';id:number};
export interface V6Pick{kind:V6Selection['kind']|'terrain';id:number;owner:number;pos:V6Point}
export function validateV6Snapshot(value:unknown):V6Snapshot {
  const s=value as V6Snapshot;
  if(!s||s.version!==6||s.width!==128||s.height!==96||!Number.isSafeInteger(s.tick)||s.tick<0||!Number.isSafeInteger(s.revision))throw Error('战场快照版本或坐标不匹配');
  for(const key of ['players','terrain','buildings','rooms','units','links','walls','resources','projectiles','events','explored','visible'] as const)if(!Array.isArray(s[key]))throw Error(`战场快照缺少 ${key}`);
  if(s.terrain.length!==128*96||s.players.length!==2||s.players.some(p=>![1,2].includes(p.owner)||![p.credits,p.compute,p.power,p.demand].every(Number.isFinite)))throw Error('战场快照不完整');
  return s;
}
export function normalizeV6Catalog(value:unknown):V6Catalog {
  const data=value as Record<string,unknown>;
  if(!data||typeof data!=='object')throw Error('装备目录未就绪');
  const items:V6CatalogItem[]=[];
  for(const category of ['rooms','buildings','units','gpus','plugins'] as V6Category[]) {
    const values=Array.isArray(data.items)?(data.items as Record<string,unknown>[]).filter(i=>i.category===category):data[category];
    if(!Array.isArray(values))continue;
    for(const raw of values) {
      const item=raw as Record<string,unknown>,id=String(item.id??item.kind??'');
      if(!id)continue;
      items.push({...item,id,kind:String(item.kind??id),name:String(item.name??id),category,role:String(item.role??item.category??''),description:String(item.description??''),tier:Number(item.tier??item.tech??1),cost:Number(item.cost??0)} as V6CatalogItem);
    }
  }
  const classic=data.classic as V6ClassicRules|undefined;
  return {items,rules:(data.rules??{shell:data.shell,research:data.research,victory:data.victory,ai:data.ai,construction:data.construction}) as Record<string,unknown>,
    classic:Array.isArray(classic?.buildings)?{buildings:classic!.buildings.map(String),unitCategories:(classic!.unitCategories??[]).map(String),deployRadius:Number(classic!.deployRadius??12)}:undefined};
}
export const V6_CLASSIC_DEFAULTS:V6ClassicRules={buildings:['extractor','airstrip'],unitCategories:['turret','ai','vehicle','air'],deployRadius:12};
/** Cards a classic session may actually order, filtered by the native descriptor. */
export function v6ClassicCards(catalog:V6Catalog,items:V6CatalogItem[]){
  const rules=catalog.classic??V6_CLASSIC_DEFAULTS;
  return items.filter(item=>item.category==='buildings'?rules.buildings.includes(item.id)
    :item.category==='units'?rules.unitCategories.includes(item.role??'')
    :item.category==='plugins');
}
export function v6AiEconomy(catalog:V6Catalog,units:V6Unit[],owner:number){
  const types=new Set(catalog.items.filter(i=>i.category==='units'&&i.role==='ai').map(i=>i.id));
  const count=units.filter(u=>u.owner===owner&&u.hp>0&&types.has(u.kind)).length;
  const rule=catalog.rules?.ai as {purchaseGrowth?:number;upkeepQuadratic?:number;deployCompute?:number}|undefined;
  const ready=Number.isFinite(rule?.purchaseGrowth)&&Number.isFinite(rule?.upkeepQuadratic);
  return {count,ready,multiplier:ready?1+rule!.purchaseGrowth!*(count*(count+1)):1,upkeep:ready?rule!.upkeepQuadratic!*count*count:0,deployCompute:rule?.deployCompute};
}
export function v6PluginLock(item:V6CatalogItem,unit:V6Unit|null|undefined,catalog:V6CatalogItem[],owner:number){
  if(!unit||unit.owner!==owner||unit.hp<=0)return '先选中己方单位';
  const branch=unit.branch??catalog.find(c=>c.category==='units'&&c.id===unit.kind)?.branch;
  if(branch!==item.branch)return '需要同一科技分支';
  if(unit.tier<item.tier)return `单位需升至T${item.tier}`;
  if(unit.plugins.includes(item.id))return '已安装';
  if(unit.plugins.some(id=>catalog.some(c=>c.category==='plugins'&&c.id===id&&(c.slot??c.role)===(item.slot??item.role))))return '此类槽位已占用';
  return '';
}
export function v6UnitCosts(unit:V6Unit,item:V6CatalogItem,catalog:V6CatalogItem[]){
  const modifier=(name:string)=>unit.plugins.reduce((n,id)=>n+(catalog.find(c=>c.category==='plugins'&&c.id===id&&c.modifier===name)?.magnitude??0),0);
  const clamp=(n:number)=>Math.max(.15,Math.min(1,n));
  const compute=clamp(1-modifier('compute-efficiency'))*(unit.statuses['compute-efficiency']>0?.8:1),payload=clamp(1-modifier('payload-efficiency'));
  return {energy:(item.energyPerAttack??0)*payload,compute:(item.computePerAttack??0)*payload*compute,skill:(item.skillCost??0)*(1+modifier('skill-power'))*compute*(item.id==='gemini'&&unit.statuses['gemini-ready']>0?.9:1)};
}
export const formatV6=(n:number|undefined)=>Math.max(0,Math.floor(n??0)).toLocaleString('en-US');
export const rateV6=(n:number|undefined)=>Math.max(0,n??0).toLocaleString('en-US',{maximumFractionDigits:2});
export const timeV6=(tick:number)=>`${String(Math.floor(tick/3600)).padStart(2,'0')}:${String(Math.floor(tick/60)%60).padStart(2,'0')}`;
export function v6EntityPosition(entity:{rect?:V6Rect;pos?:V6Point}):V6Point {return entity.rect?{x:entity.rect.x+entity.rect.w/2,y:entity.rect.y+entity.rect.h/2,z:entity.rect.z}:entity.pos??{x:0,y:0,z:0};}
export function v6UnitElevation(unit:Pick<V6Unit,'altitude'>){
  return unit.altitude??0;
}

export const V6_SKILL_NAMES:Record<string,string>={dash:'定向突进','dash-strike':'定向突进','debug-cone':'调试扫射',precision:'精确射击',overclock:'短时超频',guard:'装甲戒备','target-lock':'精确锁定','charged-shot':'蓄能打击','field-resupply':'战地充能','field-recharge':'战地充能','piercing-mark':'推理贯穿','intercept-barrier':'拦截屏障','repair-armor':'区域维护','telegraphed-bombardment':'双束轰击','repair-heal-cut':'震荡抑制','targeted-support':'定点支援'};
