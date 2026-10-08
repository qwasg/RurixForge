/** Fixed-input old/new rendering capture. Fresh replica clears ghosts/interpolation per case. */
import fs from 'node:fs';import path from 'node:path';import zlib from 'node:zlib';import assert from 'node:assert/strict';
import {launchNative} from '../../game/v6/native-rpc.mjs';
import {hash,encodePng,compareRgba} from './frozen_pixel_cases.mjs';
const arg=name=>{const i=process.argv.indexOf(name);assert.ok(i>=0&&process.argv[i+1],'Missing '+name);return process.argv[i+1];};
const project=path.resolve('projects/code-sentinels'),input=path.resolve(arg('--cases')),out=path.resolve(arg('--out')),executable=path.resolve(arg('--engine')),expected=arg('--engine-hash');
assert.ok(!fs.existsSync(out),'Preserve prior fixed capture');assert.equal(hash(fs.readFileSync(executable)),expected);const inputBytes=fs.readFileSync(input),source=JSON.parse(inputBytes);assert.equal(source.schemaVersion,1);assert.ok(source.cases.length>0);fs.mkdirSync(out,{recursive:true});
const report={schemaVersion:1,kind:'frozen-native-rgba-cases',scope:'Actual native fixed-input visual fixture only; no ordinary-command gameplay, balance or performance result.',engineHash:expected,sourceManifest:{path:input,sha256:hash(inputBytes)},wallTimePolicy:'Close/open a fresh replica for every case; playback.paused=true and winner=null fix animation time. No play.pause. Compare a second actual RGBA readback150ms later to detect residual wall-time effects.',resourceManifestSha256:hash(fs.readFileSync(path.join(project,'Content/UI/v6/resource-manifest.json'))),runtimeIndexSha256:hash(fs.readFileSync(path.join(project,'Content/Animations/v6/runtime-frames/index.json'))),cases:[],errors:[],startedAt:new Date().toISOString()};
const native=await launchNative({root:project,executable,logs:path.join(out,'engine')});report.pid=native.pid;
const rpc=(m,p={},timeout=120000)=>native.rpc(m,p,timeout);const save=()=>fs.writeFileSync(path.join(out,'cases.json'),JSON.stringify(report,null,2));
async function frame(c){const f=await rpc('viewport.frame',{width:c.width,height:c.height,format:'rgba8'});assert.equal(f.width,c.width);assert.equal(f.height,c.height);assert.equal(f.meshFallbacks,0);assert.equal(f.truncated,false);const raw=Buffer.from(f.pixelsB64,'base64');assert.equal(raw.length,c.width*c.height*4);delete f.pixelsB64;return{raw,diagnostics:f};}
try{
 const catalog=await rpc('game.session.catalog');report.rulesVersion=catalog.rulesVersion;report.rulesFingerprint=catalog.rulesFingerprint;
 for(const c of source.cases){assert.match(c.id,/^[a-z0-9-]+$/);const compressed=fs.readFileSync(c.snapshotPath);assert.equal(hash(compressed),c.compressedSnapshotSha256);const rawInput=zlib.gunzipSync(compressed);assert.equal(hash(rawInput),c.snapshotSha256);const snapshot=JSON.parse(rawInput);assert.equal(snapshot.tick,c.tick);assert.equal(snapshot.winner,null);assert.equal(snapshot.playback?.paused,true);
  await rpc('game.session.close');await rpc('game.session.open',{mode:'replica',seed:snapshot.seed,theme:snapshot.theme,localPlayer:c.view.localPlayer,opponent:'human'});await rpc('game.session.applySnapshot',{snapshot});await rpc('game.session.view',c.view);
  const first=await frame(c);await new Promise(r=>setTimeout(r,150));const second=await frame(c),stability=compareRgba(first.raw,second.raw,c.width,c.height),png=path.join(out,c.id+'.png');fs.writeFileSync(png,encodePng(first.raw,c.width,c.height));
  if(!stability.exact)fs.writeFileSync(path.join(out,c.id+'-unstable-repeat.png'),encodePng(second.raw,c.width,c.height));
  report.cases.push({...c,baselinePng:png,baselinePngSha256:hash(fs.readFileSync(png)),baselineRgbaSha256:hash(first.raw),repeatedRgbaSha256:hash(second.raw),stability,diagnostics:first.diagnostics});save();assert.ok(stability.exact,'Wall-time drift in '+c.id);console.log(JSON.stringify({case:c.id,captured:report.cases.length,stable:true}));
 }
 report.completed=true;report.completedAt=new Date().toISOString();
}catch(e){report.errors.push(String(e.stack??e));process.exitCode=1;console.error(e);}finally{save();await native.close();}
