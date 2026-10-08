/** Actual isolated Windows package test. Ordinary API commands only; no clock/resource injection.
 * Browser controls remain a separate CUA check. This does not replace the 45-minute LAN soak.
 */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import {spawn,execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {deflateSync} from 'node:zlib';
import {fileURLToPath} from 'node:url';
const here=path.dirname(fileURLToPath(import.meta.url));
const args=process.argv.slice(2),option=(name,fallback)=>{const i=args.indexOf(name);return i<0?fallback:args[i+1];};
const pack=path.resolve(option('--pack',path.join(here,'../../dist/CodeSentinels-V6-Windows')));
const out=path.resolve(option('--out',path.join(here,'cold-start-runs',new Date().toISOString().replace(/[:.]/g,'-'))));
const development=args.includes('--development');
assert.equal(process.platform,'win32','This verifies the delivered Windows package.');
assert.ok(!fs.existsSync(out),'Prior evidence is preserved.');fs.mkdirSync(out,{recursive:true});
const digest=data=>createHash('sha256').update(data).digest('hex');
const fileHash=file=>digest(fs.readFileSync(file));
function crc32(bytes){let crc=0xffffffff;for(const byte of bytes){crc^=byte;for(let i=0;i<8;i++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);}return(crc^0xffffffff)>>>0;}
function framePng(frame){const chunk=(tag,data)=>{const t=Buffer.from(tag),size=Buffer.alloc(4),crc=Buffer.alloc(4);size.writeUInt32BE(data.length);crc.writeUInt32BE(crc32(Buffer.concat([t,data])));return Buffer.concat([size,t,data,crc]);};const header=Buffer.alloc(13);header.writeUInt32BE(1280);header.writeUInt32BE(720,4);header[8]=8;header[9]=6;const pixels=Buffer.alloc(720*(1280*4+1));for(let y=0;y<720;y++)frame.copy(pixels,y*(1280*4+1)+1,20+y*1280*4,20+(y+1)*1280*4);return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(pixels)),chunk('IEND',Buffer.alloc(0))]);}
const marker=JSON.parse(fs.readFileSync(path.join(pack,'v6-candidate.json'),'utf8'));
const target=marker.target;assert.equal(marker.version,6);assert.ok(target?.payloadSha256&&target?.engineSha256&&target?.rulesFingerprint&&target?.rulesVersion,'Assemble a schema2 candidate with a real native identity first.');
const runtimeFile=relative=>/^(bin|Content|Web|v6)\//.test(relative)||['bridge.mjs','multiplayer-v6.mjs','forge.toml'].includes(relative)||/^Start-Game[^/]*\.cmd$/.test(relative);
const contained=(parent,child)=>{const relative=path.relative(parent,child);return relative!==''&&!relative.startsWith('..'+path.sep)&&relative!=='..'&&!path.isAbsolute(relative);};
function payload(root){const entries=marker.included.filter(runtimeFile).sort().map(relative=>{const file=path.resolve(root,relative);assert.ok(contained(root,file));assert.ok(fs.statSync(file).isFile());return{bytes:fs.statSync(file).size,path:relative,sha256:fileHash(file)};});return{entries,sha256:digest(JSON.stringify(entries))};}
assert.equal(payload(pack).sha256,target.payloadSha256,'Source runtime payload differs from the sealed candidate.');
assert.equal(fileHash(path.join(pack,'bin/engine-host.exe')),target.engineSha256);
const isolated=fs.mkdtempSync(path.join(os.tmpdir(),'编译防线 V6 冷启动 '));
assert.ok(!contained(pack,isolated)&&!contained(path.resolve(here,'../..'),isolated));
const clients=[],checks=[],processes=[],completedFlows=[];let failure=null,report=null;
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const record=(name,data={})=>{checks.push({at:new Date().toISOString(),name,...data});fs.appendFileSync(path.join(out,'events.jsonl'),JSON.stringify(checks.at(-1))+'\n');console.log(name);};
async function call(client,route,input){const response=await fetch(client.base+'/api/v6/'+route,{method:input===undefined?'GET':'POST',headers:input===undefined?{}:{'content-type':'application/json'},body:input===undefined?undefined:JSON.stringify(input),signal:AbortSignal.timeout(30000)});const body=await response.json();assert.ok(response.ok,JSON.stringify(body));return body;}
async function until(check,label,timeout=45000){const end=Date.now()+timeout;while(Date.now()<end){const result=await check();if(result)return result;await sleep(200);}throw Error('Timeout: '+label);}
function copyClient(name){const root=path.join(isolated,name);fs.mkdirSync(root);for(const relative of marker.included){assert.ok(!/(^|\/)(\.forge|Logs|logs)(\/|$)/.test(relative),'Runtime data was packaged.');const source=path.resolve(pack,relative),destination=path.resolve(root,relative);assert.ok(contained(pack,source)&&contained(root,destination));assert.ok(contained(pack,fs.realpathSync(source)),'Package symlink escapes input.');fs.mkdirSync(path.dirname(destination),{recursive:true});fs.copyFileSync(source,destination);}fs.copyFileSync(path.join(pack,'v6-candidate.json'),path.join(root,'v6-candidate.json'));assert.equal(payload(root).sha256,target.payloadSha256);assert.ok(!fs.existsSync(path.join(root,'.forge'))&&!fs.existsSync(path.join(root,'Logs')));return root;}
async function start(name,root){const executable=path.join(root,'bin/node.exe');const env={...process.env,NODE_PATH:'',PATH:[path.join(root,'bin'),path.join(process.env.SystemRoot,'System32'),process.env.SystemRoot].join(path.delimiter)};const child=spawn(executable,[path.join(root,'bridge.mjs'),'--no-open','--control-stdin'],{cwd:root,windowsHide:true,stdio:['pipe','pipe','pipe'],env});const client={name,root,child,base:null,frames:0,renderErrors:[],lastStatus:null,ws:null,health:null};clients.push(client);const stream=fs.createWriteStream(path.join(out,name+'-process.log'));await new Promise((resolve,reject)=>{let stdout='';const timer=setTimeout(()=>reject(Error('Package startup timed out')),60000);child.once('error',error=>{clearTimeout(timer);reject(error);});child.once('exit',code=>{clearTimeout(timer);reject(Error('Package exited '+code));});child.stderr.on('data',chunk=>stream.write(chunk));child.stdout.on('data',chunk=>{stream.write(chunk);stdout+=chunk.toString();const ready=/Code Sentinels V6 is ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(stdout);if(ready){client.base=ready[1];clearTimeout(timer);resolve();}});});client.health=await(await fetch(client.base+'/health')).json();assert.equal(path.resolve(client.health.root),root);assert.equal(client.health.compilerDisabled,true);return client;}
const powershellUtf8='[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); ';
function processPaths(){const ids=clients.flatMap(c=>[c.child.pid,c.health.enginePid]);assert.ok(ids.every(id=>Number.isSafeInteger(id)&&id>0));const rows=JSON.parse(execFileSync('powershell.exe',['-NoProfile','-Command',powershellUtf8+`Get-Process -Id ${ids.join(',')} | Select-Object Id,Path | ConvertTo-Json -Compress`],{encoding:'utf8',windowsHide:true}));fs.writeFileSync(path.join(out,'actual-process-paths.json'),JSON.stringify(rows,null,2));for(const row of rows){assert.ok(contained(isolated,path.resolve(row.Path)));const basename=path.basename(row.Path).toLowerCase();assert.ok(['node.exe','engine-host.exe'].includes(basename));processes.push({pid:row.Id,executable:row.Path});}assert.equal(new Set(processes.map(p=>p.pid)).size,4);}
function remainingOwned(){const literal="'"+(isolated+path.sep).replaceAll("'","''")+"'";return JSON.parse(execFileSync('powershell.exe',['-NoProfile','-Command',powershellUtf8+`ConvertTo-Json -Compress -InputObject @(Get-Process -Name node,engine-host -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith(${literal},[StringComparison]::OrdinalIgnoreCase) } | Select-Object Id,Path)`],{encoding:'utf8',windowsHide:true}));}
function nativeIdentity(catalog){return{rulesVersion:catalog.rulesVersion,rulesFingerprint:catalog.rulesFingerprint};}
async function frames(client){const info=await call(client,'viewport');assert.ok(['127.0.0.1','localhost','[::1]'].includes(new URL(info.wsUrl).hostname),'Native frames must come from the local bundled process.');const ws=new WebSocket(info.wsUrl);client.ws=ws;ws.binaryType='arraybuffer';await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('No native RGBA frame')),45000);ws.addEventListener('open',()=>ws.send(JSON.stringify({type:'subscribe',width:1280,height:720,maxFps:10})));ws.addEventListener('error',()=>{if(client.ws!==ws)return;client.renderErrors.push('socket-error');clearTimeout(timer);reject(Error('Native render socket failed'));});ws.addEventListener('message',event=>{if(client.ws!==ws)return;try{if(typeof event.data==='string'){const status=JSON.parse(event.data);if(status.type==='error'||status.shareError)throw Error(JSON.stringify(status));if(status.type==='status'){assert.equal(status.truncated,false);if(status.meshFallbacks!==undefined)assert.equal(status.meshFallbacks,0);client.lastStatus={...status,meshFallbacks:status.meshFallbacks??null,meshFallbacksObserved:status.meshFallbacks!==undefined};}return;}const frame=Buffer.from(event.data);assert.equal(frame.length,20+1280*720*4);assert.equal(frame.readUInt32LE(),0x31464746);assert.equal(frame.readUInt16LE(8),1280);assert.equal(frame.readUInt16LE(10),720);assert.equal(frame.readUInt32LE(12)&2,0);const colors=new Set();for(let p=20;p<frame.length;p+=4*997)colors.add(((frame[p]>>3)<<10)|((frame[p+1]>>3)<<5)|(frame[p+2]>>3));if(colors.size<=24)return;client.frames++;client.frameSha256=digest(frame);if(client.frames===1)fs.writeFileSync(path.join(out,client.name+'-native-frame.rgba'),frame);clearTimeout(timer);resolve();}catch(error){client.renderErrors.push(error.message);clearTimeout(timer);reject(error);}});});}
async function closeFrames(client){if(client.ws){const ws=client.ws;client.ws=null;ws.close();await Promise.race([new Promise(resolve=>ws.addEventListener('close',resolve,{once:true})),sleep(1000)]);}}
async function order(client,command){const state=await call(client,'status');const request={sessionId:state.session.roomId,seq:(state.session.lastSequence||0)+1,command};const receipt=await call(client,'order',request);assert.equal(receipt.accepted,true,JSON.stringify(receipt));record('ordinary paid command',{client:client.name,command,receipt});return receipt;}
async function availablePort(){const server=net.createServer();await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const port=server.address().port;await new Promise(resolve=>server.close(resolve));return port;}
try{
 const roots=['甲方 原生客户端','乙方 原生客户端'].map(copyClient);
 const initialRuntimeFiles={saves:0,tokens:0,logs:0};record('clean isolated payload copies',{isolated,payloadSha256:target.payloadSha256,initialRuntimeFiles});
 const blue=await start('blue',roots[0]),red=await start('red',roots[1]);processPaths();
 for(const client of clients){const identity=nativeIdentity(await call(client,'catalog'));assert.deepEqual(identity,{rulesVersion:target.rulesVersion,rulesFingerprint:target.rulesFingerprint});}
 await call(blue,'session',{mode:'solo',seed:6026,theme:'river',nickname:'冷启动验收'});await frames(blue);completedFlows.push('solo');
 await order(blue,{op:'shell',rect:{x:13,y:46,z:0,w:6,h:4}});
 const constructed=await until(async()=>{const state=await call(blue,'status');return state.snapshot?.buildings.find(b=>b.owner===1&&b.kind==='shell'&&b.progress>=1);},'ordinary shell construction');
 const saved=await call(blue,'save',{name:'隔离包真实存档'});const original=JSON.parse(fs.readFileSync(path.join(blue.root,'.forge/save/v6',saved.id+'.json'),'utf8'));
 assert.equal(original.save.rulesFingerprint,target.rulesFingerprint);assert.equal(original.rulesFingerprint,target.rulesFingerprint);completedFlows.push('save');
 await closeFrames(blue);await call(blue,'leave',{});await call(blue,'load',{id:saved.id});const loaded=await call(blue,'status');assert.ok(loaded.snapshot.buildings.some(b=>b.id===constructed.id&&b.progress>=1));completedFlows.push('load');
 await call(blue,'leave',{});await call(blue,'replay',{id:saved.id});await call(blue,'replay-control',{paused:true,seekTick:saved.tick,speed:8});
 const replay=await until(async()=>{const state=await call(blue,'status');return state.snapshot?.tick===saved.tick?state:null;},'exact saved-tick replay');
 const player=replay.snapshot.players.find(p=>p.owner===1),storedPlayer=original.save.snapshot.players.find(p=>p.owner===1);assert.equal(player.credits,storedPlayer.credits);assert.deepEqual(player.totals,storedPlayer.totals);assert.equal(replay.snapshot.winner,original.save.snapshot.winner);completedFlows.push('replay');
 record('real save/load/replay complete',{savedId:saved.id,tick:saved.tick,credits:player.credits});
 await call(blue,'leave',{});const port=await availablePort();const host=await call(blue,'session',{mode:'host',port,seed:6033,theme:'river',nickname:'隔离甲方'});completedFlows.push('create');
 const guest=await call(red,'session',{mode:'join',address:'127.0.0.1:'+port,code:host.session.code,nickname:'隔离乙方'});assert.equal(guest.playerId??guest.session.playerId,2);completedFlows.push('join');
 await Promise.all(clients.map(client=>call(client,'ready',{ready:true})));await call(blue,'start',{});await until(async()=>{const state=await call(red,'status');return state.snapshot?.tick>0;},'actual guest native replica');
 await Promise.all(clients.map(frames));
 await order(blue,{op:'shell',rect:{x:13,y:46,z:0,w:6,h:4}});await order(red,{op:'shell',rect:{x:109,y:46,z:0,w:6,h:4}});
 await until(async()=>{const states=await Promise.all(clients.map(client=>call(client,'status')));return states.every((state,i)=>state.snapshot.buildings.some(b=>b.owner===i+1&&b.kind==='shell'&&b.progress>=1));},'two independent paid constructors');
 assert.ok(clients.every(client=>client.frames>0&&client.renderErrors.length===0));
 const ending=await Promise.all(clients.map(client=>call(client,'status')));fs.writeFileSync(path.join(out,'ending-owner-views.json'),JSON.stringify(ending));
 record('two bundled native clients built and rendered independently',{processes,frames:clients.map(c=>c.frames)});
 const nativeFrameEvidence=clients.map(client=>{const file=path.join(out,client.name+'-native-frame.png');fs.writeFileSync(file,framePng(fs.readFileSync(path.join(out,client.name+'-native-frame.rgba'))));return{path:file,sha256:fileHash(file)};});
 report={schemaVersion:2,kind:'isolated-cold-start',testedAt:new Date().toISOString(),...target,
  scope:'Two normally launched bundled Windows clients in a new Unicode/space-containing temporary directory. Ordinary paid construction, real clocks, native RGBA and exact saved-tick replay. Short functional cold start, not balance or45-minute LAN.',
  actualNativeExecution:true,isolatedDirectory:isolated,initialRuntimeFiles,usedBundledRuntime:true,requiresCodex:false,externalCodeServicesUsed:[],completedFlows,
  nativeRgbaFrames:clients.reduce((total,c)=>total+c.frames,0),nativeFrameEvidence,processes,clients:clients.map(c=>({name:c.name,frames:c.frames,frameSha256:c.frameSha256,nativeStatus:c.lastStatus})),
  errors:[],finalEligible:!development,developmentCandidate:development,passed:true,driverSha256:fileHash(fileURLToPath(import.meta.url)),checks};
}catch(error){failure=error;console.error(error);}
finally{for(const client of clients){await closeFrames(client);if(client.base)await call(client,'leave',{}).catch(()=>{});if(client.child.exitCode===null){client.child.stdin.end('shutdown\n');await Promise.race([new Promise(resolve=>client.child.once('exit',resolve)),sleep(10000)]);if(client.child.exitCode===null)client.child.kill();}}}
const remaining=remainingOwned();
if(remaining.length){failure??=Error('Normal package shutdown left owned processes running.');record('forced cleanup of verified isolated processes',{processes:remaining});const ids=remaining.map(p=>p.Id);assert.ok(ids.every(id=>Number.isSafeInteger(id)&&id>0));execFileSync('powershell.exe',['-NoProfile','-Command',`Stop-Process -Id ${ids.join(',')} -ErrorAction SilentlyContinue`],{windowsHide:true});}
if(failure){fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({at:new Date().toISOString(),message:failure.message,stack:failure.stack,isolated,target,checks},null,2));process.exitCode=1;}
else{record('normal isolated shutdown completed');report.normalShutdown=true;report.checks=checks;fs.writeFileSync(path.join(out,'cold-start-acceptance.json'),JSON.stringify(report,null,2));console.log('isolated package test passed');}
