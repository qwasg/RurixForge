/** Browser-event/DTO fixtures only: no native pixels or simulation are substituted as acceptance. */
import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,beforeEach,describe,expect,it,vi} from 'vitest';
import CodeSentinelsV6 from '../src/views/CodeSentinelsV6';
import type {useSentinelsV6} from '../src/lib/useSentinelsV6';
import type {V6Building,V6Pick,V6Snapshot,V6Unit} from '../src/lib/sentinelsV6';
const mock=vi.hoisted(()=>({current:null as unknown,request:vi.fn()}));
vi.mock('../src/lib/useSentinelsV6',()=>({useSentinelsV6:()=>mock.current,v6Request:mock.request}));
function fixture():V6Snapshot{return{version:6,revision:1,tick:0,seed:11,width:128,height:96,minLevel:-2,maxLevel:5,players:[1,2].map(owner=>({owner,credits:2000,compute:0,computeCapacity:0,power:0,demand:0,income:.35,production:0,branches:{speed:1},research:null,dominance:0,ai:owner===2,lostValue:0})),terrain:Array(128*96).fill(0),excavated:[],buildings:[],rooms:[],entrances:[],units:[],links:[],walls:[],resources:[],shipments:[{id:50,owner:1,from:10,to:20,pos:{x:20,y:48,z:0},route:[{x:21,y:48,z:0}],amount:40,hp:120,progress:0,cargo:'ammo',waypoints:[],manualRoute:false}],projectiles:[],events:[],winner:null,winReason:'',explored:[[],[]],visible:[[],[]]};}
let runtime:ReturnType<typeof useSentinelsV6>;
beforeEach(()=>{
  vi.stubGlobal('PointerEvent',MouseEvent);
  const context={clearRect:vi.fn(),fillRect:vi.fn(),beginPath:vi.fn(),arc:vi.fn(),fill:vi.fn(),strokeRect:vi.fn()};
  vi.spyOn(HTMLCanvasElement.prototype,'getContext').mockReturnValue(context as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLElement.prototype,'getBoundingClientRect').mockReturnValue({x:0,y:0,left:0,top:0,right:1280,bottom:720,width:1280,height:720,toJSON:()=>({})});
  Object.defineProperty(HTMLElement.prototype,'setPointerCapture',{value:vi.fn(),configurable:true});
  Object.defineProperty(HTMLElement.prototype,'hasPointerCapture',{value:()=>false,configurable:true});
  const snapshot=fixture(),session={mode:'solo' as const,status:'battle' as const,roomId:'fixture-map',code:'',address:'',players:[],lastSequence:0};
  runtime={status:{protocol:'code-sentinels-pvp/6.1',playerId:1,session,snapshot},snapshot,session,playerId:1,catalog:{items:[]},connected:true,streaming:true,busy:false,error:'',notice:'',fps:0,receipt:null,canvasRef:{current:null},begin:vi.fn(async()=>true),action:vi.fn(async()=>({ok:true})),order:vi.fn(async()=>({accepted:true,sequence:1,tick:0,reason:'fixture accepted'})),camera:vi.fn(async()=>undefined),refresh:vi.fn(async()=>undefined),setNotice:vi.fn(),reconnect:vi.fn()} as ReturnType<typeof useSentinelsV6>;
  mock.current=runtime;mock.request.mockReset();mock.request.mockImplementation(async(name:string)=>name==='pick'?{kind:'shipment',id:50,owner:1,pos:{x:20,y:48,z:0}}:null);
});
afterEach(()=>{cleanup();vi.unstubAllGlobals();vi.restoreAllMocks();delete (HTMLElement.prototype as unknown as Record<string,unknown>).setPointerCapture;delete (HTMLElement.prototype as unknown as Record<string,unknown>).hasPointerCapture;});
function open(){render(<CodeSentinelsV6/>);fireEvent.click(screen.getByRole('button',{name:'关闭大厅返回战场'}));return screen.getByLabelText('原生多层战场').parentElement!;}
function clickMap(stage:HTMLElement,x=640,y=360){fireEvent.pointerDown(stage,{button:0,clientX:x,clientY:y});fireEvent.pointerUp(stage,{button:0,clientX:x,clientY:y});}
function selectedUnit(owner:number):V6Unit{return{id:60,owner,kind:'vscode',pos:{x:20,y:48,z:0},x:20.5,y:48.5,z:0,tier:1,hp:300,maxHp:300,battery:0,batteryMax:0,covered:false,wired:false,ammo:0,route:[],target:null,cooldown:0,skillCooldown:0,plugins:[],statuses:{},invested:80,moving:false,attackCount:0};}
describe('V6 actual map event handling',()=>{
  it('sends the target floor actually displayed after switching to the second floor',async()=>{
    const stage=open();fireEvent.keyDown(window,{key:'PageUp'});
    fireEvent.click(screen.getByRole('button',{name:'跨层通道'}));
    fireEvent.change(screen.getByLabelText('跨层入口类型'),{target:{value:'stairs'}});
    expect(screen.getByLabelText('入口目标楼层')).toHaveValue('0');
    clickMap(stage);
    await waitFor(()=>expect(runtime.order).toHaveBeenCalledWith({op:'entrance',pos:{x:20,y:48,z:1},toLevel:0,kind:'stairs',width:1}));
  });
  it('binds a wire only to the explicitly selected AI endpoint',async()=>{
    runtime.snapshot!.units=[{...selectedUnit(1),kind:'kimi'}];
    runtime.catalog.items=[{id:'kimi',kind:'kimi',name:'Kimi',category:'units',role:'ai',tier:2,cost:780,description:''}];
    const stage=open();fireEvent.keyDown(window,{key:'c'});
    mock.request.mockResolvedValueOnce({kind:'unit',id:60,owner:1,pos:{x:20,y:48,z:0}});
    clickMap(stage,640,350);await waitFor(()=>expect(runtime.setNotice).toHaveBeenCalledWith('起点已选，点击目标端口完成连接。'));
    mock.request.mockResolvedValueOnce(null);clickMap(stage,735,420);
    await waitFor(()=>expect(runtime.order).toHaveBeenCalledWith(expect.objectContaining({op:'wire',kind:'compute',unitEndpoints:[60]})));
    const command=vi.mocked(runtime.order).mock.calls[0][0];if(command.op==='wire')expect(command.path[0]).toEqual({x:20,y:48,z:0});
  });
  it('connects visible room ports instead of letting the enclosing shell steal the click',async()=>{
    runtime.snapshot!.buildings=[{id:10,owner:1,kind:'wind-power',rect:{x:20,y:40,z:0,w:2,h:2},tier:1,hp:700,maxHp:700,progress:1,buildTime:5,powered:true,connected:false,power:150,demand:0,capacity:0,branch:null,inventory:0,invested:160,jam:0,shield:0,born:0}];
    runtime.snapshot!.rooms=[{id:20,owner:1,shell:30,kind:'data-center',rect:{x:24,y:48,z:0,w:2,h:2},tier:1,hp:300,maxHp:300,progress:1,buildTime:5,powered:false,connected:false,branch:null,inventory:0,capacity:1,gpus:[],cooldown:0}];
    mock.request.mockResolvedValue({kind:'building',id:30,owner:1,pos:{x:26,y:49,z:0}});
    const stage=open();fireEvent.keyDown(window,{key:'l'});
    clickMap(stage,748,326);await waitFor(()=>expect(runtime.setNotice).toHaveBeenCalledWith('起点已选，点击目标端口完成连接。'));
    clickMap(stage,694,407);await waitFor(()=>expect(runtime.order).toHaveBeenCalledOnce());
    const command=vi.mocked(runtime.order).mock.calls[0][0];expect(command.op).toBe('wire');
    if(command.op==='wire'){expect(command.path[0]).toEqual({x:21,y:41,z:0});expect(command.path.at(-1)).toEqual({x:25,y:49,z:0});}
    expect(mock.request.mock.calls.some(([name])=>name==='pick')).toBe(false);
  });
  it('keeps a free cable waypoint inside a shell at the clicked microcell',async()=>{
    runtime.snapshot!.buildings=[{id:30,owner:1,kind:'shell',rect:{x:23,y:47,z:0,w:6,h:4},tier:1,hp:700,maxHp:700,progress:1,buildTime:5,powered:false,connected:false,power:0,demand:0,capacity:0,branch:null,inventory:0,invested:160,jam:0,shield:0,born:0}];
    mock.request.mockResolvedValue({kind:'building',id:30,owner:1,pos:{x:26,y:49,z:0}});
    const stage=open();fireEvent.keyDown(window,{key:'l'});clickMap(stage,640,360);
    await waitFor(()=>expect(runtime.setNotice).toHaveBeenCalledWith('起点已选，点击目标端口完成连接。'));
    clickMap(stage,735,420);await waitFor(()=>expect(runtime.order).toHaveBeenCalledOnce());
    const command=vi.mocked(runtime.order).mock.calls[0][0];
    if(command.op==='wire')expect(command.path.at(-1)).toEqual({x:27,y:48,z:0});else throw Error('Expected actual wire command');
  });
  it('distinguishes Shift queued movement from an ordinary replacing move',async()=>{
    runtime.snapshot!.units=[selectedUnit(1)];mock.request.mockResolvedValueOnce({kind:'unit',id:60,owner:1,pos:{x:20,y:48,z:0}});
    const stage=open();clickMap(stage);await screen.findByRole('button',{name:'停止指令'});
    mock.request.mockResolvedValueOnce(null);fireEvent.pointerDown(stage,{button:2,shiftKey:true,clientX:700,clientY:390});
    await waitFor(()=>expect(runtime.order).toHaveBeenCalledWith({op:'queue-move',ids:[60],pos:{x:24,y:48,z:0}}));
    mock.request.mockResolvedValueOnce(null);fireEvent.pointerDown(stage,{button:2,clientX:700,clientY:420});
    await waitFor(()=>expect(runtime.order).toHaveBeenCalledWith({op:'move',ids:[60],pos:{x:26,y:50,z:0}}));
    expect(runtime.snapshot!.units[0].route).toEqual([]);
  });
  it('never sends movement commands for a selected enemy unit',async()=>{
    runtime.snapshot!.units=[selectedUnit(2)];mock.request.mockResolvedValueOnce({kind:'unit',id:60,owner:2,pos:{x:20,y:48,z:0}});
    const stage=open();clickMap(stage);await waitFor(()=>expect(screen.getByRole('heading',{name:'vscode'})).toBeVisible());
    mock.request.mockResolvedValueOnce(null);fireEvent.pointerDown(stage,{button:2,clientX:700,clientY:390});
    await waitFor(()=>expect(runtime.setNotice).toHaveBeenCalledWith('已取消当前工具'));expect(runtime.order).not.toHaveBeenCalled();
  });
  it('sends air cargo only after picking two actual powered own runways',async()=>{
    runtime.snapshot!.buildings=[10,20].map(id=>({id,owner:1,kind:'airstrip',rect:{x:id===10?20:35,y:44,z:0,w:8,h:4},tier:2,hp:700,maxHp:700,progress:1,buildTime:5,powered:true,connected:false,power:0,demand:20,capacity:0,branch:null,inventory:200,invested:360,jam:0,shield:0,born:0,stock:{ammo:80,fuel:120}} as V6Building));
    const stage=open();fireEvent.click(screen.getByRole('button',{name:'补给'}));fireEvent.change(screen.getByLabelText('补给运输方式'),{target:{value:'air'}});
    mock.request.mockResolvedValueOnce({kind:'building',id:10,owner:1,pos:{x:24,y:46,z:0}});
    clickMap(stage);await waitFor(()=>expect(runtime.setNotice).toHaveBeenCalledWith('货源已选，点击补给目的地。'));
    mock.request.mockResolvedValueOnce({kind:'building',id:20,owner:1,pos:{x:39,y:46,z:0}});
    clickMap(stage);await waitFor(()=>expect(runtime.order).toHaveBeenCalledWith({op:'supply',from:10,to:20,amount:20,cargo:'ammo',mode:'air'}));
  });
  it('rejects an unsupported air origin before requesting paid dispatch',async()=>{
    const stage=open();fireEvent.click(screen.getByRole('button',{name:'补给'}));fireEvent.change(screen.getByLabelText('补给运输方式'),{target:{value:'air'}});
    clickMap(stage);await waitFor(()=>expect(runtime.setNotice).toHaveBeenCalledWith(expect.stringContaining('空运两端都需要')));
    expect(runtime.order).not.toHaveBeenCalled();
  });
  it('submits ordered user waypoints once on Enter and removes the last point with Backspace',async()=>{
    const stage=open();clickMap(stage);fireEvent.click(await screen.findByRole('button',{name:'指定运输途经点'}));
    clickMap(stage,700,360);clickMap(stage,700,390);fireEvent.keyDown(window,{key:'Backspace'});clickMap(stage,700,420);
    fireEvent.keyDown(window,{key:'Enter'});
    await waitFor(()=>expect(runtime.order).toHaveBeenCalledOnce());
    expect(runtime.order).toHaveBeenCalledWith({op:'reroute-shipment',id:50,waypoints:[{x:22,y:45,z:0},{x:26,y:50,z:0}]});
    expect(screen.queryByText('编辑运输航点')).not.toBeInTheDocument();
  });
  it('cancels draft routes without sending a payment or moving the shipment speculatively',async()=>{
    const stage=open();clickMap(stage);fireEvent.click(await screen.findByRole('button',{name:'指定运输途经点'}));clickMap(stage,700,360);
    fireEvent.keyDown(window,{key:'Escape'});expect(runtime.order).not.toHaveBeenCalled();expect(runtime.snapshot!.shipments[0].pos).toEqual({x:20,y:48,z:0});
  });
  it('ignores a late native pick after the player cancels the current interaction',async()=>{
    let answer:(value:V6Pick)=>void=()=>{};mock.request.mockImplementation(()=>new Promise<V6Pick>(resolve=>{answer=resolve;}));
    const stage=open();clickMap(stage);expect(mock.request).toHaveBeenCalledWith('pick',expect.objectContaining({sessionId:'fixture-map'}));
    fireEvent.keyDown(window,{key:'Escape'});await act(async()=>{answer({kind:'shipment',id:50,owner:1,pos:{x:20,y:48,z:0}});});
    expect(screen.queryByRole('button',{name:'指定运输途经点'})).not.toBeInTheDocument();
  });
});
