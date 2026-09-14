import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,describe,expect,it,vi} from 'vitest';
import V6Lobby from '../src/components/game/v6/V6Lobby';
import {normalizeV6Catalog,type V6Session} from '../src/lib/sentinelsV6';

const catalog=normalizeV6Catalog({units:[{id:'kimi',name:'Kimi',category:'ai',branch:'speed',tier:2,cost:780,description:'高速单体与定向突进',skill:'dash'}]});
function props(session:V6Session|null=null){return{session,snapshot:null,playerId:1,catalog,busy:false,error:'',connected:true,onBegin:vi.fn(async()=>true),onContinue:vi.fn(),onAction:vi.fn<(name:string,body?:unknown)=>Promise<unknown>>(async()=>({ok:true})),onRefresh:vi.fn()};}
function lobbySession():V6Session{return{mode:'host',status:'lobby',roomId:'room-v6',code:'123456',address:'192.168.1.20:6066',players:[{owner:1,nickname:'甲',ready:true,connected:true}],lastSequence:0};}
afterEach(()=>{cleanup();vi.unstubAllGlobals();});
describe('V6 lobby controls',()=>{
  it('does not offer an active start with only one player, and toggles the actual ready state',async()=>{
    const p=props(lobbySession());render(<V6Lobby {...p}/>);
    expect(screen.getByRole('button',{name:/开始基地攻防/})).toBeDisabled();
    fireEvent.click(screen.getByRole('button',{name:'切换准备状态'}));
    await waitFor(()=>expect(p.onAction).toHaveBeenCalledWith('ready',{ready:false}));
    expect(screen.getByText('192.168.1.20:6066')).toBeVisible();
  });
  it('requires a destination for joining and sends selected theme and seed without inventing another address',async()=>{
    const p=props();render(<V6Lobby {...p}/>);
    fireEvent.click(screen.getByRole('button',{name:'加入房间'}));
    expect(screen.getByRole('button',{name:'加入对战房间'})).toBeDisabled();
    fireEvent.change(screen.getByLabelText('房主IP和端口'),{target:{value:'192.168.1.20:6066'}});
    expect(screen.getByRole('button',{name:'加入对战房间'})).toBeDisabled();
    fireEvent.change(screen.getByLabelText('房间码'),{target:{value:'a1b2c3'}});
    fireEvent.click(screen.getByRole('button',{name:'加入对战房间'}));
    await waitFor(()=>expect(p.onBegin).toHaveBeenCalledWith(expect.objectContaining({mode:'join',address:'192.168.1.20:6066',code:'A1B2C3',theme:'river',seed:6026})));
    expect(p.onContinue).not.toHaveBeenCalled();
  });
  it('keeps malformed room codes out of the join request',()=>{
    const p=props();render(<V6Lobby {...p}/>);
    fireEvent.click(screen.getByRole('button',{name:'加入房间'}));
    fireEvent.change(screen.getByLabelText('房主IP和端口'),{target:{value:'203.0.113.9:6066'}});
    for(const code of ['12345','G12345']){
      fireEvent.change(screen.getByLabelText('房间码'),{target:{value:code}});
      expect(screen.getByRole('button',{name:'加入对战房间'})).toBeDisabled();
      fireEvent.click(screen.getByRole('button',{name:'加入对战房间'}));
    }
    expect(p.onBegin).not.toHaveBeenCalled();
  });
  it('passes the chosen host game port without reusing editor or client ports',async()=>{
    const p=props();render(<V6Lobby {...p}/>);
    fireEvent.click(screen.getByRole('button',{name:'创建房间'}));
    fireEvent.change(screen.getByLabelText('游戏端口'),{target:{value:'16066'}});
    fireEvent.click(screen.getByRole('button',{name:'创建对战房间'}));
    await waitFor(()=>expect(p.onBegin).toHaveBeenCalledWith(expect.objectContaining({mode:'host',port:16066})));
  });
  it('labels a fresh start as ending the current battle, leaves first, then opens the chosen operation',async()=>{
    const p=props({...lobbySession(),mode:'solo',status:'battle'});render(<V6Lobby {...p}/>);
    fireEvent.click(screen.getByRole('button',{name:/MINING/}));
    fireEvent.click(screen.getByRole('button',{name:'结束当前行动并重新部署'}));
    await waitFor(()=>expect(p.onBegin).toHaveBeenCalledWith(expect.objectContaining({mode:'solo',theme:'mining'})));
    expect(p.onAction.mock.invocationCallOrder[0]).toBeLessThan(p.onBegin.mock.invocationCallOrder[0]);
    expect(p.onAction).toHaveBeenCalledWith('leave');
  });
  it('loads a selected save only after leaving, rather than restarting an unrelated scene',async()=>{
    vi.stubGlobal('fetch',vi.fn(async()=>({ok:true,json:async()=>({saves:[{id:'saved-v6',name:'测试行动',tick:1800}]})})));
    const p=props({...lobbySession(),mode:'solo',status:'battle'});render(<V6Lobby {...p}/>);
    fireEvent.click(screen.getByRole('button',{name:'存档与回放'}));await screen.findByText('测试行动');
    fireEvent.click(screen.getByRole('button',{name:'读取'}));
    await waitFor(()=>expect(p.onAction.mock.calls).toEqual([['leave'],['load',{id:'saved-v6'}]]));
    expect(p.onContinue).toHaveBeenCalled();
  });
  it('keeps a restored multiplayer save in its new preparation room until both players return',async()=>{
    vi.stubGlobal('fetch',vi.fn(async()=>({ok:true,json:async()=>({saves:[{id:'pvp-v6',name:'双人行动',tick:1800}]})})));
    const p=props();p.onAction.mockResolvedValue({session:lobbySession()});render(<V6Lobby {...p}/>);
    fireEvent.click(screen.getByRole('button',{name:'存档与回放'}));await screen.findByText('双人行动');fireEvent.click(screen.getByRole('button',{name:'读取'}));
    await waitFor(()=>expect(p.onAction).toHaveBeenCalledWith('load',{id:'pvp-v6'}));
    expect(p.onContinue).not.toHaveBeenCalled();
  });
  it('preserves incompatible saves in the list without ending the active game to load or replay them',async()=>{
    vi.stubGlobal('fetch',vi.fn(async()=>({ok:true,json:async()=>({saves:[{id:'legacy-v6',name:'旧开发行动',tick:1800,compatible:false,compatibility:'legacy-unverified',compatibilityReason:'旧版存档没有规则指纹，原文件已保留。'}]})})));
    const p=props({...lobbySession(),mode:'solo',status:'battle'});render(<V6Lobby {...p}/>);
    fireEvent.click(screen.getByRole('button',{name:'存档与回放'}));await screen.findByText('旧开发行动');
    expect(screen.getByText('旧版存档没有规则指纹，原文件已保留。')).toBeVisible();
    for(const name of ['读取','回放']){const button=screen.getByRole('button',{name});expect(button).toBeDisabled();fireEvent.click(button);}
    expect(p.onAction).not.toHaveBeenCalled();
  });
});
