/** PVP lobby and server-authoritative command boundary. No peer may call engine RPC.
 * The first release deliberately advertises combatAvailable:false until a native
 * two-player authority adapter is supplied. Rooms, seats, recovery and ordered
 * receipts are implemented; an accepted lobby command is not a combat result.
 */
import { randomBytes, randomUUID, timingSafeEqual } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

export const NET_PROTOCOL = 'code-sentinels-pvp/1';
const PREFIX = '/api/sentinels/net';
const alphabet = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
const integer = (n, min, max) => Number.isInteger(n) && n >= min && n <= max;
const fault = (status, message) => Object.assign(new Error(message), { status });
const cleanName = value => String(value || '指挥官').replace(/[\x00-\x1f\x7f]/g, '').trim().slice(0, 24) || '指挥官';
const equalToken = (a, b) => typeof a === 'string' && /^[a-f0-9]{64}$/.test(a) && timingSafeEqual(Buffer.from(a), Buffer.from(b));

/** Validate the transport envelope separately from native resource/ownership rules.
 * Native adapters MUST recheck terrain, cost, unlocks and entity ownership using
 * the server-derived playerId/owner; never trust fields from the browser.
 */
export function validateOrder(input) {
  const kinds = ['build', 'install-gpu', 'upgrade', 'recycle', 'move', 'stop', 'skill', 'wire', 'wall', 'shield', 'research', 'attack'];
  if (!input || !kinds.includes(input.kind) || !integer(input.sequence, 1, 2147483647)) throw fault(400, '指令类型或序号无效');
  const ids = input.entityIds ?? [];
  if (!Array.isArray(ids) || ids.length > 48 || ids.some(id => !integer(id, 0, 65535)) || new Set(ids).size !== ids.length) throw fault(400, '单位选择无效');
  const cells = input.cells ?? [];
  if (!Array.isArray(cells) || cells.length > 128 || cells.some(cell => !integer(cell, 0, 65535))) throw fault(400, '地图坐标无效');
  const model = input.model ?? 0;
  if (!integer(model, 0, 255)) throw fault(400, '型号无效');
  return { sequence: input.sequence, kind: input.kind, entityIds: ids, cells, model };
}

