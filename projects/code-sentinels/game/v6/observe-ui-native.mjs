/** Read-only native evidence alongside a separately operated CUA browser.
 * This does not perform or claim any browser actions, mutate gameplay, or measure pressure FPS.
 */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
const args=process.argv.slice(2), arg=name=>args[args.indexOf(name)+1];
for(const key of ['--base','--pack','--out'])assert.ok(args.includes(key),'Missing '+key);
const base=arg('--base'), pack=path.resolve(arg('--pack')),out=path.resolve(arg('--out'));
assert.ok(['127.0.0.1','localhost','[::1]'].includes(new URL(base).hostname));
assert.ok(!fs.existsSync(out),'Prior evidence must be preserved.');
const hash=data=>createHash('sha256').update(data).digest('hex');
const marker=JSON.parse(fs.readFileSync(path.join(pack,'v6-candidate.json'),'utf8'));
const target=marker.target;assert.ok(target?.rulesFingerprint&&target?.payloadSha256);
assert.equal(hash(fs.readFileSync(path.join(pack,'bin/engine-host.exe'))),target.engineSha256);
const get=async route=>{const response=await fetch(base+route,{signal:AbortSignal.timeout(30000)});assert.ok(response.ok);return response.json();};
const [health,catalog,initial,viewport]=await Promise.all(['/health','/api/v6/catalog','/api/v6/status','/api/v6/viewport'].map(get));
const nativeCatalog=catalog.catalog??catalog;
assert.equal(nativeCatalog.rulesVersion,target.rulesVersion);
assert.equal(nativeCatalog.rulesFingerprint,target.rulesFingerprint);
assert.equal(path.resolve(health.root),pack);
assert.ok(health.enginePid>0&&initial.snapshot?.tick>=0);
assert.ok(['127.0.0.1','localhost','[::1]'].includes(new URL(viewport.wsUrl).hostname));
let frames=0, firstHash=null, lastHash=null, firstFrameAt=null, lastFrameAt=null;
const errors=[],statuses=[];
const ws=new WebSocket(viewport.wsUrl);ws.binaryType='arraybuffer';
const started=new Date().toISOString();
await new Promise(resolve=>{
 const timer=setTimeout(resolve,12000);
 ws.addEventListener('open',()=>ws.send(JSON.stringify({type:'subscribe',width:1280,height:720,maxFps:10})));
 ws.addEventListener('error',()=>{errors.push('native-websocket-error');clearTimeout(timer);resolve();});
 ws.addEventListener('message',event=>{
  try{
   if(typeof event.data==='string'){
    const data=JSON.parse(event.data);
    assert.ok(data.type!=='error'&&!data.shareError,JSON.stringify(data));
    if(data.type==='status'){
     assert.equal(data.truncated,false);
     if(data.meshFallbacks!==undefined)assert.equal(data.meshFallbacks,0);
     statuses.push({deviceName:data.deviceName,truncated:data.truncated,meshFallbacks:data.meshFallbacks??null,meshFallbacksObserved:data.meshFallbacks!==undefined});
    }
    return;
   }
   const frame=Buffer.from(event.data);
   assert.equal(frame.length,20+1280*720*4);assert.equal(frame.readUInt32LE(),0x31464746);
   assert.equal(frame.readUInt16LE(8),1280);assert.equal(frame.readUInt16LE(10),720);assert.equal(frame.readUInt32LE(12)&2,0);
   const colors=new Set();for(let p=20;p<frame.length;p+=4*997)colors.add(((frame[p]>>3)<<10)|((frame[p+1]>>3)<<5)|(frame[p+2]>>3));
   assert.ok(colors.size>24,'Native viewport is blank or lacks visible scene variation.');
   frames++;lastFrameAt=new Date().toISOString();lastHash=hash(frame);
   if(firstHash===null){firstHash=lastHash;firstFrameAt=lastFrameAt;}
   if(frames>=30&&statuses.length>0){clearTimeout(timer);resolve();}
  }catch(error){errors.push(error.message);clearTimeout(timer);resolve();}
 });
});
ws.close();
const ending=await get('/api/v6/status');
assert.equal(ending.session.roomId,initial.session.roomId,'Session changed during observation.');
const report={schemaVersion:2,kind:'native-ui-stream-observation',...target,started,completedAt:new Date().toISOString(),
 scope:'Native local RGBA observation after separate browser actions. No game commands, resource injection, browser automation claims or pressure-performance result. Stream status may omit meshFallbacks; missing telemetry is null, not a measured zero.',
 actualNativeExecution:true,nativePid:health.enginePid,bridgePid:health.bridgePid,rgbaFrames:frames,errors,
 passed:frames>=30&&statuses.length>0&&errors.length===0,firstFrameAt,lastFrameAt,firstFrameSha256:firstHash,lastFrameSha256:lastHash,
 statusObservations:statuses,initialTick:initial.snapshot.tick,finalTick:ending.snapshot.tick,replay:ending.session.replay===true};
fs.mkdirSync(path.dirname(out),{recursive:true});fs.writeFileSync(out,JSON.stringify(report,null,2));
console.log(JSON.stringify({output:out,passed:report.passed,frames,errors}));
if(!report.passed)process.exitCode=1;
