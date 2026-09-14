import {useCallback,useEffect,useMemo,useRef,useState,type PointerEvent as ReactPointerEvent} from 'react';
import {ArrowDown,ArrowUp,Box,Boxes,Check,ChevronDown,ChevronRight,ChevronUp,CircleHelp,Coins,Cpu,Expand,FlaskConical,Home,Layers3,Link2,Maximize2,MousePointer2,Move,Pause,Play,Radio,Save,Shield,Shovel,Swords,Truck,Warehouse,Wrench,X,Zap} from 'lucide-react';
import {useSentinelsV6,v6Request} from '@/lib/useSentinelsV6';
import {V6_BRANCHES,formatV6,rateV6,timeV6,v6AiEconomy,v6EntityPosition,v6UnitElevation,v6PluginLock,type V6CatalogItem,type V6Command,type V6Pick,type V6Preview,type V6Room,type V6Selection,type V6Snapshot} from '@/lib/sentinelsV6';
import {V6_INITIAL_CAMERA,cellAtV6,clampCameraV6,containsV6,dragRectV6,layerNameV6,orthogonalPathV6,rectOutlineV6,screenToWorldV6,skillOutlineV6,validShellRectV6,wireRouteV6,worldToScreenV6,type V6Camera,type V6Point,type V6Rect} from '@/lib/sentinelsV6Geometry';
import V6Lobby from '@/components/game/v6/V6Lobby';
import V6Card,{v6CardImage,v6CardLock} from '@/components/game/v6/V6Card';
import V6UnitStatus from '@/components/game/v6/V6UnitStatus';
import V6FacilityStatus from '@/components/game/v6/V6FacilityStatus';
import V6DataCenterRacks from '@/components/game/v6/V6DataCenterRacks';
import V6TechnologyPanel from '@/components/game/v6/V6TechnologyPanel';
import V6CudaControl from '@/components/game/v6/V6CudaControl';
import V6Guide from '@/components/game/v6/V6Guide';
import V6Objective,{v6OutcomeTitle} from '@/components/game/v6/V6Objective';
import './code-sentinels-v6.css';

type Tool='select'|'shell'|'room'|'excavate'|'build'|'deploy'|'power'|'compute'|'wall'|'move'|'skill'|'entrance'|'supply'|'expand'|'merge'|'shipment-route';
type Deck='construction'|'units'|'gpus'|'plugins';
type Gesture={kind:'pan'|'rect'|'select'|'line';start:{x:number;y:number};end:{x:number;y:number};cell:V6Point;camera:V6Camera;shift:boolean};
const DEFAULT_SIZE={width:1280,height:720};
const TERRAIN=['#47544c','#4b5055','#34516b','#82765d','#76816d','#b99561','#605253'];
const ENTITY_NAMES:Record<string,string>={core:'指挥核心',shell:'毛坯楼体',ore:'矿脉',coal:'煤层',node:'战略节点',physical:'实体墙',cuda:'CUDA 数据墙',moat:'CUDA 护城河',door:'通道门',stairs:'楼梯',elevator:'货梯',ramp:'车辆坡道',power:'电力线路',compute:'算力线路'};
const CARGO_NAMES:Record<string,string>={ore:'矿物',credits:'矿物',ammo:'弹药',fuel:'燃料',repair:'维修物资',energy:'蓄能'};
const TOOL_LABELS:Record<Tool,string>={select:'选择',shell:'毛坯施工',room:'房间装修',excavate:'挖掘地下室',build:'露天设施',deploy:'部署单位',power:'铺设电力线',compute:'铺设算力线',wall:'建设防御墙',move:'下达移动',skill:'技能瞄准',entrance:'设置跨层入口',supply:'调度实体补给',expand:'扩建毛坯',merge:'合并相邻房间','shipment-route':'编辑运输航点'};
const straightPath=orthogonalPathV6;
function wirePath(a:V6Point,b:V6Point,s:V6Snapshot,owner:number):V6Point[]|null{
  return wireRouteV6(a,b,s.entrances,owner);
}

function MiniMap({state,owner,camera,onMove}:{state:V6Snapshot;owner:number;camera:V6Camera;onMove:(p:V6Point)=>void}){
  const ref=useRef<HTMLCanvasElement>(null);
  useEffect(()=>{
    const c=ref.current?.getContext('2d');if(!c)return;c.clearRect(0,0,256,192);
    const explored=new Set((state.explored[owner-1]??[]).filter(p=>p.z===0).map(p=>p.y*128+p.x));
    state.terrain.forEach((terrain,i)=>{c.fillStyle=explored.has(i)?TERRAIN[terrain]??TERRAIN[0]:'#141c21';c.fillRect(i%128*2,Math.floor(i/128)*2,2,2);});
    for(const node of state.resources.filter(n=>n.kind==='node')){c.fillStyle=node.owner===owner?'#85dfbf':node.owner?'#ef9e8a':'#e8c986';c.beginPath();c.arc(node.pos.x*2,node.pos.y*2,3,0,Math.PI*2);c.fill();}
    for(const b of state.buildings){c.fillStyle=b.owner===owner?'#b4e6d2':'#de8f7d';c.fillRect(b.rect.x*2,b.rect.y*2,Math.max(3,b.rect.w*2),Math.max(3,b.rect.h*2));}
    for(const unit of state.units){c.fillStyle=unit.owner===owner?'#e6f6de':'#fa8d7c';c.fillRect(unit.x*2-1,unit.y*2-1,2,2);}
    c.strokeStyle='#efe7c7';c.lineWidth=1;c.strokeRect(camera.x*2-13,camera.y*2-9,26,18);
  },[state,owner,camera]);
  return <canvas ref={ref} width={256} height={192} role="img" aria-label="战术地图，点击定位" onClick={e=>{const rect=e.currentTarget.getBoundingClientRect();onMove({x:(e.clientX-rect.left)/rect.width*128,y:(e.clientY-rect.top)/rect.height*96,z:camera.layer});}}/>;
}

