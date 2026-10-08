import { describe,expect,it } from 'vitest';
import { V6_INITIAL_CAMERA,cellAtV6,containsV6,dragRectV6,projectV6,rectOutlineV6,screenToWorldV6,skillOutlineV6,unprojectV6,validShellRectV6,wireRouteV6,worldToScreenV6 } from '../src/lib/sentinelsV6Geometry';
describe('V6 flat-world picking',()=>{
  it('round trips ground cells across camera positions, zoom and viewport aspect ratios',()=>{
    for(const zoom of [.4,1,1.8,4])for(const viewport of [{width:1280,height:720},{width:1920,height:1080},{width:900,height:1000}]) {
      const camera={...V6_INITIAL_CAMERA,x:105,y:64,zoom};
      const point={x:17.25,y:74.625,z:0};
      const p=screenToWorldV6(worldToScreenV6(point,camera,viewport),camera,viewport);
      expect(p.x).toBeCloseTo(point.x,8);expect(p.y).toBeCloseTo(point.y,8);expect(p.z).toBe(0);
    }
  });
  it('keeps reversed drag footprints and all four visible corners aligned',()=>{
    const rect=dragRectV6({x:8.9,y:19.1,z:0},{x:3.2,y:12.9,z:0});
    expect(rect).toEqual({x:3,y:12,z:0,w:6,h:8});
    const corners=rectOutlineV6(rect,V6_INITIAL_CAMERA,{width:1280,height:720});
    const back=corners.map(p=>screenToWorldV6(p,V6_INITIAL_CAMERA,{width:1280,height:720},0));
    expect(back[2].x).toBeCloseTo(9);expect(back[2].y).toBeCloseTo(20);
    expect(containsV6(rect,{x:8.99,y:19.99,z:0})).toBe(true);
    expect(containsV6(rect,{x:9,y:20,z:0})).toBe(false);
    expect(containsV6(rect,{x:4,y:14,z:1})).toBe(false);
  });
  it('rejects outside, non-zero z and malformed cells',()=>{
    for(const p of [{x:-.01,y:0,z:0},{x:128,y:0,z:0},{x:0,y:96,z:0},{x:0,y:0,z:1},{x:0,y:0,z:-1},{x:NaN,y:0,z:0}])expect(cellAtV6(p)).toBeNull();
    expect(cellAtV6({x:127.99,y:95.99,z:0})).toEqual({x:127,y:95,z:0});
    expect(validShellRectV6({x:104,y:72,z:0,w:24,h:24})).toBe(true);
    expect(validShellRectV6({x:105,y:72,z:0,w:24,h:24})).toBe(false);
    expect(validShellRectV6({x:10,y:10,z:0,w:3,h:4})).toBe(false);
    expect(validShellRectV6({x:10,y:10,z:1,w:4,h:4})).toBe(false);
  });
  it('uses a 2:1 diamond with altitude offset only',()=>{
    expect(projectV6({x:1,y:0,z:0})).toEqual({x:.5,y:-.25});
    expect(unprojectV6(projectV6({x:2,y:3,z:0}),0)).toEqual({x:2,y:3,z:0});
  });
  it('distinguishes range-limited lines, direction cones and friendly area targeting in world space',()=>{
    const origin={x:20,y:30,z:0},target={x:40,y:30,z:0},viewport={width:1280,height:720};
    const back=(shape:string)=>skillOutlineV6(shape,origin,target,10,3,2,60,V6_INITIAL_CAMERA,viewport).map(p=>screenToWorldV6(p,V6_INITIAL_CAMERA,viewport,0));
    const line=back('line');expect(line).toHaveLength(4);expect(line[1].x).toBeCloseTo(30);expect(line[1].y).toBeCloseTo(31);
    const cone=back('cone');expect(cone[0].x).toBeCloseTo(origin.x);expect(cone.at(-1)?.y).toBeCloseTo(31.5);
    const circle=back('target-friendly');expect(circle[0].x).toBeCloseTo(43);expect(circle[0].z).toBe(0);
  });
  it('routes same-layer wires orthogonally and rejects cross-layer requests',()=>{
    const route=wireRouteV6({x:10,y:10,z:0},{x:14,y:11,z:0})!;
    expect(route[0]).toEqual({x:10,y:10,z:0});
    expect(route.at(-1)).toEqual({x:14,y:11,z:0});
    expect(route.every((p,i)=>i===0||Math.abs(p.x-route[i-1].x)+Math.abs(p.y-route[i-1].y)===1)).toBe(true);
    expect(wireRouteV6({x:10,y:10,z:0},{x:14,y:11,z:1})).toBeNull();
  });
});
