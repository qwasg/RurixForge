/** Sequential GPU-only visual QA continuation; never reuses or overwrites a run. */
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {hash} from './frozen_pixel_cases.mjs';
const project=path.resolve('projects/code-sentinels');
const oldHash='64c3188e738a4a1a3b84eccb04d974cbd54b87ee7491329295a179b40e2d60b9',newHash='4bf9973cb6bf0618e75e0fb3f9f494c9612aeed44a7ebe180f5dee4e6ecf5ef9';
const oldExe=path.join(project,'game/v6/runtime-64c3188e-pixel-baseline-20260912/engine-host.exe'),newExe=path.join(project,'game/v6/runtime-bin/engine-host.exe');
const receiptDir=path.join(project,'pipeline/v6/pixel-recovery-20260912-2300/remaining-execution-resume-fx61');
assert.ok(!fs.existsSync(receiptDir),'Preserve original execution receipts');
for(const [file,expected] of [[oldExe,oldHash],[newExe,newHash]])assert.equal(hash(fs.readFileSync(file)),expected);
const charReport=JSON.parse(fs.readFileSync(path.join(project,'Logs/v6/media-pixel-candidate-4bf-20260912/cases.json')));
assert.equal(charReport.completed,true);assert.equal(charReport.cases.length,176);assert.deepEqual(charReport.errors,[]);
fs.mkdirSync(receiptDir,{recursive:true});
const execution={schemaVersion:1,scope:'Sequential fixed visual fixtures only. Exclusive native GPU processes; no FPS or gameplay claim.',startedAt:new Date().toISOString(),engineHashes:[oldHash,newHash],stages:[]};
execution.previousExecution={path:path.join(project,'pipeline/v6/pixel-recovery-20260912-2300/remaining-execution/execution.json'),reason:'Original native FX capture completed61 cases with exit0/errors[]. Planned55 count was incorrect because two groups mix32 and48 source-frame clips. Preserve original count assertion failure and all61 original captures; do not repeat or omit those frames.'};
execution.previousExecution.sha256=hash(fs.readFileSync(execution.previousExecution.path));
execution.script={path:process.argv[1],sha256:hash(fs.readFileSync(process.argv[1]))};
const persist=()=>fs.writeFileSync(path.join(receiptDir,'execution.json'),JSON.stringify(execution,null,2));
async function run(id,args,expectedCases,reportPath){
 const log=path.join(receiptDir,id+'.log'),fd=fs.openSync(log,'wx'),stage={id,command:[process.execPath,...args],startedAt:new Date().toISOString(),expectedCases,reportPath,log};execution.stages.push(stage);persist();
 let code;
 try {code=await new Promise((resolve,reject)=>{const child=spawn(process.execPath,args,{cwd:process.cwd(),windowsHide:true,stdio:['ignore',fd,fd]});stage.pid=child.pid;persist();child.once('error',reject);child.once('exit',resolve);});}
 finally {fs.closeSync(fd);}
 stage.exitCode=code;stage.completedAt=new Date().toISOString();stage.logSha256=hash(fs.readFileSync(log));
 if(fs.existsSync(reportPath)){const bytes=fs.readFileSync(reportPath),record=JSON.parse(bytes);stage.reportSha256=hash(bytes);stage.actualCases=(record.cases??record.captures).length;stage.errors=record.errors;stage.completed=record.completed??record.captured;}
 persist();assert.equal(code,0,'Native capture stage failed: '+id);assert.equal(stage.completed,true);assert.equal(stage.actualCases,expectedCases);assert.ok(!stage.errors?.length,'Recorded errors in '+id);console.log(JSON.stringify(stage));
}
function exportRun(script,label){return ['projects/code-sentinels/pipeline/v6/'+script,'--run',label,'--engine',oldExe,'--engine-hash',oldHash,'--export-cases',path.join(project,'pipeline/v6/pixel-cases-64c-20260912'),'--representative'];}
function fixed(input,label,exe,digest){return ['projects/code-sentinels/pipeline/v6/capture_fixed_native_pixels.mjs','--cases',input,'--out',path.join(project,'Logs/v6',label),'--engine',exe,'--engine-hash',digest];}
try{
 for(const [kind,script,count] of [['fx','native_fx_lifecycle_qa.mjs',61],['wall','native_foreground_occlusion_qa.mjs',13]]){
  const oldLabel=kind+'-pixel-baseline-64c-20260912',newLabel=kind+'-pixel-candidate-4bf-20260912';
  if(kind==='fx'){
   const original=path.join(project,'Logs/v6',oldLabel,'capture-report.json'),bytes=fs.readFileSync(original),record=JSON.parse(bytes);
   assert.equal(record.captured,true);assert.equal(record.captures.length,61);assert.deepEqual(record.errors,[]);
   const manifest=JSON.parse(fs.readFileSync(path.join(project,'pipeline/v6/pixel-cases-64c-20260912',oldLabel,'cases.json')));assert.equal(manifest.cases.length,61);assert.ok(manifest.cases.every(c=>c.stability.exact));
   execution.reusedCompletedFxBaseline={path:original,sha256:hash(bytes),cases:61};persist();
  }else{
   assert.ok(!fs.existsSync(path.join(project,'Logs/v6',oldLabel)));
   await run(oldLabel,exportRun(script,oldLabel),count,path.join(project,'Logs/v6',oldLabel,'capture-report.json'));
  }
  await run(newLabel,fixed(path.join(project,'pipeline/v6/pixel-cases-64c-20260912',oldLabel,'cases.json'),newLabel,newExe,newHash),count,path.join(project,'Logs/v6',newLabel,'cases.json'));
 }
 const pressure=path.join(project,'pipeline/v6/static-pressure-pixel-cases-20260912/cases.json');
 for(const [label,exe,digest] of [['pressure-pixel-baseline-64c-20260912',oldExe,oldHash],['pressure-pixel-candidate-4bf-20260912',newExe,newHash]])await run(label,fixed(pressure,label,exe,digest),28,path.join(project,'Logs/v6',label,'cases.json'));
 execution.completed=true;execution.completedAt=new Date().toISOString();
}catch(error){execution.error=String(error.stack??error);process.exitCode=1;console.error(error);}
finally{persist();}
