import {useCallback,useEffect,useMemo,useRef,useState,type PointerEvent as ReactPointerEvent} from 'react';
import {ArrowUp,Box,Check,ChevronDown,ChevronRight,ChevronUp,CircleHelp,Coins,Cpu,Expand,FlaskConical,Home,Layers3,Link2,MousePointer2,Move,Pause,Play,Radio,Save,Shield,Shovel,Swords,Warehouse,Wrench,X,Zap} from 'lucide-react';
import {useSentinelsV6,v6Request} from '@/lib/useSentinelsV6';
import {V6_BRANCHES,formatV6,rateV6,timeV6,v6AiEconomy,v6ClassicCards,v6EntityPosition,v6IsClassic,v6UnitElevation,v6PluginLock,type V6CatalogItem,type V6Command,type V6Pick,type V6Preview,type V6Room,type V6Selection,type V6Snapshot} from '@/lib/sentinelsV6';
import {V6_INITIAL_CAMERA,cellAtV6,clampCameraV6,containsV6,dragRectV6,orthogonalPathV6,rectOutlineV6,screenToWorldV6,skillOutlineV6,validShellRectV6,wireRouteV6,worldToScreenV6,type V6Camera,type V6Point,type V6Rect} from '@/lib/sentinelsV6Geometry';
import V6Lobby from '@/components/game/v6/V6Lobby';
import V6Card,{v6CardImage,v6CardLock} from '@/components/game/v6/V6Card';
import V6UnitStatus from '@/components/game/v6/V6UnitStatus';
import V6FacilityStatus from '@/components/game/v6/V6FacilityStatus';
import V6DataCenterRacks from '@/components/game/v6/V6DataCenterRacks';
import V6TechnologyPanel from '@/components/game/v6/V6TechnologyPanel';
import V6CudaControl from '@/components/game/v6/V6CudaControl';
import V6Guide from '@/components/game/v6/V6Guide';
import V6Objective,{v6OutcomeTitle} from '@/components/game/v6/V6Objective';
import V6UtilizationCard from '@/components/game/v6/V6UtilizationCard';
import './code-sentinels-v6.css';

type Tool='select'|'shell'|'room'|'build'|'deploy'|'power'|'compute'|'wall'|'move'|'skill'|'expand'|'merge';
type Deck='construction'|'units'|'gpus'|'plugins';
type Gesture={kind:'pan'|'rect'|'select'|'line';start:{x:number;y:number};end:{x:number;y:number};cell:V6Point;camera:V6Camera;shift:boolean};
const DEFAULT_SIZE={width:1280,height:720};
const TERRAIN=['#47544c','#4b5055','#34516b','#82765d','#76816d','#b99561','#605253'];
const ENTITY_NAMES:Record<string,string>={core:'指挥核心',shell:'毛坯楼体',ore:'矿脉',coal:'煤层',node:'战略节点',physical:'实体墙',cuda:'CUDA 数据墙',moat:'CUDA 护城河',power:'电力线路',compute:'算力线路'};
const TOOL_LABELS:Record<Tool,string>={select:'选择',shell:'毛坯施工',room:'房间装修',build:'露天设施',deploy:'部署单位',power:'铺设电力线',compute:'铺设算力线',wall:'建设防御墙',move:'下达移动',skill:'技能瞄准',expand:'扩建毛坯',merge:'合并相邻房间'};
const CAMERA_KEYS=new Set(['w','a','s','d','arrowup','arrowdown','arrowleft','arrowright']);
const straightPath=orthogonalPathV6;
function wirePath(a:V6Point,b:V6Point):V6Point[]|null{return wireRouteV6(a,b);}

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
  return <canvas ref={ref} width={256} height={192} role="img" aria-label="战术地图，点击定位" onClick={e=>{const rect=e.currentTarget.getBoundingClientRect();onMove({x:(e.clientX-rect.left)/rect.width*128,y:(e.clientY-rect.top)/rect.height*96,z:0});}}/>;
}

