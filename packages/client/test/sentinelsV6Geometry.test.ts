import { describe,expect,it } from 'vitest';
import { V6_INITIAL_CAMERA,cellAtV6,containsV6,dragRectV6,projectV6,rectOutlineV6,screenToWorldV6,skillOutlineV6,unprojectV6,validShellRectV6,wireRouteV6,worldToScreenV6 } from '../src/lib/sentinelsV6Geometry';
describe('V6 native-compatible picking',()=>{
  it.each([-2,-1,0,1,2,3,4,5])('round trips layer %s across camera positions, zoom and viewport aspect ratios',(z)=>{
    for(const zoom of [.4,1,1.8,4])for(const viewport of [{width:1280,height:720},{width:1920,height:1080},{width:900,height:1000}]) {
      const camera={...V6_INITIAL_CAMERA,x:105,y:64,zoom,layer:z};
      const point={x:17.25,y:74.625,z};
      const p=screenToWorldV6(worldToScreenV6(point,camera,viewport),camera,viewport);
      expect(p.x).toBeCloseTo(point.x,8);expect(p.y).toBeCloseTo(point.y,8);expect(p.z).toBe(z);
    }
  });
  it('picks a lower plane beneath an upper-floor camera without snapping to the wrong floor',()=>{
    const p={x:49.1,y:23.5,z:-2},camera={...V6_INITIAL_CAMERA,layer:5};
    const screen=worldToScreenV6(p,camera,{width:1280,height:720});
    const actual=screenToWorldV6(screen,camera,{width:1280,height:720},-2);
    expect(actual.x).toBeCloseTo(p.x);expect(actual.y).toBeCloseTo(p.y);
  });
  it('keeps reversed drag footprints and all four visible corners aligned',()=>{
    const rect=dragRectV6({x:8.9,y:19.1,z:1},{x:3.2,y:12.9,z:1});
    expect(rect).toEqual({x:3,y:12,z:1,w:6,h:8});
    const corners=rectOutlineV6(rect,V6_INITIAL_CAMERA,{width:1280,height:720});
    const back=corners.map(p=>screenToWorldV6(p,V6_INITIAL_CAMERA,{width:1280,height:720},1));
    expect(back[2].x).toBeCloseTo(9);expect(back[2].y).toBeCloseTo(20);
    expect(containsV6(rect,{x:8.99,y:19.99,z:1})).toBe(true);
    expect(containsV6(rect,{x:9,y:20,z:1})).toBe(false);
    expect(containsV6(rect,{x:4,y:14,z:0})).toBe(false);
  });
  it('rejects outside and malformed cells instead of buying on a clamped border',()=>{
    for(const p of [{x:-.01,y:0,z:0},{x:128,y:0,z:0},{x:0,y:96,z:0},{x:0,y:0,z:6},{x:NaN,y:0,z:0}])expect(cellAtV6(p)).toBeNull();
    expect(cellAtV6({x:127.99,y:95.99,z:-2})).toEqual({x:127,y:95,z:-2});
    expect(validShellRectV6({x:104,y:72,z:5,w:24,h:24})).toBe(true);
    expect(validShellRectV6({x:105,y:72,z:5,w:24,h:24})).toBe(false);
    expect(validShellRectV6({x:10,y:10,z:0,w:3,h:4})).toBe(false);
  });
  it('uses a 2:1 diamond and a separate height offset',()=>{
    expect(projectV6({x:1,y:0,z:0})).toEqual({x:.5,y:-.25});
    expect(unprojectV6(projectV6({x:2,y:3,z:4}),4)).toEqual({x:2,y:3,z:4});
  });
  it('distinguishes range-limited lines, direction cones and friendly area targeting in world space',()=>{
    const origin={x:20,y:30,z:2},target={x:40,y:30,z:2},viewport={width:1280,height:720};
    const back=(shape:string)=>skillOutlineV6(shape,origin,target,10,3,2,60,V6_INITIAL_CAMERA,viewport).map(p=>screenToWorldV6(p,V6_INITIAL_CAMERA,viewport,2));
    const line=back('line');expect(line).toHaveLength(4);expect(line[1].x).toBeCloseTo(30);expect(line[1].y).toBeCloseTo(31);
    const cone=back('cone');expect(cone[0].x).toBeCloseTo(origin.x);expect(cone.at(-1)?.y).toBeCloseTo(31.5);
    const circle=back('target-friendly');expect(circle[0].x).toBeCloseTo(43);expect(circle[0].z).toBe(2);
  });
  it('uses every intermediate shaft cell for a multi-floor elevator wire',()=>{
    const lift={owner:1,pos:{x:12,y:10,z:0},toLevel:3,kind:'elevator',hp:250};
    const route=wireRouteV6({x:10,y:10,z:0},{x:14,y:11,z:3},[lift],1)!;
    expect(route.filter(p=>p.x===12&&p.y===10).map(p=>p.z)).toEqual([0,1,2,3]);
    expect(route.every((p,i)=>i===0||Math.abs(p.x-route[i-1].x)+Math.abs(p.y-route[i-1].y)+Math.abs(p.z-route[i-1].z)===1)).toBe(true);
    expect(wireRouteV6({x:10,y:10,z:1},{x:14,y:11,z:2},[lift],1)?.some(p=>p.z===2)).toBe(true);
    expect(wireRouteV6({x:10,y:10,z:0},{x:14,y:11,z:3},[lift],2)).toBeNull();
  });
  it('stops when a set of shafts cannot reach the requested floor',()=>{
    const entries=Array.from({length:80},(_,i)=>({owner:1,pos:{x:10+i,y:20,z:0},toLevel:1,kind:'stairs',hp:250}));
    expect(wireRouteV6({x:10,y:10,z:0},{x:14,y:11,z:5},entries,1)).toBeNull();
  });
});
