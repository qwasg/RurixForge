/** Save compatibility boundary tests with an explicit RPC double, not native gameplay. */
import assert from 'node:assert/strict';import fs from 'node:fs';import os from 'node:os';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {createSessionController} from './session-controller.mjs';
const fp='c'.repeat(64),version='v6.2',directory=fs.mkdtempSync(path.join(os.tmpdir(),'sentinels-v6-save-identity-'));
const checks=[];const controllers=[];
function fixture(name,{supported=true,saveFp=fp,failLoad=false}={}) {
 const root=path.join(directory,name);const calls=[];let state={version:6,tick:30,revision:1,seed:42,winner:null};
 const native={rulesHash:'a'.repeat(64),async rpc(method,input={}){calls.push(method);
  if(method==='game.session.catalog')return {rulesVersion:version,...(supported?{rulesFingerprint:fp}:{})};
  if(method==='game.session.open')return {snapshot:state};
  if(method==='game.session.snapshot')return state;
  if(method==='game.session.save')return {rulesVersion:version,rulesFingerprint:saveFp,snapshot:state,orders:[],sequences:[1,0],initialAi:false};
  if(method==='game.session.load'){if(failLoad)throw Error('explicit malformed native payload');return {loaded:true};}
  if(method==='game.session.replay')return {playback:true};return {};
 }};
 const controller=createSessionController({native,root});controllers.push(controller);return {root,calls,controller};
}
try {
 const f=fixture('compatible');await f.controller.session({mode:'solo',seed:42});const saved=await f.controller.save({name:'identity fixture',rulesFingerprint:'forged caller'});await f.controller.leave();
 const file=path.join(f.root,'.forge/save/v6',saved.id+'.json');const original=fs.readFileSync(file,'utf8');const record=JSON.parse(original);
 assert.equal(record.rulesFingerprint,fp);assert.equal(record.save.rulesFingerprint,fp);assert.equal(record.rulesVersion,version);
 assert.equal((await f.controller.saves()).saves[0].compatible,true);checks.push('save identity comes from active native catalog and agrees with native Save, never caller flags');
 for(const mode of ['wrapper-fingerprint','payload-fingerprint','wrapper-version','payload-version','legacy']) {
  const altered=JSON.parse(original);
  if(mode==='wrapper-fingerprint')altered.rulesFingerprint='d'.repeat(64);
  if(mode==='payload-fingerprint')altered.save.rulesFingerprint='d'.repeat(64);
  if(mode==='wrapper-version')altered.rulesVersion='v6.1';
  if(mode==='payload-version')altered.save.rulesVersion='v6.1';
  if(mode==='legacy'){delete altered.rulesVersion;delete altered.rulesFingerprint;delete altered.save.rulesVersion;delete altered.save.rulesFingerprint;}
  const bytes=JSON.stringify(altered);fs.writeFileSync(file,bytes);const before=f.calls.filter(c=>c==='game.session.open').length;
  const entry=(await f.controller.saves()).saves[0];assert.equal(entry.compatible,false);assert.equal(entry.compatibility,mode==='legacy'?'legacy-unverified':'incompatible');
  for(const replay of [false,true])await assert.rejects(()=>f.controller.load({id:saved.id,allowLegacy:true,force:true,rulesVersion:version,rulesFingerprint:fp},replay),e=>e.status===409);
  assert.equal(f.calls.filter(c=>c==='game.session.open').length,before);assert.equal(fs.readFileSync(file,'utf8'),bytes,'old save was rewritten');
 }
 checks.push('mismatched outer/inner rules and legacy files reject load/replay before native open, without rewriting old files or honoring bypass flags');
 fs.writeFileSync(file,original);await f.controller.load({id:saved.id});await f.controller.leave();await f.controller.load({id:saved.id},true);await f.controller.leave();checks.push('matching identity permits load and real replay dispatch');
 const unsupported=fixture('unsupported',{supported:false});await unsupported.controller.session({mode:'solo',seed:42});
 await assert.rejects(()=>unsupported.controller.save({rulesVersion:version,rulesFingerprint:fp}),e=>e.status===409);assert.equal(fs.readdirSync(path.join(unsupported.root,'.forge/save/v6')).length,0);await unsupported.controller.leave();
 checks.push('an engine without a trusted rules fingerprint cannot mint verified saves');
 const wrong=fixture('wrong-native',{saveFp:'d'.repeat(64)});await wrong.controller.session({mode:'solo',seed:42});await assert.rejects(()=>wrong.controller.save(),e=>e.status===409);assert.equal(fs.readdirSync(path.join(wrong.root,'.forge/save/v6')).length,0);await wrong.controller.leave();checks.push('inconsistent native save output is rejected before disk writes');
 const malformed=fixture('malformed',{failLoad:true});const destination=path.join(malformed.root,'.forge/save/v6',saved.id+'.json');fs.writeFileSync(destination,original);
 await assert.rejects(()=>malformed.controller.load({id:saved.id}),/malformed native/);assert.equal(malformed.calls.at(-1),'game.session.close');checks.push('native payload rejection closes the temporary session rather than leaving an orphan authority');
 const report={testedAt:new Date().toISOString(),passed:true,nativeGameplayTested:false,scope:'Save compatibility and lifecycle boundaries using an explicit native RPC double',checks};
 fs.writeFileSync(path.join(path.dirname(fileURLToPath(import.meta.url)),'save-identity-tests.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
} finally {for(const controller of controllers)await controller.leave();}
