/** Portable V2 integration through real HTTP + native RGBA WebSocket frames. */
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const game=path.dirname(fileURLToPath(import.meta.url));
const pack=path.join(game,'..','dist','CodeSentinels-V2-Windows');
const mode=process.argv.includes('--on')?'on':'off';
const child=spawn(path.join(pack,'bin/node.exe'),[path.join(pack,'bridge.mjs'),'--no-open',...(mode==='on'?['--gpu-particles']:[])],{
  windowsHide:true,stdio:['ignore','pipe','pipe'],
  // Opposite inherited value proves each launcher sets its own explicit mode.
  env:{...process.env,FORGE_GPU_PARTICLES:mode==='on'?'off':'on'},
});
const results=[];let stream;
try{
  const base=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Portable V2 startup timeout')),20000);child.stdout.on('data',chunk=>{const m=/ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(chunk.toString());if(m){clearTimeout(timer);resolve(m[1]);}});child.once('error',reject);child.once('exit',code=>reject(Error(`Bridge exit ${code}`)));});
  async function call(name,args={}){const response=await fetch(base+'/api/forge/mcp/call',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({tool:'mcp__engine-scene__'+name,arguments:args,workspaceId:'portable'})});const envelope=await response.json();assert.equal(response.status,200,JSON.stringify(envelope));return JSON.parse(envelope.content[0].text);}
  async function state(){return Object.fromEntries((await call('entity_list')).entities.map(e=>[e.name,e.transform]));}
  async function command(code){await call('logic_inject_input',{action:'cs',value:code});await call('play_step');}
  async function steps(count){for(let i=0;i<count;i++)await call('play_step');}
  async function start(){const summary=await call('scene_summary');if(summary.playState!=='edit')await call('play_exit');await call('asset_reload');await call('scene_load',{path:'Content/Scenes/Main.rxscene'});await call('play_enter');await call('play_pause');await call('play_step');}
  const health=await fetch(base+'/health').then(r=>r.json());assert.equal(health.gpuParticles,mode);assert.equal(health.compilerDisabled,true);results.push({case:'explicit_mode_and_no_compiler',pass:true,health});
  await start();let s=await state();assert.deepEqual(s.CS_State.translation,[0,20,0]);assert.equal(s.CS_Economy.translation[0],560);assert.equal(s.CS_Economy.scale[2],0);results.push({case:'cold_start_empty_gpu',pass:true});
  await command(5000000);s=await state();assert.equal(s.CS_Meta.translation[0],30);assert.equal(s.CS_State.scale[0],0);results.push({case:'requires_player_gpu_purchase',pass:true});
  await command(4000001);await command(4000012);await command(1003174);await command(1004125);s=await state();assert.equal(s.CS_Economy.translation[0],0);assert.equal(s.CS_Economy.translation[1],32);assert.equal(s.CS_GPU0.translation[0],1);assert.equal(s.CS_GPU1.translation[0],2);assert.equal(s.CS_Unit0.translation[0],3);assert.equal(s.CS_Unit1.translation[0],4);results.push({case:'two_real_gpu_models_and_two_ai_units',pass:true});
  const info=await call('viewport_stream_info');stream=new WebSocket(info.wsUrl);stream.binaryType='arraybuffer';let frames=0,lastFrameId=0;const statuses=[];
  const first=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Native first frame timeout')),30000);stream.addEventListener('open',()=>stream.send(JSON.stringify({type:'subscribe',width:1280,height:720,maxFps:40})));stream.addEventListener('message',event=>{if(event.data instanceof ArrayBuffer){const v=new DataView(event.data);assert.equal(v.getUint32(0,true),0x31464746);frames++;lastFrameId=v.getUint32(4,true);if(frames===1){clearTimeout(timer);resolve({width:v.getUint16(8,true),height:v.getUint16(10,true),draws:v.getUint32(16,true)});}}else{try{const value=JSON.parse(event.data);if(value.type==='status'||value.fps!==undefined)statuses.push(value);}catch{}}});stream.addEventListener('error',reject);});
  assert.equal(first.width,1280);results.push({case:'native_gpu_frame',pass:true,...first});
  await steps(1450);s=await state();assert.equal(Math.round(s.CS_State.translation[0]),720);assert.equal(s.CS_Economy.translation[0],0);assert.equal(s.CS_Economy.scale[0],0);assert.equal(s.CS_Economy.scale[1],0);results.push({case:'gpu_production_fills_capacity_without_passive_currency',pass:true,energy:s.CS_State.translation[0]});
  stream.send(JSON.stringify({type:'input',action:'cs',value:5000000}));await new Promise(r=>setTimeout(r,80));await call('play_step');
  for(let i=0;i<25;i++){await steps(60);s=await state();if(s.CS_Economy.scale[0]>0)break;}
  assert.ok(s.CS_Economy.scale[0]>0);results.push({case:'auto_attack_consumes_generated_energy',pass:true,spentAttack:s.CS_Economy.scale[0]});
  const live=Object.entries(s).filter(([name,e])=>/^CS_Enemy\d+$/.test(name)&&e.translation[0]>-50).sort((a,b)=>a[1].translation[0]-b[1].translation[0]);assert.ok(live.length);
  const [x,y]=live[0][1].translation;const target=Math.max(0,Math.min(335,Math.floor(7-y)*24+Math.floor(x+12)));const before=s.CS_Economy.scale[1];await command(3001000+target);await steps(12);s=await state();assert.equal(s.CS_Economy.scale[1]-before,110);assert.ok(s.CS_Unit1.scale[1]>23);assert.ok(Object.entries(s).some(([name,e])=>/^CS_VFXOverlay\d+$/.test(name)&&e.translation[0]>-50));results.push({case:'targeted_gpt_skill_cost_and_video_effect',pass:true,target,spentSkill:s.CS_Economy.scale[1]});
  await call('play_resume');await new Promise(r=>setTimeout(r,2000));const startFrames=frames,startTime=performance.now();await new Promise(r=>setTimeout(r,10000));const seconds=(performance.now()-startTime)/1000;const fps=(frames-startFrames)/seconds;
  results.push({case:'live_combat_websocket_fps',pass:fps>=30,fps,frames:frames-startFrames,seconds,lastFrameId,statuses:statuses.slice(-3)});assert.ok(fps>=30,`Measured ${fps.toFixed(1)} fps`);
  await call('play_pause');await start();s=await state();assert.deepEqual(s.CS_State.translation,[0,20,0]);assert.equal(s.CS_Economy.translation[0],560);assert.equal(s.CS_Economy.scale[2],0);results.push({case:'restart_clears_gameplay_state',pass:true});
  const events=await call('host_events_drain');const errors=(Array.isArray(events)?events:events.events).filter(e=>['logic.call_error','logic.unsupported','anim.warn'].includes(e.event));assert.deepEqual(errors,[]);results.push({case:'native_runtime_errors',pass:true,errors});await call('play_exit');
}finally{stream?.close();child.kill();fs.writeFileSync(path.join(game,'v2',`portable-${mode}-regression.json`),JSON.stringify(results,null,2));console.log(JSON.stringify(results,null,2));}
