import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { ArrowLeft, ArrowRight, BookOpen, Box, Bug, Check, ChevronRight, CircuitBoard, Coins, Cpu, Crosshair, Heart, Layers, Lock, Pause, Play, Plus, RotateCcw, Settings2, Shield, Sparkles, Target, Trash2, Volume2, VolumeX, X, Zap } from 'lucide-react';
import { apiGet, callTool } from '@/lib/forgeApi';
import { useWorkspaceStore, type ForgeWorkspace } from '@/lib/workspaceStore';
import { openViewportStream, type StreamFrame, type ViewportStreamHandle } from '@/lib/viewportStream';
import { GPUS, OPERATORS, BUGS, LEVELS, TERRAIN_NAMES, INITIAL_V2, decodeV2, type V2State, commandDeploy, commandUpgrade, commandSell, commandSkill, commandTarget, commandGpu, commandGpuUpgrade, commandGpuSell, commandNextWave, commandLevel, commandNextLevel, commandSpeed } from '@/lib/sentinelsV2';
import './code-sentinels-v2.css';

const A = '/games/code-sentinels/';
const WORLD_WIDTH = 224 / 9;
const FEEDBACK: Record<number, string> = {
  1: '经费不足。击破敌人或回收设施可获得经费。', 2: '部署位置已占用。', 3: '请先选中有效的单位或显卡。', 4: '已达到最高等级。',
  5: '技能正在冷却。', 6: '目标区域内没有可命中的敌人。', 7: '当前波次尚未结束。', 8: '该指令无法执行。', 9: '战斗已结束，可继续战役或重新部署。',
  10: '防御单元部署成功，敌人已重新计算通路。', 11: '单元升级完成。', 12: '单元已回收，经费已返还。', 13: '技能已释放，算力已扣除。',
  14: '入侵开始。保持显卡供能，守住核心。', 15: '战区已净化，下一战区已解锁。', 16: '核心失守。调整产能与防线后重试。', 17: '本波已清空。可补充显卡与单位，再开启下一波。',
  18: '目标策略已更新。', 20: '显卡上线，正在产出算力。', 21: '显卡升级完成，产能已提升。', 22: '显卡已回收。', 23: '算力不足以释放技能，请等待显卡充能。',
  30: '先在基地部署一块显卡，为防线提供算力。', 31: '请预留 130 经费购买第一块 RTX 5060。', 32: '该战区尚未解锁。', 33: '请先完成当前战区。',
  34: '这里不能部署。请选择基地外的平地、道路、高地或桥。', 35: '敌人占据了这个位置。', 36: '已达到 24 个防御单元上限。', 37: '这次部署会封死入口，请留出可通行路径。',
  38: '请先安装另一块显卡，再回收最后一块供能卡。', 40: 'Boss 改写了数据结构！地形变化，敌人正在重新寻路。', 41: 'Boss 崩解改变了通路，检查新的地形。',
};
const cellName = (cell: number) => `${String.fromCharCode(65 + cell % 24)}${Math.floor(cell / 24) + 1}`;
const position = (cell: number) => ({ left: `${((cell % 24 - 11.5) / WORLD_WIDTH + .5) * 100}%`, top: `${(Math.floor(cell / 24) + .5) / 14 * 100}%` });
const number = (n: number) => Math.max(0, Math.floor(n)).toLocaleString('en-US');
const rate = (n: number) => Math.max(0,n).toLocaleString('en-US',{maximumFractionDigits:1});

