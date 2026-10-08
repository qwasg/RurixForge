/** Narrow LAN game service. Simulation and victories belong exclusively to native authority. */
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { createHash, randomBytes, randomUUID, timingSafeEqual } from 'node:crypto';

export const NET_PROTOCOL = 'code-sentinels-pvp/6.1';
export const NET_PREFIX = '/api/sentinels/v6';
export const RECONNECT_GRACE_MS = 60_000;
export const fault = (status, message) => Object.assign(new Error(message), { status });
const integer = (n, min = 0, max = Number.MAX_SAFE_INTEGER) => Number.isSafeInteger(n) && n >= min && n <= max;
const safeName = value => String(value || '指挥官').replace(/[\x00-\x1f\x7f]/g, '').trim().slice(0, 24) || '指挥官';
const equalToken = (a, b) => typeof a === 'string' && /^[a-f0-9]{64}$/.test(a) && timingSafeEqual(Buffer.from(a), Buffer.from(b));
const canonicalJson = value => Array.isArray(value) ? `[${value.map(canonicalJson).join(',')}]`
  : value && typeof value === 'object' ? `{${Object.keys(value).sort().map(key => `${JSON.stringify(key)}:${canonicalJson(value[key])}`).join(',')}}` : JSON.stringify(value);
export function jsonReply(res, status, value) {
  if (res.headersSent) return res.destroy();
  res.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store', 'x-content-type-options': 'nosniff' });
  res.end(JSON.stringify(value));
}
export async function readJson(req, limit = 64 * 1024) {
  let bytes = 0; const chunks = [];
  for await (const chunk of req) { bytes += chunk.length; if (bytes > limit) throw fault(413, '请求过大'); chunks.push(chunk); }
  try { const value = JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}'); if (!value || Array.isArray(value) || typeof value !== 'object') throw 0; return value; }
  catch { throw fault(400, '请求必须是 JSON 对象'); }
}
export function validateOrder(value) {
  const sequence = value?.sequence ?? value?.seq;
  if (!integer(sequence, 1, 2_147_483_647) || !value.command || Array.isArray(value.command) || typeof value.command !== 'object') throw fault(400, '指令序号或内容无效');
  const command = structuredClone(value.command);
  if (typeof command.op !== 'string' || !/^[a-z][a-z0-9_.-]{0,47}$/.test(command.op)) throw fault(400, '指令类型无效');
  // Ownership is never forwarded from an untrusted command envelope.
  for (const key of ['owner', 'player', 'playerId', 'token', '__proto__', 'constructor', 'prototype']) {
    if (Object.hasOwn(command, key)) throw fault(400, '指令不能指定玩家身份');
  }
  let count = 0;
  const walk = (node, depth = 0) => {
    if (++count > 4096 || depth > 12) throw fault(400, '指令结构过大');
    if (typeof node === 'number' && !Number.isFinite(node)) throw fault(400, '指令数值无效');
    if (typeof node === 'string' && node.length > 512) throw fault(400, '指令文本过长');
    if (node && typeof node === 'object') for (const [key, child] of Object.entries(node)) {
      if (['__proto__', 'constructor', 'prototype'].includes(key)) throw fault(400, '指令字段无效');
      walk(child, depth + 1);
    }
  };
  walk(command); return { sequence, command };
}
const ENTITY_COLLECTIONS = new Set(['buildings','rooms','units','links','walls','resources','projectiles','events','jobs','rubble','defenseFields','shieldRegions']);
function entityIndex(items) {
  if (!Array.isArray(items)) return null;
  const result = new Map();
  for (const item of items) {
    if (!item || typeof item !== 'object' || !integer(item.id) || item.id < 1 || result.has(item.id)) return null;
    result.set(item.id, item);
  }
  return result;
}
function collectionDelta(before, after) {
  const old = entityIndex(before), next = entityIndex(after);
  if (!old || !next) return null;
  const upsert = after.filter(item => !old.has(item.id) || JSON.stringify(old.get(item.id)) !== JSON.stringify(item));
  const remove = before.filter(item => !next.has(item.id)).map(item => item.id);
  const predicted = before.filter(item => next.has(item.id)).map(item => item.id).concat(after.filter(item => !old.has(item.id)).map(item => item.id));
  const reordered = after.some((item, i) => item.id !== predicted[i]);
  return { upsert, remove, ...(reordered ? { order: after.map(item => item.id) } : {}) };
}
export function snapshotEnvelope(previous, current, forceFull = false) {
  if (!current || !integer(current.tick) || !integer(current.revision)) throw fault(502, '原生快照缺少 tick/revision');
  const header = { protocol: NET_PROTOCOL, tick: current.tick, revision: current.revision };
  if (!previous || forceFull || current.revision <= previous.revision) return { ...header, kind: 'full', snapshot: current };
  const changes = {}, removed = [], collections = {};
  for (const [key, value] of Object.entries(current)) {
    if (ENTITY_COLLECTIONS.has(key)) {
      const patch = collectionDelta(previous[key], value);
      if (patch) {
        if (!patch.upsert.length && !patch.remove.length && !patch.order) continue;
        if (JSON.stringify(patch).length < JSON.stringify(value).length) { collections[key] = patch; continue; }
      }
    }
    if (JSON.stringify(value) !== JSON.stringify(previous[key])) changes[key] = value;
  }
  for (const key of Object.keys(previous)) if (!Object.hasOwn(current, key)) removed.push(key);
  const delta = { ...header, kind: 'delta', baseRevision: previous.revision, changes, removed, ...(Object.keys(collections).length ? { collections } : {}) };
  return JSON.stringify(delta).length < JSON.stringify(current).length ? delta : { ...header, kind: 'full', snapshot: current };
}
export function applySnapshotEnvelope(previous, envelope) {
  if (envelope?.protocol !== NET_PROTOCOL || !integer(envelope.revision) || !integer(envelope.tick)) throw fault(409, '快照协议无效');
  if (envelope.kind === 'full') {
    if (envelope.snapshot?.revision !== envelope.revision || envelope.snapshot?.tick !== envelope.tick) throw fault(409, '完整快照版本不匹配');
    return structuredClone(envelope.snapshot);
  }
  if (envelope.kind !== 'delta' || !previous || previous.revision !== envelope.baseRevision || envelope.revision <= envelope.baseRevision) throw fault(409, '增量基线已失效，需要完整快照');
  const next = { ...previous, ...envelope.changes };
  for (const key of envelope.removed || []) delete next[key];
  for (const [key, patch] of Object.entries(envelope.collections || {})) {
    if (!ENTITY_COLLECTIONS.has(key) || Object.hasOwn(envelope.changes || {}, key) || (envelope.removed || []).includes(key)
      || !patch || !Array.isArray(patch.upsert) || !Array.isArray(patch.remove)) throw fault(409, '实体增量格式无效');
    const items = entityIndex(previous[key]), updates = entityIndex(patch.upsert);
    const removedIds = new Set(patch.remove);
    if (!items || !updates || removedIds.size !== patch.remove.length || patch.remove.some(id => !integer(id) || id < 1 || !items.has(id) || updates.has(id))) throw fault(409, '实体增量ID或基线无效');
    for (const id of removedIds) items.delete(id);
    for (const [id, value] of updates) items.set(id, value);
    if (patch.order !== undefined) {
      if (!Array.isArray(patch.order) || patch.order.length !== items.size || new Set(patch.order).size !== items.size
        || patch.order.some(id => !integer(id) || !items.has(id))) throw fault(409, '实体增量顺序无效');
      next[key] = patch.order.map(id => items.get(id));
    } else next[key] = [...items.values()];
  }
  if (next.revision !== envelope.revision || next.tick !== envelope.tick) throw fault(409, '增量快照版本不匹配');
  return next;
}

