// D-044 子框架导航守卫(main.cjs will-frame-navigate 所用纯函数),不启动 Electron。
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { isAllowedSubframeUrl } = require('../src/frameGuard.cjs');

const PROD = 'http://127.0.0.1:3080';
const DEV = 'http://localhost:5173';

test('loopback demo host /u/ paths on another origin are allowed (prod and dev embedders)', () => {
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/0123abcd/index.html?v=2', PROD), true);
  assert.equal(isAllowedSubframeUrl('http://[::1]:51234/u/0123abcd/', PROD), true);
  assert.equal(isAllowedSubframeUrl('http://127.0.0.1:51234/u/0123abcd/index.html', DEV), true);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/0123abcd/', DEV + '/'), true);
});

test('the app origin itself and its port under another loopback name are refused', () => {
  assert.equal(isAllowedSubframeUrl('http://127.0.0.1:3080/u/0123abcd/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:3080/u/0123abcd/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://[::1]:3080/u/0123abcd/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:5173/u/0123abcd/', DEV), false);
  assert.equal(isAllowedSubframeUrl('http://127.0.0.1:5173/u/0123abcd/', DEV), false);
  assert.equal(isAllowedSubframeUrl('http://127.0.0.1/u/0123abcd/', 'http://localhost'), false);
});

test('paths outside /u/ are refused, including dot-segment escapes', () => {
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/api/forge/sessions', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/../api/forge/sessions', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/%2e%2e/api', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/U/0123abcd/', PROD), false);
});

