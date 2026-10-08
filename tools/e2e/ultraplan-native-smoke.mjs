// Run a tiny game on both real render backends using isolated projects/processes.
// This verifies native input/game/render paths, NOT model orchestration or human approval.
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import readline from 'node:readline';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { chromium } from 'playwright-core';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const evidence = path.join(repo, 'evidence/ultraplan-native', new Date().toISOString().replace(/[:.]/g, '-'));
await fs.mkdir(evidence, { recursive: true });
const provenance = {};
for (const relative of ['target/debug/engine-scene-mcp.exe', 'target/debug/engine-host.exe', 'target/godot-runtime/bin/godot_host.dll', 'target/godot-runtime/runtime-manifest.json']) {
  const file = path.join(repo, relative), stat = await fs.stat(file);
  provenance[relative] = { sha256: createHash('sha256').update(await fs.readFile(file)).digest('hex'), bytes: stat.size, modifiedAt: stat.mtime.toISOString() };
}
await fs.writeFile(path.join(evidence, 'binary-provenance.json'), JSON.stringify(provenance, null, 2));
const results = [];
const constPin = value => ({ const: value });
const ref = (node, pin) => ({ node, pin });
const node = (id, type, inputs = {}) => ({ id, type, pos: [0, 0], inputs });
const edge = (a, pin, b) => ({ from: [a, pin], to: [b, 'exec'] });
const graph = (id, nodes, edges) => ({ version: 1, id, name: id, exposedProps: [], nodes, edges });
const playerGraph = graph('Player', [
  node('start', 'event.on_start'),
  node('right', 'var.set', { name: constPin('right'), value: constPin([1, 0, 0]) }),
  node('left', 'var.set', { name: constPin('left'), value: constPin([-1, 0, 0]) }),
  node('click', 'var.set', { name: constPin('click'), value: constPin([0, 1, 0]) }),
  node('input', 'event.on_input'),
  node('offset', 'var.get', { name: ref('input', 'action') }),
  node('move', 'transform.move_tween', { target: constPin('$self'), offset: ref('offset', 'out'), duration: constPin(0) }),
], [edge('start', 'exec', 'right'), edge('right', 'exec', 'left'), edge('left', 'exec', 'click'), edge('input', 'exec', 'move')]);
const goalGraph = graph('Goal', [
  node('touch', 'event.on_trigger_enter'),
  node('player', 'entity.has_tag', { entity: ref('touch', 'otherEntity'), tag: constPin('player') }),
  node('branch', 'flow.branch', { condition: ref('player', 'out') }),
  node('win', 'entity.add_tag', { entity: ref('touch', 'otherEntity'), tag: constPin('winner') }),
], [edge('touch', 'exec', 'branch'), edge('branch', 'then', 'win')]);
const comp = (type, props) => ({ type, enabled: true, props });
const entity = (id, name, translation, components, scale = [0.5, 0.5, 0.5]) => ({ id, name,
  transform: { translation, scale, rotation: [0, 0, 0, 1] }, components });
const scene = { name: 'Two steps and click to win', mode: '3d', gravity: [0, 0, 0], next_id: 4, entities: [
  entity(1, 'Player', [0, 0.5, 0], [comp('MeshRenderer', { mesh: 'cube' }), comp('Tag', { tag: 'player' }),
    comp('Script', { module: '', graphRef: 'Content/Graphs/player.rxgraph', props: {} })]),
  entity(2, 'Goal', [2, 1.5, 0], [comp('MeshRenderer', { mesh: 'cube' }), comp('Trigger', { kind: 'box', extents: [0.6, 0.6, 0.6] }),
    comp('Script', { module: '', graphRef: 'Content/Graphs/goal.rxgraph', props: {} })]),
  entity(3, 'Floor', [1, 0, 0], [comp('MeshRenderer', { mesh: 'cube' })], [5, 0.2, 3]),
] };

