/** Early real-native interface probe. This is not final content or gameplay acceptance. */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const origin=process.argv[2];if(!/^http:\/\/127\.0\.0\.1:\d+$/.test(origin))throw new Error('Pass owned local bridge origin');
const call=async(route,input)=>{const r=await fetch(origin+'/api/v6/'+route,{method:input===undefined?'GET':'POST',headers:input===undefined?{}:{'content-type':'application/json'},body:input===undefined?undefined:JSON.stringify(input),signal:AbortSignal.timeout(60000)});const v=await r.json();assert.equal(r.status,200,JSON.stringify(v));return v;};
const records=[];let ws;
try{
 const catalog=await call('catalog');assert.equal(catalog.version,6);records.push({case:'catalog pre-session',units:catalog.units.length});
 const session=await call('session',{mode:'solo',seed:123});assert.ok(session.session.roomId);assert.equal(session.snapshot.version,6);records.push({case:'native solo open',roomId:session.session.roomId,tick:session.snapshot.tick,credits:session.snapshot.players.map(p=>p.credits)});
 await new Promise(r=>setTimeout(r,1000));const state=await call('status');assert.ok(state.snapshot.tick>session.snapshot.tick);records.push({case:'native authority autonomous ticking',ticks:state.snapshot.tick-session.snapshot.tick});
 await call('camera',{centerX:10,centerY:48,zoom:1,layer:0,cutaway:true,localPlayer:2});
 const view=await call('viewport');assert.match(view.wsUrl,/^ws:\/\/(127\.0\.0\.1|localhost):/);
 let frameCount=0,firstFrame;
 await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('No native RGBA frame')),45000);ws=new WebSocket(view.wsUrl);ws.binaryType='arraybuffer';ws.addEventListener('open',()=>ws.send(JSON.stringify({type:'subscribe',width:1280,height:720,maxFps:40})));ws.addEventListener('error',()=>reject(new Error('Native frame socket error')));ws.addEventListener('message',event=>{if(typeof event.data==='string'){console.log(event.data);return;}const b=Buffer.from(event.data);if(b.readUInt32LE(0)!==0x31464746)return;frameCount++;firstFrame??={width:b.readUInt16LE(8),height:b.readUInt16LE(10),bytes:b.length};if(frameCount>=3){clearTimeout(timer);resolve();}});});
 assert.equal(firstFrame.width,1280);assert.equal(firstFrame.height,720);records.push({case:'local real Rurix RGBA stream',frames:frameCount,...firstFrame});
 const before=await call('status');const receipt=await call('order',{seq:1,command:{op:'shield',enabled:false}});assert.equal(receipt.accepted,true);const duplicate=await call('order',{seq:1,command:{op:'shield',enabled:false}});assert.deepEqual(receipt,duplicate);records.push({case:'native ordinary order and duplicate receipt',receipt});
 const save=await call('save',{name:'V6 interface probe'});assert.ok(save.id);records.push({case:'native full save',id:save.id,tick:save.tick});
 await call('leave',{});const loaded=await call('load',{id:save.id});assert.ok(loaded.session.roomId);assert.notEqual(loaded.session.roomId,session.session.roomId);assert.ok(loaded.snapshot.tick>=before.snapshot.tick);records.push({case:'native load and fresh UI epoch',tick:loaded.snapshot.tick});
 const report={testedAt:new Date().toISOString(),passed:true,scope:'Early native interface build only. No final animation, balance, pressure or LAN battle claim.',records};fs.writeFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)),'interface-probe.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}finally{ws?.close();await call('leave',{}).catch(()=>{});}