test('non-loopback hosts, other schemes and credentials are refused', () => {
  assert.equal(isAllowedSubframeUrl('https://example.com/u/x/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://example.com/u/x/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://127.0.0.2:51234/u/x/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://0.0.0.0:51234/u/x/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://192.168.1.10:51234/u/x/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://localhost.example.com:51234/u/x/', PROD), false);
  assert.equal(isAllowedSubframeUrl('https://localhost:51234/u/x/', PROD), false);
  assert.equal(isAllowedSubframeUrl('ws://localhost:51234/u/x/', PROD), false);
  assert.equal(isAllowedSubframeUrl('file:///C:/Windows/u/x', PROD), false);
  assert.equal(isAllowedSubframeUrl('about:blank', PROD), false);
  assert.equal(isAllowedSubframeUrl('data:text/html,<p>x</p>', PROD), false);
  assert.equal(isAllowedSubframeUrl('javascript:alert(1)', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://user:pw@localhost:51234/u/x/', PROD), false);
  assert.equal(isAllowedSubframeUrl('http://user@localhost:51234/u/x/', PROD), false);
});

test('unparseable input never allows navigation', () => {
  assert.equal(isAllowedSubframeUrl('', PROD), false);
  assert.equal(isAllowedSubframeUrl('not a url', PROD), false);
  assert.equal(isAllowedSubframeUrl(undefined, PROD), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/x/', ''), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/x/', undefined), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/x/', []), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/x/', [DEV, '']), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/x/', [DEV, undefined]), false);
});

test('every app-serving origin is refused: desktop dev passes [Vite, host]', () => {
  const both = [DEV, PROD];
  // host 在开发态仍托管构建壳与代理 API:两个回环名都要拒。
  assert.equal(isAllowedSubframeUrl('http://127.0.0.1:3080/u/x/', both), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:3080/u/x/', both), false);
  assert.equal(isAllowedSubframeUrl('http://[::1]:3080/u/x/', both), false);
  assert.equal(isAllowedSubframeUrl('http://localhost:5173/u/x/', both), false);
  assert.equal(isAllowedSubframeUrl('http://127.0.0.1:5173/u/x/', both), false);
  assert.equal(isAllowedSubframeUrl('http://127.0.0.1:51234/u/0123abcd/', both), true);
  assert.equal(isAllowedSubframeUrl('http://localhost:51234/u/0123abcd/', [PROD, PROD]), true);
});

// 从 main.cjs 截出真实的子框架守卫段(共用判定 + will-frame-navigate / will-redirect 两处注册),
// 用假 webContents 驱动(同 presenter 测试的截段法)。
function subframeListeners(env) {
  const source = fs.readFileSync(path.join(__dirname, '../src/main.cjs'), 'utf8');
  const start = source.indexOf('const guardSubframeNavigation');
  const lastLine = "mainWindow.webContents.on('will-redirect', guardSubframeNavigation);";
  const end = source.indexOf(lastLine, start);
  assert.ok(start > 0 && end > start, 'sub-frame guard and its will-redirect registration must exist');
  const listeners = {};
  const opened = [];
  const context = vm.createContext({
    process: { env }, HOST_ORIGIN: 'http://127.0.0.1:3080', isAllowedSubframeUrl,
    shell: { openExternal: (url) => opened.push(url) },
    mainWindow: { webContents: { on: (name, fn) => { listeners[name] = fn; } } },
  });
  vm.runInContext(source.slice(start, end + lastLine.length), context);
  assert.deepEqual(Object.keys(listeners).sort(), ['will-frame-navigate', 'will-redirect']);
  const drive = (name) => (url, isMainFrame) => {
    let prevented = false;
    // will-redirect 另带一串已弃用的位置参数,守卫只读 details。
    listeners[name]({ url, isMainFrame, preventDefault() { prevented = true; } }, url, false, isMainFrame);
    return prevented;
  };
  return { navigate: drive('will-frame-navigate'), redirect: drive('will-redirect'), opened };
}

test('main.cjs handler: main frame is left to will-navigate, blocked sub-frames never open externally', () => {
  const { navigate, opened } = subframeListeners({});
  assert.equal(navigate('https://example.com/', true), false);
  assert.equal(navigate('http://127.0.0.1:3080/', true), false);
  assert.equal(navigate('http://localhost:51234/u/0123abcd/index.html?v=1', false), false);
  assert.equal(navigate('https://example.com/', false), true);
  assert.equal(navigate('http://127.0.0.1:3080/', false), true);
  assert.equal(navigate('http://localhost:3080/u/0123abcd/', false), true);
  assert.deepEqual(opened, []);
});

test('main.cjs handler: FORGE_DEV_URL is the app origin in desktop dev, and the host stays refused', () => {
  const { navigate, redirect } = subframeListeners({ FORGE_DEV_URL: 'http://localhost:5173' });
  assert.equal(navigate('http://127.0.0.1:51234/u/0123abcd/', false), false);
  assert.equal(navigate('http://localhost:5173/u/0123abcd/', false), true);
  assert.equal(navigate('http://127.0.0.1:5173/', false), true);
  assert.equal(navigate('http://127.0.0.1:3080/u/x/', false), true);
  assert.equal(navigate('http://localhost:3080/u/x/', false), true);
  assert.equal(redirect('http://127.0.0.1:3080/u/x/', false), true);
  assert.equal(redirect('http://localhost:3080/u/x/', false), true);
});

test('main.cjs handler: server-side redirects of a sub-frame go through the same guard', () => {
  const { redirect, opened } = subframeListeners({});
  // 允许的 /u/ 起点被 30x 甩到外站、应用源或 /u/ 之外:整次导航取消,不转系统浏览器。
  assert.equal(redirect('https://example.com/', false), true);
  assert.equal(redirect('http://example.com/u/x/', false), true);
  assert.equal(redirect('http://127.0.0.1:3080/', false), true);
  assert.equal(redirect('http://localhost:3080/u/0123abcd/', false), true);
  assert.equal(redirect('http://localhost:51234/api/forge/sessions', false), true);
  // demo host 自己的 /u/ 内跳转(如补尾斜杠)照常。
  assert.equal(redirect('http://localhost:51234/u/0123abcd/', false), false);
  // 主框架重定向不归这里管。
  assert.equal(redirect('https://example.com/', true), false);
  assert.deepEqual(opened, []);
});
