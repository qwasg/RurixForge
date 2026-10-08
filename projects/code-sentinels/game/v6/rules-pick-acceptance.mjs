/** Normal fixed-path native launch. Rule identity is a fresh real session;
 * the room image/pick comparison is explicitly a presentation-only import of
 * an owner-filtered snapshot earned in the earlier UI development session. */
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {launchNative} from './native-rpc.mjs';
const project=path.resolve('projects/code-sentinels');
const out=path.join(project,'Logs/v6/rules-pick-20260911');fs.mkdirSync(out,{recursive:true});
const native=await launchNative({root:project,executable:path.join(project,'game/v6/runtime-bin/engine-host.exe'),logs:path.join(out,'engine')});
const rpc=(m,p={})=>native.rpc(m,p,60000);
const report={at:new Date().toISOString(),engineSha256:native.rulesHash,enginePid:native.pid,passed:false,identity:[],picks:[]};
try{
  const catalog=await rpc('game.session.catalog');
  assert.equal(catalog.rulesVersion,'v6.2');assert.match(catalog.rulesFingerprint,/^[a-f0-9]{64}$/);
  report.rulesVersion=catalog.rulesVersion;report.rulesFingerprint=catalog.rulesFingerprint;
  await rpc('game.session.open',{mode:'authority',seed:8246,opponent:'human'});await rpc('play.pause');
  assert.equal((await rpc('game.session.order',{owner:1,sequence:1,command:{op:'shield',enabled:false}})).accepted,true);
  const save=await rpc('game.session.save');assert.equal(save.rulesFingerprint,catalog.rulesFingerprint);assert.equal(save.rulesVersion,catalog.rulesVersion);
  fs.writeFileSync(path.join(out,'fresh-native-save.json'),JSON.stringify(save));
  await rpc('game.session.load',{save});await rpc('play.pause');
  await rpc('game.session.replay',{save});
  report.identity.push({case:'fresh native Save/load/exact verification replay',passed:true,tick:save.snapshot.tick});
  for(const mode of ['missing','mismatch']){
    const invalid=structuredClone(save);
    if(mode==='missing'){delete invalid.rulesFingerprint;delete invalid.rulesVersion;}else invalid.rulesFingerprint='0'.repeat(64);
    for(const playback of ['load','verify','playback']){
      const method=playback==='load'?'game.session.load':'game.session.replay';let message;
      try{await rpc(method,{save:invalid,playback:playback==='playback',allowLegacy:true});}catch(error){message=error.message;}
      assert.match(message??'',/规则指纹/);report.identity.push({case:`${mode} identity rejected by ${playback}`,passed:true,message});
    }
  }
  const source=path.join(project,'game/v6/room-floor-pick-before-20260911.snapshot.json');
  const snapshot=JSON.parse(fs.readFileSync(source,'utf8'));
  await rpc('game.session.open',{mode:'replica',seed:snapshot.seed,theme:snapshot.theme,localPlayer:1,opponent:'human'});
  await rpc('game.session.applySnapshot',{snapshot});
  const view={centerX:16,centerY:48,zoom:2,layer:1,cutaway:true,localPlayer:1};await rpc('game.session.view',view);await rpc('play.pause');
  const iso=(x,y,z)=>[(x-y)/2,-(x+y)/4+z*1.5];const center=iso(16,48,1);
  for(const point of [[16,47,1.21],[16.6,47.6,1.21]]){
    const p=iso(...point),screenX=((p[0]-center[0])/(12*(1280/720))+1)*640,screenY=(1-(p[1]-center[1])/12)*360;
    const pick=await rpc('game.session.pick',{screenX,screenY,width:1280,height:720,view});
    report.picks.push({point,screenX,screenY,pick,expectedRoom:799});assert.equal(pick.kind,'room');assert.equal(pick.id,799);
  }
  const frame=await rpc('viewport.frame',{width:1280,height:720,format:'rgba8'});
  const pixels=Buffer.from(frame.pixelsB64,'base64');delete frame.pixelsB64;
  assert.equal(frame.width,1280);assert.equal(frame.height,720);assert.equal(frame.meshFallbacks,0);assert.equal(frame.truncated,false);
  assert.equal(pixels.length,1280*720*4);
  fs.writeFileSync(path.join(out,'room-floor-rgba8.bin'),pixels);
  const rgb=Buffer.alloc(1280*720*3);for(let i=0;i<1280*720;i++){rgb[i*3]=pixels[i*4];rgb[i*3+1]=pixels[i*4+1];rgb[i*3+2]=pixels[i*4+2];}
  fs.writeFileSync(path.join(out,'room-floor.ppm'),Buffer.concat([Buffer.from('P6\n1280 720\n255\n'),rgb]));
  report.frame=frame;report.presentationFixture={source,earnedEarlier:true,usedForCurrentRuleSaveAcceptance:false};report.passed=true;
}catch(error){report.error=String(error.stack??error);throw error;}
finally{fs.writeFileSync(path.join(out,'acceptance.json'),JSON.stringify(report,null,2));await native.close();console.log(JSON.stringify(report));}
