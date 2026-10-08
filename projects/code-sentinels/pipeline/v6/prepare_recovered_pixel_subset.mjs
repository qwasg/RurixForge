/** Audit preserved evidence and export only the predeclared representative inputs. */
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import zlib from 'node:zlib';
import {hash,decodeEvidencePng} from './frozen_pixel_cases.mjs';
const project=path.resolve('projects/code-sentinels');
const original=path.join(project,'pipeline/v6/pixel-cases-64c-20260912/media-pixel-baseline-64c-20260912/cases.json');
const capture=path.join(project,'Logs/v6/media-pixel-baseline-64c-20260912/capture-report.json');
const output=path.join(project,'pipeline/v6/pixel-recovery-20260912-2300');
assert.ok(!fs.existsSync(output),'Preserve every prior recovery output');
const bytes=fs.readFileSync(original),source=JSON.parse(bytes),captureBytes=fs.readFileSync(capture),record=JSON.parse(captureBytes);
assert.equal(source.cases.length,456);assert.equal(record.captures.length,456);assert.equal(record.captured,true);assert.deepEqual(record.errors,[]);
const records=new Map(record.captures.map(c=>[c.file.replace(/\.png$/,''),c]));
const selectedIds=new Set(['baseline']);
for(const character of ['gemini','claude','kimi','minimax','glm','deepseek','gpt']){
 const metadata=JSON.parse(fs.readFileSync(path.join(project,'Content/Animations/v6/characters',character+'.json')));
 for(const action of ['idle','walk','attack','cast','hit','death']){
  const clip=metadata.clips[action].s,count=clip.endExclusive-clip.start;
  for(const frame of new Set([0,Math.floor(count/2),count-2,count-1]))selectedIds.add(`${character}-${action}-${String(frame).padStart(2,'0')}`);
 }
 selectedIds.add(character+'-death-expired');
}
const verified=[];
for(const c of source.cases){
 assert.equal(c.width,1920);assert.equal(c.height,1080);assert.equal(c.stability.exact,true);assert.equal(c.baselineRgbaSha256,c.repeatedRgbaSha256);
 const pngBytes=fs.readFileSync(c.baselinePng),png=decodeEvidencePng(pngBytes),compressed=fs.readFileSync(c.snapshotPath),raw=zlib.gunzipSync(compressed);
 assert.equal(hash(pngBytes),c.baselinePngSha256);assert.equal(hash(png.raw),c.baselineRgbaSha256);assert.equal(hash(compressed),c.compressedSnapshotSha256);assert.equal(hash(raw),c.snapshotSha256);
 assert.equal(records.get(c.id).rgbaSha256,c.baselineRgbaSha256);assert.equal(records.get(c.id).pngSha256,c.baselinePngSha256);
 verified.push({id:c.id,baselinePng:c.baselinePng,pngSha256:c.baselinePngSha256,rgbaSha256:c.baselineRgbaSha256,snapshotSha256:c.snapshotSha256,selected:selectedIds.has(c.id)});
}
const cases=source.cases.filter(c=>selectedIds.has(c.id));assert.equal(cases.length,176);assert.equal(selectedIds.size,176);
const uniqueFrames=new Set(cases.flatMap(c=>c.expected.filter(e=>Number.isInteger(e.frame)).map(e=>`${e.character}/${e.action}/${e.direction}/${e.frame}`)));
assert.equal(uniqueFrames.size,1344);
fs.mkdirSync(output,{recursive:true});
const subset={...source,scope:'176 representative exact original fixed inputs; seven operators, six actions, eight directions, first/middle/penultimate/last source frames plus baseline and seven expired deaths. 280 additional preserved baseline cases are deliberately not compared.',recoveredFrom:{path:original,sha256:hash(bytes)},cases};
fs.writeFileSync(path.join(output,'character-inputs.json'),JSON.stringify(subset,null,2));
const report={schemaVersion:1,at:new Date().toISOString(),scope:'CPU verification of original immutable screenshot and snapshot bytes; no new GPU or performance execution.',baselineEngine:source.engineHash,original:{path:original,sha256:hash(bytes)},captureReport:{path:capture,sha256:hash(captureBytes)},verifiedCases:verified.length,selectedCases:cases.length,uniqueSelectedCharacterFrames:uniqueFrames.size,dimensions:[1920,1080],uncomparedBaselineCases:456-176,verified};
fs.writeFileSync(path.join(output,'recovery-audit.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({...report,verified:verified.length,inputs:path.join(output,'character-inputs.json')}));
