import { useEffect, useRef, useState } from 'react';
import { ArrowRight, Check, Copy, Globe2, LoaderCircle, LogOut, Radio, Shield, Swords, X } from 'lucide-react';
import './CommandLobby.css';

const PROTOCOL = 'code-sentinels-pvp/1';
const API = '/api/sentinels/net';
const STORAGE = 'code-sentinels-pvp-session-v1';
type Player = { id: string; nickname: string; owner: number; team: number; ready: boolean; connected: boolean; lastSequence: number };
type Room = { id: string; code: string; revision: number; status: string; hostPlayerId: string; seed: number; map: number; players: Player[]; combatAvailable: boolean };
type Session = { room: Room; playerId: string; token: string };
type Capabilities = { protocol: string; combatAvailable: boolean; address: string | null; message: string };
const maps = ['断点森林', '泄漏湿地', '递归高地'];

/** The lobby is useful before combat sync is attached; capability flags keep the
 * distinction visible. Credentials stay in this tab's sessionStorage only. */
export default function CommandLobby({ onClose }: { onClose?: () => void }) {
  const [cap, setCap] = useState<Capabilities | null>(null), [session, setSession] = useState<Session | null>(null);
  const [nickname, setNickname] = useState('指挥官'), [code, setCode] = useState(''), [map, setMap] = useState(1);
  const [busy, setBusy] = useState(false), [error, setError] = useState(''), [notice, setNotice] = useState(''), [connected, setConnected] = useState(false);
  const mounted = useRef(true), sessionRef = useRef<Session | null>(null), pending = useRef(false), dialog = useRef<HTMLElement>(null);
  const request = async <T,>(route: string, method = 'GET', input?: unknown, token?: string): Promise<T> => {
    const controller = new AbortController(), timeout = window.setTimeout(() => controller.abort(), 8000);
    try {
      const response = await fetch(API + route, { method, signal: controller.signal,
        headers: { ...(input === undefined ? {} : { 'content-type': 'application/json' }), ...(token ? { authorization: 'Bearer ' + token } : {}) },
        body: input === undefined ? undefined : JSON.stringify(input) });
      let result: T & { error?: { message?: string } };
      try { result = await response.json(); } catch { throw new Error('当前入口未启动准备室服务，请使用 V4 独立包或 Start-PVP-Lobby.cmd'); }
      if (!response.ok) throw Object.assign(new Error(result.error?.message || '准备室请求失败'), { status: response.status });
      return result;
    } finally { clearTimeout(timeout); }
  };
  const remember = (next: Session | null) => {
    if (next && sessionRef.current?.room.id === next.room.id && sessionRef.current.room.revision > next.room.revision) return;
    sessionRef.current = next; setSession(next);
    try { if (next) sessionStorage.setItem(STORAGE, JSON.stringify({ id: next.room.id, playerId: next.playerId, token: next.token })); else sessionStorage.removeItem(STORAGE); } catch { /* The room still works with storage disabled. */ }
  };
  useEffect(() => {
    mounted.current = true; const previousFocus = document.activeElement as HTMLElement | null;
    dialog.current?.focus();
    void (async () => {
      try {
        const capability = await request<Capabilities>('/capabilities'); if (!mounted.current) return;
        if (capability.protocol !== PROTOCOL) throw new Error('准备室协议版本不匹配');
        setCap(capability); setConnected(true);
        let saved: { id: string; token: string; playerId: string } | null = null;
        try { saved = JSON.parse(sessionStorage.getItem(STORAGE) || 'null'); } catch { /* Start with an empty room. */ }
        if (saved?.id && saved?.token) {
          try {
            const restored = await request<{ room: Room; playerId: string }>('/rooms/' + encodeURIComponent(saved.id), 'GET', undefined, saved.token);
            if (mounted.current) { remember({ ...restored, token: saved.token }); setNotice('已恢复当前阵营席位'); }
          } catch (e) {
            if (mounted.current) { setError((e as Error).message); if ([401, 404].includes((e as { status?: number }).status || 0)) remember(null); }
          }
        }
      } catch (e) { if (mounted.current) { setConnected(false); setError((e as Error).message); } }
    })();
    return () => { mounted.current = false; previousFocus?.focus(); };
  }, []);
  useEffect(() => {
    if (!session) return;
    const roomId = session.room.id, token = session.token; let active = true, timer = 0;
    const poll = async () => {
      try {
        const result = await request<{ room: Room; playerId: string }>('/rooms/' + roomId, 'GET', undefined, token);
        if (active && sessionRef.current?.room.id === roomId) { remember({ ...result, token }); setConnected(true); setError(''); }
      } catch (e) {
        if (active) { setConnected(false); setError((e as Error).message); if ([401, 404].includes((e as { status?: number }).status || 0)) remember(null); }
      } finally { if (active) timer = window.setTimeout(() => void poll(), 2500); }
    };
    timer = window.setTimeout(() => void poll(), 2500);
    return () => { active = false; clearTimeout(timer); };
  }, [session?.room.id, session?.token]);
  const perform = async (action: () => Promise<void>) => {
    if (pending.current) return; pending.current = true; setBusy(true); setError(''); setNotice('');
    try { await action(); } catch (e) { if (mounted.current) setError((e as Error).message); }
    finally { pending.current = false; if (mounted.current) setBusy(false); }
  };
  const enter = (join: boolean) => void perform(async () => {
    const result = await request<Session>(join ? '/join' : '/rooms', 'POST', { protocol: PROTOCOL, nickname, code: code.trim().toUpperCase(), map });
    if (mounted.current) { remember(result); setConnected(true); }
  });
  const ready = () => void perform(async () => {
    if (!session) return; const player = session.room.players.find(p => p.id === session.playerId);
    const result = await request<{ room: Room }>('/rooms/' + session.room.id + '/ready', 'POST', { ready: !player?.ready }, session.token);
    if (mounted.current) remember({ ...session, room: result.room });
  });
  const leave = () => void perform(async () => {
    if (!session) return;
    try { await request('/rooms/' + session.room.id + '/leave', 'POST', {}, session.token); }
    catch (e) { if (![401, 404].includes((e as { status?: number }).status || 0)) throw e; }
    if (mounted.current) { remember(null); setNotice('已离开准备室'); }
  });
  const copy = async (text: string) => { try { await navigator.clipboard.writeText(text); setNotice('已复制'); } catch { setNotice('请手动复制：' + text); } };
  const room = session?.room, me = room?.players.find(p => p.id === session?.playerId);
  const close = () => { if (onClose) onClose(); };
  return <div className={`command-lobby-backdrop ${onClose ? '' : 'is-standalone'}`}>
    <section className="command-lobby" ref={dialog} tabIndex={-1} role="dialog" aria-modal={Boolean(onClose)} aria-label="玩家对战准备室" onKeyDown={event => {
      event.stopPropagation();
      if (event.key === 'Escape') close();
      if (event.key !== 'Tab') return;
      const focusable = dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled),input,select,a[href]'); if (!focusable?.length) return;
      const first = focusable[0], last = focusable[focusable.length - 1];
      if (event.shiftKey && (document.activeElement === first || document.activeElement === dialog.current)) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    }}>
      {onClose && <button className="command-lobby-close" onClick={close} aria-label="关闭对战准备室"><X size={22}/></button>}
      <header><span><Swords size={17}/> COMMAND / VERSUS</span><h1>对战准备室</h1><p>争夺资源，切断算力，攻破对方的指挥核心。</p></header>
      <div className="command-lobby-status"><i className={connected ? 'is-connected' : ''}/>{connected ? '准备室服务在线' : '等待准备室服务'}<small>1 VS 1 · PVP</small></div>
      {!room ? <div className="command-lobby-entry">
        <label>指挥官代号<input maxLength={24} value={nickname} onChange={e => setNickname(e.target.value)} autoComplete="nickname"/></label>
        <label>战区<select value={map} onChange={e => setMap(Number(e.target.value))}>{maps.map((label, i) => <option value={i + 1} key={label}>{label}</option>)}</select></label>
        <button className="command-lobby-primary" disabled={!cap || busy} onClick={() => enter(false)}>{busy ? <LoaderCircle size={18}/> : <Shield size={18}/>}建立攻防准备室<ArrowRight size={18}/></button>
        <div className="command-lobby-join"><label>房间代码<input value={code} maxLength={6} placeholder="6 位房间代码" onChange={e => setCode(e.target.value.toUpperCase().replace(/[^A-Z2-9]/g, ''))} onKeyDown={e => { if (e.key === 'Enter' && code.length === 6 && cap && !busy) enter(true); }}/></label><button disabled={!cap || busy || code.length !== 6} onClick={() => enter(true)}>加入<ArrowRight size={16}/></button></div>
      </div> : <>
        <div className="command-lobby-room"><div><small>ROOM CODE</small><strong>{room.code}</strong></div><button aria-label="复制房间代码" onClick={() => void copy(room.code)}><Copy size={18}/></button><span>{maps[room.map - 1] || '自定义战区'}<small>地形种子 {room.seed}</small></span></div>
        <div className="command-lobby-seats">{[1, 2].map(owner => {
          const p = room.players.find(player => player.owner === owner);
          return <article className={`owner-${owner} ${p ? 'is-occupied' : ''}`} key={owner}><b>0{owner}</b><Shield size={33}/><h2>{p?.nickname || '等待对手'}</h2><p>{owner === 1 ? '蓝方指挥部' : '红方指挥部'}{p?.id === session?.playerId ? ' · 你' : ''}</p><span>{p ? !p.connected ? '断线 · 席位保留 120 秒' : p.ready ? '已准备' : '布防待命' : '通过代码加入'}</span>{p?.ready && <Check size={21}/>}</article>;
        })}</div>
        <div className="command-lobby-actions"><button className="command-lobby-primary" disabled={busy || !connected || room.status !== 'lobby'} onClick={ready}><Check size={18}/>{me?.ready ? '取消准备' : '准备就绪'}</button><button disabled={busy} onClick={leave}><LogOut size={17}/>退出房间</button></div>
        <button className="command-lobby-start" disabled title="本版只预留多人战斗同步，准备室可实际使用"><Swords size={18}/>多人战斗同步待接入</button>
      </>}
      {error && <p className="command-lobby-error" role="alert">{error}</p>}{notice && <p className="command-lobby-notice" role="status">{notice}</p>}
      <footer><Radio size={20}/><div><strong>联机基础设施已预留</strong><p>房间、阵营、准备状态和短时重连可使用。原生双玩家战斗仍待接入，本版单人经营战场可以正常游玩。</p><p>在独立包运行 <code>Start-PVP-Lobby.cmd</code>，把启动窗口中的局域网地址发给同一网络的朋友。双方打开同一服务器地址，再使用房间代码加入。</p>{cap?.address && <button onClick={() => void copy(cap.address!)}><Globe2 size={14}/>{cap.address}<Copy size={13}/></button>}</div></footer>
    </section>
  </div>;
}
