/** Isolated V5 cold load, one real GPU frame, and app-local VC module paths. */
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { deflateSync } from 'node:zlib';

const out=path.dirname(fileURLToPath(import.meta.url));
const root=path.resolve(out,'../..');
const pack=path.join(root,'dist/CodeSentinels-V5-Windows');
const stamp=new Date().toISOString().replace(/[:.]/g,'-');
const evidence=path.join(out,'runtime-dependencies-runs',stamp);
const sha=bytes=>createHash('sha256').update(bytes).digest('hex');
const inside=(base,file)=>{const relative=path.relative(base,file);return relative!==''&&!relative.startsWith('..'+path.sep)&&relative!=='..'&&!path.isAbsolute(relative);};
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const vcPattern=/^(?:concrt\d+|msvcp\d+(?:_.*)?|vccorlib\d+|vcruntime\d+(?:_.*)?)\.dll$/i;
let scratch,runtime,bridge,base,enginePid,stdout='',stderr='',failed;
const record={schema:'code-sentinels-v5/runtime-dependencies/1',startedAt:new Date().toISOString(),pack,evidence,checks:[],gpuFramesRequested:0};
fs.mkdirSync(evidence,{recursive:true});

function check(name,data={}){record.checks.push({case:name,passed:true,...data});console.log(JSON.stringify(record.checks.at(-1)));}
async function call(name,args={}){
  const response=await fetch(base+'/api/forge/mcp/call',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({tool:'mcp__engine-scene__'+name,arguments:args,workspaceId:'portable'}),signal:AbortSignal.timeout(60000)});
  const envelope=await response.json();assert.equal(response.status,200,JSON.stringify(envelope));return JSON.parse(envelope.content[0].text);
}
async function modules(pids){
  assert.ok(pids.every(pid=>Number.isInteger(pid)&&pid>0&&pid!==33480));
  const script=`$ErrorActionPreference='Stop'; $result=@(); foreach($targetPid in @(${pids.join(',')})){ $runtimeProcess=Get-Process -Id $targetPid; $loaded=@($runtimeProcess.Modules | ForEach-Object { @{name=$_.ModuleName; path=$_.FileName} }); $result+=@{pid=$targetPid; executable=$runtimeProcess.Path; modules=$loaded} }; $result | ConvertTo-Json -Depth 5 -Compress`;
  const child=spawn('powershell.exe',['-NoProfile','-NonInteractive','-Command',script],{windowsHide:true,stdio:['ignore','pipe','pipe']});
  let text='',error='';child.stdout.on('data',b=>text+=b);child.stderr.on('data',b=>error+=b);
  const code=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('exit',resolve);});
  assert.equal(code,0,error);return JSON.parse(text.replace(/^\uFEFF/,''));
}
function crc32(data){let c=0xffffffff;for(const b of data){c^=b;for(let i=0;i<8;i++)c=(c>>>1)^((c&1)?0xedb88320:0);}return(c^0xffffffff)>>>0;}
function encodePng(w,h,pixels){
  const chunk=(name,data)=>{const type=Buffer.from(name),size=Buffer.alloc(4),crc=Buffer.alloc(4);size.writeUInt32BE(data.length);crc.writeUInt32BE(crc32(Buffer.concat([type,data])));return Buffer.concat([size,type,data,crc]);};
  const header=Buffer.alloc(13);header.writeUInt32BE(w);header.writeUInt32BE(h,4);header[8]=8;header[9]=6;
  const scan=Buffer.alloc(h*(1+w*4));for(let y=0;y<h;y++)pixels.copy(scan,y*(1+w*4)+1,y*w*4,(y+1)*w*4);
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(scan)),chunk('IEND',Buffer.alloc(0))]);
}