export default function CodeSentinelsV6(){
  const r=useSentinelsV6(),s=r.snapshot;
  useEffect(()=>{const previous=document.title;document.title='编译防线 · 纵深前线 V6';return()=>{document.title=previous;};},[]);
  const[lobby,setLobby]=useState(true),[deck,setDeck]=useState<Deck>('construction'),[deckOpen,setDeckOpen]=useState(true),[tool,setTool]=useState<Tool>('select');
  const[card,setCard]=useState<V6CatalogItem|null>(null),[selected,setSelected]=useState<V6Selection[]>([]),[camera,setCamera]=useState<V6Camera>(V6_INITIAL_CAMERA),[size,setSize]=useState(DEFAULT_SIZE);
  const[hover,setHover]=useState<V6Point|null>(null),[gesture,setGesture]=useState<Gesture|null>(null),[wireStart,setWireStartState]=useState<V6Point|null>(null),[branch,setBranch]=useState('speed');
  const wireStartUnit=useRef<number|null>(null);
  const setWireStart=useCallback((point:V6Point|null,unitId:number|null=null)=>{wireStartUnit.current=unitId;setWireStartState(point);},[]);
  const[panel,setPanel]=useState<'tech'|'resources'|'logistics'|'help'|null>(null),[wallKind,setWallKind]=useState('physical'),[entranceKind,setEntranceKind]=useState('door'),[toLevel,setToLevel]=useState(1),[overlay,setOverlay]=useState<'none'|'power'|'compute'>('none');
  const[tutorial,setTutorial]=useState(true),[groups,setGroups]=useState<Record<string,number[]>>({}),[supplySource,setSupplySource]=useState<number|null>(null),[cargo,setCargo]=useState('ammo'),[supplyAmount,setSupplyAmount]=useState(20);
  const[saveNotice,setSaveNotice]=useState(''),[selectedRoomKind,setSelectedRoomKind]=useState('data-center'),[splitAxis,setSplitAxis]=useState<'x'|'y'>('x'),[splitOffset,setSplitOffset]=useState(2);
  const[preview,setPreview]=useState<V6Preview|null>(null),[replayPaused,setReplayPaused]=useState(false),[replaySpeed,setReplaySpeed]=useState(1);
  const[deckBranch,setDeckBranch]=useState('all'),[roleFilter,setRoleFilter]=useState('all');
  const[seekDraft,setSeekDraft]=useState<number|null>(null);
  const[entranceWidth,setEntranceWidth]=useState(1);
  const entranceTarget=toLevel===camera.layer?(camera.layer>0?camera.layer-1:Math.min(5,camera.layer+1)):toLevel;
  const[routeShipmentId,setRouteShipmentId]=useState<number|null>(null),[routePoints,setRoutePoints]=useState<V6Point[]>([]),[routeBusy,setRouteBusy]=useState(false);
  const[transportMode,setTransportMode]=useState<'ground'|'air'>('ground');
  const[wireBusy,setWireBusy]=useState(false),toolEpoch=useRef(0);
  const pickSequence=useRef(0),latestCamera=useRef('');latestCamera.current=JSON.stringify(camera);
  const stage=useRef<HTMLDivElement>(null),board=useRef<HTMLDivElement>(null),gestureRef=useRef<Gesture|null>(null),cameraTimer=useRef(0),sessionCenter=useRef('');
  const player=s?.players.find(p=>p.owner===r.playerId),ownRooms=s?.rooms.filter(q=>q.owner===r.playerId)??[],ownUnits=s?.units.filter(u=>u.owner===r.playerId)??[];
  const selection=selected[0],unit=selection?.kind==='unit'?s?.units.find(u=>u.id===selection.id):null,room=selection?.kind==='room'?s?.rooms.find(q=>q.id===selection.id):null,building=selection?.kind==='building'?s?.buildings.find(b=>b.id===selection.id):null;
  const selectedEntity=room??unit??building??(selection?.kind==='resource'?s?.resources.find(n=>n.id===selection.id):selection?.kind==='wall'?s?.walls.find(n=>n.id===selection.id):selection?.kind==='entrance'?s?.entrances.find(n=>n.id===selection.id):selection?.kind==='shipment'?s?.shipments.find(n=>n.id===selection.id):selection?.kind==='link'?s?.links.find(n=>n.id===selection.id):selection?.kind==='rubble'?s?.rubble?.find(n=>n.id===selection.id):null);
  const selectedHealth=selectedEntity&&'hp'in selectedEntity&&'maxHp'in selectedEntity&&Math.abs(selectedEntity.hp-Number(selectedEntity.maxHp))<1e-7?Number(selectedEntity.maxHp):selectedEntity&&'hp'in selectedEntity?selectedEntity.hp:0;
  const facilityStock=(room??building)?.stock;
  const workshopStock=room?.kind==='ammunition-workshop'&&room.owner===r.playerId&&facilityStock!==undefined;
  const visibleStock=workshopStock?{ammo:0,fuel:0,repair:0,...facilityStock}:facilityStock;
  const shipment=selection?.kind==='shipment'?s?.shipments.find(n=>n.id===selection.id):null;
  const resource=selection?.kind==='resource'?s?.resources.find(n=>n.id===selection.id):null;
  const entrance=selection?.kind==='entrance'?s?.entrances.find(n=>n.id===selection.id):null;
  const aiEconomy=v6AiEconomy(r.catalog,s?.units??[],r.playerId);
  const selectedPosition=room||building?v6EntityPosition((room??building)!):unit?.pos??(selection?.kind==='wall'?s?.walls.find(w=>w.id===selection.id)?.pos:null);
  const selectedShield=selectedPosition?s?.shieldRegions?.find(region=>region.owner===r.playerId&&region.cells.some(p=>p.z===selectedPosition.z&&Math.abs(p.x-Math.floor(selectedPosition.x))+Math.abs(p.y-Math.floor(selectedPosition.y))<=(selection?.kind==='wall'?1:0))):undefined;
  const entityCard=r.catalog.items.find(c=>c.id===(selectedEntity&&'kind'in selectedEntity?selectedEntity.kind:''));
  const selectedName=entityCard?.name??(selection?.kind==='shipment'?'补给运输':selection?.kind==='rubble'?'可回收残骸':selectedEntity&&'kind'in selectedEntity?ENTITY_NAMES[selectedEntity.kind]??selectedEntity.kind:'资源节点');
  const selectedUnitIds=selected.filter(q=>q.kind==='unit').map(q=>q.id);
  const selectedUnits=selectedUnitIds.filter(id=>ownUnits.some(u=>u.id===id));
  const rectangle=gesture&&['shell','room','excavate','expand'].includes(tool)?dragRectV6(gesture.cell,screenToWorldV6(gesture.end,camera,size)):null;
  const ownShells=s?.buildings.filter(b=>b.kind==='shell'&&b.owner===r.playerId)??[];
  const initialLab=ownRooms.some(q=>q.kind==='research-lab'&&q.progress>=1&&q.hp>0);
  const tutorialSteps:[string,boolean][]=[
    ['B 拖出毛坯，等待施工',ownShells.some(b=>b.progress>=1)],
    ['通道工具 → 门 → 点击外墙',s?.entrances.some(e=>e.owner===r.playerId&&e.kind==='door')??false],
    ['划分机房，风机 → L 接电',ownRooms.some(q=>q.kind==='data-center'&&q.powered)],
    ['选中机房，I 安装显卡',ownRooms.some(q=>q.gpus.length>0)],
    ['研究所接入电力与算力',ownRooms.some(q=>q.kind==='research-lab'&&q.powered&&q.connected)],
    ['部署防御，护送补给',ownUnits.some(q=>!['builder','transport'].includes(q.kind))],
  ];
  const items=useMemo(()=>{
    const list=r.catalog.items.filter(c=>(deck==='construction'?c.category==='rooms'||c.category==='buildings':c.category===deck)
      &&(deckBranch==='all'||!c.branch||c.branch===deckBranch)&&(deck!=='units'||roleFilter==='all'||c.role===roleFilter));
    if(deck==='construction'){const priority=['wind-power','extractor','data-center','research-lab','depot','factory','mobile-relay'];const rank=(id:string)=>{const n=priority.indexOf(id);return n<0?999:n;};list.sort((a,b)=>rank(a.id)-rank(b.id));}
    return list.map(item=>item.role==='ai'&&aiEconomy.ready?{...item,cost:item.cost*aiEconomy.multiplier}:item.category==='plugins'&&unit?.owner===r.playerId?{...item,cost:item.cost*(1-Math.max(0,Math.min(.1,unit.pluginDiscount??0)))}:item);
  },[deck,r.catalog,deckBranch,roleFilter,aiEconomy.multiplier,aiEconomy.ready,unit?.owner,unit?.pluginDiscount,r.playerId]);
  const send=r.order;
  const logisticsRules=(r.catalog.rules?.logistics??{}) as Record<string,number>;
  const logisticsQuote=(name:string)=>Number.isFinite(logisticsRules[name])?rateV6(logisticsRules[name]):'待同步';
  const focus=useCallback((point:V6Point)=>setCamera(old=>clampCameraV6({...old,x:point.x,y:point.y,layer:point.z})),[]);
  function cancel(){toolEpoch.current++;setTool('select');setWireStart(null);setWireBusy(false);setRoutePoints([]);setRouteShipmentId(null);setRouteBusy(false);setGesture(null);gestureRef.current=null;setHover(null);r.setNotice('已取消当前工具');}
  async function commitShipmentRoute(){
    if(routeShipmentId===null||routeBusy||!s?.shipments.some(q=>q.id===routeShipmentId&&q.owner===r.playerId)){r.setNotice('当前运输已结束，或没有可修改的己方运输。');return;}
    const epoch=toolEpoch.current;setRouteBusy(true);
    const receipt=await send({op:'reroute-shipment',id:routeShipmentId,waypoints:routePoints});
    if(epoch!==toolEpoch.current)return;setRouteBusy(false);
    if(receipt?.accepted){toolEpoch.current++;setTool('select');setRoutePoints([]);setRouteShipmentId(null);}
  }
  function activate(next:Tool){
    if(next==='power'||next==='compute')setOverlay(next);
    if(next==='skill'){
      if(!unit||unit.owner!==r.playerId){r.setNotice('先选中一名己方作战单位。');return;}
      if(entityCard?.skillShape==='self'){void send({op:'skill',id:unit.id,pos:unit.pos});return;}
    }
    toolEpoch.current++;setTool(next);setWireStart(null);setWireBusy(false);setGesture(null);gestureRef.current=null;r.setNotice(next==='power'?'电力线：先点发电端，再点用电端；Shift 可连续拐弯布线，跨层需要竖井。':next==='compute'?'算力线：先点运行机房，再点研究所、炮台或基站；Shift 连续布线。':next==='shell'?'拖出4–24格长宽的矩形地基。':next==='room'?'在已建好的毛坯楼内拖出房间，或点击整层装修。':TOOL_LABELS[next]);
  }
  function selectCard(item:V6CatalogItem){
    setCard(item);const lock=v6CardLock(item,player);if(lock){toolEpoch.current++;setTool('select');setWireStart(null);r.setNotice(`${item.name}：需要${lock}；可以在科技面板查看发展条件。`);return;}
    if(item.category==='units'&&!initialLab){r.setNotice('先完成初始研究所施工，再部署基础炮台或研究后续单位。');return;}
    if(item.category==='gpus'){if(!room||room.kind!=='data-center'){setDeck('gpus');r.setNotice('先点击一个数据中心房间，再选择显卡安装。');return;}void send({op:'install-gpu',room:room.id,model:item.id});}
    else if(item.category==='plugins'){const reason=v6PluginLock(item,unit,r.catalog.items,r.playerId);if(reason||!unit){r.setNotice(reason);return;}void send({op:'plugin',id:unit.id,plugin:item.id});}
    else if(item.category==='rooms'){setSelectedRoomKind(item.id);activate('room');}
    else activate(item.category==='units'?'deploy':'build');
  }
  function producer(item:V6CatalogItem):V6Room|undefined{
    const role=item.role??'';
    const valid=(q:V6Room)=>q.owner===r.playerId&&q.progress>=1&&q.hp>0&&(!item.producer||q.kind===item.producer)
      &&(role==='air'?q.kind==='airfield':role==='orbital'?q.kind==='orbital-control':role==='vehicle'?q.kind==='factory':q.kind==='research-lab')
      &&(!item.branch||role!=='ai'||q.branch===item.branch);
    const operational=(q:V6Room)=>q.powered&&q.online!==false&&(!['ai','orbital'].includes(role)||q.connected);
    if(room&&valid(room)&&operational(room))return room;
    return ownRooms.find(q=>valid(q)&&operational(q))??(room&&valid(room)?room:ownRooms.find(valid));
  }
  function entityAt(p:V6Point):V6Selection|null{
    if(!s)return null;
    const foundUnit=s.units.filter(u=>u.z===p.z&&Math.hypot(u.x-(p.x+.5),u.y-(p.y+.5))<1.3).sort((a,b)=>Math.hypot(a.x-p.x,a.y-p.y)-Math.hypot(b.x-p.x,b.y-p.y))[0];if(foundUnit)return{kind:'unit',id:foundUnit.id};
    const q=s.rooms.find(q=>containsV6(q.rect,p));if(q)return{kind:'room',id:q.id};
    const b=s.buildings.find(b=>containsV6(b.rect,p));if(b)return{kind:'building',id:b.id};
    const w=s.walls.find(w=>w.pos.x===p.x&&w.pos.y===p.y&&w.pos.z===p.z);if(w)return{kind:'wall',id:w.id};
    const e=s.entrances.find(e=>e.pos.x===p.x&&e.pos.y===p.y&&e.pos.z===p.z);if(e)return{kind:'entrance',id:e.id};
    const rubble=s.rubble?.find(q=>containsV6(q.rect,p));if(rubble)return{kind:'rubble',id:rubble.id};
    const resource=s.resources.find(n=>n.pos.z===p.z&&Math.hypot(n.pos.x-p.x,n.pos.y-p.y)<2);return resource?{kind:'resource',id:resource.id}:null;
  }
  const wirePorts=(tool==='power'||tool==='compute')&&s?[
    ...[...ownRooms,...s.buildings.filter(b=>b.owner===r.playerId&&b.kind!=='shell')].filter(e=>e.rect.z===camera.layer&&e.hp>0).map(e=>({selection:{kind:'shell'in e?'room':'building',id:e.id} as V6Selection,pos:{x:e.rect.x+Math.floor(e.rect.w/2),y:e.rect.y+Math.floor(e.rect.h/2),z:e.rect.z},online:tool==='power'?e.powered:e.connected,elevator:false})),
    ...(tool==='power'?s.entrances.filter(e=>e.owner===r.playerId&&e.kind==='elevator'&&e.hp>0&&camera.layer>=Math.min(e.pos.z,e.toLevel)&&camera.layer<=Math.max(e.pos.z,e.toLevel)).map(e=>({selection:{kind:'entrance',id:e.id} as V6Selection,pos:{...e.pos,z:camera.layer},online:!!e.powered,elevator:true})):[])
  ].map(p=>({...p,screen:worldToScreenV6({x:p.pos.x+.5,y:p.pos.y+.5,z:p.pos.z},camera,size)})).filter(p=>p.screen.x>=0&&p.screen.y>=0&&p.screen.x<=size.width&&p.screen.y<=size.height):[];
  function endpoint(p:V6Point,picked?:V6Selection|null,pickedPos?:V6Point){const hit=picked===undefined?entityAt(p):picked;if(!s||!hit)return p;const entity=hit.kind==='unit'?s.units.find(u=>u.id===hit.id):hit.kind==='room'?s.rooms.find(q=>q.id===hit.id):s.buildings.find(b=>b.id===hit.id);if(hit.kind==='building'&&entity&&'kind'in entity&&entity.kind==='shell')return p;const value=entity?v6EntityPosition(entity):pickedPos??p;return{x:Math.floor(value.x),y:Math.floor(value.y),z:value.z};}
  function perform(p:V6Point,shift=false,picked?:V6Selection|null,pickedPos?:V6Point){
    if(!s)return;
    if(tool==='shipment-route'){
      if(routeBusy)return;
      if(routePoints.length>=16){r.setNotice('每条运输路线最多16个航点，按Enter提交或撤销上一个点。');return;}
      setRoutePoints(old=>old.length&&old[old.length-1].x===p.x&&old[old.length-1].y===p.y&&old[old.length-1].z===p.z?old:[...old,p]);return;
    }
    if(tool==='select'){const hit=picked===undefined?entityAt(p):picked;setSelected(old=>hit?shift?[...old.filter(q=>q.kind!==hit.kind||q.id!==hit.id),hit]:[hit]:[]);if(hit?.kind==='room'&&s.rooms.find(q=>q.id===hit.id)?.kind==='data-center'){setDeck('gpus');setDeckOpen(true);}return;}
    if(tool==='move'){if(!selectedUnits.length){r.setNotice('先选中可移动的单位。');return;}void send({op:shift?'queue-move':'move',ids:selectedUnits,pos:p});}
    else if(tool==='skill'){if(!unit){r.setNotice('先选中一名有主动能力的角色。');return;}void send({op:'skill',id:unit.id,pos:p,direction:p});}
    else if(tool==='build'&&card)void send({op:'build',pos:p,kind:card.id});
    else if(tool==='deploy'&&card){const from=producer(card);if(!from){r.setNotice('尚无可生产此单位的功能房间。');return;}void send({op:'deploy',room:from.id,kind:card.id,pos:p});}
    else if(tool==='entrance')void send({op:'entrance',pos:p,toLevel:entranceKind==='door'?p.z:entranceTarget,kind:entranceKind,width:entranceKind==='ramp'?Math.max(3,entranceWidth):entranceWidth});
    else if(tool==='merge'){const next=picked?s.rooms.find(q=>q.id===picked.id):s.rooms.find(q=>containsV6(q.rect,p));if(!room||!next){r.setNotice('先选择一个房间，再点击相邻的同用途房间。');return;}void send({op:'merge-rooms',ids:[room.id,next.id]});}
    else if(tool==='power'||tool==='compute'){
      if(wireBusy){r.setNotice('正在确认线路，请稍候。');return;}
      const targetUnit=tool==='compute'&&picked?.kind==='unit'?s.units.find(u=>u.id===picked.id&&u.owner===r.playerId):null;
      const endUnit=targetUnit&&r.catalog.items.some(d=>d.category==='units'&&d.id===targetUnit.kind&&d.role==='ai')?targetUnit.id:null;
      const end=endpoint(p,picked,pickedPos);if(!wireStart){setWireStart(end,endUnit);r.setNotice('起点已选，点击目标端口完成连接。');return;}
      const path=wirePath(wireStart,end,s,r.playerId);if(!path){r.setNotice('这两个楼层之间没有可用竖井，请先建楼梯、货梯或坡道。');return;}
      const currentTool=toolEpoch.current;setWireBusy(true);
      const unitEndpoints=tool==='compute'?[...new Set([wireStartUnit.current,endUnit].filter((id):id is number=>id!==null))]:[];
      void send({op:'wire',kind:tool==='power'?'power':'compute',path,...(unitEndpoints.length?{unitEndpoints}:{})}).then(result=>{if(currentTool!==toolEpoch.current)return;setWireBusy(false);if(result?.accepted)setWireStart(shift?end:null,shift?endUnit:null);});return;
    }else if(tool==='supply'){
      const target=picked===undefined?entityAt(p):picked;if(!target){r.setNotice('点击需要补给的单位或设施。');return;}
      if(transportMode==='air'&&!s.buildings.some(b=>b.id===target.id&&b.owner===r.playerId&&b.kind==='airstrip'&&b.powered&&b.progress>=1)){r.setNotice('空运两端都需要已建成、供电的己方机场跑道；到站后可由地面运输分拨。');return;}
      if(supplySource===null){const stash=s.rooms.find(q=>q.id===target.id)??s.buildings.find(b=>b.id===target.id);if(!stash||stash.owner!==r.playerId){r.setNotice('先点击己方仓库、核心或有库存的生产设施。');return;}setSupplySource(target.id);r.setNotice('货源已选，点击补给目的地。');return;}
      const destination=s.units.find(u=>u.id===target.id),source=s.rooms.find(q=>q.id===supplySource)??s.buildings.find(b=>b.id===supplySource);
      const reserved=s.shipments.filter(q=>q.to===target.id&&q.cargo===cargo).reduce((sum,q)=>sum+q.amount,0);
      const space=destination?Math.max(0,(cargo==='ammo'?(destination.ammoMax??0)-destination.ammo:cargo==='fuel'?(destination.fuelMax??0)-(destination.fuel??0):cargo==='repair'?(destination.maxHp-destination.hp)/4:0)-reserved):supplyAmount;
      const available=source?.stock?.[cargo==='credits'?'ore':cargo]??0;
      const amount=Math.min(supplyAmount,space,available);
      if(amount<=0){r.setNotice('发货库存不足，或目标已满载、已有在途补给。');return;}
      void send({op:'supply',from:supplySource,to:target.id,amount,cargo,mode:transportMode});
    }
    if(!shift)setTool('select');
  }
  async function clickScene(p:V6Point|null,at:{x:number;y:number},shift=false,right=false){
    if(!right&&(tool==='power'||tool==='compute')){
      const port=wirePorts.map(port=>({port,distance:Math.hypot(port.screen.x-at.x,port.screen.y-at.y)})).filter(p=>p.distance<=12).sort((a,b)=>a.distance-b.distance||a.port.selection.id-b.port.selection.id)[0]?.port;
      if(port){perform(port.pos,shift,port.selection,port.pos);return;}
    }
    const currentTool=toolEpoch.current,currentCamera=latestCamera.current,request=++pickSequence.current;
    try{
      const picked=await v6Request<V6Pick|null>('pick',{screenX:at.x,screenY:at.y,width:size.width,height:size.height,sessionId:r.session?.roomId,view:{centerX:camera.x,centerY:camera.y,zoom:camera.zoom,layer:camera.layer,cutaway:camera.cutaway,localPlayer:r.playerId}});
      if(currentTool!==toolEpoch.current||currentCamera!==latestCamera.current||(!shift&&request!==pickSequence.current))return;
      const hit:V6Selection|null=picked&&picked.id>0&&picked.kind!=='terrain'?{kind:picked.kind,id:picked.id}:null;
      const point=p??picked?.pos;if(!point){if(tool==='select')setSelected([]);return;}
      if(right){if(selectedUnits.length)void send(hit&&picked!.owner>0&&picked!.owner!==r.playerId?{op:'attack',ids:selectedUnits,target:hit.id}:{op:shift?'queue-move':'move',ids:selectedUnits,pos:point});else cancel();}
      else perform(point,shift,hit,picked?.pos);
    }catch(e){if(currentTool===toolEpoch.current)r.setNotice(`未能确认点击对象：${(e as Error).message}`);}
  }
  function point(event:{clientX:number;clientY:number}){const rect=stage.current!.getBoundingClientRect();return{x:event.clientX-rect.left,y:event.clientY-rect.top};}
  function pointerDown(e:ReactPointerEvent<HTMLDivElement>){
    if(lobby||!s||!r.streaming||!r.connected)return;if(e.button===2){e.preventDefault();const at=point(e),p=cellAtV6(screenToWorldV6(at,camera,size));void clickScene(p,at,e.shiftKey,true);return;}
    const at=point(e),raw=screenToWorldV6(at,camera,size),cell=cellAtV6(raw),kind=e.button===1||e.altKey?'pan':['shell','room','excavate','expand'].includes(tool)?'rect':tool==='wall'?'line':'select';
    if(!cell&&(kind==='rect'||kind==='line'))return;
    const g:Gesture={kind,start:at,end:at,cell:cell??raw,camera,shift:e.shiftKey};
    gestureRef.current=g;setGesture(g);e.currentTarget.setPointerCapture(e.pointerId);
  }
  function pointerMove(e:ReactPointerEvent<HTMLDivElement>){const at=point(e);setHover(cellAtV6(screenToWorldV6(at,camera,size)));const g=gestureRef.current;if(!g)return;
    if(g.kind==='pan'){const a=screenToWorldV6(g.start,g.camera,size),b=screenToWorldV6(at,g.camera,size);setCamera(clampCameraV6({...g.camera,x:g.camera.x+a.x-b.x,y:g.camera.y+a.y-b.y}));}
    else{const next={...g,end:at};gestureRef.current=next;setGesture(next);}
  }
  function pointerUp(e:ReactPointerEvent<HTMLDivElement>){const g=gestureRef.current;if(!g)return;gestureRef.current=null;setGesture(null);if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);if(g.kind==='pan')return;
    const at=point(e),cell=cellAtV6(screenToWorldV6(at,camera,size));if(!s)return;const distance=Math.hypot(at.x-g.start.x,at.y-g.start.y);
    if(g.kind==='rect'){
      if(!cell)return;
      let rect=dragRectV6(g.cell,cell);
      if(tool==='shell'){if(!validShellRectV6(rect)){r.setNotice('毛坯长宽都需4–24格，且不能越过地图边界。');return;}void send({op:'shell',rect});}
      else if(tool==='expand'){if(!building||building.kind!=='shell'){r.setNotice('先选择要扩建的毛坯楼。');return;}void send({op:'expand-shell',id:building.id,rect});}
      else if(tool==='excavate')void send({op:'excavate',rect});
      else{const shell=building?.kind==='shell'?building:ownShells.find(b=>containsV6(b.rect,g.cell));if(!shell){r.setNotice('请在已建好的己方毛坯楼内划分房间。');return;}if(distance<5)rect={...shell.rect};void send({op:'room',shell:shell.id,rect,kind:selectedRoomKind,...(selectedRoomKind==='research-lab'?{branch}:{})});}
      if(!g.shift)setTool('select');return;
    }
    if(g.kind==='line'){if(cell)void send({op:'wall',kind:wallKind,path:straightPath(g.cell,cell)});return;}
    if(tool==='select'&&distance>6){const ids=ownUnits.filter(u=>{const p=worldToScreenV6({x:u.x,y:u.y,z:v6UnitElevation(u)},camera,size);return u.z===camera.layer&&p.x>=Math.min(g.start.x,at.x)&&p.x<=Math.max(g.start.x,at.x)&&p.y>=Math.min(g.start.y,at.y)&&p.y<=Math.max(g.start.y,at.y);}).map(u=>({kind:'unit' as const,id:u.id}));setSelected(g.shift?[...selected,...ids.filter(i=>!selected.some(q=>q.kind===i.kind&&q.id===i.id))]:ids);return;}
    if(['select','power','compute','merge','supply'].includes(tool))void clickScene(cell,at,g.shift);else if(cell)perform(cell,g.shift);
  }
  useEffect(()=>{const element=board.current;if(!element)return;const observer=new ResizeObserver(entries=>{const {width,height}=entries[0].contentRect;const w=Math.min(width,height*16/9);setSize({width:w,height:w*9/16});});observer.observe(element);return()=>observer.disconnect();},[lobby]);
  useEffect(()=>{if(!r.session||r.session.status==='lobby')return;window.clearTimeout(cameraTimer.current);cameraTimer.current=window.setTimeout(()=>void r.camera(camera),50);return()=>window.clearTimeout(cameraTimer.current);},[camera,r.session?.roomId,r.session?.status,r.playerId]);
  useEffect(()=>{if(!s||!r.session||sessionCenter.current===r.session.roomId)return;const core=s.buildings.find(b=>b.owner===r.playerId&&b.kind==='core');if(core){sessionCenter.current=r.session.roomId;focus(v6EntityPosition(core));}},[s,r.session,r.playerId,focus]);
  useEffect(()=>{if(r.session?.status==='battle'&&r.session.mode==='join')setLobby(false);},[r.session?.status,r.session?.mode]);
  useEffect(()=>{toolEpoch.current++;setSelected([]);setGroups({});setWireStart(null);setTool('select');setPreview(null);setSupplySource(null);},[r.session?.roomId]);
  useEffect(()=>{if(tool==='shipment-route'&&routeShipmentId!==null&&s&&!s.shipments.some(q=>q.id===routeShipmentId&&q.owner===r.playerId)){toolEpoch.current++;setTool('select');setRouteShipmentId(null);setRoutePoints([]);setRouteBusy(false);r.setNotice('这批运输已交付或被摧毁，路线编辑结束。');}},[s,tool,routeShipmentId]);
  useEffect(()=>{if(!saveNotice)return;const timer=window.setTimeout(()=>setSaveNotice(''),3500);return()=>window.clearTimeout(timer);},[saveNotice]);
  useEffect(()=>{const key=(e:KeyboardEvent)=>{
    if(lobby||(e.target instanceof HTMLElement&&(['INPUT','TEXTAREA','SELECT'].includes(e.target.tagName)||e.target.isContentEditable)))return;
    const k=e.key.toLowerCase();if(panel&&k!=='escape')return;if(k==='escape'){e.preventDefault();if(panel)setPanel(null);else cancel();}
    else if(tool==='shipment-route'&&(k==='enter'||k==='backspace')){e.preventDefault();if(k==='enter')void commitShipmentRoute();else if(!routeBusy)setRoutePoints(points=>points.slice(0,-1));}
    else if(k==='tab'){e.preventDefault();setCamera(c=>({...c,cutaway:!c.cutaway}));}
    else if(k==='pageup'||k==='pagedown'){e.preventDefault();setCamera(c=>clampCameraV6({...c,layer:c.layer+(k==='pageup'?1:-1)}));}
    else if(k==='home'){e.preventDefault();const core=s?.buildings.find(b=>b.owner===r.playerId&&b.kind==='core');if(core)focus(v6EntityPosition(core));}
    else if(/^[0-9]$/.test(k)){if(e.ctrlKey){e.preventDefault();setGroups(old=>({...old,[k]:selectedUnits}));r.setNotice(`编队${k}：${selectedUnits.length}个单位`);}else if(groups[k])setSelected(groups[k].map(id=>({kind:'unit',id})));else if(Number(k)>0&&items[Number(k)-1])selectCard(items[Number(k)-1]);}
    else if(k==='b'){setDeck('construction');setDeckOpen(true);activate('shell');}
    else if(k==='u'){setDeck('units');setDeckOpen(true);}else if(k==='i'){setDeck('gpus');setDeckOpen(true);}
    else if(k==='l')activate('power');else if(k==='c')activate('compute');else if(k==='w')activate('wall');else if(k==='a')activate('move');else if(k==='q')activate('skill');
    else if(k==='s'&&selectedUnits.length)void send({op:'stop',ids:selectedUnits});
    else if(k==='g')setOverlay(old=>old==='none'?'power':old==='power'?'compute':'none');
    else if(k==='h')setPanel(panel==='help'?null:'help');
    else if(k===' '){e.preventDefault();if(r.session?.replay)void r.action('replay-control',{paused:!(s?.playback?.paused??false)});else if(r.session?.mode==='solo')void r.action('pause',{paused:!r.session.paused,sessionId:r.session.roomId});else r.setNotice('双人对战持续进行，不能单方面暂停。');}
  };window.addEventListener('keydown',key);return()=>window.removeEventListener('keydown',key);});

  const selectedRect=room?.rect??building?.rect;
  const ghost=rectangle??(hover&&['build','deploy','entrance','shell'].includes(tool)?{...hover,w:tool==='shell'?4:card?.width??1,h:tool==='shell'?4:card?.height??1}:null);
  const polygon=(rect:V6Rect)=>rectOutlineV6(rect,camera,size).map(p=>`${p.x},${p.y}`).join(' ');
  const activeLine=wireStart&&hover&&s?wirePath(wireStart,endpoint(hover),s,r.playerId):gesture?.kind==='line'&&hover?straightPath(gesture.cell,hover):null;
  const unitScreen=unit?worldToScreenV6({x:unit.x,y:unit.y,z:v6UnitElevation(unit)},camera,size):null;
  const core=s?.buildings.find(b=>b.kind==='core'&&b.owner===r.playerId);
  const buildingsById=new Map(s?.buildings.map(b=>[b.id,b])??[]);
  const onlineRooms=ownRooms.filter(q=>q.powered&&q.connected);
  let candidate:V6Command|null=null;
  if((tool==='power'||tool==='compute')&&activeLine)candidate={op:'wire',kind:tool==='power'?'power':'compute',path:activeLine};
  else if(tool==='shell'&&ghost&&validShellRectV6(ghost))candidate={op:'shell',rect:ghost};
  else if(tool==='build'&&hover&&card)candidate={op:'build',pos:hover,kind:card.id};
  else if(tool==='deploy'&&hover&&card){const from=producer(card);if(from)candidate={op:'deploy',room:from.id,kind:card.id,pos:hover};}
  else if(tool==='room'&&rectangle){const shell=building?.kind==='shell'?building:ownShells.find(b=>containsV6(b.rect,rectangle));if(shell)candidate={op:'room',shell:shell.id,rect:rectangle,kind:selectedRoomKind,...(selectedRoomKind==='research-lab'?{branch}:{})};}
  const previewKey=candidate?JSON.stringify(candidate):'';
  useEffect(()=>{if(!previewKey||!r.connected||lobby){setPreview(null);return;}const controller=new AbortController();const timer=window.setTimeout(()=>{void v6Request<V6Preview>('preview',{command:JSON.parse(previewKey),sessionId:r.session?.roomId},controller.signal).then(setPreview).catch(()=>{if(!controller.signal.aborted)setPreview(null);});},150);return()=>{controller.abort();window.clearTimeout(timer);};},[previewKey,r.connected,lobby,r.session?.roomId,Math.floor((s?.tick??0)/60),aiEconomy.count]);
  const skillTarget=hover?{x:hover.x+.5,y:hover.y+.5,z:hover.z}:null;
  const skillPoints=unit&&skillTarget?skillOutlineV6(entityCard?.skillShape??'target',{x:unit.pos.x+.5,y:unit.pos.y+.5,z:unit.z},skillTarget,entityCard?.skillRange??entityCard?.range??0,entityCard?.skillRadius??0,entityCard?.skillWidth??0,entityCard?.skillAngle??0,camera,size):[];
  const skillInRange=!!unit&&!!hover&&hover.z===unit.z&&Math.hypot(hover.x-unit.pos.x,hover.y-unit.pos.y)<=(entityCard?.skillRange??entityCard?.range??0);

  return <main className="v6-game">
    <div className="v6-board" ref={board}>
      <div className={`v6-stage tool-${tool}`} ref={stage} style={{width:size.width,height:size.height}} onContextMenu={e=>e.preventDefault()} onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={()=>{gestureRef.current=null;setGesture(null);}} onWheel={e=>setCamera(c=>clampCameraV6({...c,zoom:c.zoom*Math.exp(-e.deltaY*.001)}))}>
        <canvas className="v6-native-frame" ref={r.canvasRef} width={1280} height={720} aria-label="原生多层战场"/>
        <svg className="v6-map-overlay" width={size.width} height={size.height} aria-hidden="true">
          {overlay!=='none'&&s?.links.filter(l=>l.kind===overlay).map(l=><polyline key={l.id} points={l.path.map(p=>worldToScreenV6({x:p.x+.5,y:p.y+.5,z:p.z},camera,size)).map(p=>`${p.x},${p.y}`).join(' ')} fill="none" stroke={l.kind==='power'?'#e9c274':'#72d3ca'} strokeWidth={l.active?2.5:1} strokeDasharray={l.active?'':'5 5'} opacity={.85}/>)}
          {selectedRect&&<polygon points={polygon(selectedRect)} className="v6-selected-footprint"/>}
          {selectedUnitIds.map(id=>{const u=s?.units.find(u=>u.id===id);if(!u)return null;const p=worldToScreenV6({x:u.x,y:u.y,z:v6UnitElevation(u)},camera,size);return <ellipse key={id} cx={p.x} cy={p.y} rx={12*camera.zoom} ry={6*camera.zoom} className={`v6-selected-ring ${u.owner===r.playerId?'':'enemy'}`}/>;})}
          {ghost&&<polygon points={polygon(ghost)} className={`v6-blueprint ${tool==='shell'&&!validShellRectV6(ghost)?'invalid':''}`}/>}
          {activeLine&&<polyline points={activeLine.map(p=>worldToScreenV6({x:p.x+.5,y:p.y+.5,z:p.z},camera,size)).map(p=>`${p.x},${p.y}`).join(' ')} className="v6-line-preview"/>}
          {shipment&&<polyline points={[shipment.pos,...shipment.route].map(p=>worldToScreenV6({x:p.x+.5,y:p.y+.5,z:p.z},camera,size)).map(p=>`${p.x},${p.y}`).join(' ')} className="v6-shipment-route-current"/>}
          {unit&&unit.owner===r.playerId&&!!unit.queuedGoals?.length&&(()=>{const next=[...(unit.goal?[unit.goal]:[]),...unit.queuedGoals],points=[worldToScreenV6({x:unit.x,y:unit.y,z:v6UnitElevation(unit)},camera,size),...next.map(p=>worldToScreenV6({x:p.x+.5,y:p.y+.5,z:p.z+(unit.altitude??0)},camera,size))];return <g className="v6-unit-goals"><polyline points={points.map(p=>`${p.x},${p.y}`).join(' ')}/>{points.slice(1).map((p,i)=><g key={i} transform={`translate(${p.x},${p.y})`}><circle r={6}/><text x={9} y={3}>{i+1}</text></g>)}</g>;})()}
          {tool==='shipment-route'&&routeShipmentId!==null&&s&&(()=>{const current=s.shipments.find(q=>q.id===routeShipmentId);if(!current)return null;const points=[current.pos,...routePoints,...(hover?[hover]:[])].map(p=>worldToScreenV6({x:p.x+.5,y:p.y+.5,z:p.z},camera,size));return <g className="v6-shipment-route-draft"><polyline points={points.map(p=>`${p.x},${p.y}`).join(' ')}/>{routePoints.map((p,i)=>{const at=worldToScreenV6({x:p.x+.5,y:p.y+.5,z:p.z},camera,size);return <g key={i} transform={`translate(${at.x},${at.y})`}><circle r={9}/><text y={3}>{i+1}</text></g>;})}</g>;})()}
          {wirePorts.map(port=><g key={port.selection.kind+port.selection.id} className={`v6-map-port ${tool} ${port.online?'online':''}`} transform={`translate(${port.screen.x},${port.screen.y})`}><circle r={7}/><path d={port.elevator?'M-4,-4h8v8h-8z':'M-11,0h22 M0,-11v22'}/>{port.elevator?<text x={12} y={3}>货梯</text>:port.selection.id===selectedEntity?.id&&<text x={12} y={-9}>#{port.selection.id}</text>}</g>)}
          {tool==='skill'&&unit&&skillTarget&&unitScreen&&(()=>{const target=worldToScreenV6(skillTarget,camera,size);return <g className={`v6-skill-target shape-${entityCard?.skillShape??'target'} ${skillInRange?'':'invalid'}`}><line x1={unitScreen.x} y1={unitScreen.y} x2={target.x} y2={target.y}/><polygon points={skillPoints.map(p=>`${p.x},${p.y}`).join(' ')}/><path d={`M${target.x-12},${target.y}h24 M${target.x},${target.y-12}v24`}/></g>;})()}
          {gesture?.kind==='select'&&tool==='select'&&<rect x={Math.min(gesture.start.x,gesture.end.x)} y={Math.min(gesture.start.y,gesture.end.y)} width={Math.abs(gesture.end.x-gesture.start.x)} height={Math.abs(gesture.end.y-gesture.start.y)} className="v6-drag-select"/>}
        </svg>
        {rectangle&&<div className="v6-dimension-label" style={{left:gesture!.end.x+16,top:gesture!.end.y+12}}>{rectangle.w} × {rectangle.h}<small>{layerNameV6(rectangle.z)} · {rectangle.w*rectangle.h} 微格</small></div>}
      </div>
    </div>
    {!lobby&&<>
      <header className="v6-topbar"><button className="v6-brand" aria-label="返回行动大厅" onClick={()=>setLobby(true)}><span className="v6-brand-symbol">⌘</span><div><b>编译防线</b><small>VERTICAL FRONT / LIVE</small></div></button><div className="v6-resource-strip">
        <button title="持续收益；矿物运抵仓库后另行结算经费，AI维护另外支出" onClick={()=>setPanel('resources')}><Coins/><div><b>{formatV6(player?.credits)}</b><small>+{rateV6(player?.income)}/s · 持续收益{aiEconomy.ready&&aiEconomy.count>0&&<> / AI −{rateV6(aiEconomy.upkeep)}/s</>}</small></div></button>
        <button onClick={()=>setPanel('resources')}><Cpu/><div><b>{formatV6(player?.compute)}</b><small>+{rateV6(player?.production)}/s · 算力</small></div></button>
        <button className={(player?.power??0)<(player?.demand??0)?'shortage':''} onClick={()=>{setPanel('resources');setOverlay('power');}}><Zap/><div><b>{formatV6(player?.power)}<em> / {formatV6(player?.demand)}</em></b><small>电力 / 负载</small></div></button>
        <button onClick={()=>setPanel('tech')}><FlaskConical/><div><b>{formatV6(player?.science)}</b><small>科研数据</small></div></button>
      </div><div className="v6-top-actions"><span>{timeV6(s?.tick??0)}</span><button className="v6-icon" aria-label="保存行动" onClick={async()=>{const result=await r.action('save',{name:`行动 ${timeV6(s?.tick??0)}`});setSaveNotice(result?'行动已保存':'保存未完成');}}><Save size={18}/></button><button className="v6-icon" aria-label="指挥手册" onClick={()=>setPanel('help')}><CircleHelp size={18}/></button><button className="v6-icon" aria-label="对战房间" onClick={()=>setLobby(true)}><Radio size={18}/></button></div></header>
      {s&&<V6Objective state={s} owner={r.playerId} coreHp={core?.hp??0} catalog={r.catalog} onLocate={node=>focus(node.pos)}/>}
      {tutorial&&<aside className="v6-tutorial"><button className="v6-icon" aria-label="收起建设指引" onClick={()=>setTutorial(false)}><X size={13}/></button><span className="v6-eyebrow">BUILD YOUR NETWORK</span><ol>{tutorialSteps.map(([text,done],i)=><li key={text} className={done?'done':''}>{done?<Check size={12}/>:<b>{i+1}</b>}<span>{text}</span></li>)}</ol></aside>}
      <aside className="v6-tool-rail">{([['select',MousePointer2,'选择','Esc'],['move',Move,'移动','A'],['power',Zap,'电力线','L'],['compute',Link2,'算力线','C'],['wall',Shield,'防御墙','W'],['excavate',Shovel,'挖掘',''],['entrance',Layers3,'跨层通道',''],['supply',Truck,'补给','']] as const).map(([id,Icon,name,key])=><button key={id} className={tool===id?'active':''} aria-label={name} title={name+(key?` ${key}`:'')} onClick={()=>activate(id)}><Icon size={21}/>{key&&<kbd>{key}</kbd>}</button>)}<button aria-label="科技树" onClick={()=>setPanel('tech')}><FlaskConical size={20}/></button><button aria-label="后勤调度" onClick={()=>setPanel('logistics')}><Boxes size={20}/></button></aside>
      <aside className="v6-floor-controls"><button aria-label="上一楼层" onClick={()=>setCamera(c=>clampCameraV6({...c,layer:c.layer+1}))}><ChevronUp size={16}/></button>{[5,4,3,2,1,0,-1,-2].map(z=><button key={z} aria-label={`切换${layerNameV6(z)}`} className={camera.layer===z?'active':''} onClick={()=>setCamera(c=>({...c,layer:z}))}>{layerNameV6(z)}</button>)}<button aria-label="下一楼层" onClick={()=>setCamera(c=>clampCameraV6({...c,layer:c.layer-1}))}><ChevronDown size={16}/></button><button aria-label="切换剖视" className={camera.cutaway?'active':''} onClick={()=>setCamera(c=>({...c,cutaway:!c.cutaway}))}><Layers3 size={17}/></button></aside>
      {s&&<aside className="v6-minimap"><header><span>TACTICAL MAP</span><button aria-label="回到基地" onClick={()=>core&&focus(v6EntityPosition(core))}><Home size={13}/></button></header><MiniMap state={s} owner={r.playerId} camera={camera} onMove={focus}/><footer><button onClick={()=>setOverlay(old=>old==='none'?'power':old==='power'?'compute':'none')}>覆盖 / {overlay==='power'?'电力':overlay==='compute'?'算力':'关闭'}</button><div><button aria-label="缩小" onClick={()=>setCamera(c=>clampCameraV6({...c,zoom:c.zoom/1.2}))}>−</button><span>{camera.zoom.toFixed(1)}×</span><button aria-label="放大" onClick={()=>setCamera(c=>clampCameraV6({...c,zoom:c.zoom*1.2}))}>+</button></div></footer></aside>}
      {selectedEntity&&<aside className="v6-inspector"><header><div><small>{selection?.kind.toUpperCase()} / #{selectedEntity.id}</small><h2>{selectedName}</h2></div><button className="v6-icon" aria-label="取消选择" onClick={()=>setSelected([])}><X size={15}/></button></header>{entityCard&&<div className="v6-inspector-art"><img src={v6CardImage(entityCard)} alt=""/></div>}
        {'hp'in selectedEntity&&<><div className="v6-health"><i style={{width:`${'maxHp'in selectedEntity?selectedEntity.hp/Number(selectedEntity.maxHp)*100:100}%`}}/></div><div className="v6-stat-line"><span>结构完整度</span><b>{formatV6(selectedHealth)}{'maxHp'in selectedEntity?' / '+formatV6(Number(selectedEntity.maxHp)):''}</b></div></>}
        {selectedRect&&<div className="v6-stat-line"><span>{layerNameV6(selectedRect.z)} / {selectedRect.w} × {selectedRect.h}</span><b>{selectedRect.w*selectedRect.h} 微格</b></div>}
        {selectedShield&&<div className="v6-stat-line"><span>本层 CUDA 围合护盾</span><b className={selectedShield.current>0?'good':'bad'}>{formatV6(selectedShield.current)} / {formatV6(selectedShield.capacity)}</b></div>}
        {s?.jobs?.find(j=>j.target===selectedEntity.id)?.blocked&&<p className="v6-collapse-warning">施工人员无法到达 · 检查门、通路及货梯供电。</p>}
        {entrance?.kind==='elevator'&&<><div className="v6-stat-line"><span>货梯供电</span><b className={entrance.powered?'good':'bad'}>{entrance.powered?'可运行':'停运 · 需要电力线'}</b></div><p className="v6-muted">整座货梯共享一份电力负载，可从任意服务楼层的端口接电。</p>{entrance.owner===r.playerId&&<button className="v6-primary" onClick={()=>{activate('power');setWireStart({...entrance.pos,z:Math.max(Math.min(camera.layer,Math.max(entrance.pos.z,entrance.toLevel)),Math.min(entrance.pos.z,entrance.toLevel))});}}>从货梯端口拉电线<Zap size={14}/></button>}</>}
        {resource&&(resource.kind==='node'?<><div className="v6-stat-line"><span>战略控制权</span><b className={resource.contested?'bad':resource.owner===r.playerId?'good':'muted'}>{resource.contested?'正在争夺':resource.owner===r.playerId?'己方控制':resource.owner?'敌方控制':'中立'}</b></div>{resource.capture>0&&<div className="v6-stat-line"><span>{resource.capturer===r.playerId?'己方占领':'敌方占领'}进度</span><b>{formatV6(resource.capture)} / 20s</b></div>}<p className="v6-muted">控制节点可获得持续经费与科研数据。地面机动单位驻留可夺取，固定炮台不能占领。</p><p className="v6-muted">18分钟后，至少控制两处节点并累计压制360秒取胜；争夺时暂停，失去多数后每秒回退2秒。</p></>:<><div className="v6-stat-line"><span>可开采 / 回收储量</span><b>{resource.remaining<0?'尚未侦察':resource.remaining<=0?'已耗尽':formatV6(resource.remaining)}</b></div><p className="v6-muted">{resource.kind.startsWith('salvage-')?'这是实际掉落的物资，保护运输路线完成回收。':resource.kind==='coal'?'煤层产出可出售矿物与电站燃料。采出物资存于采集器，经运输交付后才能使用。':'建造采集器开采，矿物运抵己方核心或仓库后才会兑换经费。截断运输能削弱对方的后续投入。'}</p></>)}
        {(room||building)&&s&&<V6FacilityStatus entity={(room??building)!} state={s} items={r.catalog.items} owned={(room??building)!.owner===r.playerId} onWire={kind=>{
          const p=v6EntityPosition((room??building)!);activate(kind);setWireStart({x:Math.floor(p.x),y:Math.floor(p.y),z:p.z});r.setNotice('起点已选，点击目标设施端口；两端顺序不影响接通。');
        }}/>}
        {visibleStock&&<div className="v6-stock-list" aria-label="设施库存">{Object.entries(visibleStock).filter(([kind,value])=>value>0||workshopStock&&['ammo','fuel','repair'].includes(kind)).map(([kind,value])=><div key={kind}><span>{CARGO_NAMES[kind]??kind}</span><b>{formatV6(value)}</b></div>)}</div>}
        {(room||building)&&<>{(room??building)!.progress<1&&<><div className="v6-stat-line"><span>施工进度</span><b>{Math.floor((room??building)!.progress*100)}%</b></div><button className="v6-text-action" onClick={()=>void send({op:'cancel',id:(room??building)!.id})}>取消未完工施工<X size={13}/></button></>}{building?.supportRatio!==undefined&&<div className="v6-stat-line"><span>承重支撑</span><b className={building.supportRatio<1?'bad':'good'}>{Math.round(building.supportRatio*100)}%</b></div>}{(building?.collapseWarning??0)>0&&<p className="v6-collapse-warning">结构失稳 · 请立即撤离或抢修</p>}</>}
        {room?.kind==='data-center'&&<V6DataCenterRacks room={room} owned={room.owner===r.playerId} canOrder={r.connected&&!r.busy&&!lobby&&!r.session?.replay&&!s?.winner} sessionId={r.session?.roomId} tickSeconds={Math.floor((s?.tick??0)/60)} onInstall={()=>{setDeck('gpus');setDeckOpen(true);}} onOrder={send}/>}
        {unit&&<V6UnitStatus unit={unit} item={entityCard} owned={unit.owner===r.playerId} catalog={r.catalog.items} onSkill={()=>activate('skill')} onPlugins={()=>{setDeck('plugins');setDeckOpen(true);}} onMove={()=>activate('move')} onStop={()=>void send({op:'stop',ids:selectedUnits})} onReturn={()=>{
          const runway=s?.buildings.filter(b=>b.owner===r.playerId&&b.kind==='airstrip'&&b.hp>0&&b.progress>=1&&b.powered).sort((a,b)=>Math.hypot(a.rect.x-unit.x,a.rect.y-unit.y)-Math.hypot(b.rect.x-unit.x,b.rect.y-unit.y))[0];
          if(!runway){r.setNotice('没有可用的己方机场跑道，请先建成并供电。');return;}
          const p=v6EntityPosition(runway);void send({op:'move',ids:[unit.id],pos:{x:Math.floor(p.x),y:Math.floor(p.y),z:p.z}});
        }}/>}
        {shipment&&<><div className="v6-stat-line"><span>{CARGO_NAMES[shipment.cargo]??shipment.cargo}</span><b>{formatV6(shipment.amount)} · 尚未交付</b></div><div className="v6-stat-line"><span>{shipment.mode==='air'?'运输飞机':'地面运输'}</span><b>{shipment.manualRoute?'指定航点':'自动寻路'}</b></div>{shipment.mode==='air'&&<div className="v6-stat-line"><span>{{'taking-off':'起飞中',cruising:'巡航',landing:'降落卸货'}[shipment.flightState??'']??'装卸准备'} · 燃油</span><b>{rateV6(shipment.fuel)} / {formatV6(shipment.fuelMax)}</b></div>}{(shipment.unloadProgress??0)>0&&<p className="v6-unit-hint">卸货 {Math.floor(shipment.unloadProgress!*100)}% · 完成后才进入目标库存</p>}<p className="v6-muted">#{shipment.from} → #{shipment.to} · 剩余 {shipment.route.length} 格路程</p>{shipment.owner===r.playerId&&<><button className="v6-primary" onClick={()=>{activate('shipment-route');setRouteShipmentId(shipment.id);setRoutePoints(shipment.waypoints??[]);setRouteBusy(false);r.setNotice('依次点击地图设置途经点，Enter确认、Backspace撤销。运输按可达路线经过航点后交付，提交按钮显示本次改线费用。');}}>指定运输途经点<Truck size={16}/></button>{shipment.manualRoute&&<button className="v6-text-action" onClick={()=>void send({op:'reroute-shipment',id:shipment.id,waypoints:[]})}>恢复自动寻路<ChevronRight size={14}/></button>}</>}</>}
        {room?.kind==='research-lab'&&<button className="v6-primary" onClick={()=>setPanel('tech')}>发展科技<FlaskConical size={16}/></button>}
        {room&&<button className="v6-text-action" onClick={()=>setSelected([{kind:'building',id:room.shell}])}>选择所在楼体<Layers3 size={14}/></button>}
        {selectedEntity.owner===r.playerId&&<div className="v6-entrance-list">{s?.entrances.filter(e=>e.owner===r.playerId&&e.hp>0&&(selection?.kind==='entrance'?e.id===selectedEntity.id:selectedRect?containsV6(selectedRect,e.pos):'pos'in selectedEntity?e.pos.x===selectedEntity.pos.x&&e.pos.y===selectedEntity.pos.y&&e.pos.z===selectedEntity.pos.z:false)).map(e=><button key={e.id} onClick={()=>void send({op:'toggle-entrance',id:e.id,open:!(e.open??true)})}><span>{ENTITY_NAMES[e.kind]??e.kind} · {e.width??1}格</span><b>{e.open===false?'开启':'关闭'}</b></button>)}</div>}
        {building?.kind==='shell'&&<><button className="v6-primary" onClick={()=>{setDeck('construction');setDeckOpen(true);activate('room');}}>划分功能房间<Warehouse size={16}/></button><button className="v6-text-action" onClick={()=>activate('expand')}>拖拽扩建本层<Expand size={14}/></button><button className="v6-text-action" onClick={()=>{setCamera(c=>clampCameraV6({...c,layer:building.rect.z+1}));activate('shell');r.setNotice('在上层拖出受下层完整支撑的毛坯，保留跨层入口位置。');}}>向上增建一层<Layers3 size={14}/></button></>}
        {room&&room.owner===r.playerId&&room.progress>=1&&<details className="v6-room-tools"><summary>房间改造与分割<ChevronDown size={13}/></summary><div><select aria-label="分割方向" value={splitAxis} onChange={e=>setSplitAxis(e.target.value as 'x'|'y')}><option value="x">沿长边坐标分割</option><option value="y">沿宽边坐标分割</option></select><input aria-label="分割位置格数" type="number" min={2} max={(splitAxis==='x'?room.rect.w:room.rect.h)-2} value={splitOffset} onChange={e=>setSplitOffset(Number(e.target.value))}/><button onClick={()=>void send({op:'split-room',id:room.id,axis:splitAxis,offset:splitOffset})}>分割</button><button onClick={()=>{activate('merge');r.setNotice('点击另一个相邻的同用途房间，合并后资源和生命总量保持一致。');}}>选择相邻房间合并</button><select aria-label="改造后的房间用途" value={selectedRoomKind} onChange={e=>setSelectedRoomKind(e.target.value)}>{r.catalog.items.filter(c=>c.category==='rooms').map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select>{selectedRoomKind==='research-lab'&&<select aria-label="改造研究所方向" value={branch} onChange={e=>setBranch(e.target.value)}>{V6_BRANCHES.map(b=><option key={b.id} value={b.id}>{b.name}</option>)}</select>}<button onClick={()=>void send({op:'convert-room',id:room.id,kind:selectedRoomKind,...(selectedRoomKind==='research-lab'?{branch}:{})})}>支付改造费用并施工</button></div></details>}
        {selection?.kind==='rubble'&&<><p className="v6-muted">清理残骸恢复道路，并回收可用材料。</p><button className="v6-primary" onClick={()=>void send({op:'clear-rubble',id:selectedEntity.id})}>清理并回收<Shovel size={16}/></button></>}
        {selectedEntity.owner===r.playerId&&'hp'in selectedEntity&&selection?.kind!=='shipment'&&<div className="v6-inspector-actions">{(unit||building)&&<button onClick={()=>void send({op:'upgrade',id:selectedEntity.id})}><ArrowUp size={14}/>{unit?'升级单位':'强化结构'}</button>}<button onClick={()=>void send({op:'repair',id:selectedEntity.id})}><Wrench size={14}/>维修</button>{building?.kind!=='core'&&<button onClick={()=>void send({op:'recycle',id:selectedEntity.id})}><Box size={14}/>回收</button>}</div>}
      </aside>}
      {tool!=='select'&&<div className="v6-tool-banner">
        <span>{TOOL_LABELS[tool]}</span>
        {tool==='shipment-route'&&<><small>{routePoints.length} / 16 航点 · 示意连线，提交后沿可达道路绕障</small><button disabled={routeBusy||!routePoints.length} onClick={()=>setRoutePoints(points=>points.slice(0,-1))}>撤销航点</button><button disabled={routeBusy||!routePoints.length} onClick={()=>void commitShipmentRoute()}>确认路线 ◈{logisticsQuote('rerouteCost')} <kbd>Enter</kbd></button></>}
        {tool==='room'&&selectedRoomKind==='research-lab'&&<select aria-label="新研究所科技方向" value={branch} onChange={e=>setBranch(e.target.value)}>{V6_BRANCHES.map(b=><option key={b.id} value={b.id}>{b.name}</option>)}</select>}
        {tool==='wall'&&<select aria-label="墙体类型" value={wallKind} onChange={e=>setWallKind(e.target.value)}><option value="physical">实体墙</option><option value="cuda">CUDA 数据墙</option><option value="moat">CUDA 护城河</option></select>}
        {tool==='entrance'&&<>
          <select aria-label="跨层入口类型" value={entranceKind} onChange={e=>{setEntranceKind(e.target.value);if(e.target.value==='ramp')setEntranceWidth(w=>Math.max(3,w));}}><option value="door">门 / 同层通道</option><option value="stairs">楼梯</option><option value="elevator">货梯</option><option value="ramp">车辆坡道</option></select>
          <select aria-label="通道宽度" value={entranceWidth} onChange={e=>setEntranceWidth(Number(e.target.value))}>{[1,2,3,4].filter(w=>entranceKind!=='ramp'||w>=3).map(w=><option key={w} value={w}>{w}格宽</option>)}</select>
          {entranceKind==='door'?<small>点击毛坯外墙开门</small>:<select aria-label="入口目标楼层" value={entranceTarget} onChange={e=>setToLevel(Number(e.target.value))}>{[-2,-1,0,1,2,3,4,5].filter(z=>z!==camera.layer).map(z=><option key={z} value={z}>通往 {layerNameV6(z)}</option>)}</select>}
        </>}
        {tool==='supply'&&<><select aria-label="补给运输方式" value={transportMode} onChange={e=>{setTransportMode(e.target.value as 'ground'|'air');setSupplySource(null);}}><option value="ground">地面运输</option><option value="air">机场间空运</option></select><select aria-label="补给货物类型" value={cargo} onChange={e=>setCargo(e.target.value)}><option value="ammo">弹药</option><option value="fuel">燃料</option><option value="repair">维修物资</option><option value="credits">矿物入库</option></select><input className="v6-supply-amount" aria-label="本次补给数量" type="number" min={1} max={1000} value={supplyAmount} onChange={e=>setSupplyAmount(Math.max(1,Math.min(1000,Number(e.target.value)||1)))}/>{transportMode==='air'&&<small>己方供电跑道 → 跑道 · 派遣{logisticsQuote('airDispatchCost')}经费 + {logisticsQuote('airFuelMax')}燃油</small>}</>}
        <small>{card&&['room','build','deploy'].includes(tool)?card.name:wireStart?'起点已选 · 点击目标端口':'在地图上操作'}</small><button aria-label="取消当前工具" onClick={cancel}><X size={14}/><kbd>Esc</kbd></button>
      </div>}
      {preview&&<div className={`v6-cost-preview ${preview.valid?'':'invalid'}`}><span>{preview.valid?(tool==='deploy'?'可部署':'可施工'):preview.reason}</span>{preview.valid&&<><b>◈ {formatV6(preview.cost?.credits)}</b>{preview.cost?.compute>0&&<b>算力 {formatV6(preview.cost.compute)}</b>}{(['power','compute'].includes(tool)||preview.completionProjection)&&<small>{preview.completionProjection?'预计竣工后':'接线后'}电力 {formatV6(preview.powerAfter)} / {formatV6(preview.demandAfter)}</small>}{preview.targetPowered!==undefined&&<small>{preview.targetPowered?'目标将通电':'目标仍未通电'}</small>}{preview.targetConnected!==undefined&&<small>{preview.targetConnected?'目标将接通算力':'目标尚未接通算力'}</small>}</>}</div>}
      {r.session?.replay&&<div className="v6-replay-controls">
        <span>行动回放 · {timeV6(seekDraft??s?.tick??0)}</span>
        <button onClick={async()=>{await r.action('replay-control',{seekTick:0});}}>回到开始</button>
        <button onClick={async()=>{await r.action('replay-control',{seekTick:Math.max(0,(s?.tick??0)-600)});}}>后退10秒</button>
        <button onClick={async()=>{const paused=!(s?.playback?.paused??replayPaused);if(await r.action('replay-control',{paused}))setReplayPaused(paused);}}>{(s?.playback?.paused??replayPaused)?'继续':'暂停'}</button>
        <select aria-label="回放速度" value={s?.playback?.speed??replaySpeed} onChange={async e=>{const speed=Number(e.target.value);if(await r.action('replay-control',{speed}))setReplaySpeed(speed);}}>{[.5,1,2,4,8].map(v=><option key={v} value={v}>{v}×</option>)}</select>
        {s?.playback&&<input aria-label="回放时间轴" type="range" min={0} max={s.playback.totalTicks} value={seekDraft??s.tick} onChange={e=>setSeekDraft(Number(e.target.value))} onPointerUp={async e=>{const seekTick=Number(e.currentTarget.value);await r.action('replay-control',{seekTick});setSeekDraft(null);}} onKeyUp={async e=>{if(['Enter','ArrowLeft','ArrowRight','Home','End'].includes(e.key)){await r.action('replay-control',{seekTick:Number(e.currentTarget.value)});setSeekDraft(null);}}}/>}
      </div>}
      <footer className={`v6-command-deck ${deckOpen?'expanded':'collapsed'}`}>
        <header><div>{([['construction','建筑',Warehouse,'B'],['units','战斗单位',Swords,'U'],['gpus','显卡',Cpu,'I'],['plugins','插件',FlaskConical,'']] as const).map(([id,name,Icon,key])=><button key={id} aria-pressed={deck===id} onClick={()=>{setDeck(id);setDeckOpen(true);}}><Icon size={16}/>{name}{key&&<kbd>{key}</kbd>}</button>)}</div>
          {deckOpen&&<div className="v6-deck-filters">
            <select aria-label="牌组科技方向" value={deckBranch} onChange={e=>setDeckBranch(e.target.value)}><option value="all">全部方向</option>{V6_BRANCHES.map(b=><option key={b.id} value={b.id}>{b.name}</option>)}</select>
            {deck==='units'&&<select aria-label="单位兵种筛选" value={roleFilter} onChange={e=>setRoleFilter(e.target.value)}><option value="all">全部兵种</option><option value="ai">AI角色</option><option value="turret">固定炮台</option><option value="vehicle">地面载具</option><option value="air">空中力量</option><option value="orbital">天基武器</option></select>}
          </div>}
          <button className="v6-icon" aria-label={deckOpen?'收起牌组':'展开牌组'} onClick={()=>setDeckOpen(!deckOpen)}>{deckOpen?<ChevronDown size={17}/>:<ChevronUp size={17}/>}</button>
        </header>
        {deckOpen&&<div className="v6-deck-scroll">
          {deck==='construction'&&<button className={`v6-foundation-card ${tool==='shell'?'selected':''}`} onClick={()=>activate('shell')}><Layers3 size={38}/><strong>毛坯框架</strong><small>自由拖拽长宽<br/>4—24格 · 可扩建多层</small><span>先筑结构，再赋予用途</span></button>}
          {items.map((item,index)=><V6Card key={item.category+item.id} item={item} player={player} index={index} selected={card?.id===item.id} prerequisite={item.category==='units'&&!initialLab?'先建研究所':item.category==='plugins'?v6PluginLock(item,unit,r.catalog.items,r.playerId):undefined} onClick={()=>selectCard(item)}/>)}
        </div>}
      </footer>
      {r.session?.mode==='solo'&&!r.session.replay&&<button className={`v6-pause-control ${r.session.paused?'paused':''}`} aria-label={r.session.paused?'继续战斗':'暂停战斗'} onClick={()=>void r.action('pause',{paused:!r.session?.paused,sessionId:r.session?.roomId})}>{r.session.paused?<Play size={16}/>:<Pause size={16}/>}<kbd>Space</kbd></button>}
      <div className="v6-notice" role="status">{saveNotice||(r.session?.paused?(r.notice.startsWith('战斗已暂停，')?r.notice:'战斗已暂停 · 可以查看楼层、科技与资源，按空格继续。'):r.notice)}<small>{r.connected?`${r.fps.toFixed(0)} FPS`:'连接中断'}</small></div>
      {r.error&&<div className="v6-error" role="alert">{r.error}<button onClick={r.reconnect}>恢复连接</button></div>}
      {!r.streaming&&!r.error&&<div className="v6-loading"><Radio/><strong>正在接入战场画面</strong><small>保留当前行动，等待本机渲染器</small></div>}
      {s?.winner!==null&&s?.winner!==undefined&&<div className="v6-result"><span>OPERATION COMPLETE</span><h1>{v6OutcomeTitle(s,r.playerId)}</h1><p>{s.winReason}</p><button className="v6-primary" onClick={()=>setLobby(true)}>返回行动大厅<ArrowUp size={16}/></button></div>}
    </>}
    {!lobby&&panel&&<div className="v6-panel-scrim" onPointerDown={()=>setPanel(null)}><section className={`v6-panel panel-${panel}`} role="dialog" aria-modal="true" aria-label={panel==='tech'?'科技树':panel==='resources'?'资源网络':panel==='logistics'?'后勤调度':'指挥手册'} onPointerDown={e=>e.stopPropagation()}><header><div><span className="v6-eyebrow">COMMAND / {panel.toUpperCase()}</span><h2>{panel==='tech'?'选择你的技术优势':panel==='resources'?'每一份资源，都有来处':panel==='logistics'?'让补给抵达前线':'指挥手册'}</h2></div><button className="v6-icon" aria-label="关闭面板" onClick={()=>setPanel(null)}><X/></button></header>
      {panel==='tech'?<V6TechnologyPanel player={player} rooms={ownRooms} state={s} catalog={r.catalog} onResearch={(lab,branch)=>void send({op:'research',room:lab.id,branch})} onLocate={lab=>{focus(v6EntityPosition(lab));setSelected([{kind:'room',id:lab.id}]);setPanel(null);}}/>:panel==='resources'?<div className="v6-resource-panel"><div className="v6-resource-hero"><div><Cpu size={38}/><h3>算力与供电拓扑</h3><p>总发电量不等于每个机房都接通。选择下方设施，查看该设施所在楼层的线路和状态。</p><b>{formatV6(player?.compute)} <small>/ {formatV6(player?.computeCapacity)} 算力储量</small></b></div><button className="v6-primary" onClick={()=>{setOverlay('power');setPanel(null);activate('power');}}>绘制电力线路 <kbd>L</kbd></button><button className="v6-primary" onClick={()=>{setOverlay('compute');setPanel(null);activate('compute');}}>绘制算力线路 <kbd>C</kbd></button></div><V6CudaControl enabled={s?.shieldAuto?.[r.playerId-1]} regions={s?.shieldRegions?.filter(q=>q.owner===r.playerId)??[]} onToggle={enabled=>void send({op:'shield',enabled})}/><div className="v6-resource-rows">{[...(s?.buildings.filter(b=>b.owner===r.playerId&&b.kind!=='shell')??[]),...ownRooms].map(e=><button key={e.id} onClick={()=>{focus(v6EntityPosition(e));setSelected([{kind:'shell'in e?'room':'building',id:e.id}]);setPanel(null);}}><span><b>{r.catalog.items.find(c=>c.id===e.kind)?.name??e.kind}</b><small>{layerNameV6(e.rect.z)} · {e.rect.x},{e.rect.y}</small></span><span className={e.powered?'good':'bad'}>{e.powered?'供电正常':'未供电'}</span><span className={e.connected?'good':'muted'}>{e.connected?'算力在线':'未接算力'}</span><ChevronRight size={14}/></button>)}</div></div>:panel==='logistics'?<div className="v6-logistics-panel"><article><Truck size={40}/><h3>运抵仓库，才是你的资源。</h3><p>运输中的货物仍在地图上，可以被拦截。保护通路和入口，为前线补充弹药、燃料与维修物资。</p><label className="v6-field">发货仓库<select aria-label="发货仓库" value={supplySource??''} onChange={e=>setSupplySource(e.target.value?Number(e.target.value):null)}><option value="">选择仓库</option>{ownRooms.filter(q=>q.kind==='depot').map(q=><option key={q.id} value={q.id}>仓库 #{q.id} · {layerNameV6(q.rect.z)}</option>)}</select></label><button className="v6-primary" onClick={()=>{setPanel(null);activate('supply');}}>在地图上指定目的地<ChevronRight size={15}/></button></article><div className="v6-shipment-list">{s?.shipments.filter(q=>q.owner===r.playerId).map(q=><button key={q.id} onClick={()=>{focus(q.pos);setSelected([{kind:'shipment',id:q.id}]);setPanel(null);}}><Truck size={24}/><span><b>{CARGO_NAMES[q.cargo]??q.cargo} · {formatV6(q.amount)}</b><small>#{q.from} → #{q.to} · {layerNameV6(q.pos.z)}</small></span><strong>{(q.unloadProgress??0)>0?`卸货 ${Math.floor(q.unloadProgress!*100)}%`:'运输中'}</strong><ChevronRight size={15}/></button>)}{!s?.shipments.some(q=>q.owner===r.playerId)&&<p>当前没有运输中的货物。连接采集器与仓储，或下达补给任务。</p>}</div></div>:<V6Guide catalog={r.catalog}/>}
    </section></div>}
    {lobby&&<V6Lobby session={r.session} snapshot={s} playerId={r.playerId} catalog={r.catalog} busy={r.busy} error={r.error} connected={r.connected} onBegin={r.begin} onContinue={()=>setLobby(false)} onAction={r.action} onRefresh={()=>void r.refresh()}/>}
  </main>;
}
