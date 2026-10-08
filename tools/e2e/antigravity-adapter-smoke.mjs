import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import net from 'node:net';
import { mkdir, readdir } from 'node:fs/promises';
import path from 'node:path';

const listener = net.createServer();
await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve));
const port = listener.address().port;
await new Promise(resolve => listener.close(resolve));
const key = randomBytes(32).toString('hex');
const management = randomBytes(32).toString('hex');
const home = path.resolve('data/official-channels-qa/adapter-smoke');
await mkdir(home, { recursive: true });
const child = spawn(path.resolve('data/antigravity-runtime/forge-antigravity-bridge.exe'), [], {
  env: { ...process.env, FORGE_ANTIGRAVITY_BRIDGE_PORT: String(port), FORGE_ANTIGRAVITY_BRIDGE_KEY: key, FORGE_ANTIGRAVITY_MANAGEMENT_KEY: management, FORGE_ANTIGRAVITY_BRIDGE_HOME: home },
  windowsHide: true, stdio: ['pipe', 'ignore', 'ignore'],
});
const origin = `http://127.0.0.1:${port}`;
const request = async (url, method = 'GET', body) => {
  const response = await fetch(origin + url, { method, headers: { Authorization: `Bearer ${management}`, 'Content-Type': 'application/json' }, body: body && JSON.stringify(body) });
  return { status: response.status, data: await response.json() };
};
try {
  let account;
  for (let n = 0; n < 100; n++) { try { account = await request('/forge/account'); if (account.status === 200) break; } catch {} await new Promise(resolve => setTimeout(resolve, 100)); }
  console.log(JSON.stringify({ accountStatus: account?.status, configured: account?.data.configured }));
  await request('/forge/begin-login','POST',{});
  const auth = await request('/v0/management/antigravity-auth-url');
  console.log(JSON.stringify({ authorizationStatus: auth.status, error: auth.data.error, authHost: auth.data.url && new URL(auth.data.url).hostname, hasState: !!auth.data.state }));
  if (auth.status !== 200) throw new Error('Authorization initiation failed');
  const state = auth.data.state;
  const pending = await request(`/v0/management/get-auth-status?state=${state}`);
  const cancelled = await request(`/v0/management/oauth-session?state=${state}`, 'DELETE');
  await request('/forge/cancel-login','POST',{});
  const callback = await request('/v0/management/oauth-callback', 'POST', { provider: 'antigravity', state, code: 'cancelled-test-code' });
  const credentialFiles = await readdir(path.join(home, 'credentials')).catch(() => []);
  console.log(JSON.stringify({ pending: pending.data.status, cancelStatus: cancelled.status, cancelledCallbackStatus: callback.status, credentials: credentialFiles.length }));
  if (pending.data.status !== 'wait' || cancelled.status !== 200 || callback.status < 400 || credentialFiles.length) throw new Error('Cancellation isolation failed');
} finally { child.stdin.end(); await new Promise(resolve => child.once('exit', resolve)); }
