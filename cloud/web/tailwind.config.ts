import type { Config } from 'tailwindcss';

/** 颜色全部走 CSS 变量（RGB 三元组，见 src/index.css），亮/暗主题只切变量，组件不写 dark: 变体。 */
const v = (name: string) => `rgb(var(--${name}) / <alpha-value>)`;

export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        background: v('background'),
        foreground: v('foreground'),
        card: v('card'),
        muted: { DEFAULT: v('muted'), foreground: v('muted-foreground') },
        accent: v('accent'),
        border: v('border'),
        input: v('input'),
        ring: v('ring'),
        primary: { DEFAULT: v('primary'), foreground: v('primary-foreground') },
        success: v('success'),
        warning: v('warning'),
        danger: v('danger'),
        info: v('info'),
      },
      fontFamily: {
        sans: [
          'ui-sans-serif',
          'system-ui',
          '-apple-system',
          '"Segoe UI"',
          '"PingFang SC"',
          '"Hiragino Sans GB"',
          '"Microsoft YaHei"',
          '"Noto Sans CJK SC"',
          'sans-serif',
        ],
        mono: ['ui-monospace', '"Cascadia Code"', '"JetBrains Mono"', 'Consolas', 'monospace'],
      },
      fontSize: {
        xs: ['12px', '16px'],
        sm: ['13px', '18px'],
        base: ['14px', '20px'],
      },
      boxShadow: {
        pop: '0 10px 32px rgb(0 0 0 / 0.16), 0 0 0 1px rgb(0 0 0 / 0.04)',
      },
      keyframes: {
        'fade-in': { from: { opacity: '0' }, to: { opacity: '1' } },
        'dialog-in': {
          from: { opacity: '0', transform: 'scale(0.98)' },
          to: { opacity: '1', transform: 'scale(1)' },
        },
        'slide-in-right': { from: { transform: 'translateX(100%)' }, to: { transform: 'translateX(0)' } },
        'toast-in': {
          from: { opacity: '0', transform: 'translateY(8px)' },
          to: { opacity: '1', transform: 'translateY(0)' },
        },
      },
      animation: {
        'fade-in': 'fade-in 120ms ease-out',
        'dialog-in': 'dialog-in 140ms ease-out',
        'slide-in-right': 'slide-in-right 180ms ease-out',
        'toast-in': 'toast-in 160ms ease-out',
      },
    },
  },
  plugins: [],
} satisfies Config;
