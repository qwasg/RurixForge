import {verifyOwnerView} from './view-contract.mjs';
import {ConservativeActivity,EpochMovement,PostActivityMetrics,retainTimedSample,observeNative,stateHash,canonicalJson} from './lan-observations.mjs';
/** Real-time dual-native LAN combat soak. Never advances native time or creates resources.
 * --headless is an explicitly incomplete logic-only probe; final eligibility requires
 * complete media, both local Rurix streams and >=45 minutes of real wall-clock operation.
 */
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import http from 'node:http';
import {spawn} from 'node:child_process';import {createHash} from 'node:crypto';import {deflateSync} from 'node:zlib';import assert from 'node:assert/strict';
import {StringDecoder} from 'node:string_decoder';
import {fileURLToPath} from 'node:url';import {NET_PREFIX,applySnapshotEnvelope} from '../multiplayer-v6.mjs';
const dir=path.dirname(fileURLToPath(import.meta.url)),argv=process.argv.slice(2);
const driverSha256=createHash('sha256').update(fs.readFileSync(fileURLToPath(import.meta.url))).digest('hex');
const viewContractSha256=createHash('sha256').update(fs.readFileSync(path.join(dir,'view-contract.mjs'))).digest('hex');
const observationsSha256=createHash('sha256').update(fs.readFileSync(path.join(dir,'lan-observations.mjs'))).digest('hex');
const option=(name,fallback)=>{const i=argv.indexOf(name);return i<0?fallback:argv[i+1];};
const pack=path.resolve(option('--pack',path.join(dir,'../../dist/CodeSentinels-V6-Windows')));
const short=argv.includes('--short'),projectOption=option('--project',null),project=projectOption?path.resolve(projectOption):null;
assert.ok(!project||short,'source-project launches are an intermediate short probe, not final packaged acceptance');
const duration=Number(option('--seconds',short?'180':'2700')),render=!argv.includes('--headless');assert.ok(duration>=60&&duration<=5400);
const stopFile=option('--stop-file',null);
const stamp=new Date().toISOString().replace(/[:.]/g,'-'),out=path.resolve(option('--out',path.join(dir,'lan-runs',stamp)));
assert.ok(!fs.existsSync(out),'preserve prior evidence');fs.mkdirSync(out,{recursive:true});
const marker=JSON.parse(fs.readFileSync(path.join(pack,'v6-candidate.json'),'utf8'));assert.equal(marker.version,6);
const target=marker.target;assert.ok(target?.rulesVersion&&target?.rulesFingerprint&&target?.payloadSha256,'Use a schema2 candidate with observed native rule identity');
const fileHash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const runtimeFile=relative=>/^(bin|Content|Web|v6)\//.test(relative)||['bridge.mjs','multiplayer-v6.mjs','forge.toml'].includes(relative)||/^Start-Game[^/]*\.cmd$/.test(relative);
function verifyPayload(root){const rows=marker.included.filter(runtimeFile).sort().map(relative=>{const file=path.resolve(root,relative);assert.ok(file.startsWith(root+path.sep));return{bytes:fs.statSync(file).size,path:relative,sha256:fileHash(file)};});const hash=stateHash(rows);assert.equal(hash,target.payloadSha256);return hash;}
if(!project)verifyPayload(pack);
if(render&&!short)assert.equal(marker.mediaReady,true,'final rendering soak requires finished real media');
const engineFile=path.resolve(option('--engine',path.join(pack,'bin/engine-host.exe')));
const engineHash=createHash('sha256').update(fs.readFileSync(engineFile)).digest('hex');if(!project)assert.equal(engineHash,marker.engineSha256);
const scratch=fs.mkdtempSync(path.join(os.tmpdir(),'sentinels-v6-lan-soak-'));
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const checks=[],samples=[],clients=[],proxies=[],combatSaves=[],coverage=new Set();let failure,epochs=0,consistency=null,replayExact=false;
const postActivityMetrics=new PostActivityMetrics();
async function collectPostActivityMetrics(reason){
 postActivityMetrics.end(reason);
 try{const diagnostic=await postActivityMetrics.collect(clients,(name,value)=>fs.writeFileSync(path.join(out,name),JSON.stringify(value,null,2)));fs.writeFileSync(path.join(out,'post-activity-metrics.json'),JSON.stringify(diagnostic,null,2));}
 catch(error){console.error('Optional post-activity metrics diagnostic failed:',error.message);}
}
function record(name,detail={}){const row={at:new Date().toISOString(),case:name,...detail};checks.push(row);fs.appendFileSync(path.join(out,'events.jsonl'),JSON.stringify(row)+'\n');console.log(JSON.stringify(row));}
function observeMovement(client,unit){const movement=client.movement.observe(unit.id,[unit.x,unit.y,unit.z]);if(movement)record('same-epoch native unit position changed',{client:client.name,...movement});}
async function call(client,route,input){const response=await fetch(client.base+'/api/v6/'+route,{method:input===undefined?'GET':'POST',headers:input===undefined?{}:{'content-type':'application/json'},body:input===undefined?undefined:JSON.stringify(input),signal:AbortSignal.timeout(25000)});const value=await response.json();if(!response.ok)throw Object.assign(new Error(value.error?.message||JSON.stringify(value)),{status:response.status});return value;}
async function until(probe,label,timeout=20000){const end=Date.now()+timeout;while(Date.now()<end){const value=await probe();if(value)return value;await sleep(100);}throw new Error('timeout: '+label);}
async function createClient(name){
 const root=project||path.join(scratch,name);if(!project){fs.mkdirSync(root);for(const relative of marker.included){const target=path.resolve(root,relative);assert.ok(target.startsWith(root+path.sep));fs.mkdirSync(path.dirname(target),{recursive:true});fs.copyFileSync(path.join(pack,relative),target);}}
 if(!project)verifyPayload(root);
 const child=spawn(project?process.execPath:path.join(root,'bin/node.exe'),[project?path.join(project,'game/portable-bridge-v6.mjs'):path.join(root,'bridge.mjs'),...(project?['--root',project,'--web',path.join(pack,'Web'),'--engine',engineFile]:[]),'--no-open','--control-stdin','--test-advice'],{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe']});
 const movement=new EpochMovement();const client={name,root,child,base:null,sessionId:null,nextSequence:1,pending:null,accepted:0,rejected:0,retries:0,frames:0,lastFrame:null,lastFrameAt:0,statuses:[],streamErrors:[],totals:{},maxTick:0,movement,positions:movement.positions,moved:movement.moved,movedInEpoch:movement.current};clients.push(client);
 const log=fs.createWriteStream(path.join(out,name+'-process.log'));
 await new Promise((resolve,reject)=>{let text='';const timer=setTimeout(()=>reject(new Error(name+' startup timeout')),60000);child.on('error',reject);child.on('exit',code=>{if(code)reject(new Error(name+' exited '+code));});child.stderr.on('data',data=>log.write(data));child.stdout.on('data',data=>{log.write(data);text+=data;const match=/Code Sentinels V6 is ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(text);if(match){client.base=match[1];clearTimeout(timer);resolve();}});});
 client.health=await(await fetch(client.base+'/health')).json();
 const launch=JSON.parse(fs.readFileSync(path.join(root,'Logs/v6/last-launch.json'),'utf8'));assert.equal(launch.enginePid,client.health.enginePid);
 client.native=await observeNative(launch.enginePort);const catalog=await client.native.rpc('game.session.catalog');assert.equal(catalog.version,6);
 client.identity={engineSha256:fileHash(project?engineFile:path.join(root,'bin/engine-host.exe')),rulesVersion:catalog.rulesVersion,rulesFingerprint:catalog.rulesFingerprint,payloadSha256:project?null:verifyPayload(root)};
 if(!project)for(const key of['engineSha256','rulesVersion','rulesFingerprint','payloadSha256'])assert.equal(client.identity[key],target[key]);
 fs.writeFileSync(path.join(out,name+'-native-catalog.json'),JSON.stringify(catalog));client.completedRooms=0;return client;
}
async function proxyFor(port){
 const streams=new Set(),history=new Map();let blockedUntil=0,requestNumber=0,snapshotCount=0,streamBytes=0,delayedRequests=0,authorization=null;
 const ingest=(previous,envelope)=>{const snapshot=applySnapshotEnvelope(previous,envelope);verifyOwnerView(snapshot,2);history.set(snapshot.tick+':'+snapshot.revision,snapshot);while(history.size>96)history.delete(history.keys().next().value);return snapshot;};
 const server=http.createServer(async(req,res)=>{
  if(!req.url.startsWith(NET_PREFIX+'/')){res.writeHead(404);res.end();return;}
  if(Date.now()<blockedUntil){req.socket.destroy();return;}
  if(req.headers.authorization)authorization=req.headers.authorization;
  const parts=[];for await(const chunk of req)parts.push(chunk);const body=Buffer.concat(parts);
  const number=++requestNumber;if(number%17===0){delayedRequests++;await sleep(120);}
  if(Date.now()<blockedUntil){res.destroy();return;}
  const upstream=http.request({hostname:'127.0.0.1',port,path:req.url,method:req.method,headers:{...req.headers,host:`127.0.0.1:${port}`}},answer=>{
   res.writeHead(answer.statusCode,answer.headers);const isStream=req.url===NET_PREFIX+'/stream',isSnapshot=req.url===NET_PREFIX+'/snapshot',decoder=new StringDecoder('utf8');let text='',previous=null;
   if(isStream){streams.add(res);res.on('close',()=>{streams.delete(res);upstream.destroy();});}
   answer.on('data',chunk=>{if(isStream||isSnapshot)text+=decoder.write(chunk);if(isStream){streamBytes+=chunk.length;let boundary;while((boundary=text.indexOf('\n\n'))!==-1){const block=text.slice(0,boundary);if(block.includes('event: snapshot')){snapshotCount++;const data=block.split('\n').filter(line=>line.startsWith('data: ')).map(line=>line.slice(6)).join('\n');try{previous=ingest(previous,JSON.parse(data));}catch(error){failure??=error;}}text=text.slice(boundary+2);}}});
   answer.on('end',()=>{text+=decoder.end();if(isSnapshot&&answer.statusCode===200){try{ingest(null,JSON.parse(text));}catch(error){failure??=error;}}});answer.pipe(res);
  });
  upstream.on('error',()=>res.destroy());upstream.end(body);
 });
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const proxy={port:server.address().port,history,snapshots:()=>snapshotCount,bytes:()=>streamBytes,delays:()=>delayedRequests,requests:()=>requestNumber,blocked:()=>Date.now()<blockedUntil,
  async ordinaryGuestLeave(){assert.ok(authorization);const response=await fetch(`http://127.0.0.1:${server.address().port}${NET_PREFIX}/leave`,{method:'POST',headers:{authorization,'content-type':'application/json'},body:'{}'});assert.equal(response.status,200);record('ordinary authenticated guest leave exercises an early terminal epoch');},
  outage(ms){blockedUntil=Date.now()+ms;for(const s of streams)s.destroy();record('intentional network outage',{milliseconds:ms});},
  async close(){if(!server.listening)return;for(const s of streams)s.destroy();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}};
 proxies.push(proxy);return proxy;
}
function crc32(bytes){let crc=0xffffffff;for(const b of bytes){crc^=b;for(let i=0;i<8;i++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);}return(crc^0xffffffff)>>>0;}
function png(frame){const chunk=(tag,data)=>{const t=Buffer.from(tag),n=Buffer.alloc(4),crc=Buffer.alloc(4);n.writeUInt32BE(data.length);crc.writeUInt32BE(crc32(Buffer.concat([t,data])));return Buffer.concat([n,t,data,crc]);};const head=Buffer.alloc(13);head.writeUInt32BE(1280);head.writeUInt32BE(720,4);head[8]=8;head[9]=6;const raw=Buffer.alloc(720*(1280*4+1));for(let y=0;y<720;y++)frame.copy(raw,y*(1280*4+1)+1,20+y*1280*4,20+(y+1)*1280*4);return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',head),chunk('IDAT',deflateSync(raw)),chunk('IEND',Buffer.alloc(0))]);}
function visualColors(frame){const colors=new Set();for(let p=20;p<frame.length;p+=4*997){colors.add(((frame[p]>>3)<<10)|((frame[p+1]>>3)<<5)|(frame[p+2]>>3));}return colors.size;}
async function frames(client){const view=await call(client,'viewport');const ws=new WebSocket(view.wsUrl);client.ws=ws;ws.binaryType='arraybuffer';
 await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error(client.name+' no local Rurix frame')),45000);ws.addEventListener('open',()=>ws.send(JSON.stringify({type:'subscribe',width:1280,height:720,maxFps:40})));ws.addEventListener('error',()=>{if(client.ws!==ws)return;client.streamErrors.push('socket-error');reject(new Error('native frame socket failed'));});ws.addEventListener('message',event=>{if(client.ws!==ws)return;try{if(typeof event.data==='string'){const value=JSON.parse(event.data);if(value.type==='error'||value.shareError)throw new Error(JSON.stringify(value));if(value.type==='status'){assert.equal(value.truncated,false);client.statuses.push(value);if(client.statuses.length>30)client.statuses.shift();}return;}const b=Buffer.from(event.data);assert.equal(b.length,20+1280*720*4);assert.equal(b.readUInt32LE(),0x31464746);assert.equal(b.readUInt16LE(8),1280);assert.equal(b.readUInt16LE(10),720);assert.equal(b.readUInt32LE(12)&2,0);client.frames++;client.lastFrame=b;client.lastFrameAt=performance.now();clearTimeout(timer);resolve();}catch(error){client.streamErrors.push(error.message);clearTimeout(timer);reject(error);}});});
 await until(()=>client.lastFrame&&visualColors(client.lastFrame)>24,'non-black textured '+client.name+' frame');fs.writeFileSync(path.join(out,`${client.name}-epoch-${epochs}-start.png`),png(client.lastFrame));
}
async function closeFrames(client){const ws=client.ws;if(!ws)return;client.ws=null;ws.close();await Promise.race([new Promise(resolve=>ws.addEventListener('close',resolve,{once:true})),sleep(1500)]);}
async function begin(blue,red,seed){
 const host=await call(blue,'session',{mode:'host',seed,theme:'river',nickname:'Blue native test'});return joinHostLobby(blue,red,host);
}
async function joinHostLobby(blue,red,host){
 blue.sessionId=host.session.roomId;blue.nextSequence=(host.session.lastSequence||0)+1;blue.pending=null;blue.adviceQueue=[];
 for(const c of[blue,red])for(const key of['factorySite','factoryWind','manualMove'])delete c[key];
 const port=new URL('http://'+host.session.address).port;const proxy=await proxyFor(Number(port));
 const joined=await call(red,'session',{mode:'join',address:'127.0.0.1:'+proxy.port,code:host.session.code,nickname:'Red native test'});red.sessionId=joined.session.roomId;red.nextSequence=(joined.session.lastSequence||0)+1;red.pending=null;red.adviceQueue=[];
 for(const c of[blue,red])c.movement.begin(c.sessionId);
 await Promise.all([call(blue,'ready',{ready:true}),call(red,'ready',{ready:true})]);await call(blue,'start',{});
 await until(async()=>{const s=await call(red,'status');return s.snapshot&&s.snapshot.tick>=Math.max(1,host.session.resumeTick||0);},'red full snapshot');
 await assert.rejects(()=>call(blue,'pause',{paused:true}),e=>e.status===409);await assert.rejects(()=>call(red,'pause',{paused:true}),e=>e.status===409);
 record('new real LAN battle',{epoch:++epochs,seed:host.session.seed,resumed:host.session.resumed||false,blueBridge:blue.child.pid,redBridge:red.child.pid,blueEngine:blue.health.enginePid,redEngine:red.health.enginePid});return proxy;
}
async function shortMobilePlan(client,branch){
 const state=await call(client,'status'),s=state.snapshot,owner=state.playerId;if(!s)return null;
 const units=s.units.filter(u=>u.owner===owner);for(const u of units)observeMovement(client,u);
 const lab=s.rooms.find(r=>r.owner===owner&&r.kind==='research-lab'&&r.progress>=1&&r.powered&&r.connected);if(!lab)return undefined;
 client.catalog??=await call(client,'catalog');const definitions=client.catalog.units;
 const mobile=units.find(u=>definitions.find(d=>d.id===u.kind)?.category==='vehicle');
 const pos=(x,y,z=0)=>({x,y,z}),line=(a,b)=>{const out=[a];let p={...a};for(const k of['x','y','z'])while(p[k]!==b[k]){p={...p,[k]:p[k]+Math.sign(b[k]-p[k])};out.push(p);}return out;};
 const valid=async command=>(await call(client,'preview',{sessionId:client.sessionId,command})).valid;
 if(mobile){if(client.movedInEpoch.has(mobile.id))return undefined;if(!client.manualMove){for(const goal of [pos(mobile.pos.x+4,mobile.pos.y),pos(mobile.pos.x-4,mobile.pos.y),pos(mobile.pos.x,mobile.pos.y+4)]){const command={op:'move',ids:[mobile.id],pos:goal};if(await valid(command)){client.manualMove=true;return command;}}}return null;}
 if(!client.factorySite){const empty=s.buildings.find(b=>b.owner===owner&&b.kind==='shell'&&b.rect.z===0&&b.rect.w>=6&&b.rect.h>=4&&!s.rooms.some(r=>r.shell===b.id));if(empty)client.factorySite=empty.rect;}
 if(!client.factorySite){for(const [blueX,y]of[[2,38],[2,54],[12,34],[2,30]]){const x=owner===1?blueX:128-blueX-6;const rect={x,y,z:0,w:6,h:4};if(await valid({op:'shell',rect})){client.factorySite=rect;break;}}if(!client.factorySite)return undefined;}
 const rect=client.factorySite;let shell=s.buildings.find(b=>b.owner===owner&&b.kind==='shell'&&JSON.stringify(b.rect)===JSON.stringify(rect));
 // Native field order need not match the authored JSON object's property order.
 shell??=s.buildings.find(b=>b.owner===owner&&b.kind==='shell'&&['x','y','z','w','h'].every(k=>b.rect[k]===rect[k]));
 if(!shell)return {op:'shell',rect};if(shell.progress<1)return null;
 const doorPos=pos(rect.x+1,rect.y+3);if(!s.entrances.some(e=>e.owner===owner&&e.kind==='door'&&e.pos.x===doorPos.x&&e.pos.y===doorPos.y&&e.pos.z===0))return {op:'entrance',kind:'door',pos:doorPos,toLevel:0,width:3};
 const room=s.rooms.find(r=>r.owner===owner&&r.shell===shell.id&&r.kind==='factory');if(!room)return {op:'room',shell:shell.id,rect:{x:rect.x+1,y:rect.y+1,z:0,w:4,h:2},kind:'factory',branch:null};if(room.progress<1)return null;
 if(!client.factoryWind){for(const p of[pos(rect.x+6,rect.y),pos(rect.x-4,rect.y),pos(rect.x,rect.y-4)]){if(await valid({op:'build',kind:'wind-power',pos:p})){client.factoryWind=p;break;}}if(!client.factoryWind)return null;}
 const w=client.factoryWind,wind=s.buildings.find(b=>b.owner===owner&&b.kind==='wind-power'&&b.rect.x===w.x&&b.rect.y===w.y);if(!wind)return {op:'build',kind:'wind-power',pos:w};if(wind.progress<1)return null;
 if(!room.powered){const command={op:'wire',kind:'power',path:line(pos(w.x+1,w.y+1),pos(room.rect.x+1,room.rect.y+1)),unitEndpoints:[]};return await valid(command)?command:null;}
 const scout=definitions.find(d=>d.branch===branch&&d.category==='vehicle'&&d.tier===1&&d.chassis==='scout');assert.ok(scout,'native catalog needs an initial moving vehicle');
 const command={op:'deploy',room:room.id,kind:scout.id,pos:pos(rect.x+2,rect.y+4)};return await valid(command)?command:null;
}
async function act(client,branch){
 try{
  for(let issued=0;issued<2;issued++){
   if(!client.pending){
    if(!client.adviceQueue?.length){if(issued>0)return;const direct=short?await shortMobilePlan(client,branch):undefined;if(direct===null)return;const advice=direct?{command:direct}:await call(client,'suggest',{sessionId:client.sessionId,branch,style:'mixed-ai'});client.adviceQueue=(Array.isArray(advice.commands)?advice.commands:advice.command?[advice.command]:[]).slice(0,2);}
    const command=client.adviceQueue.shift();if(!command)return;client.pending={sessionId:client.sessionId,seq:client.nextSequence,command};
   }
   const receipt=await call(client,'order',client.pending);if(receipt.accepted)client.accepted++;else client.rejected++;
   if(client.nextSequence%13===0){const again=await call(client,'order',client.pending);assert.deepEqual(again,receipt);client.retries++;}
   client.nextSequence++;client.pending=null;
  }
 }catch(error){if(error.status===409){const state=await call(client,'status').catch(()=>null);if(state?.session?.status==='finished')client.pending=null;}else if(error.status&&error.status<500){throw error;}else client.retries++;}
}
async function shortOwnershipCheck(blue,red){
 const b=await call(blue,'status');const core=b.snapshot.buildings.find(x=>x.kind==='core'&&x.owner===1);assert.ok(core);
 const foreign={sessionId:red.sessionId,seq:red.nextSequence,command:{op:'repair',id:core.id}};
 const receipt=await call(red,'order',foreign);assert.equal(receipt.accepted,false,'guest may not repair host core');assert.deepEqual(await call(red,'order',foreign),receipt);red.nextSequence++;red.rejected++;red.retries++;
 await assert.rejects(()=>call(red,'order',{sessionId:red.sessionId,seq:red.nextSequence,command:{op:'shield',owner:1,enabled:false}}),e=>e.status===400);
 record('native owner binding and duplicate rejected receipt',{hostCore:core.id,receipt});
 coverage.add('ownership');coverage.add('deduplication');
}
async function alignReplica(client,proxy){
 for(let i=0;i<12;i++){
  const replica=await client.native.rpc('game.session.snapshot',{owner:2});const authority=proxy.history.get(replica.tick+':'+replica.revision);
  if(authority&&replica.tick>0){const a=stateHash(authority),b=stateHash(replica);assert.equal(b,a,'actual red native snapshot differs from the same-tick owner2 authority envelope');
   const prefix=`aligned-epoch-${epochs}-${replica.tick}-${replica.revision}`;fs.writeFileSync(path.join(out,prefix+'-authority-owner2.json'),JSON.stringify(authority));fs.writeFileSync(path.join(out,prefix+'-red-native.json'),JSON.stringify(replica));
   consistency={epoch:epochs,tick:replica.tick,revision:replica.revision,owner:2,authorityOwnerViewSha256:a,replicaSha256:b,authorityEvidence:prefix+'-authority-owner2.json',replicaEvidence:prefix+'-red-native.json'};record('actual native owner2 replica aligns with observed authority envelope',consistency);return true;
  }await sleep(25);
 }return false;
}
async function saveCombat(client,name){const saved=await call(client,'save',{name});const stored=JSON.parse(fs.readFileSync(path.join(client.root,'.forge/save/v6',saved.id+'.json'),'utf8'));assert.equal(stored.save.rulesFingerprint,client.identity.rulesFingerprint);const raw=`combat-epoch-${epochs}-${saved.id}.json`;fs.writeFileSync(path.join(out,raw),JSON.stringify(stored));combatSaves.push({...saved,epoch:epochs,save:stored.save,evidence:raw});record('full native combat epoch save',{id:saved.id,tick:saved.tick,epoch:epochs,winner:stored.save.snapshot.winner,evidence:raw});return saved;}
let blue,red,proxy;let wallStarted,seconds=0,wallSeconds=0;const activity=new ConservativeActivity();let allCombat=[0,0],allOre=[0,0];
try{
 [blue,red]=await Promise.all([createClient('blue'),createClient('red')]);assert.notEqual(blue.health.enginePid,red.health.enginePid);
 proxy=await begin(blue,red,811);if(render)await Promise.all([frames(blue),frames(red)]);await shortOwnershipCheck(blue,red);
 wallStarted=performance.now();let nextOrder=0,nextSample=0,nextCapture=60,shortDrop=false,longDrop=false,lastDropEnd=0,epochEndAccounted=false,cameraChecked=false,recoveredDrop=false,terminalExercised=false;
 let lastFrames=[blue.frames,red.frames],lastSample=wallStarted,lastTicks=null;
 while(activity.seconds<duration){
  if(failure)throw failure;if(stopFile&&fs.existsSync(stopFile))throw new Error('Owned diagnostic stopped by explicit stop-file request');wallSeconds=(performance.now()-wallStarted)/1000;seconds=activity.seconds;
  assert.ok(wallSeconds<duration*3+600,'insufficient observed active time within bounded soak watchdog');
  if(wallSeconds>=(short?30:180)&&!shortDrop){activity.reset(performance.now()+8000+3000);proxy.outage(8000);shortDrop=true;lastDropEnd=Date.now()+8000;}
  if(!short&&wallSeconds>=600&&!longDrop){activity.reset(performance.now()+45000+3000);proxy.outage(45000);longDrop=true;lastDropEnd=Date.now()+45000;}
  if(short&&argv.includes('--exercise-epoch-end')&&!terminalExercised&&wallSeconds>=duration-20){await proxy.ordinaryGuestLeave();terminalExercised=true;}
  if(wallSeconds>=nextOrder){await Promise.all([act(blue,'algorithm'),act(red,'speed')]);nextOrder=(performance.now()-wallStarted)/1000+(short?1:3);}
  if(wallSeconds>=nextSample){
   const states=await Promise.all([call(blue,'status'),call(red,'status')]);const snapshots=states.map(s=>s.snapshot);const now=performance.now();
   const sampleSeconds=(now-lastSample)/1000;const fps=clients.map((c,i)=>(c.frames-lastFrames[i])/sampleSeconds);lastFrames=[blue.frames,red.frames];lastSample=now;
   if(snapshots.every(Boolean)){
    const ticks=snapshots.map(s=>s.tick);const lag=ticks[0]-ticks[1];const stable=!proxy.blocked()&&Date.now()>lastDropEnd+3000;
    const activityIntervalMs=activity.observe(epochs,now,snapshots,stable);seconds=activity.seconds;
    const sample={sampleId:samples.length+1,seconds,wallSeconds:(now-wallStarted)/1000,activityIntervalMs,epoch:epochs,ticks,lag,stable,proxySnapshots:proxy.snapshots(),proxyBytes:proxy.bytes(),delayedRequests:proxy.delays(),fps:render?fps:null,units:snapshots.map(s=>s.units.length),rooms:snapshots.map(s=>s.rooms.length),totals:snapshots.map((s,i)=>s.players.find(p=>p.owner===i+1)?.totals||{}),winner:snapshots[0].winner};samples.push(sample);
    if(lastTicks&&activityIntervalMs>0&&seconds>30)sample.authorityTicksPerSecond=(ticks[0]-lastTicks[0])/sampleSeconds;
    await retainTimedSample(sample,states,{
     persistSample:row=>fs.appendFileSync(path.join(out,'samples.jsonl'),JSON.stringify(row)+'\n'),
     validate:()=>{
      if(stable&&snapshots[0].winner===null)assert.ok(Math.abs(lag)<=60,'replica lags more than one second outside intentional outage');
      for(let i=0;i<2;i++){verifyOwnerView(snapshots[i],i+1);const p=snapshots[i].players.find(p=>p.owner===i+1);clients[i].totals=p.totals||{};clients[i].completedRooms=Math.max(clients[i].completedRooms,snapshots[i].rooms.filter(r=>r.owner===i+1&&r.progress>=1).length);const enemy=snapshots[i].players.find(p=>p.owner===2-i);assert.equal(enemy.credits,0,'enemy credit bank leaked');assert.equal(enemy.science,0,'enemy research bank leaked');for(const u of snapshots[i].units.filter(u=>u.owner===i+1))observeMovement(clients[i],u);}
      if(shortDrop&&stable&&!recoveredDrop){assert.ok(snapshots[1].tick>0&&!snapshots[1].winner,'short drop must not forfeit peer');recoveredDrop=true;coverage.add('disconnect-recovery');record('short outage recovered into real peer snapshot',{ticks,lag});}
      if(!short&&sample.authorityTicksPerSecond!==undefined){const tickRate=sample.authorityTicksPerSecond;assert.ok(tickRate>=57&&tickRate<=63,`authority clock rate outside 60Hz tolerance: ${tickRate}`);}
     },
     persistFailure:({sample:failed,states:observed,error})=>{
      const name=`failed-sample-${failed.sampleId}-states.json`,bytes=Buffer.from(JSON.stringify(observed));
      fs.writeFileSync(path.join(out,name),bytes,{flag:'wx'});
      const retained={sampleId:failed.sampleId,epoch:failed.epoch,message:error.message,sample:failed,stateEvidence:{path:name,sha256:createHash('sha256').update(bytes).digest('hex')}};
      fs.appendFileSync(path.join(out,'failed-samples.jsonl'),JSON.stringify(retained)+'\n');
     }
    });
    lastTicks=ticks;
    if(stable&&(!consistency||consistency.epoch!==epochs))await alignReplica(red,proxy);
    if(render){for(const c of clients){assert.equal(c.streamErrors.length,0,JSON.stringify(c.streamErrors));assert.ok(now-c.lastFrameAt<3000,'local render stream stalled');}}
    if(snapshots[0].winner){
     const redFinal=await until(async()=>{const s=await call(red,'status');return s.snapshot?.winner===snapshots[0].winner?s:null;},'both native clients same terminal result');red.totals=redFinal.snapshot.players.find(p=>p.owner===2).totals||{};
     coverage.add('consistent-outcome');
     if(!epochEndAccounted){for(let i=0;i<2;i++){allCombat[i]+=clients[i].totals.fire||0;allOre[i]+=clients[i].totals['ore-delivered']||0;}epochEndAccounted=true;record('native match finished',{winner:snapshots[0].winner,reason:snapshots[0].winReason,tick:snapshots[0].tick,observedActivitySeconds:activity.seconds});await saveCombat(blue,'Completed real LAN epoch '+epochs);}
     if(activity.seconds<duration){if(render)await Promise.all(clients.map(closeFrames));await call(red,'leave',{});await call(blue,'leave',{});await proxy.close();activity.reset();proxy=await begin(blue,red,811+epochs);if(render)await Promise.all(clients.map(frames));lastSample=performance.now();lastFrames=[blue.frames,red.frames];epochEndAccounted=false;lastTicks=null;lastDropEnd=Date.now()+3000;}
    }
   }else activity.reset();
   nextSample=(performance.now()-wallStarted)/1000+10;
  }
  if(render&&wallSeconds>=nextCapture){for(const c of clients)if(c.lastFrame)fs.writeFileSync(path.join(out,`${c.name}-${Math.floor(wallSeconds)}s.png`),png(c.lastFrame));nextCapture=wallSeconds+300;}
  if(wallSeconds>=(short?60:120)&&!cameraChecked){cameraChecked=true;await call(blue,'camera',{centerX:10,centerY:48,layer:1,cutaway:true});await call(red,'camera',{centerX:114,centerY:48,layer:0,cutaway:true});await sleep(1000);if(render){for(const c of clients)fs.writeFileSync(path.join(out,c.name+'-independent-layer.png'),png(c.lastFrame));assert.ok(visualColors(red.lastFrame)>24,'changing host layer affected guest ground renderer');}record('separate native camera/layer requests preserve guest ground view',{blueLayer:1,redLayer:0,blueEngine:blue.health.enginePid,redEngine:red.health.enginePid});coverage.add('independent-camera-layers');await call(blue,'camera',{centerX:10,centerY:48,layer:0,cutaway:true});}
  await sleep(150);
 }
 const ending=await Promise.all([call(blue,'status'),call(red,'status')]);
 const measurementWallSeconds=(performance.now()-wallStarted)/1000;
 await collectPostActivityMetrics('activity-target-reached');
 assert.ok(activity.seconds>=duration);assert.ok(await alignReplica(red,proxy),'no same-tick native owner2 alignment observed');
 fs.writeFileSync(path.join(out,'ending-owner-snapshots.json'),JSON.stringify(ending));for(let i=0;i<2;i++)verifyOwnerView(ending[i].snapshot,i+1);
 if(!epochEndAccounted)for(let i=0;i<2;i++){const totals=ending[i].snapshot.players.find(p=>p.owner===i+1).totals||{};allCombat[i]+=totals.fire||0;allOre[i]+=totals['ore-delivered']||0;}
 if(short){for(let i=0;i<2;i++){assert.ok(clients[i].accepted>=8,'short test needs real accepted construction/action commands');assert.ok(clients[i].completedRooms>=2,'both clients must have completed real functional rooms');assert.ok(clients[i].moved.size>0,'both clients must operate an actually moving unit');assert.ok(allOre[i]>0,'both clients must receive a real resource shipment');}assert.ok(recoveredDrop,'short drop recovery was not verified');if(argv.includes('--exercise-epoch-end'))assert.ok(epochs>=2&&terminalExercised,'short probe must resume activity in a real new epoch after normal guest leave');record('paid short integration completed',{accepted:clients.map(c=>c.accepted),ore:allOre,shots:allCombat,movedUnits:clients.map(c=>c.moved.size),activitySeconds:activity.seconds,measurementWallSeconds});}
 else{assert.ok(allCombat.every(n=>n>50),'both sides must actually fight, not leave two idle games running');assert.ok(allOre.every(n=>n>500),'both sides must run real delivered-resource economies');}
 const saved=await saveCombat(blue,short?'Short real dual-native LAN integration':'45-minute real LAN combat soak');
 const measured=samples.filter(s=>s.seconds>30&&s.activityIntervalMs>0&&s.stable&&s.fps);const minFps=render?clients.map((_,i)=>Math.min(...measured.map(s=>s.fps[i]))):null;
 if(render){if(!short)assert.ok(minFps.every(n=>n>=30),'30FPS per local renderer threshold is not met');assert.ok(clients.every(c=>/NVIDIA.*5060/i.test(c.statuses.at(-1)?.deviceName||'')),'expected actual RTX 5060 native GPU renderer');await Promise.all(clients.map(closeFrames));}
 // Resume an actual native save through a new two-player ready room.
 await call(red,'leave',{});await call(blue,'leave',{});await proxy.close();
 let resumeFile=saved;
 if(ending[0].snapshot.winner){proxy=await begin(blue,red,9900);resumeFile=await call(blue,'save',{name:'native PVP resume boundary'});await call(red,'leave',{});await call(blue,'leave',{});await proxy.close();}
 const stored=JSON.parse(fs.readFileSync(path.join(blue.root,'.forge/save/v6',resumeFile.id+'.json'),'utf8'));
 const resumedLobby=await call(blue,'load',{id:resumeFile.id});assert.equal(resumedLobby.session.status,'lobby');assert.notEqual(resumedLobby.session.roomId,blue.sessionId);
 proxy=await joinHostLobby(blue,red,resumedLobby);assert.equal(blue.nextSequence,stored.save.sequences[0]+1);assert.equal(red.nextSequence,stored.save.sequences[1]+1);
 const restored=await call(blue,'status');assert.ok(restored.snapshot.tick>=stored.save.snapshot.tick);record('native PVP save resumed after both clients ready',{sourceTick:stored.save.snapshot.tick,resumedTick:restored.snapshot.tick,sequences:stored.save.sequences});
 coverage.add('save-load');
 // A separate peaceful test epoch prevents a legitimate battlefield victory from
 // racing the disconnect timeout and being misreported as a timeout result.
 if(!short){await call(red,'leave',{});await call(blue,'leave',{});await proxy.close();proxy=await begin(blue,red,9901);
 proxy.outage(65000);await sleep(60000);await sleep(6000);const finalBlue=await call(blue,'status');assert.equal(finalBlue.snapshot.winner,1);assert.equal(finalBlue.snapshot.winReason,'disconnect-timeout');await until(async()=>{const s=await call(red,'status');return s.snapshot?.winner===1;},'timed-out guest receives read-only final loss',20000);record('real sixty-second native disconnect forfeit',{winner:1,reason:finalBlue.snapshot.winReason});coverage.add('timeout-forfeit');coverage.add('consistent-outcome');}
 // Replay the real combat record with native pause/seek/playback controls.
 await call(red,'leave',{});await call(blue,'leave',{});await proxy.close();
 const replayEpochs=[];
 for(const entry of combatSaves){const result=await blue.native.rpc('game.session.replay',{save:entry.save},1800000);assert.equal(result.matched,true,'native full-state replay mismatch in combat epoch '+entry.epoch);const receipt={epoch:entry.epoch,id:entry.id,tick:entry.tick,matched:result.matched,winner:entry.save.snapshot.winner,sourceEvidence:entry.evidence};replayEpochs.push(receipt);record('native full-state epoch replay verified',receipt);}
 const playback=await call(blue,'replay',{id:saved.id});assert.equal(playback.session.replay,true);
 await call(blue,'replay-control',{paused:true});const pausedA=await call(blue,'status');await sleep(500);const pausedB=await call(blue,'status');assert.equal(pausedA.snapshot.tick,pausedB.snapshot.tick,'native replay pause did not freeze its tick');
 await call(blue,'replay-control',{seekTick:saved.tick,speed:8});
 const replayStarted=performance.now(),replayTimeout=Math.max(180000,Math.min(1800000,Math.ceil(saved.tick/60*1000)));
 const replayEnd=await until(async()=>{const s=await call(blue,'status');return s.snapshot?.tick===saved.tick?s:null;},'native replay reaches saved tick',replayTimeout);
 const combatSave=JSON.parse(fs.readFileSync(path.join(blue.root,'.forge/save/v6',saved.id+'.json'),'utf8')).save;
 const originalPlayer=combatSave.snapshot.players.find(p=>p.owner===1),replayedPlayer=replayEnd.snapshot.players.find(p=>p.owner===1);
 assert.deepEqual(replayedPlayer.totals,originalPlayer.totals);assert.equal(replayedPlayer.credits,originalPlayer.credits);assert.equal(replayEnd.snapshot.winner,combatSave.snapshot.winner);
 const replayFull=await blue.native.rpc('game.session.snapshot');replayFull.playback=null;const savedFull=structuredClone(combatSave.snapshot);savedFull.playback=null;assert.equal(stateHash(replayFull),stateHash(savedFull),'actual playback full simulation state differs from save');
 fs.writeFileSync(path.join(out,'replayed-native-full-state.json'),JSON.stringify(replayFull));replayExact=true;coverage.add('exact-replay');
 record('actual native combat replay reached saved state',{tick:saved.tick,ownerOneCredits:replayedPlayer.credits,nativeTotalsMatched:true,replayWallSeconds:(performance.now()-replayStarted)/1000});
 if(render){await frames(blue);fs.writeFileSync(path.join(out,'blue-replay-end.png'),png(blue.lastFrame));}
 if(proxies.reduce((n,p)=>n+p.delays(),0)>0)coverage.add('delayed-transport');
 const requiredCoverage=['ownership','deduplication','delayed-transport','disconnect-recovery','timeout-forfeit','independent-camera-layers','save-load','exact-replay','consistent-outcome'];if(!short)for(const key of requiredCoverage)assert.ok(coverage.has(key),'missing actual coverage: '+key);
 assert.equal(failure,undefined);
 const finalEligible=!short&&duration>=2700&&activity.seconds>=2700&&render&&marker.mediaReady&&!project;
 const report={schemaVersion:2,testedAt:new Date().toISOString(),passed:true,finalEligible,short,sourceProject:project,engineFile,driverSha256,viewContractSha256,observationsSha256,scope:finalEligible?'Real-time dual-native LAN combat with local Rurix frame streams':'Short actual dual-native HTTP/SSE integration and local Rurix; not 45-minute, cross-machine, balance or performance acceptance',...blue.identity,requestedSeconds:duration,observedCombatWallSeconds:activity.seconds,measurementWallSeconds,testWallSeconds:(performance.now()-wallStarted)/1000,activityDefinition:'Only full same-epoch intervals bounded by two nonterminal, advancing owner snapshots outside intentional disconnection/recovery. Terminal-edge intervals and lobby/transition/idle time are excluded. Order and sample scheduling use independent monotonic wall time.',epochs,allCombat,allOre,minFps,coverage:[...coverage],consistency,errors:[],replayExact,replayEpochs,clients:clients.map(c=>({name:c.name,...c.identity,enginePid:c.health.enginePid,bridgePid:c.child.pid,rgbaLocal:render&&c.frames>0,accepted:c.accepted,rejected:c.rejected,retries:c.retries,frames:c.frames,movedUnits:c.moved.size,completedRooms:c.completedRooms,device:c.statuses.at(-1)?.deviceName})),checks,scratchDirectory:scratch};
 fs.writeFileSync(path.join(out,'lan-acceptance.json'),JSON.stringify(report,null,2));if(finalEligible)fs.writeFileSync(path.join(dir,'lan-acceptance.json'),JSON.stringify(report,null,2));record('soak completed',{finalEligible});
}catch(error){failure=error;await collectPostActivityMetrics('failure-or-stop');const failureStates=await Promise.all(clients.map(c=>c.base?call(c,'status').catch(()=>null):null));fs.writeFileSync(path.join(out,'failure-states.json'),JSON.stringify(failureStates));fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({testedAt:new Date().toISOString(),message:error.message,stack:error.stack,seconds,engineSha256:engineHash,checks,scratchDirectory:scratch},null,2));console.error(error);}
finally{
 for(const c of clients){await closeFrames(c);if(c.base)await call(c,'leave',{}).catch(()=>{});}
 for(const c of clients)c.native?.close();
 for(const p of proxies)await p.close().catch(()=>{});
 for(const c of clients){if(c.child.exitCode===null){c.child.stdin.end('shutdown\n');await Promise.race([new Promise(resolve=>c.child.once('exit',resolve)),sleep(10000)]);}}
}
if(failure)process.exitCode=1;
