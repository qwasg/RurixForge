import {cleanup,fireEvent,render,screen} from '@testing-library/react';
import {afterEach,expect,it} from 'vitest';
import V6Guide from '../src/components/game/v6/V6Guide';
afterEach(cleanup);
it('opens space, network and sortie guidance without starting or modifying a battle',()=>{
  render(<V6Guide catalog={{items:[]}}/>);
  expect(screen.getByRole('heading',{name:'把第一座机房接通。'})).toBeVisible();
  fireEvent.click(screen.getByRole('button',{name:'空间与利用率'}));
  expect(screen.getByText(/净面积→容量、每容量造价/)).toBeVisible();
  fireEvent.click(screen.getByRole('button',{name:'电力与算力'}));
  expect(screen.getByText(/有线AI驻守接入点；无线覆盖内可移动/)).toBeVisible();
  expect(screen.getByRole('button',{name:'电力与算力'})).toHaveAttribute('aria-pressed','true');
  fireEvent.click(screen.getByRole('button',{name:'飞机整备'}));
  expect(screen.getByText(/空中飞行消耗出动时长/)).toBeVisible();
  fireEvent.click(screen.getByRole('button',{name:'攻防与目标'}));
  expect(screen.getByText(/18分钟后控制三个战略节点中的至少两个/)).toBeVisible();
});
