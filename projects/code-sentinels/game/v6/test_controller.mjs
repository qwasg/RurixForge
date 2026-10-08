/** Controller boundary tests with RPC double, not gameplay/simulation tests. */
import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {createSessionController} from './session-controller.mjs';
import {NET_PROTOCOL,NET_PREFIX} from '../multiplayer-v6.mjs';
const directory=fs.mkdtempSync(path.join(os.tmpdir(),'sentinels-v6-controller-'));
const calls=[];let state={version:6,tick:10,revision:10,seed:42,winner:null},gate=null;
const native={rulesHash:'a'.repeat(64),async rpc(method,input={}){calls.push({method,input});
 if(method==='game.session.catalog')return {rulesVersion:'v6.2',rulesFingerprint:'c'.repeat(64)};
 if(method==='game.session.open'){state={...state,tick:0,revision:1,seed:input.seed};return {snapshot:state};}
 if(method==='game.session.snapshot')return {...state};
 if(method==='game.session.order'){if(gate)await gate;return {accepted:true,tick:state.tick,sequence:input.sequence,reason:'adapter double'};}
 if(method==='game.session.preview')return {valid:true,cost:{credits:1,compute:0,science:0}};
 if(method==='game.session.save')return {rulesVersion:'v6.2',rulesFingerprint:'c'.repeat(64),snapshot:state,orders:[],sequences:[7,0],initialAi:false};
 if(method==='game.session.load'){state=input.save.snapshot;return {loaded:true};}
 if(method==='game.session.replay')return {playback:input.playback};
 return {};
}};
const controller=createSessionController({native,root:directory});const checks=[];
try{
 const first=await controller.session({mode:'solo',seed:43});assert.match(first.session.roomId,/^[a-f0-9-]{36}$/);
 await controller.camera({centerX:3,centerY:4,localPlayer:2});assert.equal(calls.at(-1).input.localPlayer,1);
 await controller.preview({sessionId:first.session.roomId,command:{op:'shield',enabled:true}});assert.equal(calls.at(-1).input.owner,1);
 await controller.pick({sessionId:first.session.roomId,screenX:10,screenY:20,width:1280,height:720,view:{localPlayer:2,layer:1}});assert.equal(calls.at(-1).method,'game.session.pick');assert.equal(calls.at(-1).input.view.localPlayer,1);
 checks.push('solo has stable UI epoch; camera and preview override caller ownership');
 await controller.pause({paused:true,sessionId:first.session.roomId});await controller.pause({paused:true,sessionId:first.session.roomId});
 assert.equal(calls.filter(c=>c.method==='play.pause').length,1);assert.equal((await controller.status()).session.paused,true);
 await controller.pause({paused:false});assert.equal(calls.filter(c=>c.method==='play.resume').length,1);
 checks.push('solo pause is idempotent and uses native play.pause/play.resume');
 await controller.leave();const second=await controller.session({mode:'solo',seed:44});assert.notEqual(second.session.roomId,first.session.roomId);
 const prior=calls.length;await assert.rejects(()=>controller.order({sessionId:first.session.roomId,seq:1,command:{op:'shield',enabled:true}}),e=>e.status===409);assert.equal(calls.length,prior);
 await assert.rejects(()=>controller.preview({sessionId:first.session.roomId,command:{op:'shield',enabled:true}}),e=>e.status===409);
 await assert.rejects(()=>controller.pick({sessionId:first.session.roomId,screenX:0,screenY:0,width:1280,height:720}),e=>e.status===409);
 checks.push('stale-session orders and previews rejected before native dispatch');
 let release;gate=new Promise(resolve=>release=resolve);
 const oldOrder=controller.order({sessionId:second.session.roomId,seq:1,command:{op:'shield',enabled:false}});oldOrder.catch(()=>{});
 await controller.leave();const third=await controller.session({mode:'solo',seed:45});release();gate=null;
 await assert.rejects(()=>oldOrder,e=>e.status===409);assert.equal((await controller.status()).session.lastSequence,0);
 checks.push('late native receipt never changes the next session sequence');
 const saved=await controller.save({name:'controller interface fixture'});await controller.leave();const loaded=await controller.load({id:saved.id});assert.notEqual(loaded.session.roomId,third.session.roomId);assert.equal(loaded.session.lastSequence,7);
 checks.push('load receives fresh UI epoch and restores owner-one sequence index zero');
 await controller.leave();const replay=await controller.load({id:saved.id},true);assert.equal(replay.session.replay,true);assert.equal(replay.session.lastSequence,0);assert.equal(calls.findLast(c=>c.method==='game.session.replay').input.playback,true);
 await controller.replayControl({paused:true,seekTick:5});assert.equal(calls.findLast(c=>c.method==='game.session.replayControl').input.seekTick,5);
 await assert.rejects(()=>controller.pause({paused:true}),e=>e.status===409);
 await assert.rejects(()=>controller.order({seq:1,command:{op:'shield',enabled:false}}),e=>e.status===409);
 await assert.rejects(()=>controller.save({}),e=>e.status===409);
 checks.push('replay requests real playback mode and forwards explicit controls');
 await controller.leave();
 const savedPath=path.join(directory,'.forge/save/v6',saved.id+'.json');const fixture=JSON.parse(fs.readFileSync(savedPath,'utf8'));
 fixture.mode='host';fixture.save.snapshot={...fixture.save.snapshot,theme:'river',tick:1200,winner:null};fixture.save.sequences=[7,11];fs.writeFileSync(savedPath,JSON.stringify(fixture));
 const loadCalls=calls.filter(c=>c.method==='game.session.load').length;
 const lobby=await controller.load({id:saved.id});assert.equal(lobby.session.mode,'host');assert.equal(lobby.session.status,'lobby');assert.equal(lobby.snapshot,null);assert.equal(lobby.session.lastSequence,7);assert.equal(calls.filter(c=>c.method==='game.session.load').length,loadCalls);
 const netBase='http://127.0.0.1:'+new URL('http://'+lobby.session.address).port;
 const netCall=async(route,input,token)=>{const r=await fetch(netBase+NET_PREFIX+route,{method:'POST',headers:{'content-type':'application/json',...(token?{authorization:'Bearer '+token}:{})},body:JSON.stringify(input)});assert.equal(r.status,200);return r.json();};
 const guest=await netCall('/join',{protocol:NET_PROTOCOL,rulesHash:native.rulesHash,code:lobby.session.code});assert.equal(guest.lastSequence,11);
 await controller.ready(true);await netCall('/ready',{ready:true},guest.token);const resumed=await controller.start();assert.equal(resumed.snapshot.tick,1200);
 const receipt=await controller.order({seq:8,sessionId:resumed.session.roomId,command:{op:'shield',enabled:true}});assert.equal(receipt.sequence,8);
 const peerReceipt=await netCall('/orders',{sequence:12,command:{op:'shield',enabled:false}},guest.token);assert.equal(peerReceipt.sequence,12);
 checks.push('unfinished PVP save opens fresh lobby, waits for both ready, restores native state and both sequence counters');
 const report={testedAt:new Date().toISOString(),passed:true,nativeGameplayTested:false,scope:'Session/ownership/lifecycle boundaries with an explicit native RPC double',checks,temporaryFixtureDirectory:directory};
 fs.writeFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)),'controller-tests.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}finally{await controller.leave();}
