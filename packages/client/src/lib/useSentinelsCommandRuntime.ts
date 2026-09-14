import { useCallback, useEffect, useRef, useState } from 'react';
import { apiGet, callTool } from './forgeApi';
import { useWorkspaceStore, type ForgeWorkspace } from './workspaceStore';
import { openViewportStream, type StreamFrame, type ViewportStreamHandle } from './viewportStream';
import { INITIAL_V4, decodeV4, GRID_HEIGHT, FEEDBACK, type V4State } from './sentinelsV4';
import { decodeV5, type V5AnimationState } from './sentinelsV5';

/** The native engine owns the world. Client state consists solely of published snapshots and UI intentions. */
export function useSentinelsCommandRuntime(version: 4 | 5 = 4) {
  const [workspace, setWorkspace] = useState<ForgeWorkspace | null>(null);
  const [state, setState] = useState<V4State>(INITIAL_V4);
  const [animationState, setAnimationState] = useState<V5AnimationState | null>(null);
  const [savedUnlocked, setSavedUnlocked] = useState(1);
  const [ready, setReady] = useState(false), [nativeReady, setNativeReady] = useState(false);
  const [busy, setBusy] = useState(false), [connected, setConnected] = useState(false), [paused, setPaused] = useState(false);
  const [generation, setGeneration] = useState(0), [connectionEpoch, setConnectionEpoch] = useState(0), [fps, setFps] = useState(0);
  const [error, setError] = useState(''), [notice, setNotice] = useState('在地形上建立电力、算力与防御网络');
  const [noticeRevision, setNoticeRevision] = useState(0);
  const canvasRef = useRef<HTMLCanvasElement>(null), streamRef = useRef<ViewportStreamHandle | null>(null);
  const frame = useRef<StreamFrame | null>(null), epoch = useRef(0), startLock = useRef(false), pauseLock = useRef(false);
  const lastFeedback = useRef('');

  useEffect(() => {
    const controller = new AbortController();
    void fetch('/api/sentinels/campaign-progress', { signal: controller.signal, cache: 'no-store' }).then(async response => {
      if (!response.ok) return;
      const data = await response.json();
      if (!controller.signal.aborted && Number.isInteger(data.unlocked) && data.unlocked >= 1 && data.unlocked <= 3) setSavedUnlocked(data.unlocked);
    }).catch(() => { /* The first sector remains available; live native state supplies unlocks after entry. */ });
    return () => controller.abort();
  }, []);

  useEffect(() => {
    let active = true;
    void apiGet<{ workspaces: ForgeWorkspace[] }>('/api/forge/workspaces').then(({ workspaces }) => {
      const id = new URLSearchParams(location.search).get('workspace');
      const found = id ? workspaces.find(w => w.id === id) : workspaces.find(w => /[\\/]code-sentinels(?:[\\/]|$)/i.test(w.root));
      if (!found) throw new Error('找不到编译防线工作区');
      if (active) { useWorkspaceStore.getState().setActive(found.id); setWorkspace(found); }
    }).catch(e => { if (active) setError((e as Error).message); });
    return () => { active = false; };
  }, []);

  const start = useCallback(async () => {
    if (!workspace || startLock.current) return false;
    startLock.current = true; setBusy(true); setReady(false); setNativeReady(false); setConnected(false);
    setError(''); setNotice('指挥网络接入中…'); epoch.current++; frame.current = null; lastFeedback.current = '';
    try {
      useWorkspaceStore.getState().setActive(workspace.id);
      const summary = await callTool<{ playState: string }>('scene_summary');
      if (summary.playState !== 'edit') await callTool('play_exit');
      await callTool('asset_reload');
      await callTool('scene_load', { path: version === 5 ? 'Content/Scenes/CommandV5.rxscene' : 'Content/Scenes/Command.rxscene' });
      await callTool('viewport_set_camera', { target: [0, 0, 0], yaw: 0, pitch: 0, dist: GRID_HEIGHT, ortho: true, orthoSize: GRID_HEIGHT / 2 });
      await callTool('play_enter');
      setState(INITIAL_V4); setAnimationState(null); setPaused(false); setGeneration(g => g + 1); setReady(true);
      setNotice('从电站开始：B 建筑牌组 → 风力发电 → 数据中心 → L 拉电线');
      return true;
    } catch (e) { setError((e as Error).message); return false; }
    finally { startLock.current = false; setBusy(false); }
  }, [workspace, version]);

  useEffect(() => {
    if (!ready || !workspace) return;
    let cancelled = false, raf = 0, timer = 0, lastFrame = -1, invalid = 0;
    const activeEpoch = ++epoch.current;
    const stream = openViewportStream({ width: 1280, height: 720, maxFps: 40,
      onFrame: f => { if (!cancelled && activeEpoch === epoch.current) frame.current = f; },
      onStatus: value => { if (!cancelled && activeEpoch === epoch.current) setFps(value.fps); },
      onError: message => { if (!cancelled && activeEpoch === epoch.current) setError(message); },
      onChannel: up => {
        if (cancelled || activeEpoch !== epoch.current) return;
        setConnected(up); setNativeReady(false); lastFeedback.current = '';
        if (up) setConnectionEpoch(value => value + 1);
        else setNotice('网络中断 · 等待原生状态恢复后重新下达指令');
      },
    });
    streamRef.current = stream;
    const draw = () => {
      const next = frame.current, canvas = canvasRef.current;
      if (next && canvas && next.frameId !== lastFrame) {
        if (canvas.width !== next.width || canvas.height !== next.height) { canvas.width = next.width; canvas.height = next.height; }
        const bytes = new Uint8ClampedArray(next.rgba.length); bytes.set(next.rgba);
        canvas.getContext('2d')?.putImageData(new ImageData(bytes, next.width, next.height), 0, 0);
        lastFrame = next.frameId;
      }
      raf = requestAnimationFrame(draw);
    };
    const poll = async () => {
      try {
        const data = await callTool<{ entities: Array<{ name: string; transform: { translation: number[]; scale: number[] } }> }>('entity_list');
        if (cancelled || activeEpoch !== epoch.current) return;
        const animated = version === 5 ? decodeV5(data.entities) : null;
        const next = version === 5 ? animated : decodeV4(data.entities);
        if (!next) { setNativeReady(false); if (++invalid > 5) throw new Error('未收到完整的指挥战场状态'); return; }
        invalid = 0; setState(next); setAnimationState(animated?.animation ?? null); setNativeReady(stream.up); setError('');
        const feedback = `${next.commandSeq}:${next.feedback}`;
        if (next.feedback && feedback !== lastFeedback.current) {
          setNotice(FEEDBACK[next.feedback] ?? '指令已执行'); setNoticeRevision(value => value + 1);
        }
        lastFeedback.current = feedback;
      } catch (e) {
        if (!cancelled && activeEpoch === epoch.current) { setNativeReady(false); setError((e as Error).message); }
      } finally { if (!cancelled && activeEpoch === epoch.current) timer = window.setTimeout(() => void poll(), 250); }
    };
    raf = requestAnimationFrame(draw); void poll();
    return () => { cancelled = true; cancelAnimationFrame(raf); clearTimeout(timer); stream.close(); if (streamRef.current === stream) streamRef.current = null; };
  }, [ready, workspace, generation, version]);

  const send = useCallback((command: number) => {
    if (!nativeReady || busy || !streamRef.current?.up) { setNotice('连接未就绪，指令未发送'); return false; }
    if (paused) { setNotice('先继续战斗，再下达指令'); return false; }
    const accepted = streamRef.current.sendInput(version === 5 ? 'cs5' : 'cs4', command);
    if (!accepted) setNotice('指令未发送，请等待连接恢复');
    return accepted;
  }, [nativeReady, busy, paused, version]);

  const sendMany = useCallback((commands: number[]) => {
    let sent = 0;
    for (const command of commands) { if (!send(command)) break; sent++; }
    return sent;
  }, [send]);

  const togglePause = useCallback(async () => {
    if (!ready || busy || pauseLock.current) return;
    pauseLock.current = true;
    try { await callTool(paused ? 'play_resume' : 'play_pause'); setPaused(!paused); }
    catch (e) { setError((e as Error).message); }
    finally { pauseLock.current = false; }
  }, [ready, busy, paused]);

  const reconnect = useCallback(() => {
    if (!ready || busy) return;
    epoch.current++; setConnected(false); setNativeReady(false); setError(''); setGeneration(value => value + 1);
    setNotice('正在恢复指挥连接，保留当前原生战局');
  }, [ready, busy]);

  return { workspace, state, animationState, savedUnlocked, ready, nativeReady, busy, connected, paused, fps, error, notice, noticeRevision, connectionEpoch,
    setNotice, start, send, sendMany, togglePause, reconnect, canvasRef };
}
