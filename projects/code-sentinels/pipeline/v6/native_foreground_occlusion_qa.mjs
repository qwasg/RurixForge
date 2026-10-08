/** Real native character-only / wall-only / combined readbacks, with native alpha picking. */
import {session,projectPoint,width,height} from './native_visual_common.mjs';
const qa=await session('foreground-occlusion-20260911',{retainPixels:true});const {base,report,capture,rpc,native,save}=qa;report.checks=[];
const x=66.75,y=50.75,p={x:66,y:50,z:0};
const specs=[['glm',7,'death'],['minimax',3,'death'],['claude',1,'death'],['gemini',0,'idle']];
const diff=(a,b,i)=>Math.max(Math.abs(a[i]-b[i]),Math.abs(a[i+1]-b[i+1]),Math.abs(a[i+2]-b[i+2]));
try{const catalogue=await rpc('game.session.catalog');const empty=await capture('baseline',base);let n=0;
 for(const [character,direction,action] of specs){const def=catalogue.units.find(d=>d.id===character),origin=++n*7200,s=structuredClone(base);s.tick=origin+48;s.revision=s.tick;s.playback.currentTick=s.tick;
  const walls=(action==='idle'?[66,67,68]:[67,68,69]).map((xx,i)=>({id:10000+n*10+i,owner:1,pos:{x:xx,y:action==='idle'?51:52,z:0},kind:'physical',hp:500,maxHp:500,shield:0,invested:0,antiHeal:0}));s.walls=walls;
  const wall=await capture(`${character}-${action}-wall-only`,s);
  s.walls=[];s.revision++;
  if(action==='death')s.events=[{id:100000+n,tick:origin,kind:'unit-death:'+character,pos:p,owner:1,magnitude:1,subject:1000,subjectKind:character,presentationPosition:[x,y,.1],facing:direction}];
  else s.units=[{id:1000,owner:1,kind:character,pos:p,x,y,z:0,tier:def.tier,hp:def.hp,maxHp:def.hp,battery:100,batteryMax:100,covered:false,wired:false,ammo:0,ammoMax:0,energy:0,energyMax:0,fuel:0,fuelMax:0,route:[],target:null,cooldown:0,skillCooldown:0,plugins:[],statuses:{},invested:0,moving:false,attackCount:0,branch:def.branch,facing:direction,altitude:0,flightState:'ground',sourceFacility:0,lastAttackTick:null,lastCastTick:null,lastHitTick:null}];
  const alone=await capture(`${character}-${action}-character-only`,s,[{character,action,direction,expectedFoot:projectPoint([x,y,.1])}]);s.walls=walls;s.revision++;
  const both=await capture(`${character}-${action}-with-foreground-wall`,s);let occluded=0,visible=0,wallPoint,actorPoint;
  for(let py=300;py<950;py++)for(let px=650;px<1250;px++){const i=(py*width+px)*4;if(diff(alone.rgba,empty.rgba,i)<=20)continue;
   if(diff(wall.rgba,empty.rgba,i)>20&&diff(both.rgba,wall.rgba,i)<=3&&diff(both.rgba,alone.rgba,i)>15){occluded++;wallPoint??=[px+.5,py+.5];}
   if(diff(wall.rgba,empty.rgba,i)<=3&&diff(both.rgba,alone.rgba,i)<=3){visible++;actorPoint??=[px+.5,py+.5];}
  }
  const pick=async(point)=>point?rpc('game.session.pick',{screenX:point[0],screenY:point[1],width,height}):null;
  const wallPick=await pick(wallPoint),actorPick=action==='idle'?await pick(actorPoint):null;
  const check={character,action,direction,foregroundWalls:walls.map(w=>w.id),opaqueCharacterPixelsCorrectlyOccluded:occluded,characterPixelsStillVisible:visible,wallPoint,wallPick,actorPoint,actorPick,pass:occluded>20&&visible>100&&wallPick?.kind==='wall'&&walls.some(w=>w.id===wallPick.id)&&(action!=='idle'||actorPick?.kind==='unit'&&actorPick.id===1000)};
  report.checks.push(check);console.log(JSON.stringify(check));if(!check.pass)report.errors.push(`${character}/${action}: foreground occlusion/picking contract failed`);
 }
 report.captured=true;report.completedAt=new Date().toISOString();
}catch(e){report.errors.push(String(e.stack??e));console.error(e);process.exitCode=1;}finally{save();await native.close();}
