import assert from 'node:assert/strict';
export function verifyOwnerView(snapshot,owner){
 const visible=new Set(snapshot.visible[owner-1].map(p=>[p.x,p.y,p.z].join(','))),seen=p=>visible.has([p.x,p.y,p.z].join(','));
 const border=r=>{for(let x=r.x;x<r.x+r.w;x++)if(seen({x,y:r.y,z:r.z})||seen({x,y:r.y+r.h-1,z:r.z}))return true;for(let y=r.y;y<r.y+r.h;y++)if(seen({x:r.x,y,z:r.z})||seen({x:r.x+r.w-1,y,z:r.z}))return true;return false;};
 assert.equal(snapshot.visible[2-owner].length,0,'other owner vision leaked');assert.equal(snapshot.explored[2-owner].length,0,'other owner explored map leaked');
 for(const b of snapshot.buildings)if(b.owner!==owner)assert.ok(border(b.rect),'hidden foreign building leaked');
 for(const r of snapshot.rooms)if(r.owner!==owner)assert.ok(seen({x:r.rect.x+Math.floor(r.rect.w/2),y:r.rect.y+Math.floor(r.rect.h/2),z:r.rect.z}),'hidden foreign room leaked');
 for(const u of snapshot.units)if(u.owner!==owner)assert.ok(seen(u.pos),'hidden foreign unit leaked');
 for(const l of snapshot.links)if(l.owner!==owner){assert.ok(l.path.every(seen),'foreign cable reveals hidden path');assert.equal((l.unitEndpoints||[]).length,0,'foreign AI cable bindings leaked');}
 assert.ok(snapshot.networkStores.every(n=>n.owner===owner),'foreign compute stores leaked');assert.ok(snapshot.powerGrids.every(n=>n.owner===owner),'foreign power topology leaked');
}