export default function CodeSentinelsV2() {
  const [workspace, setWorkspace] = useState<ForgeWorkspace | null>(null);
  const [ready, setReady] = useState(false), [busy, setBusy] = useState(false), [connected, setConnected] = useState(false), [paused, setPaused] = useState(false);
  const [state, setState] = useState<V2State>(INITIAL_V2);
  const [nativeReady,setNativeReady]=useState(false);
  const [card, setCard] = useState(1), [unitSlot, setUnitSlot] = useState<number | null>(null), [gpuSlot, setGpuSlot] = useState(0), [gpuModel, setGpuModel] = useState(1);
  const [panel, setPanel] = useState<'base' | 'unit'>('base'), [aiming, setAiming] = useState(false), [hoverCell, setHoverCell] = useState<number | null>(null);
  const [drawer, setDrawer] = useState<'help' | 'archive' | null>(null), [archiveTab, setArchiveTab] = useState<'gpus' | 'bugs' | 'heroes'>('gpus');
  const [notice, setNotice] = useState('先建立算力基地，再将防线部署到开放战场。'), [error, setError] = useState(''), [fps, setFps] = useState(0);
  const [muted, setMuted] = useState(true), [grid, setGrid] = useState(false);
  const canvas = useRef<HTMLCanvasElement>(null), modal = useRef<HTMLElement>(null), stream = useRef<ViewportStreamHandle | null>(null), frame = useRef<StreamFrame | null>(null);
  const stateRef = useRef(state), pauseLock = useRef(false), audio = useRef<AudioContext | null>(null), epoch = useRef(0), lastRevision = useRef(0);
  const standalone = new URLSearchParams(location.search).get('standalone') === '1';
  const selectedUnit = unitSlot === null ? null : state.units.find((u) => u.slot === unitSlot && u.kind > 0) ?? null;
  const operator = OPERATORS[(selectedUnit?.kind ?? card) - 1];
  const selectedGpu = state.gpus[gpuSlot];
  const model = GPUS[gpuModel - 1];
  const gameOver = ready && state.phase >= 2;
  const usable = ready && nativeReady && connected && !busy && !paused && !gameOver;
  const activeUnits = state.units.filter((u) => u.kind > 0);
  const starved = activeUnits.filter((u) => u.starved).length;

  useEffect(() => { stateRef.current = state; }, [state]);
  useEffect(() => {
    if (ready && state.phase >= 2 && paused) {
      let active=true; void callTool('play_resume').then(()=>{if(active)setPaused(false);}).catch(e=>{if(active)setError((e as Error).message);});
      return()=>{active=false;};
    }
  },[ready,state.phase,paused]);
  useEffect(() => { if (unitSlot !== null && !selectedUnit) {setAiming(false);setUnitSlot(null);} },[unitSlot,selectedUnit]);
  useEffect(() => {setAiming(false);setUnitSlot(null);},[state.level]);
  useEffect(() => {
    document.title = '编译防线 II · 算力前线';
    let cancelled = false;
    void apiGet<{ workspaces: ForgeWorkspace[] }>('/api/forge/workspaces').then(({ workspaces }) => {
      const id = new URLSearchParams(location.search).get('workspace');
      const found = id ? workspaces.find((w) => w.id === id) : workspaces.find((w) => /[\\/]code-sentinels(?:[\\/]|$)/i.test(w.root));
      if (!found) throw new Error('尚未找到编译防线工作区。');
      if (!cancelled) { useWorkspaceStore.getState().setActive(found.id); setWorkspace(found); }
    }).catch((e: Error) => { if (!cancelled) setError(e.message); });
    return () => { cancelled = true; };
  }, []);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const sections = document.querySelectorAll<HTMLElement>('.v2-shell');
    for (const section of sections) section.inert = Boolean(drawer);
    if (drawer) modal.current?.focus();
    return () => { for (const section of sections) section.inert = false; if (drawer) previous?.focus(); };
  }, [drawer]);
  useEffect(() => () => { void audio.current?.close(); }, []);

  const beep = useCallback((tone = 700) => {
    if (muted) return;
    const ctx = audio.current ?? new AudioContext(); audio.current = ctx; void ctx.resume();
    const oscillator = ctx.createOscillator(), volume = ctx.createGain();
    oscillator.frequency.setValueAtTime(tone, ctx.currentTime); oscillator.frequency.exponentialRampToValueAtTime(tone * .6, ctx.currentTime + .12);
    volume.gain.setValueAtTime(.04, ctx.currentTime); volume.gain.exponentialRampToValueAtTime(.001, ctx.currentTime + .16);
    oscillator.connect(volume).connect(ctx.destination); oscillator.start(); oscillator.stop(ctx.currentTime + .17);
  }, [muted]);
  const send = useCallback((command: number) => {
    if (!nativeReady || !stream.current?.up || busy) { setNotice('战场连接尚未就绪，指令未发送。'); return false; }
    if (paused) { setNotice('请先继续战斗。'); return false; }
    const sent = stream.current.sendInput('cs', command);
    if (!sent) setNotice('连接已中断，指令未发送。恢复后请重试。');
    return sent;
  }, [busy, paused, nativeReady]);
  const start = async () => {
    if (!workspace || busy) return;
    setBusy(true); setReady(false); setNativeReady(false); setConnected(false); setError(''); setNotice('正在载入开放战场…'); epoch.current++; frame.current = null;
    try {
      useWorkspaceStore.getState().setActive(workspace.id);
      const summary = await callTool<{playState: string}>('scene_summary');
      if (summary.playState !== 'edit') await callTool('play_exit');
      await callTool('asset_reload');
      await callTool('scene_load', {path:'Content/Scenes/Main.rxscene'});
      await callTool('viewport_set_camera', {target:[0,0,0],yaw:0,pitch:0,dist:14,ortho:true,orthoSize:7});
      await callTool('play_enter');
      setState(INITIAL_V2); setUnitSlot(null); setGpuSlot(0); setGpuModel(1); setPanel('base'); setAiming(false); setPaused(false); setReady(true); lastRevision.current = 0;
      setNotice('基地已就绪。选择显卡并点击「安装到基地」，建立第一条算力供给。');
    } catch (e) { setError((e as Error).message); }
    finally { setBusy(false); }
  };
  useEffect(() => {
    if (!ready || !workspace) return;
    let cancelled = false, animation = 0, timer = 0, lastFrame = -1, invalidCount = 0;
    const current = ++epoch.current;
    const channel = openViewportStream({width:1280,height:720,maxFps:40,onFrame:f => {frame.current=f;},onStatus:s => setFps(s.fps),onChannel:up => setConnected(up),onError:setError});
    stream.current = channel;
    const draw = () => {
      const f = frame.current, el = canvas.current;
      if (f && el && f.frameId !== lastFrame) {
        if (el.width !== f.width || el.height !== f.height) {el.width=f.width;el.height=f.height;}
        const bytes = new Uint8ClampedArray(f.rgba.length); bytes.set(f.rgba);
        el.getContext('2d')?.putImageData(new ImageData(bytes,f.width,f.height),0,0); lastFrame=f.frameId;
      }
      animation = requestAnimationFrame(draw);
    };
    const poll = async () => {
      try {
        const response = await callTool<{entities: Array<{name:string;transform:{translation:number[];scale:number[]}}>}>('entity_list');
        if (cancelled || current !== epoch.current) return;
        const next = decodeV2(response.entities);
        if (!next) {setNativeReady(false);if (++invalidCount > 5) throw new Error('场景状态协议不完整，正在等待完整 V2 战场。'); return;}
        invalidCount = 0; setNativeReady(true); setState(next); stateRef.current = next;
        if (next.feedback) setNotice(FEEDBACK[next.feedback] ?? '指令已执行。');
        if (next.mapRevision !== lastRevision.current) lastRevision.current=next.mapRevision;
      } catch (e) {if (!cancelled && current === epoch.current) setError((e as Error).message);}
      finally {if (!cancelled && current === epoch.current) timer=window.setTimeout(() => void poll(),400);}
    };
    animation=requestAnimationFrame(draw); void poll();
    return () => {cancelled=true; clearTimeout(timer); cancelAnimationFrame(animation); channel.close(); if(stream.current===channel)stream.current=null;};
  }, [ready,workspace]);
  const togglePause = useCallback(async () => {
    if (!ready || busy || pauseLock.current) return; pauseLock.current=true;
    try {await callTool(paused?'play_resume':'play_pause');setPaused(!paused);} catch(e){setError((e as Error).message);} finally {pauseLock.current=false;}
  },[ready,busy,paused]);
  const chooseOperator = useCallback((kind: number) => {setCard(kind);setUnitSlot(null);setPanel('unit');setAiming(false);beep(400+kind*100);},[beep]);
  const aim = useCallback(() => {
    if (!selectedUnit) {setPanel('unit');setNotice('先点击战场中已部署的单元，再释放它的技能。');return;}
    if (!usable) return;
    if (selectedUnit.skillCooldown>0) {setNotice('该单元的技能还在冷却。');return;}
    if (state.energy<selectedUnit.skillCost) {setNotice(`技能需要 ${selectedUnit.skillCost} 算力，请等待显卡充能。`);return;}
    setAiming(a=>!a);setNotice(`在战场上选择「${operator.skillName}」的释放位置。按 Esc 取消。`);
  },[selectedUnit,usable,state.energy,operator.skillName]);
  const clickCell = (cell: number) => {
    if (!usable) return;
    if (aiming) {if(selectedUnit && send(commandSkill(selectedUnit.slot,cell)))beep(280);setAiming(false);return;}
    const existing=state.units.find(u=>u.kind>0&&u.cell===cell);
    if(existing){setUnitSlot(existing.slot);setCard(existing.kind);setPanel('unit');setNotice(`已选中 ${OPERATORS[existing.kind-1].name} · ${cellName(cell)}`);return;}
    if (state.terrain[cell]===5 || cell%24<4) {setPanel('base');setNotice('基地显卡负责供能。在右侧选择一个机架插槽。');return;}
    if (![0,3,4,7].includes(state.terrain[cell])) {setNotice(FEEDBACK[34]);return;}
    if(send(commandDeploy(card,cell)))beep(620);
  };
  const changeLevel=(level:number)=>{if(send(commandLevel(level))){setUnitSlot(null);setAiming(false);setPanel('base');}};
  useEffect(()=>{
    if(!ready)return;
    const key=(e:KeyboardEvent)=>{
      if(e.repeat||busy||(e.target as HTMLElement)?.closest('input,textarea,select'))return;
      if(e.key==='Escape'){setAiming(false);setDrawer(null);return;}
      if(drawer||state.phase>=2)return;
      if(/^[1-4]$/.test(e.key)){e.preventDefault();chooseOperator(Number(e.key));}
      else if(e.code==='Space'){e.preventDefault();void togglePause();}
      else if(e.key.toLowerCase()==='q'){e.preventDefault();aim();}
      else if(e.key.toLowerCase()==='n')send(commandNextWave());
      else if(e.key.toLowerCase()==='b'){setPanel('base');setAiming(false);}
    };window.addEventListener('keydown',key);return()=>window.removeEventListener('keydown',key);
  },[ready,busy,drawer,state.phase,chooseOperator,togglePause,aim,send]);

  return <main className="v2-game"><div className="v2-shell">
    <header className="v2-header"><div className="v2-logo"><span><CircuitBoard size={25}/></span><div><strong>编译防线 <b>II</b></strong><small>COMPUTE FRONTIER</small></div></div><nav className="v2-campaign">{LEVELS.map((name,i)=><button key={name} disabled={!ready||busy||paused||i+1>state.unlocked} className={state.level===i+1?'is-current':''} onClick={()=>changeLevel(i+1)}>{i+1>state.unlocked?<Lock size={11}/>:<span>0{i+1}</span>}{name}{i<2&&<ChevronRight size={11}/>}</button>)}</nav><div className="v2-header-actions"><button aria-label="作战手册" onClick={()=>setDrawer('help')}><BookOpen size={16}/></button><button aria-label="资料图鉴" onClick={()=>setDrawer('archive')}><Layers size={16}/></button>{!standalone&&<a aria-label="返回编辑器" href="/"><ArrowLeft size={17}/></a>}</div></header>
    <section className="v2-statusbar"><div className="v2-resource"><Coins/><span><small>建设经费</small><strong data-testid="v2-credits">{ready?number(state.credits):'—'}</strong></span></div><div className={`v2-resource v2-power ${starved?'is-starved':''}`}><Zap/><span><small>可用算力</small><strong data-testid="v2-energy">{ready?number(state.energy):'—'}<em> / {number(state.capacity)}</em></strong></span><div className="v2-power-track"><i style={{width:`${state.capacity?Math.min(100,state.energy/state.capacity*100):0}%`}}/></div></div><div className="v2-resource"><Cpu/><span><small>基地总产能</small><strong data-testid="v2-production">+{ready?rate(state.production):0}<em> / s</em></strong></span></div><div className="v2-resource"><Heart/><span><small>核心</small><strong data-testid="v2-hp">{ready?number(state.hp):20}<em> / 20</em></strong></span></div><div className="v2-wave"><span>WAVE <strong data-testid="v2-wave">{String(state.wave).padStart(2,'0')}<em> / 04</em></strong></span><small>{state.phase===1?`${number(state.enemies)} 个入侵进程`:'战术准备阶段'}</small></div><div className="v2-transport"><button aria-label="切换速度" disabled={!usable} onClick={()=>send(commandSpeed())}>{state.speed}×</button><button aria-label={paused?'继续战斗':'暂停战斗'} disabled={!ready||busy||gameOver} onClick={()=>void togglePause()}>{paused?<Play size={16}/>:<Pause size={16}/>}</button><button aria-label={muted?'开启音效':'关闭音效'} onClick={()=>setMuted(!muted)}>{muted?<VolumeX size={16}/>:<Volume2 size={16}/>}</button></div></section>
    <div className="v2-main">
      <div className="v2-battle-column"><div className="v2-map-caption"><span><i/> {LEVELS[state.level-1]} <small>OPEN BATTLEFIELD</small></span><div><button aria-pressed={grid} onClick={()=>setGrid(!grid)}><Box size={12}/>部署网格</button><span>{connected?`${Math.round(fps)} FPS`:'原生战场'}</span></div></div>
        <div className={`v2-board ${aiming?'is-aiming':''} ${grid?'has-grid':''}`} data-testid="v2-board">
          {ready?<canvas ref={canvas} width={1280} height={720} aria-label="开放原生战场"/>:<img className="v2-board-cover" src={A+'open-battlefield.png'} alt="开放岩土地形战场"/>}
          {ready&&nativeReady&&!gameOver&&<div className="v2-grid">{state.terrain.map((terrain,cell)=><button key={cell} data-testid={`v2-cell-${cell}`} aria-label={`${cellName(cell)} ${TERRAIN_NAMES[terrain]}`} title={`${cellName(cell)} · ${TERRAIN_NAMES[terrain]}`} disabled={!usable} className={`${[1,2,6].includes(terrain)?'blocked':''} ${selectedUnit?.cell===cell?'is-selected':''}`} style={position(cell)} onMouseEnter={()=>setHoverCell(cell)} onMouseLeave={()=>setHoverCell(null)} onClick={()=>clickCell(cell)}/>)}</div>}
          {ready&&selectedUnit&&!gameOver&&<div className="v2-range" style={{...position(aiming&&hoverCell!==null?hoverCell:selectedUnit.cell),width:`${(aiming?(selectedUnit.kind===1?1.8:3.1):selectedUnit.range)*2/WORLD_WIDTH*100}%`,aspectRatio:'1'}}/>}
          <div className="v2-base-marker"><Shield size={14}/><span>主分支核心<small>COMPUTE BASE</small></span></div>
          {state.bossPhase>0&&state.bossHealth>0&&<div className="v2-boss"><span><Bug size={14}/>{BUGS[4+state.level]?.name} <b>PHASE {state.bossPhase}</b></span><div><i style={{width:`${state.bossHealth*100}%`}}/></div></div>}
          {ready&&state.terrainStage>0&&<div className="v2-terrain-alert"><Layers size={14}/>数据结构已改写 · 地形阶段 {state.terrainStage}<small>敌人按新地形重新寻路</small></div>}
          {aiming&&<div className="v2-aim-label"><Crosshair size={17}/>选择技能落点 · {operator.skillName}<kbd>Esc 取消</kbd></div>}
          {!ready&&<div className="v2-intro"><span className="v2-eyebrow">THE FRONTIER HAS CHANGED</span><h1>供能。部署。<br/><em>改写战局。</em></h1><p>建立你的显卡基地，在会变化的开放战场上，<br/>让每一点算力都成为决定战局的力量。</p><button className="v2-primary" disabled={!workspace||busy} onClick={()=>void start()}>{busy?'正在接入战区…':'启动算力前线'}<ArrowRight size={18}/></button><div><span>7 款真实显卡</span><i/><span>3 个动态战区</span><i/><span>4 组角色技能</span></div></div>}
          {paused&&!gameOver&&<div className="v2-overlay"><Pause size={30}/><h2>战术暂停</h2><p>重新思考产能、阵地与下一次施法。</p><button className="v2-primary" onClick={()=>void togglePause()}>继续战斗<Play size={16}/></button></div>}
          {gameOver&&<div className="v2-overlay"><Shield size={38}/><span className="v2-eyebrow">{state.phase===2?'SECTOR SECURED':'MISSION REPORT'}</span><h2>{state.phase===2?(state.level===3?'全部战区净化完成':'战区已净化'):'核心失守'}</h2><p>{number(state.kills)} 次修复 · 普攻耗能 {number(state.spentAttack)} · 技能耗能 {number(state.spentSkill)}</p>{state.phase===2&&state.level<3&&<button className="v2-primary" disabled={busy||!connected} onClick={()=>{if(send(commandNextLevel())){setUnitSlot(null);setPanel('base');}}}>带着显卡进入下一战区<ArrowRight size={17}/></button>}<button className={state.phase===2?'v2-ghost':'v2-primary'} disabled={busy} onClick={()=>void start()}><RotateCcw size={15}/>{state.phase===2?'重新开始战役':'重新部署'}</button></div>}
        </div>
        <div className={`v2-commandbar ${starved?'is-warning':''}`}><p role="status">{starved>0?<Zap size={14}/>:<ChevronRight size={14}/>}<span>{starved>0&&!state.feedback?`${starved} 个单元因算力不足等待充能。升级或增设显卡。`:notice}</span></p><button disabled={!usable||state.phase!==0} onClick={()=>send(commandNextWave())}>开启下一波<kbd>N</kbd><ArrowRight size={14}/></button></div>
        <div className="v2-dock"><div className="v2-operator-deck"><div className="v2-section-head"><span>防御单元 <small>OPERATORS</small></span><span>{activeUnits.length} / 24</span></div><div className="v2-operators">{OPERATORS.map(o=><button key={o.id} aria-label={`选择 ${o.name}`} aria-pressed={card===o.id&&panel==='unit'} className={card===o.id&&panel==='unit'?'is-selected':''} style={{'--unit':o.color} as CSSProperties} onClick={()=>chooseOperator(o.id)}><kbd>{o.id}</kbd><img src={A+o.image} alt=""/><strong>{o.name}</strong><span><Coins size={10}/>{o.cost}<i/><Zap size={10}/>{o.attackCost}/发</span></button>)}</div></div><div className="v2-economy-summary"><span className="v2-eyebrow">SUPPLY BEFORE FIREPOWER</span><h3>火力，由算力驱动。</h3><p>显卡持续供能；每发普攻与每次技能，都从同一个算力池扣除。</p><div><span>普攻累计<strong>{number(state.spentAttack)}</strong></span><span>技能累计<strong>{number(state.spentSkill)}</strong></span><span>协同等级<strong>{state.combo}</strong></span></div></div></div>
      </div>
      <aside className="v2-sidebar"><div className="v2-tabs"><button aria-pressed={panel==='base'} onClick={()=>{setPanel('base');setAiming(false);}}><CircuitBoard size={15}/>算力基地<kbd>B</kbd></button><button aria-pressed={panel==='unit'} onClick={()=>setPanel('unit')}><Crosshair size={15}/>单元 / 技能</button></div>
        {panel==='base'?<div className="v2-base-panel"><div className="v2-section-head"><span>基地机架</span><span>{state.gpuCount} / 8 在线</span></div><div className="v2-rack">{state.gpus.map(g=><button key={g.slot} aria-label={`显卡插槽 ${g.slot+1}`} aria-pressed={gpuSlot===g.slot} className={`${gpuSlot===g.slot?'is-selected':''} ${g.model?'is-filled':''}`} onClick={()=>{setGpuSlot(g.slot);if(g.model)setGpuModel(g.model);}}>{g.model?<><img src={A+GPUS[g.model-1].image} alt={GPUS[g.model-1].name}/><small>+{rate(g.rate)}/s</small></>:<><Plus size={16}/><small>0{g.slot+1}</small></>}</button>)}</div>
          {selectedGpu?.model>0&&<div className="v2-rack-detail"><span>{GPUS[selectedGpu.model-1].name}<small>机架 {gpuSlot+1} · Lv.{selectedGpu.tier} · +{rate(selectedGpu.rate)}/s</small></span><div><button aria-label="升级显卡" disabled={!usable||selectedGpu.upgradeCost<=0||state.credits<selectedGpu.upgradeCost} onClick={()=>send(commandGpuUpgrade(gpuSlot))}>升级 <Coins size={11}/>{number(selectedGpu.upgradeCost)}</button><button aria-label="回收显卡" disabled={!usable||state.gpuCount<=1} title={`回收返还 ${number(selectedGpu.sellRefund)} 经费`} onClick={()=>send(commandGpuSell(gpuSlot))}><Trash2 size={12}/></button></div></div>}
          <div className="v2-section-head v2-hardware-title"><span>选择显卡 <small>HARDWARE</small></span><button aria-label="查看显卡真实来源" onClick={()=>{setArchiveTab('gpus');setDrawer('archive');}}><BookOpen size={12}/></button></div><div className="v2-gpu-catalog">{GPUS.map(g=><button key={g.id} aria-label={`选择 ${g.name}`} aria-pressed={gpuModel===g.id} className={gpuModel===g.id?'is-selected':''} onClick={()=>setGpuModel(g.id)}><img src={A+g.image} alt={g.name}/><strong>{g.name.replace('GeForce ','').replace(' Blackwell','')}</strong><small>{g.vram} GB <span>+{g.rate}/s</span></small><span className="v2-gpu-cost"><Coins size={10}/>{g.cost}</span>{gpuModel===g.id&&<Check size={12} className="v2-check"/>}</button>)}</div>
          <div className="v2-install"><div><strong>{model.name}</strong><span>游戏产能 +{model.rate}/s · 容量 +{model.capacity}</span></div><button className="v2-primary" disabled={!usable||selectedGpu?.model>0||state.credits<model.cost} onClick={()=>{if(send(commandGpu(gpuSlot,gpuModel)))beep(480);}}>{selectedGpu?.model>0?'选择空插槽安装':'安装到基地'}<span><Coins size={13}/>{model.cost}</span></button>{!state.gpuCount&&<p>开局先安装显卡，才能让 AI 开火。</p>}</div></div>:
        <div className="v2-unit-panel"><div className="v2-profile"><div className={operator.id<3?'is-logo':''}><img src={A+operator.image} alt={operator.name}/></div><span>{operator.role}</span><h2>{operator.name}</h2><p>{operator.description}</p></div><div className="v2-unit-metrics"><span>单发耗能<strong><Zap size={12}/>{selectedUnit?.attackCost??operator.attackCost}</strong></span><span>攻击射程<strong>{(selectedUnit?.range??operator.range).toFixed(1)} 格</strong></span><span>部署经费<strong><Coins size={12}/>{operator.cost}</strong></span></div>
          {selectedUnit?<><div className="v2-selected-unit"><span>{cellName(selectedUnit.cell)} · 单元 {selectedUnit.slot+1}<b>Lv.{selectedUnit.tier}</b></span><div className="v2-hp-track"><i style={{width:`${Math.min(100,selectedUnit.hp/(130+45*(selectedUnit.tier-1))*100)}%`}}/></div><small>{number(selectedUnit.hp)} HP{selectedUnit.jam>0?` · 被死锁干扰 ${selectedUnit.jam.toFixed(1)}s`:selectedUnit.starved?' · 缺少算力，暂时停火':''}</small></div><div className="v2-target-mode"><label htmlFor="target-strategy">目标策略</label><select id="target-strategy" value={selectedUnit.targetMode} disabled={!usable} onChange={e=>send(commandTarget(selectedUnit.slot,Number(e.target.value)))}><option value={0}>优先逼近基地</option><option value={1}>优先最低血量</option><option value={2}>优先 Boss</option></select></div><div className="v2-unit-actions"><button disabled={!usable||selectedUnit.tier>=3||state.credits<selectedUnit.upgradeCost} onClick={()=>send(commandUpgrade(selectedUnit.slot))}>{selectedUnit.tier>=3?'已满级':'升级单元'}<span><Coins size={11}/>{number(selectedUnit.upgradeCost)}</span></button><button aria-label="回收单元" title={`返还 ${number(selectedUnit.sellRefund)} 经费`} disabled={!usable} onClick={()=>{if(send(commandSell(selectedUnit.slot)))setUnitSlot(null);}}><Trash2 size={14}/></button></div></>:<div className="v2-deploy-tip"><Plus size={17}/><p>点击战场空地部署。<br/><small>高地提高射程，墙体与水域阻止通行。</small></p></div>}
          <section className={`v2-skill ${aiming?'is-aiming':''}`} style={{'--unit':operator.color} as CSSProperties}><div><Sparkles size={20}/><span><strong>{operator.skillName}</strong><small>ACTIVE ABILITY <kbd>Q</kbd></small></span></div><p>{operator.skillDescription}</p><div className="v2-skill-cost"><span><Zap size={13}/>{selectedUnit?.skillCost??operator.skillCost} 算力</span><span>{selectedUnit?.skillCooldown?`${Math.ceil(selectedUnit.skillCooldown)}s`:`冷却 ${operator.cooldown}s`}</span></div><button disabled={!selectedUnit||!usable||selectedUnit.skillCooldown>0||state.energy<selectedUnit.skillCost} onClick={aim}>{!selectedUnit?'先选中已部署单元':aiming?'正在选择目标…':selectedUnit.skillCooldown>0?`${Math.ceil(selectedUnit.skillCooldown)}s 后可释放`:'选择技能落点'}<Crosshair size={15}/></button></section>
        </div>}
      </aside>
    </div>
    {error&&<section className="v2-error" role="alert"><strong>战场连接需要恢复</strong><span>{error}</span><button onClick={()=>void start()}>重新接入</button></section>}
    <footer className="v2-footer"><span><CircuitBoard size={12}/>RURIX FORGE · NATIVE 2D</span><span>岩壁阻挡 · 高地射程 · 水域绕行 · Boss 动态改图 <Layers size={12}/></span></footer>
  </div>
  {drawer&&<div className="v2-modal-backdrop" onClick={()=>setDrawer(null)}><section ref={modal} tabIndex={-1} className="v2-modal" role="dialog" aria-modal="true" aria-label={drawer==='help'?'作战手册':'资料图鉴'} onClick={e=>e.stopPropagation()} onKeyDown={e=>{if(e.key==='Escape'){setDrawer(null);return;}if(e.key!=='Tab')return;const items=modal.current?.querySelectorAll<HTMLElement>('button:not(:disabled),a[href],select');if(!items?.length)return;const first=items[0],last=items[items.length-1];if(e.shiftKey&&(document.activeElement===first||document.activeElement===modal.current)){e.preventDefault();last.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first.focus();}}}><button className="v2-modal-close" aria-label="关闭" onClick={()=>setDrawer(null)}><X size={20}/></button><span className="v2-eyebrow">COMPUTE FRONTIER / FIELD ARCHIVE</span><h2>{drawer==='help'?'让算力，成为战略。':'真实来源，清晰机制。'}</h2>
    {drawer==='help'?<><p>你指挥的是一条完整的算力防线：建设显卡 → 储存算力 → 驱动普攻与技能 → 击破错误获得经费 → 扩建基地。</p><ol className="v2-guide"><li><strong>先给基地通电</strong><span>选择空机架、选择显卡并安装。起始算力为零，只有显卡会持续产出算力；击杀奖励经费。</span></li><li><strong>在开放地图构筑防线</strong><span>按 1–4 选择守护者，点击平地、道路、高地或桥部署。不能部署水中或岩壁，也不能堵死所有通路。</span></li><li><strong>关注供能，不只堆火力</strong><span>每发普攻都消耗算力。算力用尽会停火；新增或升级显卡，提高每秒产能和存储容量。</span></li><li><strong>选择单元，选择落点</strong><span>点击已部署的单元，按 Q 或技能按钮，再点击地面。技能消耗大量算力，并有各自冷却。GPT 技能还能修复核心。</span></li><li><strong>应对 Boss 改图</strong><span>每个战区四波。Boss 半血和死亡会改变岩壁、水路或桥梁，双方按新地形寻路。完成后可带着显卡进入下一战区。</span></li></ol><div className="v2-shortcuts"><kbd>1–4</kbd>选单元 <kbd>B</kbd>基地 <kbd>Q</kbd>技能 <kbd>N</kbd>下一波 <kbd>Space</kbd>暂停</div><p className="v2-note">显卡型号、图片和显存信息来自真实厂商资料；经费、每秒产能和容量均为游戏平衡值，不是硬件价格或性能测试。</p></>:
    <><div className="v2-archive-tabs"><button aria-pressed={archiveTab==='gpus'} onClick={()=>setArchiveTab('gpus')}>真实显卡</button><button aria-pressed={archiveTab==='bugs'} onClick={()=>setArchiveTab('bugs')}>Bug 图鉴</button><button aria-pressed={archiveTab==='heroes'} onClick={()=>setArchiveTab('heroes')}>角色与特效</button></div>{archiveTab==='gpus'?<><div className="v2-archive-gpus">{GPUS.map(g=><article key={g.id}><img src={A+g.image} alt={g.name}/><div><h3>{g.name}</h3><small>{g.caption}</small><p>{g.vram} GB {g.memory}</p><a href={g.sourceUrl} target="_blank" rel="noreferrer">查看官方产品与图片来源 ↗</a></div></article>)}</div><p className="v2-note">图片归 NVIDIA、MSI、PNY 等原权利人。游戏供能数值独立于硬件真实规格。</p></>:archiveTab==='bugs'?<div className="v2-bug-archive">{BUGS.map(b=><article key={b.id}><img src={A+b.image} alt={b.name} loading="lazy"/><div><span>{b.id>=5?'BOSS':'BUG'} · {b.cwe}</span><h3>{b.name}</h3><small>{b.english}</small><p>{b.description}</p><a href={`https://cwe.mitre.org/data/definitions/${b.cwe.replace('CWE-','')}.html`} target="_blank" rel="noreferrer">真实软件错误概念 ↗</a></div></article>)}</div>:<div className="v2-hero-archive"><article><img src={A+'deepseek.png'} alt="鲸鱼娘"/><div><h3>DeepSeek · 鲸鱼娘</h3><p>上善无形原型 / ZipZipPipe 女仆二创 / Neko3000 参考设定。</p><a href="https://www.bilibili.com/video/BV1EvKK6NEoi/" target="_blank" rel="noreferrer">哔哩哔哩来源 ↗</a></div></article><article><img src={A+'gpt.png'} alt="GPT白龙娘"/><div><h3>GPT · 白龙娘</h3><p>ゆうまEthan〜 / JPEthan Token Monitor 的白龙娘半身版本。</p><a href="https://www.youtube.com/watch?v=xASRX37IIiY" target="_blank" rel="noreferrer">YouTube 来源 ↗</a></div></article><p className="v2-note">角色动画与三套新技能特效，均通过本项目 Codex agent 调用真实图生视频后截帧制作。新 Bug 形象为依据真实错误概念创作的游戏美术。实验性 GPU 粒子有独立启动开关，未取代视频特效。</p></div>}</>}
    </section></div>}
  </main>;
}
