import { useCallback, useEffect, useRef, useState } from 'react';
import { apiGet, callTool } from './forgeApi';
import { useWorkspaceStore, type ForgeWorkspace } from './workspaceStore';
import { openViewportStream, type StreamFrame, type ViewportStreamHandle } from './viewportStream';
import { INITIAL_V2, decodeV2, type V2State } from './sentinelsV2';

export const SENTINELS_FEEDBACK: Record<number,string> = {
  1:'建设经费不足',2:'位置已被占用',3:'先选中已部署的对象',4:'已达到最高等级',5:'技能仍在冷却',6:'目标区域没有敌人',7:'当前入侵尚未结束',8:'指令无法执行',9:'战斗已结束',
  10:'部署完成 · 通路已更新',11:'单元升级完成',12:'单元已回收',13:'技能释放 · 算力已扣除',14:'入侵开始，守住核心',15:'战区净化完成',16:'核心失守',17:'入侵已清除 · 可以重新布防',18:'目标策略已切换',
  20:'显卡已接入供能网络',21:'显卡升级 · 产能提升',22:'显卡已回收',23:'算力不足以释放技能',30:'先部署显卡，为防线供能',31:'请预留 130 经费安装首张显卡',32:'战区尚未解锁',33:'先完成当前战区',
  34:'此处无法部署',35:'敌人占据此位置',36:'已达到 24 单元上限',37:'部署会封死通路，请更换位置',38:'最后一张供能显卡无法回收',40:'结构重写 · Boss 改变了地形',41:'Boss 崩解 · 通路重新计算',
};

/** Native game connection only. The browser never simulates combat or predicts purchases. */
export function useSentinelsRuntime() {
  const [workspace,setWorkspace]=useState<ForgeWorkspace|null>(null);
  const [state,setState]=useState<V2State>(INITIAL_V2);
  const [ready,setReady]=useState(false),[nativeReady,setNativeReady]=useState(false),[busy,setBusy]=useState(false);
  const [sessionGeneration,setSessionGeneration]=useState(0);
  const [connectionEpoch,setConnectionEpoch]=useState(0);
  const [connected,setConnected]=useState(false),[paused,setPaused]=useState(false),[fps,setFps]=useState(0);
  const [error,setError]=useState(''),[notice,setNotice]=useState('建立供能网络，然后部署守护者');
  const canvasRef=useRef<HTMLCanvasElement>(null),streamRef=useRef<ViewportStreamHandle|null>(null),frame=useRef<StreamFrame|null>(null);
  const epoch=useRef(0),pauseLock=useRef(false),startLock=useRef(false);
  useEffect(()=>{
    let active=true;
    void apiGet<{workspaces:ForgeWorkspace[]}>('/api/forge/workspaces').then(({workspaces})=>{
      const id=new URLSearchParams(location.search).get('workspace');
      const found=id?workspaces.find(w=>w.id===id):workspaces.find(w=>/[\\/]code-sentinels(?:[\\/]|$)/i.test(w.root));
      if(!found)throw new Error('找不到编译防线工作区');
      if(active){useWorkspaceStore.getState().setActive(found.id);setWorkspace(found);}
    }).catch(e=>{if(active)setError((e as Error).message);});return()=>{active=false;};
  },[]);
  const start=useCallback(async()=>{
    if(!workspace||startLock.current)return false;
    startLock.current=true;setBusy(true);setReady(false);setNativeReady(false);setConnected(false);setError('');setNotice('正在接入原生战场…');epoch.current++;frame.current=null;
    try{
      useWorkspaceStore.getState().setActive(workspace.id);
      const summary=await callTool<{playState:string}>('scene_summary');
      if(summary.playState!=='edit')await callTool('play_exit');
      await callTool('asset_reload');await callTool('scene_load',{path:'Content/Scenes/Main.rxscene'});
      await callTool('viewport_set_camera',{target:[0,0,0],yaw:0,pitch:0,dist:14,ortho:true,orthoSize:7});
      await callTool('play_enter');setState(INITIAL_V2);setPaused(false);setSessionGeneration(g=>g+1);setReady(true);setNotice('显卡手牌已展开 · 1–7 选卡，Enter 快速接入基地');return true;
    }catch(e){setError((e as Error).message);return false;}
    finally{startLock.current=false;setBusy(false);}
  },[workspace]);
  useEffect(()=>{
    if(!ready||!workspace)return;
    let cancelled=false,raf=0,timer=0,lastFrame=-1,invalid=0;
    const generation=++epoch.current;
    const stream=openViewportStream({width:1280,height:720,maxFps:40,onFrame:f=>{frame.current=f;},onStatus:s=>setFps(s.fps),onChannel:up=>{setConnected(up);if(up)setConnectionEpoch(n=>n+1);else {setNativeReady(false);setNotice('连接中断 · 指令不会自动重发');}},onError:setError});
    streamRef.current=stream;
    const draw=()=>{
      const f=frame.current,el=canvasRef.current;
      if(f&&el&&f.frameId!==lastFrame){if(el.width!==f.width||el.height!==f.height){el.width=f.width;el.height=f.height;}const bytes=new Uint8ClampedArray(f.rgba.length);bytes.set(f.rgba);el.getContext('2d')?.putImageData(new ImageData(bytes,f.width,f.height),0,0);lastFrame=f.frameId;}
      raf=requestAnimationFrame(draw);
    };
    const poll=async()=>{
      try{
        const data=await callTool<{entities:Array<{name:string;transform:{translation:number[];scale:number[]}}>}>('entity_list');
        if(cancelled||generation!==epoch.current)return;
        const next=decodeV2(data.entities);
        if(!next){setNativeReady(false);if(++invalid>5)throw new Error('原生战场尚未发布完整状态');return;}
        invalid=0;setNativeReady(true);setState(next);if(next.feedback)setNotice(SENTINELS_FEEDBACK[next.feedback]??'指令已执行');
      }catch(e){if(!cancelled&&generation===epoch.current)setError((e as Error).message);}
      finally{if(!cancelled&&generation===epoch.current)timer=window.setTimeout(()=>void poll(),400);}
    };
    raf=requestAnimationFrame(draw);void poll();
    return()=>{cancelled=true;cancelAnimationFrame(raf);clearTimeout(timer);stream.close();if(streamRef.current===stream)streamRef.current=null;};
  },[ready,workspace,sessionGeneration]);
  const send=useCallback((command:number)=>{
    if(!nativeReady||busy||!streamRef.current?.up){setNotice('连接未就绪，指令未发送');return false;}
    if(paused){setNotice('先继续战斗，再下达指令');return false;}
    const sent=streamRef.current.sendInput('cs',command);if(!sent)setNotice('连接中断，指令未发送');return sent;
  },[nativeReady,busy,paused]);
  const togglePause=useCallback(async()=>{
    if(!ready||busy||pauseLock.current)return;pauseLock.current=true;
    try{await callTool(paused?'play_resume':'play_pause');setPaused(!paused);}catch(e){setError((e as Error).message);}finally{pauseLock.current=false;}
  },[ready,busy,paused]);
  useEffect(()=>{
    if(!ready||state.phase<2||!paused)return;
    let active=true;void callTool('play_resume').then(()=>{if(active)setPaused(false);}).catch(e=>{if(active)setError((e as Error).message);});return()=>{active=false;};
  },[ready,state.phase,paused]);
  return{workspace,state,ready,nativeReady,connectionEpoch,busy,connected,paused,fps,error,notice,setNotice,start,send,togglePause,canvasRef};
}
