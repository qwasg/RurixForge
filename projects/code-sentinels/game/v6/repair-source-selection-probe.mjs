/** Routing fixture through the unchanged native authority. No GPU, normal Repair orders/fees. */
import assert from 'node:assert/strict';import fs from 'node:fs';import path from 'node:path';
import {launchNative} from './native-rpc.mjs';
const root=path.resolve(process.argv[2]),exe=path.resolve(process.argv[3]),out=path.resolve(process.argv[4]);
const expectFixed=process.argv.includes('--expect-fixed');assert(!fs.existsSync(out));fs.mkdirSync(out,{recursive:true});
const native=await launchNative({root,executable:exe,logs:path.join(out,'engine')});const rpc=(m,p={})=>native.rpc(m,p);
let report;
try{
 await rpc('game.session.open',{mode:'authority',seed:91,opponent:'human'});await rpc('play.pause');
 const catalog=await rpc('game.session.catalog'),base=await rpc('game.session.save'),s=base.snapshot;
 s.terrain.fill(0);s.jobs=[];s.units=[];s.rooms=[];s.entrances=[];s.links=[];s.walls=[];s.shipments=[];s.projectiles=[];s.networkStores=[];s.powerGrids=[];s.events=[];
 const core=s.buildings.find(b=>b.kind==='core'&&b.owner===1);core.stock.repair=10;
 const shell={...structuredClone(core),id:10000,kind:'shell',rect:{x:13,y:45,z:0,w:6,h:6},power:0,demand:0,powered:false,stock:{},inventory:0,invested:0,supportRatio:1,collapseWarning:0};s.buildings.push(shell);
 for(const [id,x,y]of[[10001,16,48],[10002,14,46]])s.rooms.push({id,shell:10000,owner:1,rect:{x,y,z:0,w:2,h:2},kind:'depot',branch:null,tier:1,hp:280,maxHp:280,antiHeal:0,equipmentShare:1,powered:false,connected:false,online:false,maintenance:0,capacity:1,gpus:[],inventory:10,stock:{repair:10},progress:1,buildTime:1,cooldown:0,invested:0});
 const d=catalog.units.find(u=>u.id==='vscode');const target=10003;
 s.units.push({id:target,owner:1,kind:d.id,pos:{x:21,y:48,z:0},x:21.5,y:48.5,z:0,tier:d.tier,hp:20.8,maxHp:d.hp,battery:0,batteryMax:0,covered:false,wired:false,ammo:d.ammoCapacity,ammoMax:d.ammoCapacity,fuel:d.fuelCapacity,fuelMax:d.fuelCapacity,energy:d.energyCapacity,energyMax:d.energyCapacity,route:[],goal:null,queuedGoals:[],target:null,cooldown:999,skillCooldown:0,plugins:[],statuses:{},invested:d.cost,moving:false,attackCount:0,branch:d.branch,facing:0,altitude:0,flightState:'ground',sourceFacility:0});base.nextId=10004;base.orders=[];base.sequences=[0,0];base.administrativeEvents=[];
 fs.writeFileSync(path.join(out,'fixture.json'),JSON.stringify(base));
 const load=async save=>{await rpc('game.session.load',{save});await rpc('play.pause');return rpc('game.session.snapshot');};
 const values=snap=>({credits:snap.players.find(p=>p.owner===1).credits,far:snap.buildings.find(b=>b.id===core.id).stock.repair,near:snap.rooms.map(r=>r.stock.repair),jobs:snap.jobs.filter(j=>j.kind==='repair')});
 const order={owner:1,sequence:1,command:{op:'repair',id:target}};
 const before=values(await load(base));const primary=await rpc('game.session.order',order);const after=values(await rpc('game.session.snapshot'));
 if(expectFixed){assert(primary.accepted,primary.reason);assert.equal(after.credits,before.credits-20);assert.equal(after.far,before.far-10);assert.deepEqual(after.near,before.near);}else{assert(!primary.accepted&&primary.reason.includes('无可达路线'),JSON.stringify(primary));assert.deepEqual(after,before);}
 const farOnly=structuredClone(base);for(const r of farOnly.snapshot.rooms){r.stock.repair=0;r.inventory=0;}
 const availableBefore=values(await load(farOnly));const accepted=await rpc('game.session.order',order);assert(accepted.accepted,accepted.reason);const availableAfter=values(await rpc('game.session.snapshot'));
 assert.equal(availableAfter.credits,availableBefore.credits-20);assert.equal(availableAfter.far,availableBefore.far-10);assert.equal(availableAfter.jobs.length,1);assert(availableAfter.jobs[0].route.length>0);
 const duplicate=await rpc('game.session.order',order);assert(duplicate.accepted);assert.deepEqual(values(await rpc('game.session.snapshot')),availableAfter);
 const busy=await rpc('game.session.order',{...order,sequence:2});assert(!busy.accepted);assert.deepEqual(values(await rpc('game.session.snapshot')),availableAfter);
 const noneReachable=structuredClone(base);noneReachable.snapshot.buildings.find(b=>b.id===core.id).stock.repair=0;
 const blockedBefore=values(await load(noneReachable));const blocked=await rpc('game.session.order',order);assert(!blocked.accepted&&blocked.reason.includes('无可达路线'));assert.deepEqual(values(await rpc('game.session.snapshot')),blockedBefore);
 report={kind:'native-repair-source-routing-fixture',actualNativeExecution:true,engineSha256:native.rulesHash,rulesVersion:catalog.rulesVersion,rulesFingerprint:catalog.rulesFingerprint,expectFixed,bugReproduced:!expectFixed,scope:'Explicit paused geometry/inventory fixture, not earned economy or gameplay balance. Normal Repair order and sequence validation prove reachable far supply, atomic20credit/10material charge and duplicate/busy/no-route no extra charge.',primary:{before,receipt:primary,after},farOnly:{before:availableBefore,receipt:accepted,after:availableAfter,duplicate,busy},allInaccessible:{before:blockedBefore,receipt:blocked}};
 fs.writeFileSync(path.join(out,'report.json'),JSON.stringify(report,null,2));console.log(JSON.stringify({bugReproduced:report.bugReproduced,primary,farOnly:accepted,allInaccessible:blocked,rulesFingerprint:catalog.rulesFingerprint}));
}finally{await native.close();}
