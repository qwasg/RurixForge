"use strict";
(() => {
const $=id=>document.getElementById(id), canvas=$("game"),ctx=canvas.getContext("2d");
const W=1280,H=720,S=64,DT=1/60,BASE=647;
const C={ivory:"#F3EBDD",orange:"#C77958",ink:"#283C48",fog:"#AABABC",gold:"#E9B969"};
const clamp=(v,a,b)=>Math.max(a,Math.min(b,v)),mix=(a,b,t)=>a+(b-a)*t;
const round=v=>Math.round(v*1000)/1000;
const random=n=>{const v=Math.sin(n*127.1+311.7)*43758.5453;return v-Math.floor(v)};
const held=new Set(),pressed=new Set();
let profile={schemaVersion:1,unlockedLevelIds:["rooftops"],completed:false,settings:{musicVolume:.35,sfxVolume:.55,muted:false}};
let storageFailed=false;
try{const v=JSON.parse(localStorage.getItem("lanternkeeper.demo.v1")||"null");if(v&&v.schemaVersion===1){profile.unlockedLevelIds=v.unlockedLevelIds?.includes("docks")?["rooftops","docks"]:["rooftops"];profile.completed=!!v.completed;for(const k of ["musicVolume","sfxVolume"])profile.settings[k]=clamp(Number(v.settings?.[k])||0,0,1);profile.settings.muted=!!v.settings?.muted;}}catch(_){storageFailed=true}
function save(){try{localStorage.setItem("lanternkeeper.demo.v1",JSON.stringify(profile))}catch(_){storageFailed=true;$("storageNote").classList.remove("hidden")}}
const definitions=[
{id:"rooftops",name:"屋顶",title:"瓦上的微光",length:92,start:{x:1.8,y:3.4},exit:88.5,
platforms:[
["r0",0,8,3.4],["r1",10,2.8,3.65,"g1"],["r2",15,3,4.1,"g1"],["r3",20.5,8,3.7],
["r4",31,3,4.3,"g2"],["r5",36.6,2.8,4.8,"g2"],["r6",42,8.5,4.2],["r7",53,3,4.5,"g2"],
["r8",58.5,8,3.7],["r9",69,3,4.25,"g3"],["r10",74.6,3.1,4.5,"g3"],["r11",80.2,11.8,3.8],
["rb1",26.7,2.6,5.3,null,"branch"],["rb2",30.5,2.5,6.5,null,"branch"],["rb3",35,3,6.8,null,"branch"],["rb4",39.5,1.7,6,"opt","branch"]],
lanterns:[
{id:"roof-main-1",x:5.6,y:3.4,group:"g1",kind:"main",spawn:{x:5.4,y:3.4}},
{id:"roof-main-2",x:25,y:3.7,group:"g2",kind:"main",spawn:{x:24.6,y:3.7}},
{id:"roof-main-3",x:62,y:3.7,group:"g3",kind:"main",spawn:{x:61.6,y:3.7}},
{id:"roof-optional",x:36.2,y:6.8,group:"opt",kind:"optional"}]},
{id:"docks",name:"码头",title:"潮声里的灯",length:94,start:{x:1.8,y:2.9},exit:87.5,
platforms:[
["d0",0,8,2.9],["d1",10.5,3,3,"g1","moving",1.35,4.8],["d2",16.2,6.8,3.1],
["d3",25.5,3,3.6,"g2"],["d4",31,3.1,3.8,"g2","moving",1.4,5.2],["d5",37,7,3.5],
["d6",46.3,3.1,3.9,"g2"],["d7",52,8.6,3.2],["d8",63,3,3.8,"g3"],
["d9",69,3,3.6,"g3","moving",1.5,5.6],["d10",75,3,3.9,"g3"],["d11",80.4,13.6,3.3],
["db1",40,2.6,5.2,null,"branch"],["db2",44,3,6.4,null,"branch"],["db3",49.2,2.3,5.8,"opt","branch"]],
lanterns:[
{id:"dock-main-1",x:5.6,y:2.9,group:"g1",kind:"main",spawn:{x:5.4,y:2.9}},
{id:"dock-main-2",x:20.7,y:3.1,group:"g2",kind:"main",spawn:{x:20.3,y:3.1}},
{id:"dock-main-3",x:56.5,y:3.2,group:"g3",kind:"main",spawn:{x:56.1,y:3.2}},
{id:"dock-optional",x:45.5,y:6.4,group:"opt",kind:"optional"}]}
];
let run,level,player,platforms,lit,groups,checkpoint,cameraX=0,cameraY=0,visualTime=0;
let phase="TITLE",timer=0,toastTime=0,toastMessage="",igniteTime=0,landTime=0,particles=[];
let nearest=null,endingLit=false,restartCount=0,frameNo=0,fallCount=0,totalFalls=0,elapsed=0,menuIndex=0,results={};
let stats,events,checkpointsUsed=new Set(),motionClock=0;
function clearKeys(){held.clear();pressed.clear()}
function event(type,data={}){events.push({frame:frameNo,type,...data});if(events.length>32)events.shift()}
function resetCounters(){stats={jumps:0,landings:0,distance:0,lightInteractions:0,keyboardInputs:0,movingLandings:0,rideDistance:0,jumpPeak:0};events=[];checkpointsUsed=new Set()}
function createPlatforms(def){return def.platforms.map(a=>({id:a[0],baseX:a[1],x:a[1],oldX:a[1],w:a[2],y:a[3],group:a[4]||null,kind:a[5]||"static",amp:a[6]||0,period:a[7]||1,age:0,visibility:a[4]?0:1,enabled:!a[4],delta:0}))}
function loadLevel(id){level=definitions.find(d=>d.id===id)||definitions[0];platforms=createPlatforms(level);lit=new Set();groups=new Set();checkpoint={id:"start",...level.start};player={...level.start,vx:0,vy:0,grounded:true,facing:1,coyote:.1,buffer:0,cut:false,standing:platforms[0].id,jumpStartY:level.start.y};motionClock=0;fallCount=0;endingLit=false;particles=[];nearest=null;igniteTime=0;cameraX=0;cameraY=0;snapCamera();clearKeys();event("LevelLoaded",{id:level.id})}
function initialize(){replayReport=null;phase="TITLE";frameNo=0;visualTime=0;elapsed=0;totalFalls=0;restartCount=0;results={};timer=0;toastTime=0;resetCounters();loadLevel("rooftops");run={id:1};syncUI();render()}
function startNew(id="rooftops"){nextNote=0;resetCounters();results={};elapsed=0;totalFalls=0;loadLevel(id);phase="PLAYING";timer=0;toast(id==="docks"?"码头 · 先点灯，再观察浮台的往返。":"夜色初临 · 点亮第一盏灯，雾里的路就会出现。",3);startAudio();syncUI()}
function restartLevel(){restartCount++;clearKeys();loadLevel(level.id);phase="PLAYING";toast("这一章重新开始，港口仍在等你。");event("LevelRestarted");syncUI()}
function goHome(){clearKeys();phase="TITLE";loadLevel("rooftops");endingLit=false;syncUI()}
function pause(){if(phase!=="PLAYING")return;phase="PAUSED";clearKeys();menuIndex=0;event("Paused");syncUI();applyVolume();$("resumeButton").focus()}
function resume(){phase="PLAYING";clearKeys();event("Resumed");syncUI();applyVolume()}
function toast(s,seconds=2.8){toastMessage=s;toastTime=seconds}
function snapCamera(){cameraX=clamp(player.x-6,0,level.length-W/S);cameraY=clamp(player.y-4.6,0,1.1)}
function count(kind){return level.lanterns.filter(l=>l.kind===kind&&lit.has(l.id)).length}
function getNearest(){let best=null,dist=1.25;for(const l of level.lanterns){if(lit.has(l.id))continue;const d=Math.hypot(l.x-player.x,l.y-player.y);if(d<dist){best=l;dist=d}}if(level.id==="docks"&&Math.hypot(level.exit-player.x,3.3-player.y)<1.8)return{id:"lighthouse",kind:"lighthouse",x:level.exit,y:3.3};return best}
function light(l){if(!l)return;if(l.kind==="lighthouse"){if(count("main")<3){toast("还需点亮 "+(3-count("main"))+" 盏引路灯 · 向左寻找暖光标记");return}if(endingLit)return;endingLit=true;phase="ENDING";timer=2;clearKeys();recordResult();profile.completed=true;save();bell(true);event("LighthouseLit");for(let i=0;i<26;i++)particles.push({x:level.exit,y:7,age:0,seed:i+1,life:2});return}
if(lit.has(l.id))return;lit.add(l.id);groups.add(l.group);stats.lightInteractions++;igniteTime=.4;if(l.kind==="main"){checkpoint={id:l.id,...l.spawn};checkpointsUsed.add(l.id);toast("引路灯已点亮 · 隐藏平台显现 · 检查点已记录");event("CheckpointChanged",{id:l.id})}else toast("支路灯已点亮 · 这片屋檐也有了归途");
for(let i=0;i<12;i++)particles.push({x:l.x,y:l.y+.8,age:0,seed:i+frameNo,life:1.4});bell();event("LanternLit",{id:l.id,group:l.group})}
function recordResult(){results[level.id]={main:count("main"),optional:count("optional"),falls:fallCount}}
function die(){phase="DYING";timer=.48;fallCount++;totalFalls++;clearKeys();event("PlayerFell",{checkpoint:checkpoint.id});tone(220,.3,.11,"sine")}
function respawn(){player={...checkpoint,vx:0,vy:0,grounded:true,facing:1,coyote:.1,buffer:0,cut:false,standing:null,jumpStartY:checkpoint.y};motionClock=0;for(const p of platforms){p.x=p.baseX-(p.kind==="moving"?p.amp:0);p.oldX=p.x;p.delta=0;p.age=0}phase="PLAYING";clearKeys();snapCamera();toast("回到引路灯旁 · 已亮的灯和平台都还在");event("Respawned",{checkpoint:checkpoint.id})}
function fixedUpdate(){
frameNo++;if(phase==="TITLE"){visualTime+=DT;pressed.clear();return}if(phase==="PAUSED"||phase==="SELECT"||phase==="VICTORY"){pressed.clear();return}
visualTime+=DT;if(toastTime>0)toastTime-=DT;igniteTime=Math.max(0,igniteTime-DT);landTime=Math.max(0,landTime-DT);
particles=particles.filter(p=>(p.age+=DT)<p.life);
if(phase==="DYING"){timer-=DT;if(timer<=0)respawn();pressed.clear();return}
if(phase==="TRANSITION"){timer-=DT;if(timer<=0){loadLevel("docks");phase="PLAYING";toast("潮声里的灯 · 先在岸边观察浮台，再起跳。",4)}pressed.clear();return}
if(phase==="ENDING"){timer-=DT;if(timer<=0){phase="VICTORY";event("RunCompleted");syncUI()}pressed.clear();return}
if(phase!=="PLAYING"){pressed.clear();return}
elapsed+=DT;motionClock+=DT;
for(const p of platforms){if(p.group&&groups.has(p.group)&&p.visibility<1)p.visibility=Math.min(1,p.visibility+DT/.35);p.enabled=p.visibility>=.8;p.oldX=p.x;if(p.kind==="moving"&&p.enabled){p.age+=DT;p.x=p.baseX-p.amp*Math.cos(p.age*Math.PI*2/p.period)}p.delta=p.x-p.oldX}
if(player.standing){const support=platforms.find(p=>p.id===player.standing);if(support&&support.enabled){player.x+=support.delta;if(support.kind==="moving")stats.rideDistance+=Math.abs(support.delta)}}
const right=held.has("ArrowRight")||held.has("KeyD"),left=held.has("ArrowLeft")||held.has("KeyA");
const axis=(right?1:0)-(left?1:0);if(axis)player.facing=axis;
const wasGrounded=player.grounded;const jumpDown=pressed.has("Space"),jumpHeld=held.has("Space");
player.buffer=jumpDown?.12:Math.max(0,player.buffer-DT);
player.coyote=player.grounded?.1:Math.max(0,player.coyote-DT);
if(player.buffer>0&&player.coyote>0){player.vy=7;player.grounded=false;player.standing=null;player.buffer=0;player.coyote=0;player.cut=false;player.jumpStartY=player.y;stats.jumps++;event("Jumped");tone(380,.1,.018,"triangle")}
if(!jumpHeld&&player.vy>0&&!player.cut){player.vy*=.45;player.cut=true}
const target=axis*4.6,accel=axis?34:44;player.vx+=clamp(target-player.vx,-accel*DT,accel*DT);
const oldX=player.x,oldY=player.y;player.x=clamp(player.x+player.vx*DT,.28,level.length-.28);stats.distance+=Math.abs(player.x-oldX);
player.vy-=9.81*DT;player.y+=player.vy*DT;player.grounded=false;player.standing=null;
if(player.vy<=0){let top=-Infinity,surface=null;for(const p of platforms){if(!p.enabled)continue;if(player.x+.23>p.x&&player.x-.23<p.x+p.w&&oldY>=p.y-.045&&player.y<=p.y+.001&&p.y>top){top=p.y;surface=p}}if(surface){player.y=top;player.vy=0;player.grounded=true;player.standing=surface.id;if(!wasGrounded){stats.landings++;landTime=.12;if(surface.kind==="moving"){stats.movingLandings++;event("MovingPlatformLanded",{id:surface.id})}if(Math.abs(player.vx)>.5)tone(level.id==="docks"?130:170,.07,.022,"triangle")}}}
stats.jumpPeak=Math.max(stats.jumpPeak,player.y-player.jumpStartY);
if(player.y<-.65){die();pressed.clear();return}
nearest=getNearest();if(pressed.has("KeyE"))light(nearest);
if(phase==="PLAYING"&&level.id==="rooftops"&&player.x>level.exit&&player.grounded){if(count("main")===3){recordResult();phase="TRANSITION";timer=1.25;clearKeys();if(!profile.unlockedLevelIds.includes("docks")){profile.unlockedLevelIds.push("docks");save()}event("LevelCompleted",{id:level.id})}else toast("还需点亮 "+(3-count("main"))+" 盏引路灯 · 向左寻找标记")}
const desired=clamp(player.x-8.0+player.facing*1.4,0,level.length-W/S);cameraX=mix(cameraX,desired,1-Math.exp(-8*DT));cameraY=mix(cameraY,clamp(player.y-4.8,0,1.1),1-Math.exp(-4*DT));if(player.y<1)cameraY=Math.max(0,cameraY-DT);
updateAudio();pressed.clear();
}
function state(){return{phase,levelId:level.id,player:{x:round(player.x),y:round(player.y),vx:round(player.vx),vy:round(player.vy),grounded:player.grounded,facing:player.facing,standingOn:player.standing,animation:phase==="DYING"?"respawning":igniteTime>0?"ignite":!player.grounded?(player.vy>0?"rise":"fall"):Math.abs(player.vx)>.2?"walk":"idle"},lights:{main:count("main"),mainTotal:3,optional:count("optional"),optionalTotal:1},litLanternIds:[...lit],revealedGroupIds:[...groups],platforms:platforms.map(p=>({id:p.id,x:round(p.x),y:p.y,width:p.w,kind:p.kind,group:p.group,enabled:p.enabled,visibility:round(p.visibility)})),checkpoint:{...checkpoint},activeCheckpointId:checkpoint.id,fallCount,totalFalls,restartCount,elapsedActiveSeconds:round(elapsed),won:phase==="VICTORY",lighthouseLit:endingLit,stats:{...stats,distance:round(stats.distance),rideDistance:round(stats.rideDistance),jumpPeak:round(stats.jumpPeak)},results:{...results},profile:{unlockedLevelIds:[...profile.unlockedLevelIds],completed:profile.completed,settings:{...profile.settings}},events:[...events],camera:{x:round(cameraX),y:round(cameraY)},frame:frameNo,routeValidation:routeValidation(),replayReport}}
function routeValidation(){const report={};for(const d of definitions){const main=d.platforms.filter(p=>!p[0].includes("b"));let maxGap=0,maxRise=0;const issues=[];for(let i=1;i<main.length;i++){const a=main[i-1],b=main[i],gap=b[1]-(a[1]+a[2]);maxGap=Math.max(maxGap,gap);maxRise=Math.max(maxRise,b[3]-a[3]);if(b[4]){const lamp=d.lanterns.find(l=>l.group===b[4]);if(!lamp||!main.slice(0,i).some(p=>!p[4]&&lamp.x>=p[1]&&lamp.x<=p[1]+p[2]))issues.push(b[0]+":switch")}}
report[d.id]={mainJumps:main.length-1,maxRestGap:round(maxGap),maxRise:round(maxRise),groupErrors:issues}}return report}
// This replay is a deterministic physics check. It only supplies normal control
// states to fixedUpdate; it never changes player poses, lights, phases or results.
let replayReport=null;
function replayJourney(withOptional=true){
initialize();startNew();let driven=0;const log=[];
function step(keys=[]){const next=new Set(keys);for(const k of next)if(!held.has(k))pressed.add(k);held.clear();for(const k of next)held.add(k);fixedUpdate();driven++}
function toward(x){return player.x<x-.12?"KeyD":player.x>x+.12?"KeyA":null}
function walkTo(getX){for(let i=0;i<300;i++){const x=getX(),k=toward(x);if(!k&&Math.abs(player.vx)<.16){step();return true}step(k?[k]:[]);if(phase!=="PLAYING")return false}return false}
function illuminateOn(p){for(const l of level.lanterns){if(lit.has(l.id)||l.x<p.baseX||l.x>p.baseX+p.w||Math.abs(l.y-p.y)>.05)continue;if(!walkTo(()=>l.x))return false;step(["KeyE"]);step();for(let j=0;j<25;j++)step();if(!lit.has(l.id))return false}return true}
const paths=withOptional?{
rooftops:["r0","r1","r2","r3","rb1","rb2","rb3","rb4","r6","r7","r8","r9","r10","r11"],
docks:["d0","d1","d2","d3","d4","d5","db1","db2","db3","d7","d8","d9","d10","d11"]}:{
rooftops:["r0","r1","r2","r3","r4","r5","r6","r7","r8","r9","r10","r11"],
docks:["d0","d1","d2","d3","d4","d5","d6","d7","d8","d9","d10","d11"]};
let error="";
for(const id of ["rooftops","docks"]){
if(level.id!==id){error="transition:"+level.id;break}
const path=paths[id];
for(let i=0;i<path.length;i++){
const p=platforms.find(a=>a.id===path[i]);if(!illuminateOn(p)){error="light:"+p.id;break}
if(i===path.length-1)break;
const target=platforms.find(a=>a.id===path[i+1]);
let ready=false;
for(let j=0;j<500;j++){
const takeoff=()=>target.x<=p.x+p.w-.5?clamp(target.x+Math.min(target.w/2,1.3),p.x+.55,p.x+p.w-.6):p.x+p.w-.6;
if(!walkTo(takeoff)){error="approach:"+p.id;break}
if(target.x-(p.x+p.w)<3.15){ready=true;break}
step();
}
if(!ready){error=error||"timing:"+p.id;break}
let landed=false;
for(let j=0;j<155;j++){
const k=toward(target.x+Math.min(target.w/2,1.3));step(k?["Space",k]:["Space"]);
if(j>3&&player.grounded&&player.standing===target.id){landed=true;break}
if(phase!=="PLAYING")break;
}
step();log.push({level:id,from:p.id,to:target.id,landed});
if(!landed){error="jump:"+p.id+"->"+target.id;break}
}
if(error)break;
if(id==="rooftops"){if(!walkTo(()=>level.exit+.2)&&phase!=="TRANSITION"){error="exit";break}for(let j=0;j<100&&phase==="TRANSITION";j++)step();if(level.id!=="docks"){error="next-level";break}}
else if(!walkTo(()=>level.exit-.2)){error="lighthouse-approach";break}
}
clearKeys();replayReport={passed:!error,error,drivenFrames:driven,landings:log,mode:withOptional?"optional":"main"};syncUI();render();return state()
}
window.__demo={reset:initialize,tick(frames=1){for(let i=0;i<clamp(Math.floor(Number(frames)||0),0,36000);i++)fixedUpdate();syncUI();render();return state()},getState:state};
function visible(id,on){$(id).classList.toggle("hidden",!on)}
function syncUI(){
visible("titleScreen",phase==="TITLE");visible("pauseScreen",phase==="PAUSED");visible("selectScreen",phase==="SELECT");visible("victoryScreen",phase==="VICTORY");
visible("hud",!["TITLE","SELECT","VICTORY"].includes(phase));visible("pauseButton",["PLAYING","PAUSED"].includes(phase));visible("transition",phase==="TRANSITION");visible("deathFlash",phase==="DYING");visible("continueButton",profile.unlockedLevelIds.includes("docks"));
$("soundToggle").textContent=profile.settings.muted?"声音 关":"声音 开";
$("musicVolume").value=Math.round(profile.settings.musicVolume*100);$("sfxVolume").value=Math.round(profile.settings.sfxVolume*100);
$("chapterNumber").textContent=level.id==="rooftops"?"01":"02";$("chapterName").textContent=level.title;$("mainLabel").textContent=level.name+" · 引路灯";$("mainCount").textContent=count("main");$("optionalCount").textContent=count("optional")+" / 1";
$("lampTrack").innerHTML=level.lanterns.filter(l=>l.kind==="main").map(l=>'<span class="lamp-dot '+(lit.has(l.id)?"lit":"")+'" aria-label="'+(lit.has(l.id)?"已亮":"未亮")+'"></span>').join("");
$("checkpointLabel").textContent=checkpoint.id==="start"?"起点":"已存检查点";
$("routeDot").style.left=(clamp(player.x/level.length,0,1)*100)+"%";$("routeEnd").textContent=level.id==="rooftops"?"码头 →":"灯塔 →";
$("toast").textContent=toastMessage;$("toast").classList.toggle("visible",toastTime>0);
nearest=phase==="PLAYING"?getNearest():null;visible("context",!!nearest);
if(nearest){$("context").querySelector("span").textContent=nearest.kind==="lighthouse"?(count("main")===3?"点亮灯塔":"灯塔 · 尚缺引路灯"):nearest.kind==="main"?"点亮引路灯":"点亮支路灯 · 可选";$("context").style.left=clamp(sx(nearest.x)/W*100,13,87)+"%";$("context").style.top=clamp(sy(nearest.y+2.2)/H*100,25,65)+"%"}
let guide=level.id==="rooftops"?"灯火显露前路。前方的引路灯，是新的归处。":"浮台会水平往返，先在岸边看好时机，再按住空格起跳。";
if(count("main")===0)guide="先走近屋缘的引路灯，按 E，让雾里的路显现。";
if(count("main")===3)guide=level.id==="rooftops"?"三盏引路灯已亮。沿屋檐向右，前往码头。":"三盏引路灯已亮。去最右侧的灯塔，按 E 为归航留灯。";
if(player.y>5.6)guide="◇ 高处藏着一盏支路灯。它是今夜额外的温柔。";
$("guidance").textContent=guide;
if(phase==="VICTORY"){const main=Object.values(results).reduce((s,v)=>s+v.main,0),opt=Object.values(results).reduce((s,v)=>s+v.optional,0);$("results").innerHTML='<div>引路灯<strong>'+main+' / 6</strong></div><div>支路灯<strong>'+opt+' / 2</strong></div><div>跌落<strong>'+totalFalls+'</strong></div><div>巡行用时<strong>'+Math.floor(elapsed/60)+':'+String(Math.floor(elapsed%60)).padStart(2,"0")+'</strong></div>'}
}
function menuButtons(){const root=phase==="TITLE"?$("titleScreen"):phase==="PAUSED"?$("pauseScreen"):phase==="SELECT"?$("selectScreen"):phase==="VICTORY"?$("victoryScreen"):null;return root?[...root.querySelectorAll("button")].filter(b=>!b.classList.contains("hidden")):[]}
function handleMenu(code){if(code==="Escape"){if(phase==="PLAYING")pause();else if(phase==="PAUSED")resume();else if(phase==="SELECT"){phase="TITLE";syncUI()}return true}
if(!["TITLE","PAUSED","VICTORY","SELECT"].includes(phase))return false;
const buttons=menuButtons();if(["ArrowDown","KeyS","ArrowUp","KeyW"].includes(code)){const active=buttons.indexOf(document.activeElement);menuIndex=(active+(["ArrowDown","KeyS"].includes(code)?1:-1)+buttons.length)%buttons.length;buttons[menuIndex]?.focus();return true}
if(code==="Enter"||code==="Space"){const active=buttons.includes(document.activeElement)?document.activeElement:buttons[menuIndex]||buttons[0];active?.click();return true}return false}
window.addEventListener("keydown",e=>{const code=e.code||e.key;if(["Space","ArrowLeft","ArrowRight","ArrowUp","ArrowDown","Enter","Escape"].includes(code)&&!["INPUT"].includes(document.activeElement?.tagName))e.preventDefault();if(e.repeat)return;if(code==="KeyM"){e.preventDefault();$("soundToggle").click();return}if(code==="F8"){e.preventDefault();replayJourney(!e.shiftKey);return}if(handleMenu(code))return;if(phase==="PLAYING"){held.add(code);pressed.add(code);stats.keyboardInputs++}});
window.addEventListener("keyup",e=>{held.delete(e.code||e.key)});
window.addEventListener("blur",()=>{clearKeys();pause()});document.addEventListener("visibilitychange",()=>{if(document.hidden){clearKeys();pause()}});
$("startButton").onclick=()=>startNew();$("resumeButton").onclick=resume;$("restartButton").onclick=restartLevel;$("homeButton").onclick=goHome;$("replayButton").onclick=()=>startNew();$("victoryHome").onclick=goHome;$("pauseButton").onclick=()=>phase==="PAUSED"?resume():pause();
$("continueButton").onclick=()=>{phase="SELECT";menuIndex=0;syncUI();$("roofSelect").focus()};$("roofSelect").onclick=()=>startNew();$("dockSelect").onclick=()=>{if(profile.unlockedLevelIds.includes("docks"))startNew("docks")};$("selectBack").onclick=()=>{phase="TITLE";syncUI()};
$("soundToggle").onclick=()=>{profile.settings.muted=!profile.settings.muted;startAudio();save();syncUI();applyVolume()};
for(const [id,key]of [["musicVolume","musicVolume"],["sfxVolume","sfxVolume"]])$(id).oninput=e=>{profile.settings[key]=Number(e.target.value)/100;save();applyVolume()};
let audioContext=null,musicGain=null,sfxGain=null,nextNote=0,noteIndex=0;
function startAudio(){try{if(!audioContext){const AC=window.AudioContext||window.webkitAudioContext;if(!AC)return;audioContext=new AC();musicGain=audioContext.createGain();sfxGain=audioContext.createGain();musicGain.connect(audioContext.destination);sfxGain.connect(audioContext.destination);
const buffer=audioContext.createBuffer(1,audioContext.sampleRate*4,audioContext.sampleRate),data=buffer.getChannelData(0);let smooth=0;for(let i=0;i<data.length;i++){smooth=(smooth+(random(i)-.5)*.09)/1.015;data[i]=smooth}const src=audioContext.createBufferSource();src.buffer=buffer;src.loop=true;const filter=audioContext.createBiquadFilter();filter.type="lowpass";filter.frequency.value=440;const gain=audioContext.createGain();gain.gain.value=.28;src.connect(filter);filter.connect(gain);gain.connect(musicGain);src.start();}if(audioContext.state==="suspended")audioContext.resume().catch(()=>{});applyVolume()}catch(_){audioContext=null}}
function applyVolume(){if(!audioContext)return;const mute=profile.settings.muted;musicGain.gain.setTargetAtTime(mute?0:profile.settings.musicVolume*(phase==="PAUSED"?.2:.55),audioContext.currentTime,.08);sfxGain.gain.setTargetAtTime(mute?0:profile.settings.sfxVolume,audioContext.currentTime,.04)}
function tone(freq,duration=.5,volume=.07,type="sine",music=false,delay=0){if(!audioContext||profile.settings.muted)return;try{const at=audioContext.currentTime+delay,osc=audioContext.createOscillator(),gain=audioContext.createGain();osc.type=type;osc.frequency.setValueAtTime(freq,at);gain.gain.setValueAtTime(.0001,at);gain.gain.exponentialRampToValueAtTime(volume,at+.015);gain.gain.exponentialRampToValueAtTime(.0001,at+duration);osc.connect(gain);gain.connect(music?musicGain:sfxGain);osc.start(at);osc.stop(at+duration+.03)}catch(_){}}
function bell(big=false){for(const [i,n]of(big?[261.63,329.63,392,523.25]:[523.25,659.25,783.99]).entries())tone(n,big?2:.95,big?.1:.08,"sine",false,i*.09)}
function updateAudio(){if(!audioContext)return;if(elapsed>nextNote){nextNote=elapsed+3.2;const notes=[261.63,329.63,392,293.66,349.23,329.63,220,293.66];tone(notes[noteIndex++%notes.length],2.5,.085,"triangle",true);tone(130.81,3,.03,"sine",true,.2)}applyVolume()}
function sx(x){return(x-cameraX)*S}function sy(y){return BASE-(y-cameraY)*S}
function line(points,color,width=1){ctx.beginPath();points.forEach((p,i)=>i?ctx.lineTo(...p):ctx.moveTo(...p));ctx.strokeStyle=color;ctx.lineWidth=width;ctx.stroke()}
function polygon(points,color,stroke=null,width=1){ctx.beginPath();points.forEach((p,i)=>i?ctx.lineTo(...p):ctx.moveTo(...p));ctx.closePath();ctx.fillStyle=color;ctx.fill();if(stroke){ctx.strokeStyle=stroke;ctx.lineWidth=width;ctx.stroke()}}
function ellipse(x,y,rx,ry,color){ctx.beginPath();ctx.ellipse(x,y,rx,ry,0,0,Math.PI*2);ctx.fillStyle=color;ctx.fill()}
function glow(x,y,r,strength=1){const g=ctx.createRadialGradient(x,y,0,x,y,r);g.addColorStop(0,"rgba(239,190,100,"+(.25*strength)+")");g.addColorStop(.25,"rgba(231,165,87,"+(.16*strength)+")");g.addColorStop(1,"rgba(231,165,87,0)");ctx.fillStyle=g;ctx.fillRect(x-r,y-r,r*2,r*2)}
function lighthouse(x,y,scale=1,litNow=false){ctx.save();ctx.translate(x,y);ctx.scale(scale,scale);if(litNow){ctx.save();const grad=ctx.createLinearGradient(0,-280,-700,-200);grad.addColorStop(0,"#f3d59155");grad.addColorStop(1,"#f3d59100");polygon([[-15,-260],[-800,-360],[-800,-100]],grad);polygon([[15,-260],[700,-330],[700,-120]],grad);ctx.restore();glow(0,-256,140,1.5)}
polygon([[-42,0],[-26,-230],[26,-230],[43,0]],"#d0c8b3","#a8b6ac",1);polygon([[13,0],[15,-230],[26,-230],[43,0]],"#92a3a0");polygon([[-37,-40],[-33,-72],[32,-72],[36,-40]],"#ba785b");polygon([[-30,-151],[-28,-182],[28,-182],[31,-151]],"#ba785b");
for(let i=0;i<3;i++){ctx.fillStyle="#344c53";ctx.fillRect(-6,-45-i*62,12,20);if(litNow){ctx.fillStyle="#e9b969";ctx.fillRect(-4,-43-i*62,8,16)}}
ctx.fillStyle="#8d9d97";ctx.fillRect(-36,-237,72,8);polygon([[-26,-238],[-26,-274],[26,-274],[26,-238]],litNow?"#f1d193":"#6b8586","#e7ddc3",2);line([[-9,-272],[-9,-239]],"#607c7e",2);line([[10,-272],[10,-239]],"#607c7e",2);
polygon([[-37,-274],[0,-298],[37,-274]],"#c77958","#efcfaa",1.5);ctx.fillStyle="#d7b991";ctx.fillRect(-1,-312,2,16);ellipse(0,-314,3,3,"#e6c58d");ctx.fillStyle="#304a52";ctx.beginPath();ctx.roundRect(-11,-30,22,31,[10,10,0,0]);ctx.fill();line([[-48,-233],[48,-233]],"#e6d7ba",2);line([[-46,-237],[-46,-248]],"#e6d7ba",2);line([[46,-237],[46,-248]],"#e6d7ba",2);ctx.restore()}
function background(title=false){
const sky=ctx.createLinearGradient(0,0,0,H);sky.addColorStop(0,"#243e4a");sky.addColorStop(.52,"#526f76");sky.addColorStop(.78,"#829692");sky.addColorStop(1,"#375760");ctx.fillStyle=sky;ctx.fillRect(0,0,W,H);
for(let i=0;i<70;i++){const x=random(i+2)*W,y=random(i+80)*330;ellipse(x,y,i%7===0?1.6:.8,i%7===0?1.6:.8,"rgba(243,235,221,"+(.2+random(i)*.35)+")")}
const mx=title?930:1010-cameraX*1.4;glow(mx,132,115,.8);ellipse(mx,132,36,36,"#ede4c9");ellipse(mx-9,125,29,29,"#e5dec5");for(let i=0;i<13;i++)ellipse(mx-25+random(i+8)*45,105+random(i+11)*45,random(i+100)*4+1,random(i+200)*3+1,"#c6cbb655");
for(let k=0;k<2;k++){const off=(title?0:cameraX*S*(k?.16:.075));for(let i=-2;i<22;i++){const x=i*86-off%(86*8)+(k?17:0),ht=45+random(i+30+k*60)*110,y=489-ht+k*22,w=68+random(i+29)*30;
ctx.fillStyle=k?"#3d5961":"#4e6b70";ctx.fillRect(x,y,w,ht+70);polygon([[x-5,y],[x+w*.5,y-20-random(i+5)*20],[x+w+6,y]],k?"#52666a":"#607978");ctx.fillStyle="#4b666a";ctx.fillRect(x+10,y-35,7,27);
for(let a=0;a<3;a++)for(let b=0;b<3;b++)if(random(i*14+a*4+b+5)>.42){ctx.fillStyle=(endingLit||title)&&random(i+a+b)>.4?"#d0af73bb":"#81948d55";ctx.fillRect(x+12+a*20,y+18+b*25,7,10)}
}}
ctx.save();ctx.globalAlpha=.55;lighthouse(title?1135:1180-cameraX*2.1,505,.68,endingLit);ctx.restore();
const water=ctx.createLinearGradient(0,513,0,H);water.addColorStop(0,"#718b89");water.addColorStop(.4,"#3b6068");water.addColorStop(1,"#264951");ctx.fillStyle=water;ctx.fillRect(0,517,W,H-517);
for(let i=0;i<90;i++){const x=random(i+600)*W,y=528+random(i+360)*187,len=8+random(i+275)*65;line([[x+Math.sin(visualTime*.4+i)*5,y],[x+len,y]],i%5===0?"#d9c8a631":"#aababa20",i%4===0?2:1)}
for(let i=0;i<4;i++){const x=((i*370+160-(title?0:cameraX*5))%1600+1600)%1600-150,y=545+(i%2)*28;boat(x,y,.7+(i%2)*.3)}
for(let i=0;i<5;i++){const x=100+i*216-(title?0:cameraX*.5),y=174+Math.sin(i*8)*40;line([[x-8,y],[x-3,y-3],[x,y],[x+3,y-3],[x+8,y]],"#d9dfcb50",1)}
}
function boat(x,y,s){ctx.save();ctx.translate(x,y);ctx.scale(s,s);polygon([[-58,0],[51,0],[35,17],[-40,17]],"#2d4b55");line([[0,-74],[0,0]],"#334f57",2);polygon([[4,-70],[4,-8],[43,-8]],"#90a29d66");line([[-48,4],[40,4]],"#c6c0a455",1);ctx.restore()}
function roof(x,y,w,seed,branch=false,hidden=false){
const colors=["#a06a53","#aa7359","#9e715b","#b17a5b"],fill=hidden?"#b38361":colors[Math.abs(seed)%4];
if(branch){polygon([[x-4,y+1],[x+w+4,y+1],[x+w,y+15],[x+3,y+17]],fill);line([[x,y-1],[x+w,y-1]],"#f3ebdd",3);line([[x+6,y+16],[x+14,y+38],[x+30,y+16]],"#47616a",4);return}
const bodyHeight=w>250?260:24;polygon([[x+9,y+20],[x+w-8,y+20],[x+w-12,y+bodyHeight],[x+5,y+bodyHeight]],w>250?"#53636a":"#6b6c65");
if(w>250){ctx.fillStyle="#476069";ctx.fillRect(x+w*.68,y+56,w*.21,150);for(let a=0;a<Math.floor(w/64);a++)for(let b=0;b<2;b++){const wx=x+27+a*62,wy=y+66+b*76;ctx.fillStyle="#304e57";ctx.fillRect(wx,wy,22,33);if(random(seed+a*5+b)>.4){ctx.fillStyle="#e9ba7277";ctx.fillRect(wx+3,wy+3,16,26);glow(wx+10,wy+15,40,.25)}line([[wx+11,wy],[wx+11,wy+32]],"#7f8173",2)}}
polygon([[x-7,y+2],[x+w+7,y+2],[x+w+2,y+30],[x+2,y+33]],fill,"#d8ab8155",1);
for(let r=0;r<3;r++)for(let a=0;a<Math.ceil(w/23);a++){const xx=x+a*23+(r%2?11:0),yy=y+7+r*9;line([[xx,yy],[xx+18,yy+Math.sin(a+seed)*2]],"#ebc49d40",1);line([[xx+19,yy-5],[xx+17,yy+3]],"#644d4744",1)}
line([[x-5,y],[x+w*.3,y-1.5],[x+w*.65,y+.4],[x+w+5,y-1]],hidden?"#f1d29b":"#e5d8bb",3);
if(w>270){const cx=x+w*.78;polygon([[cx,y],[cx,y-42],[cx+22,y-42],[cx+22,y]],"#65726e");ctx.fillStyle="#a08e75";ctx.fillRect(cx-4,y-46,30,7);line([[cx+3,y-25],[cx+18,y-25]],"#9c9c85",1)}
if(hidden){line([[x+18,y+34],[x+w/2,y+46],[x+w-18,y+34]],"#deba8c99",1.3);ellipse(x+w/2,y+50,3,6,"#e2b278")}
}
function dock(x,y,w,seed,moving=false,branch=false){
if(moving){ellipse(x+w/2,y+22,w*.56,12,"#243f4c88");polygon([[x-7,y+8],[x+w+7,y+8],[x+w-4,y+25],[x+3,y+25]],"#ac7958");for(const at of[x+15,x+w-15]){ellipse(at,y+17,9,13,"#c77958");line([[at-9,y+16],[at+9,y+16]],"#efd8af",2)}}
else if(!branch){for(let at=x+18;at<x+w;at+=90){polygon([[at-5,y+5],[at+5,y+5],[at+3,680],[at-7,680]],"#425b5d");line([[at-3,y+20],[at-3,660]],"#9b9c8055",2)}}
polygon([[x-3,y],[x+w+4,y],[x+w,y+16],[x,y+18]],"#9d8163","#d2bc91",1);
for(let at=x+6;at<x+w;at+=21){line([[at,y+2],[at-1,y+13]],"#645f52",1);line([[at+3,y+6],[at+17,y+7]],"#d6bd9033",1)}
line([[x-4,y-1],[x+w+4,y-1]],"#f1e4c6",3);
if(!moving&&!branch&&w>250){for(let at=x+22;at<x+w-10;at+=130){line([[at,y],[at,y-32]],"#63706a",8);ellipse(at,y-34,7,3,"#bbaa89");if(at+130<x+w)line([[at,y-24],[at+65,y-11],[at+130,y-24]],"#a2a18a",2)}}
if(branch){line([[x+6,y+16],[x+w*.45,y+33],[x+w-8,y+16]],"#c1ab84",2)}
}
function drawPlatform(p){if(p.visibility===0)return;const x=sx(p.x),y=sy(p.y),w=p.w*S;if(x>W+100||x+w<-100)return;ctx.save();ctx.globalAlpha=p.visibility;if(p.group&&p.visibility<1)glow(x+w/2,y,w,.6);
if(level.id==="rooftops")roof(x,y,w,p.id.charCodeAt(p.id.length-1),p.kind==="branch",!!p.group);else dock(x,y,w,1,p.kind==="moving",p.kind==="branch");
if(p.kind==="moving"){for(let i=0;i<3;i++){const yy=y+33+i*5;line([[x-12-i*4,yy],[x+w+12+i*4,yy]],"#d7ccab"+(i===0?"60":"22"),1)}}
ctx.restore()}
function lantern(x,y,isLit,kind="main",index=1,scale=1){
ctx.save();ctx.translate(x,y);ctx.scale(scale,scale);if(isLit)glow(0,-53,135,1+.1*Math.sin(visualTime*2));line([[0,0],[0,-91]],"#b1ac8c",4);line([[-10,0],[10,0]],"#d8c69e",4);line([[-1,-90],[22,-90],[22,-78]],"#c8bea2",3);line([[22,-78],[22,-72]],"#e1cda3",1.5);
const bx=22,by=-58;polygon([[bx-13,by-13],[bx-17,by],[bx-11,by+17],[bx+11,by+17],[bx+17,by],[bx+13,by-13]],isLit?"#efc27b":"#708b89",isLit?"#f5dfaa":"#bec2a2",1.5);
line([[bx-10,by-14],[bx+10,by-14]],"#c99d68",4);line([[bx-9,by+17],[bx+9,by+17]],"#b18c61",3);line([[bx,by+19],[bx,by+28]],"#d8ba83",1.5);ellipse(bx,by+30,2,4,"#d7ab72");
for(const off of[-7,0,7])line([[bx+off,by-9],[bx+off*1.2,by+12]],isLit?"#fff2c366":"#c6c4aa33",1);if(isLit)ellipse(bx,by,4,8,"#fff0be");
ctx.font='12px "Microsoft YaHei",sans-serif';ctx.textAlign="center";ctx.fillStyle=isLit?"#f7ddb0":"#dbe0cc";ctx.fillText(kind==="optional"?"◇":["I","II","III"][index-1]||"I",22,-106);
if(kind==="optional"){line([[-8,-92],[-18,-83],[-8,-74],[2,-83],[-8,-92]],"#e1bd86",1.5)}
ctx.restore()}
function character(x,y,facing=1,mode="idle",scale=1){
ctx.save();ctx.translate(x,y);ctx.scale(scale*facing,scale);const walk=mode==="walk"?Math.sin(visualTime*13):0,bob=mode==="walk"?Math.abs(walk)*2:Math.sin(visualTime*2)*.7;
ellipse(0,2,22,4,"#18363d55");ctx.translate(0,-bob);
line([[-7,-13],[-8+walk*8,-2],[-2+walk*8,-2]],"#e0d5bd",6);line([[7,-14],[8-walk*8,-2],[14-walk*8,-2]],"#bdbda9",6);
const tail=mode==="rise"?-8:mode==="fall"?-3:Math.sin(visualTime*5)*3;
polygon([[-11,-46],[12,-43],[15,-24],[23,-7],[-22+tail,-6],[-18,-28]],"#c77958","#e4aa7f",1.2);
polygon([[-11,-42],[-2,-33],[-3,-9],[-22+tail,-6],[-18,-28]],"#b26951");line([[-14,-16],[-6,-12]],"#eab58a66",1.5);line([[6,-35],[9,-16]],"#e6ac7f88",1);
ellipse(2,-48,14,13,"#e6d8bb");polygon([[-15,-52],[-11,-62],[3,-66],[15,-57],[18,-50]],"#d89b73","#e8bd92",1);polygon([[-18,-51],[18,-51],[19,-47],[-17,-46]],"#f3ebdd");ellipse(9,-47,1.7,2,"#3a4a4b");line([[12,-42],[15,-43]],"#a77c64",1);
polygon([[-12,-42],[11,-40],[8,-34],[-10,-36]],"#eee4cf");const handY=mode==="ignite"?-40:-24;
line([[9,-34],[17,handY],[26,handY]],"#d8a07a",6);ellipse(26,handY,3.4,3.4,"#f0dbb8");line([[30,handY],[30,handY+7]],"#d4b97e",1);glow(30,handY+16,45,.85);polygon([[23,handY+8],[37,handY+8],[39,handY+25],[22,handY+25]],"#e8bd76","#f0dcac",1);line([[24,handY+25],[36,handY+25]],"#b98958",2);ellipse(30,handY+16,3,5,"#fff0c1");
ctx.restore()}
const fogCanvas=document.createElement("canvas");fogCanvas.width=W;fogCanvas.height=H;const fctx=fogCanvas.getContext("2d");
function fog(title=false){
fctx.clearRect(0,0,W,H);for(let n=0;n<3;n++){const y=490+n*67;fctx.beginPath();fctx.moveTo(0,y);for(let x=0;x<=W;x+=40)fctx.lineTo(x,y+Math.sin(x*.007+visualTime*.12+n*2)*14+Math.cos(x*.019+n)*7);fctx.lineTo(W,y+90);fctx.lineTo(0,y+90);fctx.closePath();const grad=fctx.createLinearGradient(0,y-20,0,y+100);grad.addColorStop(0,"#b6c7bc00");grad.addColorStop(.5,n===0?"#c6cec225":"#b8cbbd1c");grad.addColorStop(1,"#b6c7bc00");fctx.fillStyle=grad;fctx.fill()}
if(!title){fctx.globalCompositeOperation="destination-out";for(const l of level.lanterns){if(!lit.has(l.id))continue;const x=sx(l.x),y=sy(l.y+.7),g=fctx.createRadialGradient(x,y,20,x,y,180);g.addColorStop(0,"#000");g.addColorStop(1,"#0000");fctx.fillStyle=g;fctx.fillRect(x-180,y-180,360,360)}fctx.globalCompositeOperation="source-over"}ctx.drawImage(fogCanvas,0,0)}
function drawExit(){if(level.id==="rooftops"){const x=sx(88.5),y=sy(3.8);line([[x,y],[x,y-85]],"#b9b28f",4);polygon([[x-57,y-86],[x+56,y-86],[x+69,y-70],[x+56,y-55],[x-57,y-55]],"#c77958","#e4c39a",1);ctx.fillStyle="#f3ebdd";ctx.font='14px "Microsoft YaHei",sans-serif';ctx.textAlign="center";ctx.fillText("前往码头 →",x,y-66);line([[x-25,y+4],[x+24,y+4]],"#d9c9a4",2)}else lighthouse(sx(level.exit),sy(3.3),1,endingLit)}
function scenery(){if(level.id==="rooftops"){for(const id of["r3","r6","r8"]){const p=platforms.find(p=>p.id===id),x=sx(p.x+p.w*.65),y=sy(p.y);if(x<-100||x>W+100)continue;line([[x,y-8],[x+60,y-8]],"#b7b49a",1);for(let j=0;j<4;j++){polygon([[x+j*14,y-8],[x+j*14+8,y-8],[x+j*14+6,y+5]],j%2?"#be835e":"#c9c5a7")}}}else{for(const id of["d2","d5","d7"]){const p=platforms.find(p=>p.id===id),x=sx(p.x+1),y=sy(p.y);if(x<-150||x>W+150)continue;ctx.fillStyle="#867d63";ctx.fillRect(x,y-30,34,30);line([[x,y-29],[x+34,y-1]],"#c6b795",2);line([[x,y-1],[x+34,y-29]],"#c6b795",2);for(let i=0;i<3;i++){ctx.strokeStyle="#a6ad91";ctx.lineWidth=2;ctx.beginPath();ctx.ellipse(x+50+i*2,y-12,14-i*2,8-i,0,0,Math.PI*2);ctx.stroke()}}}}
function render(){
ctx.clearRect(0,0,W,H);const title=phase==="TITLE"||phase==="SELECT";background(title);
if(title){roof(700,496,390,3);roof(1165,570,180,1);line([[700,496],[675,534]],"#dfc49c",2);lantern(1008,496,true,"main",1,1.1);character(849,494,1,"idle",1.55);fog(true);glow(1015,435,180,.4);for(let i=0;i<5;i++){const x=740+i*52,y=230+Math.sin(visualTime*.5+i)*8;ellipse(x,y,1,1,"#e9b96966")}return}
const sorted=[...platforms].sort((a,b)=>a.y-b.y);for(const p of sorted)drawPlatform(p);scenery();drawExit();
for(const l of level.lanterns){const x=sx(l.x);if(x<-180||x>W+180)continue;const index=level.lanterns.filter(a=>a.kind==="main").indexOf(l)+1;lantern(x,sy(l.y),lit.has(l.id),l.kind,index)}
const mode=igniteTime>0?"ignite":!player.grounded?(player.vy>0?"rise":"fall"):Math.abs(player.vx)>.2?"walk":"idle";if(phase!=="DYING"||timer>.24){ctx.save();if(phase==="DYING")ctx.globalAlpha=timer/.48;character(sx(player.x),sy(player.y),player.facing,mode,landTime>0?1-.05*landTime/.12:1);ctx.restore()}
for(const p of particles){const t=p.age/p.life,x=sx(p.x)+Math.sin(p.seed*3)*t*65,y=sy(p.y)-t*75+Math.cos(p.seed)*t*15;ellipse(x,y,(1-t)*2.5,(1-t)*2.5,"rgba(243,216,150,"+(1-t)+")")}
fog();const waterShade=ctx.createLinearGradient(0,658,0,H);waterShade.addColorStop(0,"#18343f00");waterShade.addColorStop(1,"#142d38bb");ctx.fillStyle=waterShade;ctx.fillRect(0,658,W,62);
}
let last=0,accumulator=0;
function loop(t){if(!last)last=t;const gap=(t-last)/1000;last=t;if(gap>.75){if(phase==="PLAYING")pause();accumulator=0}else{accumulator+=Math.min(gap,5*DT);let steps=0;while(accumulator>=DT&&steps++<5){fixedUpdate();accumulator-=DT}}syncUI();render();requestAnimationFrame(loop)}
initialize();requestAnimationFrame(loop);if(storageFailed)$("storageNote").classList.remove("hidden");
})();