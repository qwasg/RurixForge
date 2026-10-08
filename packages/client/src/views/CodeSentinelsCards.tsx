import { useEffect, useRef, useState, type CSSProperties, type DragEvent } from 'react';
import { ArrowLeft, ArrowRight, ArrowUp, BookOpen, Box, ChevronDown, ChevronUp, ChevronsRight, CircleHelp, Cpu, Crosshair, Eye, Layers, Pause, Play, RotateCcw, Shield, Target, Trash2, Volume2, VolumeX, X, Zap } from 'lucide-react';
import { GPUS, OPERATORS, BUGS, LEVELS, TERRAIN_NAMES, commandDeploy, commandGpu, commandGpuSell, commandGpuUpgrade, commandLevel, commandNextLevel, commandNextWave, commandSell, commandSkill, commandSpeed, commandTarget, commandUpgrade } from '@/lib/sentinelsV2';
import { useSentinelsRuntime } from '@/lib/useSentinelsRuntime';
import { TACTICAL_HOTKEYS, useTacticalHotkeys, type TacticalHandMode } from '@/lib/useTacticalHotkeys';
import TacticalCard from '@/components/game/TacticalCard';
import './code-sentinels-cards.css';

const ASSETS='/games/code-sentinels/',UI=ASSETS+'ui-v3/',ART=UI+'art/';
const OP_ART=['operator-vscode','operator-pycharm','operator-deepseek','operator-gpt'];
const CLASS_ART=['class-precision','class-debug','class-tide','class-support'];
const ENGLISH=['VSCODE','PYCHARM','DEEPSEEK','GPT'];
const MISSION_ART=['mission-forest','mission-marsh','mission-highland'];
const WORLD_WIDTH=224/9;
const number=(n:number)=>Math.max(0,Math.floor(n)).toLocaleString('en-US');
const rate=(n:number)=>Math.max(0,n).toLocaleString('en-US',{maximumFractionDigits:1});
const cellName=(cell:number)=>`${String.fromCharCode(65+cell%24)}${Math.floor(cell/24)+1}`;
const pos=(cell:number)=>({left:`${((cell%24-11.5)/WORLD_WIDTH+.5)*100}%`,top:`${(Math.floor(cell/24)+.5)/14*100}%`});
const hardwareArt=(id:number)=>ART+(id>=6?'hardware-datacenter':id===5?'hardware-pro':'hardware-consumer')+'.png';
type CardDrag={kind:'operator'|'hardware';id:number};

