import {cleanup,fireEvent,render,screen} from '@testing-library/react';
import {afterEach,describe,expect,it,vi} from 'vitest';
import V6CudaControl from '../src/components/game/v6/V6CudaControl';
afterEach(cleanup);
describe('V6 CUDA charge control',()=>{
  it('changes the native automatic charging option without pretending to clear existing shields',()=>{
    const onToggle=vi.fn(),regions=[{owner:1,anchor:{x:20,y:40,z:0},cells:[],current:300,capacity:600,network:{x:21,y:41,z:0}}];
    const {rerender}=render(<V6CudaControl enabled regions={regions} onToggle={onToggle}/>);
    fireEvent.click(screen.getByRole('button',{name:'停止自动充能'}));expect(onToggle).toHaveBeenCalledWith(false);
    expect(screen.getByText('1 个有效区域 · 300 / 600 护盾')).toBeVisible();expect(regions[0].current).toBe(300);
    rerender(<V6CudaControl enabled={false} regions={regions} onToggle={onToggle}/>);
    expect(screen.getByRole('button',{name:'恢复自动充能'})).toBeEnabled();
  });
  it('does not invent a saved toggle value before the authoritative state arrives',()=>{
    render(<V6CudaControl enabled={undefined} regions={[]} onToggle={vi.fn()}/>);
    expect(screen.getByRole('button',{name:'状态同步中'})).toBeDisabled();
  });
});
