/** Explicit unearned pressure fixture. Only private localhost native RPC is used. */
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';import {performance} from 'node:perf_hooks';
import {launchNative} from './native-rpc.mjs';
import {spawn} from 'node:child_process';
const project=path.resolve(process.argv[2]||'projects/code-sentinels');
const executable=path.resolve(process.argv[3]||path.join(project,'dist/CodeSentinels-V6-Windows/bin/engine-host.exe'));
const output=path.join(project,'Logs/v6/pressure');fs.mkdirSync(output,{recursive:true});
process.env.FORGE_V6_DIAGNOSTICS='1';
const native=await launchNative({root:project,executable,logs:path.join(output,'engine')});
const rpc=(method,input={},timeout=600000)=>native.rpc(method,input,timeout);
let ws,telemetryProcess;const details=[];
const telemetry={requested:process.argv.includes('--gpu-telemetry'),sampleIntervalMs:1000,errors:[],scope:'Optional read-only nvidia-smi samples. UTC window markers map the raw local CSV timestamp using the recorded timezone offset. N/A stays missing; no power/clock settings are changed.'};
function startTelemetry(){
 if(!telemetry.requested)return;
 telemetry.command=['nvidia-smi','--query-gpu=timestamp,index,name,pstate,temperature.gpu,clocks.gr,clocks.mem,power.draw,power.limit,utilization.gpu,utilization.memory,memory.used,memory.total','--format=csv','--loop-ms=1000'];
 telemetry.startedAtUtc=new Date().toISOString();telemetry.localUtcOffsetMinutes=-new Date().getTimezoneOffset();
 try{telemetryProcess=spawn(telemetry.command[0],telemetry.command.slice(1),{windowsHide:true,stdio:['ignore','pipe','pipe']});telemetry.pid=telemetryProcess.pid??null;
  telemetryProcess.on('error',error=>telemetry.errors.push(error.message));
  for(const [stream,file]of[[telemetryProcess.stdout,'gpu-telemetry.csv'],[telemetryProcess.stderr,'gpu-telemetry-stderr.log']])stream.on('data',bytes=>{try{fs.appendFileSync(path.join(output,file),bytes);}catch(error){telemetry.errors.push(error.message);}});
 }catch(error){telemetry.errors.push(error.message);}
}
async function stopTelemetry(){
 if(telemetryProcess&&telemetryProcess.exitCode===null&&telemetryProcess.signalCode===null){telemetryProcess.kill();await Promise.race([new Promise(resolve=>telemetryProcess.once('exit',resolve)),new Promise(resolve=>setTimeout(resolve,2000))]);}
 telemetry.stoppedAtUtc=new Date().toISOString();
 try{fs.writeFileSync(path.join(output,'gpu-telemetry.json'),JSON.stringify(telemetry,null,2));}catch(error){console.error('Optional telemetry write failed:',error.message);}
}
function fixture(save,catalog){
 const s=save.snapshot;let id=10000;const next=()=>id++;const pos=(x,y,z)=>({x,y,z});
 for(const key of ['rooms','units','links','walls','entrances','jobs','resources','shipments','projectiles','events','rubble','defenseFields','shieldRegions','networkStores','powerGrids','excavated'])s[key]=[];
 s.excavationOwners={};s.terrain.fill(0);s.visible=[[],[]];s.explored=[[],[]];s.tick=0;s.revision=0;s.winner=null;s.playback=null;
 s.buildings=s.buildings.filter(b=>b.kind==='core');for(const b of s.buildings){b.hp=b.maxHp=1e9;}
 for(const p of s.players){p.ai=false;p.credits=1e9;p.science=1e9;p.branches=Object.fromEntries(['speed','security','algorithm','science','lightweight'].map(b=>[b,5]));}
 const baseBuilding=s.buildings[0];const rect=(x,y,z,w=12,h=12)=>({x,y,z,w,h});
 const line=(a,b)=>{const out=[a];let p={...a};for(const k of ['x','y','z'])while(p[k]!==b[k]){p={...p,[k]:p[k]+Math.sign(b[k]-p[k])};out.push(p);}return out;};
 const link=(owner,kind,points)=>s.links.push({id:next(),owner,kind,path:points,hp:150,active:true,invested:0,unitEndpoints:[]});
 for(let n=0;n<16;n++){
  const x=24+(n%4)*22,y=7+Math.floor(n/4)*22,owner=1+n%2;
  s.buildings.push({...structuredClone(baseBuilding),id:next(),owner,kind:'wind-power',rect:rect(x-4,y+4,0,3,3),tier:5,hp:1e6,maxHp:1e6,power:1e6,demand:0,stock:{},inventory:0,invested:0});
  s.entrances.push({id:next(),owner,pos:pos(x+5,y+5,-2),toLevel:5,kind:'elevator',hp:300,open:true,powered:true,width:2,axis:'x'});
  link(owner,'power',line(pos(x-3,y+5,0),pos(x+5,y+5,0)));link(owner,'power',line(pos(x+5,y+5,-2),pos(x+5,y+5,5)));
  for(let z=-2;z<=5;z++){
   const shell=next();s.buildings.push({...structuredClone(baseBuilding),id:shell,owner,kind:'shell',rect:rect(x,y,z),tier:5,hp:1e6,maxHp:1e6,power:0,demand:0,stock:{},inventory:0,invested:0,supportRatio:1,collapseWarning:0});
   for(let yy=y;yy<y+12;yy++)for(let xx=x;xx<x+12;xx++){const p=pos(xx,yy,z);s.visible[0].push(p);s.explored[0].push(p);if(z<0){s.excavated.push(p);s.excavationOwners[(z+2)*128*96+yy*128+xx]=owner;}}
   for(let slot=0;slot<4;slot++){
    const dx=[1,7,1,7][slot],dy=[1,1,7,7][slot],kind=['data-center','research-lab','factory','depot'][slot];const definition=catalog.rooms.find(r=>r.id===kind);
    const r={id:next(),shell,owner,rect:rect(x+dx,y+dy,z,4,4),kind,branch:slot===1?'algorithm':null,tier:5,hp:1e6,maxHp:1e6,antiHeal:0,equipmentShare:1,powered:true,connected:true,online:true,maintenance:0,capacity:Math.max(1,Math.floor(16*definition.capacityPerArea)),gpus:slot===0?Array(4).fill('h200'):[],inventory:0,stock:{},progress:1,buildTime:1,cooldown:0,invested:0};
    s.rooms.push(r);link(owner,'power',line(pos(x+5,y+5,z),pos(x+dx+1,y+dy+1,z)));if(slot>0)link(owner,'compute',line(pos(x+2,y+2,z),pos(x+dx+1,y+dy+1,z)));
   }
   s.entrances.push({id:next(),owner,pos:pos(x+5,y+11,z),toLevel:z,kind:'door',hp:300,open:true,powered:false,width:2,axis:'x'});
  }
 }
 for(let z=-2;z<=5;z++)for(let n=0;n<25;n++){
  const c=Math.floor(n/2),x=24+(c%4)*22+5,base=7+Math.floor(c/4)*22,y=base+(n%2===0?3:8);const kind=n%5===0?'deepseek':n%5===1?'claude':'scout-buggy';const d=catalog.units.find(u=>u.id===kind);assert(d);
  const goal=pos(x,base+(n%2===0?9:2),z);
  s.units.push({id:next(),owner:1+n%2,kind,pos:pos(x,y,z),x:x+.5,y:y+.5,z,tier:d.tier,hp:1e6,maxHp:1e6,battery:240,batteryMax:240,covered:false,wired:false,ammo:d.ammoCapacity,ammoMax:d.ammoCapacity,fuel:d.fuelCapacity,fuelMax:d.fuelCapacity,energy:d.energyCapacity,energyMax:d.energyCapacity,route:line(pos(x,y,z),goal).slice(1),goal,queuedGoals:[],target:null,cooldown:0,skillCooldown:0,plugins:[],pluginDiscount:0,chargedShotMultiplier:1,statuses:{},invested:0,moving:true,attackCount:0,branch:d.branch,facing:0,altitude:0,flightState:'ground',sourceFacility:0,sortieTarget:null,transitProgress:0,lastAttackTick:null,lastCastTick:null,lastHitTick:null,dash:null});
 }
 for(let n=0;n<600;n++){const u=s.units[n%s.units.length],to=pos(u.pos.x,Math.min(94,u.pos.y+3),u.z);s.projectiles.push({id:next(),owner:u.owner,source:u.id,target:null,origin:u.pos,destination:to,x:u.x,y:u.y,z:u.z+.5,age:0,duration:.75,damage:.1,radius:n%3===0?2:0,kind:n%3===0?'arc':'direct',damageType:n%3===0?'explosive':'kinetic',penetration:0,structureMultiplier:1,sourceAltitude:.5,targetAltitude:.5,jammed:false,launchPosition:[u.x,u.y,u.z+.5],aimPosition:[to.x+.5,to.y+.5,to.z+.5],passedSurfaces:[],onHitStatus:null,targetMultiplier:1,movingTargetMultiplier:1});}
 save.nextId=id;save.initialAi=false;save.sequences=[0,0];save.orders=[];save.administrativeEvents=[];return save;
}
try{
 await rpc('game.session.open',{mode:'authority',seed:600128,opponent:'human'});await rpc('play.pause');const catalog=await rpc('game.session.catalog');
 const save=fixture(await rpc('game.session.save'),catalog);fs.writeFileSync(path.join(output,'pressure-save.json'),JSON.stringify(save));
 if(process.argv.includes('--generate-only')){await rpc('game.session.load',{save});await rpc('play.pause');console.log(JSON.stringify({fixtureValidated:true,output,nativeHash:native.rulesHash}));}
 else {
  const result=await rpc('game.session.pressureBenchmark',{save,seconds:60});console.log(JSON.stringify({simulation:result}));
  if(!process.argv.includes('--simulation-only')){
  const base=save.snapshot,frames=JSON.parse(fs.readFileSync(path.join(output,'render-frames.json'),'utf8'));
  await rpc('game.session.open',{mode:'replica',seed:base.seed,theme:base.theme,localPlayer:1,opponent:'human'});
  await rpc('game.session.view',{centerX:63,centerY:46,zoom:.65,layer:0,cutaway:true,localPlayer:1});
  startTelemetry();
  let frameCount=0,truncated=0,firstAt=0,lastAt=0,maxGap=0,dimensions=new Set(),errors=[],statuses=[];let currentLayer=0;const layerBuckets={};
  const info=await rpc('viewport.streamInfo');ws=new WebSocket(info.wsUrl);ws.binaryType='arraybuffer';
  await new Promise((resolve,reject)=>{ws.addEventListener('open',()=>{ws.send(JSON.stringify({type:'subscribe',width:1280,height:720,maxFps:60}));resolve();});ws.addEventListener('error',()=>reject(new Error('Native viewport socket error')));});
  let measuring=false;ws.addEventListener('message',event=>{if(typeof event.data==='string'){const value=JSON.parse(event.data);if(value.type==='error')errors.push(value.message);if(value.type==='status')statuses.push(value);return;}if(!measuring)return;const b=Buffer.from(event.data);if(b.readUInt32LE(0)!==0x31464746)return;const now=performance.now();firstAt||=now;if(lastAt)maxGap=Math.max(maxGap,now-lastAt);lastAt=now;frameCount++;const bucket=layerBuckets[currentLayer];if(bucket){bucket.frames++;if(bucket.frames===5){const width=b.readUInt16LE(8),height=b.readUInt16LE(10),rgb=Buffer.alloc(width*height*3);for(let i=0;i<width*height;i++){rgb[i*3]=b[20+i*4];rgb[i*3+1]=b[21+i*4];rgb[i*3+2]=b[22+i*4];}fs.writeFileSync(path.join(output,'layer-'+currentLayer+'.ppm'),Buffer.concat([Buffer.from('P6\n'+width+' '+height+'\n255\n'),rgb]));}}if(b.readUInt32LE(12)&2)truncated++;dimensions.add(b.readUInt16LE(8)+'x'+b.readUInt16LE(10));});
  const span=frames.at(-1).tick-frames[0].tick+3;let sent=0;const update=async()=>{const f=structuredClone(frames[sent%frames.length]);const offset=Math.floor(sent/frames.length)*span;f.tick+=offset;f.revision+=offset;for(const e of f.events)e.tick+=offset;for(const u of f.units)for(const k of ['lastAttackTick','lastCastTick','lastHitTick'])if(u[k]!==null)u[k]+=offset;sent++;await rpc('game.session.applySnapshot',{snapshot:{...base,...f}});};
  for(let i=0;i<80;i++){await update();await new Promise(r=>setTimeout(r,50));}
  const pre=await rpc('viewport.frame',{width:1280,height:720,format:'rgba8'});delete pre.pixelsB64;details.push(pre);
  telemetry.measurementStartUtc=new Date().toISOString();measuring=true;const start=performance.now();let layer=0;while(performance.now()-start<60000){const before=performance.now();const next=Math.floor((before-start)/7500)-2;if(next!==layer){if(layerBuckets[currentLayer])layerBuckets[currentLayer].end=before;layer=next;currentLayer=layer;layerBuckets[layer]={frames:0,start:before};await rpc('game.session.view',{layer});}await update();await new Promise(r=>setTimeout(r,Math.max(0,50-(performance.now()-before))));}const measurementEnd=performance.now();const measurementWall=(measurementEnd-start)/1000;measuring=false;telemetry.measurementEndUtc=new Date().toISOString();if(layerBuckets[currentLayer])layerBuckets[currentLayer].end=measurementEnd;for(const bucket of Object.values(layerBuckets)){bucket.seconds=(bucket.end-bucket.start)/1000;bucket.fps=bucket.frames/bucket.seconds;}
  const end=await rpc('viewport.frame',{width:1280,height:720,format:'rgba8'});delete end.pixelsB64;details.push(end);
  const gpu={scope:'Actual native Rurix1280x720 RGBA stream, replaying120 real native pressure snapshots with continuous local visual clock; no LAN RGBA forwarding. Eight cutaway layers sampled for7.5sec each. Separate from Game::step timing.',wallSeconds:measurementWall,frameCount,perLayer:layerBuckets,observedFps:frameCount/measurementWall,maxInterframeGapMs:maxGap,truncatedFrames:truncated,dimensions:[...dimensions],errors,statuses,frameDiagnostics:details,privateMetrics:await rpc('game.session.metrics'),nativeHash:native.rulesHash};gpu.passed=Object.keys(layerBuckets).length===8&&Object.values(layerBuckets).every(b=>b.fps>=30)&&gpu.observedFps>=30&&truncated===0&&errors.length===0&&details.every(d=>d.meshFallbacks===0&&d.width===1280&&d.height===720);fs.writeFileSync(path.join(output,'gpu-report.json'),JSON.stringify(gpu,null,2));console.log(JSON.stringify({gpu}));
  }
 }
}finally{await stopTelemetry();ws?.close();await native.close();}
