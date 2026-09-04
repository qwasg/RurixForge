import type { Config } from 'tailwindcss';

/**
 * 设计 token:复刻 Cursor Agent IDE 浅色主题(见录屏)。
 * 整体为暖灰中性色:白底、暖灰边框、近黑文字、黑色主按钮。
 *
 * F7 wave.3:新增 shell-* 语义 token → CSS 变量映射(Moonlit 派色,见 styles/theme.css
 * 与 lib/themeStore.ts);旧 ink/muted/line/panel/accent.green 等 token 保留不动——
 * EditorView(游戏原生)类名零改动继续解析(视觉与新壳有缝,如实留档,wave.5 再评统一)。
 * UI 融合波(2026-08-20 用户拍板):编辑器域(EditorView/editor-* 组件/index.css 组件类)
 * 已批量迁 shell 语义 token,亮暗主题打通;旧 token 仅余兼容定义,不再被编辑器面消费。
 */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        ink: {
          DEFAULT: '#1f1f1f', // 主文字
          soft: '#3d3d3d',
        },
        muted: {
          DEFAULT: '#6e6c68', // 次级文字
          faint: '#9c9994', // 更弱提示文字 / 占位
        },
        line: {
          DEFAULT: '#e8e6e2', // 常规边框
          soft: '#efedea', // 更浅分隔线
        },
        panel: {
          DEFAULT: '#faf9f7', // 侧栏 / 面板底色
          hover: '#f2f0ed', // 悬停行
          active: '#ebe9e5', // 选中行
        },
        accent: {
          green: '#0d7d4d', // Changes +N / 成功
          blue: '#2563eb', // 链接
        },
        // ---- F7 wave.3 新壳语义 token(映射 CSS 变量,随主题切换) ----
        shell: {
          bg: 'var(--bg)',
          sunk: 'var(--bg-sunk)',
          panel: 'var(--bg-panel)',
          input: 'var(--bg-input)',
          float: 'var(--bg-float)',
          sidebar: 'var(--bg-sidebar)',
          hover: 'var(--bg-hover)',
          active: 'var(--bg-active)',
          selection: 'var(--bg-selection)',
        },
        fg: {
          DEFAULT: 'var(--text)',
          2: 'var(--text-2)',
          3: 'var(--text-3)',
          4: 'var(--text-4)',
          inv: 'var(--text-inv)',
        },
        edge: {
          DEFAULT: 'var(--line)',
          strong: 'var(--line-strong)',
        },
        acc: {
          DEFAULT: 'var(--accent)',
          soft: 'var(--accent-soft)',
          bg: 'var(--accent-bg)',
          ring: 'var(--accent-ring)',
        },
        sage: {
          DEFAULT: 'var(--sage)',
          bg: 'var(--sage-bg)',
        },
        danger: {
          DEFAULT: 'var(--danger)',
          bg: 'var(--danger-bg)',
        },
        warn: {
          DEFAULT: 'var(--warn)',
          bg: 'var(--warn-bg)',
        },
        info: {
          DEFAULT: 'var(--info)',
          bg: 'var(--info-bg)',
        },
        dot: {
          running: 'var(--dot-running)',
          done: 'var(--dot-done)',
          idle: 'var(--dot-idle)',
          blocked: 'var(--dot-blocked)',
          queued: 'var(--dot-queued)',
        },
      },
      fontFamily: {
        // 净化波:统一走主题字体变量(Inter Variable + 系统中文栈,见 theme.css)
        sans: ['var(--font-sans)'],
        mono: ['"JetBrains Mono Variable"', 'Consolas', 'monospace'],
        shell: ['var(--font-sans)'],
        serif: ['var(--font-serif)'],
        code: ['var(--font-mono)'],
      },
      fontSize: {
        '2xs': ['11px', '14px'],
        xs: ['12px', '16px'],
        sm: ['13px', '18px'],
        base: ['14px', '20px'],
        // F7:UI 基准字号走变量(themeStore 11–18 可调)
        shell: ['var(--ui-size)', '1.45'],
        'shell-code': ['var(--code-size)', '1.5'],
      },
      borderRadius: {
        xl: '12px',
        '2xl': '16px',
      },
      boxShadow: {
        composer: '0 1px 2px rgba(0,0,0,0.04), 0 0 0 1px #e8e6e2',
        pop: '0 8px 30px rgba(0,0,0,0.12), 0 0 0 1px rgba(0,0,0,0.04)',
        // F7:参考 sh1/sh_float
        sh1: 'var(--sh-1)',
        float: 'var(--sh-float)',
      },
    },
  },
  plugins: [],
} satisfies Config;
