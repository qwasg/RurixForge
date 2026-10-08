/** Browser-event/DTO fixtures only: no native pixels or simulation are substituted as acceptance. */
import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,beforeEach,describe,expect,it,vi} from 'vitest';
import CodeSentinelsV6 from '../src/views/CodeSentinelsV6';
import type {useSentinelsV6} from '../src/lib/useSentinelsV6';
import type {V6Catalog,V6CatalogItem,V6Ruleset,V6Snapshot} from '../src/lib/sentinelsV6';
const mock=vi.hoisted(()=>({current:null as unknown,request:vi.fn()}));
vi.mock('../src/lib/useSentinelsV6',()=>({useSentinelsV6:()=>mock.current,v6Request:mock.request}));
function fixture(ruleset:V6Ruleset):V6Snapshot{return{version:6,revision:1,tick:0,seed:11,width:128,height:96,minLevel:0,maxLevel:0,ruleset,
  players:[1,2].map(owner=>({owner,credits:2000,compute:0,computeCapacity:0,power:0,demand:0,income:.35,production:0,branches:{speed:1},research:null,researches:[],dominance:0,ai:owner===2,lostValue:0,science:0})),
    terrain:Array(128*96).fill(0),
  buildings:[{id:10,owner:1,kind:'core',rect:{x:18,y:46,z:0,w:4,h:4},tier:1,hp:6000,maxHp:6000,progress:1,buildTime:0,powered:true,connected:true,power:0,demand:0,capacity:0,branch:null,inventory:500,invested:0,jam:0,shield:0,born:0}],
  rooms:[],units:[],links:[],walls:[],resources:[],projectiles:[],events:[],winner:null,winReason:'',explored:[[],[]],visible:[[],[]]};}
const CARDS:V6CatalogItem[]=[
  {id:'extractor',kind:'extractor',name:'矿物采集器',category:'buildings',role:'buildings',tier:1,cost:180,description:'',width:2,height:2},
  {id:'wind-power',kind:'wind-power',name:'风力发电机',category:'buildings',role:'buildings',tier:1,cost:160,description:'',width:2,height:2},
  {id:'data-center',kind:'data-center',name:'数据中心',category:'rooms',role:'rooms',tier:1,cost:120,description:''},
  {id:'vscode',kind:'vscode',name:'VSCode 炮台',category:'units',role:'turret',tier:1,cost:90,description:''},
  {id:'rtx-5060',kind:'rtx-5060',name:'RTX 5060',category:'gpus',role:'gpus',tier:1,cost:130,description:''},
];
const catalog:V6Catalog={items:CARDS,rules:{research:{credits:[100,350,700,1400,2400],compute:[60,150,400,900,1800],scienceData:[0,0,60,180,360],seconds:[35,60,100,160,220],labComputeUpkeep:[.5,1.5,3,6,10],additionalBranchFactor:.8,additionalBranchFromTier:3}},
    classic:{buildings:['extractor','airstrip'],unitCategories:['turret','ai','vehicle','air'],deployRadius:12}};
let runtime:ReturnType<typeof useSentinelsV6>;
function mount(ruleset:V6Ruleset){
  const snapshot=fixture(ruleset),session={mode:'solo' as const,status:'battle' as const,roomId:'fixture-classic',code:'',address:'',ruleset,players:[],lastSequence:0};
  runtime={status:{protocol:'code-sentinels-pvp/6.1',playerId:1,session,snapshot},snapshot,session,playerId:1,catalog,connected:true,streaming:true,busy:false,error:'',notice:'',fps:0,receipt:null,canvasRef:{current:null},begin:vi.fn(async()=>true),action:vi.fn(async()=>({ok:true})),order:vi.fn(async()=>({accepted:true,sequence:1,tick:0,reason:'fixture accepted'})),camera:vi.fn(async()=>undefined),refresh:vi.fn(async()=>undefined),setNotice:vi.fn(),reconnect:vi.fn()} as ReturnType<typeof useSentinelsV6>;
  mock.current=runtime;
  render(<CodeSentinelsV6/>);
  fireEvent.click(screen.getByRole('button',{name:'关闭大厅返回战场'}));
  return screen.getByLabelText('原生平面战场').parentElement!;
}
beforeEach(()=>{
    vi.stubGlobal('PointerEvent',MouseEvent);
  const context={clearRect:vi.fn(),fillRect:vi.fn(),beginPath:vi.fn(),arc:vi.fn(),fill:vi.fn(),strokeRect:vi.fn()};
  vi.spyOn(HTMLCanvasElement.prototype,'getContext').mockReturnValue(context as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLElement.prototype,'getBoundingClientRect').mockReturnValue({x:0,y:0,left:0,top:0,right:1280,bottom:720,width:1280,height:720,toJSON:()=>({})});
  Object.defineProperty(HTMLElement.prototype,'setPointerCapture',{value:vi.fn(),configurable:true});
  Object.defineProperty(HTMLElement.prototype,'hasPointerCapture',{value:()=>false,configurable:true});
  mock.request.mockReset();mock.request.mockImplementation(async()=>null);
});
afterEach(()=>{cleanup();vi.unstubAllGlobals();vi.restoreAllMocks();delete (HTMLElement.prototype as unknown as Record<string,unknown>).setPointerCapture;delete (HTMLElement.prototype as unknown as Record<string,unknown>).hasPointerCapture;});

