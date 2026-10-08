import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/react';
import {afterEach,beforeEach,describe,expect,it,vi} from 'vitest';
import V6DataCenterRacks from '../src/components/game/v6/V6DataCenterRacks';
import type {V6Preview,V6Room} from '../src/lib/sentinelsV6';

const mock=vi.hoisted(()=>({request:vi.fn()}));
vi.mock('../src/lib/useSentinelsV6',()=>({v6Request:mock.request}));
function room(patch:Partial<V6Room>={}):V6Room{return{id:10,shell:8,owner:1,rect:{x:20,y:40,z:0,w:4,h:3},kind:'data-center',branch:null,tier:1,hp:300,maxHp:300,powered:true,connected:true,capacity:2,capacityBudget:2,potentialCapacity:3,gpus:[],inventory:0,progress:1,buildTime:8,cooldown:0,...patch};}
function quote(patch:Partial<V6Preview>={}):V6Preview{return{valid:true,reason:'',cost:{credits:37.5,compute:0,science:0},powerBefore:100,powerAfter:100,demandBefore:20,demandAfter:20,...patch};}
function props(patch:Partial<React.ComponentProps<typeof V6DataCenterRacks>>={}){return{room:room(),owned:true,canOrder:true,sessionId:'rack-test',tickSeconds:5,onInstall:vi.fn(),onOrder:vi.fn(async()=>({accepted:true})),...patch};}
beforeEach(()=>{mock.request.mockReset();mock.request.mockResolvedValue(quote());});
afterEach(cleanup);

describe('V6 paid rack budget inspector',()=>{
  it('shows native available/budget/potential values without converting 12 room cells into free slots',async()=>{
    const p=props();render(<V6DataCenterRacks {...p}/>);
    expect(screen.getByText('0 / 2')).toBeVisible();
    expect(screen.getByText(/空间上限 3/)).toBeVisible();
    expect(screen.getByText(/合并保留已有的 2 个机架/)).toBeVisible();
    expect(screen.getAllByRole('button',{name:/空机架/})).toHaveLength(2);
    expect(screen.queryByRole('button',{name:'空机架3'})).not.toBeInTheDocument();
    const reorganize=await screen.findByRole('button',{name:/重整机架.*37.5/});
    expect(reorganize).toBeEnabled();
    expect(mock.request).toHaveBeenCalledWith('preview',{command:{op:'convert-room',id:10,kind:'data-center'},sessionId:'rack-test'},expect.any(AbortSignal));
    fireEvent.click(reorganize);
    await waitFor(()=>expect(p.onOrder).toHaveBeenCalledExactlyOnceWith({op:'convert-room',id:10,kind:'data-center'}));
    expect(screen.getByText(/施工 8 秒/)).toBeVisible();
  });
  it('uses native rejection and never automatically unloads installed GPUs to enable reorganization',async()=>{
    mock.request.mockResolvedValue(quote({valid:false,reason:'请先卸载显卡，并完成在途运输'}));
    const p=props({room:room({gpus:['rtx-5060']})});render(<V6DataCenterRacks {...p}/>);
    expect(await screen.findByText('请先卸载显卡，并完成在途运输')).toBeVisible();
    const reorganize=screen.getByRole('button',{name:/重整机架/});expect(reorganize).toBeDisabled();
    fireEvent.click(reorganize);expect(p.onOrder).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText('卸载与回收显卡'));
    fireEvent.click(screen.getByRole('button',{name:/卸载机架 1/}));
    await waitFor(()=>expect(p.onOrder).toHaveBeenCalledExactlyOnceWith({op:'remove-gpu',room:10,bay:0}));
  });
  it('explains temporarily blocked paid capacity without proposing another purchase',()=>{
    render(<V6DataCenterRacks {...props({room:room({capacity:1,capacityBudget:3,potentialCapacity:1,gpus:['rtx-5060']})})}/>);
    expect(screen.getByText('1 / 1')).toBeVisible();
    expect(screen.getByText(/空间占用使 2 个已购机架暂不可用；调整分隔或移除占用后恢复/)).toBeVisible();
    expect(screen.queryByRole('button',{name:/重整机架/})).not.toBeInTheDocument();
    expect(mock.request).not.toHaveBeenCalled();
  });
  it.each([{}, {capacityBudget:null,potentialCapacity:null}])('does not infer expansion for legacy missing/null fields %j',fields=>{
    const legacy=room({capacity:2,capacityBudget:undefined,potentialCapacity:undefined,...fields});
    render(<V6DataCenterRacks {...props({room:legacy})}/>);
    expect(screen.getByText('0 / 2')).toBeVisible();
    expect(screen.queryByText(/空间上限/)).not.toBeInTheDocument();
    expect(screen.queryByRole('button',{name:/重整机架/})).not.toBeInTheDocument();
    expect(mock.request).not.toHaveBeenCalled();
  });
  it.each([{owned:false},{canOrder:false},{room:room({progress:.4})}])('does not request or send refit for unavailable controls %j',patch=>{
    const p=props(patch);render(<V6DataCenterRacks {...p}/>);
    const button=screen.queryByRole('button',{name:/重整机架/});
    if(button){expect(button).toBeDisabled();fireEvent.click(button);}
    expect(mock.request).not.toHaveBeenCalled();expect(p.onOrder).not.toHaveBeenCalled();
  });
  it('rejects an old room quote when selection changes and uses only the new room command',async()=>{
    let resolveOld!:(value:V6Preview)=>void,resolveNew!:(value:V6Preview)=>void;
    mock.request.mockImplementationOnce(()=>new Promise<V6Preview>(resolve=>{resolveOld=resolve;}));
    mock.request.mockImplementationOnce(()=>new Promise<V6Preview>(resolve=>{resolveNew=resolve;}));
    const p=props(),view=render(<V6DataCenterRacks {...p}/>);
    view.rerender(<V6DataCenterRacks {...p} room={room({id:20})}/>);
    await act(async()=>resolveOld(quote({cost:{credits:999,compute:0,science:0}})));
    expect(screen.queryByText(/999/)).not.toBeInTheDocument();
    expect(screen.getByRole('button',{name:/重整机架/})).toBeDisabled();
    await act(async()=>resolveNew(quote({cost:{credits:78,compute:0,science:0}})));
    fireEvent.click(screen.getByRole('button',{name:/重整机架.*78/}));
    await waitFor(()=>expect(p.onOrder).toHaveBeenCalledExactlyOnceWith({op:'convert-room',id:20,kind:'data-center'}));
  });
  it('does not manufacture a price or submit after preview transport failure',async()=>{
    mock.request.mockRejectedValue(new Error('连接中断'));
    const p=props();render(<V6DataCenterRacks {...p}/>);
    expect(await screen.findByText('连接中断')).toBeVisible();
    const button=screen.getByRole('button',{name:/重整机架/});expect(button).toBeDisabled();
    expect(button).not.toHaveTextContent('◈');fireEvent.click(button);expect(p.onOrder).not.toHaveBeenCalled();
  });
});