export default function CodeSentinelsV6(){
  const r=useSentinelsV6(),s=r.snapshot;
  useEffect(()=>{const previous=document.title;document.title='编译防线 · 平面前线 V6';return()=>{document.title=previous;};},[]);
  const[lobby,setLobby]=useState(true),[deck,setDeck]=useState<Deck>('construction'),[deckOpen,setDeckOpen]=useState(true),[tool,setTool]=useState<Tool>('select');
  const[card,setCard]=useState<V6CatalogItem|null>(null),[selected,setSelected]=useState<V6Selection[]>([]),[camera,setCamera]=useState<V6Camera>(V6_INITIAL_CAMERA),[size,setSize]=useState(DEFAULT_SIZE);
  const[hover,setHover]=useState<V6Point|null>(null),[gesture,setGesture]=useState<Gesture|null>(null),[wireStart,setWireStartState]=useState<V6Point|null>(null),[branch,setBranch]=useState('speed');
  const wireStartUnit=useRef<number|null>(null);
  const setWireStart=useCallback((point:V6Point|null,unitId:number|null=null)=>{wireStartUnit.current=unitId;setWireStartState(point);},[]);
  const[panel,setPanel]=useState<'tech'|'resources'|'help'|null>(null),[wallKind,setWallKind]=useState('physical'),[overlay,setOverlay]=useState<'none'|'power'|'compute'>('none');
  const[tutorial,setTutorial]=useState(true),[groups,setGroups]=useState<Record<string,number[]>>({});
  const[saveNotice,setSaveNotice]=useState(''),[selectedRoomKind,setSelectedRoomKind]=useState('data-center'),[splitAxis,setSplitAxis]=useState<'x'|'y'>('x'),[splitOffset,setSplitOffset]=useState(2);
  const[preview,setPreview]=useState<V6Preview|null>(null),[replayPaused,setReplayPaused]=useState(false),[replaySpeed,setReplaySpeed]=useState(1);
  const[deckBranch,setDeckBranch]=useState('all'),[roleFilter,setRoleFilter]=useState('all');
  const[seekDraft,setSeekDraft]=useState<number|null>(null);
  const[wireBusy,setWireBusy]=useState(false),toolEpoch=useRef(0);
  const pickSequence=useRef(0),latestCamera=useRef('');latestCamera.current=JSON.stringify(camera);
  const stage=useRef<HTMLDivElement>(null),board=useRef<HTMLDivElement>(null),gestureRef=useRef<Gesture|null>(null),cameraTimer=useRef(0),sessionCenter=useRef('');
  const panKeys=useRef(new Set<string>()),panFrame=useRef(0),heldCamera=useRef(camera);heldCamera.current=camera;
  const player=s?.players.find(p=>p.owner===r.playerId),ownRooms=s?.rooms.filter(q=>q.owner===r.playerId)??[],ownUnits=s?.units.filter(u=>u.owner===r.playerId)??[];
  const selection=selected[0],unit=selection?.kind==='unit'?s?.units.find(u=>u.id===selection.id):null,room=selection?.kind==='room'?s?.rooms.find(q=>q.id===selection.id):null,building=selection?.kind==='building'?s?.buildings.find(b=>b.id===selection.id):null;
  const selectedEntity=room??unit??building??(selection?.kind==='resource'?s?.resources.find(n=>n.id===selection.id):selection?.kind==='wall'?s?.walls.find(n=>n.id===selection.id):selection?.kind==='link'?s?.links.find(n=>n.id===selection.id):selection?.kind==='rubble'?s?.rubble?.find(n=>n.id===selection.id):null);
  const selectedHealth=selectedEntity&&'hp'in selectedEntity&&'maxHp'in selectedEntity&&Math.abs(selectedEntity.hp-Number(selectedEntity.maxHp))<1e-7?Number(selectedEntity.maxHp):selectedEntity&&'hp'in selectedEntity?selectedEntity.hp:0;
  const resource=selection?.kind==='resource'?s?.resources.find(n=>n.id===selection.id):null;
  const aiEconomy=v6AiEconomy(r.catalog,s?.units??[],r.playerId);
  const selectedPosition=room||building?v6EntityPosition((room??building)!):unit?.pos??(selection?.kind==='wall'?s?.walls.find(w=>w.id===selection.id)?.pos:null);
  const selectedShield=selectedPosition?s?.shieldRegions?.find(region=>region.owner===r.playerId&&region.cells.some(p=>p.z===0&&Math.abs(p.x-Math.floor(selectedPosition.x))+Math.abs(p.y-Math.floor(selectedPosition.y))<=(selection?.kind==='wall'?1:0))):undefined;
  const entityCard=r.catalog.items.find(c=>c.id===(selectedEntity&&'kind'in selectedEntity?selectedEntity.kind:''));
  const selectedName=entityCard?.name??(selection?.kind==='rubble'?'可回收残骸':selectedEntity&&'kind'in selectedEntity?ENTITY_NAMES[selectedEntity.kind]??selectedEntity.kind:'资源节点');
  const selectedUnitIds=selected.filter(q=>q.kind==='unit').map(q=>q.id);
  const selectedUnits=selectedUnitIds.filter(id=>ownUnits.some(u=>u.id===id));
  const rectangle=gesture&&['shell','room','expand'].includes(tool)?dragRectV6(gesture.cell,screenToWorldV6(gesture.end,camera,size)):null;
  const ownShells=s?.buildings.filter(b=>b.kind==='shell'&&b.owner===r.playerId)??[];
  // Classic removes the whole base-building layer: no shells, rooms, wires or GPUs.
  const classic=v6IsClassic(s,r.session);
  const initialLab=classic||ownRooms.some(q=>q.kind==='research-lab'&&q.progress>=1&&q.hp>0);
  const tutorialSteps:[string,boolean][]=classic?[
    ['B 在矿脉旁建采集器',s?.buildings.some(b=>b.owner===r.playerId&&b.kind==='extractor'&&b.progress>=1)??false],
    ['U 在核心12格内部署炮台',ownUnits.length>0],
    ['在核心研究一个科技分支',Object.keys(player?.branches??{}).length>0],
    ['部署分支单位组建部队',ownUnits.some(q=>!['vscode','pycharm'].includes(q.kind))],
    ['争夺战略节点完成压制',(s?.resources.filter(n=>n.kind==='node'&&n.owner===r.playerId).length??0)>0],
  ]:[
    ['B 拖出毛坯，等待施工',ownShells.some(b=>b.progress>=1)],
    ['划分机房，风机 → L 接电',ownRooms.some(q=>q.kind==='data-center'&&q.powered)],
    ['选中机房，I 安装显卡',ownRooms.some(q=>q.gpus.length>0)],
    ['研究所接入电力与算力',ownRooms.some(q=>q.kind==='research-lab'&&q.powered&&q.connected)],
    ['部署防御单位',ownUnits.some(q=>q.kind!=='builder')],
  ];
  const items=useMemo(()=>{
    const available=classic?v6ClassicCards(r.catalog,r.catalog.items):r.catalog.items;
    const list=available.filter(c=>(deck==='construction'?c.category==='rooms'||c.category==='buildings':c.category===deck)
      &&(deckBranch==='all'||!c.branch||c.branch===deckBranch)&&(deck!=='units'||roleFilter==='all'||c.role===roleFilter));
    if(deck==='construction'){const priority=classic?['extractor','airstrip']:['wind-power','extractor','data-center','research-lab','factory','mobile-relay'];const rank=(id:string)=>{const n=priority.indexOf(id);return n<0?999:n;};list.sort((a,b)=>rank(a.id)-rank(b.id));}
    return list.map(item=>item.role==='ai'&&aiEconomy.ready?{...item,cost:item.cost*aiEconomy.multiplier}:item.category==='plugins'&&unit?.owner===r.playerId?{...item,cost:item.cost*(1-Math.max(0,Math.min(.1,unit.pluginDiscount??0)))}:item);
  },[classic,deck,r.catalog,deckBranch,roleFilter,aiEconomy.multiplier,aiEconomy.ready,unit?.owner,unit?.pluginDiscount,r.playerId]);
  const send=r.order;
  const focus=useCallback((point:V6Point)=>setCamera(old=>clampCameraV6({...old,x:point.x,y:point.y})),[]);
  function cancel(){toolEpoch.current++;setTool('select');setWireStart(null);setWireBusy(false);setGesture(null);gestureRef.current=null;setHover(null);r.setNotice('已取消当前工具');}
  function activate(next:Tool){
    if(next==='power'||next==='compute')setOverlay(next);
    if(next==='skill'){
      if(!unit||unit.owner!==r.playerId){r.setNotice('先选中一名己方作战单位。');return;}
      if(entityCard?.skillShape==='self'){void send({op:'skill',id:unit.id,pos:unit.pos});return;}
    }
    toolEpoch.current++;setTool(next);setWireStart(null);setWireBusy(false);setGesture(null);gestureRef.current=null;r.setNotice(next==='power'?'电力线：先点发电端，再点用电端；Shift 可连续拐弯布线。':next==='compute'?'算力线：先点运行机房，再点研究所、炮台或基站；Shift 连续布线。':next==='shell'?'拖出4–24格长宽的矩形地基。':next==='room'?'在已建好的毛坯内拖出房间，或点击整层装修。':TOOL_LABELS[next]);
  }
  function selectCard(item:V6CatalogItem){
    setCard(item);const lock=v6CardLock(item,player);if(lock){toolEpoch.current++;setTool('select');setWireStart(null);r.setNotice(`${item.name}：需要${lock}；可以在科技面板查看发展条件。`);return;}
    if(item.category==='units'&&!initialLab){r.setNotice('先完成初始研究所施工，再部署基础炮台或研究后续单位。');return;}
    if(classic&&['rooms','gpus'].includes(item.category)){r.setNotice('单层塔防模式没有房间与显卡，直接部署单位或建造露天设施。');return;}
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
    const rubble=s.rubble?.find(q=>containsV6(q.rect,p));if(rubble)return{kind:'rubble',id:rubble.id};
    const node=s.resources.find(n=>n.pos.z===p.z&&Math.hypot(n.pos.x-p.x,n.pos.y-p.y)<2);return node?{kind:'resource',id:node.id}:null;
  }
  const wirePorts=(tool==='power'||tool==='compute')&&s?[
    ...[...ownRooms,...s.buildings.filter(b=>b.owner===r.playerId&&b.kind!=='shell')].filter(e=>e.rect.z===0&&e.hp>0).map(e=>({selection:{kind:'shell'in e?'room':'building',id:e.id} as V6Selection,pos:{x:e.rect.x+Math.floor(e.rect.w/2),y:e.rect.y+Math.floor(e.rect.h/2),z:e.rect.z},online:tool==='power'?e.powered:e.connected}))
  ].map(p=>({...p,screen:worldToScreenV6({x:p.pos.x+.5,y:p.pos.y+.5,z:p.pos.z},camera,size)})).filter(p=>p.screen.x>=0&&p.screen.y>=0&&p.screen.x<=size.width&&p.screen.y<=size.height):[];
  function endpoint(p:V6Point,picked?:V6Selection|null,pickedPos?:V6Point){const hit=picked===undefined?entityAt(p):picked;if(!s||!hit)return p;const entity=hit.kind==='unit'?s.units.find(u=>u.id===hit.id):hit.kind==='room'?s.rooms.find(q=>q.id===hit.id):s.buildings.find(b=>b.id===hit.id);if(hit.kind==='building'&&entity&&'kind'in entity&&entity.kind==='shell')return p;const value=entity?v6EntityPosition(entity):pickedPos??p;return{x:Math.floor(value.x),y:Math.floor(value.y),z:0};}
  function perform(p:V6Point,shift=false,picked?:V6Selection|null,pickedPos?:V6Point){
    if(!s)return;
    if(tool==='select'){const hit=picked===undefined?entityAt(p):picked;setSelected(old=>hit?shift?[...old.filter(q=>q.kind!==hit.kind||q.id!==hit.id),hit]:[hit]:[]);if(hit?.kind==='room'&&s.rooms.find(q=>q.id===hit.id)?.kind==='data-center'){setDeck('gpus');setDeckOpen(true);}return;}
    if(tool==='move'){if(!selectedUnits.length){r.setNotice('先选中可移动的单位。');return;}void send({op:shift?'queue-move':'move',ids:selectedUnits,pos:p});}
    else if(tool==='skill'){if(!unit){r.setNotice('先选中一名有主动能力的角色。');return;}void send({op:'skill',id:unit.id,pos:p,direction:p});}
    else if(tool==='build'&&card)void send({op:'build',pos:p,kind:card.id});
    else if(tool==='deploy'&&card&&classic)void send({op:'deploy',room:0,kind:card.id,pos:p});
    else if(tool==='deploy'&&card){const from=producer(card);if(!from){r.setNotice('尚无可生产此单位的功能房间。');return;}void send({op:'deploy',room:from.id,kind:card.id,pos:p});}
    else if(tool==='merge'){const next=picked?s.rooms.find(q=>q.id===picked.id):s.rooms.find(q=>containsV6(q.rect,p));if(!room||!next){r.setNotice('先选择一个房间，再点击相邻的同用途房间。');return;}void send({op:'merge-rooms',ids:[room.id,next.id]});}
    else if(tool==='power'||tool==='compute'){
      if(wireBusy){r.setNotice('正在确认线路，请稍候。');return;}
      const targetUnit=tool==='compute'&&picked?.kind==='unit'?s.units.find(u=>u.id===picked.id&&u.owner===r.playerId):null;
      const endUnit=targetUnit&&r.catalog.items.some(d=>d.category==='units'&&d.id===targetUnit.kind&&d.role==='ai')?targetUnit.id:null;
      const end=endpoint(p,picked,pickedPos);if(!wireStart){setWireStart(end,endUnit);r.setNotice('起点已选，点击目标端口完成连接。');return;}
      const path=wirePath(wireStart,end);if(!path){r.setNotice('线路必须落在同一平面可走格上。');return;}
      const currentTool=toolEpoch.current;setWireBusy(true);
      const unitEndpoints=tool==='compute'?[...new Set([wireStartUnit.current,endUnit].filter((id):id is number=>id!==null))]:[];
      void send({op:'wire',kind:tool==='power'?'power':'compute',path,...(unitEndpoints.length?{unitEndpoints}:{})}).then(result=>{if(currentTool!==toolEpoch.current)return;setWireBusy(false);if(result?.accepted)setWireStart(shift?end:null,shift?endUnit:null);});return;
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
      const picked=await v6Request<V6Pick|null>('pick',{screenX:at.x,screenY:at.y,width:size.width,height:size.height,sessionId:r.session?.roomId,view:{centerX:camera.x,centerY:camera.y,zoom:camera.zoom,localPlayer:r.playerId}});
      if(currentTool!==toolEpoch.current||currentCamera!==latestCamera.current||(!shift&&request!==pickSequence.current))return;
      const hit:V6Selection|null=picked&&picked.id>0&&picked.kind!=='terrain'&&['building','room','unit','resource','wall','link','rubble'].includes(picked.kind)?{kind:picked.kind as V6Selection['kind'],id:picked.id}:null;
      const point=p??picked?.pos;if(!point){if(tool==='select')setSelected([]);return;}
      if(right){if(selectedUnits.length)void send(hit&&picked!.owner>0&&picked!.owner!==r.playerId?{op:'attack',ids:selectedUnits,target:hit.id}:{op:shift?'queue-move':'move',ids:selectedUnits,pos:{...point,z:0}});else cancel();}
      else perform({...point,z:0},shift,hit,picked?.pos);
    }catch(e){if(currentTool===toolEpoch.current)r.setNotice(`未能确认点击对象：${(e as Error).message}`);}
  }
  function point(event:{clientX:number;clientY:number}){const rect=stage.current!.getBoundingClientRect();return{x:event.clientX-rect.left,y:event.clientY-rect.top};}
  function pointerDown(e:ReactPointerEvent<HTMLDivElement>){
    if(lobby||!s||!r.streaming||!r.connected)return;if(e.button===2){e.preventDefault();const at=point(e),p=cellAtV6(screenToWorldV6(at,camera,size));void clickScene(p,at,e.shiftKey,true);return;}
    const at=point(e),raw=screenToWorldV6(at,camera,size),cell=cellAtV6(raw),kind=e.button===1||e.altKey?'pan':['shell','room','expand'].includes(tool)?'rect':tool==='wall'?'line':'select';
    if(!cell&&(kind==='rect'||kind==='line'))return;
    const g:Gesture={kind,start:at,end:at,cell:cell??{...raw,z:0},camera,shift:e.shiftKey};
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
      else{const shell=building?.kind==='shell'?building:ownShells.find(b=>containsV6(b.rect,g.cell));if(!shell){r.setNotice('请在已建好的己方毛坯楼内划分房间。');return;}if(distance<5)rect={...shell.rect};void send({op:'room',shell:shell.id,rect,kind:selectedRoomKind,...(selectedRoomKind==='research-lab'?{branch}:{})});}
      if(!g.shift)setTool('select');return;
    }
    if(g.kind==='line'){if(cell)void send({op:'wall',kind:classic?'physical':wallKind,path:straightPath(g.cell,cell)});return;}
    if(tool==='select'&&distance>6){const ids=ownUnits.filter(u=>{const p=worldToScreenV6({x:u.x,y:u.y,z:v6UnitElevation(u)},camera,size);return p.x>=Math.min(g.start.x,at.x)&&p.x<=Math.max(g.start.x,at.x)&&p.y>=Math.min(g.start.y,at.y)&&p.y<=Math.max(g.start.y,at.y);}).map(u=>({kind:'unit' as const,id:u.id}));setSelected(g.shift?[...selected,...ids.filter(i=>!selected.some(q=>q.kind===i.kind&&q.id===i.id))]:ids);return;}
    if(['select','power','compute','merge'].includes(tool))void clickScene(cell,at,g.shift);else if(cell)perform(cell,g.shift);
  }
  useEffect(()=>{const element=board.current;if(!element)return;const observer=new ResizeObserver(entries=>{const {width,height}=entries[0].contentRect;const w=Math.min(width,height*16/9);setSize({width:w,height:w*9/16});});observer.observe(element);return()=>observer.disconnect();},[lobby]);
  useEffect(()=>{if(!r.session||r.session.status==='lobby')return;window.clearTimeout(cameraTimer.current);cameraTimer.current=window.setTimeout(()=>void r.camera(camera),50);return()=>window.clearTimeout(cameraTimer.current);},[camera,r.session?.roomId,r.session?.status,r.playerId]);
  useEffect(()=>{if(!s||!r.session||sessionCenter.current===r.session.roomId)return;const core=s.buildings.find(b=>b.owner===r.playerId&&b.kind==='core');if(core){sessionCenter.current=r.session.roomId;focus(v6EntityPosition(core));}},[s,r.session,r.playerId,focus]);
  useEffect(()=>{if(r.session?.status==='battle'&&r.session.mode==='join')setLobby(false);},[r.session?.status,r.session?.mode]);
  useEffect(()=>{toolEpoch.current++;setSelected([]);setGroups({});setWireStart(null);setTool('select');setPreview(null);},[r.session?.roomId]);
  useEffect(()=>{if(!saveNotice)return;const timer=window.setTimeout(()=>setSaveNotice(''),3500);return()=>window.clearTimeout(timer);},[saveNotice]);
  useEffect(()=>{
    const stop=()=>{panKeys.current.clear();if(panFrame.current){window.cancelAnimationFrame(panFrame.current);panFrame.current=0;}};
    if(lobby){stop();return;}
    const step=()=>{
      const held=panKeys.current;let dx=0,dy=0;
      if(held.has('w')||held.has('arrowup')){dx-=1;dy-=1;}
      if(held.has('s')||held.has('arrowdown')){dx+=1;dy+=1;}
      if(held.has('a')||held.has('arrowleft')){dx-=1;dy+=1;}
      if(held.has('d')||held.has('arrowright')){dx+=1;dy-=1;}
      if(!dx&&!dy){panFrame.current=0;return;}
      const n=Math.hypot(dx,dy),stride=.42/Math.max(.4,heldCamera.current.zoom);
      setCamera(c=>clampCameraV6({...c,x:c.x+dx/n*stride,y:c.y+dy/n*stride}));
      panFrame.current=window.requestAnimationFrame(step);
    };
    const typing=(e:KeyboardEvent)=>e.target instanceof HTMLElement&&(['INPUT','TEXTAREA','SELECT'].includes(e.target.tagName)||e.target.isContentEditable);
    const down=(e:KeyboardEvent)=>{
      if(typing(e)||e.ctrlKey||e.metaKey)return;
      const k=e.key.toLowerCase();if(!CAMERA_KEYS.has(k))return;
      e.preventDefault();panKeys.current.add(k);if(!panFrame.current)panFrame.current=window.requestAnimationFrame(step);
    };
    const up=(e:KeyboardEvent)=>{panKeys.current.delete(e.key.toLowerCase());};
    window.addEventListener('keydown',down);window.addEventListener('keyup',up);window.addEventListener('blur',stop);
    return()=>{window.removeEventListener('keydown',down);window.removeEventListener('keyup',up);window.removeEventListener('blur',stop);stop();};
  },[lobby]);
  useEffect(()=>{const key=(e:KeyboardEvent)=>{
    if(lobby||(e.target instanceof HTMLElement&&(['INPUT','TEXTAREA','SELECT'].includes(e.target.tagName)||e.target.isContentEditable)))return;
    const k=e.key.toLowerCase();if(CAMERA_KEYS.has(k)||e.ctrlKey&&k.length===1&&k>='a'&&k<='z')return;
    if(panel&&k!=='escape')return;if(k==='escape'){e.preventDefault();if(panel)setPanel(null);else cancel();}
    else if(k==='home'){e.preventDefault();const core=s?.buildings.find(b=>b.owner===r.playerId&&b.kind==='core');if(core)focus(v6EntityPosition(core));}
    else if(/^[0-9]$/.test(k)){if(e.ctrlKey){e.preventDefault();setGroups(old=>({...old,[k]:selectedUnits}));r.setNotice(`编队${k}：${selectedUnits.length}个单位`);}else if(groups[k])setSelected(groups[k].map(id=>({kind:'unit',id})));else if(Number(k)>0&&items[Number(k)-1])selectCard(items[Number(k)-1]);}
    else if(k==='b'){setDeck('construction');setDeckOpen(true);activate(classic?'build':'shell');}
    else if(k==='u'){setDeck('units');setDeckOpen(true);}else if(k==='i'){if(classic)r.setNotice('单层塔防模式没有显卡机架。');else{setDeck('gpus');setDeckOpen(true);}}
    else if(k==='l'||k==='c'){if(classic)r.setNotice('单层塔防模式没有电力与算力线路。');else activate(k==='l'?'power':'compute');}
    else if(k==='t')activate('wall');else if(k==='m')activate('move');else if(k==='q')activate('skill');
    else if(k==='x'&&selectedUnits.length)void send({op:'stop',ids:selectedUnits});
    else if(k==='g'&&!classic)setOverlay(old=>old==='none'?'power':old==='power'?'compute':'none');
    else if(k==='h')setPanel(panel==='help'?null:'help');
    else if(k===' '){e.preventDefault();if(r.session?.replay)void r.action('replay-control',{paused:!(s?.playback?.paused??false)});else if(r.session?.mode==='solo')void r.action('pause',{paused:!r.session.paused,sessionId:r.session.roomId});else r.setNotice('双人对战持续进行，不能单方面暂停。');}
  };window.addEventListener('keydown',key);return()=>window.removeEventListener('keydown',key);});

  const selectedRect=room?.rect??building?.rect;
  const ghost=rectangle??(hover&&['build','deploy','shell'].includes(tool)?{...hover,w:tool==='shell'?4:card?.width??1,h:tool==='shell'?4:card?.height??1}:null);
  const polygon=(rect:V6Rect)=>rectOutlineV6(rect,camera,size).map(p=>`${p.x},${p.y}`).join(' ');
  const activeLine=wireStart&&hover?wirePath(wireStart,endpoint(hover)):gesture?.kind==='line'&&hover?straightPath(gesture.cell,hover):null;
  const unitScreen=unit?worldToScreenV6({x:unit.x,y:unit.y,z:v6UnitElevation(unit)},camera,size):null;
  const core=s?.buildings.find(b=>b.kind==='core'&&b.owner===r.playerId);
  let candidate:V6Command|null=null;
  if((tool==='power'||tool==='compute')&&activeLine)candidate={op:'wire',kind:tool==='power'?'power':'compute',path:activeLine};
  else if(tool==='shell'&&ghost&&validShellRectV6(ghost))candidate={op:'shell',rect:ghost};
  else if(tool==='build'&&hover&&card)candidate={op:'build',pos:hover,kind:card.id};
  else if(tool==='deploy'&&hover&&card&&classic)candidate={op:'deploy',room:0,kind:card.id,pos:hover};
  else if(tool==='deploy'&&hover&&card){const from=producer(card);if(from)candidate={op:'deploy',room:from.id,kind:card.id,pos:hover};}
  else if(tool==='room'&&rectangle){const shell=building?.kind==='shell'?building:ownShells.find(b=>containsV6(b.rect,rectangle));if(shell)candidate={op:'room',shell:shell.id,rect:rectangle,kind:selectedRoomKind,...(selectedRoomKind==='research-lab'?{branch}:{})};}
  else if(tool==='expand'&&rectangle&&building?.kind==='shell')candidate={op:'expand-shell',id:building.id,rect:rectangle};
  const previewKey=candidate?JSON.stringify(candidate):'';
  useEffect(()=>{if(!previewKey||!r.connected||lobby){setPreview(null);return;}const controller=new AbortController();const timer=window.setTimeout(()=>{void v6Request<V6Preview>('preview',{command:JSON.parse(previewKey),sessionId:r.session?.roomId},controller.signal).then(setPreview).catch(()=>{if(!controller.signal.aborted)setPreview(null);});},150);return()=>{controller.abort();window.clearTimeout(timer);};},[previewKey,r.connected,lobby,r.session?.roomId,Math.floor((s?.tick??0)/60),aiEconomy.count]);
  const skillTarget=hover?{x:hover.x+.5,y:hover.y+.5,z:hover.z}:null;
  const skillPoints=unit&&skillTarget?skillOutlineV6(entityCard?.skillShape??'target',{x:unit.pos.x+.5,y:unit.pos.y+.5,z:unit.z},skillTarget,entityCard?.skillRange??entityCard?.range??0,entityCard?.skillRadius??0,entityCard?.skillWidth??0,entityCard?.skillAngle??0,camera,size):[];
  const skillInRange=!!unit&&!!hover&&hover.z===unit.z&&Math.hypot(hover.x-unit.pos.x,hover.y-unit.pos.y)<=(entityCard?.skillRange??entityCard?.range??0);
  const inspectorPreview=room||building?{valid:true,reason:'',cost:{credits:0,compute:0,science:0},powerBefore:player?.power??0,powerAfter:player?.power??0,demandBefore:player?.demand??0,demandAfter:player?.demand??0,netArea:selectedRect?selectedRect.w*selectedRect.h:undefined,capacity:room?.capacity??building?.capacity,costPerCapacity:(()=>{const entity=room??building!;const invested='invested'in entity?entity.invested:0;return entity.capacity>0?invested/entity.capacity:undefined;})(),computeBefore:player?.compute,computeAfter:player?.compute,computeCapacityBefore:player?.computeCapacity,computeCapacityAfter:player?.computeCapacity} as V6Preview:null;

  return <main className="v6-game">
    <div className="v6-board" ref={board}>
      <div className={`v6-stage tool-${tool}`} ref={stage} style={{width:size.width,height:size.height}} onContextMenu={e=>e.preventDefault()} onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp} onPointerCancel={()=>{gestureRef.current=null;setGesture(null);}} onWheel={e=>setCamera(c=>clampCameraV6({...c,zoom:c.zoom*Math.exp(-e.deltaY*.001)}))}>
        <canvas className="v6-native-frame" ref={r.canvasRef} width={1280} height={720} aria-label="原生平面战场"/>
        <svg className="v6-map-overlay" width={size.width} height={size.height} aria-hidden="true">
          {overlay!=='none'&&s?.links.filter(l=>l.kind===overlay).map(l=><polyline key={l.id} points={l.path.map(p=>worldToScreenV6({x:p.x+.5,y:p.y+.5,z:p.z},camera,size)).map(p=>`${p.x},${p.y}`).join(' ')} fill="none" stroke={l.kind==='power'?'#e9c274':'#72d3ca'} strokeWidth={l.active?2.5:1} strokeDasharray={l.active?'':'5 5'} opacity={.85}/>)}
          {selectedRect&&<polygon points={polygon(selectedRect)} className="v6-selected-footprint"/>}
          {selectedUnitIds.map(id=>{const u=s?.units.find(u=>u.id===id);if(!u)return null;const p=worldToScreenV6({x:u.x,y:u.y,z:v6UnitElevation(u)},camera,size);return <ellipse key={id} cx={p.x} cy={p.y} rx={12*camera.zoom} ry={6*camera.zoom} className={`v6-selected-ring ${u.owner===r.playerId?'':'enemy'}`}/>;})}
          {ghost&&<polygon points={polygon(ghost)} className={`v6-blueprint ${tool==='shell'&&!validShellRectV6(ghost)?'invalid':''}`}/>}
          {activeLine&&<polyline points={activeLine.map(p=>worldToScreenV6({x:p.x+.5,y:p.y+.5,z:p.z},camera,size)).map(p=>`${p.x},${p.y}`).join(' ')} className="v6-line-preview"/>}
          {unit&&unit.owner===r.playerId&&!!unit.queuedGoals?.length&&(()=>{const next=[...(unit.goal?[unit.goal]:[]),...unit.queuedGoals],points=[worldToScreenV6({x:unit.x,y:unit.y,z:v6UnitElevation(unit)},camera,size),...next.map(p=>worldToScreenV6({x:p.x+.5,y:p.y+.5,z:p.z+(unit.altitude??0)},camera,size))];return <g className="v6-unit-goals"><polyline points={points.map(p=>`${p.x},${p.y}`).join(' ')}/>{points.slice(1).map((p,i)=><g key={i} transform={`translate(${p.x},${p.y})`}><circle r={6}/><text x={9} y={3}>{i+1}</text></g>)}</g>;})()}
          {wirePorts.map(port=><g key={port.selection.kind+port.selection.id} className={`v6-map-port ${tool} ${port.online?'online':''}`} transform={`translate(${port.screen.x},${port.screen.y})`}><circle r={7}/><path d="M-11,0h22 M0,-11v22"/>{port.selection.id===selectedEntity?.id&&<text x={12} y={-9}>#{port.selection.id}</text>}</g>)}
          {tool==='skill'&&unit&&skillTarget&&unitScreen&&(()=>{const target=worldToScreenV6(skillTarget,camera,size);return <g className={`v6-skill-target shape-${entityCard?.skillShape??'target'} ${skillInRange?'':'invalid'}`}><line x1={unitScreen.x} y1={unitScreen.y} x2={target.x} y2={target.y}/><polygon points={skillPoints.map(p=>`${p.x},${p.y}`).join(' ')}/><path d={`M${target.x-12},${target.y}h24 M${target.x},${target.y-12}v24`}/></g>;})()}
          {gesture?.kind==='select'&&tool==='select'&&<rect x={Math.min(gesture.start.x,gesture.end.x)} y={Math.min(gesture.start.y,gesture.end.y)} width={Math.abs(gesture.end.x-gesture.start.x)} height={Math.abs(gesture.end.y-gesture.start.y)} className="v6-drag-select"/>}
        </svg>
        {rectangle&&<div className="v6-dimension-label" style={{left:gesture!.end.x+16,top:gesture!.end.y+12}}>{rectangle.w} × {rectangle.h}<small>{rectangle.w*rectangle.h} 微格</small></div>}
      </div>
    </div>
    {!lobby&&<>
      <header className="v6-topbar"><button className="v6-brand" aria-label="返回行动大厅" onClick={()=>setLobby(true)}><span className="v6-brand-symbol">⌘</span><div><b>编译防线</b><small>FLAT FRONT / LIVE</small></div></button><div className="v6-resource-strip">
        <button title="采集器每秒直接结算经费；AI维护另外支出" onClick={()=>setPanel('resources')}><Coins/><div><b>{formatV6(player?.credits)}</b><small>+{rateV6(player?.income)}/s · 持续收益{aiEconomy.ready&&aiEconomy.count>0&&<> / AI −{rateV6(aiEconomy.upkeep)}/s</>}</small></div></button>
        {!classic&&<button onClick={()=>setPanel('resources')}><Cpu/><div><b>{formatV6(player?.compute)}</b><small>+{rateV6(player?.production)}/s · 算力</small></div></button>}
        {!classic&&<button className={(player?.power??0)<(player?.demand??0)?'shortage':''} onClick={()=>{setPanel('resources');setOverlay('power');}}><Zap/><div><b>{formatV6(player?.power)}<em> / {formatV6(player?.demand)}</em></b><small>电力 / 负载</small></div></button>}
        <button onClick={()=>setPanel('tech')}>{classic?<Swords/>:<FlaskConical/>}<div><b>{classic?`T${Math.max(1,...Object.values(player?.branches??{}))}`:formatV6(player?.science)}</b><small>{classic?'科技等级':'科研数据'}</small></div></button>
      </div><div className="v6-top-actions"><span>{timeV6(s?.tick??0)}</span><button className="v6-icon" aria-label="保存行动" onClick={async()=>{const result=await r.action('save',{name:`行动 ${timeV6(s?.tick??0)}`});setSaveNotice(result?'行动已保存':'保存未完成');}}><Save size={18}/></button><button className="v6-icon" aria-label="指挥手册" onClick={()=>setPanel('help')}><CircleHelp size={18}/></button><button className="v6-icon" aria-label="对战房间" onClick={()=>setLobby(true)}><Radio size={18}/></button></div></header>
      {s&&<V6Objective state={s} owner={r.playerId} coreHp={core?.hp??0} catalog={r.catalog} onLocate={node=>focus(node.pos)}/>}
      {tutorial&&<aside className="v6-tutorial"><button className="v6-icon" aria-label="收起建设指引" onClick={()=>setTutorial(false)}><X size={13}/></button><span className="v6-eyebrow">BUILD YOUR NETWORK</span><ol>{tutorialSteps.map(([text,done],i)=><li key={text} className={done?'done':''}>{done?<Check size={12}/>:<b>{i+1}</b>}<span>{text}</span></li>)}</ol></aside>}
      <aside className="v6-tool-rail">{([['select',MousePointer2,'选择','Esc'],['move',Move,'移动','M'],['build',Warehouse,'露天设施','B'],['deploy',Swords,'部署单位','U'],['power',Zap,'电力线','L'],['compute',Link2,'算力线','C'],['wall',Shield,'防御墙','T']] as const).filter(([id])=>classic?!['power','compute'].includes(id):!['build','deploy'].includes(id)).map(([id,Icon,name,key])=><button key={id} className={tool===id?'active':''} aria-label={name} title={name+(key?` ${key}`:'')} onClick={()=>{if(id==='build'||id==='deploy'){setDeck(id==='build'?'construction':'units');setDeckOpen(true);}activate(id);}}><Icon size={21}/>{key&&<kbd>{key}</kbd>}</button>)}<button aria-label="科技树" onClick={()=>setPanel('tech')}><FlaskConical size={20}/></button></aside>
      {s&&<aside className="v6-minimap"><header><span>TACTICAL MAP</span><button aria-label="回到基地" onClick={()=>core&&focus(v6EntityPosition(core))}><Home size={13}/></button></header><MiniMap state={s} owner={r.playerId} camera={camera} onMove={focus}/><footer>{classic?<span>单层塔防</span>:<button onClick={()=>setOverlay(old=>old==='none'?'power':old==='power'?'compute':'none')}>覆盖 / {overlay==='power'?'电力':overlay==='compute'?'算力':'关闭'}</button>}<div><button aria-label="缩小" onClick={()=>setCamera(c=>clampCameraV6({...c,zoom:c.zoom/1.2}))}>−</button><span>{camera.zoom.toFixed(1)}×</span><button aria-label="放大" onClick={()=>setCamera(c=>clampCameraV6({...c,zoom:c.zoom*1.2}))}>+</button></div></footer></aside>}
      {selectedEntity&&<aside className="v6-inspector"><header><div><small>{selection?.kind.toUpperCase()} / #{selectedEntity.id}</small><h2>{selectedName}</h2></div><button className="v6-icon" aria-label="取消选择" onClick={()=>setSelected([])}><X size={15}/></button></header>{entityCard&&<div className="v6-inspector-art"><img src={v6CardImage(entityCard)} alt=""/></div>}
        {'hp'in selectedEntity&&<><div className="v6-health"><i style={{width:`${'maxHp'in selectedEntity?selectedEntity.hp/Number(selectedEntity.maxHp)*100:100}%`}}/></div><div className="v6-stat-line"><span>结构完整度</span><b>{formatV6(selectedHealth)}{'maxHp'in selectedEntity?' / '+formatV6(Number(selectedEntity.maxHp)):''}</b></div></>}
        {selectedRect&&<div className="v6-stat-line"><span>{selectedRect.w} × {selectedRect.h}</span><b>{selectedRect.w*selectedRect.h} 微格</b></div>}
        {selectedShield&&<div className="v6-stat-line"><span>CUDA 围合护盾</span><b className={selectedShield.current>0?'good':'bad'}>{formatV6(selectedShield.current)} / {formatV6(selectedShield.capacity)}</b></div>}
        {s?.jobs?.find(j=>j.target===selectedEntity.id)?.blocked&&<p className="v6-unit-hint">施工人员无法到达 · 检查通路是否被建筑或残骸阻挡。</p>}
        {resource&&(resource.kind==='node'?<><div className="v6-stat-line"><span>战略控制权</span><b className={resource.contested?'bad':resource.owner===r.playerId?'good':'muted'}>{resource.contested?'正在争夺':resource.owner===r.playerId?'己方控制':resource.owner?'敌方控制':'中立'}</b></div>{resource.capture>0&&<div className="v6-stat-line"><span>{resource.capturer===r.playerId?'己方占领':'敌方占领'}进度</span><b>{formatV6(resource.capture)} / 20s</b></div>}<p className="v6-muted">控制节点可获得持续经费与科研数据。地面机动单位驻留可夺取，固定炮台不能占领。</p><p className="v6-muted">18分钟后，至少控制两处节点并累计压制360秒取胜；争夺时暂停，失去多数后每秒回退2秒。</p></>:<><div className="v6-stat-line"><span>可开采储量</span><b>{resource.remaining<0?'尚未侦察':resource.remaining<=0?'已耗尽':formatV6(resource.remaining)}</b></div><p className="v6-muted">{resource.kind==='coal'?'煤层旁建造采集器，采出量每秒直接结算为经费。':'建造采集器开采，矿物每秒直接兑换经费。摧毁采集器可切断对方收入。'}</p></>)}
        {(room||building)&&s&&<V6FacilityStatus entity={(room??building)!} state={s} items={r.catalog.items} owned={(room??building)!.owner===r.playerId} classic={classic} onWire={kind=>{
          const p=v6EntityPosition((room??building)!);activate(kind);setWireStart({x:Math.floor(p.x),y:Math.floor(p.y),z:0});r.setNotice('起点已选，点击目标设施端口；两端顺序不影响接通。');
        }}/>}
        {(room||building)&&inspectorPreview&&!classic&&<V6UtilizationCard preview={inspectorPreview} mode="inspector"/>}
        {(room||building)&&<>{(room??building)!.progress<1&&<><div className="v6-stat-line"><span>施工进度</span><b>{Math.floor((room??building)!.progress*100)}%</b></div><button className="v6-text-action" onClick={()=>void send({op:'cancel',id:(room??building)!.id})}>取消未完工施工<X size={13}/></button></>}</>}
        {!classic&&room?.kind==='data-center'&&<V6DataCenterRacks room={room} owned={room.owner===r.playerId} canOrder={r.connected&&!r.busy&&!lobby&&!r.session?.replay&&!s?.winner} sessionId={r.session?.roomId} tickSeconds={Math.floor((s?.tick??0)/60)} onInstall={()=>{setDeck('gpus');setDeckOpen(true);}} onOrder={send}/>}
        {unit&&<V6UnitStatus unit={unit} item={entityCard} owned={unit.owner===r.playerId} catalog={r.catalog.items} onSkill={()=>activate('skill')} onPlugins={()=>{setDeck('plugins');setDeckOpen(true);}} onMove={()=>activate('move')} onStop={()=>void send({op:'stop',ids:selectedUnits})} onReturn={()=>{
          const runway=s?.buildings.filter(b=>b.owner===r.playerId&&b.kind==='airstrip'&&b.hp>0&&b.progress>=1&&b.powered).sort((a,b)=>Math.hypot(a.rect.x-unit.x,a.rect.y-unit.y)-Math.hypot(b.rect.x-unit.x,b.rect.y-unit.y))[0];
          if(!runway){r.setNotice('没有可用的己方机场跑道，请先建成并供电。');return;}
          const p=v6EntityPosition(runway);void send({op:'move',ids:[unit.id],pos:{x:Math.floor(p.x),y:Math.floor(p.y),z:0}});
        }}/>}
        {classic&&building?.kind==='core'&&building.owner===r.playerId&&<><p className="v6-muted">在核心研究科技分支，只消耗经费。已完工的己方建筑同样是 12 格部署锚点。</p><button className="v6-primary" onClick={()=>setPanel('tech')}>发展科技<FlaskConical size={16}/></button><button className="v6-text-action" onClick={()=>{setDeck('units');setDeckOpen(true);activate('deploy');}}>部署防御单位<Swords size={14}/></button></>}
        {!classic&&room?.kind==='research-lab'&&<button className="v6-primary" onClick={()=>setPanel('tech')}>发展科技<FlaskConical size={16}/></button>}
        {!classic&&room&&<button className="v6-text-action" onClick={()=>setSelected([{kind:'building',id:room.shell}])}>选择所在楼体<Layers3 size={14}/></button>}
        {!classic&&building?.kind==='shell'&&<><button className="v6-primary" onClick={()=>{setDeck('construction');setDeckOpen(true);activate('room');}}>划分功能房间<Warehouse size={16}/></button><button className="v6-text-action" onClick={()=>activate('expand')}>拖拽扩建毛坯<Expand size={14}/></button></>}
        {!classic&&room&&room.owner===r.playerId&&room.progress>=1&&<details className="v6-room-tools"><summary>房间改造与分割<ChevronDown size={13}/></summary><div><select aria-label="分割方向" value={splitAxis} onChange={e=>setSplitAxis(e.target.value as 'x'|'y')}><option value="x">沿长边坐标分割</option><option value="y">沿宽边坐标分割</option></select><input aria-label="分割位置格数" type="number" min={2} max={(splitAxis==='x'?room.rect.w:room.rect.h)-2} value={splitOffset} onChange={e=>setSplitOffset(Number(e.target.value))}/><button onClick={()=>void send({op:'split-room',id:room.id,axis:splitAxis,offset:splitOffset})}>分割</button><button onClick={()=>{activate('merge');r.setNotice('点击另一个相邻的同用途房间，合并后资源和生命总量保持一致。');}}>选择相邻房间合并</button><select aria-label="改造后的房间用途" value={selectedRoomKind} onChange={e=>setSelectedRoomKind(e.target.value)}>{r.catalog.items.filter(c=>c.category==='rooms').map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select>{selectedRoomKind==='research-lab'&&<select aria-label="改造研究所方向" value={branch} onChange={e=>setBranch(e.target.value)}>{V6_BRANCHES.map(b=><option key={b.id} value={b.id}>{b.name}</option>)}</select>}<button onClick={()=>void send({op:'convert-room',id:room.id,kind:selectedRoomKind,...(selectedRoomKind==='research-lab'?{branch}:{})})}>支付改造费用并施工</button></div></details>}
        {selection?.kind==='rubble'&&<><p className="v6-muted">清理残骸恢复道路，并回收可用材料。</p><button className="v6-primary" onClick={()=>void send({op:'clear-rubble',id:selectedEntity.id})}>清理并回收<Shovel size={16}/></button></>}
        {selectedEntity.owner===r.playerId&&'hp'in selectedEntity&&<div className="v6-inspector-actions">{(unit||building)&&<button onClick={()=>void send({op:'upgrade',id:selectedEntity.id})}><ArrowUp size={14}/>{unit?'升级单位':'强化结构'}</button>}<button onClick={()=>void send({op:'repair',id:selectedEntity.id})}><Wrench size={14}/>维修</button>{building?.kind!=='core'&&<button onClick={()=>void send({op:'recycle',id:selectedEntity.id})}><Box size={14}/>回收</button>}</div>}
      </aside>}
      {tool!=='select'&&<div className="v6-tool-banner">
        <span>{TOOL_LABELS[tool]}</span>
        {tool==='room'&&selectedRoomKind==='research-lab'&&<select aria-label="新研究所科技方向" value={branch} onChange={e=>setBranch(e.target.value)}>{V6_BRANCHES.map(b=><option key={b.id} value={b.id}>{b.name}</option>)}</select>}
        {tool==='wall'&&(classic?<span>实体墙</span>:<select aria-label="墙体类型" value={wallKind} onChange={e=>setWallKind(e.target.value)}><option value="physical">实体墙</option><option value="cuda">CUDA 数据墙</option><option value="moat">CUDA 护城河</option></select>)}
        <small>{card&&['room','build','deploy'].includes(tool)?card.name:wireStart?'起点已选 · 点击目标端口':'在地图上操作'}</small><button aria-label="取消当前工具" onClick={cancel}><X size={14}/><kbd>Esc</kbd></button>
      </div>}
      {preview&&<div className={`v6-cost-preview ${preview.valid?'':'invalid'}`}><span>{preview.valid?(tool==='deploy'?'可部署':'可施工'):preview.reason}</span>{preview.valid&&<><b>◈ {formatV6(preview.cost?.credits)}</b>{preview.cost?.compute>0&&<b>算力 {formatV6(preview.cost.compute)}</b>}{(['power','compute'].includes(tool)||preview.completionProjection)&&<small>{preview.completionProjection?'预计竣工后':'接线后'}电力 {formatV6(preview.powerAfter)} / {formatV6(preview.demandAfter)}</small>}{preview.targetPowered!==undefined&&<small>{preview.targetPowered?'目标将通电':'目标仍未通电'}</small>}{preview.targetConnected!==undefined&&<small>{preview.targetConnected?'目标将接通算力':'目标尚未接通算力'}</small>}</>}<V6UtilizationCard preview={preview} mode="preview"/></div>}
      {r.session?.replay&&<div className="v6-replay-controls">
        <span>行动回放 · {timeV6(seekDraft??s?.tick??0)}</span>
        <button onClick={async()=>{await r.action('replay-control',{seekTick:0});}}>回到开始</button>
        <button onClick={async()=>{await r.action('replay-control',{seekTick:Math.max(0,(s?.tick??0)-600)});}}>后退10秒</button>
        <button onClick={async()=>{const paused=!(s?.playback?.paused??replayPaused);if(await r.action('replay-control',{paused}))setReplayPaused(paused);}}>{(s?.playback?.paused??replayPaused)?'继续':'暂停'}</button>
        <select aria-label="回放速度" value={s?.playback?.speed??replaySpeed} onChange={async e=>{const speed=Number(e.target.value);if(await r.action('replay-control',{speed}))setReplaySpeed(speed);}}>{[.5,1,2,4,8].map(v=><option key={v} value={v}>{v}×</option>)}</select>
        {s?.playback&&<input aria-label="回放时间轴" type="range" min={0} max={s.playback.totalTicks} value={seekDraft??s.tick} onChange={e=>setSeekDraft(Number(e.target.value))} onPointerUp={async e=>{const seekTick=Number(e.currentTarget.value);await r.action('replay-control',{seekTick});setSeekDraft(null);}} onKeyUp={async e=>{if(['Enter','ArrowLeft','ArrowRight','Home','End'].includes(e.key)){await r.action('replay-control',{seekTick:Number(e.currentTarget.value)});setSeekDraft(null);}}}/>}
      </div>}
      <footer className={`v6-command-deck ${deckOpen?'expanded':'collapsed'}`}>
        <header><div>{([['construction',classic?'露天设施':'建筑',Warehouse,'B'],['units','战斗单位',Swords,'U'],['gpus','显卡',Cpu,'I'],['plugins','插件',FlaskConical,'']] as const).filter(([id])=>!classic||id!=='gpus').map(([id,name,Icon,key])=><button key={id} aria-pressed={deck===id} onClick={()=>{setDeck(id);setDeckOpen(true);}}><Icon size={16}/>{name}{key&&<kbd>{key}</kbd>}</button>)}</div>
          {deckOpen&&<div className="v6-deck-filters">
            <select aria-label="牌组科技方向" value={deckBranch} onChange={e=>setDeckBranch(e.target.value)}><option value="all">全部方向</option>{V6_BRANCHES.map(b=><option key={b.id} value={b.id}>{b.name}</option>)}</select>
            {deck==='units'&&<select aria-label="单位兵种筛选" value={roleFilter} onChange={e=>setRoleFilter(e.target.value)}><option value="all">全部兵种</option><option value="ai">AI角色</option><option value="turret">固定炮台</option><option value="vehicle">地面载具</option><option value="air">空中力量</option>{!classic&&<option value="orbital">天基武器</option>}</select>}
          </div>}
          <button className="v6-icon" aria-label={deckOpen?'收起牌组':'展开牌组'} onClick={()=>setDeckOpen(!deckOpen)}>{deckOpen?<ChevronDown size={17}/>:<ChevronUp size={17}/>}</button>
        </header>
        {deckOpen&&<div className="v6-deck-scroll">
          {deck==='construction'&&!classic&&<button className={`v6-foundation-card ${tool==='shell'?'selected':''}`} onClick={()=>activate('shell')}><Layers3 size={38}/><strong>毛坯框架</strong><small>自由拖拽长宽<br/>4—24格 · 可相邻扩建</small><span>先筑结构，再赋予用途</span></button>}
          {items.map((item,index)=><V6Card key={item.category+item.id} item={item} player={player} index={index} selected={card?.id===item.id} prerequisite={item.category==='units'&&!initialLab?'先建研究所':item.category==='plugins'?v6PluginLock(item,unit,r.catalog.items,r.playerId):undefined} onClick={()=>selectCard(item)}/>)}
        </div>}
      </footer>
      {r.session?.mode==='solo'&&!r.session.replay&&<button className={`v6-pause-control ${r.session.paused?'paused':''}`} aria-label={r.session.paused?'继续战斗':'暂停战斗'} onClick={()=>void r.action('pause',{paused:!r.session?.paused,sessionId:r.session?.roomId})}>{r.session.paused?<Play size={16}/>:<Pause size={16}/>}<kbd>Space</kbd></button>}
      <div className="v6-notice" role="status">{saveNotice||(r.session?.paused?(r.notice.startsWith('战斗已暂停，')?r.notice:'战斗已暂停 · 可以查看科技与资源，按空格继续。'):r.notice)}<small>{r.connected?`${r.fps.toFixed(0)} FPS`:'连接中断'}</small></div>
      {r.error&&<div className="v6-error" role="alert">{r.error}<button onClick={r.reconnect}>恢复连接</button></div>}
      {!r.streaming&&!r.error&&<div className="v6-loading"><Radio/><strong>正在接入战场画面</strong><small>保留当前行动，等待本机渲染器</small></div>}
      {s?.winner!==null&&s?.winner!==undefined&&<div className="v6-result"><span>OPERATION COMPLETE</span><h1>{v6OutcomeTitle(s,r.playerId)}</h1><p>{s.winReason}</p><button className="v6-primary" onClick={()=>setLobby(true)}>返回行动大厅<ArrowUp size={16}/></button></div>}
    </>}
    {!lobby&&panel&&<div className="v6-panel-scrim" onPointerDown={()=>setPanel(null)}><section className={`v6-panel panel-${panel}`} role="dialog" aria-modal="true" aria-label={panel==='tech'?'科技树':panel==='resources'?'资源网络':'指挥手册'} onPointerDown={e=>e.stopPropagation()}><header><div><span className="v6-eyebrow">COMMAND / {panel.toUpperCase()}</span><h2>{panel==='tech'?'选择你的技术优势':panel==='resources'?'每一份资源，都有来处':'指挥手册'}</h2></div><button className="v6-icon" aria-label="关闭面板" onClick={()=>setPanel(null)}><X/></button></header>
      {panel==='tech'?<V6TechnologyPanel player={player} rooms={ownRooms} state={s} catalog={r.catalog} classic={classic} onResearch={(lab,branch)=>void send({op:'research',room:lab?.id??0,branch})} onLocate={lab=>{focus(v6EntityPosition(lab));setSelected([{kind:'room',id:lab.id}]);setPanel(null);}}/>:panel==='resources'?<div className="v6-resource-panel"><div className="v6-resource-hero"><div><Cpu size={38}/><h3>算力与供电拓扑</h3><p>总发电量不等于每个机房都接通。选择下方设施，查看它接入的电网与算力网络。</p><b>{formatV6(player?.compute)} <small>/ {formatV6(player?.computeCapacity)} 算力储量</small></b></div><button className="v6-primary" onClick={()=>{setOverlay('power');setPanel(null);activate('power');}}>绘制电力线路 <kbd>L</kbd></button><button className="v6-primary" onClick={()=>{setOverlay('compute');setPanel(null);activate('compute');}}>绘制算力线路 <kbd>C</kbd></button></div><V6CudaControl enabled={s?.shieldAuto?.[r.playerId-1]} regions={s?.shieldRegions?.filter(q=>q.owner===r.playerId)??[]} onToggle={enabled=>void send({op:'shield',enabled})}/><div className="v6-resource-rows">{[...(s?.buildings.filter(b=>b.owner===r.playerId&&b.kind!=='shell')??[]),...ownRooms].map(e=><button key={e.id} onClick={()=>{focus(v6EntityPosition(e));setSelected([{kind:'shell'in e?'room':'building',id:e.id}]);setPanel(null);}}><span><b>{r.catalog.items.find(c=>c.id===e.kind)?.name??e.kind}</b><small>{e.rect.x},{e.rect.y} · {e.rect.w}×{e.rect.h}</small></span><span className={e.powered?'good':'bad'}>{e.powered?'供电正常':'未供电'}</span><span className={e.connected?'good':'muted'}>{e.connected?'算力在线':'未接算力'}</span><ChevronRight size={14}/></button>)}</div></div>:<V6Guide catalog={r.catalog} classic={classic}/>}
    </section></div>}
    {lobby&&<V6Lobby session={r.session} snapshot={s} playerId={r.playerId} catalog={r.catalog} busy={r.busy} error={r.error} connected={r.connected} onBegin={r.begin} onContinue={()=>setLobby(false)} onAction={r.action} onRefresh={()=>void r.refresh()}/>}
  </main>;
}
