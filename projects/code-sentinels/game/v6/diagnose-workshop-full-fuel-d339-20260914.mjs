/** Bounded native continuation of unchanged real saves. Not matrix or release evidence. */
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
import {launchNative} from './native-rpc.mjs';

const project=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..');
const out=path.join(project,'game/v6/workshop-full-fuel-d339-20260914-a');
assert.ok(!fs.existsSync(out),'Preserve previous diagnostic');fs.mkdirSync(out);
const root=path.join(project,'dist/final-candidate-d33999c4-20260914/CodeSentinels-V6-Windows');
const executable=path.join(project,'game/v6/native-room-d33999c4-20260914-a/runtime/engine-host.exe');
const expectedH='1b1738b72e8f2533af60cc058b11894c6e47536f4acfe278b675ea010f21d2bf';
const expectedR='d33999c4bf39fea8de5f89df32e2a59c3fc2d242ba2a433078f347c6779f0d35';
const hash=file=>createHash('sha256').update(fs.readFileSync(file)).digest('hex');
const write=(name,value)=>fs.writeFileSync(path.join(out,name),JSON.stringify(value,null,2)+'\n',{flag:'wx'});
const ref=file=>({path:path.relative(project,file).replaceAll('\\','/'),sha256:hash(file)});
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
assert.equal(hash(executable),expectedH);
const inputs=[399,878,916,783].map(index=>{
 const file=path.join(project,`game/v6/final-balance-d33999c4-20260914/branch/diagnostics/match-${index}.save.json`);
 const save=JSON.parse(fs.readFileSync(file,'utf8'));assert.equal(save.rulesFingerprint,expectedR);
 return{index,file,save,reference:ref(file)};
});
write('input-manifest.json',{recordedAtUtc:new Date().toISOString(),expectedH,expectedR,inputs:inputs.map(x=>({index:x.index,...x.reference,initialAi:x.save.initialAi,tick:x.save.snapshot.tick})),driver:ref(fileURLToPath(import.meta.url)),scope:'Unmodified real d339 saves. Baseline short native natural-clock continuation; no external matrix planner is resumed. One paired reload uses a normal paid Supply order. No GPU subscription, resource injection, altered save, production edit, or full-match acceptance.'});
let native,failed=null;const results=[];const startedAtUtc=new Date().toISOString();
function observation(snapshot,initialTick){
 const rooms=snapshot.rooms.filter(r=>['ammunition-workshop','depot','repair-bay'].includes(r.kind));
 return {tick:snapshot.tick,elapsedSimulationSeconds:(snapshot.tick-initialTick)/60,winner:snapshot.winner,
  players:snapshot.players.map(p=>({owner:p.owner,credits:p.credits,totals:p.totals})),
  workshops:rooms.filter(r=>r.kind==='ammunition-workshop').map(r=>({...r,totalStock:Object.values(r.stock).reduce((a,b)=>a+b,0)})),
  warehouses:[...rooms.filter(r=>r.kind!=='ammunition-workshop'),...snapshot.buildings.filter(b=>b.kind==='core')],
  shipments:snapshot.shipments,
  units:snapshot.units.map(u=>({id:u.id,owner:u.owner,kind:u.kind,hp:u.hp,maxHp:u.maxHp,ammo:u.ammo,ammoMax:u.ammoMax,fuel:u.fuel,fuelMax:u.fuelMax,pos:u.pos,moving:u.moving,goal:u.goal})),
  jobs:snapshot.jobs,
  events:snapshot.events.filter(e=>e.tick>=initialTick&&/supply|transport|repair|ammo|fuel/.test(e.kind))};
}
async function continuation(input,name,seconds,withSupply=false){
 const dir=path.join(out,name);fs.mkdirSync(dir);
 const raw=(file,value)=>fs.writeFileSync(path.join(dir,file),JSON.stringify(value)+'\n',{flag:'wx'});
 const loaded=await native.rpc('game.session.load',{save:input.save});
 assert.equal(loaded.snapshot.tick,input.save.snapshot.tick);raw('loaded-native.json',loaded);
 const initialTick=loaded.snapshot.tick,wallStarted=performance.now();let latest=loaded.snapshot;
 let order=null;
 if(withSupply){
  const workshop=latest.rooms.find(r=>r.owner===1&&r.kind==='ammunition-workshop');
  const depot=latest.rooms.find(r=>r.owner===1&&r.kind==='depot'&&Object.values(r.stock).reduce((a,b)=>a+b,0)===0);
  assert.ok(workshop&&depot);assert.equal(workshop.stock.fuel,40);
  const command={op:'supply',from:workshop.id,to:depot.id,amount:40,cargo:'fuel',mode:'ground'};
  const preview=await native.rpc('game.session.preview',{owner:1,command});raw('supply-preview.json',preview);
  order={owner:1,sequence:input.save.sequences[0]+1,command};raw('supply-order.json',order);
  const before=await native.rpc('game.session.save');raw('before-supply-native-save.json',before);
  const receipt=await native.rpc('game.session.order',order);raw('supply-receipt.json',receipt);assert.equal(receipt.accepted,true);
  const after=await native.rpc('game.session.save');raw('after-supply-native-save.json',after);
  latest=after.snapshot;
 }
 const samples=[];
 while(true){
  const row=observation(latest,initialTick);row.observedAtUtc=new Date().toISOString();row.elapsedWallSeconds=(performance.now()-wallStarted)/1000;
  samples.push(row);fs.appendFileSync(path.join(dir,'native-observations.jsonl'),JSON.stringify(row)+'\n');
  if(latest.tick-initialTick>=seconds*60||latest.winner!==null)break;
  assert.ok(performance.now()-wallStarted<(seconds+30)*1000,'Bounded diagnostic did not advance');
  await sleep(withSupply?250:1000);latest=await native.rpc('game.session.snapshot');
 }
 const final=await native.rpc('game.session.save');raw('final-native-save.json',final);
 await native.rpc('game.session.close');
 const result={case:input.index,name,initialTick,finalTick:final.snapshot.tick,requestedDiagnosticSeconds:seconds,observedSimulationSeconds:(final.snapshot.tick-initialTick)/60,observedWallSeconds:(performance.now()-wallStarted)/1000,samples:samples.length,ordinarySupplyOrder:order,input:input.reference,outputs:['loaded-native.json','native-observations.jsonl','final-native-save.json',...(withSupply?['supply-preview.json','supply-order.json','supply-receipt.json','before-supply-native-save.json','after-supply-native-save.json']:[])].map(n=>ref(path.join(dir,n))),scope:'Bounded continuation only; no winner or balance acceptance inferred.'};
 results.push(result);write(name+'-receipt.json',result);console.log(JSON.stringify({milestone:'case-complete',...result,outputs:undefined}));
}
try{
 native=await launchNative({root,executable,logs:path.join(out,'native-process')});
 const catalog=await native.rpc('game.session.catalog');assert.equal(catalog.rulesFingerprint,expectedR);write('actual-native-catalog.json',catalog);
 write('process-start.json',{startedAtUtc,nativePid:native.pid,driverPid:process.pid,engine:ref(executable),rulesFingerprint:catalog.rulesFingerprint,nativePort:native.port,gpuSubscribed:false});console.log(JSON.stringify({milestone:'native-started',nativePid:native.pid,driverPid:process.pid}));
 for(const input of inputs)await continuation(input,'baseline-'+input.index,18);
 await continuation(inputs[0],'paid-fuel-transfer-399',30,true);
}catch(error){failed={message:error.message,stack:error.stack};write('failure.json',failed);console.error(error);process.exitCode=1;}
finally{
 if(native)await native.close();
 for(const input of inputs)assert.equal(hash(input.file),input.reference.sha256,'Original save changed');
 assert.equal(hash(executable),expectedH);
 write('completion.json',{startedAtUtc,completedAtUtc:new Date().toISOString(),nativePid:native?.pid??null,driverPid:process.pid,exitCode:failed?1:0,failed,inputsUnchanged:true,engineUnchanged:true,rulesFingerprint:expectedR,results,finalEligible:false,scope:'Diagnostic baseline and ordinary paid control only. Native helper closed; no GPU, LAN, production change or original-matrix row change.'});
}
