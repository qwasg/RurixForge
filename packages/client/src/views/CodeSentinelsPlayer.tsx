import { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowLeft, ArrowRight, BookOpen, Check, ChevronRight, Code2, Cpu, Heart, Pause, Play, RotateCcw, Shield, Sparkles, Terminal, Volume2, VolumeX, Waves, X, Zap } from 'lucide-react';
import { apiGet, callTool } from '@/lib/forgeApi';
import { useWorkspaceStore, type ForgeWorkspace } from '@/lib/workspaceStore';
import { openViewportStream, type StreamFrame, type ViewportStreamHandle } from '@/lib/viewportStream';
import './code-sentinels.css';

const ASSETS = '/games/code-sentinels/';
const UNITS = [
  { id: 1, name: 'VS Code', role: '精准编译', cost: 80, image: 'vscode.svg', accent: '#63b6ff', description: '快速锁定并攻击最前方的入侵者。', synergy: '与 PyCharm 协同，提高编译效率。', tag: '编辑器' },
  { id: 2, name: 'PyCharm', role: '调试火力', cost: 115, image: 'pycharm.svg', accent: '#b5e75c', description: '重型调试器，对密集错误集群发起攻击。', synergy: '与 VS Code 组成工具链。', tag: 'IDE' },
  { id: 3, name: 'DeepSeek 娘', role: '深度推理', cost: 145, image: 'deepseek.png', accent: '#6cd1f4', description: '鲸鱼娘以推理波控制入侵进程。', synergy: '与 GPT 娘组成双模型推理链。', tag: 'AI · 控制' },
  { id: 4, name: 'GPT 娘', role: '算力支援', cost: 165, image: 'gpt.png', accent: '#c9a7ff', description: '白龙娘每 4 秒生成算力，并增强同路伙伴伤害。', synergy: '与 DeepSeek 娘配合，增强防线。', tag: 'AI · 支援' },
];
type Entity = { id: number; name: string; transform: { translation: number[]; scale: number[]; rotation: number[] } };
type GameState = { energy: number; hp: number; wave: number; phase: number; kills: number; combo: number; cooldown: number; spawned: number; total: number; cells: Record<number, { type: number; level: number; upgradeCost: number }>; feedback: number; activeEnemies: number };
const INITIAL: GameState = { energy: 0, hp: 20, wave: 0, phase: 0, kills: 0, combo: 0, cooldown: 0, spawned: 0, total: 0, cells: {}, feedback: 0, activeEnemies: 0 };

