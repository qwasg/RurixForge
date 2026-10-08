// Real-time public commands prepare an earned T2 floor for browser interaction QA.
// Never advances the clock, replaces snapshots, or supplies free resources.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const base=process.argv[2],output=process.argv[3];
assert.ok(base&&output&&!fs.existsSync(output));
const branch=process.argv[4]??'algorithm';
assert.ok(['speed','security','algorithm','science','lightweight'].includes(branch));
const records=[];
const log=(event,detail)=>{records.push({at:new Date().toISOString(),event,...detail});fs.writeFileSync(output,JSON.stringify(records,null,2));console.log(event);};
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function api(route,body){const response=await fetch(base+'/api/v6/'+route,{method:body===undefined?'GET':'POST',headers:body===undefined?{}:{'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body)});const data=await response.json();assert.ok(response.ok,JSON.stringify(data));return data;}
const initial=await api('status'),sessionId=initial.session.roomId;
assert.equal(initial.session.mode,'solo');assert.equal(initial.snapshot.players.find(p=>p.owner===1).branches[branch],undefined);
let seq=(initial.session.lastSequence||0)+1;
const pos=(x,y,z=0)=>({x,y,z});
const rect=(x,y,w,h,z=0)=>({x,y,z,w,h});
function line(a,b){let p={...a};const points=[p];for(const key of['x','y','z'])while(p[key]!==b[key]){p={...p,[key]:p[key]+Math.sign(b[key]-p[key])};points.push(p);}return points;}
async function cmd(command){const receipt=await api('order',{sessionId,seq:seq++,command});log('order',{command,receipt});assert.equal(receipt.accepted,true);return receipt;}
async function until(condition,label){const deadline=Date.now()+180000;while(Date.now()<deadline){const status=await api('status');assert.equal(status.session.roomId,sessionId);assert.equal(status.snapshot.winner,null);if(condition(status.snapshot))return status.snapshot;await pause(500);}throw Error('Real-time timeout: '+label);}
await cmd({op:'shell',rect:rect(13,46,6,4)});
await cmd({op:'build',kind:'wind-power',pos:pos(13,42)});
await cmd({op:'build',kind:'extractor',pos:pos(16,40)});
let state=await until(s=>s.buildings.some(b=>b.owner===1&&b.kind==='shell'&&b.progress>=1),'shell');
const shell=state.buildings.find(b=>b.owner===1&&b.kind==='shell').id;
await cmd({op:'entrance',kind:'door',pos:pos(13,48),toLevel:0,width:1});
await cmd({op:'room',shell,rect:rect(13,46,2,2),kind:'data-center'});
await cmd({op:'room',shell,rect:rect(17,46,2,2),kind:'research-lab',branch});
state=await until(s=>s.rooms.filter(r=>r.owner===1&&r.progress>=1).length===2,'functional rooms');
const dc=state.rooms.find(r=>r.owner===1&&r.kind==='data-center').id,lab=state.rooms.find(r=>r.owner===1&&r.kind==='research-lab').id;
await cmd({op:'wire',kind:'power',path:[...line(pos(13,42),pos(13,46)),...line(pos(13,46),pos(18,46)).slice(1)]});
await cmd({op:'install-gpu',room:dc,model:'rtx-5060'});
await cmd({op:'wire',kind:'compute',path:line(pos(13,46),pos(18,46))});
await cmd({op:'deploy',room:lab,kind:'vscode',pos:pos(12,52)});
await cmd({op:'deploy',room:lab,kind:'pycharm',pos:pos(10,52)});
await cmd({op:'wire',kind:'compute',path:[...line(pos(13,46),pos(12,46)),...line(pos(12,46),pos(12,52)).slice(1)]});
await cmd({op:'wire',kind:'compute',path:line(pos(12,52),pos(10,52))});
state=await until(s=>s.players.find(p=>p.owner===1).compute>=150,'real compute charge');
const player=state.players.find(p=>p.owner===1);assert.ok(player.credits>=500);log('paid opening verified',{tick:state.tick,player});
await cmd({op:'research',room:lab,branch});
await until(s=>s.players.find(p=>p.owner===1).branches[branch]===2,'normal T2 research');
await cmd({op:'shell',rect:rect(13,46,6,4,1)});
state=await until(s=>s.buildings.some(b=>b.owner===1&&b.kind==='shell'&&b.rect.z===1&&b.progress>=1),'supported upper floor');
const saved=await api('save',{name:'真实付费 T2 楼层交互验收'});
await api('pause',{sessionId,paused:true});
log('ready for browser stairs test',{sessionId,tick:state.tick,shell,dc,lab,branch,saved,scope:'Ordinary paid public commands and real clock. Browser stair interaction remains pending.'});
