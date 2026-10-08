import {cleanup,render,screen} from '@testing-library/react';
import {afterEach,describe,expect,it} from 'vitest';
import V6UtilizationCard from '../src/components/game/v6/V6UtilizationCard';
import type {V6Preview} from '../src/lib/sentinelsV6';

afterEach(cleanup);

const preview=(patch:Partial<V6Preview>={}):V6Preview=>({
  valid:true,reason:'',cost:{credits:240,compute:0,science:0},
  powerBefore:100,powerAfter:100,demandBefore:40,demandAfter:60,
  netArea:12,capacity:3,costPerCapacity:80,
  computeBefore:20,computeAfter:20,computeCapacityBefore:200,computeCapacityAfter:200,
  ...patch,
});

describe('V6UtilizationCard',()=>{
  it('renders net area, capacity, cost per capacity and before/after power and compute',()=>{
    render(<V6UtilizationCard preview={preview()} mode="preview"/>);
    const card=screen.getByLabelText('空间与资源利用率');
    expect(card).toBeVisible();
    expect(screen.getByText('12 格')).toBeVisible();
    expect(screen.getByText('3')).toBeVisible();
    expect(screen.getByText(/◈ 80/)).toBeVisible();
    expect(screen.getByText(/40 \/ 100 → 60 \/ 100/)).toBeVisible();
    expect(screen.getByText(/20 \/ 200 → 20 \/ 200/)).toBeVisible();
  });
  it('shows invalid reason without inventing utilization numbers',()=>{
    render(<V6UtilizationCard preview={preview({valid:false,reason:'毛坯超出边界',netArea:undefined,capacity:undefined,costPerCapacity:undefined})}/>);
    expect(screen.getByText('预览无效')).toBeVisible();
    expect(screen.getByText('毛坯超出边界')).toBeVisible();
    expect(screen.queryByText('净面积')).not.toBeInTheDocument();
  });
  it('returns null when preview is absent',()=>{
    const {container}=render(<V6UtilizationCard preview={null}/>);
    expect(container).toBeEmptyDOMElement();
  });
});
