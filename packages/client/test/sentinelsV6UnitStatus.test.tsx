import {cleanup,fireEvent,render,screen} from '@testing-library/react';
import {afterEach,describe,expect,it,vi} from 'vitest';
import V6UnitStatus from '../src/components/game/v6/V6UnitStatus';
import {normalizeV6Catalog,v6AiEconomy,v6UnitElevation,v6PluginLock,v6UnitCosts,type V6Unit,type V6CatalogItem} from '../src/lib/sentinelsV6';

const catalog=normalizeV6Catalog({units:[{id:'kimi',category:'ai',name:'Kimi',tier:2,branch:'speed',cost:780,skill:'dash',skillShape:'direction',skillCost:60,computePerAttack:3,speed:4},{id:'speed-tank',category:'vehicle',name:'突击坦克',skill:'overclock',skillShape:'self',skillCost:65,speed:2}]}).items;
function unit(patch:Partial<V6Unit>={}):V6Unit{return{id:30,owner:1,kind:'kimi',pos:{x:20,y:30,z:0},x:20.5,y:30.5,z:0,tier:2,hp:300,maxHp:400,battery:0,batteryMax:120,covered:false,wired:false,route:[],target:null,cooldown:0,skillCooldown:0,plugins:[],statuses:{},invested:780,moving:false,attackCount:0,...patch};}
function props(patch:Partial<V6Unit>={},item:V6CatalogItem=catalog[0]){return{unit:unit(patch),item,owned:true,catalog,onSkill:vi.fn(),onPlugins:vi.fn(),onMove:vi.fn(),onStop:vi.fn(),onReturn:vi.fn()};}
afterEach(cleanup);
describe('V6 local reserves and commands',()=>{
  it('quotes AI surcharge and ongoing upkeep from public rules including duplicate names',()=>{
    const c=normalizeV6Catalog({ai:{purchaseGrowth:.12,upkeepQuadratic:.35,deployCompute:120},items:catalog});
    const units=[unit(),unit({id:2}),unit({id:3,owner:2}),unit({id:4,kind:'speed-tank'}),unit({id:5,hp:0})];
    const result=v6AiEconomy(c,units,1);
    expect(result.count).toBe(2);expect(result.multiplier).toBeCloseTo(1.72);expect(result.upkeep).toBeCloseTo(1.4);expect(result.deployCompute).toBe(120);
    expect(c.items[0].cost).toBe(780);
  });
  it('uses altitude only for elevation without floor transit',()=>{
    expect(v6UnitElevation(unit({altitude:0}))).toBe(0);
    expect(v6UnitElevation(unit({altitude:2}))).toBe(2);
  });
  it('explains empty offline compute without showing ammunition',()=>{
    render(<V6UnitStatus {...props()}/>);
    expect(screen.getByText('离网缓存已耗尽，请返回基站覆盖')).toBeVisible();
    expect(screen.getByLabelText('随身算力')).toHaveAttribute('value','0');
    expect(screen.queryByLabelText('弹药')).not.toBeInTheDocument();
    expect(screen.getByText('3 算力')).toBeVisible();
  });
  it('hides ground fuel and exposes modules for branch equipment',()=>{
    const p=props({kind:'speed-tank',branch:'speed',batteryMax:0,fuelMax:120,fuel:40},catalog[1]);
    render(<V6UnitStatus {...p}/>);
    expect(screen.queryByLabelText('出动时长')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('燃料')).not.toBeInTheDocument();
    expect(screen.queryByText('插件整备')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button',{name:'武器模块'}));expect(p.onPlugins).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button',{name:'停止指令'}));expect(p.onStop).toHaveBeenCalledOnce();
  });
  it('does not treat an empty covered cache as an automatic stop and prevents cooldown skill clicks',()=>{
    const p=props({covered:true,skillCooldown:12});render(<V6UnitStatus {...p}/>);
    expect(screen.getByText('缓存已空，攻击依赖当前接入网络余量')).toBeVisible();
    expect(screen.getByRole('button',{name:/发动主动能力/})).toBeDisabled();
    fireEvent.click(screen.getByRole('button',{name:/发动主动能力/}));expect(p.onSkill).not.toHaveBeenCalled();
  });
  it('shows aircraft sortie endurance and return order',()=>{
    const p=props({kind:'science-bomber',batteryMax:0,energy:0,energyMax:200,fuel:0,fuelMax:120,flightState:'emergency'},{...catalog[1],role:'air',energyPerAttack:20});render(<V6UnitStatus {...p}/>);
    expect(screen.getByText('出动时长耗尽 · 紧急迫降')).toBeVisible();
    expect(screen.getByLabelText('出动时长')).toHaveAttribute('value','0');
    expect(screen.getByText('蓄能耗尽，需接电或停靠供电设施充能')).toBeVisible();
    fireEvent.click(screen.getByRole('button',{name:'返回机场'}));expect(p.onReturn).toHaveBeenCalledOnce();
  });
  it('keeps enemy controls and reserves out of the inspector',()=>{
    render(<V6UnitStatus {...props({owner:2})} owned={false}/>);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('出动时长')).not.toBeInTheDocument();
  });
  it('retains unit roles and native costs when a catalog has already been normalized',()=>{
    const copy=normalizeV6Catalog({items:catalog}).items;
    expect(copy[0].role).toBe('ai');expect(copy[1].role).toBe('vehicle');
    expect(copy[0].computePerAttack).toBe(3);
  });
  it('shows public passive rules and native workshop discount without inventing stored compute',()=>{
    const p=props({pluginDiscount:.1},{...catalog[0],passive:{id:'pre-read',name:'预读侦察',description:'基础视野26格，保持墙体遮挡'}});
    render(<V6UnitStatus {...p}/>);
    expect(screen.getByText('预读侦察 · 被动')).toBeVisible();
    expect(screen.getByText(/安装经费优惠 10%，算力费用不变/)).toBeVisible();
    expect(screen.getByLabelText('随身算力')).toHaveAttribute('value','0');
  });
  it('keeps slot, unit tier and branch restrictions distinct',()=>{
    const plug={id:'speed-core',name:'核心',category:'plugins',role:'core',branch:'speed',tier:2,cost:200} as V6CatalogItem;
    expect(v6PluginLock(plug,undefined,[...catalog,plug],1)).toBe('先选中己方单位');
    expect(v6PluginLock(plug,unit({tier:1}),[...catalog,plug],1)).toBe('单位需升至T2');
    expect(v6PluginLock(plug,unit({branch:'science'}),[...catalog,plug],1)).toBe('需要同一科技分支');
    expect(v6PluginLock({...plug,id:'another-core'},unit({plugins:[plug.id]}),[...catalog,plug],1)).toBe('此类槽位已占用');
    expect(v6PluginLock(plug,unit(),[...catalog,plug],1)).toBe('');
  });
  it('quotes fractional payload use and paid skill amplification without ammunition',()=>{
    const plugins=normalizeV6Catalog({plugins:[{id:'payload',modifier:'payload-efficiency',magnitude:.15},{id:'compute',modifier:'compute-efficiency',magnitude:.15},{id:'skill',modifier:'skill-power',magnitude:.2}]}).items;
    const item={...catalog[0],id:'gemini',energyPerAttack:10,computePerAttack:4,skillCost:120};
    const result=v6UnitCosts(unit({plugins:['payload','compute','skill'],statuses:{'compute-efficiency':4,'gemini-ready':20}}),item,plugins);
    expect(result.energy).toBeCloseTo(8.5);expect(result.compute).toBeCloseTo(2.312);expect(result.skill).toBeCloseTo(88.128);
    expect(item.skillCost).toBe(120);
    expect('ammo' in result).toBe(false);
  });
});
