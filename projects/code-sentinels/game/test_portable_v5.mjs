/** V5 candidate acceptance through the packaged Node/bridge/engine and real HTTP/WS.
 * This script is intentionally not run until genuine media and the GPU are ready.
 * No browser automation, gameplay-state injection, compiler, or user save is used.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { deflateSync } from 'node:zlib';

const game=path.dirname(fileURLToPath(import.meta.url));
const argv=process.argv.slice(2);
const option=(key,fallback)=>{const i=argv.indexOf(key);return i<0?fallback:argv[i+1];};
if(argv.includes('--help')){
  console.log('node game/test_portable_v5.mjs --pack <candidate-folder> --off|--on [--out <evidence-folder>] [--min-fps 30] [--fps-seconds 8] [--keep-runtime]');
  process.exit(0);
}
assert.ok(!(argv.includes('--on')&&argv.includes('--off')),'choose one particle mode');
const mode=argv.includes('--on')?'on':'off';
const pack=path.resolve(option('--pack',path.join(game,'../dist/CodeSentinels-V5-Windows')));
const stamp=new Date().toISOString().replace(/[:.]/g,'-');
const out=path.resolve(option('--out',path.join(game,'v5/portable-runs',`${stamp}-${mode}`)));
const minFps=Number(option('--min-fps','30')),fpsSeconds=Number(option('--fps-seconds','8'));
assert.ok(Number.isFinite(minFps)&&minFps>0&&minFps<=60);
assert.ok(Number.isFinite(fpsSeconds)&&fpsSeconds>=5&&fpsSeconds<=60);
assert.ok(!fs.existsSync(out),'evidence directory must be new; prior receipts are preserved');
const inside=(parent,child)=>{const rel=path.relative(parent,child);return rel!==''&&!rel.startsWith('..'+path.sep)&&rel!=='..'&&!path.isAbsolute(rel);};
assert.ok(!inside(pack,out)&&out!==pack,'evidence cannot modify the candidate package');
const readJson=file=>JSON.parse(fs.readFileSync(file,'utf8').replace(/^\uFEFF/,''));
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const walk=dir=>fs.readdirSync(dir,{withFileTypes:true}).flatMap(e=>e.isDirectory()?walk(path.join(dir,e.name)):[path.join(dir,e.name)]);
const checks=[],runtimeErrors=[],streamStatuses=[],wsDiagnostics=[];
const ownedSockets=new Set(),closingSockets=new WeakSet();
let scratch,runtime,child,ws,base,enginePid,latestFrame,frameCount=0,streamFailure,enginePaused=false;
let stdout='',stderr='',failure,undrainedSteps=0;
fs.mkdirSync(out,{recursive:true});

function check(name,detail={}){const row={case:name,pass:true,...detail};checks.push(row);console.log(JSON.stringify(row));}
function named(names,values){return Object.fromEntries(names.split(' ').map((n,i)=>[n,values[i]]));}
function decode(entities){
  const map=new Map(entities.map(e=>[e.name,e]));
  const f=name=>{const e=map.get(name),v=[...(e?.transform?.translation??[]),...(e?.transform?.scale??[])];assert.equal(v.length,6,`${name} absent`);assert.ok(v.every(Number.isFinite),`${name} non-finite`);return v;};
  const s={entities:map,...named('credits compute baseHP phase wave level',f('C4_State')),
    ...named('powerGenerated powerDemand production capacity income tech',f('C4_Economy')),
    ...named('paused speed feedback activeEnemies spawned total',f('C4_Status')),
    ...named('revision shield shieldCapacity closedArea shieldAuto commandSeq',f('C4_Network')),
    ...named('unlocked baseCell enemyBaseCell spentAttack spentSkill spentShield',f('C4_Progress')),
    buildings:[],units:[],gpus:[],links:[],enemies:[],terrain:[],wireCells:[],wallCells:[],closedCells:[],shieldCells:[]};
  for(let i=0;i<48;i++)s.buildings.push({slot:i,...named('kind cell tier hp powered connected owner demand supply active rate repairCost range upgradeCost sellRefund targetCell x y',[...f(`C4_Building${i}`),...f(`C4_BuildingMeta${i}`),...f(`C4_BuildingExtra${i}`)])});
  for(let i=0;i<32;i++){
    s.units.push({slot:i,...named('kind cell tier hp battery batteryMax covered wired skillCooldown skillCost owner moving attackCost range upgradeCost starved active attacks x y targetCell targetMode casts centerSlot',[...f(`C4_Unit${i}`),...f(`C4_UnitMeta${i}`),...f(`C4_UnitCombat${i}`),...f(`C4_UnitPos${i}`)])});
    s.gpus.push({slot:i,...named('centerSlot model tier rate demand active powered capacity invested upgradeCost sellRefund bay',[...f(`C4_Gpu${i}`),...f(`C4_GpuMeta${i}`)])});
  }
  for(let i=0;i<64;i++){
    s.links.push({slot:i,...named('fromCell toCell kind hp active owner powered length',[...f(`C4_Link${i}`),...f(`C4_LinkMeta${i}`)])});
    const a=f(`C4_EnemyStat${i}`),b=f(`C4_EnemyPos${i}`);
    s.enemies.push({slot:i,...named('kind hp maxHp owner active x y cell',[...a.slice(0,5),...b.slice(0,3)])});
  }
  for(let row=0;row<20;row++){
    const t=f(`C4_MapRow${row}`),w=f(`C4_WireRow${row}`),a=f(`C4_WallRow${row}`);
    for(let c=0;c<32;c++){
      s.terrain.push((Math.round(t[Math.floor(c/8)])>>((c%8)*3))&7);
      s.wireCells.push((Math.round(w[Math.floor(c/8)])>>((c%8)*2))&3);
      for(const [name,offset] of [['wallCells',0],['closedCells',2],['shieldCells',4]])s[name].push(Boolean((Math.round(a[offset+Math.floor(c/16)])>>(c%16))&1));
    }
  }
  const global=f('C5_Global');assert.deepEqual(global.slice(4),[5,0]);
  s.animation={...named('time eventSeq ghostCount fxCount',global),buildings:[],units:[],events:[]};
  for(const [name,count,field] of [['Building',48,'buildings'],['Unit',32,'units']])for(let i=0;i<count;i++)s.animation[field].push({slot:i,...named('phase frame workAge phaseAge kind hitTime',f(`C5_${name}Anim${i}`))});
  for(let i=0;i<256;i++){
    const e=named('seq type kind x y age duration subject owner magnitude fxKind active',[...f(`C5_Event${i}`),...f(`C5_EventMeta${i}`)]);
    if(e.seq>0)s.animation.events.push(e);
  }
  s.animation.events.sort((a,b)=>a.seq-b.seq);
  assert.ok(s.animation.ghostCount<=24&&s.animation.fxCount<=64);
  return s;
}

async function jsonGet(route){const response=await fetch(base+route,{signal:AbortSignal.timeout(20000)});assert.equal(response.status,200,route);return response.json();}
async function call(name,args={}){
  if(streamFailure)throw streamFailure;
  const response=await fetch(base+'/api/forge/mcp/call',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({tool:`mcp__engine-scene__${name}`,arguments:args,workspaceId:'portable'}),signal:AbortSignal.timeout(60000)});
  const envelope=await response.json();assert.equal(response.status,200,JSON.stringify(envelope));assert.ok(!envelope.isError,JSON.stringify(envelope));
  return JSON.parse(envelope.content.find(c=>c.type==='text').text);
}
const state=async()=>decode((await call('entity_list')).entities);
async function steps(count){assert.ok(enginePaused);for(let i=0;i<count;i+=16){const n=Math.min(16,count-i);await Promise.all(Array.from({length:n},()=>call('play_step')));undrainedSteps+=n;if(undrainedSteps>=128){await drain();undrainedSteps=0;}}}
async function sim(seconds){const s=await state();await steps(Math.ceil(seconds*60/s.speed));return state();}
async function command(code,feedback,viaWs=false){
  const before=await state();assert.ok(enginePaused);
  if(viaWs){assert.equal(ws.readyState,WebSocket.OPEN);ws.send(JSON.stringify({type:'input',action:'cs5',value:code}));await sleep(60);}
  else await call('logic_inject_input',{action:'cs5',value:code});
  let after;
  for(let i=0;i<8;i++){await steps(1);after=await state();if(after.commandSeq>before.commandSeq)break;await sleep(15);}
  assert.equal(after.commandSeq,before.commandSeq+1,`command receipt ${code}`);
  if(feedback!==undefined)assert.equal(after.feedback,feedback,`command ${code}, credits ${after.credits}`);
  return after;
}
async function drain(){const result=await call('host_events_drain');const events=Array.isArray(result)?result:result.events;runtimeErrors.push(...events.filter(e=>/logic\.(call_error|unsupported)|anim\.warn|character\.error|DEV_ENV_DEGRADE/i.test(e.event??e.name??'')));return events;}
function cells(kind,cell){const n=kind===6?3:[1,2,4,5,8].includes(kind)?2:1;if(cell%32+n>32||Math.floor(cell/32)+n>20)return[];return Array.from({length:n*n},(_,i)=>cell+Math.floor(i/n)*32+i%n);}
const distance=(a,b)=>Math.hypot(a%32-b%32,Math.floor(a/32)-Math.floor(b/32));
function candidates(s,kind,near=s.baseCell,predicate=()=>true){
  const occupied=new Set([...s.buildings.filter(b=>b.active).flatMap(b=>cells(b.kind,b.cell)),...s.units.filter(u=>u.active).map(u=>u.cell)]);
  return Array.from({length:640},(_,c)=>c).filter(c=>{
    const f=cells(kind,c);return f.length&&f.every(n=>![1,2].includes(s.terrain[n])&&!occupied.has(n)&&!s.wallCells[n])&&(kind!==9||[5,6].includes(s.terrain[c]))&&predicate(c);
  }).sort((a,b)=>distance(a,near)-distance(b,near)||a-b);
}
async function buy(kind,cell,near){let s=await state();cell??=candidates(s,kind,near)[0];s=await command(1000000+kind*1000+cell,10);return s.buildings.find(b=>b.active&&b.cell===cell).slot;}
async function deploy(kind,cell){const s=await command(4000000+kind*1000+cell,10);return s.units.find(u=>u.active&&u.cell===cell).slot;}
function line(a,b){const p=[a];while(a%32!==b%32){a+=a%32<b%32?1:-1;p.push(a);}while(Math.floor(a/32)!==Math.floor(b/32)){a+=a<b?32:-32;p.push(a);}return p;}
async function connect(kind,a,b){const s=await state();if(!line(a,b).every(c=>![1,2].includes(s.terrain[c])))[a,b]=[b,a];assert.ok(line(a,b).every(c=>![1,2].includes(s.terrain[c])));const old=new Set(s.links.filter(l=>l.active).map(l=>l.slot));const after=await command((kind===1?5000000:6000000)+a*1000+b,kind===1?24:25);return after.links.find(l=>l.active&&!old.has(l.slot)).slot;}

function crc32(bytes){let crc=0xffffffff;for(const b of bytes){crc^=b;for(let i=0;i<8;i++)crc=(crc>>>1)^((crc&1)?0xedb88320:0);}return(crc^0xffffffff)>>>0;}
function png(width,height,rgba){
  const chunk=(tag,data)=>{const t=Buffer.from(tag),size=Buffer.alloc(4),crc=Buffer.alloc(4);size.writeUInt32BE(data.length);crc.writeUInt32BE(crc32(Buffer.concat([t,data])));return Buffer.concat([size,t,data,crc]);};
  const header=Buffer.alloc(13);header.writeUInt32BE(width);header.writeUInt32BE(height,4);header[8]=8;header[9]=6;
  const raw=Buffer.alloc(height*(width*4+1));for(let y=0;y<height;y++)rgba.copy(raw,y*(width*4+1)+1,y*width*4,(y+1)*width*4);
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(raw)),chunk('IEND',Buffer.alloc(0))]);
}
function pixelDifference(a,b){assert.equal(a.length,b.length);let changed=0,total=0;for(let i=0;i<a.length;i+=4){const d=Math.abs(a[i]-b[i])+Math.abs(a[i+1]-b[i+1])+Math.abs(a[i+2]-b[i+2]);if(d>3)changed++;total+=d;}return{changedPixels:changed,meanRgbDifference:total/(a.length/4*3)};}
async function capture(name){
  const f=await call('viewport_frame',{width:1280,height:720,format:'rgba8'});
  assert.equal(f.format,'rgba8');assert.equal(f.width,1280);assert.equal(f.height,720);assert.equal(f.truncated,false);assert.equal(f.meshFallbacks,0);assert.ok(f.deviceName&&f.draws>0&&f.nonZeroPixels>0);
  const bytes=Buffer.from(f.pixelsB64,'base64');assert.equal(bytes.length,1280*720*4);
  fs.writeFileSync(path.join(out,`${name}.png`),png(f.width,f.height,bytes));
  const {pixelsB64,...metadata}=f;fs.writeFileSync(path.join(out,`${name}.json`),JSON.stringify({...metadata,rgbaSha256:hash(bytes)},null,2));
  return {bytes,metadata};
}
async function connectStream(){
  const info=await call('viewport_stream_info');
  const socket=new WebSocket(info.wsUrl);socket.binaryType='arraybuffer';
  ownedSockets.add(socket);socket.addEventListener('close',event=>{ownedSockets.delete(socket);wsDiagnostics.push({event:'close',code:event.code,reason:event.reason,wasClean:event.wasClean,intentional:closingSockets.has(socket)});});
  await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>reject(Error('native WS frame timeout')),45000);
    socket.addEventListener('open',()=>{wsDiagnostics.push({event:'open'});socket.send(JSON.stringify({type:'subscribe',width:1280,height:720,maxFps:60}));});
    socket.addEventListener('error',event=>{const detail={event:'error',intentional:closingSockets.has(socket),message:event.error?.message??event.message??'',cause:event.error?.cause?.message??''};wsDiagnostics.push(detail);if(detail.intentional)return;streamFailure=Error(`native WS error: ${JSON.stringify(detail)}`);clearTimeout(timer);reject(streamFailure);});
    socket.addEventListener('message',event=>{
      try{
        if(event.data instanceof ArrayBuffer){
          const bytes=Buffer.from(event.data);assert.equal(bytes.readUInt32LE(0),0x31464746);assert.equal(bytes.readUInt16LE(8),1280);assert.equal(bytes.readUInt16LE(10),720);assert.equal(bytes.length,20+1280*720*4);assert.equal(bytes.readUInt32LE(12)&2,0,'truncated native stream');
          frameCount++;latestFrame={id:bytes.readUInt32LE(4),flags:bytes.readUInt32LE(12),draws:bytes.readUInt32LE(16),at:performance.now()};clearTimeout(timer);resolve();
        }else{
          const value=JSON.parse(event.data);if(value.type==='error'||value.shareError){streamFailure=Error(JSON.stringify(value));throw streamFailure;}
          if(value.type==='status'){assert.equal(value.truncated,false);streamStatuses.push(value);if(streamStatuses.length>60)streamStatuses.shift();}
        }
      }catch(error){streamFailure=error;clearTimeout(timer);reject(error);}
    });
  });
  return socket;
}
async function closeStream(){if(!ws)return;const socket=ws;ws=undefined;closingSockets.add(socket);socket.close();await Promise.race([new Promise(resolve=>socket.addEventListener('close',resolve,{once:true})),sleep(1500)]);}

async function main(){
  const inventory=readJson(path.join(pack,'Sources/video-frame-inventory.json'));
  assert.equal(inventory.version,5);assert.equal(inventory.newVideoFrames,1856);assert.equal(inventory.items.length,20);assert.ok(inventory.items.every(i=>i.provenance),'genuine video provenance missing');
  assert.ok(fs.existsSync(path.join(pack,'Content/Scenes/CommandV5.rxscene')));
  const authoredScene=readJson(path.join(pack,'Content/Scenes/CommandV5.rxscene'));
  const body=authoredScene.entities.find(e=>e.name==='C4_Structure0')?.components.find(c=>(c.type??c.ctype)==='Sprite')?.props;
  assert.equal(body?.spriteVariants?.length,12);assert.equal(body.variantStride,128);assert.equal(body.pixelsPerUnit,256);
  assert.ok(fs.existsSync(path.join(pack,'bin/node.exe'))&&fs.existsSync(path.join(pack,'bin/engine-host.exe')));
  scratch=fs.mkdtempSync(path.join(os.tmpdir(),'sentinels-v5-portable-'));runtime=path.join(scratch,'runtime');assert.ok(inside(scratch,runtime)&&!inside(pack,runtime));fs.mkdirSync(runtime);
  // Independent copies, never hardlinks/junctions into the original candidate.
  for(const name of ['bin','Content','Web','.forge/cache','bridge.mjs','multiplayer-v4.mjs','forge.toml']){
    const destination=path.join(runtime,name);assert.ok(inside(runtime,destination));fs.mkdirSync(path.dirname(destination),{recursive:true});fs.cpSync(path.join(pack,name),destination,{recursive:true,errorOnExist:true,force:false,dereference:true});
  }
  check('complete_real_media_candidate_and_isolated_runtime',{pack,runtime,frames:inventory.newVideoFrames});
  child=spawn(path.join(runtime,'bin/node.exe'),[path.join(runtime,'bridge.mjs'),'--no-open',...(mode==='on'?['--gpu-particles']:[])],{cwd:runtime,windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,FORGE_GPU_PARTICLES:mode==='on'?'off':'on',FORGE_RUSTC:path.join(runtime,'compiler-not-required.exe')}});
  base=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('candidate bridge startup timeout')),30000);child.stdout.on('data',chunk=>{stdout+=chunk.toString();const m=/ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(stdout);if(m){clearTimeout(timer);resolve(m[1]);}});child.stderr.on('data',chunk=>stderr+=chunk.toString());child.once('error',reject);child.once('exit',code=>{clearTimeout(timer);reject(Error(`candidate bridge exit ${code}`));});});
  const health=await jsonGet('/health');assert.equal(path.resolve(health.root),runtime);assert.equal(health.gpuParticles,mode);assert.equal(health.compilerDisabled,true);enginePid=health.enginePid;assert.ok(Number.isInteger(enginePid)&&enginePid!==process.pid);
  assert.equal((await call('host_ping')).pid,enginePid);
  check('packaged_node_engine_and_explicit_mode_no_compiler',{health,nodeSha256:hash(fs.readFileSync(path.join(runtime,'bin/node.exe'))),engineSha256:hash(fs.readFileSync(path.join(runtime,'bin/engine-host.exe')))});
  const capability=await jsonGet('/api/sentinels/net/capabilities');assert.equal(capability.combatAvailable,false);assert.equal(capability.lobbyAvailable,true);check('pvp_truthfully_lobby_only',{capability});
  const progress=await jsonGet('/api/sentinels/campaign-progress');assert.equal(progress.unlocked,1);
  const web=path.join(runtime,'Web'),files=walk(web),missing=[];
  for(let i=0;i<files.length;i+=8)await Promise.all(files.slice(i,i+8).map(async file=>{const rel=path.relative(web,file).split(path.sep).map(encodeURIComponent).join('/');const response=await fetch(base+'/'+rel,{method:'HEAD',signal:AbortSignal.timeout(20000)});if(response.status!==200||Number(response.headers.get('content-length'))!==fs.statSync(file).size)missing.push(rel);}));
  assert.deepEqual(missing,[]);
  const html=await fetch(base+'/?play=code-sentinels&workspace=portable&standalone=1&version=5').then(r=>r.text());assert.ok(/\/assets\/.+\.js/.test(html));
  const references=new Set();for(const file of files.filter(f=>/\.(js|css|html)$/.test(f)))for(const m of fs.readFileSync(file,'utf8').matchAll(/["'`](\/(?:assets|games)\/[^\s"'`]+\.(?:png|jpg|jpeg|webp|svg|avif|gif|woff2?|ttf|js|css|json)(?:\?[^\s"'`]*)?)["'`]/g))references.add(m[1]);
  for(const ref of references){const response=await fetch(base+ref,{method:'HEAD',signal:AbortSignal.timeout(20000)});assert.equal(response.status,200,ref);}
  check('all_ui_assets_and_literal_references_resolve',{files:files.length,literalReferences:references.size});
  assert.equal((await call('scene_summary')).playState,'edit');
  await call('scene_load',{path:'Content/Scenes/CommandV5.rxscene'});await call('play_enter');await call('play_pause');enginePaused=true;await steps(1);
  let s=await state();assert.equal(s.credits,2000);assert.equal(s.compute,0);assert.equal(s.baseHP,400);assert.equal(s.phase,0);assert.equal(s.level,1);assert.equal(s.unlocked,progress.unlocked);assert.equal(s.gpus.filter(g=>g.active).length,0);
  check('complete_c4_c5_cold_native_protocol',{credits:s.credits,compute:s.compute,buildings:s.buildings.length,units:s.units.length,links:s.links.length,eventSlots:256,core:s.animation.buildings[0]});
  const land=await capture('01-core-land');s=await sim(.6);assert.equal(s.animation.buildings[0].phase,1);assert.ok(s.animation.buildings[0].frame>0);const later=await capture('02-core-land-progress');const difference=pixelDifference(land.bytes,later.bytes);assert.ok(difference.changedPixels>20);s=await sim(1.6);assert.equal(s.animation.buildings[0].phase,2);check('real_rgba_land_frames_change',{difference,core:s.animation.buildings[0],device:later.metadata.deviceName});
  const extractor=await buy(9),dc=await buy(2),wind=await buy(3),lab=await buy(8);s=await state();
  const dcCell=s.buildings[dc].cell,windCell=s.buildings[wind].cell,labCell=s.buildings[lab].cell;
  const power=await connect(1,windCell,dcCell);await connect(1,dcCell,labCell);await connect(2,dcCell,labCell);
  await command(3000000+dc*100+1,20);await command(3000000+dc*100+11,20);s=await sim(2.2);assert.equal(s.production,24);assert.equal(s.capacity,600);assert.equal(s.animation.buildings[dc].phase,2);assert.ok(s.income>0);
  check('real_extractor_generation_gpu_and_lab_network',{extractor,dc,wind,lab,production:s.production,power:s.powerGenerated,demand:s.powerDemand});
  const workAge=s.animation.buildings[dc].workAge;await command(5800000+power,26);s=await sim(.2);assert.equal(s.animation.buildings[dc].phase,3);const stopped=s.animation.buildings[dc];s=await sim(.7);assert.equal(s.animation.buildings[dc].workAge,stopped.workAge);await connect(1,windCell,dcCell);s=await sim(.2);assert.equal(s.animation.buildings[dc].phase,2);assert.ok(s.animation.buildings[dc].workAge>workAge);check('power_loss_stops_work_and_restoration_resumes',{stopped,resumed:s.animation.buildings[dc]});
  s=await sim(6);assert.ok(s.compute>=120);const researchBefore={credits:s.credits,compute:s.compute,time:s.animation.time};s=await command(9000001,46);assert.equal(s.tech,1);assert.ok(s.credits<researchBefore.credits-249&&s.compute<researchBefore.compute-118);check('native_research_cost_and_animation',{before:researchBefore,after:{credits:s.credits,compute:s.compute,tech:s.tech}});
  // Wait for real mine income only, using the game's ordinary 3x speed command.
  await command(10000001);await command(10000001);s=await state();assert.equal(s.speed,3);if(s.credits<700)s=await sim((700-s.credits)/s.income+1);await command(10000001);
  s=await state();const free=candidates(s,3,dcCell,c=>Math.floor(c/32)===Math.floor(dcCell/32)&&c%32>dcCell%32+2&&c%32<14);assert.ok(free.length>=3);free.sort((a,b)=>a-b);
  const middle=free[free.length-3],endpoint=free[free.length-1];const ai=await deploy(3,endpoint),middleAi=await deploy(3,middle);const tether=await connect(2,dcCell,endpoint);s=await state();assert.equal(s.units[ai].wired,1);assert.equal(s.units[middleAi].wired,0);
  const target=candidates(s,3,endpoint)[0];await command(4300000+ai*1000+target,42);await command(5800000+tether,26);await command(4300000+ai*1000+target,43);s=await sim(.25);assert.ok(s.units[ai].moving||s.units[ai].cell===target);assert.equal(s.animation.units[ai].frame,-1);check('real_mobile_ai_and_endpoint_only_stationing',{middle:s.units[middleAi],moving:s.units[ai]});
  await command(4200000+ai,12);await command(4200000+middleAi,12);
  ws=await connectStream();s=await state();const gpuCount=s.gpus.filter(g=>g.active).length;const beforeSeq=s.commandSeq;
  await command(3000000+dc*100+21,20,true);s=await state();assert.equal(s.gpus.filter(g=>g.active).length,gpuCount+1);assert.equal(s.commandSeq,beforeSeq+1);
  const purchased={credits:s.credits,compute:s.compute,seq:s.commandSeq,tech:s.tech,time:s.animation.time,gpus:s.gpus};await closeStream();ws=await connectStream();s=await state();assert.deepEqual({credits:s.credits,compute:s.compute,seq:s.commandSeq,tech:s.tech,time:s.animation.time,gpus:s.gpus},purchased);check('ws_reconnect_preserves_purchase_economy_and_progress',{commandSeq:s.commandSeq,gpus:gpuCount+1});
  // Three cards can overload the starter wind: restore actual supply with another generator.
  const wind2=await buy(3);s=await state();await connect(1,s.buildings[wind2].cell,dcCell);
  s=await state();if(s.credits<500){await closeStream();await command(10000001);await command(10000001);s=await sim((500-s.credits)/s.income+1);await command(10000001);ws=await connectStream();}
  await buy(7,undefined,326);s=await state();const turret=await deploy(1,candidates(s,3,327)[0]);await sim(3);await command(10000000,14);
  for(let i=0;i<150;i++){s=await sim(.5);if(s.units[turret].attacks>0)break;}assert.ok(s.units[turret].attacks>0&&s.activeEnemies>0);assert.equal(s.animation.units[turret].phase,2);
  const firstAttack=s.animation.units[turret];s=await sim(.5);assert.ok(s.animation.units[turret].workAge>firstAttack.workAge+.35);check('real_software_work_cycle_advances_after_paid_attack',{first:firstAttack,later:s.animation.units[turret],attacks:s.units[turret].attacks});await capture('03-live-combat');
  await call('play_resume');enginePaused=false;await sleep(1500);const countStart=frameCount,t0=performance.now(),combatSamples=[];
  while(performance.now()-t0<fpsSeconds*1000){await sleep(500);const live=await state();combatSamples.push({time:live.animation.time,phase:live.phase,enemies:live.activeEnemies,coreHp:live.baseHP});if(live.phase===1&&live.baseHP<200&&live.credits>=40)ws.send(JSON.stringify({type:'input',action:'cs5',value:2200000}));}
  const seconds=(performance.now()-t0)/1000,sampledFrames=frameCount-countStart,fps=sampledFrames/seconds,lastLiveFrame=latestFrame;
  await call('play_pause');enginePaused=true;s=await state();await drain();if(streamFailure)throw streamFailure;assert.ok(lastLiveFrame.draws>0);check('live_combat_fps_sample',{pass:fps>=minFps,fps,threshold:minFps,frames:sampledFrames,seconds,lastFrame:lastLiveFrame,statuses:streamStatuses.slice(-4),combatSamples,spentAttack:s.spentAttack});assert.ok(fps>=minFps,`actual live FPS ${fps.toFixed(2)} < ${minFps}`);assert.ok(combatSamples.every(sample=>sample.phase===1&&sample.enemies>0),'FPS window must contain an active wave throughout');
  await closeStream();
  // Remove compute by normal sale so the real remaining enemies can destroy the core.
  for(const g of s.gpus.filter(g=>g.active))await command(3200000+g.slot,22);
  const hits=[];for(let i=0;i<2000;i++){s=await sim(.1);if(s.animation.events.some(e=>e.active&&[4,6,17].includes(e.type)))hits.push(...s.animation.events.filter(e=>e.active&&[4,6,17].includes(e.type)));if(s.phase===3)break;}
  assert.equal(s.phase,3,'real enemy-driven defeat must occur');assert.equal(s.baseHP,0);assert.equal(s.animation.buildings[0].phase,4);const deathClock=s.animation.time,phaseAge=s.animation.buildings[0].phaseAge;
  const economy={credits:s.credits,compute:s.compute,production:s.production,income:s.income};await capture('04-core-destroy-start');s=await sim(Math.max(0,1.2-phaseAge));assert.equal(s.animation.buildings[0].phase,4);const mid=await capture('05-core-destroy-mid');s=await sim(.95);assert.equal(s.animation.buildings[0].phase,5);assert.equal(s.animation.buildings[0].frame,127);const wreck=await capture('06-core-wreck-hold');assert.ok(pixelDifference(mid.bytes,wreck.bytes).changedPixels>20);
  s=await sim(2);assert.equal(s.animation.buildings[0].phase,5);s=await sim(.6);assert.equal(s.animation.buildings[0].phase,0);assert.deepEqual({credits:s.credits,compute:s.compute,production:s.production,income:s.income},economy);await capture('07-core-tail-finished');check('real_core_defeat_complete_destroy_and_wreck_tail',{clockBefore:deathClock,clockAfter:s.animation.time,economy,observedHitEvents:hits.slice(-12)});
  await drain();assert.deepEqual(runtimeErrors,[]);await call('play_exit');enginePaused=false;
  // Endpoint parser fixture is confined to the private save, after gameplay ends.
  const saveFile=path.join(runtime,'.forge/save/code-sentinels-v4.txt');assert.ok(inside(runtime,saveFile));fs.mkdirSync(path.dirname(saveFile),{recursive:true});
  for(const [text,expected] of [['2\n',2],['3',3],['invalid',1],['4294967296',1]]){fs.writeFileSync(saveFile,text);assert.equal((await jsonGet('/api/sentinels/campaign-progress')).unlocked,expected);}
  fs.rmSync(saveFile);check('campaign_progress_reads_actual_isolated_file',{scope:'file-parser fixture after play.exit; does not claim earned unlocks',nativeFreshUnlocked:progress.unlocked});
  check('no_runtime_or_stream_errors',{runtimeErrors,statuses:streamStatuses.slice(-4)});
}

try{await main();}catch(error){failure={message:error.message,stack:error.stack};console.error(error.stack);}
finally{
  await closeStream().catch(()=>{});
  for(const socket of ownedSockets){try{closingSockets.add(socket);socket.close();}catch{}}
  if(base&&child?.exitCode===null){try{const summary=await call('scene_summary');if(summary.playState!=='edit')await call('play_exit');}catch{}}
  if(child?.pid&&child.exitCode===null){
    if(process.platform==='win32')await new Promise(resolve=>{const killer=spawn('taskkill.exe',['/PID',String(child.pid),'/T','/F'],{windowsHide:true,stdio:'ignore'});killer.once('exit',resolve);killer.once('error',resolve);});
    else child.kill('SIGTERM');
  }
  await sleep(250);
  if(enginePid){try{process.kill(enginePid,0);process.kill(enginePid);await sleep(250);}catch(error){if(error.code!=='ESRCH')failure??={message:'could not close owned engine process',detail:error.message};}}
  fs.writeFileSync(path.join(out,'bridge-stdout.log'),stdout);fs.writeFileSync(path.join(out,'bridge-stderr.log'),stderr);
  if(runtime&&fs.existsSync(path.join(runtime,'Logs')))fs.cpSync(path.join(runtime,'Logs'),path.join(out,'Logs'),{recursive:true});
  if(runtime){const log=path.join(runtime,'Logs',mode==='on'?'gpu-particles':'normal','engine.log');if(fs.existsSync(log)){const text=fs.readFileSync(log,'utf8');const errors=text.split(/\r?\n/).filter(l=>/DEV_ENV_DEGRADE|logic\.call_error|logic\.unsupported|anim\.warn|panicked at|failed to load.*(?:dll|sprite)/i.test(l));if(errors.length)failure??={message:'native runtime log errors',errors:errors.slice(-20)};}}
  let runtimeCleaned=false;
  if(scratch&&!argv.includes('--keep-runtime')){
    try{const resolved=fs.realpathSync(scratch),tempRoot=fs.realpathSync(os.tmpdir());assert.ok(inside(tempRoot,resolved)&&path.basename(resolved).startsWith('sentinels-v5-portable-')&&resolved!==pack&&!inside(resolved,pack));fs.rmSync(resolved,{recursive:true,force:false,maxRetries:5,retryDelay:300});runtimeCleaned=true;}
    catch(error){failure??={message:'isolated runtime cleanup failed',detail:error.message};}
  }
  const report={passed:!failure,executed:true,mode,pack,enginePid,runtimeDirectory:runtime,runtimeCleaned,finishedAt:new Date().toISOString(),checks,failure:failure??null,runtimeErrors,wsDiagnostics,gpuVisualAcceptance:!failure,scope:'real candidate HTTP/WS/native/RGBA integration; browser UI interaction and all three campaign levels are separate acceptance',evidenceDirectory:out};
  fs.writeFileSync(path.join(out,'portable-acceptance.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify({passed:!failure,mode,out,failure:failure?.message}));
}
process.exitCode=failure?1:0;
