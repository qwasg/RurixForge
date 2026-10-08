/** Real localhost HTTP/SSE with explicit RPC doubles. No native executable or external connection. */
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {createSessionController} from './session-controller.mjs';
import {NET_PREFIX,NET_PROTOCOL} from '../multiplayer-v6.mjs';
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'sentinels-bootstrap-'));
let tick=0,firstApply=true,applyAttempts=0,lastApplied=0,opens=0,snapshotFailures=0,streamAttempts=0;
const hostNative={rulesHash:'b'.repeat(64),async rpc(method,input){if(method==='game.session.snapshot')return {version:6,tick:++tick,revision:tick,seed:55,phase:'running',winner:null,players:[{owner:1},{owner:2}],units:[],rooms:[],buildings:[],visible:[input.owner]};return {};}};
const guestNative={rulesHash:hostNative.rulesHash,async rpc(method,input){if(method==='game.session.open'){opens++;return {};}
 if(method==='game.session.applySnapshot'){applyAttempts++;if(firstApply){firstApply=false;throw new Error('injected first native apply failure');}lastApplied=input.snapshot.tick;return {applied:true};}return {};}};
const host=createSessionController({native:hostNative,root:path.join(dir,'host')});
const guest=createSessionController({native:guestNative,root:path.join(dir,'guest'),heartbeatMs:40,streamStaleMs:160});
const originalFetch=globalThis.fetch;
try {
 const lobby=await host.session({mode:'host',nickname:'Host',seed:55});const port=new URL('http://'+lobby.session.address).port;
 globalThis.fetch=(input,options={})=>{const url=new URL(typeof input==='string'?input:input.url);
   if(url.port===port&&url.pathname===NET_PREFIX+'/snapshot'&&snapshotFailures===0){snapshotFailures++;return Promise.resolve(new Response(JSON.stringify({error:{message:'injected initial snapshot HTTP failure'}}),{status:502,headers:{'content-type':'application/json'}}));}
   if(url.port===port&&url.pathname===NET_PREFIX+'/stream'){streamAttempts++;if(streamAttempts===1){return new Promise((resolve,reject)=>{const stop=()=>reject(new DOMException('injected silent stream aborted','AbortError'));if(options.signal?.aborted)stop();else options.signal?.addEventListener('abort',stop,{once:true});});}}
   return originalFetch(input,options);
 };
 await guest.session({mode:'join',address:'127.0.0.1:'+port,code:lobby.session.code.toLowerCase(),nickname:'Guest'});await host.ready(true);await guest.ready(true);await host.start();
 const limit=Date.now()+8000;let state;
 while(Date.now()<limit){state=await guest.status();if(snapshotFailures===1&&applyAttempts>=3&&streamAttempts>=2&&state.session.replicaSynced&&state.session.connected&&lastApplied>0)break;await new Promise(resolve=>setTimeout(resolve,20));}
 assert.equal(opens,1,'retry must reuse the already opened replica');assert.equal(snapshotFailures,1);assert.ok(applyAttempts>=3,'first apply failure must be retried');assert.ok(streamAttempts>=2,'silent initial stream must be aborted and retried');assert.equal(state.session.replicaSynced,true);assert.equal(state.session.connected,true);
 const previous=lastApplied;await new Promise(resolve=>setTimeout(resolve,130));assert.ok(lastApplied>previous,'recovered SSE must continue applying advancing snapshots');
 const report={testedAt:new Date().toISOString(),protocol:NET_PROTOCOL,passed:true,nativeAssertionsExecuted:false,scope:'Real localhost HTTP/SSE with explicit native RPC doubles; injected first snapshot HTTP failure, first apply failure and silent first stream',checks:['initial snapshot failure retries full synchronization without reopening replica','initial native apply failure does not strand replicaOpened state','silent stream watchdog aborts stale stream, reapplies a full snapshot and resumes advancing updates'],opens,snapshotFailures,applyAttempts,streamAttempts};
 fs.writeFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)),'reconnect-bootstrap-tests.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}finally{await guest.leave();await host.leave();globalThis.fetch=originalFetch;}