class Mcp {
  constructor(project, backendDir) {
    const env = { ...process.env, FORGE_PROJECT_ROOT: project,
      FORGE_ENGINE_HOST_BIN: path.join(repo, 'target/debug/engine-host.exe'),
      FORGE_GODOT_RUNTIME_DIR: path.join(repo, 'target/godot-runtime'),
      FORGE_HOST_EVENTS_LOG: path.join(backendDir, 'host-events.jsonl') };
    for (const key of ['FORGE_RENDER_BACKEND', 'FORGE_RENDER_METHOD', 'FORGE_RENDER_DRIVER', 'FORGE_HOST_PORT', 'FORGE_GODOT_GPU_INDEX', 'FORGE_GODOT_ARGS', 'FORGE_GAME_SCENE']) delete env[key];
    this.child = spawn(path.join(repo, 'target/debug/engine-scene-mcp.exe'), [], { cwd: project, env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    this.id = 0; this.pending = new Map(); this.logs = [];
    this.child.stderr.on('data', chunk => { this.logs.push(String(chunk)); });
    readline.createInterface({ input: this.child.stdout }).on('line', line => {
      try { const response = JSON.parse(line); const p = this.pending.get(response.id); if (p) { clearTimeout(p.timer); this.pending.delete(response.id); response.error ? p.reject(new Error(JSON.stringify(response.error))) : p.resolve(response.result); } }
      catch (error) { this.logs.push(`${error.message}: ${line.slice(0, 300)}`); }
    });
    this.child.on('error', error => { for (const p of this.pending.values()) p.reject(error); });
    this.child.on('exit', code => { for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(new Error(`MCP exited ${code}`)); } this.pending.clear(); });
  }
  request(method, params) {
    return new Promise((resolve, reject) => {
      const id = ++this.id;
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`${method} timed out`)); }, 45000);
      this.pending.set(id, { resolve, reject, timer });
      this.child.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
    });
  }
  async tool(name, args = {}) {
    const wrapped = await this.request('tools/call', { name, arguments: args });
    if (wrapped.isError) throw new Error(`${name}: ${wrapped.content?.[0]?.text}`);
    return JSON.parse(wrapped.content[0].text);
  }
  async close() {
    if (this.child.exitCode !== null || this.child.signalCode !== null) return;
    const exited = new Promise(resolve => this.child.once('exit', resolve));
    this.child.stdin.end();
    let timer;
    const ended = await Promise.race([exited.then(() => true), new Promise(resolve => { timer = setTimeout(() => resolve(false), 5000); })]);
    clearTimeout(timer);
    if (!ended && this.child.pid) {
      // Only the exact child tree created by this test. Never enumerate/kill other sessions.
      await new Promise(resolve => spawn('taskkill', ['/PID', String(this.child.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' }).once('exit', resolve));
    }
  }
}

let browser;
try {
  for (const channel of ['msedge', 'chrome']) {
    try { browser = await chromium.launch({ channel, headless: true }); break; } catch { /* next installed browser */ }
  }
  if (!browser) throw new Error('No system Edge/Chrome browser; native keyboard/pointer test unavailable');
  for (const backend of ['rurix', 'godot']) {
    const dir = path.join(evidence, backend), project = path.join(dir, 'project');
    await fs.mkdir(path.join(project, 'Content/Graphs'), { recursive: true });
    await fs.mkdir(path.join(project, 'Content/Scenes'), { recursive: true });
    await fs.writeFile(path.join(project, 'forge.toml'), `[project]\nname = "ultraplan-input-smoke"\nmode = "3d"\nentry-scene = "Content/Scenes/game.rxscene"\n[render]\nbackend = "${backend}"\n`);
    await fs.writeFile(path.join(project, 'Content/Graphs/player.rxgraph'), JSON.stringify(playerGraph, null, 2));
    await fs.writeFile(path.join(project, 'Content/Graphs/goal.rxgraph'), JSON.stringify(goalGraph, null, 2));
    const scenePath = path.join(project, 'Content/Scenes/game.rxscene');
    await fs.writeFile(scenePath, JSON.stringify(scene, null, 2));
    await fs.writeFile(path.join(project, 'matrix.json'), JSON.stringify({ scene: scenePath, enterPlay: true,
      camera: { target: [1, 0.75, 0], yaw: 30, pitch: 20, dist: 7, fovY: 55 },
      inputs: [{ action: 'right', value: 1, settle: 3 }, { action: 'right', value: 1, settle: 3 }, { action: 'click', value: 1, settle: 3 }],
      settleFrames: 3, cases: [
        { name: 'Reached goal', assert: { kind: 'transform_near', entity: 'Player', translation: [2, 1.5, 0], tolerance: 0.01 } },
        { name: 'Won through goal trigger', assert: { kind: 'component_field', entity: 'Player', type: 'Tag', field: 'props.tag', expected: 'winner' } },
      ] }, null, 2));
    const report = { backend, ok: false, project, checks: [], inputs: [], screenshots: [], startedAt: new Date().toISOString(), scope: 'Real native MCP + engine + browser-input fixture. Model orchestration and human approval are not tested.' };
    const mcp = new Mcp(project, dir);
    let page;
    try {
      await mcp.request('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'ultraplan-native-smoke', version: '1' } });
      report.host = await mcp.tool('host_ping');
      report.backendInfo = await mcp.tool('render_backend_info');
      report.capabilities = await mcp.tool('render_capabilities');
      assert.equal(report.backendInfo.renderBackend, backend);
      await mcp.tool('scene_load', { path: scenePath });
      await mcp.tool('viewport_set_camera', { target: [1, 0.75, 0], yaw: 30, pitch: 20, dist: 7, fovY: 55 });
      await mcp.tool('play_enter'); await mcp.tool('play_pause');
      await mcp.tool('host_events_drain');
      page = await browser.newPage({ viewport: { width: 640, height: 360 } });
      await page.setContent('<!doctype html><style>body{margin:0}canvas{display:block}</style><canvas width="640" height="360" tabindex="0"></canvas>');
      await page.exposeFunction('nativeInput', async (input) => {
        report.inputs.push(input);
        if (input.kind === 'keyboard') await mcp.tool('logic_inject_input', { action: input.action, value: input.value });
        else await mcp.tool('logic_inject_pointer', { x: input.x, y: input.y, action: 'click', width: 640, height: 360 });
        for (let i = 0; i < 3; i++) await mcp.tool('play_step');
      });
      await page.evaluate(() => {
        window.inputDone = Promise.resolve();
        addEventListener('keydown', event => {
          const action = event.key === 'ArrowRight' ? 'right' : event.key === 'ArrowLeft' ? 'left' : null;
          if (action) window.inputDone = window.inputDone.then(() => window.nativeInput({ kind: 'keyboard', key: event.key, action, value: action === 'right' ? 1 : -1 }));
        });
        document.querySelector('canvas').addEventListener('click', event => {
          window.inputDone = window.inputDone.then(() => window.nativeInput({ kind: 'pointer', x: event.clientX / 640, y: event.clientY / 360 }));
        });
      });
      const screenshot = async name => {
        const frame = await mcp.tool('viewport_frame', { width: 640, height: 360 });
        const bytes = Buffer.from(frame.pixelsB64, 'base64');
        assert.equal(bytes.length, frame.width * frame.height * 4);
        const unique = new Set(); for (let i = 0; i < bytes.length; i += 4) unique.add(bytes.readUInt32LE(i));
        assert.ok(unique.size > 1, 'Actual rendered frame must contain visual content');
        await page.evaluate(({ pixels, width, height }) => {
          const canvas = document.querySelector('canvas'); canvas.width = width; canvas.height = height;
          const bytes = Uint8ClampedArray.from(atob(pixels), c => c.charCodeAt(0));
          canvas.getContext('2d').putImageData(new ImageData(bytes, width, height), 0, 0);
        }, { pixels: frame.pixelsB64, width: frame.width, height: frame.height });
        const filename = path.join(dir, name + '.png');
        await page.screenshot({ path: filename });
        report.screenshots.push({ path: filename, sha256: createHash('sha256').update(await fs.readFile(filename)).digest('hex'), draws: frame.draws, nonZeroPixels: frame.nonZeroPixels, uniqueColors: unique.size, deviceName: frame.deviceName });
      };
      await screenshot('before');
      await page.keyboard.press('ArrowRight'); await page.evaluate(() => window.inputDone);
      await page.keyboard.press('ArrowRight'); await page.evaluate(() => window.inputDone);
      const moved = await mcp.tool('transform_get', { id: 1 });
      assert.deepEqual(moved.translation, [2, 0.5, 0]);
      const tagBefore = await mcp.tool('component_get', { id: 1, type: 'Tag' });
      assert.equal(tagBefore.props.tag, 'player', 'Cannot win before the click');
      report.checks.push({ kind: 'transform_near', input: 'ArrowRight x2', actual: moved.translation, expected: [2, 0.5, 0], pass: true });
      await page.mouse.click(320, 180); await page.evaluate(() => window.inputDone);
      const final = await mcp.tool('transform_get', { id: 1 });
      const tag = await mcp.tool('component_get', { id: 1, type: 'Tag' });
      assert.deepEqual(final.translation, [2, 1.5, 0]);
      assert.equal(tag.props.tag, 'winner');
      report.checks.push({ kind: 'component_field', input: 'mouse click', actual: tag.props.tag, expected: 'winner', pass: true });
      await screenshot('won');
      report.events = await mcp.tool('host_events_drain');
      const errors = report.events.filter(event => /unsupported|call_error|host.crashed|anim.warn/.test(event.event ?? ''));
      assert.deepEqual(errors, []);
      await mcp.tool('play_exit');
      report.endState = await mcp.tool('play_state');
      report.ok = true;
    } catch (error) { report.error = error.stack; }
    finally {
      if (page) await page.close();
      await mcp.close();
      await fs.writeFile(path.join(dir, 'mcp-stderr.log'), mcp.logs.join(''));
      report.finishedAt = new Date().toISOString();
      await fs.writeFile(path.join(dir, 'report.json'), JSON.stringify(report, null, 2));
      results.push(report);
      console.log(JSON.stringify({ backend, ok: report.ok, checks: report.checks, error: report.error, evidence: dir }));
    }
  }
} finally {
  if (browser) await browser.close();
  await fs.writeFile(path.join(evidence, 'summary.json'), JSON.stringify({ ok: results.length === 2 && results.every(row => row.ok), results }, null, 2));
}
process.exitCode = results.length === 2 && results.every(row => row.ok) ? 0 : 1;
