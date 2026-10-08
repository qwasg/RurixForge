/** Shared with the V6 native presentation: one microcell is a 1 × .5 diamond. Single ground plane z=0. */
export const V6_WORLD = { width: 128, height: 96, minLayer: 0, maxLayer: 0, orthoSize: 24, floorRise: 1.5 } as const;
export interface V6Point { x: number; y: number; z: number }
export interface V6Rect extends V6Point { w: number; h: number }
export interface V6Camera { x: number; y: number; zoom: number }
export interface V6Viewport { width: number; height: number }
export const V6_INITIAL_CAMERA: V6Camera = { x: 20, y: 48, zoom: 1.8 };
export function projectV6(point: V6Point) {
  return { x: (point.x - point.y) * .5, y: -(point.x + point.y) * .25 + point.z * V6_WORLD.floorRise };
}
export function unprojectV6(point: {x:number;y:number}, z = 0): V6Point {
  const sum = -4 * (point.y - z * V6_WORLD.floorRise);
  return { x: (sum + point.x * 2) / 2, y: (sum - point.x * 2) / 2, z };
}
export function worldToScreenV6(point: V6Point, camera: V6Camera, viewport: V6Viewport) {
  const origin = projectV6({x:camera.x,y:camera.y,z:0});
  const value = projectV6(point), scale = viewport.height * camera.zoom / (V6_WORLD.orthoSize * 2);
  return { x: viewport.width / 2 + (value.x - origin.x) * scale, y: viewport.height / 2 - (value.y - origin.y) * scale };
}
export function screenToWorldV6(point: {x:number;y:number}, camera: V6Camera, viewport: V6Viewport, z = 0): V6Point {
  const origin = projectV6({x:camera.x,y:camera.y,z:0});
  const scale = viewport.height * camera.zoom / (V6_WORLD.orthoSize * 2);
  return unprojectV6({x:origin.x+(point.x-viewport.width/2)/scale,y:origin.y-(point.y-viewport.height/2)/scale},z);
}
export function cellAtV6(point: V6Point): V6Point | null {
  if (![point.x,point.y,point.z].every(Number.isFinite)) return null;
  const {floor} = Math, x = floor(point.x), y = floor(point.y), z = floor(point.z);
  return x < 0 || y < 0 || x >= V6_WORLD.width || y >= V6_WORLD.height || z !== 0 ? null : {x,y,z};
}
export function dragRectV6(a: V6Point,b: V6Point): V6Rect {
  const x=Math.floor(Math.min(a.x,b.x)),y=Math.floor(Math.min(a.y,b.y));
  return {x,y,z:0,w:Math.floor(Math.max(a.x,b.x))-x+1,h:Math.floor(Math.max(a.y,b.y))-y+1};
}
export function containsV6(rect: V6Rect, point: V6Point) {
  return rect.z===point.z && point.x>=rect.x && point.y>=rect.y && point.x<rect.x+rect.w && point.y<rect.y+rect.h;
}
export function rectOutlineV6(rect: V6Rect,camera: V6Camera, viewport:V6Viewport) {
  return [[0,0],[rect.w,0],[rect.w,rect.h],[0,rect.h]].map(([dx,dy])=>worldToScreenV6({x:rect.x+dx,y:rect.y+dy,z:rect.z},camera,viewport));
}
export function validShellRectV6(rect: V6Rect) {
  return Number.isInteger(rect.x)&&Number.isInteger(rect.y)&&Number.isInteger(rect.z)&&Number.isInteger(rect.w)&&Number.isInteger(rect.h)
    && rect.w>=4&&rect.h>=4&&rect.w<=24&&rect.h<=24&&rect.x>=0&&rect.y>=0&&rect.x+rect.w<=128&&rect.y+rect.h<=96&&rect.z===0;
}
export function orthogonalPathV6(a:V6Point,b:V6Point):V6Point[]{
  if(a.z!==b.z||a.z!==0||![a.x,a.y,a.z,b.x,b.y,b.z].every(Number.isInteger))return [];
  const path=[{...a}];let x=a.x,y=a.y;
  while(x!==b.x){x+=Math.sign(b.x-x);path.push({x,y,z:0});}
  while(y!==b.y){y+=Math.sign(b.y-y);path.push({x,y,z:0});}
  return path;
}
/** Same-layer orthogonal wire path only (flat world, no shafts). */
export function wireRouteV6(a:V6Point,b:V6Point):V6Point[]|null{
  if(!cellAtV6(a)||!cellAtV6(b)||a.z!==b.z)return null;
  return orthogonalPathV6(a,b);
}
export function clampCameraV6(camera:V6Camera):V6Camera {
  return {...camera,x:Math.max(0,Math.min(128,camera.x)),y:Math.max(0,Math.min(96,camera.y)),zoom:Math.max(.4,Math.min(4,camera.zoom))};
}
/** Target outlines use world-space shapes, then the exact native isometric projection. */
export function skillOutlineV6(shape:string,origin:V6Point,target:V6Point,range:number,radius:number,width:number,angle:number,camera:V6Camera,viewport:V6Viewport){
  const distance=Math.hypot(target.x-origin.x,target.y-origin.y),heading=Math.atan2(target.y-origin.y,target.x-origin.x);
  const length=Math.min(distance,range),end={x:origin.x+Math.cos(heading)*length,y:origin.y+Math.sin(heading)*length,z:origin.z};
  const point=(x:number,y:number,z=origin.z)=>({x,y,z});let points:V6Point[];
  if(shape==='cone'){
    const a=(angle||60)*Math.PI/180,reach=radius>0?radius:range;points=[origin,...Array.from({length:25},(_,i)=>point(origin.x+Math.cos(heading-a/2+a*i/24)*reach,origin.y+Math.sin(heading-a/2+a*i/24)*reach))];
  }else if(shape==='line'||shape==='direction'){
    const nx=-Math.sin(heading)*(width||1)/2,ny=Math.cos(heading)*(width||1)/2;
    points=[point(origin.x+nx,origin.y+ny),point(end.x+nx,end.y+ny),point(end.x-nx,end.y-ny),point(origin.x-nx,origin.y-ny)];
  }else{
    const center=shape==='self'?origin:target,r=radius||.75;
    points=Array.from({length:40},(_,i)=>point(center.x+Math.cos(i*Math.PI/20)*r,center.y+Math.sin(i*Math.PI/20)*r,center.z));
  }
  return points.map(p=>worldToScreenV6(p,camera,viewport));
}
