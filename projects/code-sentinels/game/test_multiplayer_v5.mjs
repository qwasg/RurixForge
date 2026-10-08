/** Real HTTP lobby/transport tests. This does not claim to test native PVP combat. */
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { createMultiplayerService, NET_PROTOCOL } from './multiplayer-v4.mjs';

const results = [];
async function withServer(authority, run) {
  const service = createMultiplayerService({ authority });
  const server = http.createServer(async (req, res) => {
    if (!await service.handle(req, res, new URL(req.url, `http://${req.headers.host}`))) { res.writeHead(404); res.end(); }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  const call = async (route, { method = 'GET', input, token, origin } = {}) => {
    const response = await fetch(base + '/api/sentinels/net' + route, { method,
      headers: { ...(input === undefined ? {} : { 'content-type': 'application/json' }), ...(token ? { authorization: 'Bearer ' + token } : {}), ...(origin ? { origin } : {}) },
      body: input === undefined ? undefined : JSON.stringify(input) });
    return { status: response.status, body: await response.json() };
  };
  try { await run(call); }
  finally { service.dispose(); server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
}

await withServer(null, async call => {
  let response = await call('/capabilities');
  assert.equal(response.status, 200); assert.equal(response.body.protocol, NET_PROTOCOL); assert.equal(response.body.combatAvailable, false);
  results.push({ case: 'capability truthfully reports lobby-only PVP support', pass: true });
  assert.equal((await call('/rooms', { method: 'POST', input: { protocol: 'wrong' } })).status, 409);
  response = await call('/rooms', { method: 'POST', input: { protocol: NET_PROTOCOL, nickname: '蓝方', seed: 42, map: 2 } });
  assert.equal(response.status, 201); const blue = response.body, id = blue.room.id;
  assert.equal(blue.room.players[0].owner, 1); assert.equal(blue.room.seed, 42);
  const red = (await call('/join', { method: 'POST', input: { protocol: NET_PROTOCOL, nickname: '红方', code: blue.room.code.toLowerCase() } })).body;
  assert.equal(red.room.players.length, 2); assert.equal(red.room.players.find(p => p.id === red.playerId).owner, 2);
  assert.equal((await call('/join', { method: 'POST', input: { protocol: NET_PROTOCOL, code: blue.room.code } })).status, 409);
  results.push({ case: 'real room create/join, server assigned teams and two-player capacity', pass: true });
  assert.equal((await call('/rooms/' + id)).status, 401);
  assert.equal((await call('/rooms/' + id, { token: 'a'.repeat(64) })).status, 401);
  assert.equal((await call('/rooms/' + id, { token: blue.token, origin: 'https://unrelated.invalid' })).status, 403);
  results.push({ case: 'room credentials and same-origin boundary', pass: true });
  for (const player of [blue, red]) assert.equal((await call(`/rooms/${id}/ready`, { method: 'POST', token: player.token, input: { ready: true } })).status, 200);
  response = await call('/rooms/' + id, { token: blue.token });
  assert.equal(response.body.playerId, blue.playerId); assert.equal(response.body.room.players.length, 2);
  assert.ok(response.body.room.players.every(p => p.ready && p.connected));
  const events = (await call(`/rooms/${id}/events?since=0`, { token: blue.token })).body.events;
  assert.ok(events.some(e => e.kind === 'ready')); assert.ok(events.every((e, i) => i === 0 || e.revision > events[i - 1].revision));
  results.push({ case: 'ready state, token resume and ordered room events', pass: true });
  assert.equal((await call(`/rooms/${id}/start`, { method: 'POST', token: blue.token, input: {} })).status, 501);
  assert.equal((await call(`/rooms/${id}/orders`, { method: 'POST', token: blue.token, input: { sequence: 1, kind: 'move' } })).status, 501);
  results.push({ case: 'unimplemented native PVP cannot be started or simulated by the lobby', pass: true });
  await call(`/rooms/${id}/leave`, { method: 'POST', token: blue.token, input: {} });
  response = await call('/rooms/' + id, { token: red.token });
  assert.equal(response.body.room.hostPlayerId, red.playerId); assert.equal(response.body.room.players.length, 1);
  assert.equal((await call('/rooms/' + id, { token: blue.token })).status, 401);
  await call(`/rooms/${id}/leave`, { method: 'POST', token: red.token, input: {} });
  assert.equal((await call('/rooms/' + id, { token: red.token })).status, 404);
  results.push({ case: 'leave releases the seat, transfers host and closes the empty room', pass: true });
});

const executed = [];
await withServer({ start: async () => {}, close: async () => {}, snapshot: async () => ({ tick: executed.length }),
  execute: async command => { executed.push(command); return { accepted: true, tick: executed.length }; } }, async call => {
  const blue = (await call('/rooms', { method: 'POST', input: { protocol: NET_PROTOCOL } })).body;
  const red = (await call('/join', { method: 'POST', input: { protocol: NET_PROTOCOL, code: blue.room.code } })).body;
  const id = blue.room.id;
  for (const p of [blue, red]) await call(`/rooms/${id}/ready`, { method: 'POST', token: p.token, input: { ready: true } });
  assert.equal((await call(`/rooms/${id}/start`, { method: 'POST', token: red.token, input: {} })).status, 409);
  assert.equal((await call(`/rooms/${id}/start`, { method: 'POST', token: blue.token, input: {} })).status, 200);
  const order = { sequence: 1, kind: 'move', entityIds: [4], cells: [120], model: 0, owner: 2 };
  const send = (input, token = blue.token) => call(`/rooms/${id}/orders`, { method: 'POST', token, input });
  const first = await send(order), again = await send(order);
  assert.equal(first.status, 200); assert.deepEqual(again.body, first.body); assert.equal(executed.length, 1);
  assert.equal(executed[0].owner, 1); assert.equal(executed[0].playerId, blue.playerId); assert.ok(!('owner' in executed[0].order));
  assert.equal((await send({ ...order, cells: [121] })).status, 409);
  assert.equal((await send({ ...order, sequence: 3 })).status, 409);
  const duplicate = { ...order, sequence: 2, cells: [121] };
  const pair = await Promise.all([send(duplicate), send(duplicate)]);
  assert.ok(pair.every(r => r.status === 200)); assert.deepEqual(pair[0].body, pair[1].body); assert.equal(executed.length, 2);
  assert.equal((await send({ ...order, sequence: 1 }, red.token)).status, 200); assert.equal(executed[2].owner, 2);
  results.push({ case: 'adapter boundary assigns identity and serializes/deduplicates transport commands', pass: true,
    scope: 'Protocol adapter stub only; no claim of playable native multiplayer combat' });
});

const output = path.join(path.dirname(fileURLToPath(import.meta.url)), 'v5', 'multiplayer-tests.json');
fs.writeFileSync(output, JSON.stringify({ testedAt: new Date().toISOString(), checks: results, nativePvpCombatTested: false }, null, 2));
console.log(JSON.stringify({ passed: results.length, report: output }));
