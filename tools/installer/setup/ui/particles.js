'use strict';

/*
 * 背景粒子:灰阶浮尘从窗口底部缓缓升起;只有安装中的汇聚和完成时的迸发掺入少量强调色。
 *   ambient  平时:稀疏的浮尘上升、轻微摆动,鼠标靠近时被推开。
 *   forge    安装中:粒子拖着尾迹从四周盘旋汇入进度环,被吸收时回调 onAbsorb。
 *   burst()  完成时从中心迸发一圈。
 * 系统开启「减少动画」时只画一帧静态画面。
 */
(function () {
  const NEUTRAL = [
    { core: [70, 70, 70], glow: [140, 140, 140] },
    { core: [105, 105, 105], glow: [170, 170, 170] },
    { core: [140, 140, 140], glow: [196, 196, 196] },
  ];
  const ACCENT = [
    { core: [194, 90, 50], glow: [226, 140, 108] }, // Claude 陶土橙
    { core: [217, 119, 87], glow: [236, 168, 140] },
  ];

  function canvas(size) {
    const c = document.createElement('canvas');
    c.width = c.height = size;
    return c;
  }

  function emberSprite({ core, glow }) {
    const c = canvas(64);
    const g = c.getContext('2d');
    const grad = g.createRadialGradient(32, 32, 0, 32, 32, 32);
    grad.addColorStop(0, `rgba(${core}, 1)`);
    grad.addColorStop(0.16, `rgba(${core}, 0.95)`);
    grad.addColorStop(0.32, `rgba(${glow}, 0.55)`);
    grad.addColorStop(0.62, `rgba(${glow}, 0.16)`);
    grad.addColorStop(1, `rgba(${glow}, 0)`);
    g.fillStyle = grad;
    g.fillRect(0, 0, 64, 64);
    return c;
  }

  const rand = (a, b) => a + Math.random() * (b - a);
  const pick = (list) => list[(Math.random() * list.length) | 0];

  class Embers {
    constructor(el) {
      this.canvas = el;
      this.ctx = el.getContext('2d');
      const tone = (t) => ({ sprite: emberSprite(t), glow: `rgb(${t.glow})` });
      this.neutral = NEUTRAL.map(tone);
      this.accent = ACCENT.map(tone);
      this.particles = [];
      this.mode = 'ambient';
      this.target = null;
      this.pointer = null;
      this.onAbsorb = null;
      this.running = false;
      this.last = 0;
      this.spawnDebt = 0;
      this.slowFrames = 0;
      this.budget = 1;
      this.reduced = matchMedia('(prefers-reduced-motion: reduce)');
      this.frame = this.frame.bind(this);
      window.addEventListener('resize', () => this.resize());
      window.addEventListener('pointermove', (e) => { this.pointer = { x: e.clientX, y: e.clientY }; });
      document.addEventListener('pointerleave', () => { this.pointer = null; });
      document.addEventListener('visibilitychange', () => (document.hidden ? this.stop() : this.start()));
      this.reduced.addEventListener('change', () => this.start());
      this.resize();
      this.seed();
      this.start();
    }

    get width() { return this.canvas.clientWidth; }
    get height() { return this.canvas.clientHeight; }

    resize() {
      this.dpr = Math.min(window.devicePixelRatio || 1, 2);
      this.canvas.width = Math.round(this.width * this.dpr);
      this.canvas.height = Math.round(this.height * this.dpr);
      if (this.reduced.matches) this.drawStatic();
    }

    ambientTarget() {
      return Math.round(((this.width * this.height) / 8200) * this.budget);
    }

    spawnAmbient(anywhere) {
      const h = this.height;
      const life = rand(7, 15);
      this.particles.push({
        kind: 'ambient',
        x: rand(0, this.width),
        y: anywhere ? rand(h * 0.12, h + 20) : h + rand(8, 40),
        vx: rand(-6, 6),
        vy: -rand(12, 34),
        sway: rand(6, 20),
        freq: rand(0.4, 1.1),
        phase: rand(0, Math.PI * 2),
        size: rand(1.1, 3),
        age: anywhere ? rand(0, life * 0.6) : 0,
        life,
        tone: pick(this.neutral),
        twinkle: rand(1.5, 4),
      });
    }

    spawnForge() {
      const t = this.target;
      if (!t) return;
      const angle = rand(0, Math.PI * 2);
      const dist = Math.max(this.width, this.height) * rand(0.42, 0.62);
      this.particles.push({
        kind: 'forge',
        x: t.x + Math.cos(angle) * dist,
        y: t.y + Math.sin(angle) * dist,
        vx: 0,
        vy: 0,
        swirl: rand(0.45, 1.05) * (Math.random() < 0.5 ? -1 : 1),
        speed: rand(80, 150),
        size: rand(1.4, 3.2),
        age: 0,
        life: 12,
        tone: pick(Math.random() < 0.35 ? this.accent : this.neutral),
        twinkle: rand(2, 5),
        phase: rand(0, Math.PI * 2),
      });
    }

    seed() {
      this.particles = [];
      const n = this.ambientTarget();
      for (let i = 0; i < n; i++) this.spawnAmbient(true);
    }

    setMode(mode) {
      this.mode = mode;
    }

    setTarget(x, y, r) {
      this.target = { x, y, r };
    }

    burst(x, y, count = 100) {
      if (this.reduced.matches) return;
      for (let i = 0; i < count; i++) {
        const angle = rand(0, Math.PI * 2);
        const speed = rand(150, 540) * (i % 5 === 0 ? 1.25 : 1);
        this.particles.push({
          kind: 'spark',
          x,
          y,
          vx: Math.cos(angle) * speed,
          vy: Math.sin(angle) * speed - rand(40, 150),
          size: rand(1.3, 3.6),
          age: 0,
          life: rand(1.1, 2.2),
          tone: pick(Math.random() < 0.45 ? this.accent : this.neutral),
          twinkle: rand(6, 12),
          phase: rand(0, Math.PI * 2),
        });
      }
      this.start();
    }

    start() {
      if (this.reduced.matches) {
        this.stop();
        this.drawStatic();
        return;
      }
      if (this.running || document.hidden) return;
      this.running = true;
      this.last = performance.now();
      requestAnimationFrame(this.frame);
    }

    stop() {
      this.running = false;
    }

    frame(now) {
      if (!this.running) return;
      const elapsed = now - this.last;
      const dt = Math.min(elapsed / 1000, 0.05);
      // 持续掉帧就少画一些,保证界面本身流畅。
      if (elapsed > 28) this.slowFrames++;
      else this.slowFrames = Math.max(0, this.slowFrames - 1);
      if (this.slowFrames > 40 && this.budget > 0.45) {
        this.budget -= 0.15;
        this.slowFrames = 0;
      }
      this.last = now;
      this.step(dt, now / 1000);
      this.draw(now / 1000);
      requestAnimationFrame(this.frame);
    }

    step(dt, time) {
      let ambient = 0;
      for (const p of this.particles) if (p.kind === 'ambient') ambient++;
      const wantAmbient = this.mode === 'forge' ? Math.round(this.ambientTarget() * 0.45) : this.ambientTarget();
      for (let i = ambient; i < wantAmbient; i++) {
        if (Math.random() < dt * 3) this.spawnAmbient(false);
      }
      if (this.mode === 'forge' && this.target) {
        this.spawnDebt += dt * 24 * this.budget;
        while (this.spawnDebt >= 1) {
          this.spawnDebt -= 1;
          this.spawnForge();
        }
      }

      const ptr = this.pointer;
      const keep = [];
      for (const p of this.particles) {
        p.age += dt;
        if (p.kind === 'ambient') {
          if (this.mode === 'forge' && p.age > p.life * 0.5 && Math.random() < dt * 0.2) p.age = p.life;
          p.x += (p.vx + Math.cos(time * p.freq + p.phase) * p.sway * 0.9) * dt;
          p.y += p.vy * dt;
          if (ptr) {
            const dx = p.x - ptr.x;
            const dy = p.y - ptr.y;
            const d2 = dx * dx + dy * dy;
            if (d2 < 120 * 120 && d2 > 1) {
              const d = Math.sqrt(d2);
              const f = (1 - d / 120) * 260 * dt;
              p.x += (dx / d) * f;
              p.y += (dy / d) * f;
            }
          }
          if (p.age < p.life && p.y > -30) keep.push(p);
        } else if (p.kind === 'forge') {
          const t = this.target;
          if (!t) continue;
          const dx = t.x - p.x;
          const dy = t.y - p.y;
          const d = Math.hypot(dx, dy) || 1;
          // 越靠近越快,带切向分量形成漩涡。
          const pull = p.speed * (1 + 260 / (d + 40));
          const nx = dx / d;
          const ny = dy / d;
          const ease = Math.min(1, dt * 3);
          p.vx += ((nx - ny * p.swirl) * pull - p.vx) * ease;
          p.vy += ((ny + nx * p.swirl) * pull - p.vy) * ease;
          p.x += p.vx * dt;
          p.y += p.vy * dt;
          if (d < t.r) {
            if (this.onAbsorb) this.onAbsorb();
            continue;
          }
          if (p.age < p.life) keep.push(p);
        } else {
          p.vx *= Math.exp(-1.8 * dt);
          p.vy = p.vy * Math.exp(-1.8 * dt) + 300 * dt;
          p.x += p.vx * dt;
          p.y += p.vy * dt;
          if (p.age < p.life) keep.push(p);
        }
      }
      this.particles = keep;
    }

    alpha(p, time) {
      const fadeIn = Math.min(1, p.age / 0.8);
      const fadeOut = p.kind === 'spark' ? 1 - p.age / p.life : Math.min(1, (p.life - p.age) / 1.5);
      const tw = 0.62 + 0.38 * Math.sin(time * p.twinkle + p.phase);
      let a = Math.max(0, Math.min(fadeIn, fadeOut)) * tw;
      if (p.kind === 'ambient') {
        // 浮尘整体压淡,越往上越淡,别抢文字。
        a *= 0.55 * (0.4 + 0.6 * Math.min(1, p.y / (this.height * 0.75)));
      }
      return a;
    }

    draw(time) {
      const { ctx, dpr } = this;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, this.width, this.height);
      ctx.lineCap = 'round';
      for (const p of this.particles) {
        const a = this.alpha(p, time);
        if (a <= 0.01) continue;
        if (p.kind !== 'ambient') {
          // 尾迹:沿速度方向拖一小段
          const k = p.kind === 'forge' ? 0.045 : 0.035;
          ctx.globalAlpha = a * 0.35;
          ctx.strokeStyle = p.tone.glow;
          ctx.lineWidth = Math.max(1, p.size * 0.75);
          ctx.beginPath();
          ctx.moveTo(p.x - p.vx * k, p.y - p.vy * k);
          ctx.lineTo(p.x, p.y);
          ctx.stroke();
        }
        const s = p.size * (p.kind === 'forge' ? 4.6 : 4.2);
        ctx.globalAlpha = a;
        ctx.drawImage(p.tone.sprite, p.x - s / 2, p.y - s / 2, s, s);
      }
      ctx.globalAlpha = 1;
    }

    drawStatic() {
      const { ctx } = this;
      const dpr = this.dpr || 1;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, this.width, this.height);
      const n = Math.round(this.ambientTarget() * 0.6);
      for (let i = 0; i < n; i++) {
        const y = rand(this.height * 0.3, this.height);
        const s = rand(1.1, 3) * 4.2;
        ctx.globalAlpha = 0.15 + 0.3 * (y / this.height);
        ctx.drawImage(this.neutral[i % this.neutral.length].sprite, rand(0, this.width) - s / 2, y - s / 2, s, s);
      }
      ctx.globalAlpha = 1;
    }
  }

  window.Embers = Embers;
})();
