/** V3 transport/static-resource smoke; card interaction QA belongs to the actual UI browser pass. */
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const game=path.dirname(fileURLToPath(import.meta.url));
const pack=path.join(game,'..','dist','CodeSentinels-V3-Windows');
const mode=process.argv.includes('--on')?'on':'off';
const build=JSON.parse(fs.readFileSync(path.join(game,'v3','portable-build.json'),'utf8'));
const child=spawn(path.join(pack,'bin/node.exe'),[path.join(pack,'bridge.mjs'),'--no-open',...(mode==='on'?['--gpu-particles']:[])],{
  windowsHide:true,stdio:['ignore','pipe','pipe'],env:{...process.env,FORGE_GPU_PARTICLES:mode==='on'?'off':'on'},
});
let ws;const checks=[];
try{
  const base=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('V3 bridge startup timeout')),20000);child.stdout.on('data',chunk=>{const match=/ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(chunk.toString());if(match){clearTimeout(timer);resolve(match[1]);}});child.once('error',reject);child.once('exit',code=>reject(Error(`Bridge exited ${code}`)));});
  const health=await fetch(base+'/health').then(r=>r.json());assert.equal(health.gpuParticles,mode);assert.equal(health.compilerDisabled,true);checks.push({case:'explicit_launcher_mode_and_no_compiler',pass:true,health});
  const html=await fetch(base+'/?play=code-sentinels&workspace=portable&standalone=1').then(r=>r.text());assert.ok(html.includes(`/assets/${build.clientBundle}`));
  const walk=dir=>fs.readdirSync(dir,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?walk(path.join(dir,entry.name)):[path.join(dir,entry.name)]);
  const web=path.join(pack,'Web');const files=walk(web);const failures=[];
  for(let offset=0;offset<files.length;offset+=6){
    await Promise.all(files.slice(offset,offset+6).map(async file=>{
      const relative=path.relative(web,file).split(path.sep).map(encodeURIComponent).join('/');const response=await fetch(base+'/'+relative,{method:'HEAD'});
      if(response.status!==200||Number(response.headers.get('content-length'))!==fs.statSync(file).size)failures.push({path:relative,status:response.status});
    }));
  }
  assert.deepEqual(failures,[]);checks.push({case:'all_packaged_ui_resources_resolve',pass:true,files:files.length,clientBundle:build.clientBundle});
  const staticReferences=new Set();
  for(const file of files.filter(f=>/\.(js|css|html)$/.test(f))){
    const text=fs.readFileSync(file,'utf8');
    for(const match of text.matchAll(/["'`](\/(?:assets|games)\/[^\s"'`]+\.(?:png|jpg|jpeg|webp|svg|avif|gif|woff2?|ttf|js|css)(?:\?[^\s"'`]*)?)["'`]/g))staticReferences.add(match[1]);
  }
  const missing=[];
  for(const ref of staticReferences){const response=await fetch(base+ref,{method:'HEAD'});if(response.status!==200)missing.push({ref,status:response.status});}
  assert.deepEqual(missing,[]);checks.push({case:'literal_bundle_resource_references_resolve',pass:true,references:staticReferences.size,note:'Dynamic card URLs still require the real browser image pass.'});
  async function call(name,args={}){const response=await fetch(base+'/api/forge/mcp/call',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({tool:'mcp__engine-scene__'+name,arguments:args,workspaceId:'portable'})});const envelope=await response.json();assert.equal(response.status,200,JSON.stringify(envelope));return JSON.parse(envelope.content[0].text);}
  async function state(){return Object.fromEntries((await call('entity_list')).entities.map(e=>[e.name,e.transform]));}
  async function start(){const summary=await call('scene_summary');if(summary.playState!=='edit')await call('play_exit');await call('asset_reload');await call('scene_load',{path:'Content/Scenes/Main.rxscene'});await call('play_enter');await call('play_pause');await call('play_step');}
  await start();let s=await state();assert.deepEqual(s.CS_State.translation,[0,20,0]);assert.equal(s.CS_Economy.translation[0],560);assert.equal(s.CS_Economy.scale[2],0);checks.push({case:'cold_start_native_state',pass:true});
  async function connect(){
    const info=await call('viewport_stream_info');const socket=new WebSocket(info.wsUrl);socket.binaryType='arraybuffer';
    const frame=await new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Native frame timeout')),30000);socket.addEventListener('open',()=>socket.send(JSON.stringify({type:'subscribe',width:1280,height:720,maxFps:20})));socket.addEventListener('message',event=>{if(event.data instanceof ArrayBuffer){const view=new DataView(event.data);assert.equal(view.getUint32(0,true),0x31464746);clearTimeout(timer);resolve({width:view.getUint16(8,true),height:view.getUint16(10,true),draws:view.getUint32(16,true)});}});socket.addEventListener('error',reject);});
    return {socket,frame};
  }
  let connected=await connect();ws=connected.socket;checks.push({case:'native_rgba_frame',pass:true,...connected.frame});
  ws.send(JSON.stringify({type:'input',action:'cs',value:4000001}));await new Promise(r=>setTimeout(r,70));await call('play_step');
  s=await state();assert.equal(s.CS_Economy.translation[0],430);assert.equal(s.CS_GPU0.translation[0],1);checks.push({case:'offline_native_gpu_purchase_command',pass:true});
  ws.close();await new Promise(r=>setTimeout(r,70));connected=await connect();ws=connected.socket;await call('play_step');s=await state();assert.equal(s.CS_Economy.translation[0],430);assert.equal(s.CS_Economy.scale[2],1);checks.push({case:'reconnect_preserves_live_state_without_duplicate_purchase',pass:true});
  await start();s=await state();assert.deepEqual(s.CS_State.translation,[0,20,0]);assert.equal(s.CS_Economy.translation[0],560);assert.equal(s.CS_Economy.scale[2],0);checks.push({case:'restart_resets_native_battle',pass:true});
  const events=await call('host_events_drain');const errors=(Array.isArray(events)?events:events.events).filter(e=>['logic.call_error','logic.unsupported','anim.warn'].includes(e.event));assert.deepEqual(errors,[]);checks.push({case:'no_native_runtime_errors',pass:true});await call('play_exit');
}finally{
  ws?.close();child.kill();
  fs.writeFileSync(path.join(game,'v3',`portable-${mode}-smoke.json`),JSON.stringify({mode,checks,scope:'Static delivery and real native transport; new card interactions must be checked in browser.'},null,2));
  console.log(JSON.stringify(checks,null,2));
}
