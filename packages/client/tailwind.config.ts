import type { Config } from 'tailwindcss';

/**
 * 设计 token:复刻 Cursor Agent IDE 浅色主题(见录屏)。
 * 整体为暖灰中性色:白底、暖灰边框、近黑文字、黑色主按钮。
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
      },
      fontFamily: {
        sans: ['"DM Sans Variable"', 'system-ui', '-apple-system', '"Segoe UI"', 'sans-serif'],
        mono: ['"JetBrains Mono Variable"', 'Consolas', 'monospace'],
      },
      fontSize: {
        '2xs': ['11px', '14px'],
        xs: ['12px', '16px'],
        sm: ['13px', '18px'],
        base: ['14px', '20px'],
      },
      borderRadius: {
        xl: '12px',
        '2xl': '16px',
      },
      boxShadow: {
        composer: '0 1px 2px rgba(0,0,0,0.04), 0 0 0 1px #e8e6e2',
        pop: '0 8px 30px rgba(0,0,0,0.12), 0 0 0 1px rgba(0,0,0,0.04)',
      },
    },
  },
  plugins: [],
} satisfies Config;
