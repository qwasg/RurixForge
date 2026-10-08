/** V5 visual/lifecycle publications layered over unchanged V4 gameplay state. */
import { decodeV4, type V4State } from './sentinelsV4';
export * from './sentinelsV4';

export const V5_EVENT_LIMIT = 256;
export const V5_GHOST_LIMIT = 24;
export const V5_EVENT = { land:1, workStart:2, workStop:3, buildingHit:4, buildingDestroy:5, unitHit:6, unitAttack:7, impact:8, skill:9, wallHit:10, shieldAbsorb:11, linkBreak:12, repair:13, upgrade:14, wallLand:15, wallDestroy:16, unitDestroy:17 } as const;
export const BUILDING_ANIMATION = { landStart:0, landFrames:32, landFps:16, workStart:32, workFrames:48, workFps:16, destroyStart:80, destroyFrames:48, destroyFps:24, tailSeconds:2.5, stride:128 } as const;
export const V5_FX = [
  {id:1,name:'kinetic-hit',frames:32,fps:32,blend:'additive'},
  {id:2,name:'plasma-hit',frames:32,fps:24,blend:'additive'},
  {id:3,name:'heavy-impact',frames:48,fps:24,blend:'alpha'},
  {id:4,name:'shield-hit',frames:32,fps:32,blend:'additive'},
  {id:5,name:'power-arc',frames:32,fps:24,blend:'additive'},
  {id:6,name:'repair',frames:48,fps:24,blend:'additive'},
  {id:7,name:'upgrade',frames:48,fps:24,blend:'additive'},
  {id:8,name:'collapse-explosion',frames:48,fps:24,blend:'alpha'},
] as const;
export interface V5AnimationEvent { seq:number; type:number; kind:number; x:number; y:number; age:number; duration:number; subject:number; owner:number; magnitude:number; fxKind:number; active:boolean }
export interface V5SubjectAnimation { slot:number; phase:number; frame:number; workAge:number; phaseAge:number; kind:number; hitTime:number }
export interface V5AnimationState { time:number; eventSeq:number; ghostCount:number; fxCount:number; events:V5AnimationEvent[]; buildings:V5SubjectAnimation[]; units:V5SubjectAnimation[] }
export interface V5State extends V4State { animation:V5AnimationState }
export const INITIAL_V5_ANIMATION:V5AnimationState={time:0,eventSeq:0,ghostCount:0,fxCount:0,events:[],buildings:[],units:[]};
type Entity={name:string;transform:{translation:number[];scale:number[]}};
export function decodeV5(input:{entities:Entity[]}|Entity[]):V5State|null {
  const state=decodeV4(input);if(!state)return null;
  const entities=new Map((Array.isArray(input)?input:input.entities).map(e=>[e.name,e]));
  const fields=(name:string):number[]=>{const e=entities.get(name);const out=[...(e?.transform?.translation??[]),...(e?.transform?.scale??[])];if(out.length!==6||out.some(n=>!Number.isFinite(n)))throw Error('Incomplete V5 visual state');return out;};
  try{
    const global=fields('C5_Global');const animation:V5AnimationState={time:global[0],eventSeq:global[1],ghostCount:global[2],fxCount:global[3],events:[],buildings:[],units:[]};
    for(let slot=0;slot<48;slot++){const a=fields(`C5_BuildingAnim${slot}`);animation.buildings.push({slot,phase:a[0],frame:a[1],workAge:a[2],phaseAge:a[3],kind:a[4],hitTime:a[5]});}
    for(let slot=0;slot<32;slot++){const a=fields(`C5_UnitAnim${slot}`);animation.units.push({slot,phase:a[0],frame:a[1],workAge:a[2],phaseAge:a[3],kind:a[4],hitTime:a[5]});}
    for(let slot=0;slot<V5_EVENT_LIMIT;slot++){const a=fields(`C5_Event${slot}`),b=fields(`C5_EventMeta${slot}`);if(a[0]>0)animation.events.push({seq:a[0],type:a[1],kind:a[2],x:a[3],y:a[4],age:a[5],duration:b[0],subject:b[1],owner:b[2],magnitude:b[3],fxKind:b[4],active:b[5]>0.5});}
    animation.events.sort((a,b)=>a.seq-b.seq);return {...state,animation};
  }catch{return null;}
}