/** The injectable adapter exists for isolated protocol tests; production supplies native RPC only. */
export function createMultiplayerV6({ authority, journalDir, address = () => null, now = Date.now, graceMs = RECONNECT_GRACE_MS, allowSuggestions = false, initialSequences = [0,0], resumeTick = null } = {}) {
  if (!authority || ['open', 'order', 'snapshot', 'forfeit', 'close'].some(name => typeof authority[name] !== 'function')) throw new Error('A complete native authority adapter is required');
  const rulesHash = authority.rulesHash;
  if (typeof rulesHash !== 'string' || !/^[a-f0-9]{64}$/.test(rulesHash)) throw new Error('Native rules hash required');
  if (!Array.isArray(initialSequences) || initialSequences.length !== 2 || initialSequences.some(n=>!integer(n,0,2147483647))) throw new Error('Invalid saved player sequences');
  let room = null, disposed = false, publishing = false, publishCount = 0;
  let refreshWaiters = [];
  const limits = new Map();
  if (journalDir) fs.mkdirSync(journalDir, { recursive: true });
  const publicRoom = () => room && ({ id: room.id, code: room.code, protocol: NET_PROTOCOL, status: room.status, seed: room.seed, theme: room.theme, ruleset: room.ruleset,
    address: address(), rulesHash, resumed: resumeTick !== null, resumeTick, revision: room.revision, lastTick: room.latest?.tick ?? resumeTick ?? 0, winner: room.latest?.winner ?? null, winReason: room.latest?.winReason ?? '', reconnectGraceSeconds: graceMs / 1000,
    players: [...room.players.values()].map(p => ({ owner: p.owner, nickname: p.nickname, ready: p.ready, connected: now() - p.lastSeen < 6000, forfeited: p.forfeited, lastSequence: p.lastSequence })) });
  const record = (kind, detail = {}) => {
    if (!room) return;
    const event = { protocol: NET_PROTOCOL, roomId: room.id, revision: ++room.revision, at: now(), tick: room.latest?.tick ?? 0, kind, ...detail };
    if (journalDir) fs.appendFileSync(path.join(journalDir, `${room.id}.jsonl`), JSON.stringify(event) + '\n');
    return event;
  };
  const serialize = work => { const result = room.tail.then(work); room.tail = result.catch(() => {}); return result; };
  const playerSession = player => ({ room: publicRoom(), playerId: player.owner, token: player.token, lastSequence: player.lastSequence });
  const authenticate = (req, readOnly = false) => {
    if (!room) throw fault(404, '房间不存在');
    const token = req.headers.authorization?.replace(/^Bearer /, '');
    const player = [...room.players.values()].find(p => equalToken(token, p.token));
    if (!player) throw fault(401, '房间凭据无效');
    if (player.forfeited && !readOnly) throw fault(409, '重连期限已过，对局已判负');
    player.lastSeen = now(); return player;
  };
  const seat = (owner, nickname) => {
    const player = { owner, nickname: safeName(nickname), token: randomBytes(32).toString('hex'), ready: false, lastSeen: now(), lastSequence: initialSequences[owner-1], receipts: new Map(), streams: new Set(), forfeited: false };
    room.players.set(owner, player); return player;
  };
  const send = (res, event, value) => {
    if (res.destroyed || res.writableLength > 1024 * 1024) { res.destroy(); return; }
    res.write(`event: ${event}\ndata: ${JSON.stringify(value)}\n\n`);
  };
  const refresh = async (force = false) => {
    if (!room || !['battle', 'finished'].includes(room.status) || disposed) return room?.latest;
    if (publishing) {
      if (!force) return room.latest;
      await new Promise(resolve => refreshWaiters.push(resolve));
      return refresh(true);
    }
    publishing = true;
    try {
      const active = room;
      const snapshots = await Promise.all([...active.players.keys()].map(async owner => [owner, await authority.snapshot({ owner })]));
      if (room !== active) return null;
      const snapshot = snapshots.find(([owner]) => owner === 1)?.[1] ?? snapshots[0]?.[1];
      const prior = active.latest;
      if (!force && prior?.revision === snapshot.revision && prior?.tick === snapshot.tick) return prior;
      const forceFull = force || ++publishCount % 40 === 0;
      for (const [owner, state] of snapshots) {
        const envelope = snapshotEnvelope(active.views.get(owner), state, forceFull);
        active.views.set(owner, state);
        for (const stream of active.players.get(owner)?.streams || []) send(stream, 'snapshot', { ...envelope, sessionId: active.id, rulesHash });
      }
      room.latest = snapshot;
      if (snapshot.phase === 'finished' || snapshot.winner > 0 || snapshot.result?.winner > 0) room.status = 'finished';
      if (force || !prior || snapshot.tick - room.lastCheckpoint >= 600) {
        room.lastCheckpoint = snapshot.tick; record('checkpoint', { snapshot });
      }
      return snapshot;
    } finally { publishing = false; const waiting=refreshWaiters;refreshWaiters=[];for(const resolve of waiting)resolve(); }
  };
  const publisher = setInterval(() => { refresh().catch(error => { for (const p of room?.players.values() || []) for (const s of p.streams) send(s, 'error', { message: error.message }); }); }, 50);
  publisher.unref();
  const watcher = setInterval(() => {
    if (room?.status === 'lobby') {
      const guest = room.players.get(2);
      if (guest && now() - guest.lastSeen >= graceMs) {
        for (const stream of guest.streams) stream.end();
        room.players.delete(2); room.views.delete(2); record('lobby-seat-expired', { owner: 2 });
      }
      return;
    }
    if (!room || room.status !== 'battle') return;
    for (const player of room.players.values()) if (!player.forfeited && now() - player.lastSeen >= graceMs) {
      player.forfeited = true;
      serialize(async () => { await authority.forfeit({ owner: player.owner, reason: 'disconnect-timeout' }); record('disconnect-forfeit', { owner: player.owner }); await refresh(true); }).catch(error => record('authority-error', { message: error.message }));
    }
  }, 500); watcher.unref();
  const dispose = async () => {
    disposed = true; clearInterval(publisher); clearInterval(watcher);
    if (room) { for (const p of room.players.values()) for (const s of p.streams) s.end(); await room.tail; await authority.close(); room = null; }
  };
  return {
    publicRoom, refresh, dispose,
    async handle(req, res, url) {
      if (!url.pathname.startsWith(NET_PREFIX + '/')) return false;
      try {
        // LAN browsers do not receive local UI, assets, viewport or arbitrary native RPC.
        if (req.headers.origin) throw fault(403, '请使用本机游戏客户端连接局域网');
        const ip = req.socket.remoteAddress, time = now();
        let limit = limits.get(ip); if (!limit || time - limit.at >= 1000) { limit = { at: time, count: 0 }; limits.set(ip, limit); }
        if (++limit.count > 100) throw fault(429, '游戏请求过于频繁');
        const route = url.pathname.slice(NET_PREFIX.length);
        if (req.method === 'GET' && route === '/info') { jsonReply(res, 200, { protocol: NET_PROTOCOL, rulesHash, snapshotHz: 20, simulationHz: 60,
          room: room ? { status: room.status, seatCount: room.players.size, acceptingPlayers: room.status === 'lobby' && !room.players.has(2) } : null }); return true; }
        if (req.method === 'POST' && route === '/rooms') {
          if (!['127.0.0.1', '::1', '::ffff:127.0.0.1'].includes(req.socket.remoteAddress)) throw fault(403, '房间只能由本机创建');
          if (room) throw fault(409, '本机已有房间');
          const input = await readJson(req);
          if (input.protocol !== NET_PROTOCOL) throw fault(409, '游戏协议版本不一致');
          if (input.rulesHash !== rulesHash) throw fault(409, '原生规则版本不一致，请使用相同版本游戏包');
          if (input.theme !== undefined && !['river','mining','highland'].includes(input.theme)) throw fault(400, '地图主题无效');
          if (input.ruleset !== undefined && !['full','classic'].includes(input.ruleset)) throw fault(400, '规则集无效');
          room = { id: randomUUID(), code: randomBytes(3).toString('hex').toUpperCase(), seed: integer(input.seed, 1, 2147483647) ? input.seed : 42,
            theme: input.theme ?? 'river', ruleset: input.ruleset ?? 'full', status: 'lobby', revision: 0, players: new Map(), views: new Map(), latest: null, lastCheckpoint: 0, tail: Promise.resolve() };
          const player = seat(1, input.nickname); record('room-created', { seed: room.seed, theme: room.theme, ruleset: room.ruleset }); jsonReply(res, 201, playerSession(player)); return true;
        }
        if (req.method === 'POST' && route === '/join') {
          const input = await readJson(req);
          if (input.protocol !== NET_PROTOCOL) throw fault(409, '游戏协议版本不一致');
          if (input.rulesHash !== rulesHash) throw fault(409, '原生规则版本不一致，请使用相同版本游戏包');
          if (!room || typeof input.code !== 'string' || !/^[0-9a-f]{6}$/i.test(input.code.trim()) || input.code.trim().toUpperCase() !== room.code) throw fault(404, '房间码无效');
          if (room.status !== 'lobby' || room.players.size >= 2) throw fault(409, '房间已满或对局已开始');
          const player = seat(2, input.nickname); record('player-joined', { owner: 2, nickname: player.nickname }); jsonReply(res, 200, playerSession(player)); return true;
        }
        const readOnly = req.method === 'GET' && ['/room','/snapshot','/stream'].includes(route) || req.method === 'POST' && ['/heartbeat','/leave'].includes(route);
        const player = authenticate(req, readOnly);
        if (req.method === 'GET' && route === '/room') { jsonReply(res, 200, playerSession(player)); return true; }
        if (req.method === 'GET' && route === '/snapshot') {
          if (!room.latest) await refresh(true);
          if (!room.latest) throw fault(409, '对局尚未开始');
          jsonReply(res, 200, { ...snapshotEnvelope(null, room.views.get(player.owner)), sessionId: room.id, rulesHash, room: publicRoom() }); return true;
        }
        if (req.method === 'GET' && route === '/stream') {
          if (!['battle', 'finished'].includes(room.status)) throw fault(409, '对局尚未开始');
          if (!room.latest) await refresh(true);
          for (const old of player.streams) old.end(); player.streams.clear();
          res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive', 'x-accel-buffering': 'no' });
          player.streams.add(res); send(res, 'snapshot', { ...snapshotEnvelope(null, room.views.get(player.owner)), sessionId: room.id, rulesHash }); send(res, 'room', publicRoom());
          res.on('close', () => player.streams.delete(res)); return true;
        }
        if (req.method !== 'POST') throw fault(405, '请求方式无效');
        const input = await readJson(req);
        if (route === '/suggest') {
          if (!allowSuggestions || typeof authority.suggest !== 'function') throw fault(404, '辅助测试接口未启用');
          if (room.status !== 'battle') throw fault(409, '当前没有进行中的对局');
          jsonReply(res,200,await authority.suggest({owner:player.owner,branch:String(input.branch||'algorithm'),style:String(input.style||'mixed-ai')}));return true;
        }
        if (route === '/preview') {
          if (room.status !== 'battle') throw fault(409, '当前没有进行中的对局');
          if (typeof authority.preview !== 'function') throw fault(501, '原生预览接口尚未就绪');
          const { command } = validateOrder({ sequence: 1, command: input.command });
          jsonReply(res, 200, await authority.preview({ owner: player.owner, command })); return true;
        }
        if (route === '/heartbeat') { jsonReply(res, 200, { room: publicRoom() }); return true; }
        if (route === '/ready') {
          if (room.status !== 'lobby' || typeof input.ready !== 'boolean') throw fault(409, '当前不能改变准备状态');
          player.ready = input.ready; record('ready', { owner: player.owner, ready: player.ready }); jsonReply(res, 200, { room: publicRoom() }); return true;
        }
        if (route === '/start') {
          if (player.owner !== 1 || room.status !== 'lobby' || room.players.size !== 2 || [...room.players.values()].some(p => !p.ready || now() - p.lastSeen >= 6000)) throw fault(409, '需要双方在线准备，由房主开始');
          room.status = 'starting';
          try { await authority.open({ mode: 'authority', seed: room.seed, theme: room.theme, ruleset: room.ruleset ?? 'full', localPlayer: 1, opponent: 'human' }); }
          catch (error) { room.status = 'lobby'; throw error; }
          room.status = 'battle'; for (const p of room.players.values()) p.lastSeen = now();
          record('battle-started'); await refresh(true); jsonReply(res, 200, { room: publicRoom(), snapshot: room.latest }); return true;
        }
        if (route === '/orders') {
          if (room.status !== 'battle') throw fault(409, '当前没有进行中的对局');
          const order = validateOrder(input), payload = createHash('sha256').update(canonicalJson(order)).digest('hex');
          const result = await serialize(async () => {
            const previous = player.receipts.get(order.sequence);
            if (previous) { if (previous.payload !== payload) throw fault(409, '重复序号不能更改指令'); return previous.result; }
            if (order.sequence !== player.lastSequence + 1) throw fault(409, `下一个序号应为 ${player.lastSequence + 1}`);
            const receipt = await authority.order({ owner: player.owner, sequence: order.sequence, command: order.command });
            if (typeof receipt?.accepted !== 'boolean' || !integer(receipt.tick)) throw fault(502, '原生回执无效');
            const result = { ...receipt, sequence: order.sequence };
            player.lastSequence = order.sequence; player.receipts.set(order.sequence, { payload, result });
            // Retain the whole match's receipts: a reconnect can retry any already seen sequence safely.
            record('order', { owner: player.owner, order, result }); return result;
          });
          jsonReply(res, 200, result); return true;
        }
        if (route === '/leave') {
          if (room.status === 'battle') { await serialize(() => authority.forfeit({ owner: player.owner, reason: 'left-match' })); player.forfeited = true; record('player-forfeit', { owner: player.owner }); await refresh(true); }
          else if (room.status === 'lobby') { room.players.delete(player.owner); record('player-left', { owner: player.owner }); if (player.owner === 1) { room = null; } }
          jsonReply(res, 200, { left: true }); return true;
        }
        throw fault(404, '游戏网络接口不存在');
      } catch (error) { jsonReply(res, error.status || 500, { error: { message: error.message || '游戏网络请求失败' } }); return true; }
    },
  };
}

export async function startLanServer(options) {
  const service = createMultiplayerV6(options);
  const server = http.createServer(async (req, res) => {
    try { if (!await service.handle(req, res, new URL(req.url, 'http://game.invalid'))) jsonReply(res, 404, { error: { message: 'Only the V6 game protocol is exposed' } }); }
    catch { jsonReply(res, 400, { error: { message: 'Invalid game request' } }); }
  });
  server.requestTimeout = 15_000; server.headersTimeout = 10_000;
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(options.port ?? 0, options.host ?? '0.0.0.0', resolve); });
  return { service, server, port: server.address().port, async close() {
    await service.dispose();
    await new Promise(resolve => {
      const timer=setTimeout(()=>server.closeAllConnections(),3000);timer.unref();
      server.close(()=>{clearTimeout(timer);resolve();});
    });
  } };
}