export default function CodeSentinelsCards(){
  const r=useSentinelsRuntime(),s=r.state;
  const [mode,setMode]=useState<TacticalHandMode>('operators'),[deckOpen,setDeckOpen]=useState(true);
  const [operatorId,setOperatorId]=useState(1),[gpuModel,setGpuModel]=useState(1);
  const [unitSlot,setUnitSlot]=useState<number|null>(null),[gpuSlot,setGpuSlot]=useState<number|null>(null);
  const [placing,setPlacing]=useState(false),[aiming,setAiming]=useState(false),[inspecting,setInspecting]=useState(false),[rackOpen,setRackOpen]=useState(false);
  const [hover,setHover]=useState<number|null>(null),[dragging,setDragging]=useState<CardDrag|null>(null);
  const [drawer,setDrawer]=useState<'help'|'archive'|'missions'|null>(null),[archiveTab,setArchiveTab]=useState<'operators'|'hardware'|'bugs'>('operators');
  const [grid,setGrid]=useState(false),[muted,setMuted]=useState(true),[noticeOpen,setNoticeOpen]=useState(true),[viewAfterBattle,setViewAfterBattle]=useState(false);
  const boardRef=useRef<HTMLDivElement>(null),modalRef=useRef<HTMLElement>(null),audioRef=useRef<AudioContext|null>(null);
  const pendingDeploy=useRef<{cell:number;kind:number;issued:number}|null>(null),pendingGpu=useRef<{slot:number;model:number;first:boolean;issued:number}|null>(null);
  const selectedUnit=unitSlot===null?null:s.units.find(u=>u.slot===unitSlot&&u.kind>0)??null;
  const selectedGpu=gpuSlot===null?null:s.gpus.find(g=>g.slot===gpuSlot&&g.model>0)??null;
  const op=OPERATORS[(selectedUnit?.kind??operatorId)-1],gpu=GPUS[(selectedGpu?.model??gpuModel)-1];
  const gameOver=r.ready&&s.phase>=2,usable=r.ready&&r.nativeReady&&r.connected&&!r.busy&&!r.paused&&!gameOver;
  const activeUnits=s.units.filter(u=>u.kind>0),starved=activeUnits.filter(u=>u.starved).length;
  const standalone=new URLSearchParams(location.search).get('standalone')==='1';
  const immersive=aiming||placing||Boolean(dragging);
  const skillReady=Boolean(selectedUnit&&usable&&selectedUnit.skillCooldown<=0&&s.energy>=selectedUnit.skillCost);

  useEffect(()=>{document.title='编译防线 · 战术牌组';},[]);
  useEffect(()=>{pendingGpu.current=null;pendingDeploy.current=null;},[r.connectionEpoch]);
  useEffect(()=>{setNoticeOpen(true);const t=window.setTimeout(()=>setNoticeOpen(false),4200);return()=>clearTimeout(t);},[r.notice]);
  useEffect(()=>{if(unitSlot!==null&&!selectedUnit){setUnitSlot(null);setAiming(false);setInspecting(false);}},[selectedUnit,unitSlot]);
  useEffect(()=>{if(gpuSlot!==null&&!selectedGpu&& !pendingGpu.current){setGpuSlot(null);}},[selectedGpu,gpuSlot]);
  useEffect(()=>{setUnitSlot(null);setGpuSlot(null);setAiming(false);setPlacing(false);setInspecting(false);setViewAfterBattle(false);pendingDeploy.current=null;pendingGpu.current=null;},[s.level]);
  useEffect(()=>{
    const pending=pendingDeploy.current;
    if(pending){const unit=s.units.find(u=>u.cell===pending.cell&&u.kind===pending.kind);if(unit){pendingDeploy.current=null;setPlacing(false);setInspecting(false);setDeckOpen(true);}}
    const install=pendingGpu.current;
    if(install){if(s.gpus[install.slot]?.model===install.model){pendingGpu.current=null;setPlacing(false);setInspecting(false);setGpuSlot(null);setDeckOpen(true);if(install.first){setMode('operators');setRackOpen(false);}}}
  },[s]);
  useEffect(()=>{
    const previous=document.activeElement as HTMLElement|null;const shell=document.querySelector<HTMLElement>('.cards-shell');
    if(shell)shell.inert=Boolean(drawer);if(drawer)modalRef.current?.focus();
    return()=>{if(shell)shell.inert=false;if(drawer)previous?.focus();};
  },[drawer]);
  useEffect(()=>()=>{void audioRef.current?.close();},[]);

  const sound=(frequency=600)=>{
    if(muted)return;const audio=audioRef.current??new AudioContext();audioRef.current=audio;void audio.resume();
    const oscillator=audio.createOscillator(),gain=audio.createGain();oscillator.type='triangle';oscillator.frequency.setValueAtTime(frequency,audio.currentTime);oscillator.frequency.exponentialRampToValueAtTime(frequency*.55,audio.currentTime+.12);
    gain.gain.setValueAtTime(.025,audio.currentTime);gain.gain.exponentialRampToValueAtTime(.0001,audio.currentTime+.17);oscillator.connect(gain).connect(audio.destination);oscillator.start();oscillator.stop(audio.currentTime+.18);
  };
  const start=async()=>{if(await r.start()){setMode('hardware');setGpuModel(1);setDeckOpen(true);setRackOpen(true);setPlacing(false);setAiming(false);setInspecting(false);setUnitSlot(null);setGpuSlot(null);setViewAfterBattle(false);pendingDeploy.current=null;pendingGpu.current=null;}};
  const choose=(kind:'operator'|'hardware',id:number)=>{
    pendingDeploy.current=null;pendingGpu.current=null;
    document.querySelector<HTMLElement>('.cards-shell')?.focus({preventScroll:true});
    if(kind==='operator'){setOperatorId(id);setMode('operators');setRackOpen(false);}else{setGpuModel(id);setMode('hardware');setRackOpen(true);}
    setUnitSlot(null);setGpuSlot(null);setAiming(false);setPlacing(true);setInspecting(true);setDeckOpen(false);sound(380+id*60);
    r.setNotice(kind==='operator'?'点击战场部署，或按 Esc 取消':'点击基地插槽，或 Enter 安装到首个空槽');
  };
  const selectUnit=(slot:number)=>{const u=s.units.find(u=>u.slot===slot&&u.kind>0);if(!u)return;setUnitSlot(slot);setOperatorId(u.kind);setGpuSlot(null);setMode('operators');setPlacing(false);setAiming(false);setRackOpen(false);setInspecting(true);setDeckOpen(false);};
  const installGpu=(slot?:number,model=gpuModel)=>{
    if(!usable||(pendingGpu.current&&performance.now()-pendingGpu.current.issued<1200))return;
    const target=slot??s.gpus.find(g=>g.model===0)?.slot;
    if(target===undefined){r.setNotice('机架已满，选中显卡升级或回收');return;}
    if(s.gpus[target]?.model){r.setNotice('这个插槽已经有显卡');return;}
    if(s.credits<GPUS[model-1].cost){r.setNotice(`还需要 ${number(GPUS[model-1].cost-s.credits)} 建设经费`);return;}
    if(r.send(commandGpu(target,model))){pendingGpu.current={slot:target,model,first:s.gpuCount===0,issued:performance.now()};sound(420);}
  };
  const deploy=(cell:number,kind=operatorId)=>{
    if(!usable||(pendingDeploy.current&&performance.now()-pendingDeploy.current.issued<1200))return;
    if(cell%24<4||![0,3,4,7].includes(s.terrain[cell])){r.setNotice('选择基地外的平地、道路、高地或桥');return;}
    if(s.credits<OPERATORS[kind-1].cost){r.setNotice('建设经费不足，暂时无法部署这张卡');return;}
    if(r.send(commandDeploy(kind,cell))){pendingDeploy.current={cell,kind,issued:performance.now()};sound(680);}
  };
  const activateSkill=()=>{
    if(!selectedUnit){r.setNotice('先点击已部署的守护者，Z / X 可以快速切换');return;}
    if(!usable)return;
    if(selectedUnit.skillCooldown>0){r.setNotice(`技能还需 ${Math.ceil(selectedUnit.skillCooldown)} 秒`);return;}
    if(s.energy<selectedUnit.skillCost){r.setNotice(`技能需要 ${selectedUnit.skillCost} 算力`);return;}
    setAiming(!aiming);setPlacing(false);setDeckOpen(false);r.setNotice('选择技能落点 · Esc 取消');
  };
  const clickCell=(cell:number)=>{
    if(!usable)return;
    if(aiming){if(selectedUnit&&r.send(commandSkill(selectedUnit.slot,cell)))sound(230);setAiming(false);return;}
    const unit=s.units.find(u=>u.kind>0&&u.cell===cell);if(unit){selectUnit(unit.slot);return;}
    if(cell%24<4||s.terrain[cell]===5){if(mode==='hardware'&&placing){installGpu();}else{setMode('hardware');setRackOpen(true);setDeckOpen(true);setInspecting(false);setUnitSlot(null);}return;}
    if(placing&&mode==='operators'){deploy(cell);return;}
    setInspecting(false);setUnitSlot(null);setGpuSlot(null);
  };
  const upgrade=()=>{if(!usable)return;if(selectedUnit){r.send(commandUpgrade(selectedUnit.slot));}else if(selectedGpu){r.send(commandGpuUpgrade(selectedGpu.slot));}};
  const sell=()=>{if(!usable)return;if(selectedUnit){if(r.send(commandSell(selectedUnit.slot))){setUnitSlot(null);setInspecting(false);}}else if(selectedGpu){if(r.send(commandGpuSell(selectedGpu.slot))){setGpuSlot(null);setInspecting(false);}}};
  const cycleTarget=()=>{if(usable&&selectedUnit)r.send(commandTarget(selectedUnit.slot,(selectedUnit.targetMode+1)%3));};
  const cycleUnit=(delta:number)=>{if(!activeUnits.length)return;const index=activeUnits.findIndex(u=>u.slot===unitSlot);const next=index<0?(delta<0?activeUnits.length-1:0):(index+delta+activeUnits.length)%activeUnits.length;selectUnit(activeUnits[next].slot);};
  const toggleHand=()=>{setMode(m=>m==='operators'?'hardware':'operators');setDeckOpen(true);setRackOpen(mode==='operators');setPlacing(false);setAiming(false);setInspecting(false);setUnitSlot(null);setGpuSlot(null);};
  const cancel=()=>{
    pendingDeploy.current=null;pendingGpu.current=null;
    if(drawer){setDrawer(null);return;}if(aiming){setAiming(false);return;}if(placing){setPlacing(false);setInspecting(false);setDeckOpen(true);return;}if(inspecting){setInspecting(false);setUnitSlot(null);setGpuSlot(null);return;}setDeckOpen(false);setRackOpen(false);
  };
  const nextWave=()=>{if(usable)r.send(commandNextWave());};
  useTacticalHotkeys({enabled:r.ready&&r.nativeReady&&!r.busy,blocked:Boolean(drawer)||gameOver,mode,hasSelection:Boolean(selectedUnit||selectedGpu),selectCard:i=>choose(mode==='operators'?'operator':'hardware',i+1),toggleHand,toggleDeck:()=>setDeckOpen(v=>!v),activateSkill,upgrade,sell,cycleTarget,nextWave,togglePause:()=>void r.togglePause(),cancel,cycleUnit,quickInstall:()=>installGpu(),help:()=>setDrawer('help'),toggleGrid:()=>setGrid(v=>!v),toggleSound:()=>setMuted(v=>!v)});
  const dropCard=(event:DragEvent<HTMLDivElement>)=>{
    event.preventDefault();setDragging(null);if(!usable)return;
    let card:CardDrag;try{card=JSON.parse(event.dataTransfer.getData('application/x-code-sentinels-card'));}catch{return;}
    if(!card||!Number.isInteger(card.id)||!['operator','hardware'].includes(card.kind)||card.id<1||card.id>(card.kind==='operator'?4:7))return;
    const rect=boardRef.current?.getBoundingClientRect();if(!rect)return;
    const worldX=((event.clientX-rect.left)/rect.width-.5)*WORLD_WIDTH;const col=Math.floor(worldX+12),row=Math.floor((event.clientY-rect.top)/rect.height*14);
    if(col<0||col>=24||row<0||row>=14)return;
    if(card.kind==='hardware'){if(col<4||s.terrain[row*24+col]===5)installGpu(undefined,card.id);else r.setNotice('将显卡拖到左侧基地');}else deploy(row*24+col,card.id);
  };
  const dropGpuOnSocket=(event:DragEvent<HTMLElement>,slot?:number)=>{
    event.preventDefault();event.stopPropagation();setDragging(null);if(!usable)return;
    let card:CardDrag;try{card=JSON.parse(event.dataTransfer.getData('application/x-code-sentinels-card'));}catch{return;}
    if(card?.kind!=='hardware'||!Number.isInteger(card.id)||card.id<1||card.id>7)return;
    installGpu(slot,card.id);
  };
  const mission=(level:number)=>{if(!r.paused&&r.send(commandLevel(level))){setDrawer(null);setDeckOpen(true);setMode('hardware');setRackOpen(true);setPlacing(false);setAiming(false);setInspecting(false);setViewAfterBattle(false);}};

  return <main className={`cards-game ${r.ready?'is-live':'is-title'} ${immersive?'is-immersive':''} ${deckOpen?'deck-open':'deck-closed'} ${dragging?'is-dragging':''}`}>
    <div className="cards-shell" tabIndex={-1}>
      <img className="cards-world-backdrop" src={ART+'title-scene.png'} alt=""/>
      {r.ready?<div ref={boardRef} className={`cards-battlefield ${grid?'show-grid':''}`} data-testid="v3-board" onDragOver={e=>{if(e.dataTransfer.types.includes('application/x-code-sentinels-card')){e.preventDefault();e.dataTransfer.dropEffect='copy';}}} onDrop={dropCard}>
        <canvas ref={r.canvasRef} width={1280} height={720} aria-label="原生开放战场"/>
        {r.nativeReady&&!gameOver&&<div className="cards-cell-grid">{s.terrain.map((tile,cell)=><button key={cell} data-testid={`v3-cell-${cell}`} aria-label={`${cellName(cell)} ${TERRAIN_NAMES[tile]}`} title={`${cellName(cell)} · ${TERRAIN_NAMES[tile]}`} disabled={!usable} className={`${[1,2,6].includes(tile)?'is-blocked':''} ${selectedUnit?.cell===cell?'is-selected':''}`} style={pos(cell)} onMouseEnter={()=>setHover(cell)} onMouseLeave={()=>setHover(null)} onClick={()=>clickCell(cell)}/>)}</div>}
        {selectedUnit&&!gameOver&&<div className="cards-range" style={{...pos(aiming&&hover!==null?hover:selectedUnit.cell),width:`${(aiming?(selectedUnit.kind===1?1.8:3.1):selectedUnit.range)*2/WORLD_WIDTH*100}%`}}/>}
        {hover!==null&&immersive&&<img className="cards-reticle" src={UI+'deployment-reticle.svg'} alt="" style={pos(hover)}/>}
        {selectedUnit&&!aiming&&<img className="cards-selection" src={UI+'selection-corners.svg'} alt="" style={pos(selectedUnit.cell)}/>}
      </div>:<section className="cards-title-scene"><div className="cards-title-copy"><span className="cards-kicker">RURIX FORGE / TACTICAL ARCHIVE 03</span><h1>编译<br/><em>防线</em><span>CODE SENTINELS</span></h1><p>让算力成为武器。<br/>让每一张卡，改变战局。</p><button className="cards-start" disabled={!r.workspace||r.busy} onClick={()=>void start()}>{r.busy?'接入战场…':'开始行动'}<ArrowRight size={24}/></button><small>BUILD THE NETWORK. DEFEND THE CORE.</small></div><div className="cards-title-posters"><div className="cards-poster poster-gpt"><img src={ART+'operator-gpt.png'} alt="GPT 白龙娘卡面"/><span>SUPPORT / 04</span><strong>GPT</strong></div><div className="cards-poster poster-deepseek"><img src={ART+'operator-deepseek.png'} alt="DeepSeek 鲸鱼娘卡面"/><span>CONTROL / 03</span><strong>DEEPSEEK</strong></div><img className="cards-title-seal" src={UI+'operation-seal.svg'} alt="Code Sentinels 行动印章"/></div><div className="cards-title-missions">{LEVELS.map((name,i)=><button key={name} onClick={()=>setDrawer('missions')}><img src={ART+MISSION_ART[i]+'.png'} alt=""/><span>OPERATION / 0{i+1}</span><strong>{name}</strong><ArrowRight size={16}/></button>)}</div></section>}

      <header className="cards-mission-head"><button className="cards-brand" aria-label="查看战区" onClick={()=>setDrawer('missions')}><img src={UI+'operation-seal.svg'} alt=""/><span><strong>{r.ready?LEVELS[s.level-1]:'CODE SENTINELS'}</strong><small>{r.ready?`OPERATION 0${s.level} / ${['BREAKPOINT','MEMORY LEAK','RECURSION'][s.level-1]}`:'TACTICAL CARD EDITION'}</small></span></button>{r.ready&&<div className="cards-wave"><strong data-testid="v3-wave">{String(s.wave).padStart(2,'0')}<span>/ 04</span></strong><small>{s.phase===1?`${s.enemies} ENEMIES`:'STANDBY'}</small></div>}</header>
      <nav className="cards-command-corner"><button aria-label="作战手册" title="快捷键 / H" onClick={()=>setDrawer('help')}><CircleHelp size={19}/></button><button aria-label="资料图鉴" title="角色、显卡与 Bug 档案" onClick={()=>setDrawer('archive')}><BookOpen size={18}/></button>{r.ready&&<><button aria-label="切换速度" disabled={!usable} onClick={()=>r.send(commandSpeed())}>{s.speed}×</button><button aria-label={r.paused?'继续战斗':'暂停战斗'} disabled={!r.ready||r.busy||gameOver} onClick={()=>void r.togglePause()}>{r.paused?<Play size={20}/>:<Pause size={20}/>}</button></>}<button aria-label={muted?'开启音效':'关闭音效'} title="音效 / M" onClick={()=>setMuted(v=>!v)}>{muted?<VolumeX size={17}/>:<Volume2 size={17}/>}</button>{!standalone&&<a href="/" aria-label="返回编辑器"><ArrowLeft size={17}/></a>}</nav>
      {r.ready&&<>
        <div className="cards-resource-rig"><div className="cards-credit" title="建设经费"><img src={UI+'credit-emblem.svg'} alt="建设经费"/><strong data-testid="v3-credits">{number(s.credits)}</strong></div><div className={`cards-energy ${starved?'is-starved':''}`} title={`算力 ${number(s.energy)} / ${number(s.capacity)}，基地每秒 +${rate(s.production)}`}><span className="cards-energy-dial" style={{'--charge':`${s.capacity?Math.min(100,s.energy/s.capacity*100):0}%`} as CSSProperties}><img src={UI+'energy-core-emblem.svg'} alt="算力"/></span><div><strong data-testid="v3-energy">{number(s.energy)}</strong><small><span data-testid="v3-production">+{rate(s.production)}/s</span> · {number(s.capacity)} CAP</small></div></div><div className="cards-core" title={`核心生命 ${number(s.hp)} / 20`}><Shield size={22}/><strong data-testid="v3-hp">{number(s.hp)}</strong><div>{Array.from({length:10},(_,i)=><i key={i} className={i*2<s.hp?'lit':''}/>)}</div></div></div>
        <button className={`cards-wave-order ${s.phase===0?'is-ready':''}`} disabled={!usable||s.phase!==0} onClick={nextWave}><span>OPERATION</span><strong>{s.phase===1?'交战中':'开始入侵'}</strong><ArrowRight size={22}/><kbd>N</kbd></button>
        {s.bossPhase>0&&s.bossHealth>0&&<div className="cards-boss-signal"><span>BOSS // {BUGS[4+s.level].name}<small>PHASE {s.bossPhase}</small></span><div><i style={{width:`${s.bossHealth*100}%`}}/></div></div>}
        {s.terrainStage>0&&<div className="cards-map-rewrite"><img src={UI+'diagonal-hazard-strip.svg'} alt=""/><span>DATA RESTRUCTURED <b>0{s.terrainStage}</b></span></div>}
        {rackOpen&&!aiming&&<div className="cards-base-rack"><div><Cpu size={15}/><span>COMPUTE BAY</span><b>{s.gpuCount}/8</b><button aria-label="收起机架" onClick={()=>setRackOpen(false)}><X size={13}/></button></div><section onDragOver={e=>{if(e.dataTransfer.types.includes('application/x-code-sentinels-card'))e.preventDefault();}} onDrop={e=>dropGpuOnSocket(e)}>{s.gpus.map(g=><button key={g.slot} onDragOver={e=>{if(e.dataTransfer.types.includes('application/x-code-sentinels-card'))e.preventDefault();}} onDrop={e=>dropGpuOnSocket(e,g.slot)} aria-label={`显卡插槽 ${g.slot+1}`} title={g.model?`${GPUS[g.model-1].name} · +${rate(g.rate)}/s`:`空槽 ${g.slot+1}`} className={`${g.model?'is-filled':''} ${gpuSlot===g.slot?'is-selected':''}`} onClick={()=>{if(g.model){setGpuSlot(g.slot);setGpuModel(g.model);setUnitSlot(null);setMode('hardware');setPlacing(false);setInspecting(true);setDeckOpen(false);}else if(placing&&mode==='hardware')installGpu(g.slot);else{setMode('hardware');setDeckOpen(true);}}}><small>0{g.slot+1}</small>{g.model?<img src={ASSETS+GPUS[g.model-1].image} alt={GPUS[g.model-1].name}/>:<span>＋</span>}{g.model>0&&<em>+{rate(g.rate)}</em>}</button>)}</section></div>}
        {!rackOpen&&!aiming&&<button className="cards-base-beacon" title="显卡牌组 / B" aria-label="打开算力基地" onClick={()=>{setMode('hardware');setDeckOpen(true);setRackOpen(true);setInspecting(false);setPlacing(false);setUnitSlot(null);}}><Cpu size={19}/><span>{s.gpuCount}<small>GPU</small></span></button>}
        {inspecting&&!aiming&&!dragging&&<aside className={`cards-dossier ${mode==='hardware'?'is-hardware':''}`}><button className="cards-dossier-close" aria-label="关闭卡牌详情" onClick={()=>{setInspecting(false);setPlacing(false);setUnitSlot(null);setGpuSlot(null);}}><X size={16}/></button><img className="cards-dossier-art" src={mode==='operators'?ART+OP_ART[op.id-1]+'.png':hardwareArt(gpu.id)} alt=""/>{mode==='hardware'&&<img className="cards-dossier-hardware" src={ASSETS+gpu.image} alt={gpu.name}/>}<div className="cards-dossier-type"><img src={UI+(mode==='operators'?CLASS_ART[op.id-1]:'energy-core-emblem')+'.svg'} alt=""/><span>{mode==='operators'?(selectedUnit?`UNIT ${String(selectedUnit.slot+1).padStart(2,'0')} / ${cellName(selectedUnit.cell)}`:'READY TO DEPLOY'):(selectedGpu?`BAY 0${selectedGpu.slot+1}`:'INFRASTRUCTURE')}</span></div><div className="cards-dossier-copy"><small>{mode==='operators'?ENGLISH[op.id-1]:`${gpu.vram}GB ${gpu.memory}`}</small><h2>{mode==='operators'?op.name:gpu.name.replace('GeForce ','')}</h2>{selectedUnit?<><div className="cards-dossier-health"><i style={{width:`${Math.min(100,selectedUnit.hp/(130+45*(selectedUnit.tier-1))*100)}%`}}/></div><span className="cards-object-level">Lv.{selectedUnit.tier} · {number(selectedUnit.hp)} HP {selectedUnit.jam>0?' / JAMMED':selectedUnit.starved?' / NO ENERGY':''}</span></>:selectedGpu?<span className="cards-object-level">Lv.{selectedGpu.tier} · +{rate(selectedGpu.rate)} / s</span>:<p>{mode==='operators'?'点击空地部署 · Esc 取消':'点击空槽安装 · Enter 快速接入'}</p>}
          {mode==='operators'?<div className="cards-dossier-cost"><span><Zap size={13}/>{selectedUnit?.attackCost??op.attackCost}<small>/ 发</small></span><span>{(selectedUnit?.range??op.range).toFixed(1)}<small>RANGE</small></span><span><img src={UI+'credit-emblem.svg'} alt="经费"/>{op.cost}</span></div>:<div className="cards-dossier-cost"><span>+{rate(selectedGpu?.rate??gpu.rate)}<small>/ s</small></span><span>{gpu.capacity}<small>{selectedGpu?'基础 CAP':'CAP'}</small></span><span><img src={UI+'credit-emblem.svg'} alt="经费"/>{gpu.cost}</span></div>}
          {(selectedUnit||selectedGpu)&&<div className="cards-object-actions"><button aria-label={selectedUnit?'升级单元':'升级显卡'} title={`强化 / E · ${number(selectedUnit?.upgradeCost??selectedGpu?.upgradeCost??0)} 经费`} disabled={!usable||(selectedUnit?.tier??selectedGpu?.tier??0)>=3||s.credits<(selectedUnit?.upgradeCost??selectedGpu?.upgradeCost??0)} onClick={upgrade}><ArrowUp size={18}/><kbd>E</kbd><small>{number(selectedUnit?.upgradeCost??selectedGpu?.upgradeCost??0)}</small></button><button aria-label={selectedUnit?'回收单元':'回收显卡'} title={`回收 / R · 返还 ${number(selectedUnit?.sellRefund??selectedGpu?.sellRefund??0)}`} disabled={!usable||Boolean(selectedGpu&&s.gpuCount<=1)} onClick={sell}><Trash2 size={17}/><kbd>R</kbd></button>{selectedUnit&&<button aria-label="切换目标策略" title={`目标 / T · ${['逼近基地','最低生命','优先Boss'][selectedUnit.targetMode]}`} disabled={!usable} onClick={cycleTarget}><Target size={18}/><kbd>T</kbd></button>}</div>}
        </div>{selectedUnit&&<div className="cards-skill-module"><button className="cards-skill-diamond" aria-label="选择技能落点" title={`${op.skillName} / Q · ${selectedUnit.skillCost} 算力`} disabled={!skillReady} onClick={activateSkill} style={{'--skill-x':`${(op.id-1)%2*100}%`,'--skill-y':`${Math.floor((op.id-1)/2)*100}%`,'--skill-color':op.color} as CSSProperties}><i/><img src={UI+'skill-diamond-frame.svg'} alt=""/><kbd>Q</kbd>{selectedUnit.skillCooldown>0&&<strong>{Math.ceil(selectedUnit.skillCooldown)}</strong>}</button><div><strong>{op.skillName}</strong><small><Zap size={11}/>{selectedUnit.skillCost} CE</small></div></div>}</aside>}
        {aiming&&<div className="cards-aim-banner"><img src={UI+'deployment-reticle.svg'} alt=""/><div><small>TARGET ACQUISITION</small><strong>{op.skillName}</strong><span>{selectedUnit?.skillCost} CE <kbd>Esc 取消</kbd></span></div></div>}
        <div className={`cards-notice ${noticeOpen?'is-visible':''}`} role="status"><span>SYS</span><p>{r.notice}</p></div>
        {starved>0&&!s.feedback&&<div className="cards-brownout"><Zap size={15}/>{starved} UNIT(S) NEED POWER</div>}
        <div className={`cards-hand ${deckOpen?'is-open':'is-collapsed'} ${mode==='hardware'?'is-hardware':''}`}><div className="cards-hand-bar"><button aria-label="切换角色与显卡牌组" onClick={toggleHand}><Layers size={15}/><span>{mode==='operators'?'OPERATORS':'HARDWARE'}</span><kbd>B</kbd></button><small>{mode==='operators'?`${activeUnits.length} / 24 DEPLOYED`:`${s.gpuCount} / 8 CONNECTED`}</small><button aria-label={deckOpen?'收起手牌':'展开手牌'} onClick={()=>setDeckOpen(v=>!v)}>{deckOpen?<ChevronDown size={16}/>:<ChevronUp size={16}/>}<kbd>V</kbd></button></div><div className="cards-hand-fan">{mode==='operators'?OPERATORS.map((o,i)=><div className="cards-hand-slot" key={o.id} style={{'--i':i,'--count':4} as CSSProperties}><TacticalCard variant="operator" id={o.id} name={o.name} subtitle={o.role} code={`PRTCL-${String(o.id).padStart(2,'0')}`} cost={o.cost} energyCost={o.attackCost} artwork={ART+OP_ART[i]+'.png'} accent={o.color} selected={operatorId===o.id&&mode==='operators'} affordable={s.credits>=o.cost} hotkey={String(o.id)} details={[o.description,`${o.skillName} · ${o.skillCost} 算力 · ${o.cooldown}s`,o.skillDescription]} onChoose={()=>choose('operator',o.id)} onDragState={active=>setDragging(active?{kind:'operator',id:o.id}:null)}/></div>):GPUS.map((g,i)=><div className="cards-hand-slot" key={g.id} style={{'--i':i,'--count':7} as CSSProperties}><TacticalCard variant="hardware" id={g.id} name={g.name.replace('GeForce ','').replace(' Blackwell','')} subtitle={`${g.vram} GB ${g.memory}`} code={`HW-${String(g.id).padStart(2,'0')}`} cost={g.cost} production={g.rate} artwork={hardwareArt(g.id)} photo={ASSETS+g.image} accent={g.id>=5?'#EDB63B':'#D8DBD4'} selected={gpuModel===g.id&&mode==='hardware'} affordable={s.credits>=g.cost} hotkey={String(g.id)} details={[g.caption,`游戏产能 +${g.rate}/s · 容量 ${g.capacity}`,'真实厂商图片；产能与经费为游戏平衡值']} onChoose={()=>choose('hardware',g.id)} onDragState={active=>setDragging(active?{kind:'hardware',id:g.id}:null)}/></div>)}</div>{mode==='hardware'&&<button className="cards-quick-install" disabled={!usable||s.credits<GPUS[gpuModel-1].cost||s.gpuCount>=8} onClick={()=>installGpu()}>接入首个空槽<kbd>Enter</kbd><ArrowRight size={14}/></button>}</div>
        <div className="cards-bottom-code"><span>{r.connected?`${Math.round(r.fps)} FPS`:'CONNECTING'}</span><i/>NATIVE 2D · RURIX</div>
      </>}
      {r.paused&&!gameOver&&<section className="cards-pause"><span>TACTICAL SUSPENSION</span><h2>作战暂停</h2><button onClick={()=>void r.togglePause()}><Play size={20}/>继续行动<kbd>Space</kbd></button></section>}
      {gameOver&&!viewAfterBattle&&<section className="cards-result" style={{backgroundImage:`url(${ART+(s.phase===2?'result-victory':'result-defeat')+'.png'})`}}><img className="cards-result-stamp" src={UI+(s.phase===2?'victory-stamp':'defeat-stamp')+'.svg'} alt={s.phase===2?'行动成功':'行动失利'}/><div><span>OPERATION 0{s.level} / {LEVELS[s.level-1]}</span><h2>{s.phase===2?(s.level===3?'全战区净化完成':'战区净化完成'):'防线需要重新部署'}</h2><p>{s.kills} 次修复 <i/> {number(s.spentAttack+s.spentSkill)} 算力投入</p>{s.phase===2&&s.level<3&&<button className="cards-start" disabled={!r.connected||r.paused} onClick={()=>{if(r.send(commandNextLevel())){setViewAfterBattle(false);setMode('operators');setDeckOpen(true);}}}>推进下一战区<ArrowRight size={21}/></button>}<button className="cards-result-retry" onClick={()=>void start()}><RotateCcw size={15}/>{s.phase===2?'重新行动':'重新部署'}</button><button className="cards-result-review" onClick={()=>setViewAfterBattle(true)}><Eye size={14}/>检视地形变化</button></div></section>}
      {gameOver&&viewAfterBattle&&<button className="cards-return-result" onClick={()=>setViewAfterBattle(false)}>返回战报<ArrowRight size={15}/></button>}
      {r.error&&<div className="cards-error" role="alert"><strong>连接需要恢复</strong><p>{r.error}</p><button onClick={()=>r.workspace?void start():location.reload()}>重新接入</button></div>}
    </div>
    {drawer&&<div className="cards-modal-backdrop" onClick={()=>setDrawer(null)}><section ref={modalRef} tabIndex={-1} className={`cards-modal ${drawer==='missions'?'is-missions':''}`} role="dialog" aria-modal="true" aria-label={drawer==='help'?'作战手册':drawer==='missions'?'战区选择':'资料图鉴'} onClick={e=>e.stopPropagation()} onKeyDown={e=>{if(e.key==='Escape'){setDrawer(null);return;}if(e.key!=='Tab')return;const items=modalRef.current?.querySelectorAll<HTMLElement>('button:not(:disabled),a[href]');if(!items?.length)return;const first=items[0],last=items[items.length-1];if(e.shiftKey&&(document.activeElement===first||document.activeElement===modalRef.current)){e.preventDefault();last.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first.focus();}}}><button className="cards-modal-close" aria-label="关闭" onClick={()=>setDrawer(null)}><X size={20}/></button><div className="cards-modal-heading"><img src={UI+'operation-seal.svg'} alt=""/><span>RURIX / FIELD ARCHIVE</span><h2>{drawer==='help'?'从手牌，到战场。':drawer==='missions'?'选择行动区域':'档案与来源'}</h2></div>
      {drawer==='help'?<><div className="cards-guide-flow"><article><b>01</b><strong>建立供能</strong><p>B 切显卡牌组，1–7 选卡，Enter 接入首个空槽。也可以把显卡拖到基地。</p></article><article><b>02</b><strong>打出角色卡</strong><p>1–4 选角色后点击战场，或拖拽卡牌到可部署地形。选卡后手牌收起，V 可随时展开。</p></article><article><b>03</b><strong>释放选点技能</strong><p>点击已部署单位或用 Z/X 切换，Q 进入技能瞄准，然后点击地面。普攻与技能均消耗 GPU 产出的算力。</p></article></div><div className="cards-hotkey-list">{TACTICAL_HOTKEYS.map(k=><div key={k.id}><kbd>{k.keyLabel}</kbd><span>{k.label}</span></div>)}</div><p className="cards-modal-note">单独的翻面图标可查看卡背资料，不会部署。岩壁与水域会阻挡通行，高地增加射程；Boss 半血与死亡会改写地形。资料图鉴保留真实角色、显卡与 Bug 来源。</p></>:drawer==='missions'?<div className="cards-mission-gallery">{LEVELS.map((name,i)=><button key={name} disabled={!r.ready||r.paused||i+1>s.unlocked} onClick={()=>mission(i+1)}><img src={ART+MISSION_ART[i]+'.png'} alt={name}/><span>OPERATION / 0{i+1}</span><h3>{name}</h3><p>{['岩壁与森林通路','水域与断桥重构','高地与递归崩塌'][i]}</p><small>{!r.ready?'开始行动后解锁选择':i+1>s.unlocked?'LOCKED':r.paused?'先继续战斗':'重新部署此战区'}</small><ArrowRight size={20}/></button>)}</div>:<><nav className="cards-archive-tabs"><button onClick={()=>setArchiveTab('operators')} aria-pressed={archiveTab==='operators'}>守护者</button><button onClick={()=>setArchiveTab('hardware')} aria-pressed={archiveTab==='hardware'}>真实显卡</button><button onClick={()=>setArchiveTab('bugs')} aria-pressed={archiveTab==='bugs'}>BUG 图鉴</button></nav>{archiveTab==='operators'?<><div className="cards-operator-gallery">{OPERATORS.map((o,i)=><article key={o.id}><img src={ART+OP_ART[i]+'.png'} alt={o.name+'卡面'}/><div><small>PRTCL / 0{o.id}</small><h3>{o.name}</h3><p>{o.description}</p><strong>{o.skillName}</strong><p>{o.skillDescription}</p></div></article>)}</div><p className="cards-modal-note">DeepSeek 鲸鱼娘：上善无形 / ZipZipPipe / Neko3000 参考链；GPT 白龙娘：ゆうまEthan〜 / JPEthan 半身版本。新卡面沿用已核实形象，游戏内动作保持原有真实视频帧。<a href="https://www.bilibili.com/video/BV1EvKK6NEoi/" target="_blank" rel="noreferrer">B站来源 ↗</a><a href="https://www.youtube.com/watch?v=xASRX37IIiY" target="_blank" rel="noreferrer">YouTube来源 ↗</a></p></>:archiveTab==='hardware'?<><div className="cards-hardware-gallery">{GPUS.map(g=><article key={g.id}><img src={ASSETS+g.image} alt={g.name}/><div><h3>{g.name}</h3><p>{g.caption}</p><strong>{g.vram} GB {g.memory}</strong><a href={g.sourceUrl} target="_blank" rel="noreferrer">官方图片与型号 ↗</a></div></article>)}</div><p className="cards-modal-note">真实显存与产品图片来自厂商。游戏中的建设经费、产能和容量为独立平衡值，不代表价格或性能测试。</p></>:<div className="cards-bug-gallery">{BUGS.map(b=><article key={b.id}><img src={ASSETS+b.image} alt={b.name} loading="lazy"/><span>{b.id>=5?'BOSS':'BUG'} / {b.cwe}</span><h3>{b.name}</h3><small>{b.english}</small><p>{b.description}</p><a href={`https://cwe.mitre.org/data/definitions/${b.cwe.replace('CWE-','')}.html`} target="_blank" rel="noreferrer">错误类型出处 ↗</a></article>)}</div>}</>}
    </section></div>}
  </main>;
}
