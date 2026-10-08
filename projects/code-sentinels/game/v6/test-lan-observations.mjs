// Isolated timing/canonicalization tests, not runtime acceptance evidence.
import assert from 'node:assert/strict';
import fs from 'node:fs';import os from 'node:os';import path from 'node:path';
import {ConservativeActivity,EpochMovement,PostActivityMetrics,retainTimedSample,stateHash} from './lan-observations.mjs';
const a=new ConservativeActivity(),s=(t,winner=null)=>[{tick:t,winner},{tick:t,winner}];
assert.equal(a.observe(1,0,s(1)),0);assert.equal(a.observe(1,10000,s(601)),10000);
assert.equal(a.observe(1,20000,s(900,1)),0);assert.equal(a.observe(1,60000,s(900,1)),0);
assert.equal(a.observe(2,70000,s(1)),0);assert.equal(a.observe(2,80000,s(601)),10000);
assert.equal(a.observe(2,90000,s(601)),0);assert.equal(a.observe(2,100000,s(1201),false),0);
assert.equal(a.observe(2,110000,s(1801)),0);assert.equal(a.observe(2,120000,s(2401)),10000);
assert.equal(a.seconds,30);a.reset();assert.equal(a.observe(2,130000,s(3001)),0);
assert.equal(stateHash({b:[2,1],a:{x:3}}),stateHash({a:{x:3},b:[2,1]}));
assert.notEqual(stateHash({b:[2,1]}),stateHash({b:[1,2]}));
const m=new EpochMovement();m.begin('first');assert.equal(m.observe(7,[1,1,0]),null);assert.ok(m.observe(7,[2,1,0]));assert.equal(m.moved.size,1);
m.begin('second');assert.equal(m.current.size,0);assert.equal(m.observe(7,[100,90,0]),null,'new epoch reused ID is a new spawn, not movement');assert.equal(m.moved.size,1);
assert.ok(m.observe(7,[101,90,0]));assert.equal(m.moved.size,2);assert.ok(m.current.has(7));
console.log('Conservative activity: terminal edge, terminal idle, epoch gap, stalled replica, outage boundaries, reset and canonical hashes passed.');
const delayed=new ConservativeActivity();delayed.observe(1,0,s(1));
// The actual outage starts at2s, ends at10s and recovery ends at13s. No
// observation is available until15s, so both observed endpoint states are stable.
delayed.reset(13000);assert.equal(delayed.observe(1,15000,s(901)),0,'a whole outage hidden between delayed samples must never count');
assert.equal(delayed.observe(1,25000,s(1501)),10000);
delayed.reset(38000);assert.equal(delayed.observe(1,32000,s(1901)),0,'recovery is excluded even if a caller reports stable');
assert.equal(delayed.observe(1,39000,s(2401)),0,'the recovery boundary cannot bridge two samples');
assert.equal(delayed.observe(1,49000,s(3001)),10000);assert.equal(delayed.seconds,20);
const sampleEvents=[],rawStates=[{snapshot:{tick:100,owner:1}},{snapshot:{tick:1,owner:2}}],badSample={sampleId:7,lag:99,authorityTicksPerSecond:56.7};
const sampleFixtureDirectory=fs.mkdtempSync(path.join(os.tmpdir(),'v6-lan-timed-sample-fixture-'));let fixtureIndex=0;
for(const message of ['lag failed','owner view failed','recovery failed','tick rate failed']){
 const original=new Error(message),sampleFile=path.join(sampleFixtureDirectory,`${++fixtureIndex}-sample.json`),statesFile=path.join(sampleFixtureDirectory,`${fixtureIndex}-states.json`);let retained;
 await assert.rejects(retainTimedSample(badSample,rawStates,{
  persistSample:row=>{fs.writeFileSync(sampleFile,JSON.stringify(row),{flag:'wx'});sampleEvents.push('write');},
  validate:()=>{assert.deepEqual(JSON.parse(fs.readFileSync(sampleFile)),badSample,'failed window must already be written before validation');sampleEvents.push('validate');throw original;},
  persistFailure:value=>{retained=value;fs.writeFileSync(statesFile,JSON.stringify(value.states),{flag:'wx'});sampleEvents.push('raw-failure');}
 }),error=>error===original);
 assert.equal(retained.states,rawStates,'retain the exact sampled states instead of a later status request');assert.equal(retained.sample,badSample);assert.equal(retained.error,original);
 assert.deepEqual(JSON.parse(fs.readFileSync(statesFile)),rawStates);
}
assert.deepEqual(sampleEvents,Array(4).fill(['write','validate','raw-failure']).flat());
let passingWrites=0;await retainTimedSample({sampleId:8},rawStates,{persistSample:()=>passingWrites++,validate:()=>{},persistFailure:()=>assert.fail('passing sample must not become a failed-state observation')});assert.equal(passingWrites,1);
const assertionFailure=new Error('original threshold failed');await assert.rejects(retainTimedSample(badSample,rawStates,{persistSample:()=>{},validate:()=>{throw assertionFailure;},persistFailure:()=>{throw new Error('diagnostic storage unavailable');}}),error=>error===assertionFailure&&error.evidenceWriteError==='diagnostic storage unavailable');
console.log('Timing boundaries and failed samples: delayed whole outage and recovery excluded; lag/owner/recovery/tick failures persist timed rows first and retain exact states; diagnostic write errors preserve original assertion.');
console.log(JSON.stringify({scope:'Synthetic unit-test files only, not native LAN acceptance',sampleFixtureDirectory}));
const rpcCalls=[],writes=[];
const diagnostics=new PostActivityMetrics();
const clients=['blue','red'].map((name,i)=>({name,health:{enginePid:101+i},identity:{rulesFingerprint:'fixture-only'},native:{async rpc(method,params,timeout){rpcCalls.push({name,method,params,timeout});return{tick:60,renderStagesByLayer:{0:{prepareUploads:{p99:2}}}};}}}));
const write=(name,value)=>writes.push({name,value});
assert.equal((await diagnostics.collect(clients,write)).status,'skipped-active');
assert.equal(rpcCalls.length,0,'no native calls within the measured activity window');assert.equal(writes.length,0);
diagnostics.end('activity-target-reached');
const [first,second]=await Promise.all([diagnostics.collect(clients,write),diagnostics.collect(clients,write)]);
assert.deepEqual(second,first);assert.equal(rpcCalls.length,2,'one metrics call per actual client, including concurrent collection');assert.equal(writes.length,2);
assert.ok(rpcCalls.every(call=>call.method==='game.session.metrics'&&call.timeout===10000));
diagnostics.end('later-replay-failure');await diagnostics.collect(clients,write);assert.equal(rpcCalls.length,2);assert.equal(first.ended.reason,'activity-target-reached');
const failed=new PostActivityMetrics(),originalFailure=new Error('original FPS threshold failed');let preservedFailure=originalFailure;
failed.end('failure-or-stop');
const failureResult=await failed.collect([{name:'unavailable'},{name:'timeout',native:{async rpc(){throw new Error('metrics timeout');}}}],()=>{throw new Error('disk diagnostic failure');}).catch(error=>{preservedFailure=error;});
assert.equal(preservedFailure,originalFailure,'optional collection never replaces the original failure');
assert.equal(failureResult.clients[0].error,'native observer unavailable');assert.equal(failureResult.clients[1].error,'metrics timeout');assert.ok(failureResult.clients.every(row=>row.storageError==='disk diagnostic failure'));
console.log('Post-activity diagnostics: active zero calls; completed one read per native; concurrent/repeated collection deduplicated; RPC/storage errors preserve original failure.');
