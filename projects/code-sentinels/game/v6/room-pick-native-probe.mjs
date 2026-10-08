// Read-only diagnostic of a running local authority. Never changes view or world.
import net from 'node:net';
import fs from 'node:fs';
const port=Number(process.argv[2]);
const output=process.argv[3];
if(!port||!output) throw new Error('Usage: node room-pick-native-probe.mjs PORT OUTPUT.json');
const socket=net.createConnection({host:'127.0.0.1',port});
await new Promise((ok,no)=>{socket.once('connect',ok);socket.once('error',no);});
let buffer=Buffer.alloc(0),next=1;const pending=new Map();
socket.on('data',data=>{buffer=Buffer.concat([buffer,data]);while(buffer.length>=4){const size=buffer.readUInt32LE();if(buffer.length<size+4)break;const response=JSON.parse(buffer.subarray(4,size+4));buffer=buffer.subarray(size+4);const call=pending.get(response.id);if(call){pending.delete(response.id);clearTimeout(call.timer);response.error?call.no(new Error(response.error.message)):call.ok(response.result);}}});
function rpc(method,params){return new Promise((ok,no)=>{const id=next++;const timer=setTimeout(()=>{pending.delete(id);no(new Error(`timeout ${method}`));},30000);pending.set(id,{ok,no,timer});const body=Buffer.from(JSON.stringify({jsonrpc:'2.0',id,method,params}));const head=Buffer.alloc(4);head.writeUInt32LE(body.length);socket.write(Buffer.concat([head,body]));});}
try{
  const world=await rpc('game.session.snapshot',{owner:1});
  fs.writeFileSync(output.replace(/\.json$/,'.snapshot.json'),JSON.stringify(world));
  const rooms=world.rooms.filter(r=>r.owner===1&&r.rect.z===1);
  const view={centerX:16,centerY:48,zoom:2,layer:1,cutaway:true,localPlayer:1};
  const iso=(x,y,z)=>[(x-y)/2,-(x+y)/4+z*1.5];const center=iso(view.centerX,view.centerY,view.layer);const width=1280,height=720,half=24/view.zoom;
  const results=[];
  for(const room of rooms){const r=room.rect;for(const offset of [[.5,.5],[r.w/2,r.h/2],[r.w-.4,r.h-.4]]){
    const point=[r.x+offset[0],r.y+offset[1],r.z+.21];const p=iso(...point);
    const screenX=((p[0]-center[0])/(half*(width/height))+1)*width/2;
    const screenY=(1-(p[1]-center[1])/half)*height/2;
    results.push({room:room.id,rect:r,progress:room.progress,gpus:room.gpus.length,point,screenX,screenY,pick:await rpc('game.session.pick',{screenX,screenY,width,height,view})});
  }}
  const report={kind:'read-only-live-room-floor-pick',port,tick:world.tick,view,results};fs.writeFileSync(output,JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}finally{socket.destroy();}