export function createMultiplayerService({ journalDir, authority = null, address = () => null } = {}) {
  const rooms = new Map(), codes = new Map(), limits = new Map();
  const combatAvailable = Boolean(authority?.start && authority?.execute && authority?.snapshot && authority?.close);
  if (journalDir) fs.mkdirSync(journalDir, { recursive: true });
  const json = (res, status, data) => { res.writeHead(status, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' }); res.end(JSON.stringify(data)); };
  const read = async req => {
    let bytes = 0; const chunks = [];
    for await (const chunk of req) { bytes += chunk.length; if (bytes > 32768) throw fault(413, '请求过大'); chunks.push(chunk); }
    try { const value = JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}'); if (!value || Array.isArray(value) || typeof value !== 'object') throw new Error(); return value; }
    catch { throw fault(400, '请求内容无效'); }
  };
  const snapshot = room => ({
    id: room.id, code: room.code, protocol: NET_PROTOCOL, mode: 'pvp', status: room.status,
    revision: room.revision, hostPlayerId: room.hostPlayerId, seed: room.seed, map: room.map,
    createdAt: room.createdAt, combatAvailable, maxPlayers: 2,
    players: [...room.players.values()].map(p => ({ id: p.id, nickname: p.nickname, owner: p.owner, team: p.owner,
      ready: p.ready, connected: Date.now() - p.lastSeen < 12000, lastSequence: p.lastSequence })),
  });
  const publish = (room, kind, detail = {}) => {
    room.revision++; room.lastActivity = Date.now();
    const event = { protocol: NET_PROTOCOL, roomId: room.id, revision: room.revision, at: new Date().toISOString(), kind, ...detail };
    room.events.push(event); if (room.events.length > 256) room.events.shift();
    if (journalDir) fs.appendFileSync(path.join(journalDir, `${room.id}.jsonl`), JSON.stringify(event) + '\n');
    return event;
  };
  const seat = (room, nickname) => {
    if (room.players.size >= 2) throw fault(409, '房间已满');
    const owner = [...room.players.values()].some(p => p.owner === 1) ? 2 : 1;
    const player = { id: randomUUID(), token: randomBytes(32).toString('hex'), nickname: cleanName(nickname), owner,
      ready: false, lastSeen: Date.now(), lastSequence: 0, receipts: new Map() };
    room.players.set(player.id, player); return player;
  };
  const authenticate = (req, room) => {
    const token = req.headers.authorization?.replace(/^Bearer /, '');
    const player = [...room.players.values()].find(p => equalToken(token, p.token));
    if (!player) throw fault(401, '房间凭据已失效，请重新加入');
    player.lastSeen = Date.now(); return player;
  };
  const session = (room, player) => ({ room: snapshot(room), playerId: player.id, token: player.token });
  const close = room => {
    if (room.status === 'battle' && combatAvailable) Promise.resolve(authority.close(room.id)).catch(() => {});
    codes.delete(room.code); rooms.delete(room.id);
  };
  const reap = setInterval(() => {
    const now = Date.now();
    for (const room of rooms.values()) {
      for (const player of room.players.values()) if (now - player.lastSeen > 120000) {
        room.players.delete(player.id); publish(room, 'player-timeout', { playerId: player.id });
        if (room.hostPlayerId === player.id) room.hostPlayerId = room.players.keys().next().value ?? null;
      }
      if (!room.players.size || now - room.lastActivity > 6 * 60 * 60 * 1000) close(room);
    }
    for (const [ip, limit] of limits) if (now - limit.at > 60000) limits.delete(ip);
  }, 10000); reap.unref();

  return {
    dispose() { clearInterval(reap); for (const room of rooms.values()) close(room); },
    async handle(req, res, url) {
      if (!url.pathname.startsWith(PREFIX + '/')) return false;
      try {
        // Cross-origin websites cannot create sessions or send bearer commands.
        if (req.headers.origin && req.headers.origin !== `http://${req.headers.host}` && req.headers.origin !== `https://${req.headers.host}`) throw fault(403, '请在房间页面中操作');
        const ip = req.socket.remoteAddress || 'local', now = Date.now();
        let limit = limits.get(ip); if (!limit || now - limit.at > 60000) { limit = { at: now, count: 0 }; limits.set(ip, limit); }
        if (++limit.count > 600) throw fault(429, '指令过于频繁，请稍后重试');
        const route = url.pathname.slice(PREFIX.length);
        if (req.method === 'GET' && route === '/capabilities') {
          json(res, 200, { protocol: NET_PROTOCOL, mode: 'pvp', combatAvailable, lobbyAvailable: true,
            maxPlayers: 2, reconnectGraceSeconds: 120, address: address(),
            features: ['rooms', 'owner-seats', 'ready', 'resume-token', 'ordered-commands', 'receipt-journal', 'authority-adapter'],
            message: combatAvailable ? '对战服务器已就绪' : '准备室可使用；双玩家战斗同步预留，当前战斗为单人原生模式' }); return true;
        }
        if (req.method === 'POST' && route === '/rooms') {
          if (rooms.size >= 16) throw fault(503, '准备室已达到容量上限');
          const input = await read(req);
          if (input.protocol !== NET_PROTOCOL) throw fault(409, '客户端协议版本不兼容');
          let code; do { code = [...randomBytes(6)].map(b => alphabet[b % alphabet.length]).join(''); } while (codes.has(code));
          const room = { id: randomUUID(), code, seed: integer(input.seed, 1, 2147483647) ? input.seed : randomBytes(4).readUInt32LE(0) % 2147483646 + 1,
            map: integer(input.map, 1, 3) ? input.map : 1, status: 'lobby', players: new Map(), events: [], revision: 0,
            createdAt: new Date().toISOString(), lastActivity: now, hostPlayerId: null, commandTail: Promise.resolve() };
          const player = seat(room, input.nickname); room.hostPlayerId = player.id;
          rooms.set(room.id, room); codes.set(code, room.id); publish(room, 'room-created', { seed: room.seed, map: room.map, playerId: player.id, owner: player.owner });
          json(res, 201, session(room, player)); return true;
        }
        if (req.method === 'POST' && route === '/join') {
          const input = await read(req); if (input.protocol !== NET_PROTOCOL) throw fault(409, '客户端协议版本不兼容');
          const room = rooms.get(codes.get(String(input.code || '').trim().toUpperCase()));
          if (!room) throw fault(404, '房间不存在或已关闭');
          if (room.status !== 'lobby') throw fault(409, '对局已经开始');
          const player = seat(room, input.nickname); publish(room, 'player-joined', { playerId: player.id, owner: player.owner });
          json(res, 200, session(room, player)); return true;
        }
        const match = /^\/rooms\/([a-f0-9-]{36})(?:\/(ready|leave|events|start|orders|snapshot))?$/.exec(route);
        if (!match) throw fault(404, '网络接口不存在');
        const room = rooms.get(match[1]); if (!room) throw fault(404, '房间已关闭');
        const player = authenticate(req, room), action = match[2]; room.lastActivity = now;
        if (req.method === 'GET' && !action) { json(res, 200, { room: snapshot(room), playerId: player.id }); return true; }
        if (req.method === 'GET' && action === 'events') {
          const since = Number(url.searchParams.get('since') || 0); if (!integer(since, 0, 2147483647)) throw fault(400, '事件游标无效');
          json(res, 200, { room: snapshot(room), events: room.events.filter(e => e.revision > since),
            reset: since > room.revision || (room.events[0]?.revision ?? 0) > since + 1 }); return true;
        }
        if (req.method === 'GET' && action === 'snapshot') {
          if (!combatAvailable || room.status !== 'battle') throw fault(409, '战斗同步接口尚未启动');
          json(res, 200, await authority.snapshot({ roomId: room.id, playerId: player.id, owner: player.owner })); return true;
        }
        if (req.method !== 'POST') throw fault(405, '请求方式无效');
        const input = await read(req);
        if (action === 'ready') {
          if (room.status !== 'lobby' || typeof input.ready !== 'boolean') throw fault(409, '当前不能改变准备状态');
          player.ready = input.ready; publish(room, 'ready', { playerId: player.id, ready: player.ready }); json(res, 200, { room: snapshot(room) }); return true;
        }
        if (action === 'leave') {
          room.players.delete(player.id); if (room.hostPlayerId === player.id) room.hostPlayerId = room.players.keys().next().value ?? null;
          publish(room, 'player-left', { playerId: player.id }); if (!room.players.size) close(room);
          json(res, 200, { left: true }); return true;
        }
        if (action === 'start') {
          if (!combatAvailable) throw fault(501, '双玩家战斗同步尚未接入，准备室不会伪装成可对战');
          if (player.id !== room.hostPlayerId || room.status !== 'lobby' || room.players.size !== 2 || [...room.players.values()].some(p => !p.ready || now - p.lastSeen > 12000)) throw fault(409, '需要双方在线准备，由房主发起');
          room.status = 'starting';
          try { await authority.start({ roomId: room.id, seed: room.seed, map: room.map, players: [...room.players.values()].map(p => ({ id: p.id, owner: p.owner })) }); }
          catch (e) { room.status = 'lobby'; throw e; }
          room.status = 'battle'; publish(room, 'battle-started'); json(res, 200, { room: snapshot(room) }); return true;
        }
        if (action === 'orders') {
          if (!combatAvailable || room.status !== 'battle') throw fault(501, '战斗指令接口预留，当前没有多人战斗');
          const order = validateOrder(input);
          const dispatch = async () => {
            if (!room.players.has(player.id)) throw fault(401, '玩家已离开房间');
            const previous = player.receipts.get(order.sequence);
            if (previous) { if (previous.payload !== JSON.stringify(order)) throw fault(409, '同一序号不得更改指令'); return previous.receipt; }
            if (order.sequence !== player.lastSequence + 1) throw fault(409, `下一个指令序号应为 ${player.lastSequence + 1}`);
            // The adapter returns accepted:false for game-rule rejections and
            // must supply a stable tick. No browser-side resource prediction.
            const result = await authority.execute({ roomId: room.id, playerId: player.id, owner: player.owner, order });
            if (!result || typeof result.accepted !== 'boolean' || !integer(result.tick, 0, Number.MAX_SAFE_INTEGER)) throw fault(502, '权威服务器回执无效');
            player.lastSequence = order.sequence;
            const event = publish(room, 'order-receipt', { playerId: player.id, owner: player.owner, sequence: order.sequence, order, result });
            const receipt = { protocol: NET_PROTOCOL, sequence: order.sequence, revision: event.revision, ...result };
            player.receipts.set(order.sequence, { payload: JSON.stringify(order), receipt });
            if (player.receipts.size > 256) player.receipts.delete(player.receipts.keys().next().value);
            return receipt;
          };
          const work = room.commandTail.then(dispatch); room.commandTail = work.catch(() => {}); json(res, 200, await work); return true;
        }
        throw fault(404, '网络接口不存在');
      } catch (error) { json(res, error.status || 500, { error: { message: error.status ? error.message : '准备室服务暂时无法完成请求' } }); return true; }
    },
  };
}
