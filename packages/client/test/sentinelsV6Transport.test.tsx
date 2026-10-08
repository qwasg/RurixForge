import {act,cleanup,renderHook,waitFor} from '@testing-library/react';
import {afterEach,describe,expect,it,vi} from 'vitest';
import {normalizeV6Catalog,validateV6Snapshot,type V6Snapshot,type V6Status} from '../src/lib/sentinelsV6';
import {attachV6Media,useSentinelsV6} from '../src/lib/useSentinelsV6';
import {v6CardLock} from '../src/components/game/v6/V6Card';

/** Protocol fixture for client behavior only. Native gameplay acceptance uses public Rust commands. */
function fixture():V6Snapshot{return{version:6,revision:1,tick:0,seed:11,width:128,height:96,minLevel:0,maxLevel:0,players:[1,2].map(owner=>({owner,credits:2000,compute:0,computeCapacity:0,power:0,demand:0,income:2,production:0,branches:{speed:1},research:null,dominance:0,ai:owner===2,lostValue:0})),terrain:Array(128*96).fill(0),buildings:[],rooms:[],units:[],links:[],walls:[],resources:[],projectiles:[],events:[],winner:null,winReason:'',explored:[[],[]],visible:[[],[]]};}
function service(options:{phase?:'lobby'|'battle';sequence?:number;retry?:boolean;failures?:number;malformed?:boolean;replay?:boolean;paused?:boolean}={}){
  const snapshot=fixture();
  const status:V6Status={protocol:'code-sentinels-pvp/6.1',playerId:1,session:{mode:'solo',status:options.phase??'battle',roomId:'native-session',code:'',address:'',players:[],lastSequence:options.sequence??0,replay:options.replay,paused:options.paused},snapshot};
  const orders:{seq:number;command:unknown}[]=[];let retries=0;
  let socket:{onmessage?:((event:{data:unknown})=>void)|null}|undefined;
  vi.stubGlobal('WebSocket',class{constructor(){socket=this;}binaryType='arraybuffer';onopen:unknown;onmessage?:((event:{data:unknown})=>void)|null;onerror:unknown;onclose:unknown;send(){}close(){}});
  const fetcher=vi.fn(async(input:string|URL|Request,init?:RequestInit)=>{
    const url=String(input);const response=(value:unknown)=>({ok:true,status:200,json:async()=>value});
    if(url==='/api/v6/status')return response(options.malformed?{...status,snapshot:{...snapshot,width:32}}:status);
    if(url==='/api/v6/catalog')return response({units:[{id:'vscode',name:'VS Code',category:'turret',tier:1,cost:80}]});
    if(url.endsWith('resource-manifest.json'))return response({version:6,models:{}});
    if(url==='/api/v6/viewport')return response({wsUrl:'ws://127.0.0.1:60000/stream'});
    if(url.startsWith('/api/v6/snapshot'))return response({snapshot,session:status.session});
    if(url==='/api/v6/order'){const data=JSON.parse(String(init?.body));orders.push(data);if(retries++<(options.failures??(options.retry?1:0)))throw new TypeError('network lost after write');return response({accepted:true,sequence:data.seq,tick:7,reason:'native accepted'});}
    throw Error('Unexpected test request '+url);
  });
  vi.stubGlobal('fetch',fetcher);return{snapshot,status,orders,fetcher,get socket(){return socket;}};
}
afterEach(()=>{cleanup();vi.unstubAllGlobals();vi.restoreAllMocks();});
describe('V6 client authoritative transport',()=>{
  it('refuses an older communication protocol before enabling the map',async()=>{
    const backend=service();backend.status.protocol='code-sentinels-pvp/6';
    const{result}=renderHook(()=>useSentinelsV6());await waitFor(()=>expect(result.current.error).toContain('通信协议不匹配'));
    expect(result.current.connected).toBe(false);expect(result.current.snapshot).toBeNull();
  });
  it('restores sequence and sends two orders in order without speculative credit changes',async()=>{
    const backend=service({sequence:12}),{result}=renderHook(()=>useSentinelsV6());
    await waitFor(()=>expect(result.current.connected).toBe(true));
    await act(async()=>{await Promise.all([result.current.order({op:'shell',rect:{x:15,y:40,z:0,w:4,h:4}}),result.current.order({op:'build',pos:{x:18,y:37,z:0},kind:'wind-power'})]);});
    expect(backend.orders.map(o=>o.seq)).toEqual([13,14]);
    expect(result.current.snapshot?.players[0].credits).toBe(2000);
    expect(result.current.receipt?.sequence).toBe(14);
  });
  it('retries an uncertain network delivery with the identical command identity',async()=>{
    const backend=service({retry:true}),{result}=renderHook(()=>useSentinelsV6());await waitFor(()=>expect(result.current.connected).toBe(true));
    await act(async()=>{await result.current.order({op:'install-gpu',room:42,model:'rtx-5060'});});
    expect(backend.orders).toHaveLength(2);expect(backend.orders[0]).toEqual(backend.orders[1]);expect(backend.orders[0].seq).toBe(1);
  });
  it('freezes later commands after two lost replies, then reconciles the original sequence before accepting more',async()=>{
    const backend=service({failures:2}),{result}=renderHook(()=>useSentinelsV6());await waitFor(()=>expect(result.current.connected).toBe(true));
    await act(async()=>{await Promise.all([result.current.order({op:'install-gpu',room:42,model:'rtx-5060'}),result.current.order({op:'upgrade',id:99})]);});
    await waitFor(()=>expect(result.current.receipt?.sequence).toBe(1));
    expect(backend.orders).toHaveLength(3);expect(backend.orders.every(o=>o.seq===1)).toBe(true);expect(backend.orders[0]).toEqual(backend.orders[2]);
    await act(async()=>{await result.current.order({op:'upgrade',id:99});});expect(backend.orders[3].seq).toBe(2);
  });
  it('takes a resumed guest sequence from the lobby-to-battle snapshot before its first order',async()=>{
    const backend=service({phase:'lobby'});backend.status.playerId=2;backend.status.session!.mode='join';
    const{result}=renderHook(()=>useSentinelsV6());await waitFor(()=>expect(result.current.connected).toBe(true));
    backend.status.session={...backend.status.session!,status:'battle',lastSequence:456};
    await waitFor(()=>expect(result.current.session?.status).toBe('battle'));
    await act(async()=>{await result.current.order({op:'upgrade',id:44});});
    expect(backend.orders[0].seq).toBe(457);
  });
  it('preserves the last local frame/state but prevents orders while the authority is offline',async()=>{
    const backend=service();backend.status.session={...backend.status.session!,mode:'join',connected:false,replicaSynced:true,error:'快照流连接失败'};
    const{result}=renderHook(()=>useSentinelsV6());
    await waitFor(()=>expect(result.current.error).toBe('快照流连接失败'));
    await waitFor(()=>expect(backend.fetcher.mock.calls.filter(([url])=>String(url).startsWith('/api/v6/snapshot')).length).toBeGreaterThan(1));
    expect(result.current.connected).toBe(false);expect(result.current.snapshot?.seed).toBe(11);
    await act(async()=>{await result.current.order({op:'upgrade',id:44});});expect(backend.orders).toHaveLength(0);
    backend.status.session={...backend.status.session!,connected:true,error:null};
    await waitFor(()=>expect(result.current.connected).toBe(true));expect(result.current.error).toBe('');
    await act(async()=>{await result.current.order({op:'upgrade',id:44});});expect(backend.orders).toHaveLength(1);
  });
  it('does not enable a guest battle before its first authoritative snapshot has applied',async()=>{
    const backend=service();backend.status.session={...backend.status.session!,mode:'join',connected:true,replicaSynced:false};
    const{result}=renderHook(()=>useSentinelsV6());await waitFor(()=>expect(result.current.session?.mode).toBe('join'));
    expect(result.current.connected).toBe(false);
    await act(async()=>{await result.current.order({op:'upgrade',id:44});});expect(backend.orders).toHaveLength(0);
    backend.status.session={...backend.status.session!,replicaSynced:true};
    await waitFor(()=>expect(result.current.connected).toBe(true));
  });
  it('keeps a native viewport error visible across successful snapshot polls',async()=>{
    const backend=service(),{result}=renderHook(()=>useSentinelsV6());
    await waitFor(()=>expect(backend.socket?.onmessage).toBeTypeOf('function'));
    act(()=>backend.socket?.onmessage?.({data:JSON.stringify({type:'error',message:'画面绘制数量超限'})}));
    await waitFor(()=>expect(backend.fetcher.mock.calls.filter(([url])=>String(url).startsWith('/api/v6/snapshot')).length).toBeGreaterThan(1));
    expect(result.current.error).toBe('画面绘制数量超限');expect(result.current.connected).toBe(true);
  });
  it.each([{phase:'lobby' as const},{replay:true},{paused:true}])('does not send combat commands from a lobby, replay or paused battle (%j)',async options=>{
    const backend=service(options),{result}=renderHook(()=>useSentinelsV6());await waitFor(()=>expect(result.current.connected).toBe(true));
    await act(async()=>{await result.current.order({op:'upgrade',id:2});});expect(backend.orders).toEqual([]);
  });
  it('refuses mismatched world snapshots instead of showing selectable old coordinates',async()=>{
    service({malformed:true});const{result}=renderHook(()=>useSentinelsV6());await waitFor(()=>expect(result.current.error).toContain('坐标不匹配'));expect(result.current.snapshot).toBeNull();expect(result.current.connected).toBe(false);
  });
});
describe('V6 native catalog presentation',()=>{
  it('resolves real media aliases while keeping unit-specific art and native costs intact',()=>{
    const catalog=normalizeV6Catalog({units:[{id:'science-tank',category:'vehicle',chassis:'tank',tier:2,cost:420}],rooms:[{id:'wireless-relay',cost:160,power:10}]});
    attachV6Media(catalog,{version:6,models:{'science-tank':{image:'/specific.png'},'light-tank':{image:'/generic.png'},'secure-switch':{image:'/relay.png'}},chassisAliases:{tank:'light-tank'},facilityAliases:{'wireless-relay':'secure-switch'}});
    expect(catalog.items.find(c=>c.id==='science-tank')).toMatchObject({image:'/specific.png',cost:420,role:'vehicle'});
    expect(catalog.items.find(c=>c.id==='wireless-relay')).toMatchObject({image:'/relay.png',cost:160,power:10});
  });
  it('preserves native price, chassis and role while grouping the UI deck',()=>{
    const c=normalizeV6Catalog({units:[{id:'science-fighter',category:'air',chassis:'attack-aircraft',tier:3,cost:572,skillShape:'line',skillWidth:2}],gpus:[{id:'rtx-5060',name:'RTX 5060',tier:1,cost:130,power:55}]});
    expect(c.items[0]).toMatchObject({category:'units',role:'air',chassis:'attack-aircraft',cost:572,skillShape:'line',skillWidth:2});expect(c.items[1].power).toBe(55);
  });
  it('uses the exact selected research branch, with a separate funding reason',()=>{
    const player=fixture().players[0];player.branches={speed:5,science:1};
    const item=normalizeV6Catalog({units:[{id:'particle-cannon',branch:'science',tier:4,cost:100}]}).items[0];
    expect(v6CardLock(item,player)).toBe('科研 T4');player.branches.science=4;player.credits=99;expect(v6CardLock(item,player)).toBe('经费不足');player.credits=100;expect(v6CardLock(item,player)).toBe('');
  });
  it('rejects missing arrays and non-finite resources before the scene is enabled',()=>{
    const s=fixture();expect(()=>validateV6Snapshot({...s,rooms:null})).toThrow('rooms');s.players[0].compute=NaN;expect(()=>validateV6Snapshot(s)).toThrow('不完整');
  });
});
