'use strict';

/*
 * 安装界面逻辑。与 Rust 外壳(src/gui.rs)之间只传 JSON:
 *   发出  window.ipc.postMessage({type: ready | validate | pick | install | launch | open | openFolder | drag | minimize | close, ...})
 *   收到  window.__setup.receive({type: init | validation | picked | progress | done | failed | busy, ...})
 */
(function () {
  const $ = (sel, root = document) => root.querySelector(sel);
  const $$ = (sel, root = document) => Array.from(root.querySelectorAll(sel));
  const reduced = matchMedia('(prefers-reduced-motion: reduce)');

  function post(message) {
    try {
      window.ipc.postMessage(JSON.stringify(message));
    } catch (_) {
      // 在普通浏览器里预览时没有 window.ipc
    }
  }

  const state = {
    page: 'welcome',
    info: null,
    opts: { installDir: '', dataDir: '', desktopShortcut: true, startMenuShortcut: true, launchAfter: true },
    check: null,
    seq: 0,
    normalizeInstall: false,
    installing: false,
    target: 0,
    shown: 0,
    flare: 0,
    phase: 'prepare',
    installedDir: '',
    nav: 0,
    autoLaunch: null,
  };

  const embers = window.Embers ? new window.Embers($('#embers')) : null;

  // ───────────── 工具 ─────────────

  function formatBytes(n) {
    if (n == null || !isFinite(n)) return '—';
    const gb = n / 1024 ** 3;
    if (gb >= 1) return `${gb >= 100 ? gb.toFixed(0) : gb.toFixed(1)} GB`;
    const mb = n / 1024 ** 2;
    return `${mb >= 100 ? mb.toFixed(0) : Math.max(mb, 0.1).toFixed(1)} MB`;
  }

  // 路径在反斜杠后断行,长路径折成两行时更好读。
  function setPathText(el, value) {
    el.textContent = '';
    const parts = String(value).split(/(?<=\\)/);
    parts.forEach((part, i) => {
      el.append(part);
      if (i < parts.length - 1) el.append(document.createElement('wbr'));
    });
    el.title = value;
  }

  function bind(name, value) {
    $$(`[data-bind="${name}"]`).forEach((el) => {
      if (el.classList.contains('path') || el.classList.contains('mono')) setPathText(el, value);
      else el.textContent = value;
    });
  }

  function compareVersions(a, b) {
    const pa = String(a || '').split(/[.+-]/).map((x) => parseInt(x, 10) || 0);
    const pb = String(b || '').split(/[.+-]/).map((x) => parseInt(x, 10) || 0);
    for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
      const d = (pa[i] || 0) - (pb[i] || 0);
      if (d) return Math.sign(d);
    }
    return 0;
  }

  const samePath = (a, b) => a.trim().replace(/[\\/]+$/, '').toLowerCase() === b.trim().replace(/[\\/]+$/, '').toLowerCase();

  let toastTimer = 0;
  function toast(text) {
    const el = $('#toast');
    el.textContent = text;
    el.hidden = false;
    el.style.animation = 'none';
    void el.offsetWidth;
    el.style.animation = '';
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { el.hidden = true; }, 3200);
  }

  // ───────────── 页面切换 ─────────────

  const ORDER = ['welcome', 'options', 'installing', 'done', 'failed'];
  const STEP_OF = { welcome: 'welcome', options: 'options', installing: 'installing', done: 'done', failed: 'installing' };

  function updateStepper() {
    const current = STEP_OF[state.page];
    const steps = $$('.stepper li[data-step]');
    const index = steps.findIndex((li) => li.dataset.step === current);
    steps.forEach((li, i) => {
      li.classList.toggle('active', i === index && state.page !== 'failed');
      li.classList.toggle('failed', i === index && state.page === 'failed');
      li.classList.toggle('done', i < index);
      if (i === index) li.setAttribute('aria-current', 'step');
      else li.removeAttribute('aria-current');
    });
    const active = steps[index];
    const thumb = $('.stepper-thumb');
    if (active && thumb) {
      thumb.style.width = `${active.offsetWidth}px`;
      thumb.style.transform = `translateX(${active.offsetLeft - 3}px)`;
    }
  }

  // 键盘操作时把焦点交给新页面的主按钮;鼠标操作时只把焦点移到标题(读屏软件会朗读,不显示焦点框)。
  let keyboardMode = false;
  document.addEventListener('keydown', (e) => { if (e.key === 'Tab' || e.key === 'Enter' || e.key === ' ') keyboardMode = true; }, true);
  document.addEventListener('pointerdown', () => { keyboardMode = false; }, true);

  function focusFirst(page) {
    const primary = page.querySelector('.btn-primary:not(:disabled)');
    const target = keyboardMode && primary ? primary : page.querySelector('h1');
    if (target) target.focus({ preventScroll: true });
  }

  function go(page) {
    if (page === state.page) return;
    const from = $(`[data-page="${state.page}"]`);
    const to = $(`[data-page="${page}"]`);
    const forward = ORDER.indexOf(page) > ORDER.indexOf(state.page);
    const token = ++state.nav;
    state.page = page;
    document.body.classList.remove('intro');
    updateStepper();
    if (embers) embers.setMode(page === 'installing' ? 'forge' : 'ambient');
    const dx = forward ? '44px' : '-44px';
    from.style.setProperty('--dx', dx);
    to.style.setProperty('--dx', dx);
    const show = () => {
      if (token !== state.nav) return;
      $$('.page').forEach((p) => {
        if (p !== to) {
          p.hidden = true;
          p.classList.remove('is-leaving', 'is-entering');
        }
      });
      to.hidden = false;
      to.classList.remove('is-entering');
      void to.offsetWidth;
      to.classList.add('is-entering');
      focusFirst(to);
      if (page === 'installing') placeForgeTarget();
    };
    if (reduced.matches) {
      show();
      return;
    }
    from.classList.add('is-leaving');
    setTimeout(show, 190);
  }

  // ───────────── 初始化 ─────────────

  function buildHeroMark() {
    const svg = $('#hero-mark');
    if (!svg || svg.childElementCount) return;
    const faces = $('#mk-faces').cloneNode(true);
    faces.removeAttribute('id');
    // 每个折面从各自方向飞入、旋转着落位。
    faces.querySelectorAll('.mk-f').forEach((poly) => {
      const pts = poly.getAttribute('points').trim().split(/\s+/).map((p) => p.split(',').map(Number));
      const cx = pts.reduce((s, p) => s + p[0], 0) / pts.length;
      const cy = pts.reduce((s, p) => s + p[1], 0) / pts.length;
      const dx = cx - 626;
      const dy = cy - 628;
      const len = Math.hypot(dx, dy) || 1;
      poly.style.setProperty('--fx', `${((dx / len) * 210).toFixed(1)}px`);
      poly.style.setProperty('--fy', `${((dy / len) * 210).toFixed(1)}px`);
      poly.style.setProperty('--fr', `${dx >= 0 ? 16 : -16}deg`);
    });
    svg.append(faces);
    const ns = 'http://www.w3.org/2000/svg';
    const group = document.createElementNS(ns, 'g');
    group.setAttribute('clip-path', 'url(#mk-clip)');
    const sheen = document.createElementNS(ns, 'rect');
    sheen.setAttribute('class', 'mk-sheen');
    sheen.setAttribute('x', '-640');
    sheen.setAttribute('y', '-200');
    sheen.setAttribute('width', '380');
    sheen.setAttribute('height', '1600');
    sheen.setAttribute('fill', 'url(#mk-sheen-g)');
    group.append(sheen);
    const outline = document.createElementNS(ns, 'path');
    outline.setAttribute('class', 'mk-outline');
    outline.setAttribute('pathLength', '1');
    outline.setAttribute('d', $('#mk-sil').getAttribute('d'));
    svg.append(group, outline);
  }

  function applyOptions() {
    const o = state.opts;
    $('#install-dir').value = o.installDir;
    $('#data-dir').value = o.dataDir;
    $$('.switch').forEach((sw) => sw.setAttribute('aria-checked', String(Boolean(o[sw.dataset.opt]))));
    const shortcuts = [o.desktopShortcut && '桌面', o.startMenuShortcut && '开始菜单'].filter(Boolean);
    bind('shortcutSummary', shortcuts.length ? shortcuts.join('、') : '不创建');
    bind('dataDir', o.dataDir);
    const info = state.info;
    const locked = info && info.defaults.dataDirLocked;
    $('#data-dir').disabled = Boolean(locked);
    $('#btn-pick-data').disabled = Boolean(locked);
    const custom = info && !locked && o.dataDir.trim().toLowerCase() !== info.defaults.dataDirDefault.toLowerCase();
    $('#btn-data-reset').hidden = !custom;
  }

  function onInit(m) {
    state.info = m;
    const d = m.defaults;
    state.opts = {
      installDir: d.installDir,
      dataDir: d.dataDir,
      desktopShortcut: d.desktopShortcut,
      startMenuShortcut: d.startMenuShortcut,
      launchAfter: d.launchAfter,
    };
    bind('product', m.product);
    bind('version', m.version);
    if (m.description) bind('description', m.description);
    bind('requiredSize', formatBytes(m.requiredBytes));
    document.title = `${m.product} 安装程序`;
    $('#license-text').textContent = m.license || 'Apache License 2.0';
    $('#btn-homepage').hidden = !m.homepage;
    const existing = m.existing;
    $('#upgrade-note').hidden = !existing;
    let quick = '立即安装';
    let title = '准备就绪';
    if (existing) {
      const old = existing.version || '旧版本';
      const order = compareVersions(m.version, existing.version);
      $('#upgrade-title').textContent = order < 0 ? `已安装较新的 ${old}` : `检测到已安装 ${old}`;
      $('#upgrade-detail').textContent = order > 0
        ? `将更新到 ${m.version},你的项目与设置会保留。`
        : order === 0
          ? '将重新安装同一版本,你的项目与设置会保留。'
          : `将换成较旧的 ${m.version},你的项目与设置会保留。`;
      quick = order > 0 ? '立即更新' : order === 0 ? '重新安装' : '继续安装';
      title = order > 0 ? '准备更新' : order === 0 ? '准备重新安装' : '准备安装';
      $('[data-phase="remove"]').hidden = false;
    }
    bind('quickLabel', quick);
    $('.card-title').textContent = title;
    $('#data-help').firstChild.textContent = d.dataDirLocked
      ? '由环境变量 FORGE_DATA_DIR 指定,安装程序不会更改。'
      : '卸载和升级都不会删除这里的数据。';
    applyOptions();
    validate(0);
    buildHeroMark();
    document.body.classList.add('intro');
    document.body.classList.remove('booting');
    requestAnimationFrame(updateStepper);
  }

  // ───────────── 校验 ─────────────

  let validateTimer = 0;
  function validate(delay = 160) {
    clearTimeout(validateTimer);
    validateTimer = setTimeout(() => {
      state.seq += 1;
      post({ type: 'validate', seq: state.seq, installDir: state.opts.installDir, dataDir: state.opts.dataDir });
    }, delay);
  }

  const INSTALL_MESSAGES = {
    existing: ['info', '这里已安装旧版本,将直接覆盖更新,项目与设置不受影响。'],
    foreign: ['warn', '这个文件夹里已有其他文件。卸载或升级时会删除整个安装文件夹,请换一个新的或空的文件夹。'],
    file: ['warn', '这是一个文件,请选择文件夹。'],
    invalid: ['warn', '请填写完整、存在的盘符下的路径,例如 D:\\Apps\\RurixForge。'],
    protected: ['warn', '这个位置需要管理员权限,按用户安装写不进去。请换到其他磁盘或用户文件夹,例如 D:\\Apps\\RurixForge。'],
  };

  const DATA_MESSAGES = {
    'inside-install': '数据位置不能放在程序文件夹里,卸载或升级时会被一起删除。',
    file: '这是一个文件,请选择文件夹。',
    invalid: '请填写完整路径,例如 D:\\RurixForge 数据。',
  };

  function problems(c) {
    const list = [];
    if (!c) return list;
    if (c.installState !== 'fresh' && c.installState !== 'existing') list.push(INSTALL_MESSAGES[c.installState][1]);
    if (c.freeBytes != null && c.freeBytes < c.neededBytes) {
      list.push(`${c.drive || '目标'} 盘空间不足:安装过程需要约 ${formatBytes(c.neededBytes)}(含临时文件),可用 ${formatBytes(c.freeBytes)}。`);
    }
    if (c.tempFreeBytes != null && c.tempFreeBytes < c.tempRequiredBytes) {
      list.push(`临时文件夹所在磁盘空间不足,安装时需要约 ${formatBytes(c.tempRequiredBytes)} 临时空间。`);
    }
    if (c.dataState !== 'ok') list.push(DATA_MESSAGES[c.dataState] || '数据位置不可用。');
    return list;
  }

  function canInstall() {
    return Boolean(state.check) && problems(state.check).length === 0;
  }

  function onValidation(c) {
    if (c.seq !== state.seq) return;
    state.check = c;
    if (state.normalizeInstall && document.activeElement !== $('#install-dir')) {
      state.normalizeInstall = false;
      state.opts.installDir = c.installDir;
      $('#install-dir').value = c.installDir;
    }
    bind('installDirResolved', c.installDir);
    $('#install-help').hidden = samePath(c.installDir, state.opts.installDir);

    const msg = INSTALL_MESSAGES[c.installState];
    const status = $('#install-status');
    status.textContent = msg ? msg[1] : '';
    status.className = `status${msg && msg[0] === 'info' ? ' info' : ''}`;
    $('#install-dir').closest('.field').classList.toggle('invalid', Boolean(msg && msg[0] === 'warn'));

    const dataStatus = $('#data-status');
    dataStatus.textContent = c.dataState === 'ok' ? '' : DATA_MESSAGES[c.dataState] || '数据位置不可用。';
    $('#data-dir').closest('.field').classList.toggle('invalid', c.dataState !== 'ok');

    const space = $('#space');
    const short = c.freeBytes != null && c.freeBytes < c.neededBytes;
    space.classList.toggle('short', short);
    if (c.totalBytes) {
      const used = Math.max(0, c.totalBytes - (c.freeBytes || 0));
      space.style.setProperty('--used', `${Math.min(100, (used / c.totalBytes) * 100).toFixed(2)}%`);
      space.style.setProperty('--need', `${Math.min(100, (c.requiredBytes / c.totalBytes) * 100).toFixed(2)}%`);
    }
    $('#space-text').textContent = c.freeBytes == null
      ? `需要 ${formatBytes(c.requiredBytes)}`
      : `需要 ${formatBytes(c.requiredBytes)} · ${c.drive || ''} 盘可用 ${formatBytes(c.freeBytes)}`;
    $('#foot-summary').innerHTML = '';
    const foot = $('#foot-summary');
    foot.append('将占用 ');
    const b = document.createElement('b');
    b.textContent = formatBytes(c.requiredBytes);
    foot.append(b, c.freeBytes == null ? '' : ` · ${c.drive || ''} 盘剩余 ${formatBytes(Math.max(0, c.freeBytes - c.requiredBytes))}`);

    $('#running-note').hidden = !c.running;

    const list = problems(c);
    const ok = list.length === 0;
    $('#btn-install').disabled = !ok;
    const alert = $('#welcome-alert');
    alert.hidden = ok;
    alert.textContent = ok ? '' : `${list[0]} 请在「自定义安装选项」里调整。`;
    $('#btn-quick').disabled = !ok;
  }

  // ───────────── 选项页交互 ─────────────

  $('#install-dir').addEventListener('input', (e) => {
    state.opts.installDir = e.target.value;
    validate();
  });
  $('#install-dir').addEventListener('change', () => {
    state.normalizeInstall = true;
    validate(0);
  });
  $('#data-dir').addEventListener('input', (e) => {
    state.opts.dataDir = e.target.value;
    bind('dataDir', state.opts.dataDir);
    applyResetLink();
    validate();
  });

  function applyResetLink() {
    const info = state.info;
    if (!info) return;
    const custom = !info.defaults.dataDirLocked
      && state.opts.dataDir.trim().toLowerCase() !== info.defaults.dataDirDefault.toLowerCase();
    $('#btn-data-reset').hidden = !custom;
  }

  $('#btn-data-reset').addEventListener('click', () => {
    state.opts.dataDir = state.info.defaults.dataDirDefault;
    applyOptions();
    validate(0);
  });

  $('#btn-pick-install').addEventListener('click', () => post({ type: 'pick', target: 'install', current: state.opts.installDir }));
  $('#btn-pick-data').addEventListener('click', () => post({ type: 'pick', target: 'data', current: state.opts.dataDir }));

  function onPicked(m) {
    if (!m.path) return;
    if (m.target === 'data') {
      state.opts.dataDir = m.path;
    } else {
      state.opts.installDir = m.path;
      state.normalizeInstall = true;
    }
    applyOptions();
    validate(0);
  }

  $$('.switch').forEach((sw) => {
    sw.addEventListener('click', (e) => {
      e.stopPropagation();
      const key = sw.dataset.opt;
      state.opts[key] = !state.opts[key];
      applyOptions();
    });
  });
  $$('.toggle-row').forEach((row) => row.addEventListener('click', () => row.querySelector('.switch').click()));

  $('#btn-customize').addEventListener('click', () => go('options'));
  $('#btn-back').addEventListener('click', () => go('welcome'));
  $('#btn-options-back').addEventListener('click', () => go('welcome'));
  $('#btn-quick').addEventListener('click', startInstall);
  $('#btn-install').addEventListener('click', startInstall);

  // ───────────── 安装进度 ─────────────

  function placeForgeTarget() {
    if (!embers) return;
    const ring = $('.forge').getBoundingClientRect();
    embers.setTarget(ring.left + ring.width / 2, ring.top + ring.height / 2, ring.width * 0.36);
  }

  if (embers) {
    embers.onAbsorb = () => { state.flare = Math.min(1, state.flare + 0.12); };
  }
  window.addEventListener('resize', () => {
    updateStepper();
    if (state.page === 'installing') placeForgeTarget();
  });

  const PHASES = ['prepare', 'remove', 'copy', 'finish'];

  function renderSteps(phase, done) {
    const index = PHASES.indexOf(phase);
    $$('#steps li').forEach((li) => {
      const i = PHASES.indexOf(li.dataset.phase);
      li.classList.toggle('done', done || i < index);
      li.classList.toggle('active', !done && i === index);
    });
  }

  function startInstall() {
    if (state.installing) return;
    if (!canInstall()) {
      go('options');
      return;
    }
    state.installing = true;
    state.target = 0;
    state.shown = 0;
    state.phase = 'prepare';
    $('#copy-meta').textContent = '';
    renderSteps('prepare', false);
    setBusy(true);
    post({ type: 'install', ...state.opts });
    go('installing');
  }

  function onProgress(m) {
    state.target = Math.max(state.target, Math.min(100, m.percent));
    if (m.phase !== state.phase) {
      state.phase = m.phase;
      renderSteps(m.phase, false);
      const label = $(`#steps [data-phase="${m.phase}"] .step-text`);
      if (label) $('#progress-live').textContent = label.textContent;
    }
    if (m.phase === 'copy' || m.phase === 'finish') {
      $('#copy-meta').textContent = `${formatBytes(Math.min(m.bytes, m.total))} / ${formatBytes(m.total)}`;
    }
  }

  // 进度环、百分比与进度条跟着目标值平滑追赶。
  let lastFrame = performance.now();
  function animate(now) {
    const dt = Math.min(0.1, (now - lastFrame) / 1000);
    lastFrame = now;
    if (state.page === 'installing' || state.shown < state.target) {
      const k = reduced.matches ? 1 : Math.min(1, dt * 3.2);
      state.shown += (state.target - state.shown) * k;
      if (state.target - state.shown < 0.05) state.shown = state.target;
      const p = state.shown;
      $('.ring-fill').style.strokeDashoffset = String(100 - p);
      $('.ring-glow').style.strokeDashoffset = String(100 - p);
      $('#percent').textContent = String(Math.floor(p));
      $('#bar-fill').style.width = `${p}%`;
      state.flare = Math.max(0, state.flare - dt * 1.6);
      $('.ring-glow').style.opacity = String(0.42 + state.flare * 0.5);
    }
    requestAnimationFrame(animate);
  }
  requestAnimationFrame(animate);

  function whenFull(callback) {
    state.target = 100;
    const started = performance.now();
    const wait = () => {
      if (state.shown >= 99.5 || reduced.matches || performance.now() - started > 1600) callback();
      else requestAnimationFrame(wait);
    };
    wait();
  }

  function onDone(m) {
    state.installing = false;
    state.installedDir = m.installDir;
    bind('installedDir', m.installDir);
    renderSteps('finish', true);
    setBusy(false);
    whenFull(() => {
      setTimeout(() => {
        go('done');
        setTimeout(() => {
          const badge = $('#page-done .badge').getBoundingClientRect();
          if (embers) embers.burst(badge.left + badge.width / 2, badge.top + badge.height / 2, 150);
        }, reduced.matches ? 0 : 520);
        if (state.opts.launchAfter) startAutoLaunch();
      }, 260);
    });
  }

  function onFailed(m) {
    state.installing = false;
    setBusy(false);
    $('#failed-message').textContent = m.message || '安装程序意外结束。';
    go('failed');
  }

  // ───────────── 完成页 ─────────────

  const COUNTDOWN_MS = 3200;

  function startAutoLaunch() {
    const button = $('#btn-launch');
    button.style.setProperty('--countdown', `${COUNTDOWN_MS}ms`);
    button.classList.remove('counting');
    void button.offsetWidth;
    button.classList.add('counting');
    $('#autolaunch-text').hidden = false;
    clearTimeout(state.autoLaunch);
    state.autoLaunch = setTimeout(launch, COUNTDOWN_MS + 500);
  }

  function cancelAutoLaunch() {
    clearTimeout(state.autoLaunch);
    state.autoLaunch = null;
    $('#btn-launch').classList.remove('counting');
    $('#autolaunch-text').hidden = true;
  }

  function launch() {
    cancelAutoLaunch();
    post({ type: 'launch' });
  }

  $('#btn-launch').addEventListener('click', launch);
  $('#btn-cancel-auto').addEventListener('click', cancelAutoLaunch);
  $('#btn-finish').addEventListener('click', () => {
    cancelAutoLaunch();
    post({ type: 'close' });
  });
  $('#btn-open-folder').addEventListener('click', () => {
    cancelAutoLaunch();
    post({ type: 'openFolder' });
  });
  $('#btn-retry').addEventListener('click', () => {
    validate(0);
    go('options');
  });
  $('#btn-failed-close').addEventListener('click', () => post({ type: 'close' }));

  // ───────────── 标题栏 ─────────────

  function setBusy(busy) {
    const close = $('#btn-close');
    close.setAttribute('aria-disabled', String(busy));
    close.title = busy ? '安装进行中,完成后才能关闭' : '关闭';
  }

  $('#btn-min').addEventListener('click', () => post({ type: 'minimize' }));
  $('#btn-close').addEventListener('click', () => {
    if (state.installing) toast('正在安装,完成后才能关闭窗口');
    else post({ type: 'close' });
  });

  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    const region = e.target.closest('[data-drag]');
    if (region && !e.target.closest('button, input, a')) post({ type: 'drag' });
  });

  // ───────────── 许可协议 ─────────────

  let sheetReturn = null;

  function openSheet() {
    sheetReturn = document.activeElement;
    $('#license-sheet').hidden = false;
    $('#license-text').scrollTop = 0;
    $('#btn-license-ok').focus({ preventScroll: true });
  }

  function closeSheet() {
    $('#license-sheet').hidden = true;
    if (sheetReturn) sheetReturn.focus({ preventScroll: true });
  }

  $('#btn-license').addEventListener('click', openSheet);
  $('#btn-license-close').addEventListener('click', closeSheet);
  $('#btn-license-ok').addEventListener('click', closeSheet);
  $('#license-sheet').addEventListener('mousedown', (e) => {
    if (e.target === e.currentTarget) closeSheet();
  });
  $('#btn-homepage').addEventListener('click', () => {
    if (state.info && state.info.homepage) post({ type: 'open', url: state.info.homepage });
  });

  document.addEventListener('keydown', (e) => {
    const sheet = $('#license-sheet');
    if (!sheet.hidden) {
      if (e.key === 'Escape') {
        e.preventDefault();
        closeSheet();
      } else if (e.key === 'Tab') {
        // 焦点留在弹层里
        const items = $$('button, [tabindex="0"]', sheet).filter((el) => !el.hidden && !el.disabled);
        const first = items[0];
        const last = items[items.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first.focus();
        }
      }
      return;
    }
    if (e.key === 'Escape' && state.page === 'options') go('welcome');
  });

  document.addEventListener('contextmenu', (e) => {
    if (!e.target.closest('input, .license-text, .mono')) e.preventDefault();
  });

  // ───────────── 消息入口 ─────────────

  const handlers = {
    init: onInit,
    validation: onValidation,
    picked: onPicked,
    progress: onProgress,
    done: onDone,
    failed: onFailed,
    busy: () => toast('正在安装,完成后才能关闭窗口'),
  };

  window.__setup = {
    receive(message) {
      const handler = message && handlers[message.type];
      if (handler) handler(message);
    },
  };

  post({ type: 'ready' });
})();
