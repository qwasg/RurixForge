import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  applyPalette,
  defaultAppearance,
  effectiveDark,
  mixHex,
  MOONLIT_DARK,
  MOONLIT_LIGHT,
  normalizeHex,
  presetById,
  THEME_PRESETS,
  useThemeStore,
} from '@/lib/themeStore';

/**
 * F7 wave.3 G-F7-3:派色算法对照参考 appearance.rs apply_palette 固定输入期望值。
 * 期望值手工按参考公式核算(截断语义:rust f32 as u8 → Math.trunc):
 * - text_2/3/4 = mix(fg,bg, 0.42·(1.1-c/2) / 0.62·(1.05-0.35c) / 0.78·(1.02-0.25c))
 * - accent_bg alpha 亮 0x14/暗 0x24;ring 亮 0x47/暗 0x6B
 * - translucent:sidebar=sunk+0xCC、float=panel+0xEE;否则实体
 */

const initialTheme = useThemeStore.getState();

beforeEach(() => {
  useThemeStore.setState(initialTheme, true);
  globalThis.localStorage?.clear();
  document.documentElement.removeAttribute('data-theme');
  document.documentElement.style.cssText = '';
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('applyPalette 派色对照(参考逐行移植)', () => {
  it('moonlit 亮表固定输入 → 全 token 期望值', () => {
    const t = applyPalette(MOONLIT_LIGHT, false);
    expect(t['bg']).toBe('#FAF9F5');
    expect(t['bg-sunk']).toBe('#F0EFEB'); // 压暗 0.04
    expect(t['bg-panel']).toBe('#FAF9F5'); // 亮:panel=bg
    expect(t['bg-input']).toBe('#FAF9F5');
    expect(t['text']).toBe('#2A2724');
    expect(t['text-2']).toBe('#767470'); // mix(fg,bg,0.42·(1.1-0.225))
    expect(t['text-3']).toBe('#9D9B97');
    expect(t['text-4']).toBe('#BDBBB7');
    expect(t['text-inv']).toBe('#FAF9F5'); // 亮:固定奶油
    expect(t['accent']).toBe('#C96442');
    expect(t['accent-soft']).toBe('#B85C3C'); // 压暗 0.08
    expect(t['accent-bg']).toBe('#C9644214'); // alpha 0x14
    expect(t['accent-ring']).toBe('#C9644247'); // alpha 0x47
    expect(t['bg-selection']).toBe('#C9644214'); // = accent_bg
    expect(t['line']).toBe('#2A272417'); // fg+0x17
    expect(t['line-strong']).toBe('#2A272424'); // fg+0x24
    expect(t['bg-hover']).toBe('#2A27240A'); // rgba(42,39,36,0.04)
    expect(t['bg-active']).toBe('#2A272412'); // rgba(42,39,36,0.07)
    expect(t['bg-sidebar']).toBe('#F0EFEB'); // translucent=false → 实体 sunk
    expect(t['bg-float']).toBe('#FAF9F5'); // 实体 panel
    expect(t['dot-running']).toBe('#C96442'); // = accent
  });

  it('moonlit 暗表固定输入 → 全 token 期望值', () => {
    const t = applyPalette(MOONLIT_DARK, true);
    expect(t['bg']).toBe('#1C1B18');
    expect(t['bg-sunk']).toBe('#1A1916'); // 压暗 0.06
    expect(t['bg-panel']).toBe('#252421'); // 暗:提亮 0.04
    expect(t['text']).toBe('#ECE8DF');
    expect(t['text-2']).toBe('#A6A39C'); // mix(fg,bg,0.42·(1.1-0.3))
    expect(t['text-3']).toBe('#7F7D77');
    expect(t['text-4']).toBe('#5E5C57');
    expect(t['text-inv']).toBe('#1C1B18'); // 暗:= palette.bg
    expect(t['accent']).toBe('#E2886A');
    expect(t['accent-soft']).toBe('#E69980'); // 提亮 0.15
    expect(t['accent-bg']).toBe('#E2886A24'); // alpha 0x24
    expect(t['accent-ring']).toBe('#E2886A6B'); // alpha 0x6B
    expect(t['line']).toBe('#ECE8DF12'); // fg+0x12
    expect(t['line-strong']).toBe('#ECE8DF21'); // fg+0x21
    expect(t['bg-hover']).toBe('#FFFFFF0D'); // 白 0x0d
    expect(t['bg-active']).toBe('#FFFFFF14'); // 白 0x14
    expect(t['bg-sidebar']).toBe('#1A1916');
    expect(t['bg-float']).toBe('#252421');
  });

  it('accent_bg/ring alpha 明暗差异(0x14/0x24、0x47/0x6B)', () => {
    const p = { ...MOONLIT_LIGHT };
    const light = applyPalette(p, false);
    const dark = applyPalette({ ...MOONLIT_LIGHT, background: '#1C1B18', foreground: '#ECE8DF' }, true);
    expect(light['accent-bg'].slice(7)).toBe('14');
    expect(dark['accent-bg'].slice(7)).toBe('24');
    expect(light['accent-ring'].slice(7)).toBe('47');
    expect(dark['accent-ring'].slice(7)).toBe('6B');
  });

  it('contrast 两极端混色方向:对比度越高混得越少(更靠近 fg)', () => {
    // fg 纯黑 / bg 纯白 → mix 比例 t 越小越深
    const base = { accent: '#C96442', background: '#FFFFFF', foreground: '#000000', translucentSidebar: false };
    const c0 = applyPalette({ ...base, contrast: 0 }, false);
    const c100 = applyPalette({ ...base, contrast: 100 }, false);
    // text_2:t=0.42·1.1=0.462 → #757575;t=0.42·0.6=0.252 → #404040
    expect(c0['text-2']).toBe('#757575');
    expect(c100['text-2']).toBe('#404040');
    // text_3:t=0.62·1.05=0.651 → #A6A6A6;t=0.62·0.7=0.434 → #6E6E6E
    expect(c0['text-3']).toBe('#A6A6A6');
    expect(c100['text-3']).toBe('#6E6E6E');
    // text_4:t=0.78·1.02=0.7956 → #CACACA;t=0.78·0.77=0.6006 → #999999
    expect(c0['text-4']).toBe('#CACACA');
    expect(c100['text-4']).toBe('#999999');
  });

  it('translucent_sidebar 双 alpha(0xCC/0xEE)vs 实体', () => {
    const solid = applyPalette({ ...MOONLIT_LIGHT, translucentSidebar: false }, false);
    expect(solid['bg-sidebar']).toBe('#F0EFEB');
    expect(solid['bg-float']).toBe('#FAF9F5');
    const trans = applyPalette({ ...MOONLIT_LIGHT, translucentSidebar: true }, false);
    expect(trans['bg-sidebar']).toBe('#F0EFEBCC'); // sunk + 0xCC
    expect(trans['bg-float']).toBe('#FAF9F5EE'); // panel + 0xEE
  });

  it('mixHex 截断语义(rust as u8,非四舍五入)', () => {
    // 250·0.96=240.0 / 249·0.96=239.04→239 / 245·0.96=235.2→235
    expect(mixHex('#FAF9F5', '#000000', 0.04)).toBe('#F0EFEB');
    expect(normalizeHex('339cff')).toBe('#339CFF');
    expect(normalizeHex('gggggg')).toBeNull();
  });
});

describe('themeStore', () => {
  it('默认 = moonlit 预设,mode auto,13/12', () => {
    const d = defaultAppearance();
    expect(d.presetId).toBe('moonlit');
    expect(d.light.accent).toBe('#C96442');
    expect(d.dark.accent).toBe('#E2886A');
    expect(d.uiSize).toBe(13);
    expect(d.codeSize).toBe(12);
    expect(THEME_PRESETS).toHaveLength(10);
    expect(presetById('codex')?.light.translucentSidebar).toBe(true);
  });

  it('effectiveDark:light/dark/auto 三态', () => {
    expect(effectiveDark('light', true)).toBe(false);
    expect(effectiveDark('dark', false)).toBe(true);
    expect(effectiveDark('auto', true)).toBe(true);
    expect(effectiveDark('auto', false)).toBe(false);
  });

  it('setMode(dark):data-theme + --accent 注入 + localStorage 持久化', () => {
    useThemeStore.getState().setMode('dark');
    expect(useThemeStore.getState().isDark).toBe(true);
    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#E2886A');
    const raw = globalThis.localStorage?.getItem('forge:appearance');
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw as string).mode).toBe('dark');
    useThemeStore.getState().setMode('light');
    expect(document.documentElement.dataset.theme).toBe('light');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#C96442');
  });

  it('applyPreset(github):亮暗双表切换 + 预设 id 记录', () => {
    useThemeStore.getState().setMode('light');
    useThemeStore.getState().applyPreset('github');
    const s = useThemeStore.getState();
    expect(s.presetId).toBe('github');
    expect(s.light.accent).toBe('#0969DA');
    expect(s.dark.accent).toBe('#4493F8');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#0969DA');
  });

  it('mode=auto:syncSystemDark 随系统明暗翻转(matchMedia mock)', () => {
    useThemeStore.getState().setMode('auto');
    vi.stubGlobal('matchMedia', (q: string) => ({
      matches: true,
      media: q,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
    }));
    useThemeStore.getState().syncSystemDark();
    expect(useThemeStore.getState().isDark).toBe(true);
    expect(document.documentElement.dataset.theme).toBe('dark');
  });

  it('字号 clamp:uiSize 11–18 / codeSize 10–20', () => {
    useThemeStore.getState().setUiSize(99);
    expect(useThemeStore.getState().uiSize).toBe(18);
    useThemeStore.getState().setUiSize(1);
    expect(useThemeStore.getState().uiSize).toBe(11);
    useThemeStore.getState().setCodeSize(99);
    expect(useThemeStore.getState().codeSize).toBe(20);
    expect(document.documentElement.style.getPropertyValue('--code-size')).toBe('20px');
  });

  it('持久化读回:localStorage 预置 → 模块加载合并默认', async () => {
    globalThis.localStorage?.setItem(
      'forge:appearance',
      JSON.stringify({ mode: 'dark', presetId: 'gruvbox', light: { accent: '#B57614' } }),
    );
    vi.resetModules();
    const mod = await import('@/lib/themeStore');
    const s = mod.useThemeStore.getState();
    expect(s.mode).toBe('dark');
    expect(s.presetId).toBe('gruvbox');
    expect(s.light.accent).toBe('#B57614'); // 局部覆盖
    expect(s.light.background).toBe('#FAF9F5'); // 缺省回默认 moonlit
  });
});
