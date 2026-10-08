import fs from 'node:fs';import path from 'node:path';
import {session,projectPoint} from './native_visual_common.mjs';
const qa=await session('fx-lifecycle-20260911');const {project,base,report,capture,rpc,native,save}=qa;
const definitions=[
 ['collapse-explosion','destroy',''],['heavy-impact','projectile-impact','arc'],['kinetic-hit','projectile-impact','direct'],['plasma-hit','projectile-impact','beam'],
 ['power-arc','guidance-jammed',''],['shield-hit','shield-absorb',''],['upgrade','construction-complete',''],['cone-shockwave','skill-debug-cone',''],
 ['directional-beam','skill-penetrating-mark',''],['energy-barrier','skill-intercept-barrier',''],['floor-collapse','collapse',''],['network-shield','skill-target-lock',''],
 ['orbital-strike','projectile-impact','orbital'],['repair-field','skill-targeted-support','']
].map(([asset,kind,subjectKind])=>({asset,kind,subjectKind,metadata:JSON.parse(fs.readFileSync(path.join(project,'Content/Animations/v6/effects',asset+'.json'),'utf8'))}));
const places=[{x:55,y:50,z:0},{x:66,y:39,z:0},{x:66,y:61,z:0},{x:77,y:50,z:0}],deltas=[[1,1],[0,1],[-1,1],[-1,0],[-1,-1],[0,-1],[1,-1],[1,0]];
let caseIndex=1;
function state(tick){const s=structuredClone(base);s.tick=tick;s.revision=tick;s.playback.currentTick=tick;return s;}
function event(def,p,tick,id,direction={x:p.x+1,y:p.y-1,z:0}){return {id,tick,kind:def.kind,pos:p,owner:1,magnitude:0,subject:5000+id,subjectKind:def.subjectKind,presentationPosition:[p.x+.5,p.y+.5,.4],direction};}
try{
 await capture('baseline',base);
 for(let start=0;start<definitions.length;start+=4){const group=definitions.slice(start,start+4),count=Math.max(...group.map(d=>d.metadata.frameCount)),origin=caseIndex++*7200;
  const selected=process.argv.includes('--representative')?[...new Set(group.flatMap(d=>[0,Math.floor(d.metadata.frameCount/2),d.metadata.frameCount-1,d.metadata.frameCount]))].sort((a,b)=>a-b):Array.from({length:count+1},(_,i)=>i);
  for(const frame of selected){const s=state(origin+(frame+1)*60),expected=[];
   group.forEach((def,i)=>{const c=def.metadata.clips.oneshot,p=places[i],scale=def.kind.startsWith('skill-')?1.8:1;
    if(frame>=def.metadata.frameCount){s.events.push(event(def,p,s.tick-Math.ceil(def.metadata.frameCount/c.fps*60)-2,100000+start*100+i));expected.push({asset:def.asset,expectedPresent:false,slot:i});return;}
    const ageTicks=Math.round((frame+.5)/c.fps*60);s.events.push(event(def,p,s.tick-ageTicks,100000+start*100+i));
    expected.push({asset:def.asset,slot:i,frame:c.start+Math.floor(ageTicks/60*c.fps),pivot:def.metadata.pivot??[.5,.88],span:(def.metadata.nativePlaneSpan??2.5456)*scale,frameSize:[256,256],blend:def.metadata.blend==='additive'?'additive':'alpha',origin:[p.x+.5,p.y+.5,.4],expectedFoot:projectPoint([p.x+.5,p.y+.5,.4]),direction:s.events.at(-1).direction});
   });await capture(`fx-group-${start/4}-frame-${String(frame).padStart(2,'0')}`,s,expected);
  }console.log(JSON.stringify({effects:group.map(d=>d.asset),captures:report.captures.length}));
 }
 for(const asset of ['directional-beam','cone-shockwave']){const def=definitions.find(d=>d.asset===asset),origin=caseIndex++*7200,p={x:64,y:48,z:0};for(let d=0;d<8;d++){const s=state(origin+(d+1)*60),ageTicks=Math.round(24.5/24*60);s.events.push(event(def,p,s.tick-ageTicks,200000+d,{x:p.x+deltas[d][0]*3,y:p.y+deltas[d][1]*3,z:0}));await capture(`${asset}-direction-${d}`,s,[{asset,directionIndex:d,frame:24,origin:[p.x+.5,p.y+.5,.4],expectedFoot:projectPoint([p.x+.5,p.y+.5,.4]),kind:'directional-rotation-check'}]);}}
 report.persistentLoops=[];
 for(const asset of ['construction-dust','energy-barrier']){const meta=JSON.parse(fs.readFileSync(path.join(project,'Content/Animations/v6/effects',asset+'.json'),'utf8')),c=meta.clips.oneshot,origin=caseIndex++*7200,count=meta.frameCount;let first;
  const selected=process.argv.includes('--representative')?[0,Math.floor(count/2),count-1,count]:Array.from({length:count+1},(_,i)=>i);
  for(const n of selected){const frame=n%count,tick=origin+Math.round((n+.5)/c.fps*60),s=state(tick),point=asset==='construction-dust'?[64,48,.1]:[64.5,48.5,.2],scale=asset==='construction-dust'?1:2;
   if(asset==='construction-dust')s.jobs=[{id:400001,owner:1,target:400002,rect:{x:63,y:47,z:0,w:2,h:2},kind:'shell',worker:{x:3,y:3,z:0},route:[],progress:.5,duration:20,invested:0,blocked:false}];
   else s.defenseFields=[{id:400003,owner:1,pos:{x:64,y:48,z:0},direction:{x:67,y:45,z:0},radius:6,angle:60,hp:100,remaining:10,kind:'intercept-barrier'}];
   const result=await capture(`${asset}-persistent-${String(n).padStart(2,'0')}`,s,[{asset,frame,pivot:meta.pivot,span:(meta.nativePlaneSpan??2.5456)*scale,frameSize:[256,256],blend:meta.blend==='additive'?'additive':'alpha',origin:point,expectedFoot:projectPoint(point),persistentLoop:true}]);if(n===0)first=result.rgbaSha256;if(n===count)report.persistentLoops.push({asset,fullFrames:count,metadataDuration:count/c.fps,sourceStartAndNextLoopStartHaveIdenticalGpuPixels:first===result.rgbaSha256});
  }console.log(JSON.stringify({persistentEffect:asset,captures:report.captures.length}));
 }
 // Exercise the fallback world-vector->screen-facing mapping used by ghosts,
 // separately from the explicit facing field used in the full character set.
 const prior=JSON.parse(fs.readFileSync(path.join(project,'Logs/v6/media-lifecycle-20260911/capture-report.json'),'utf8'));
 for(const character of ['gemini','claude','kimi','minimax','glm','deepseek','gpt']){const meta=JSON.parse(fs.readFileSync(path.join(project,'Content/Animations/v6/characters',character+'.json'),'utf8')),origin=caseIndex++*7200;
  for(const frame of [5,9]){const ageTicks=Math.round((frame+.5)/12*60),s=state(origin+ageTicks),expected=[];for(const p of prior.positions){s.events.push({id:300000+caseIndex*10+p.index,tick:origin,kind:'unit-death:'+character,pos:p.pos,owner:1,magnitude:1,subject:1000+p.index,subjectKind:character,presentationPosition:[p.x,p.y,.1],direction:{x:p.pos.x+deltas[p.index][0],y:p.pos.y+deltas[p.index][1],z:0}});const c=meta.clips.death[p.direction];expected.push({character,action:'death',direction:p.direction,frame:c.start+frame,pivot:c.pivot,span:c.nativePlaneSpan,frameSize:c.frameSize,expectedFoot:p.expectedFoot,sourceJob:c.sourceJob,kind:'world-vector-facing-check'});}await capture(`${character}-death-world-direction-${frame}`,s,expected);}
 }
 report.unmappedLegacyAsset='repair is retained V5 media with no current V6 render event mapping; no unsupported direct-asset draw was claimed.';report.constructionDust=process.argv.includes('--representative')?'Explicit first/middle/last/next-loop samples, not all48 source frames.':'Actual job and defense-field persistent-loop fixture covers all48@24fps source frames and next loop entry; numerical source consumption is checked separately.';report.frameSelection=process.argv.includes('--representative')?'Every active FX first/middle/last/expired, loop seam and eight-direction examples; not full656-frame coverage.':'All current active effect source frames';report.captured=true;report.completedAt=new Date().toISOString();
}catch(e){report.errors.push(String(e.stack??e));console.error(e);process.exitCode=1;}finally{save();await native.close();}
