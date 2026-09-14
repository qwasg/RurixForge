import {cleanup,fireEvent,render,screen} from '@testing-library/react';
import {afterEach,expect,it} from 'vitest';
import V6Guide from '../src/components/game/v6/V6Guide';
afterEach(cleanup);
it('opens specific network and logistics guidance without starting or modifying a battle',()=>{
  render(<V6Guide catalog={{items:[]}}/>);
  expect(screen.getByRole('heading',{name:'把第一座机房接通。'})).toBeVisible();
  fireEvent.click(screen.getByRole('button',{name:'电力与算力'}));
  expect(screen.getByText(/有线AI驻守接入点；无线覆盖内可移动/)).toBeVisible();
  expect(screen.getByRole('button',{name:'电力与算力'})).toHaveAttribute('aria-pressed','true');
  fireEvent.click(screen.getByRole('button',{name:'地面与空运'}));
  expect(screen.getByText(/两端都必须是己方已完成、供电的机场跑道/)).toBeVisible();
  fireEvent.click(screen.getByRole('button',{name:'攻防与目标'}));
  expect(screen.getByText(/18分钟后控制三个战略节点中的至少两个/)).toBeVisible();
});
