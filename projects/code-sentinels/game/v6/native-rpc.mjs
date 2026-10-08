import net from 'node:net';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';

/** Private, length-framed localhost RPC. This object is never exposed on a LAN listener. */
export async function launchNative({ root, executable = path.join(root, 'bin', 'engine-host.exe'), logs = path.join(root, 'Logs', 'v6'), gpuParticles = false } = {}) {
  const rulesHash = createHash('sha256').update(fs.readFileSync(executable)).digest('hex');
  fs.mkdirSync(logs, { recursive: true });
  const stderr = fs.openSync(path.join(logs, 'engine.log'), 'a');
  const child = spawn(executable, ['--port', '0'], { cwd: root, windowsHide: true, stdio: ['ignore', 'pipe', stderr], env: {
    ...process.env, FORGE_PROJECT_ROOT: root, FORGE_GAME_SAVE_DIR: path.join(root, '.forge', 'save'),
    FORGE_GPU_PARTICLES: gpuParticles ? 'on' : 'off', RURIX_PREFER_DISCRETE_GPU: '1', FORGE_RUSTC: path.join(root, 'compiler-not-required.exe'),
  } });
  fs.closeSync(stderr);
  let socket, port, stopped = false, id = 1, buffer = Buffer.alloc(0), lines = '';
  const pending = new Map();
  const failPending = error => { for (const p of pending.values()) { clearTimeout(p.timer); p.reject(error); } pending.clear(); };
  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Native engine startup timed out')), 60_000);
    child.once('error', error => { clearTimeout(timer); reject(error); });
    child.once('exit', code => { clearTimeout(timer); reject(new Error(`Native engine exited (${code})`)); });
    child.stdout.on('data', data => {
      lines += data.toString(); let end;
      while ((end = lines.indexOf('\n')) !== -1) {
        const line = lines.slice(0, end).trim(); lines = lines.slice(end + 1);
        const match = /FORGE_HOST_LISTENING port=(\d+)/.exec(line);
        if (!match || socket) continue;
        port = Number(match[1]); socket = net.createConnection({ host: '127.0.0.1', port });
        socket.once('connect', () => { clearTimeout(timer); resolve(); });
        socket.on('error', error => { clearTimeout(timer); reject(error); failPending(error); });
        socket.on('close', () => failPending(new Error('Native RPC connection closed')));
        socket.on('data', chunk => {
          buffer = Buffer.concat([buffer, chunk]);
          while (buffer.length >= 4) {
            const length = buffer.readUInt32LE();
            if (length > 64 * 1024 * 1024) { failPending(new Error('Native RPC message too large')); socket.destroy(); return; }
            if (buffer.length < length + 4) break;
            let result; try { result = JSON.parse(buffer.subarray(4, length + 4).toString()); } catch { failPending(new Error('Invalid native RPC JSON')); socket.destroy(); return; }
            buffer = buffer.subarray(length + 4);
            const p = pending.get(result.id); if (!p) continue;
            pending.delete(result.id); clearTimeout(p.timer);
            if (result.error) p.reject(new Error(result.error.message || JSON.stringify(result.error))); else p.resolve(result.result);
          }
        });
      }
    });
  });
  child.on('exit', code => { if (!stopped) failPending(new Error(`Native engine exited (${code})`)); });
  try { await ready; } catch (error) { child.kill(); throw error; }
  return {
    pid: child.pid, port, executable, rulesHash,
    rpc(method, params = {}, timeoutMs = 30_000) {
      if (stopped || socket.destroyed) return Promise.reject(new Error('Native engine unavailable'));
      return new Promise((resolve, reject) => {
        const requestId = id++;
        const timer = setTimeout(() => { pending.delete(requestId); reject(new Error(`Native RPC timeout: ${method}`)); }, timeoutMs);
        pending.set(requestId, { resolve, reject, timer });
        const data = Buffer.from(JSON.stringify({ jsonrpc: '2.0', id: requestId, method, params }));
        const head = Buffer.alloc(4); head.writeUInt32LE(data.length); socket.write(Buffer.concat([head, data]));
      });
    },
    async close() { stopped = true; failPending(new Error('Game closed')); socket?.destroy(); if (child.exitCode === null) { child.kill(); await Promise.race([new Promise(resolve => child.once('exit', resolve)), new Promise(resolve => setTimeout(resolve, 3000))]); } },
  };
}
