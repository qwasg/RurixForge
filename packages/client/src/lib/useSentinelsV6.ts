import {useCallback,useEffect,useRef,useState} from 'react';
import {parseFrameMessage,type StreamFrame} from './viewportStream';
import {V6_PROTOCOL,normalizeV6Catalog,validateV6Snapshot,type V6Catalog,type V6Command,type V6Receipt,type V6SessionOptions,type V6Snapshot,type V6Status} from './sentinelsV6';
import type {V6Camera} from './sentinelsV6Geometry';

class V6HttpError extends Error{constructor(message:string,readonly status:number){super(message);}}

export async function v6Request<T>(path:string,body?:unknown,signal?:AbortSignal):Promise<T>{
  const response=await fetch('/api/v6/'+path,{method:body===undefined?'GET':'POST',headers:body===undefined?undefined:{'content-type':'application/json'},body:body===undefined?undefined:JSON.stringify(body),cache:'no-store',signal});
  const result=await response.json().catch(()=>({error:{message:'游戏服务没有返回有效数据'}}));
  if(!response.ok)throw new V6HttpError(typeof result.error==='string'?result.error:result.error?.message??result.message??`请求失败 (${response.status})`,response.status);
  return result as T;
}
async function loadV6Catalog():Promise<V6Catalog>{
  const catalog=normalizeV6Catalog(await v6Request('catalog'));
  // Presentation aliases come from the shipped media manifest; gameplay values remain native-owned.
  const manifest=await fetch('/games/code-sentinels/ui-v6/resource-manifest.json',{cache:'no-store'}).then(r=>r.ok?r.json():null).catch(()=>null);
  return attachV6Media(catalog,manifest);
}
type V6MediaManifest={version?:number;characters?:Record<string,{portrait?:string}>;models?:Record<string,{image?:string}>;facilityAliases?:Record<string,string>;chassisAliases?:Record<string,string>;roleModelMap?:Record<string,{model?:string}>};
export function attachV6Media(catalog:V6Catalog,manifest:V6MediaManifest|null):V6Catalog{
  if(manifest?.version===6)for(const item of catalog.items){
    const candidates=[item.id,item.chassis,manifest.facilityAliases?.[item.id],manifest.chassisAliases?.[item.chassis??item.id],manifest.roleModelMap?.[item.id]?.model];
    const actor=manifest.characters?.[item.id],model=candidates.filter((id):id is string=>!!id).map(id=>manifest.models?.[id]).find(m=>m?.image);
    if(actor?.portrait)item.image=actor.portrait;else if(model?.image)item.image=model.image;
  }
  return catalog;
}

