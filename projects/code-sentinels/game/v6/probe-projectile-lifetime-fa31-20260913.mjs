/** Explicit unearned, paused native fixtures. No GPU, balance or earned-game acceptance. */
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {launchNative} from './native-rpc.mjs';

const project=path.resolve('projects/code-sentinels');
const out=path.join(project,'game/v6/projectile-lifetime-fa31-20260913-b');
assert.ok(!fs.existsSync(out),'Preserve every previous native probe');
fs.mkdirSync(out);
const executable=path.join(project,'game/v6/runtime-bin/engine-host.exe');
const sha=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const expectedHost='fa31b3608a0f418f6bed58d0cac70c2c09885ba2d9912733c1c8aff07c0f1a1e';
const expectedRules='0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a';
assert.equal(sha(executable),expectedHost);
const write=(name,value)=>fs.writeFileSync(path.join(out,name),JSON.stringify(value,null,2),{flag:'wx'});
const ref=file=>({path:path.relative(project,file).replaceAll('\\','/'),sha256:sha(file)});
const startedAtUtc=new Date().toISOString();
const methods=new Set();
const native=await launchNative({root:project,executable,logs:path.join(out,'process')});
const rpc=(method,input={})=>{methods.add(method);return native.rpc(method,input);};
const reports=[];
let error=null,catalog=null;
const pos=(x,y,z=0)=>({x,y,z});
function actor(id,owner,kind,x,y,definition,route=[]){
 return {id,owner,kind,pos:pos(x,y),x:x+.5,y:y+.5,z:0,velocityX:0,velocityY:0,tier:definition.tier,
  hp:definition.hp,maxHp:definition.hp,battery:0,batteryMax:0,covered:false,wired:false,
  ammo:definition.ammoCapacity,ammoMax:definition.ammoCapacity,fuel:definition.fuelCapacity,fuelMax:definition.fuelCapacity,
  energy:definition.energyCapacity,energyMax:definition.energyCapacity,route,goal:route.at(-1)??null,queuedGoals:[],
  target:owner===1?10002:null,cooldown:owner===1?0:1000,skillCooldown:1000,plugins:[],pluginDiscount:0,chargedShotMultiplier:1,
  statuses:{},invested:definition.cost,moving:route.length>0,attackCount:0,branch:definition.branch,facing:0,altitude:0,
  flightState:'ground',sourceFacility:0,sortieTarget:null,transitProgress:0,lastAttackTick:null,lastCastTick:null,lastHitTick:null,dash:null};
}
function trace(snapshot,stage){
 return {stage,tick:snapshot.tick,revision:snapshot.revision,units:snapshot.units.map(u=>({id:u.id,kind:u.kind,x:u.x,y:u.y,z:u.z,pos:u.pos,hp:u.hp,
  moving:u.moving,velocityX:u.velocityX,velocityY:u.velocityY,route:u.route,ammo:u.ammo,energy:u.energy,cooldown:u.cooldown,attackCount:u.attackCount,lastAttackTick:u.lastAttackTick})),
  projectiles:snapshot.projectiles,walls:snapshot.walls,events:snapshot.events.filter(e=>e.tick===snapshot.tick)};
}
try{
 catalog=await rpc('game.session.catalog');
 assert.equal(catalog.rulesFingerprint,expectedRules);
 write('catalog.json',catalog);
 await rpc('game.session.open',{mode:'authority',seed:602113,theme:'river',opponent:'human'});
 await rpc('play.pause');
 const original=await rpc('game.session.save');
 write('initial-native-save.json',original);
 for(const kind of ['light-tank','algorithm-tank','laser-tank'])for(const mode of ['stationary','away-10','lateral-10','wall-stationary','away-13-boundary']){
  const label=kind+'-'+mode;
  const save=structuredClone(original),s=save.snapshot;
  for(const key of ['rooms','units','links','walls','entrances','jobs','resources','shipments','projectiles','events','rubble','defenseFields','shieldRegions','networkStores','powerGrids','excavated'])s[key]=[];
  s.excavationOwners={};s.terrain.fill(0);s.tick=0;s.revision=0;s.winner=null;s.winReason='';s.playback=null;
  s.buildings=s.buildings.filter(b=>b.kind==='core');
  for(const b of s.buildings){b.hp=b.maxHp=1e9;b.stock={};b.inventory=0;}
  for(const player of s.players){player.ai=false;player.credits=2000;player.science=0;player.totals={};player.researches=[];player.research=null;player.dominance=0;}
  s.visible=[[],[]];s.explored=[[],[]];
  for(let y=35;y<60;y++)for(let x=32;x<68;x++)for(let owner=0;owner<2;owner++){s.visible[owner].push(pos(x,y));s.explored[owner].push(pos(x,y));}
  const targetX=mode==='away-13-boundary'?53:50;
  const route=mode.startsWith('away')?Array.from({length:8},(_,i)=>pos(targetX+i+1,45)):
   mode==='lateral-10'?Array.from({length:8},(_,i)=>pos(targetX,46+i)):[];
  const def=catalog.units.find(u=>u.id===kind),targetDef=catalog.units.find(u=>u.id==='algorithm-scout');
  s.units=[actor(10001,1,kind,40,45,def),actor(10002,2,targetDef.id,targetX,45,targetDef,route)];
  if(mode==='wall-stationary')s.walls=[{id:10003,owner:2,pos:pos(45,45),kind:'physical',hp:1000,maxHp:1000,shield:0,antiHeal:0,invested:0}];
  save.nextId=20000;save.initialAi=false;save.sequences=[0,0];save.orders=[];save.administrativeEvents=[];
  write(label+'-fixture.json',save);
  const loaded=await rpc('game.session.load',{save});
  await rpc('play.pause');
  let snapshot=await rpc('game.session.snapshot');
  const rows=[trace(loaded.snapshot,'load-response')];
  assert.equal(loaded.snapshot.tick,0);
  assert.ok(snapshot.tick<=1,'Load/pause race skipped more than the first native tick; retain attempt and refuse deterministic trace');
  if(snapshot.tick>0)rows.push(trace(snapshot,'paused-after-load'));
  for(let i=snapshot.tick;i<60;i++){
   const previous=snapshot.tick;
   await rpc('play.step');snapshot=await rpc('game.session.snapshot');
   assert.equal(snapshot.tick,previous+1,'Each private play.step must advance exactly one actual native tick');
   rows.push(trace(snapshot,'actual-native-step'));
  }
  fs.writeFileSync(path.join(out,label+'-ticks.jsonl'),rows.map(row=>JSON.stringify(row)).join('\n')+'\n',{flag:'wx'});
  write(label+'-final-snapshot.json',snapshot);
  const shooter=snapshot.units.find(u=>u.id===10001),target=snapshot.units.find(u=>u.id===10002);
  const first=rows.flatMap(row=>row.projectiles.map(p=>({tick:row.tick,...p}))).find(p=>p.source===10001);
  assert.ok(first,'No actual launch observed');
  assert.equal(shooter.attackCount,1,'The one-second experiment must contain exactly one shot');
  const pathRows=rows.filter(row=>row.projectiles.some(p=>p.id===first.id));
  const gone=rows.find(row=>row.tick>first.tick&&!row.projectiles.some(p=>p.id===first.id));
  const impacts=rows.flatMap(row=>row.events.filter(e=>['projectile-impact','impact','energy-impact','explosive-impact'].includes(e.kind)).map(e=>({observedTick:row.tick,...e})));
  const report={label,kind,mode,sourceRange:def.range,unitBodyHalfWidth:.45,targetSpeed:targetDef.speed,
   initialDistance:targetX-40,firstProjectile:first,lastPresentTick:pathRows.at(-1)?.tick??null,firstAbsentTick:gone?.tick??null,
   targetAtProjectileRemoval:gone?.units.find(u=>u.id===10002)??null,initialTargetHp:targetDef.hp,finalTargetHp:target?.hp??0,
   measuredTargetHpLoss:targetDef.hp-(target?.hp??0),shooterShots:shooter.attackCount,finalWalls:snapshot.walls,impacts,
   ticksRecorded:rows.length,simulatedTicks:snapshot.tick,
   fixture:ref(path.join(out,label+'-fixture.json')),trace:ref(path.join(out,label+'-ticks.jsonl')),finalSnapshot:ref(path.join(out,label+'-final-snapshot.json'))};
  reports.push(report);write(label+'-report.json',report);
  console.log(JSON.stringify({label,launchTick:first.tick,duration:first.duration,aim:first.aimPosition,removedAt:gone?.tick??null,targetHpLoss:report.measuredTargetHpLoss,shooterShots:shooter.attackCount}));
 }
 write('post-probe-native-metrics.json',await rpc('game.session.metrics'));
}catch(failure){error={message:failure.message,stack:failure.stack};write('failure.json',error);throw failure;}
finally{
 await native.close();
 write('probe-receipt.json',{kind:'native-projectile-lifetime-diagnostic',startedAtUtc,completedAtUtc:new Date().toISOString(),actualNativeExecution:true,
  syntheticFixture:true,finalEligible:false,nativePid:native.pid,engineSha256:native.rulesHash,rulesVersion:catalog?.rulesVersion??null,rulesFingerprint:catalog?.rulesFingerprint??null,
  executableUnchanged:sha(executable)===expectedHost,observedMethods:[...methods],gpuOrViewportMethodsRequested:false,error,completedCases:reports.length,reports,
  scope:'Explicit unearned deterministic geometry/weapon fixtures loaded into frozen native FA31/0ac. Real native play.step and snapshots, no GPU methods. This probes projectile lifecycle only; not normal economy, balance matches, LAN or release approval.'});
}
