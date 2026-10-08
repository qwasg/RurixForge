import net from 'node:net';
import {createHash} from 'node:crypto';
export const canonicalJson=value=>Array.isArray(value)?`[${value.map(canonicalJson).join(',')}]`:value&&typeof value==='object'?`{${Object.keys(value).sort().map(key=>`${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(',')}}`:JSON.stringify(value);
export const stateHash=value=>createHash('sha256').update(canonicalJson(value)).digest('hex');
export class EpochMovement {
 positions=new Map();moved=new Set();current=new Set();epoch=null;
 begin(epoch){this.epoch=epoch;this.positions.clear();this.current.clear();}
 observe(id,position){const encoded=JSON.stringify(position),previous=this.positions.get(id);this.positions.set(id,encoded);if(previous===undefined||previous===encoded)return null;
  const first=!this.current.has(id);this.current.add(id);this.moved.add(`${this.epoch}:${id}`);
  return first?{sessionId:this.epoch,entityId:id,from:JSON.parse(previous),to:position}:null;
 }
}
/** Counts only complete observed intervals, never terminal, stalled or cross-epoch gaps. */
export class ConservativeActivity {
 totalMs=0;previous=null;blockedUntil=0;
 observe(epoch,at,snapshots,stable=true){
  const current={epoch,at,ticks:snapshots.map(s=>s?.tick),active:stable&&at>=this.blockedUntil&&snapshots.length===2&&snapshots.every(s=>s&&s.winner===null&&Number.isSafeInteger(s.tick))};
  const old=this.previous;let added=0;
  if(old?.active&&current.active&&old.epoch===epoch&&at>old.at&&current.ticks.every((tick,i)=>tick>old.ticks[i]))added=at-old.at;
  this.totalMs+=added;this.previous=current;return added;
 }
 // Reset at the actual outage boundary, even when the next delayed sample is
 // already past recovery. blockedUntil uses the same monotonic clock as at.
 reset(blockedUntil=0){this.previous=null;this.blockedUntil=Math.max(this.blockedUntil,blockedUntil);}
 get seconds(){return this.totalMs/1000;}
}
/** Persist the measured row before assertions, retaining the exact states on failure. */
export async function retainTimedSample(sample,states,{persistSample,validate,persistFailure}){
 await persistSample(sample);
 try{return await validate();}
 catch(error){
  try{await persistFailure({sample,states,error});}
  catch(storageError){error.evidenceWriteError=storageError instanceof Error?storageError.message:String(storageError);}
  throw error;
 }
}
/** One diagnostic read per native, only after the measured activity has ended.
 * RPC/storage errors stay in this separate evidence and never decide acceptance.
 */
export class PostActivityMetrics {
 ended=null;collection=null;
 end(reason){this.ended??={reason,at:new Date().toISOString()};}
 async collect(clients,write){
  if(!this.ended)return{status:'skipped-active',clients:[]};
  if(this.collection)return this.collection;
  this.collection=(async()=>{
   const result={scope:'Read-only bounded native timing rings, collected after activity timing ended; not an additional FPS sample or full-run trace.',ended:this.ended,clients:[]};
   result.clients=await Promise.all(clients.map(async c=>{
    const row={client:c.name,enginePid:c.health?.enginePid??null,identity:c.identity??null,collectedAt:new Date().toISOString()};
    try{if(!c.native)throw new Error('native observer unavailable');row.metrics=await c.native.rpc('game.session.metrics',{},10000);}
    catch(error){row.error=error instanceof Error?error.message:String(error);}
    try{await write(`post-activity-metrics-${c.name}.json`,row);}
    catch(error){row.storageError=error instanceof Error?error.message:String(error);}
    return row;
   }));
   return result;
  })();
  return this.collection;
 }
}
/** Additional read-only observer connection to an already-running private localhost host. */
export async function observeNative(port){
 const socket=net.createConnection({host:'127.0.0.1',port});await new Promise((ok,no)=>{socket.once('connect',ok);socket.once('error',no);});
 let buffer=Buffer.alloc(0),id=1;const pending=new Map();
 const fail=error=>{for(const p of pending.values()){clearTimeout(p.timer);p.no(error);}pending.clear();};
 socket.on('error',fail);socket.on('close',()=>fail(new Error('private observer closed')));
 socket.on('data',chunk=>{buffer=Buffer.concat([buffer,chunk]);while(buffer.length>=4){const size=buffer.readUInt32LE();if(size>64*1024*1024){socket.destroy();fail(new Error('oversized native response'));return;}if(buffer.length<size+4)break;let result;try{result=JSON.parse(buffer.subarray(4,size+4));}catch(error){fail(error);socket.destroy();return;}buffer=buffer.subarray(size+4);const p=pending.get(result.id);if(p){pending.delete(result.id);clearTimeout(p.timer);result.error?p.no(new Error(result.error.message)):p.ok(result.result);}}});
 return{rpc(method,params={},timeout=30000){return new Promise((ok,no)=>{const requestId=id++;const timer=setTimeout(()=>{pending.delete(requestId);no(new Error('native observer timeout: '+method));},timeout);pending.set(requestId,{ok,no,timer});const data=Buffer.from(JSON.stringify({jsonrpc:'2.0',id:requestId,method,params})),head=Buffer.alloc(4);head.writeUInt32LE(data.length);socket.write(Buffer.concat([head,data]));});},close(){socket.destroy();}};
}
