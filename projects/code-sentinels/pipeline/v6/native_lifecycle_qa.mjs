/** Explicit unearned visual fixture. Every PNG is an actual Rurix GPU RGBA readback. */
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import {createHash} from 'node:crypto';
import {launchNative} from '../../game/v6/native-rpc.mjs';
import {fixtureWriter} from './frozen_pixel_cases.mjs';
const project=path.resolve('projects/code-sentinels');
const option=(key,fallback)=>{const i=process.argv.indexOf(key);return i<0?fallback:process.argv[i+1];};
const run=option('--run','media-lifecycle-20260911');if(!/^[a-z0-9-]+$/.test(run))throw Error('Invalid QA output label');
const out=path.join(project,'Logs/v6',run);fs.mkdirSync(out,{recursive:true});
const executable=path.resolve(option('--engine',path.join(project,'game/v6/runtime-bin/engine-host.exe')));
const expectedHash=option('--engine-hash','9974933a7c984103c6bc09592ce78a6f6ba809ce8e3fc5ef1341220db88fdffc');
const sha=b=>createHash('sha256').update(b).digest('hex');
if(sha(fs.readFileSync(executable))!==expectedHash)throw Error('The fixed approved engine changed; do not silently test a different build');
const crcTable=Array.from({length:256},(_,n)=>{for(let k=0;k<8;k++)n=n&1?0xedb88320^(n>>>1):n>>>1;return n>>>0;});
function chunk(type,data){const name=Buffer.from(type),size=Buffer.alloc(4),crc=Buffer.alloc(4);size.writeUInt32BE(data.length);let c=0xffffffff;for(const b of Buffer.concat([name,data]))c=crcTable[(c^b)&255]^(c>>>8);crc.writeUInt32BE((c^0xffffffff)>>>0);return Buffer.concat([size,name,data,crc]);}
function png(rgba,width,height){const header=Buffer.alloc(13);header.writeUInt32BE(width);header.writeUInt32BE(height,4);header[8]=8;header[9]=6;const scan=Buffer.alloc(height*(width*4+1));for(let y=0;y<height;y++)rgba.copy(scan,y*(width*4+1)+1,y*width*4,(y+1)*width*4);return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',zlib.deflateSync(scan,{level:2})),chunk('IEND',Buffer.alloc(0))]);}
const width=1920,height=1080,view={centerX:64,centerY:48,zoom:4,layer:0,cutaway:true,localPlayer:1};
const dirs=['s','sw','w','nw','n','ne','e','se'];
const positions=dirs.map((direction,i)=>{const sx=330+(i%4)*420,sy=430+Math.floor(i/4)*440,dx=(sx-width/2)/90,dy=(height/2-sy)/90-.15;const x=view.centerX+dx-2*dy,y=view.centerY-dx-2*dy;return {direction,index:i,x,y,pos:{x:Math.floor(x),y:Math.floor(y),z:0},expectedFoot:[sx,sy]};});
const report={scope:'Unearned, validated replica visual fixture; actual native Rurix GPU readback. Not earned gameplay, balance, network or performance acceptance.',engineHash:expectedHash,engine:executable,startedAt:new Date().toISOString(),view,width,height,positions,visualClock:'Snapshot.playback.paused fixes visual time at snapshot.tick/60; private replica only, no new authority gameplay or arbitrary render API.',captures:[],errors:[]};
const fixedCases=fixtureWriter({project,captureDir:out,width,height,view,engineHash:expectedHash});
const native=await launchNative({root:project,executable,logs:path.join(out,'engine')});report.pid=native.pid;report.port=native.port;
const rpc=(method,p={},timeout=60000)=>native.rpc(method,p,timeout);
let baseline;
function clean(s){s=structuredClone(s);for(const k of ['buildings','rooms','units','links','walls','entrances','resources','shipments','projectiles','events','jobs','rubble','defenseFields','shieldRegions','networkStores','powerGrids','excavated'])s[k]=[];s.excavationOwners={};s.terrain.fill(3);s.winner=null;s.winReason='';const visible=[];for(let y=0;y<96;y++)for(let x=0;x<128;x++)visible.push({x,y,z:0});s.visible=[visible,visible];s.explored=[visible,visible];s.tick=0;s.revision=0;s.playback={paused:true,speed:1,currentTick:0,totalTicks:10000000};return s;}
function unit(character,def,p,action,origin,tick){const u={id:1000+p.index,owner:1,kind:character,pos:p.pos,x:p.x,y:p.y,z:0,tier:def.tier,hp:def.hp,maxHp:def.hp,battery:100,batteryMax:100,covered:false,wired:false,ammo:0,ammoMax:0,energy:0,energyMax:0,fuel:0,fuelMax:0,route:[],goal:null,queuedGoals:[],target:null,cooldown:def.period,skillCooldown:0,plugins:[],statuses:{},invested:0,moving:action==='walk',attackCount:0,branch:def.branch,facing:p.index,altitude:0,flightState:'ground',sourceFacility:0,sortieTarget:null,transitProgress:0,lastAttackTick:null,lastCastTick:null,lastHitTick:null};if(action==='attack')u.lastAttackTick=origin;if(action==='cast')u.lastCastTick=origin;if(action==='hit')u.lastHitTick=origin;return u;}
async function capture(name,s,expected=[]){await rpc('game.session.applySnapshot',{snapshot:s});const f=await rpc('viewport.frame',{width,height,format:'rgba8'},120000);const raw=Buffer.from(f.pixelsB64,'base64');if(f.width!==width||f.height!==height||raw.length!==width*height*4)throw Error('Unexpected actual GPU dimensions');if(f.meshFallbacks||f.truncated)throw Error('Native fallback or frame truncation');delete f.pixelsB64;const file=name+'.png';fs.writeFileSync(path.join(out,file),png(raw,width,height));const entry={file,tick:s.tick,revision:s.revision,rgbaSha256:sha(raw),pngSha256:sha(fs.readFileSync(path.join(out,file))),diagnostics:f,expected};report.captures.push(entry);fs.writeFileSync(path.join(out,'capture-report.json'),JSON.stringify(report,null,2));if(fixedCases)await fixedCases.append(name,s,expected,raw,path.join(out,file),async()=>{const r=await rpc('viewport.frame',{width,height,format:'rgba8'},120000);if(r.meshFallbacks||r.truncated)throw Error('Repeated native frame failed');return Buffer.from(r.pixelsB64,'base64');});return entry;}
try{
 const opened=await rpc('game.session.open',{mode:'replica',seed:611006,theme:'river',localPlayer:1,opponent:'human'});baseline=clean(opened.snapshot);const catalog=await rpc('game.session.catalog');
 await rpc('game.session.view',view);await capture('baseline',baseline);
 const chars=process.argv.includes('--pilot')?['gemini']:['gemini','claude','kimi','minimax','glm','deepseek','gpt'];
 let caseIndex=1;
 for(const character of chars){
  const metadata=JSON.parse(fs.readFileSync(path.join(project,'Content/Animations/v6/characters',character+'.json'),'utf8'));const def=catalog.units.find(u=>u.id===character);if(!def)throw Error('Missing native operator '+character);
  const actions=process.argv.includes('--pilot')?['idle']:['idle','walk','attack','cast','hit','death'];
  for(const action of actions){
   const clip=metadata.clips[action].s,count=clip.endExclusive-clip.start,origin=caseIndex++*7200;
   const frameIndices=process.argv.includes('--pilot')?[0]:process.argv.includes('--representative')?[...new Set([0,Math.floor(count/2),count-2,count-1])]:Array.from({length:count},(_,i)=>i);
   report.frameSelection=process.argv.includes('--representative')?'Explicit representative first/middle/penultimate/last frames of every character/action/direction; not full512-frame coverage.':'All delivered character atlas frames';
   for(const n of frameIndices){
    const duration=count/clip.fps,window=action==='attack'?Math.min(duration,def.period):duration;
    const age=(n+.5)/clip.fps*(action==='attack'?window/duration:1);const ticks=Math.round(age*60);const s=structuredClone(baseline);s.tick=origin+ticks;s.revision=s.tick;s.playback.currentTick=s.tick;
    s.units=action==='death'?[]:positions.map(p=>unit(character,def,p,action,origin,s.tick));
    if(action==='death')s.events=positions.map(p=>({id:100000+caseIndex*10+p.index,tick:origin,kind:'unit-death:'+character,pos:p.pos,owner:1,magnitude:1,subject:1000+p.index,subjectKind:character,presentationPosition:[p.x,p.y,.1],facing:p.index}));
    const expected=positions.map(p=>{const c=metadata.clips[action][p.direction],seconds=(action==='idle'||action==='walk')?s.tick/60:(s.tick-origin)/60*(action==='attack'?duration/window:1);const local=Math.floor(seconds*c.fps);return {character,action,direction:p.direction,frame:c.start+(c.loop?local%(c.endExclusive-c.start):Math.min(local,c.endExclusive-c.start-1)),pivot:c.pivot??metadata.pivot,span:c.nativePlaneSpan??metadata.nativePlaneSpan,frameSize:c.frameSize??metadata.frameSize,expectedFoot:p.expectedFoot,sourceJob:c.sourceJob};});
    const entry=await capture(`${character}-${action}-${String(n).padStart(2,'0')}`,s,expected);
    if(process.argv.includes('--pilot')){await new Promise(r=>setTimeout(r,300));const frozen=await capture('pilot-clock-repeat',s,expected);report.exactPausedClockProven=entry.rgbaSha256===frozen.rgbaSha256;if(!report.exactPausedClockProven)throw Error('Paused replica visual clock drifted');}
   }
   if(action==='death'){const s=structuredClone(baseline);s.tick=origin+Math.ceil(count/clip.fps*60)+2;s.revision=s.tick;s.playback.currentTick=s.tick;await capture(character+'-death-expired',s,[{character,action:'death-expired',expectedPresent:false}]);}
   console.log(JSON.stringify({character,action,captures:report.captures.length}));
  }
 }
 report.completedAt=new Date().toISOString();report.captured=true;
}catch(e){report.errors.push(String(e.stack??e));console.error(String(e.stack??e));process.exitCode=1;}
finally{fs.writeFileSync(path.join(out,'capture-report.json'),JSON.stringify(report,null,2));await native.close();}