describe('V6 classic single-layer tower defense',()=>{
    it('hides the base-building layer: shells, rooms, wiring and GPUs',()=>{
    mount('classic');
    expect(screen.queryByRole('button',{name:'电力线'})).not.toBeInTheDocument();
    expect(screen.queryByRole('button',{name:'算力线'})).not.toBeInTheDocument();
    expect(screen.queryByText('毛坯框架')).not.toBeInTheDocument();
    expect(screen.queryByRole('button',{name:/显卡/})).not.toBeInTheDocument();
    expect(screen.getByRole('button',{name:'露天设施'})).toBeVisible();
    expect(screen.getByRole('button',{name:'部署单位'})).toBeVisible();
  });
  it('keeps the full ruleset controls untouched',()=>{
      mount('full');
    expect(screen.getByRole('button',{name:'电力线'})).toBeVisible();
    expect(screen.getByRole('button',{name:'算力线'})).toBeVisible();
    expect(screen.queryByRole('button',{name:'部署单位'})).not.toBeInTheDocument();
  });
  it('offers only extractor-style outdoor cards, never rooms or GPUs',()=>{
    mount('classic');
    fireEvent.keyDown(window,{key:'b'});
    expect(screen.getByText('矿物采集器')).toBeVisible();
    expect(screen.queryByText('风力发电机')).not.toBeInTheDocument();
      expect(screen.queryByText('数据中心')).not.toBeInTheDocument();
  });
  it('refuses the wiring and GPU shortcuts with an explanation',()=>{
    mount('classic');
    fireEvent.keyDown(window,{key:'l'});
    expect(runtime.setNotice).toHaveBeenCalledWith('单层塔防模式没有电力与算力线路。');
    fireEvent.keyDown(window,{key:'i'});
    expect(runtime.setNotice).toHaveBeenCalledWith('单层塔防模式没有显卡机架。');
  });
  it('deploys without a producing room by sending room 0',async()=>{
      const stage=mount('classic');
    fireEvent.keyDown(window,{key:'u'});
    fireEvent.click(screen.getByText('VSCode 炮台'));
    fireEvent.pointerDown(stage,{button:0,clientX:640,clientY:360});
    fireEvent.pointerUp(stage,{button:0,clientX:640,clientY:360});
    await waitFor(()=>expect(runtime.order).toHaveBeenCalledWith(expect.objectContaining({op:'deploy',room:0,kind:'vscode'})));
  });
  it('researches a branch at the core for credits only',async()=>{
    mount('classic');
    fireEvent.click(screen.getByRole('button',{name:/科技等级/}));
    await screen.findByRole('dialog',{name:'科技树'});
    expect(screen.getAllByText(/只消耗经费/).length).toBeGreaterThan(0);
    fireEvent.click(screen.getAllByRole('button',{name:/研究 T/})[0]);
    await waitFor(()=>expect(runtime.order).toHaveBeenCalledWith(expect.objectContaining({op:'research',room:0})));
    expect(screen.queryByText(/定位研究室/)).not.toBeInTheDocument();
  });
  it('builds a physical wall without offering CUDA variants',()=>{
    mount('classic');
    fireEvent.click(screen.getByRole('button',{name:'防御墙'}));
    expect(screen.queryByLabelText('墙体类型')).not.toBeInTheDocument();
    expect(screen.getByText('实体墙')).toBeVisible();
  });
    it('drops the compute and power readouts from the resource strip',()=>{
    mount('classic');
    expect(screen.queryByText('算力')).not.toBeInTheDocument();
    expect(screen.queryByText('电力 / 负载')).not.toBeInTheDocument();
    expect(screen.getByText('科技等级')).toBeVisible();
  });
  it('guides the player through the classic loop instead of the shell loop',()=>{
    mount('classic');
    expect(screen.getByText('B 在矿脉旁建采集器')).toBeVisible();
    expect(screen.queryByText('B 拖出毛坯，等待施工')).not.toBeInTheDocument();
    });
});
