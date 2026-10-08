import {cleanup,fireEvent,render,screen} from '@testing-library/react';
import {afterEach,expect,it,vi} from 'vitest';
import V6Objective,{v6OutcomeTitle} from '../src/components/game/v6/V6Objective';
import type {V6Snapshot} from '../src/lib/sentinelsV6';
function fixture(){return{tick:20*3600,winner:null,winReason:'',players:[{owner:1,dominance:0},{owner:2,dominance:315}],resources:[1,2,3].map(id=>({id,kind:'node',owner:2,pos:{x:64,y:id*24,z:0},capture:0,contested:false}))} as V6Snapshot;}
afterEach(cleanup);
it('shows the real enemy countdown and allows locating a threatened strategic node',()=>{
  const state=fixture(),onLocate=vi.fn();render(<V6Objective state={state} owner={1} coreHp={6000} catalog={{items:[]}} onLocate={onLocate}/>);
  expect(screen.getByText('敌方即将完成压制，争夺节点')).toBeVisible();
  expect(screen.getByLabelText('敌方压制')).toHaveAttribute('value','315');
  fireEvent.click(screen.getByRole('button',{name:'定位战略节点2'}));expect(onLocate).toHaveBeenCalledWith(state.resources[1]);
});
it('distinguishes a contested timer from losing the majority and rolling back',()=>{
  const state=fixture();state.resources[0].contested=true;state.players[0].dominance=80;
  render(<V6Objective state={state} owner={1} coreHp={6000} catalog={{items:[]}} onLocate={vi.fn()}/>);
  expect(screen.getByText('敌方 · 争夺暂停')).toBeVisible();expect(screen.getByText('己方 · 回退×2')).toBeVisible();
  expect(screen.queryByText('敌方即将完成压制，争夺节点')).not.toBeInTheDocument();
});
it('does not claim an intact command core was destroyed after a node victory',()=>{
  const state={...fixture(),winner:2,winReason:'节点压制达成'};
  expect(v6OutcomeTitle(state,1)).toBe('战略压制失利');expect(v6OutcomeTitle(state,2)).toBe('战略压制胜利');
  expect(v6OutcomeTitle({...state,winReason:'核心被摧毁'},1)).toBe('指挥核心已摧毁');
});
it('keeps the majority countdown active when only a third-party node is contested',()=>{
  const state=fixture();state.resources[0].owner=1;state.resources[0].contested=true;
  render(<V6Objective state={state} owner={1} coreHp={6000} catalog={{items:[]}} onLocate={vi.fn()}/>);
  expect(screen.getByText('敌方 · 压制中')).toBeVisible();
  expect(screen.getByText('敌方即将完成压制，争夺节点')).toBeVisible();
  expect(screen.queryByText('敌方 · 争夺暂停')).not.toBeInTheDocument();
});
