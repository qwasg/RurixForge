/** UltraPlan browser probe. Fixed runner; generated games supply data, never host code. */
import fs from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const clip = (value, max = 2000) => String(value).slice(0, max);
const plain = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const safePath = (value) => typeof value === 'string' && value.length <= 200 &&
  value.split('.').every((part) => part && !['__proto__', 'prototype', 'constructor'].includes(part));

export function validateRequest(raw) {
  if (!plain(raw)) throw new Error('Probe request must be an object');
  const url = new URL(raw.url);
  if (url.protocol !== 'http:' || !['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) ||
      !/^\/u\/[A-Za-z0-9_-]+\/$/.test(url.pathname) || url.username || url.password || url.search || url.hash) {
    throw new Error('Probe URL must be a loopback UltraPlan demo root');
  }
  const script = raw.script ?? [];
  const inputs = raw.inputs ?? [];
  const assertions = raw.assertions ?? [];
  if (!Array.isArray(script) || script.length > 32 || !Array.isArray(inputs) || inputs.length > 64 ||
      !Array.isArray(assertions) || assertions.length > 64) throw new Error('Probe sequence exceeds limits');
  for (const step of script) {
    if (!plain(step) || !['reset', 'tick', 'getState'].includes(step.call) ||
        (step.args !== undefined && !Array.isArray(step.args))) throw new Error('Unsupported demo hook');
    if (step.call === 'tick' && (!Number.isInteger(step.args?.[0]) || step.args[0] < 0 || step.args[0] > 600)) {
      throw new Error('tick requires 0..600 frames');
    }
    if (JSON.stringify(step.args ?? []).length > 2000) throw new Error('Hook arguments too large');
  }
  let waitMs = 0;
  for (const input of inputs) {
    if (!plain(input) || !['keyDown', 'keyUp', 'keyPress', 'click', 'move', 'wait'].includes(input.kind)) {
      throw new Error('Unsupported browser input');
    }
    if (input.kind.startsWith('key') && (typeof input.key !== 'string' || input.key.length < 1 || input.key.length > 40)) {
      throw new Error('Keyboard input requires a key');
    }
    if (['click', 'move'].includes(input.kind) && (!Number.isFinite(input.x) || !Number.isFinite(input.y) ||
        input.x < 0 || input.y < 0 || input.x > 1920 || input.y > 1080)) throw new Error('Invalid input coordinates');
    if (input.kind === 'wait') {
      if (!Number.isInteger(input.ms) || input.ms < 0 || input.ms > 2000) throw new Error('Wait must be 0..2000 ms');
      waitMs += input.ms;
    }
  }
  if (waitMs > 15000) throw new Error('Total input waits exceed 15 seconds');
  for (const assertion of assertions) {
    if (!plain(assertion) || !safePath(assertion.path) ||
        !['eq', 'neq', 'gt', 'ge', 'lt', 'le', 'includes', 'truthy'].includes(assertion.op ?? 'eq')) {
      throw new Error('Invalid state assertion');
    }
  }
  return { ...raw, url: url.href, script, inputs, assertions };
}

export function evaluateAssertions(state, assertions) {
  return assertions.map((assertion) => {
    let actual = state;
    for (const part of assertion.path.split('.')) {
      actual = actual !== null && typeof actual === 'object' && Object.hasOwn(actual, part) ? actual[part] : undefined;
    }
    const expected = assertion.expected;
    const op = assertion.op ?? 'eq';
    let pass = false;
    if (actual !== undefined) {
      if (op === 'eq') pass = JSON.stringify(actual) === JSON.stringify(expected);
      if (op === 'neq') pass = JSON.stringify(actual) !== JSON.stringify(expected);
      if (op === 'truthy') pass = Boolean(actual);
      if (op === 'includes') pass = (Array.isArray(actual) || typeof actual === 'string') && actual.includes(expected);
      if (typeof actual === 'number' && typeof expected === 'number') {
        if (op === 'gt') pass = actual > expected;
        if (op === 'ge') pass = actual >= expected;
        if (op === 'lt') pass = actual < expected;
        if (op === 'le') pass = actual <= expected;
      }
    }
    return { ...assertion, actual: actual ?? null, pass };
  });
}

export async function runProbe(raw) {
  const started = Date.now();
  const report = { ok: false, unavailable: false, errors: [], console: [], assertions: [], inputVerified: false,
    screenshot: null, browser: null, stateBefore: null, stateAfter: null, inputs: [], durationMs: 0 };
  let browser;
  let page;
  let watchdog;
  try {
    const request = validateRequest(raw);
    report.url = request.url;
    let chromium;
    try {
      ({ chromium } = await import('playwright-core'));
    } catch (error) {
      report.unavailable = true;
      throw new Error(`PLAYWRIGHT_UNAVAILABLE: ${clip(error.message)}`);
    }
    const launchErrors = [];
    for (const channel of ['msedge', 'chrome']) {
      try {
        browser = await chromium.launch({ channel, headless: true, timeout: 12000 });
        report.browser = channel;
        break;
      } catch (error) { launchErrors.push(`${channel}: ${clip(error.message, 600)}`); }
    }
    if (!browser) {
      report.unavailable = true;
      throw new Error(`BROWSER_UNAVAILABLE: ${launchErrors.join('; ')}`);
    }
    // Includes stalled renderer/evaluate calls: closing the browser interrupts the operation.
    watchdog = setTimeout(() => {
      report.errors.push('PROBE_TIMEOUT: browser probe exceeded 40 seconds');
      void browser.close();
    }, 40000);
    const context = await browser.newContext({ viewport: { width: 960, height: 540 }, serviceWorkers: 'block' });
    const expected = new URL(request.url);
    await context.route('**/*', async (route) => {
      const target = new URL(route.request().url());
      if (target.origin === expected.origin && target.pathname.startsWith(expected.pathname)) await route.continue();
      else {
        if (report.errors.length < 100) report.errors.push(`BLOCKED_REQUEST: ${clip(target.origin + target.pathname, 200)}`);
        await route.abort();
      }
    });
    page = await context.newPage();
    page.setDefaultTimeout(5000);
    page.on('pageerror', (error) => report.errors.length < 100 && report.errors.push(`PAGE_ERROR: ${clip(error.message)}`));
    page.on('console', (message) => {
      if (report.console.length < 100) report.console.push({ type: message.type(), text: clip(message.text()) });
      if (message.type() === 'error' && report.errors.length < 100) report.errors.push(`CONSOLE_ERROR: ${clip(message.text())}`);
    });
    page.on('dialog', (dialog) => {
      if (report.errors.length < 100) report.errors.push(`DIALOG: ${clip(dialog.message(), 200)}`);
      void dialog.dismiss();
    });
    page.on('response', (response) => {
      if (response.status() >= 400 && report.errors.length < 100) {
        report.errors.push(`HTTP_${response.status()}: ${clip(response.url(), 250)}`);
      }
    });
    const response = await page.goto(request.url, { waitUntil: 'load', timeout: 10000 });
    if (!response?.ok()) throw new Error(`Demo entry failed: HTTP ${response?.status() ?? 'none'}`);
    await page.waitForFunction(() => typeof window.__demo?.getState === 'function', undefined, { timeout: 5000 });
    for (const step of request.script) {
      await page.evaluate(async ({ call, args }) => {
        const fn = window.__demo?.[call];
        if (typeof fn !== 'function') throw new Error(`Missing demo hook: ${call}`);
        await fn.apply(window.__demo, args ?? []);
      }, step);
    }
    const getState = () => page.evaluate(async () => {
      const state = await window.__demo.getState();
      const serialized = JSON.stringify(state);
      if (serialized === undefined || serialized.length > 65536) throw new Error('getState must return JSON under 64 KiB');
      return JSON.parse(serialized);
    });
    report.stateBefore = await getState();
    let realInputs = 0;
    for (const input of request.inputs) {
      if (input.kind === 'keyDown') await page.keyboard.down(input.key);
      else if (input.kind === 'keyUp') await page.keyboard.up(input.key);
      else if (input.kind === 'keyPress') await page.keyboard.press(input.key);
      else if (input.kind === 'click') await page.mouse.click(input.x, input.y);
      else if (input.kind === 'move') await page.mouse.move(input.x, input.y);
      else await page.waitForTimeout(input.ms);
      report.inputs.push(input);
      if (input.kind !== 'wait' && input.kind !== 'move') realInputs++;
    }
    await page.waitForTimeout(100);
    report.stateAfter = await getState();
    report.inputVerified = realInputs > 0 && JSON.stringify(report.stateBefore) !== JSON.stringify(report.stateAfter);
    report.assertions = evaluateAssertions(report.stateAfter, request.assertions);
    if (!report.inputVerified) report.errors.push('INPUT_UNVERIFIED: provide real keyboard/mouse inputs that change game state');
    if (report.assertions.length === 0) report.errors.push('ASSERTIONS_MISSING: provide at least one gameplay state assertion');
    for (const assertion of report.assertions.filter((a) => !a.pass)) report.errors.push(`ASSERTION_FAILED: ${assertion.path}`);
    await page.screenshot({ path: request.screenshotPath, fullPage: false, timeout: 5000 });
    report.screenshot = request.screenshotPath;
    report.ok = report.errors.length === 0 && report.inputVerified && report.assertions.length > 0;
  } catch (error) {
    report.errors.push(clip(error.message));
    if (page && raw.screenshotPath && !report.screenshot) {
      try { await page.screenshot({ path: raw.screenshotPath, timeout: 2000 }); report.screenshot = raw.screenshotPath; } catch { /* original error retained */ }
    }
  } finally {
    clearTimeout(watchdog);
    if (browser) await browser.close().catch(() => {});
    report.durationMs = Date.now() - started;
  }
  return report;
}

async function main() {
  let text = '';
  process.stdin.setEncoding('utf8');
  for await (const chunk of process.stdin) {
    text += chunk;
    if (text.length > 131072) throw new Error('Probe request exceeds 128 KiB');
  }
  const request = JSON.parse(text);
  const report = await runProbe(request);
  if (request.reportPath) await fs.writeFile(request.reportPath, JSON.stringify(report, null, 2));
  process.stdout.write(JSON.stringify(report));
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    process.stdout.write(JSON.stringify({ ok: false, unavailable: false, inputVerified: false, errors: [clip(error.message)] }));
    process.exitCode = 1;
  });
}
