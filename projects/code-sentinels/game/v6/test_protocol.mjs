/** HTTP and transport tests with an explicit adapter double. NOT native gameplay evidence. */
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { NET_PROTOCOL, NET_PREFIX, startLanServer, snapshotEnvelope, applySnapshotEnvelope, validateOrder } from '../multiplayer-v6.mjs';
import { lanAddress } from './session-controller.mjs';

const checks = [], dir = path.dirname(fileURLToPath(import.meta.url));
let time = 1000, tick = 0, active = false, winner = 0;
const calls = [];
const adapter = { rulesHash: '0'.repeat(64),
  async open(input) { calls.push({ kind: 'open', input }); active = true; },
  async order(input) { calls.push({ kind: 'order', input }); return { accepted: input.command.op !== 'invalid', sequence: input.sequence, tick: ++tick, reason: '' }; },
  async preview(input) { calls.push({ kind: 'preview', input }); return { valid: true, cost: { credits: 12, compute: 0, science: 0 }, powerBefore: 150, powerAfter: 150, demandBefore: 20, demandAfter: 20 }; },
  async snapshot(input) { return { version: 6, tick, revision: tick, players: [{ id: 1 }, { id: 2 }], terrain: Array(4096).fill(0), visible: [input.owner], winner }; },
  async forfeit(input) { calls.push({ kind: 'forfeit', input }); winner = input.owner === 1 ? 2 : 1; ++tick; },
  async close() { active = false; },
};
const server = await startLanServer({ authority: adapter, host: '127.0.0.1', now: () => time });
const origin = `http://127.0.0.1:${server.port}`;
const call = async (route, { input, token, originHeader, path: requestPath } = {}) => {
  const response = await fetch(origin + (requestPath || NET_PREFIX + route), { method: input === undefined ? 'GET' : 'POST', headers: {
    ...(input === undefined ? {} : { 'content-type': 'application/json' }), ...(token ? { authorization: `Bearer ${token}` } : {}), ...(originHeader ? { origin: originHeader } : {}),
  }, body: input === undefined ? undefined : JSON.stringify(input) });
  return { status: response.status, body: await response.json() };
};
try {
  assert.equal((await call('/info')).body.protocol, NET_PROTOCOL);
  for (const requestPath of ['/api/forge/mcp/call', '/api/v6/order', '/viewport.frame', '/Web/index.html', '/health']) assert.equal((await call('', { path: requestPath })).status, 404);
  assert.equal((await call('/info', { originHeader: 'http://malicious.example' })).status, 403);
  checks.push('LAN exposes only narrow game protocol; rejects browser-origin and Forge/viewport/file routes');
  assert.equal((await call('/rooms', { input: { protocol: 'wrong' } })).status, 409);
  const host = (await call('/rooms', { input: { protocol: NET_PROTOCOL, rulesHash: adapter.rulesHash, nickname: '蓝方', seed: 91 } })).body;
  assert.equal(host.playerId, 1); assert.match(host.token, /^[a-f0-9]{64}$/);
  const mismatch = await call('/join', { input: { protocol: NET_PROTOCOL, rulesHash: 'f'.repeat(64), code: host.room.code } });
  assert.equal(mismatch.status, 409); assert.match(mismatch.body.error.message, /规则版本/);
  for (const code of [undefined, '', 'nothex', host.room.code === '000000' ? '000001' : '000000']) {
    assert.equal((await call('/join', { input: { protocol: NET_PROTOCOL, rulesHash: adapter.rulesHash, code } })).status, 404);
  }
  let guest = (await call('/join', { input: { protocol: NET_PROTOCOL, rulesHash: adapter.rulesHash, nickname: '红方', code: host.room.code.toLowerCase() } })).body;
  assert.equal(guest.playerId, 2);
  assert.equal((await call('/join', { input: { protocol: NET_PROTOCOL, rulesHash: adapter.rulesHash, code: host.room.code } })).status, 409);
  const anonymous = (await call('/info')).body;
  assert.deepEqual(Object.keys(anonymous.room).sort(), ['acceptingPlayers','seatCount','status']);
  const info = JSON.stringify(anonymous); for (const secret of [host.token,guest.token,host.room.code,'蓝方','红方']) assert.ok(!info.includes(secret));
  checks.push('join requires a valid case-insensitive room code; anonymous info reveals no code, token or player names');
  const expiredToken=guest.token;time+=61000;await new Promise(resolve=>setTimeout(resolve,550));
  assert.equal((await call('/info')).body.room.acceptingPlayers,true);
  assert.equal((await call('/heartbeat',{input:{},token:expiredToken})).status,401);
  guest=(await call('/join',{input:{protocol:NET_PROTOCOL,rulesHash:adapter.rulesHash,code:host.room.code,nickname:'红方'}})).body;
  assert.notEqual(guest.token,expiredToken);
  checks.push('an abandoned lobby guest seat expires without declaring a battle loss and can be rejoined with the code');
  assert.equal((await call('/room')).status, 401);
  assert.equal((await call('/start', { input: {}, token: host.token })).status, 409);
  for (const p of [host, guest]) assert.equal((await call('/ready', { input: { ready: true }, token: p.token })).status, 200);
  assert.equal((await call('/start', { input: {}, token: guest.token })).status, 409);
  assert.equal((await call('/start', { input: {}, token: host.token })).status, 200);
  assert.equal(active, true);
  checks.push('server creates owner seats, protects credentials, requires both ready, only host starts');
  const previewTick = tick;
  const preview = await call('/preview', { input: { owner: 2, command: { op: 'wall', kind: 'physical', path: [{ x: 1, y: 1, z: 0 }] } }, token: host.token });
  assert.equal(preview.status, 200); assert.equal(preview.body.valid, true); assert.equal(tick, previewTick);
  assert.equal(calls.find(c => c.kind === 'preview').input.owner, 1);
  checks.push('rules hash rejects different native versions; read-only preview derives owner and does not consume a sequence');
  const order = { sequence: 1, owner: 2, command: { op: 'move', ids: [10], pos: { x: 5, y: 6, z: -1 } } };
  const first = await call('/orders', { input: order, token: host.token });
  const again = await call('/orders', { input: order, token: host.token });
  assert.deepEqual(first.body, again.body); assert.equal(calls.filter(c => c.kind === 'order').length, 1);
  const reordered=await call('/orders',{input:{command:{pos:{z:-1,y:6,x:5},ids:[10],op:'move'},sequence:1},token:host.token});
  assert.deepEqual(reordered.body,first.body);assert.equal(calls.filter(c=>c.kind==='order').length,1);
  assert.equal(calls.find(c => c.kind === 'order').input.owner, 1);
  assert.equal((await call('/orders', { input: { ...order, command: { ...order.command, owner: 2 } }, token: host.token })).status, 400);
  assert.equal((await call('/orders', { input: { ...order, command: { ...order.command, ids: [11] } }, token: host.token })).status, 409);
  assert.equal((await call('/orders', { input: { ...order, sequence: 3 }, token: host.token })).status, 409);
  const duplicate = { ...order, sequence: 2 };
  const pair = await Promise.all([call('/orders', { input: duplicate, token: host.token }), call('/orders', { input: duplicate, token: host.token })]);
  assert.deepEqual(pair[0].body, pair[1].body); assert.equal(calls.filter(c => c.kind === 'order').length, 2);
  const rejected = await call('/orders', { input: { sequence: 3, command: { op: 'invalid' } }, token: host.token });
  assert.equal(rejected.body.accepted, false);
  assert.deepEqual((await call('/orders', { input: { sequence: 3, command: { op: 'invalid' } }, token: host.token })).body, rejected.body);
  checks.push('native identity binding, contiguous sequence, concurrent dedup, rejection receipts stay idempotent');
  await server.service.refresh(true);
  const hostState = (await call('/snapshot', { token: host.token })).body.snapshot;
  const guestState = (await call('/snapshot', { token: guest.token })).body.snapshot;
  assert.deepEqual(hostState.visible, [1]); assert.deepEqual(guestState.visible, [2]);
  const next = { ...hostState, tick: hostState.tick + 3, revision: hostState.revision + 3, winner: 1 };
  const delta = snapshotEnvelope(hostState, next);
  assert.equal(delta.kind, 'delta'); assert.deepEqual(applySnapshotEnvelope(hostState, delta), next);
  assert.throws(() => applySnapshotEnvelope({ ...hostState, revision: 99 }, delta));
  assert.deepEqual(applySnapshotEnvelope(null, snapshotEnvelope(null, next)), next);
  checks.push('owner-filtered snapshots, actual compact deltas, full recovery, wrong-baseline rejection');
  const abort = new AbortController();
  const stream = await fetch(origin + NET_PREFIX + '/stream', { headers: { authorization: `Bearer ${guest.token}` }, signal: abort.signal });
  assert.equal(stream.headers.get('content-type'), 'text/event-stream');
  const firstStream = await stream.body.getReader().read(); assert.match(new TextDecoder().decode(firstStream.value), /event: snapshot/); abort.abort();
  checks.push('real authenticated HTTP SSE stream begins with full snapshot');
  time += 59_000; await call('/heartbeat', { input: {}, token: host.token });
  await new Promise(resolve => setTimeout(resolve, 550)); assert.equal(calls.filter(c => c.kind === 'forfeit').length, 0);
  time += 2000; await call('/heartbeat', { input: {}, token: host.token });
  await new Promise(resolve => setTimeout(resolve, 550));
  assert.deepEqual(calls.find(c => c.kind === 'forfeit')?.input, { owner: 2, reason: 'disconnect-timeout' });
  assert.equal((await call('/heartbeat', { input: {}, token: guest.token })).status, 200);
  assert.equal((await call('/snapshot', { token: guest.token })).body.snapshot.winner, 1);
  assert.equal((await call('/orders', { input: {sequence:1,command:{op:'shield',enabled:true}}, token: guest.token })).status, 409);
  checks.push('60-second grace invokes authority forfeit once; expired seat reads final result but cannot resume commands');
  assert.equal(lanAddress('192.168.1.20:54321'), 'http://192.168.1.20:54321');
  assert.equal(lanAddress('8.8.8.8:80'),'http://8.8.8.8:80');
  assert.equal(lanAddress('[2001:db8::10]:6066'),'http://[2001:db8::10]:6066');
  assert.equal(lanAddress('[::1]:6066'),'http://[::1]:6066');
  for (const input of ['https://192.168.1.20:80', 'http://example.com:80', 'http://127.0.0.1:80/api/forge', 'http://user:secret@127.0.0.1:80', '0.0.0.0:6066', '255.255.255.255:6066', '224.1.2.3:6066', '[::]:6066', '[ff02::1]:6066', '[::ffff:255.255.255.255]:6066', '127.0.0.1:0', '127.0.0.1:65536', '127.0.0.1', '127.1:6066', '2130706433:6066', 'http://127.0.0.1:6066?x=1', 'http://127.0.0.1:6066#room', 'http://127.0.0.1:6066/%2e%2e']) assert.throws(() => lanAddress(input));
  assert.throws(() => validateOrder({ sequence: 1, command: { op: 'move', x: Infinity } }));
  checks.push('direct public/private IPv4 and IPv6 literals require explicit ports and reject multicast, unspecified, broadcast, credentials, paths and invalid numbers; no external connection performed');
  const report = { testedAt: new Date().toISOString(), passed: true, checks, nativeGameplayTested: false, scope: 'Real Node HTTP/SSE and transport logic using explicit authority adapter double. No native economy or combat claim.' };
  fs.writeFileSync(path.join(dir, 'protocol-tests.json'), JSON.stringify(report, null, 2)); console.log(JSON.stringify(report));
} finally { await server.close(); }
