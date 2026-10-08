/** Exact transport round trips and byte measurements on declared synthetic snapshots.
 * This is not native simulation, GPU or LAN gameplay acceptance. */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
import {NET_PROTOCOL,snapshotEnvelope,applySnapshotEnvelope} from '../multiplayer-v6.mjs';
const events=Array.from({length:4096},(_,i)=>({id:i+1,tick:i*3,kind:'projectile-impact',owner:i%2+1,pos:{x:i%128,y:i%96,z:i%8-2},magnitude:2,subject:i+5000,subjectKind:'arc',presentationPosition:[i%128+.5,i%96+.5,0]}));
const before={version:6,tick:12288,revision:12288,terrain:Array(128*96).fill(0),events,units:[{id:5000,owner:1,x:10.5,y:12.5},{id:5001,owner:2,x:90.5,y:72.5}]};
const next={...before,tick:12291,revision:12291,events:[...events.slice(1),{...events[0],id:4097,tick:12291}],units:[{...before.units[0],x:11},before.units[1]]};
const frozen=JSON.stringify(before),delta=snapshotEnvelope(before,next);
assert.equal(delta.kind,'delta');assert.equal(delta.protocol,NET_PROTOCOL);
assert.equal(delta.collections.events.upsert.length,1);assert.deepEqual(delta.collections.events.remove,[1]);assert.equal(delta.collections.events.order,undefined);
assert.deepEqual(applySnapshotEnvelope(before,delta),next);assert.equal(JSON.stringify(before),frozen);
const fullBytes=Buffer.byteLength(JSON.stringify(next)),deltaBytes=Buffer.byteLength(JSON.stringify(delta));assert.ok(deltaBytes<fullBytes*.05);
const reordered={...next,revision:12292,events:[next.events.at(-1),...next.events.slice(0,-1)]};
const reorderDelta=snapshotEnvelope(next,reordered);assert.ok(reorderDelta.collections.events.order);assert.deepEqual(applySnapshotEnvelope(next,reorderDelta),reordered);
const emptied={...next,revision:12293,events:[]};assert.deepEqual(applySnapshotEnvelope(next,snapshotEnvelope(next,emptied)),emptied);
assert.equal(snapshotEnvelope(before,next,true).kind,'full');
assert.throws(()=>applySnapshotEnvelope({...before,revision:1},delta));
assert.throws(()=>applySnapshotEnvelope(before,{...delta,protocol:'code-sentinels-pvp/6'}));
const corrupt=structuredClone(delta);corrupt.collections.events.upsert.push(corrupt.collections.events.upsert[0]);assert.throws(()=>applySnapshotEnvelope(before,corrupt));
const badOrder=structuredClone(reorderDelta);badOrder.collections.events.order[0]=999999;assert.throws(()=>applySnapshotEnvelope(next,badOrder));
const forbidden=structuredClone(delta);forbidden.collections.players={upsert:[],remove:[]};assert.throws(()=>applySnapshotEnvelope(before,forbidden));
const report={at:new Date().toISOString(),scope:'synthetic protocol fixtures only',nativeAssertionsExecuted:false,protocol:NET_PROTOCOL,checks:8,fullBytes,deltaBytes,ratio:deltaBytes/fullBytes,status:'passed'};
fs.writeFileSync(fileURLToPath(new URL('./collection-delta-tests.json',import.meta.url)),JSON.stringify(report,null,2));
console.log(JSON.stringify(report));
