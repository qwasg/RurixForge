import {cleanup,fireEvent,render,screen,within} from '@testing-library/react';
import {afterEach,describe,expect,it,vi} from 'vitest';
import V6TechnologyPanel,{v6ResearchQuote} from '../src/components/game/v6/V6TechnologyPanel';
import {normalizeV6Catalog,type V6Player,type V6Room,type V6Snapshot} from '../src/lib/sentinelsV6';
const catalog=normalizeV6Catalog({research:{credits:[100,350,700,1400,2400],compute:[60,150,400,900,1800],scienceData:[0,0,60,180,360],seconds:[35,60,100,160,220],labComputeUpkeep:[.5,1.5,3,6,10],additionalBranchFactor:.8,additionalBranchFromTier:3},units:[]});
function props(){return{catalog,rooms:[{id:10,shell:8,owner:1,rect:{x:20,y:40,z:0,w:4,h:4},kind:'research-lab',branch:'speed',progress:1,hp:300,powered:true,connected:true,online:true}] as V6Room[],player:{owner:1,credits:1000,science:100,branches:{speed:1},research:null,researches:[]} as unknown as V6Player,state:{networkStores:[{owner:1,anchor:{x:21,y:41,z:0},cells:[{x:21,y:41,z:0}],compute:200,capacity:1000,production:20}]} as V6Snapshot,onResearch:vi.fn(),onLocate:vi.fn()};}
function speed(){return within(screen.getByRole('heading',{name:'速度'}).closest('article')!);}
afterEach(cleanup);
describe('V6 branch research commands',()=>{
  it('reads next-tier upfront costs, science, duration and runtime compute from native rules',()=>{
    expect(v6ResearchQuote(catalog,2)).toEqual({credits:350,compute:150,science:0,seconds:60,upkeep:1.5,multiplier:1});
    expect(v6ResearchQuote(catalog,6)).toBeNull();
    const p=props();render(<V6TechnologyPanel {...p}/>);expect(speed().getByText('◈ 350 · 算力 150')).toBeVisible();
    fireEvent.click(speed().getByRole('button',{name:'研究 T2'}));expect(p.onResearch).toHaveBeenCalledWith(p.rooms[0],'speed');
  });
  it('does not count another owner or another disconnected component balance',()=>{
    const p=props();p.state.networkStores![0].owner=2;render(<V6TechnologyPanel {...p}/>);
    expect(speed().getByRole('button',{name:'接入网络算力不足'})).toBeDisabled();
  });
  it('includes other advanced branches in the real price without multiplying compute upkeep',()=>{
    const quote=v6ResearchQuote(catalog,4,'speed',{speed:3,science:3,security:2})!;
    expect(quote).toEqual({credits:2520,compute:900,science:324,seconds:288,upkeep:6,multiplier:1.8});
    const p=props();p.player.branches={speed:3,science:3,security:2};p.player.credits=2286;p.player.science=400;p.state.networkStores![0].compute=2000;
    render(<V6TechnologyPanel {...p}/>);
    expect(speed().getByText('◈ 2,520 · 算力 900')).toBeVisible();
    expect(speed().getByRole('button',{name:'经费不足'})).toBeDisabled();
    expect(speed().getByText(/兼修投入 ×1.8/)).toBeVisible();
  });
  it('prefers a completed operating lab rather than an older unfinished construction',()=>{
    const p=props();p.rooms.unshift({...p.rooms[0],id:9,progress:.4,powered:false});render(<V6TechnologyPanel {...p}/>);
    fireEvent.click(speed().getByRole('button',{name:'研究 T2'}));expect(p.onResearch).toHaveBeenCalledWith(expect.objectContaining({id:10}),'speed');
  });
  it('shows a paused existing research and offers its lab location instead of paying twice',()=>{
    const p=props();p.rooms[0].powered=false;p.player.researches=[{lab:10,branch:'speed',target:2,progress:.5,duration:60}];render(<V6TechnologyPanel {...p}/>);
    expect(speed().getByText('研究暂停 T2 · 30/60s')).toBeVisible();expect(speed().getByRole('button',{name:'恢复供给后继续'})).toBeDisabled();
    fireEvent.click(speed().getByRole('button',{name:/定位研究室/}));expect(p.onLocate).toHaveBeenCalledWith(p.rooms[0]);
  });
});