try{
  const available=fs.readdirSync(path.join(pack,'bin')).filter(name=>vcPattern.test(name));assert.equal(available.length,10);
  record.packagedVcFiles=available.map(name=>({name,bytes:fs.statSync(path.join(pack,'bin',name)).size}));
  scratch=fs.mkdtempSync(path.join(os.tmpdir(),'sentinels-v5-runtime-deps-'));runtime=path.join(scratch,'runtime');assert.ok(inside(scratch,runtime));fs.mkdirSync(runtime);
  for(const name of ['bin','Content','Web','.forge/cache','bridge.mjs','multiplayer-v4.mjs','forge.toml']){
    const target=path.join(runtime,name);assert.ok(inside(runtime,target));fs.mkdirSync(path.dirname(target),{recursive:true});fs.cpSync(path.join(pack,name),target,{recursive:true,dereference:true,force:false,errorOnExist:true});
  }
  record.isolatedRuntime=runtime;record.originalCandidateUnmodified=true;
  bridge=spawn(path.join(runtime,'bin/node.exe'),[path.join(runtime,'bridge.mjs'),'--no-open'],{cwd:runtime,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,FORGE_GPU_PARTICLES:'off'}});
  assert.notEqual(bridge.pid,33480);
  base=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('cold bridge startup timeout')),30000);bridge.stdout.on('data',b=>{stdout+=b;const match=/ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(stdout);if(match){clearTimeout(timer);resolve(match[1]);}});bridge.stderr.on('data',b=>stderr+=b);bridge.once('error',reject);bridge.once('exit',code=>{clearTimeout(timer);reject(Error(`bridge exited ${code}`));});});
  const health=await fetch(base+'/health').then(r=>r.json());enginePid=health.enginePid;assert.notEqual(enginePid,33480);assert.equal(path.resolve(health.root),runtime);assert.equal(health.compilerDisabled,true);assert.equal((await call('host_ping')).pid,enginePid);
  record.ownedPids={bridge:bridge.pid,engine:enginePid};check('candidate_owned_node_and_engine_cold_start',{health});
  const html=await fetch(base+'/?play=code-sentinels&workspace=portable&standalone=1&version=5').then(r=>r.text());
  record.clientBundle=/\/assets\/(index-[^"']+\.js)/.exec(html)?.[1];assert.ok(record.clientBundle);check('current_packaged_web_entry',{clientBundle:record.clientBundle});
  assert.equal((await call('scene_summary')).playState,'edit');await call('scene_load',{path:'Content/Scenes/CommandV5.rxscene'});await call('play_enter');await call('play_pause');await call('play_step');
  const entities=(await call('entity_list')).entities;const fields=name=>{const e=entities.find(e=>e.name===name);assert.ok(e);return [...e.transform.translation,...e.transform.scale];};
  const state=fields('C4_State'),global=fields('C5_Global');assert.deepEqual(state,[2000,0,400,0,0,1]);assert.deepEqual(global.slice(4),[5,0]);check('real_v5_native_scene_and_dll_initial_state',{state,global});
  record.gpuFramesRequested++;const frame=await call('viewport_frame',{width:1280,height:720,format:'rgba8'});assert.equal(frame.width,1280);assert.equal(frame.height,720);assert.equal(frame.truncated,false);assert.equal(frame.meshFallbacks,0);assert.ok(frame.deviceName.includes('NVIDIA'));assert.ok(frame.draws>0&&frame.nonZeroPixels>0);
  const pixels=Buffer.from(frame.pixelsB64,'base64');assert.equal(pixels.length,1280*720*4);fs.writeFileSync(path.join(evidence,'cold-native-frame.png'),encodePng(1280,720,pixels));const {pixelsB64,...metadata}=frame;
  check('one_real_gpu_frame',{...metadata,rgbaSha256:sha(pixels),image:path.join(evidence,'cold-native-frame.png')});
  const loaded=await modules([bridge.pid,enginePid]);fs.writeFileSync(path.join(evidence,'owned-process-modules.json'),JSON.stringify(loaded,null,2));
  const appBin=path.join(runtime,'bin').toLowerCase();
  for(const proc of loaded){
    assert.ok(inside(runtime,path.resolve(proc.executable)));const vc=proc.modules.filter(m=>vcPattern.test(m.name));
    for(const module of vc)assert.equal(path.dirname(path.resolve(module.path)).toLowerCase(),appBin,`${module.name} loaded outside packaged bin`);
    proc.loadedVc=vc;
  }
  const engine=loaded.find(p=>p.pid===enginePid),node=loaded.find(p=>p.pid===bridge.pid);assert.ok(engine.loadedVc.length>0);
  const native=engine.modules.find(m=>/^sentinels_v5-[a-f0-9]+\.dll$/i.test(m.name));assert.ok(native,'actual V5 native DLL was not loaded');assert.ok(inside(path.join(runtime,'.forge/cache/rxdll'),path.resolve(native.path)));
  check('vc_modules_resolve_from_packaged_bin',{bridge:{pid:node.pid,executable:node.executable,loadedVc:node.loadedVc},engine:{pid:engine.pid,executable:engine.executable,loadedVc:engine.loadedVc},nativeModule:native,systemUcrt:engine.modules.filter(m=>/^ucrtbase\.dll$/i.test(m.name)),availableButNotRequired:available.filter(name=>!engine.loadedVc.some(m=>m.name.toLowerCase()===name.toLowerCase()))});
  const events=await call('host_events_drain');const errors=events.filter(e=>/logic\.(call_error|unsupported)|anim\.warn|character\.error|DEV_ENV_DEGRADE/.test(e.event??e.name??''));assert.deepEqual(errors,[]);check('no_native_startup_errors',{errors});
  await call('play_exit');
}catch(error){failed={message:error.message,stack:error.stack};console.error(error.stack);}
finally{
  if(bridge?.pid&&bridge.exitCode===null){await new Promise(resolve=>{const killer=spawn('taskkill.exe',['/PID',String(bridge.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});killer.once('exit',resolve);killer.once('error',resolve);});}
  await sleep(250);
  if(enginePid){try{process.kill(enginePid,0);process.kill(enginePid);await sleep(250);}catch(error){if(error.code!=='ESRCH')failed??={message:'owned engine cleanup failed',detail:error.message};}}
  fs.writeFileSync(path.join(evidence,'bridge-stdout.log'),stdout);fs.writeFileSync(path.join(evidence,'bridge-stderr.log'),stderr);
  if(runtime&&fs.existsSync(path.join(runtime,'Logs')))fs.cpSync(path.join(runtime,'Logs'),path.join(evidence,'Logs'),{recursive:true});
  if(scratch){try{const resolved=fs.realpathSync(scratch),temp=fs.realpathSync(os.tmpdir());assert.ok(inside(temp,resolved)&&path.basename(resolved).startsWith('sentinels-v5-runtime-deps-'));fs.rmSync(resolved,{recursive:true,force:false,maxRetries:5,retryDelay:300});record.runtimeCleaned=true;}catch(error){failed??={message:'runtime copy cleanup failed',detail:error.message};}}
  Object.assign(record,{passed:!failed,finishedAt:new Date().toISOString(),failure:failed??null,scope:'one isolated cold native start, actual owned-process module paths, one true GPU frame; no repeated full campaign or user-instance operation'});
  fs.writeFileSync(path.join(evidence,'acceptance.json'),JSON.stringify(record,null,2));fs.writeFileSync(path.join(out,'runtime-dependencies-acceptance.json'),JSON.stringify(record,null,2));console.log(JSON.stringify({passed:record.passed,evidence,failure:record.failure}));
}
process.exitCode=failed?1:0;