export function useSentinelsV6(){
  const [status,setStatus]=useState<V6Status>({protocol:V6_PROTOCOL,playerId:null,session:null,snapshot:null});
  const [catalog,setCatalog]=useState<V6Catalog>({items:[]});
  const [connected,setConnected]=useState(false),[streaming,setStreaming]=useState(false),[busy,setBusy]=useState(false);
  const [error,setError]=useState(''),[viewportError,setViewportError]=useState(''),[notice,setNotice]=useState('选择行动，建立你的第一座平面基地。'),[fps,setFps]=useState(0);
  const [generation,setGeneration]=useState(0),[receipt,setReceipt]=useState<V6Receipt|null>(null);
  const canvasRef=useRef<HTMLCanvasElement>(null),frameRef=useRef<StreamFrame|null>(null),seq=useRef(0),epoch=useRef(0);
  const queue=useRef<Promise<unknown>>(Promise.resolve()),tick=useRef(-1),abort=useRef<AbortController|null>(null);
  const uncertain=useRef<{seq:number;command:V6Command;sessionId:string}|null>(null);
  const activeSession=useRef<string|null>(null);
  const authorityReady=useRef(false);
  const authorityOnline=(session:V6Status['session'])=>session?.connected!==false&&!(session?.mode==='join'&&session.status!=='lobby'&&session.replicaSynced===false);
  const authorityError=(session:V6Status['session'])=>session?.error||'与房主的连接中断，正在恢复同步。';
  const accept=useCallback((next:V6Status)=>{
    if(next.protocol!==V6_PROTOCOL)throw Error('游戏通信协议不匹配，请启动同一版本的V6游戏服务。');
    const snapshot=next.snapshot?validateV6Snapshot(next.snapshot):null;
    const sessionId=next.session?.roomId??null;
    if(activeSession.current!==sessionId){activeSession.current=sessionId;seq.current=next.session?.lastSequence??0;uncertain.current=null;}
    else seq.current=Math.max(seq.current,next.session?.lastSequence??0);
    if(uncertain.current?.sessionId!==next.session?.roomId)uncertain.current=null;
    tick.current=snapshot?.tick??-1;
    authorityReady.current=authorityOnline(next.session);
    setStatus({...next,snapshot});setConnected(authorityReady.current);setError(authorityReady.current?'':authorityError(next.session));
  },[]);
  const refresh=useCallback(async()=>{
    try{accept(await v6Request<V6Status>('status'));setCatalog(await loadV6Catalog());}
    catch(e){authorityReady.current=false;setConnected(false);setError((e as Error).message);}
  },[accept]);
  useEffect(()=>{void refresh();return()=>{epoch.current++;abort.current?.abort();};},[refresh]);

  const sessionId=status.session?.roomId;
  useEffect(()=>{
    if(!sessionId)return;
    let cancelled=false,timer=0,failures=0;const controller=new AbortController();abort.current=controller;const pollEpoch=epoch.current;
    const poll=async()=>{
      try{
        const data=await v6Request<{snapshot:V6Snapshot|null;session:V6Status['session'];unchanged?:boolean}>('snapshot?afterTick='+tick.current,undefined,controller.signal);
        if(cancelled||pollEpoch!==epoch.current)return;
        const snapshot=data.snapshot?validateV6Snapshot(data.snapshot):null;
        seq.current=Math.max(seq.current,data.session?.lastSequence??0);
        const online=authorityOnline(data.session);authorityReady.current=online;
        if(uncertain.current&&online){
          const pending=uncertain.current;
          if(pending.sessionId!==data.session?.roomId)uncertain.current=null;
          else{
            const recovered=await v6Request<V6Receipt>('order',pending,controller.signal);
            if(cancelled||pollEpoch!==epoch.current)return;
            uncertain.current=null;seq.current=Math.max(seq.current,recovered.sequence);setReceipt(recovered);
            setNotice(recovered.reason||'已确认断线前的指令，后续未发送操作可重新下达。');
          }
        }
        if(snapshot&&snapshot.tick>=tick.current){tick.current=snapshot.tick;setStatus(s=>({...s,snapshot,session:data.session??s.session}));}
        else if(data.session)setStatus(s=>({...s,session:data.session}));
        failures=0;setConnected(online);setError(online?'':authorityError(data.session));
      }catch(e){if(!cancelled&&pollEpoch===epoch.current){failures++;authorityReady.current=false;setConnected(false);setError((e as Error).message);}}
      finally{if(!cancelled&&pollEpoch===epoch.current)timer=window.setTimeout(()=>void poll(),failures?Math.min(5000,250*2**Math.min(failures,5)):100);}
    };
    void poll();return()=>{cancelled=true;controller.abort();window.clearTimeout(timer);};
  },[sessionId,generation]);

  useEffect(()=>{
    setViewportError('');
    if(!sessionId||status.session?.status==='lobby')return;
    let cancelled=false,socket:WebSocket|null=null,timer=0,raf=0,lastFrame=-1;
    const connect=async()=>{
      try{
        const info=await v6Request<{wsUrl:string}>('viewport');if(cancelled)return;
        const url=new URL(info.wsUrl);if(!['127.0.0.1','localhost','[::1]'].includes(url.hostname))throw Error('画面服务必须在本机运行');
        socket=new WebSocket(info.wsUrl);socket.binaryType='arraybuffer';
        socket.onopen=()=>{if(!cancelled)socket?.send(JSON.stringify({type:'subscribe',width:1280,height:720,maxFps:40}));};
        socket.onmessage=event=>{
          if(cancelled)return;
          if(event.data instanceof ArrayBuffer){const frame=parseFrameMessage(event.data);if(frame){frameRef.current=frame;setStreaming(true);setViewportError(frame.truncated?'画面绘制数量超限':'');}}
          else if(typeof event.data==='string'){try{const data=JSON.parse(event.data);if(data.type==='status')setFps(data.fps??0);if(data.type==='error')setViewportError(data.message);}catch{/* ignore non-protocol text */}}
        };
        socket.onerror=()=>{};
        socket.onclose=()=>{setStreaming(false);if(!cancelled)timer=window.setTimeout(()=>void connect(),1500);};
      }catch(e){if(!cancelled){setStreaming(false);setViewportError((e as Error).message);timer=window.setTimeout(()=>void connect(),1500);}}
    };
    const draw=()=>{
      const frame=frameRef.current,canvas=canvasRef.current;
      if(frame&&canvas&&frame.frameId!==lastFrame){if(canvas.width!==frame.width)canvas.width=frame.width;if(canvas.height!==frame.height)canvas.height=frame.height;
        canvas.getContext('2d')?.putImageData(new ImageData(new Uint8ClampedArray(frame.rgba),frame.width,frame.height),0,0);lastFrame=frame.frameId;}
      raf=requestAnimationFrame(draw);
    };
    void connect();raf=requestAnimationFrame(draw);
    return()=>{cancelled=true;window.clearTimeout(timer);cancelAnimationFrame(raf);setStreaming(false);frameRef.current=null;socket?.close();};
  },[sessionId,status.session?.status==='lobby',generation]);

  const begin=useCallback(async(options:V6SessionOptions)=>{
    setBusy(true);setError('');epoch.current++;seq.current=0;tick.current=-1;queue.current=Promise.resolve();uncertain.current=null;
    try{accept(await v6Request<V6Status>('session',options));setCatalog(await loadV6Catalog());setGeneration(n=>n+1);return true;}
    catch(e){setError((e as Error).message);return false;}finally{setBusy(false);}
  },[accept]);
  const action=useCallback(async(name:string,body:unknown={})=>{
    const replaces=name==='load'||name==='replay'||name==='leave'||(name==='replay-control'&&body&&typeof body==='object'&&'seekTick'in body);
    if(replaces){epoch.current++;abort.current?.abort();}
    setBusy(true);try{const result=await v6Request<unknown>(name,body);await refresh();return result;}catch(e){setError((e as Error).message);return null;}finally{if(replaces)setGeneration(n=>n+1);setBusy(false);}
  },[refresh]);
  const order=useCallback((command:V6Command):Promise<V6Receipt|null>=>{
    if(!connected||!status.snapshot||status.session?.status!=='battle'||status.session.replay||status.session.paused||status.snapshot.winner!==null){setNotice(status.session?.replay?'回放中不能下达战斗指令':status.session?.paused?'战斗已暂停，按空格继续后再下达指令。':'当前战局尚未就绪或已经结束');return Promise.resolve(null);}
    const currentEpoch=epoch.current,sessionId=status.session.roomId;
    const run=async()=>{
      if(currentEpoch!==epoch.current)return null;
      if(!authorityReady.current){setNotice('正在恢复与房主的连接，本次操作尚未发送。');return null;}
      if(uncertain.current){setNotice('上一个操作还在等待确认，本次操作尚未发送。');return null;}
      const sequence=++seq.current;
      try{
        let result:V6Receipt;
        try{result=await v6Request<V6Receipt>('order',{seq:sequence,command,sessionId});}
        catch(e){if(currentEpoch!==epoch.current)return null;if(!(e instanceof TypeError))throw e;result=await v6Request<V6Receipt>('order',{seq:sequence,command,sessionId});}
        if(currentEpoch===epoch.current){setReceipt(result);setNotice(result.reason||(result.accepted?'指令已执行':'指令未通过验证'));}
        return result;
      }catch(e){if(currentEpoch===epoch.current){
        if(e instanceof V6HttpError&&e.status>=400&&e.status<500){seq.current=sequence-1;setNotice((e as Error).message);}
        else{uncertain.current={seq:sequence,command,sessionId};authorityReady.current=false;setConnected(false);setNotice('指令结果待确认，恢复连接后会核对同一操作。');}
        setError((e as Error).message);
      }return null;}
    };
    const pending=queue.current.then(run,run);queue.current=pending;return pending;
  },[connected,status.snapshot,status.session]);
  const camera=useCallback((value:V6Camera)=>v6Request('camera',{centerX:value.x,centerY:value.y,zoom:value.zoom,localPlayer:status.playerId??1}).catch(e=>setError((e as Error).message)),[status.playerId]);
  return {status,snapshot:status.snapshot,session:status.session,playerId:status.playerId??1,catalog,connected,streaming,busy,error:error||viewportError,notice,fps,receipt,canvasRef,begin,action,order,camera,refresh,setNotice,reconnect:()=>setGeneration(n=>n+1)};
}
