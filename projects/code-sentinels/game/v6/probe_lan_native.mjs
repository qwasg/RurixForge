/** Two independent real native processes through real local bridges and LAN HTTP/SSE.
 * This short probe validates integration only; the 45-minute combat soak is separate.
 */
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';import {fileURLToPath} from 'node:url';import {createHash} from 'node:crypto';
const here=path.dirname(fileURLToPath(import.meta.url)),project=path.resolve(here,'../..'),repo=path.resolve(project,'../..');
const engine=path.resolve(process.argv[2]||path.join(here,'ui-runtime/engine-host.exe'));
const children=[],checks=[];const sleep=ms=>new Promise(r=>setTimeout(r,ms));
const call=async(base,route,input)=>{const response=await fetch(base+'/api/v6/'+route,{method:input===undefined?'GET':'POST',headers:input===undefined?{}:{'content-type':'application/json'},body:input===undefined?undefined:JSON.stringify(input),signal:AbortSignal.timeout(30000)});const value=await response.json();if(!response.ok)throw Object.assign(new Error(JSON.stringify(value)),{status:response.status});return value;};
async function bridge(){
 const child=spawn(process.execPath,[path.join(project,'game/portable-bridge-v6.mjs'),'--root',project,'--web',path.join(repo,'packages/client/dist'),'--engine',engine,'--no-open','--control-stdin'],{cwd:repo,windowsHide:true,stdio:['pipe','pipe','pipe']});
 children.push(child);let output='';return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(new Error('bridge startup timeout')),60000);child.on('error',reject);child.on('exit',code=>{if(code)reject(new Error('bridge exit '+code));});child.stderr.on('data',data=>{output+=data;});child.stdout.on('data',data=>{output+=data;const found=/Code Sentinels V6 is ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(output);if(found){clearTimeout(timer);resolve({base:found[1],child});}});});
}
async function waitFor(fn,description,ms=15000){const until=Date.now()+ms;while(Date.now()<until){const v=await fn();if(v)return v;await sleep(100);}throw new Error('Timeout: '+description);}
let blue,red,failure;
try{
 [blue,red]=await Promise.all([bridge(),bridge()]);assert.notEqual(blue.child.pid,red.child.pid);
 const host=await call(blue.base,'session',{mode:'host',nickname:'native-blue',seed:71});assert.equal(host.playerId,1);
 const lanPort=new URL('http://'+host.session.address).port;
 const guest=await call(red.base,'session',{mode:'join',nickname:'native-red',address:'127.0.0.1:'+lanPort,code:host.session.code});assert.equal(guest.playerId,2);
 await Promise.all([call(blue.base,'ready',{ready:true}),call(red.base,'ready',{ready:true})]);
 const started=await call(blue.base,'start',{});assert.equal(started.session.status,'battle');
 const replica=await waitFor(async()=>{const s=await call(red.base,'status');return s.snapshot?s:null;},'replica opening and first full snapshot');
 assert.equal(replica.snapshot.seed,71);assert.ok(replica.snapshot.tick>0);checks.push({case:'two real native processes create/join/ready/start and replica receives full snapshot',bluePid:blue.child.pid,redPid:red.child.pid,tick:replica.snapshot.tick});
 const first=await call(blue.base,'order',{seq:1,command:{op:'build',kind:'wind-power',pos:{x:5,y:42,z:0}}});assert.equal(first.accepted,true,first.reason);
 const duplicate=await call(blue.base,'order',{seq:1,command:{op:'build',kind:'wind-power',pos:{x:5,y:42,z:0}}});assert.deepEqual(first,duplicate);
 const redBuild=await call(red.base,'order',{seq:1,command:{op:'build',kind:'wind-power',pos:{x:113,y:42,z:0}}});assert.equal(redBuild.accepted,true,redBuild.reason);
 const blueState=await call(blue.base,'status');const blueWind=blueState.snapshot.buildings.find(b=>b.kind==='wind-power'&&b.owner===1);assert.ok(blueWind);
 const foreign=await call(red.base,'order',{seq:2,command:{op:'repair',id:blueWind.id}});assert.equal(foreign.accepted,false);
 await assert.rejects(()=>call(red.base,'order',{seq:3,command:{op:'shield',owner:1,enabled:false}}),e=>e.status===400);
 checks.push({case:'both owners spend through native ordinary build; native rejects enemy repair; duplicate has one receipt',blueReceipt:first,redReceipt:redBuild,ownershipReceipt:foreign});
 const samples=[];for(let i=0;i<40;i++){const s=await call(red.base,'status');samples.push({at:performance.now(),tick:s.snapshot.tick,revision:s.snapshot.revision});await sleep(50);}
 const uniqueTicks=new Set(samples.map(s=>s.tick)).size;assert.ok(uniqueTicks>=20,'20Hz transport must actually advance');
 const ticks=samples.at(-1).tick-samples[0].tick,seconds=(samples.at(-1).at-samples[0].at)/1000;assert.ok(ticks/seconds>45&&ticks/seconds<75,`native tick rate ${ticks/seconds}`);
 const rs=await call(red.base,'status');assert.ok(!rs.snapshot.buildings.some(b=>b.id===blueWind.id),'unseen hostile structure leaked');
 checks.push({case:'native fixed tick and real incremental stream observed; enemy fog filtered',uniqueTicks,samples:samples.length,ticksPerSecond:ticks/seconds});
 const saved=await call(blue.base,'save',{name:'two-native LAN interface probe'});assert.ok(saved.id);checks.push({case:'native authority full save through host',id:saved.id,tick:saved.tick});
 await call(red.base,'leave',{});const victory=await waitFor(async()=>{const s=await call(blue.base,'status');return s.snapshot?.winner?s:null;},'native forfeit settlement');assert.equal(victory.snapshot.winner,1);checks.push({case:'leaving guest causes real native authority winner',winner:victory.snapshot.winner,reason:victory.snapshot.winReason});
 const report={testedAt:new Date().toISOString(),passed:true,scope:'Short actual native LAN integration; no 45-minute combat, final media, performance or balance claim.',engineSha256:createHash('sha256').update(fs.readFileSync(engine)).digest('hex'),checks};
 fs.writeFileSync(path.join(here,'lan-interface-probe.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}catch(error){failure=error;fs.writeFileSync(path.join(here,'lan-interface-probe-failure.json'),JSON.stringify({at:new Date().toISOString(),message:error.message,stack:error.stack,checks},null,2));console.error(error);}
finally{
 for(const item of[red,blue])if(item)await call(item.base,'leave',{}).catch(()=>{});
 for(const child of children){if(child.exitCode===null){child.stdin.end('shutdown\n');await Promise.race([new Promise(r=>child.once('exit',r)),sleep(10000)]);}}
}
if(failure)process.exitCode=1;
