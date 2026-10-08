/** End-to-end portable regression against a fresh native process and HTTP/WS. */
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
const game = path.dirname(fileURLToPath(import.meta.url));
const pack = path.join(game, '..', 'dist', 'CodeSentinels-Windows');
const processHandle = spawn(path.join(pack, 'bin', 'node.exe'), [path.join(pack, 'bridge.mjs'), '--no-open'], { windowsHide:true,stdio:['ignore','pipe','pipe'] });
const results=[];let stream;
try {
  const base=await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>reject(new Error('Portable startup timeout')),15000);
    processHandle.stdout.on('data',chunk=>{const m=/ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(chunk.toString());if(m){clearTimeout(timer);resolve(m[1]);}});
    processHandle.once('error',reject);processHandle.once('exit',code=>reject(new Error(`Portable exited ${code}`)));
  });
  async function call(name,args={}) {
    const response=await fetch(base+'/api/forge/mcp/call',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({tool:'mcp__engine-scene__'+name,arguments:args,workspaceId:'portable'})});
    const envelope=await response.json();assert.equal(response.status,200,JSON.stringify(envelope));return JSON.parse(envelope.content[0].text);
  }
  const nativeHealth=await fetch(base+'/health').then(r=>r.json());
  const cold=await call('scene_summary');assert.equal(cold.playState,'edit');results.push({case:'cold_native_edit_session',pass:true,nativeHealth});
  async function start() {
    const summary=await call('scene_summary');if(summary.playState!=='edit')await call('play_exit');
    await call('asset_reload');await call('scene_load',{path:'Content/Scenes/Main.rxscene'});
    await call('play_enter');await call('play_pause');await call('play_step');
  }
  async function state(){return(await call('entity_list')).entities.find(e=>e.name==='CS_State').transform;}
  await start();assert.deepEqual((await state()).translation,[420,20,0]);results.push({case:'compilerless_ui_start',pass:true});
  const info=await call('viewport_stream_info');
  stream=new WebSocket(info.wsUrl);stream.binaryType='arraybuffer';
  const frame=await new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>reject(new Error('Native frame timeout')),20000);
    stream.addEventListener('open',()=>stream.send(JSON.stringify({type:'subscribe',width:960,height:540,maxFps:20})));
    stream.addEventListener('message',event=>{if(event.data instanceof ArrayBuffer){clearTimeout(timer);resolve(event.data);}});
    stream.addEventListener('error',reject);
  });
  const view=new DataView(frame);assert.equal(view.getUint32(0,true),0x31464746);assert.ok(view.getUint32(16,true)>0);
  results.push({case:'native_gpu_websocket_frame',pass:true,bytes:frame.byteLength,width:view.getUint16(8,true),height:view.getUint16(10,true),draws:view.getUint32(16,true)});
  stream.send(JSON.stringify({type:'input',action:'cs',value:1001}));
  await new Promise(resolve=>setTimeout(resolve,80));await call('play_step');
  assert.equal((await state()).translation[0],340);results.push({case:'browser_channel_input_purchase',pass:true});
  await start();assert.equal((await state()).translation[0],420);results.push({case:'restart_resets_native_state',pass:true});
  const foreign=await fetch(base+'/api/forge/mcp/call',{method:'POST',headers:{'content-type':'application/json','origin':'https://example.com'},body:'{}'});assert.equal(foreign.status,403);
  const badScene=await fetch(base+'/api/forge/mcp/call',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({tool:'mcp__engine-scene__scene_load',arguments:{path:'../other.rxscene'}})});assert.equal(badScene.status,400);
  results.push({case:'local_bridge_scope',pass:true});
  const events=await call('host_events_drain');const errors=(events.events||events).filter(e=>['logic.call_error','logic.unsupported','anim.warn'].includes(e.event));assert.deepEqual(errors,[]);
  results.push({case:'native_runtime_errors',pass:true,errors});
  await call('play_exit');
} finally {
  stream?.close();processHandle.kill();
  fs.writeFileSync(path.join(game,'native','portable-regression.json'),JSON.stringify(results,null,2));
  console.log(JSON.stringify(results,null,2));
}