/** Only the native engine simulates combat. This view draws its real frame stream and sends inputs. */
export default function CodeSentinelsPlayer() {
  const [workspace, setWorkspace] = useState<ForgeWorkspace | null>(null);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [paused, setPaused] = useState(false);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState('');
  const [selection, setSelection] = useState(1);
  const [cell, setCell] = useState<number | null>(null);
  const [state, setState] = useState<GameState>(INITIAL);
  const [drawer, setDrawer] = useState<'help' | 'sources' | null>(null);
  const [fps, setFps] = useState(0);
  const [muted, setMuted] = useState(true);
  const [speed, setSpeed] = useState(1);
  const [skillLane, setSkillLane] = useState(1);
  const feedbackRef = useRef(0);
  const pauseBusy = useRef(false);
  const [notice, setNotice] = useState('选择防御单元，再点击战场上的部署位。');
  const boardRef = useRef<HTMLDivElement>(null);
  const modalRef = useRef<HTMLElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const streamRef = useRef<ViewportStreamHandle | null>(null);
  const frameRef = useRef<StreamFrame | null>(null);
  const audioRef = useRef<AudioContext | null>(null);
  const readyRef = useRef(false);
  const selectedUnit = UNITS[selection - 1];
  const standalone = new URLSearchParams(window.location.search).get('standalone') === '1';

  useEffect(() => {
    document.title = '编译防线 · Code Sentinels';
    let cancelled = false;
    void apiGet<{ workspaces: ForgeWorkspace[] }>('/api/forge/workspaces').then(({ workspaces }) => {
      const requestedWorkspace = new URLSearchParams(window.location.search).get('workspace');
      const found = requestedWorkspace ? workspaces.find((w) => w.id === requestedWorkspace) : workspaces.find((w) => /[\\/]code-sentinels(?:[\\/]|$)/i.test(w.root));
      if (!found) throw new Error('尚未注册编译防线项目，请先在 RurixForge 中打开项目。');
      if (cancelled) return;
      useWorkspaceStore.getState().setActive(found.id);
      setWorkspace(found);
    }).catch((err: Error) => { if (!cancelled) setError(err.message); });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    const previousFocus = document.activeElement as HTMLElement | null;
    const sections = document.querySelectorAll<HTMLElement>('.cs-header, .cs-content');
    for (const section of sections) section.inert = Boolean(drawer);
    if (drawer) modalRef.current?.focus();
    return () => { for (const section of sections) section.inert = false; if (drawer) previousFocus?.focus(); };
  }, [drawer]);

  const sound = useCallback((frequency = 520) => {
    if (muted) return;
    const audio = audioRef.current ?? new AudioContext();
    audioRef.current = audio;
    void audio.resume();
    const oscillator = audio.createOscillator();
    const gain = audio.createGain();
    oscillator.type = 'sine';
    oscillator.frequency.setValueAtTime(frequency, audio.currentTime);
    oscillator.frequency.exponentialRampToValueAtTime(frequency * 0.65, audio.currentTime + 0.14);
    gain.gain.setValueAtTime(0.06, audio.currentTime);
    gain.gain.exponentialRampToValueAtTime(0.001, audio.currentTime + 0.2);
    oscillator.connect(gain).connect(audio.destination);
    oscillator.start(); oscillator.stop(audio.currentTime + 0.21);
  }, [muted]);

  const send = useCallback((action: string, value: number) => {
    if (!streamRef.current?.up) { setNotice('正在连接战场，请稍候。'); return false; }
    if (paused) { setNotice('请先继续战斗。'); return false; }
    const commands: Record<string, number> = { deploy: 1000 + value * 10 + selection, upgrade: 2000 + value, sell: 3000 + value, skill: 4000 + value, next_wave: 5000, speed: 7000 };
    const sent = commands[action] !== undefined && streamRef.current.sendInput('cs', commands[action]);
    if (!sent) setNotice('战场连接已中断，指令未发送，请重连后重试。');
    return sent;
  }, [paused, selection]);

  const start = async () => {
    if (!workspace || busy) return;
    setBusy(true); setError(''); setReady(false); readyRef.current = false; setConnected(false); frameRef.current = null; setNotice('正在载入防线…');
    try {
      useWorkspaceStore.getState().setActive(workspace.id);
      const summary = await callTool<{ playState?: string }>('scene_summary');
      if (summary.playState && summary.playState !== 'edit') await callTool('play_exit');
      await callTool('asset_reload');
      await callTool('scene_load', { path: 'Content/Scenes/Main.rxscene' });
      await callTool('viewport_set_camera', { target: [0, 0, 0], yaw: 0, pitch: 0, dist: 12, ortho: true, orthoSize: 6 });
      await callTool('play_enter');
      setReady(true); readyRef.current = true; setPaused(false); setCell(null); setSpeed(1); setState(INITIAL); feedbackRef.current = 0;
      setNotice('防线已就绪。先部署单元，再释放下一波。');
      sound(760);
    } catch (err) { setError((err as Error).message); }
    finally { setBusy(false); }
  };

  useEffect(() => {
    if (!ready || !workspace) return;
    let cancelled = false;
    let raf = 0;
    let pollTimer = 0;
    let lastFrame = -1;
    const stream = openViewportStream({ width: 1280, height: 720, maxFps: 40,
      onFrame: (frame) => { frameRef.current = frame; },
      onStatus: (status) => { setFps(status.fps); if (status.shareError) setError(status.shareError); },
      onError: (message) => setError(message),
      onChannel: (up) => { setConnected(up); },
    });
    streamRef.current = stream;
    const draw = () => {
      const frame = frameRef.current;
      const canvas = canvasRef.current;
      if (frame && canvas && frame.frameId !== lastFrame) {
        canvas.width = frame.width; canvas.height = frame.height;
        const bytes = new Uint8ClampedArray(frame.rgba.length); bytes.set(frame.rgba);
        canvas.getContext('2d')?.putImageData(new ImageData(bytes, frame.width, frame.height), 0, 0);
        lastFrame = frame.frameId;
      }
      raf = requestAnimationFrame(draw);
    };
    const poll = async () => {
      try {
        const { entities } = await callTool<{ entities: Entity[] }>('entity_list');
        if (cancelled) return;
        const main = entities.find((e) => e.name === 'CS_State')?.transform;
        const aux = entities.find((e) => e.name === 'CS_StateAux')?.transform;
        const meta = entities.find((e) => e.name === 'CS_Meta')?.transform;
        if (!main) throw new Error('原生场景缺少 CS_State 状态实体。');
        const cells: GameState['cells'] = {};
        for (const entity of entities) {
          const match = /^CS_Cell_?(\d+)$/.exec(entity.name);
          if (match) cells[Number(match[1])] = { type: entity.transform.translation[0], level: entity.transform.translation[1], upgradeCost: entity.transform.translation[2] };
        }
        setState({ energy: main.translation[0], hp: main.translation[1], wave: main.translation[2], phase: main.scale[0], kills: main.scale[1], combo: main.scale[2], cooldown: aux?.translation[0] ?? 0, spawned: aux?.translation[1] ?? 0, total: aux?.translation[2] ?? 0, cells, feedback: meta?.translation[0] ?? 0, activeEnemies: aux?.scale[0] ?? 0 });
        setSpeed(aux?.scale[1] ?? 1);
        const feedback = Math.round(meta?.translation[0] ?? 0);
        if (feedback !== 0) {
          const messages: Record<number,string> = { 1: '算力不足，请等待战斗产出或回收其他单元。', 2: '部署位已占用。', 3: '请先选择一个已部署的单元。', 4: '该单元已达到最高等级。', 5: '热修复正在冷却。', 6: '这条通路当前没有入侵者。', 7: '当前波次尚未结束，请继续守住防线。', 8: '无法识别该操作。', 9: '战斗已经结束，请重新部署。', 10: '单元部署成功。', 11: '升级完成，火力与射程提升。', 12: '单元已回收，返还 70% 已投入算力。', 13: '热修复已释放，目标通路入侵进程被压制。', 14: '入侵波次已启动，守住主分支！' };
          setNotice(messages[feedback] ?? '指令已处理。');
        }
        feedbackRef.current = feedback;
      } catch (err) { if (!cancelled) setError((err as Error).message); }
      finally { if (!cancelled) pollTimer = window.setTimeout(() => void poll(), 500); }
    };
    raf = requestAnimationFrame(draw); void poll();
    return () => { cancelled = true; cancelAnimationFrame(raf); clearTimeout(pollTimer); stream.close(); streamRef.current = null; };
  }, [ready, workspace]);

  const choose = useCallback((id: number) => { setSelection(id); setCell(null); sound(400 + id * 100); }, [sound]);
  const deploy = (index: number) => {
    setCell(index); setSkillLane(Math.floor(index / 4));
    if (state.cells[index]?.type > 0) { setSelection(Math.round(state.cells[index].type)); setNotice('已选中防御单元，可升级或回收。'); return; }
    if (!send('deploy', index)) return; sound(680);
    // Purchase success or failure is reported by CS_Meta, not assumed from a sent socket message.
  };
  const togglePause = useCallback(async () => {
    if (!readyRef.current || pauseBusy.current) return;
    pauseBusy.current = true;
    try { await callTool(paused ? 'play_resume' : 'play_pause'); setPaused(!paused); }
    catch (err) { setError((err as Error).message); }
    finally { pauseBusy.current = false; }
  }, [paused]);
  useEffect(() => {
    if (!ready) return;
    const listener = (event: KeyboardEvent) => {
      if (event.repeat || busy || (event.target as HTMLElement)?.closest('input,textarea,select')) return;
      if (event.key === 'Escape') { setDrawer(null); return; }
      if (drawer || state.phase >= 2) return;
      if (/^[1-4]$/.test(event.key)) { event.preventDefault(); choose(Number(event.key)); }
      else if (event.code === 'Space') { event.preventDefault(); void togglePause(); }
      else if (event.key.toLowerCase() === 'q') { send('skill', skillLane); sound(240); }
      else if (event.key.toLowerCase() === 'n') send('next_wave', 1);
      else if (event.key === 'Escape') setDrawer(null);
    };
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  }, [ready, choose, togglePause, send, sound, skillLane, drawer, busy, state.phase]);
  useEffect(() => () => { void audioRef.current?.close(); }, []);

  const exitToEditor = async () => {
    if (ready) await callTool('play_exit').catch(() => undefined);
    window.location.assign('/');
  };
  const filledCell = cell !== null && state.cells[cell]?.type > 0;
  const finished = ready && (state.phase === 2 || state.phase === 3);

  return <main className="cs-game">
    <header className="cs-header">
      <a className="cs-brand" href={window.location.search || "?play=code-sentinels"} aria-label="编译防线首页"><span className="cs-brand-icon"><Code2 size={25} /></span><span><strong>编译防线<span className="cs-brand-dot">.</span></strong><small>CODE SENTINELS</small></span></a>
      <div className="cs-chapter"><span className="cs-live-dot" />第一章 <i /> 服务器花园</div>
      <nav><button aria-label="作战手册" onClick={() => setDrawer('help')}><BookOpen size={16} /><span>作战手册</span></button><button aria-label="角色档案" onClick={() => setDrawer('sources')}><Sparkles size={16} /><span>角色档案</span></button>{!standalone && <button onClick={() => void exitToEditor()} className="cs-icon-btn" title="返回 RurixForge 编辑器" aria-label="返回编辑器"><ArrowLeft size={18} /></button>}</nav>
    </header>
    <div className="cs-content">
      <section className="cs-heading"><div><span className="cs-eyebrow">TACTICAL DEPLOYMENT / 01</span><h1>灵感就位，防线启动。</h1><p>让开发工具与 AI 伙伴协同，守住最后一行代码。</p></div><div className="cs-heading-badge"><Shield size={19}/><span>守护主分支<small>MAIN BRANCH DEFENSE</small></span></div></section>
      <section className="cs-battle-layout">
        <div className="cs-main-column">
          <div className="cs-hud"><div className="cs-stat cs-energy"><Cpu size={19}/><span><small>算力</small><strong data-testid="game-energy">{ready ? Math.floor(state.energy) : '—'}</strong></span></div><div className="cs-stat"><Heart size={18}/><span><small>核心完整度</small><strong data-testid="game-hp">{ready ? Math.max(0, Math.ceil(state.hp)) : '—'}<em> / 20</em></strong></span></div><div className="cs-stat"><Waves size={19}/><span><small>入侵波次</small><strong data-testid="game-wave">{ready ? String(Math.floor(state.wave)).padStart(2, '0') : '00'}<em> / 8</em></strong></span></div><div className="cs-hud-actions"><button disabled={!ready} onClick={() => { send('speed', 1); }} aria-label="切换战斗速度">{speed}×</button><button disabled={!ready} onClick={() => void togglePause()} aria-label={paused ? '继续战斗' : '暂停战斗'}>{paused ? <Play size={17}/> : <Pause size={17}/>}</button><button onClick={() => setMuted(!muted)} aria-label={muted ? '开启音效' : '关闭音效'}>{muted ? <VolumeX size={17}/> : <Volume2 size={17}/>}</button></div></div>
          <div ref={boardRef} className="cs-board" data-testid="game-board">
            {ready ? <canvas ref={canvasRef} width={1280} height={720} aria-label="Rurix 原生塔防战场" /> : <img className="cs-arena-cover" src={`${ASSETS}server-garden.png`} alt="夜色中的服务器花园"/>}
            {ready && !finished && <div className="cs-cell-layer">{Array.from({ length: 12 }, (_, index) => {
              const x = [-6.3, -3.5, -0.7, 2.1][index % 4], y = [2.6, 0, -2.6][Math.floor(index / 4)];
              return <button key={index} aria-label={`部署位 ${index + 1}`} data-testid={`cell-${index}`} className={`cs-cell ${cell === index ? 'is-focused' : ''}`} style={{ left: `${(x + 10.6667) / 21.3334 * 100}%`, top: `${(6 - y) / 12 * 100}%` }} disabled={paused || !connected} onClick={() => deploy(index)}><span>{state.cells[index]?.type > 0 ? `Lv.${state.cells[index].level || 1}` : '+'}</span></button>;
            })}</div>}
            <div className="cs-board-label"><span className="cs-live-dot" /> SERVER GARDEN <span>03 ROUTES · 12 NODES</span></div>
            {ready && <div className="cs-stream-label">{connected ? `${Math.round(fps)} FPS` : '连接战场…'}</div>}
            {!ready && <div className="cs-welcome"><div className="cs-welcome-symbol"><Shield size={34}/></div><span className="cs-eyebrow">BUILD. CONNECT. DEFEND.</span><h2>把灵感，<br/>编译成防线。</h2><p>12 个节点，3 条数据通路。<br/>组建你的开发者守护阵容。</p><button className="cs-primary" disabled={!workspace || busy} onClick={() => void start()}>{busy ? '正在启动原生战场…' : '部署防线'}<ArrowRight size={18}/></button><small>点击部署 · 组合协同 · 守住八波入侵</small></div>}
            {paused && !finished && <div className="cs-pause-overlay"><Pause size={38}/><h2>战术暂停</h2><p>每一次部署，都值得想清楚。</p><button className="cs-primary" onClick={() => void togglePause()}>继续战斗 <Play size={17}/></button></div>}
            {finished && <div className="cs-pause-overlay"><Shield size={42}/><span className="cs-eyebrow">MISSION {state.phase === 2 ? 'COMPLETE' : 'REPORT'}</span><h2>{state.phase === 2 ? '编译成功，防线无恙。' : '主分支需要你的支援。'}</h2><p>通过 {Math.max(0, Math.floor(state.wave) - (state.phase === 3 ? 1 : 0))} 波 · 修复 {Math.floor(state.kills)} 个入侵错误</p><button className="cs-primary" disabled={busy} onClick={() => void start()}><RotateCcw size={17}/>重新部署</button></div>}
          </div>
          <div className="cs-battle-footer"><p><Terminal size={15}/><span role="status">{notice}</span></p><button disabled={!ready || paused || finished || state.phase !== 0} className="cs-next" onClick={() => { if (send('next_wave', 1)) { sound(370); } }}>下一波 <kbd>N</kbd><ChevronRight size={17}/></button></div>
          <div className="cs-roster-heading"><h2>部署阵容 <span>OPERATORS</span></h2><span>按 <kbd>1</kbd> — <kbd>4</kbd> 快速切换</span></div>
          <div className="cs-roster">{UNITS.map((unit) => <button key={unit.id} className={`cs-unit ${selection === unit.id ? 'is-selected' : ''}`} style={{ '--unit-color': unit.accent } as React.CSSProperties} onClick={() => choose(unit.id)} aria-pressed={selection === unit.id}><div className="cs-unit-top"><kbd>{unit.id}</kbd><span><Cpu size={11}/>{unit.cost}</span></div><div className={`cs-unit-portrait ${unit.id > 2 ? 'is-character' : ''}`}><img src={ASSETS + unit.image} alt={unit.name}/></div><strong>{unit.name}</strong><small>{unit.role}</small>{selection === unit.id && <span className="cs-unit-check"><Check size={12}/></span>}</button>)}</div>
        </div>
        <aside className="cs-sidebar"><section className="cs-operator-detail"><div className="cs-section-caption">OPERATOR PROFILE <span>0{selection}</span></div><div className={`cs-profile-portrait ${selection > 2 ? 'is-character' : ''}`} style={{ '--unit-color': selectedUnit.accent } as React.CSSProperties}><img src={ASSETS + selectedUnit.image} alt=""/><span className="cs-profile-orbit"/></div><span className="cs-tag">{selectedUnit.tag}</span><h2>{selectedUnit.name}</h2><p>{selectedUnit.description}</p><div className="cs-detail-row"><span>部署算力</span><strong><Cpu size={14}/>{selectedUnit.cost}</strong></div><div className="cs-synergy"><Sparkles size={16}/><div><strong>协同编译</strong><p>{selectedUnit.synergy}</p></div></div><div className="cs-cell-actions"><button disabled={!filledCell || paused || !connected || (cell !== null && (state.cells[cell]?.level >= 3 || state.energy < (state.cells[cell]?.upgradeCost ?? 0)))} onClick={() => { if (cell !== null) { send('upgrade', cell); sound(820); } }}>{cell !== null && state.cells[cell]?.level >= 3 ? '已满级' : '升级'} {cell !== null && state.cells[cell] && state.cells[cell].level < 3 ? `· ${Math.round(state.cells[cell].upgradeCost)} 算力` : '单元'} <ArrowRight size={14}/></button><button disabled={!filledCell || paused || !connected} onClick={() => { if (cell !== null) send('sell', cell); }}>回收</button></div></section>
          <section className="cs-skill-panel"><div className="cs-section-caption">GLOBAL SKILL <kbd>Q</kbd></div><div className="cs-skill-title"><div><Zap size={23}/></div><h3>热修复<small>HOTFIX PULSE</small></h3></div><p>向选定通路释放净化脉冲，造成伤害、减速并标记敌人。</p><div className="cs-lane-picker">{['上路','中路','下路'].map((label, lane) => <button key={lane} aria-pressed={skillLane === lane} onClick={() => setSkillLane(lane)}>{label}</button>)}</div><button disabled={!ready || paused || state.cooldown > 0 || finished} onClick={() => { send('skill', skillLane); sound(180); }}>{state.cooldown > 0 ? `${Math.ceil(state.cooldown)}s 后就绪` : '释放热修复'}<Zap size={15}/></button></section>
          <section className="cs-chain-panel"><span className="cs-section-caption">COMPILATION CHAIN</span><div><span/><i/><span/><i/><span/></div><h3>单元很强，协同更强。</h3><p>将不同开发工具与 AI 伙伴部署在同一路，激活你的组合策略。</p><small><Sparkles size={13}/>{ready ? Math.floor(state.combo) : 0} 级最高协同已激活</small></section>
        </aside>
      </section>
      {error && <div className="cs-error" role="alert"><strong>战场连接需要修复</strong><p>{error}</p><button onClick={() => { setError(''); void start(); }}>重试</button></div>}
      <footer className="cs-page-footer"><span><Code2 size={14}/> RURIX FORGE <i/> 原生 2D 塔防</span><span>一起守住，每一行创造。<span className="cs-footer-star">✦</span></span></footer>
    </div>
    {drawer && <div className="cs-modal-backdrop" onClick={() => setDrawer(null)}><section ref={modalRef} tabIndex={-1} onKeyDown={(event) => { if (event.key === 'Escape') { setDrawer(null); return; } if (event.key !== 'Tab') return; const items = modalRef.current?.querySelectorAll<HTMLElement>('button:not(:disabled), a[href]'); if (!items?.length) return; const first = items[0], last = items[items.length - 1]; if (event.shiftKey && (document.activeElement === first || document.activeElement === modalRef.current)) { event.preventDefault(); last.focus(); } else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); } }} className="cs-modal" role="dialog" aria-modal="true" aria-label={drawer === 'help' ? '作战手册' : '角色档案'} onClick={(event) => event.stopPropagation()}><button className="cs-modal-close" onClick={() => setDrawer(null)} aria-label="关闭"><X size={20}/></button><span className="cs-eyebrow">CODE SENTINELS / FIELD GUIDE</span><h2>{drawer === 'help' ? '你的第一条防线。' : '角色，来自真实的创作。'}</h2>{drawer === 'help' ? <><p>入侵进程从右向左流动。部署守护者，在它们抵达主分支前完成拦截。</p><ol><li><strong>选择单元</strong><span>点击下方角色卡，或按 1–4 切换。</span></li><li><strong>部署并连成工具链</strong><span>点击战场的空位。同一路的不同单元可以协同。</span></li><li><strong>释放入侵波次</strong><span>准备好后点击「下一波」。算力随战斗恢复，用于补充和升级。</span></li><li><strong>留住热修复</strong><span>选定通路后按 Q 发动范围技能；空格暂停，N 进入下一波。</span></li></ol><p className="cs-modal-note">通过八波入侵即获胜。核心完整度归零则防线失守，可以重新部署。</p></> : <><p>采用具体、可追溯的网络二创版本。角色并无统一官方娘化设定，游戏中保留版本与来源。</p><div className="cs-credit"><strong>DeepSeek · 鲸鱼娘女仆版</strong><p>原型：上善无形；女仆二创：ZipZipPipe；造型参考：Neko3000 / Whale-chan。</p><a href="https://www.bilibili.com/video/BV1EvKK6NEoi/" target="_blank" rel="noreferrer">哔哩哔哩形象来源 ↗</a><a href="https://github.com/Neko3000/deepseek-whalechan" target="_blank" rel="noreferrer">参考设定与署名链 ↗</a></div><div className="cs-credit"><strong>GPT · 白龙娘投影版</strong><p>参考 ゆうまEthan〜 / JPEthan 的 Token Monitor 龙娘视频版本。仅采用参考中可见的半身造型。</p><a href="https://www.youtube.com/watch?v=xASRX37IIiY" target="_blank" rel="noreferrer">YouTube 原视频 ↗</a><a href="https://github.com/JPEthan/token-monitor" target="_blank" rel="noreferrer">角色版本来源 ↗</a></div><p className="cs-modal-note">游戏立绘由 Codex 根据参考重新生成，动作由图生视频截帧制作。软件标识归原权利人；来源图仅供研究。此项目为本地同人制作与流程测试，商业使用授权尚未核实。</p></>}</section></div>}
  </main>;
}
