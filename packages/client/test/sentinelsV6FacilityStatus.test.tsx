import {cleanup,fireEvent,render,screen} from '@testing-library/react';
import {afterEach,describe,expect,it,vi} from 'vitest';
import V6FacilityStatus,{facilityV6Network} from '../src/components/game/v6/V6FacilityStatus';
import {normalizeV6Catalog,type V6Room,type V6Snapshot} from '../src/lib/sentinelsV6';

const items=normalizeV6Catalog({rooms:[{id:'data-center',power:20},{id:'research-lab',power:20,upkeepCompute:.5}],gpus:[{id:'rtx-5060',power:40}]}).items;
const port={x:21,y:41,z:0};
function room(patch:Partial<V6Room>={}):V6Room{return{id:10,shell:8,owner:1,rect:{x:20,y:40,z:0,w:4,h:4},kind:'data-center',branch:null,tier:1,hp:300,maxHp:300,powered:false,connected:false,capacity:4,gpus:['rtx-5060'],inventory:0,progress:1,buildTime:10,cooldown:0,...patch};}
function state():V6Snapshot{return{powerGrids:[{owner:1,cells:[port],output:50,load:60},{owner:2,cells:[port],output:500,load:0}],networkStores:[{owner:2,cells:[port],anchor:port,compute:1000,capacity:1000,production:20}]} as V6Snapshot;}
afterEach(cleanup);
describe('V6 facility connection diagnosis',()=>{
  it('scales shared facility equipment power once while retaining each physical GPU load',()=>{
    const status=facilityV6Network(room({equipmentShare:.5}),state(),items);
    expect(status.demand).toBe(50);
  });
  it('uses the selected owner local grid instead of global or enemy surplus',()=>{
    const status=facilityV6Network(room(),state(),items);
    expect(status.reason).toBe('当前电网过载 · 缺 10 电力');
    expect(status.demand).toBe(60);expect(status.network).toBeUndefined();
  });
  it('distinguishes an absent connection from empty generation and an unpopulated rack',()=>{
    const s=state();s.powerGrids=[];expect(facilityV6Network(room(),s,items).reason).toMatch('未接电力线路');
    s.powerGrids=[{owner:1,cells:[port],output:0,load:60}];expect(facilityV6Network(room(),s,items).reason).toMatch('没有运行中的发电设施');
    expect(facilityV6Network(room({powered:true,gpus:[]}),s,items).reason).toBe('机房已就绪 · I 安装显卡');
  });
  it('gives unfinished construction priority over network warnings',()=>{
    expect(facilityV6Network(room({progress:.25}),state(),items).reason).toBe('施工中 · 25%');
  });
  it('distinguishes disconnected compute from insufficient runtime compute',()=>{
    const q=room({kind:'research-lab',gpus:[],powered:true,maintenance:.5,online:false});
    expect(facilityV6Network(q,state(),items).reason).toMatch('未接算力网络');
    q.connected=true;expect(facilityV6Network(q,state(),items).reason).toMatch('接入网络算力不足');
  });
  it('offers an explicit cable start and hides enemy diagnostics and controls',()=>{
    const onWire=vi.fn(),p={entity:room(),state:state(),items,owned:true,onWire};
    const {rerender}=render(<V6FacilityStatus {...p}/>);
    fireEvent.click(screen.getByRole('button',{name:/从此处拉电线/}));expect(onWire).toHaveBeenCalledWith('power');
    expect(screen.getByText('50 / 60')).toBeVisible();
    rerender(<V6FacilityStatus {...p} owned={false}/>);expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });
});
