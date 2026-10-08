import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import net from 'node:net';
import { randomUUID } from 'node:crypto';
import { NET_PROTOCOL, NET_PREFIX, startLanServer, applySnapshotEnvelope, validateOrder, fault } from '../multiplayer-v6.mjs';

export function lanAddress(input) {
  const raw = typeof input === 'string' ? input.trim() : '';
  const match = /^(?:http:\/\/)?(\[[0-9a-f:.]+\]|(?:\d{1,3}\.){3}\d{1,3}):(\d{1,5})\/?$/i.exec(raw);
  if (!match) throw fault(400, '请输入 IP:端口；IPv6 使用 [地址]:端口，不接受账号、域名或路径');
  const host = match[1].replace(/^\[|\]$/g, ''), family = net.isIP(host), port = Number(match[2]);
  if (!family || !Number.isInteger(port) || port < 1 || port > 65535) throw fault(400, 'IP 或游戏端口无效');
  const blockedV4 = octets => octets[0] === 0 || octets[0] >= 224;
  if (family === 4 && blockedV4(host.split('.').map(Number))) throw fault(400, '不能连接未指定、广播、组播或保留地址');
  let url; try { url = new URL(`http://${match[1]}:${port}`); } catch { throw fault(400, 'IP 地址无效'); }
  const normalized = url.hostname.replace(/^\[|\]$/g, '').toLowerCase();
  if (family === 6) {
    if (normalized === '::' || normalized.startsWith('ff')) throw fault(400, '不能连接未指定或组播地址');
    const mapped = /^::ffff:([0-9a-f]{1,4}):([0-9a-f]{1,4})$/.exec(normalized);
    if (mapped) { const high = parseInt(mapped[1], 16), low = parseInt(mapped[2], 16);
      if (blockedV4([high >>> 8, high & 255, low >>> 8, low & 255])) throw fault(400, '不能连接映射的广播、组播或未指定地址'); }
  }
  // Keep an explicitly supplied :80 as well; WHATWG URL.origin drops default ports.
  return `http://${url.hostname}:${port}`;
}
export function createSessionController({ native, root, record = () => {}, allowSuggestions = false, heartbeatMs = 2000, streamStaleMs = 6000 }) {
  let current = null, lan = null, remote = null, token = null, streamAbort = null, heartbeatTimer = null, reconnectTimer = null;
  let latest = null, applying = Promise.resolve(), stopped = false, localSequence = 0, heartbeatBusy = false;
  let resumeSave = null;
  let lastStreamProgressAt = 0;
  const saves = path.join(root, '.forge', 'save', 'v6'); fs.mkdirSync(saves, { recursive: true });
  const rpc = (method, params = {}, timeout) => native.rpc(`game.session.${method}`, params, timeout);
  let rulesPromise = null;
  const rulesIdentity = () => {
    if (!rulesPromise) rulesPromise = rpc('catalog').then(catalog => ({
      rulesVersion: catalog?.rulesVersion, rulesFingerprint: catalog?.rulesFingerprint,
      supported: typeof catalog?.rulesVersion === 'string' && catalog.rulesVersion.length > 0
        && /^[0-9a-f]{64}$/.test(catalog?.rulesFingerprint || ''),
    })).catch(error => { rulesPromise = null; throw error; });
    return rulesPromise;
  };
  const compatibility = (stored, identity) => {
    const legacy = !stored?.rulesVersion || !/^[0-9a-f]{64}$/.test(stored?.rulesFingerprint || '')
      || !stored?.save?.rulesVersion || !/^[0-9a-f]{64}$/.test(stored?.save?.rulesFingerprint || '');
    if (legacy) return { compatible: false, compatibility: 'legacy-unverified', compatibilityReason: '旧V6开发存档缺少规则指纹，原文件已保留；请用生成它的旧版本查看。' };
    if (!identity?.supported) return { compatible: false, compatibility: 'engine-unverified', compatibilityReason: '当前引擎未提供可验证规则指纹，请使用完成更新的版本。' };
    if (stored.version !== 6 || stored.save?.snapshot?.version !== 6
      || stored.rulesVersion !== identity.rulesVersion || stored.rulesFingerprint !== identity.rulesFingerprint
      || stored.save.rulesVersion !== identity.rulesVersion || stored.save.rulesFingerprint !== identity.rulesFingerprint)
      return { compatible: false, compatibility: 'incompatible', compatibilityReason: '存档规则与当前引擎不兼容；不会按新规则静默加载或回放。' };
    return { compatible: true, compatibility: 'compatible', compatibilityReason: '' };
  };
  const adapter = { rulesHash: native.rulesHash, open: async args => {const opened=await rpc('open',args);return resumeSave ? rpc('load',{save:resumeSave}) : opened;}, order: args => rpc('order', args), preview: args => rpc('preview', args), suggest: args => rpc('suggest', args), snapshot: args => rpc('snapshot', args), forfeit: args => rpc('forfeit', args), close: () => rpc('close') };
  const request = async (route, input, authenticated = true) => {
    const response = await fetch(remote + NET_PREFIX + route, { method: input === undefined ? 'GET' : 'POST', headers: {
      ...(input === undefined ? {} : { 'content-type': 'application/json' }), ...(authenticated && token ? { authorization: `Bearer ${token}` } : {}),
    }, body: input === undefined ? undefined : JSON.stringify(input), signal: AbortSignal.timeout(10_000), redirect: 'error' });
    const value = await response.json(); if (!response.ok) throw fault(response.status, value.error?.message || '局域网请求失败'); return value;
  };
  const setRoom = room => {
    if (!current || !room) return;
    current = { ...current, status: room.status, roomId: room.id, code: room.code, theme: room.theme, seed: room.seed, ruleset: room.ruleset ?? current.ruleset, players: room.players, resumed: room.resumed, resumeTick: room.resumeTick, address: current.mode === 'join' ? remote : current.address };
    current.lastSequence = room.players?.find(p => p.owner === current.playerId)?.lastSequence ?? current.lastSequence;
  };
  const apply = envelope => {
    if (stopped || current?.mode !== 'join') throw fault(409, '当前不接收联机快照');
    if (envelope.sessionId !== current?.roomId || envelope.rulesHash !== native.rulesHash) throw fault(409, '快照属于不同对局或规则版本');
    const sessionId = current.roomId;
    latest = applySnapshotEnvelope(latest, envelope);
    const snapshot = latest;
    const work = applying.then(() => {
      if (stopped || current?.roomId !== sessionId) return { applied: false, discarded: true };
      return rpc('applySnapshot', { snapshot });
    }); applying = work.catch(() => {}); return work;
  };
  const connectStream = async () => {
    if (stopped || current?.mode !== 'join' || streamAbort || !['battle', 'finished'].includes(current.status)) return;
    const sessionId = current.roomId;
    const abort = new AbortController(); streamAbort = abort;
    lastStreamProgressAt = Date.now();
    try {
      const response = await fetch(remote + NET_PREFIX + '/stream', { headers: { authorization: `Bearer ${token}` }, signal: abort.signal, redirect: 'error' });
      if (!response.ok) throw new Error(`快照流连接失败 (${response.status})`);
      current.connected = true; current.error = null;
      let text = ''; const decoder = new TextDecoder();
      for await (const chunk of response.body) {
        text += decoder.decode(chunk, { stream: true }); let boundary;
        if (text.length > 32 * 1024 * 1024) throw new Error('快照流消息过大');
        while ((boundary = text.indexOf('\n\n')) !== -1) {
          const frame = text.slice(0, boundary); text = text.slice(boundary + 2);
          const event = /^event: (.+)$/m.exec(frame)?.[1], data = /^data: (.+)$/m.exec(frame)?.[1];
          if (!data) continue;
          if (stopped || current?.roomId !== sessionId || abort.signal.aborted || streamAbort !== abort) return;
          const value = JSON.parse(data);
          if (event === 'snapshot') { await apply(value); if (stopped || current?.roomId !== sessionId || abort.signal.aborted || streamAbort !== abort) return; lastStreamProgressAt = Date.now(); current.connected = true; current.error = null; if (latest.winner > 0) current.status = 'finished'; }
          else if (event === 'room') setRoom(value);
          else if (event === 'error') throw new Error(value.message);
        }
      }
    } catch (error) { if (!abort.signal.aborted && current?.roomId === sessionId) { current.connected = false; current.error = error.message; record('lan-stream-error', { message: error.message }); } }
    finally {
      if (streamAbort === abort) streamAbort = null;
      if (!stopped && current?.roomId === sessionId && current.status === 'battle') reconnectTimer = setTimeout(() => { connectStream().catch(() => {}); }, 1000);
    }
  };
  const heartbeat = async () => {
    if (!current || current.mode === 'solo' || heartbeatBusy) return;
    const sessionId = current.roomId;
    heartbeatBusy = true;
    try {
      const response = await request('/heartbeat', {});
      if (stopped || current?.roomId !== sessionId) return;
      setRoom(response.room);
      if (current.mode !== 'join' || current.status === 'lobby') { current.connected = true; current.error = null; }
      if (current.mode === 'join' && ['battle', 'finished'].includes(current.status)) {
        if (current.status === 'battle' && streamAbort && Date.now() - lastStreamProgressAt > streamStaleMs) {
          const stale = streamAbort; streamAbort = null; stale.abort();
          current.replicaSynced = false; current.connected = false; current.error = '快照通道超时，正在重新同步';
        }
        if (!current.replicaOpened) {
          await rpc('open', { mode: 'replica', localPlayer: 2, seed: current.seed, theme: current.theme, ruleset: current.ruleset ?? 'full', opponent: 'human' });
          if (stopped || current?.roomId !== sessionId) return;
          current.replicaOpened = true;
        }
        if (!current.replicaSynced) {
          await apply(await request('/snapshot'));
          if (stopped || current?.roomId !== sessionId) return;
          current.replicaSynced = true; current.connected = true; current.error = null; lastStreamProgressAt = Date.now();
        }
        if (!streamAbort) connectStream().catch(() => {});
      }
      if (current.mode === 'join' && current.replicaOpened && current.status === 'finished' && !latest?.winner) {
        await apply(await request('/snapshot'));
      }
    } catch (error) { if (current?.roomId === sessionId) { current.connected = false; current.error = error.message; } }
    finally { heartbeatBusy = false; }
  };
  async function leave() {
    stopped = true; clearInterval(heartbeatTimer); clearTimeout(reconnectTimer); streamAbort?.abort(); streamAbort = null;
    if (current && current.mode !== 'solo' && remote) await request('/leave', {}).catch(() => {});
    await applying;
    if (lan) await lan.close(); else await rpc('close').catch(() => {});
    current = null; latest = null; token = null; remote = null; lan = null; localSequence = 0; resumeSave = null;
  }
  async function snapshot() {
    if (!current || current.status === 'lobby') return null;
    const sessionId = current.roomId;
    if (current.mode !== 'join') {
      const snapshot = await rpc('snapshot', { owner: current.playerId });
      if (current?.roomId !== sessionId) return null;
      latest = snapshot;
    }
    if (latest?.winner > 0) current.status = 'finished';
    return latest;
  }
  async function status() { return { protocol: NET_PROTOCOL, playerId: current?.playerId ?? null, session: current, snapshot: await snapshot() }; }
  return {
    status, snapshot, leave,
    async session(input) {
      if (current) throw fault(409, '请先离开当前行动');
      if (!['solo', 'host', 'join'].includes(input.mode)) throw fault(400, '游戏模式无效');
      stopped = false;
      const seed = Number.isSafeInteger(input.seed) && input.seed > 0 ? input.seed : 42;
      const ruleset = input.ruleset === 'full' ? 'full' : 'classic';
      try {
        if (input.mode === 'solo') {
          await rpc('open', { mode: 'authority', localPlayer: 1, seed, theme: input.theme, ruleset, opponent: 'ai' });
          current = { mode: 'solo', status: 'battle', roomId: randomUUID(), playerId: 1, seed, theme: input.theme, ruleset, players: [], lastSequence: 0, connected: true, paused: false };
        } else {
          let result;
          if (input.mode === 'host') {
            let publicAddress;
            lan = await startLanServer({ authority: adapter, port: Number.isInteger(input.port) && input.port >= 1024 && input.port <= 65535 ? input.port : 0,
              journalDir: path.join(root, '.forge', 'replays', 'v6'), address: () => publicAddress, allowSuggestions,
              initialSequences: resumeSave?.sequences ?? [0,0], resumeTick: resumeSave?.snapshot?.tick ?? null });
            const ip = Object.values(os.networkInterfaces()).flat().find(n => n?.family === 'IPv4' && !n.internal)?.address || '127.0.0.1';
            publicAddress = `${ip}:${lan.port}`; remote = `http://127.0.0.1:${lan.port}`;
            result = await request('/rooms', { protocol: NET_PROTOCOL, rulesHash: native.rulesHash, seed, theme: input.theme, ruleset, nickname: input.nickname }, false);
            current = { mode: 'host', playerId: 1, address: publicAddress, seed, theme: input.theme, ruleset, connected: true, lastSequence: 0 };
          } else {
            remote = lanAddress(input.address);
            result = await request('/join', { protocol: NET_PROTOCOL, rulesHash: native.rulesHash, code: input.code, nickname: input.nickname }, false);
            current = { mode: 'join', playerId: 2, address: remote, seed: result.room.seed, theme: result.room.theme, ruleset: result.room.ruleset ?? 'full', connected: true, lastSequence: result.lastSequence };
          }
          token = result.token; setRoom(result.room);
          heartbeatTimer = setInterval(() => { heartbeat().catch(() => {}); }, heartbeatMs); heartbeatTimer.unref();
        }
        record('session-open', { mode: current.mode, seed: current.seed }); return status();
      } catch (error) { await leave(); throw error; }
    },
    async ready(value) { if (!current || current.mode === 'solo') throw fault(409, '当前没有准备室'); const result = await request('/ready', { ready: value }); setRoom(result.room); return status(); },
    async start() { if (current?.mode !== 'host') throw fault(403, '仅房主可开始'); const result = await request('/start', {}); setRoom(result.room); latest = result.snapshot; return status(); },
    async order(value) {
      if (!current || current.status !== 'battle') throw fault(409, '当前没有进行中的行动');
      if (current.replay) throw fault(409, '回放不能接收作战指令');
      const sessionId = current.roomId;
      if (value.sessionId !== undefined && value.sessionId !== sessionId) throw fault(409, '指令属于已结束的旧对局');
      const order = validateOrder(value);
      const result = current.mode === 'solo' ? await rpc('order', { owner: 1, ...order }) : await request('/orders', order);
      if (current?.roomId !== sessionId) throw fault(409, '对局已切换，旧指令回执已丢弃');
      localSequence = Math.max(localSequence, order.sequence); current.lastSequence = Math.max(current.lastSequence ?? 0, result.sequence ?? 0);
      record('order', { owner: current.playerId, order, result }); return result;
    },
    async camera(input) { if (!current) throw fault(409, '请先进入行动'); return rpc('view', { ...input, localPlayer: current.playerId }); },
    async pick(input) {
      if (!current || !['battle','finished'].includes(current.status)) throw fault(409, '当前没有可选取的战场');
      const sessionId=current.roomId;
      if(input.sessionId!==undefined&&input.sessionId!==sessionId)throw fault(409,'选取请求属于旧对局');
      const result=await rpc('pick',{screenX:input.screenX,screenY:input.screenY,width:input.width,height:input.height,view:{...(input.view||{}),localPlayer:current.playerId}});
      if(current?.roomId!==sessionId)throw fault(409,'对局已切换，旧选取已丢弃');
      return result;
    },
    async pause(input) {
      if (!current || current.mode !== 'solo' || current.replay) throw fault(409, '只有单人行动可本地暂停');
      if (typeof input.paused !== 'boolean') throw fault(400, '暂停状态无效');
      const sessionId = current.roomId;
      if (input.sessionId !== undefined && input.sessionId !== sessionId) throw fault(409, '暂停请求属于旧对局');
      if (Boolean(current.paused) !== input.paused) {
        await native.rpc(input.paused ? 'play.pause' : 'play.resume');
        if (current?.roomId !== sessionId) throw fault(409, '对局已切换');
        current.paused = input.paused;
      }
      return status();
    },
    async suggest(input) {
      if (!allowSuggestions) throw fault(404, '辅助测试接口未启用');
      if (!current || current.status !== 'battle' || current.replay) throw fault(409, '当前没有进行中的行动');
      const sessionId = current.roomId;
      if (input.sessionId !== undefined && input.sessionId !== sessionId) throw fault(409, '建议请求属于旧对局');
      const args={branch:String(input.branch || 'algorithm'),style:String(input.style || 'mixed-ai')};
      const result=await (current.mode==='join' ? request('/suggest',args) : rpc('suggest',{owner:current.playerId,...args}));
      if (current?.roomId !== sessionId) throw fault(409, '对局已切换');
      return result;
    },
    async preview(input) {
      if (!current || current.status !== 'battle') throw fault(409, '当前没有进行中的行动');
      const sessionId = current.roomId;
      if (input.sessionId !== undefined && input.sessionId !== sessionId) throw fault(409, '预览属于已结束的旧对局');
      const { command } = validateOrder({ sequence: 1, command: input.command });
      const result = await (current.mode === 'join' ? request('/preview', { command }) : rpc('preview', { owner: current.playerId, command }));
      if (current?.roomId !== sessionId) throw fault(409, '对局已切换，旧预览已丢弃');
      return result;
    },
    async viewport() { return native.rpc('viewport.streamInfo'); },
    async catalog() { return rpc('catalog'); },
    async save(input = {}) {
      if (!current || current.mode === 'join' || current.replay) throw fault(409, '仅非回放的权威主机可保存当前行动');
      const sessionId = current.roomId, mode = current.mode;
      if (input.sessionId !== undefined && input.sessionId !== sessionId) throw fault(409, '存档请求属于旧对局');
      const identity = await rulesIdentity();
      if (!identity.supported) throw fault(409, '当前引擎未提供可验证规则指纹，无法生成已验证存档');
      const save = await rpc('save');
      if (current?.roomId !== sessionId) throw fault(409, '对局已切换，未写入迟到的存档');
      const id = randomUUID(), file = path.join(saves, `${id}.json`);
      const result = { version: 6, rulesVersion: identity.rulesVersion, rulesFingerprint: identity.rulesFingerprint,
        engineSha256: native.rulesHash, id, name: String(input.name || '行动存档').slice(0, 60), createdAt: new Date().toISOString(), mode, save };
      const check = compatibility(result, identity);
      if (!check.compatible) throw fault(409, '原生存档与当前引擎规则指纹不一致，未写入文件');
      fs.writeFileSync(file + '.tmp', JSON.stringify(result)); fs.renameSync(file + '.tmp', file);
      return { id, name: result.name, createdAt: result.createdAt, tick: save.snapshot?.tick };
    },
    async saves() {
      const identity = await rulesIdentity().catch(() => ({ supported: false }));
      return { saves: fs.readdirSync(saves).filter(name => /^[a-f0-9-]{36}\.json$/.test(name)).map(name => {
        try { const s = JSON.parse(fs.readFileSync(path.join(saves, name), 'utf8')); return { id: name.slice(0, -5), name: String(s.name || '未命名存档'), createdAt: String(s.createdAt || ''), tick: s.save?.snapshot?.tick, mode: s.mode,
          rulesVersion: s.rulesVersion, rulesFingerprint: s.rulesFingerprint, ...compatibility(s, identity) }; } catch { return null; }
      }).filter(Boolean).sort((a, b) => b.createdAt.localeCompare(a.createdAt)) };
    },
    async load(input, replay = false) {
      if (current) throw fault(409, '请先离开当前行动再加载');
      if (!/^[a-f0-9-]{36}$/.test(input.id)) throw fault(400, '存档标识无效');
      let stored; try { stored = JSON.parse(fs.readFileSync(path.join(saves, `${input.id}.json`), 'utf8')); } catch { throw fault(404, '存档不存在'); }
      const check = compatibility(stored, await rulesIdentity());
      if (!check.compatible) throw fault(409, check.compatibilityReason);
      const storedRuleset = stored.save.snapshot.ruleset === 'classic' ? 'classic' : 'full';
      if (!replay && stored.mode === 'host' && !stored.save.snapshot.winner) {
        resumeSave = stored.save;
        return this.session({mode:'host',seed:stored.save.snapshot.seed,theme:stored.save.snapshot.theme,ruleset:storedRuleset,nickname:'房主'});
      }
      let result;
      try {
        await rpc('open', { mode: 'authority', localPlayer: 1, seed: stored.save.snapshot.seed, theme: stored.save.snapshot.theme, ruleset: storedRuleset, opponent: stored.save.initialAi ? 'ai' : 'human' });
        result = await rpc(replay ? 'replay' : 'load', { save: stored.save, ...(replay ? { playback: true } : {}) }, 180_000);
      } catch (error) { await rpc('close').catch(() => {}); throw error; }
      current = { mode: 'solo', status: 'battle', roomId: randomUUID(), playerId: 1, seed: stored.save.snapshot.seed, theme: stored.save.snapshot.theme, ruleset: storedRuleset, lastSequence: replay ? 0 : stored.save.sequences?.[0] ?? 0, connected: true, paused: false, replay };
      stopped = false; return { ...await status(), replay: replay ? result : undefined };
    },
    async replayControl(input) {
      if (!current?.replay) throw fault(409, '当前不是回放');
      const result = await rpc('replayControl', input, 180_000); return { ...await status(), replay: result };
    },
  };
}
