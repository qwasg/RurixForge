import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  applyPalette,
  defaultAppearance,
  effectiveDark,
  FORGE_DARK,
  FORGE_LIGHT,
  migrateSurfaces,
  mixHex,
  MOONLIT_DARK,
  MOONLIT_LIGHT,
  normalizeHex,
  presetById,
  THEME_PRESETS,
  useThemeStore,
  type ThemePalette,
} from '@/lib/themeStore';

/**
 * F7 wave.3 G-F7-3:派色算法对照参考 appearance.rs apply_palette 固定输入期望值。
 * 期望值手工按参考公式核算(截断语义:rust f32 as u8 → Math.trunc):
 * - text_2/3/4 = mix(fg,bg, 0.33·(1.1-c/2) / 0.46·(1.05-0.35c) / 0.64·(1.02-0.25c))
 *   (可读性波 2026-10-07:基比由参考 0.42/0.62/0.78 收深,见 themeStore 文件头)
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
    expect(t['text-2']).toBe('#666360'); // mix(fg,bg,0.33·(1.1-0.225))
    expect(t['text-3']).toBe('#7F7D79');
    expect(t['text-4']).toBe('#A2A09D');
    expect(t['text-inv']).toBe('#FFFFFF'); // 净化波:亮反色文字 → 纯白
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
    expect(t['text-2']).toBe('#B5B1AA'); // mix(fg,bg,0.33·(1.1-0.3))
    expect(t['text-3']).toBe('#9B9892');
    expect(t['text-4']).toBe('#787570');
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
    // text_2:t=0.33·1.1=0.363 → #5C5C5C;t=0.33·0.6=0.198 → #323232
    expect(c0['text-2']).toBe('#5C5C5C');
    expect(c100['text-2']).toBe('#323232');
    // text_3:t=0.46·1.05=0.483 → #7B7B7B;t=0.46·0.7=0.322 → #525252
    expect(c0['text-3']).toBe('#7B7B7B');
    expect(c100['text-3']).toBe('#525252');
    // text_4:t=0.64·1.02=0.6528 → #A6A6A6;t=0.64·0.77=0.4928 → #7D7D7D
    expect(c0['text-4']).toBe('#A6A6A6');
    expect(c100['text-4']).toBe('#7D7D7D');
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

  it('forge 亮表固定输入 → 全 token 期望值(纯色波中性底,兜底 theme.css 静态表)', () => {
    const t = applyPalette(FORGE_LIGHT, false);
    expect(t['bg']).toBe('#FFFFFF');
    expect(t['bg-sunk']).toBe('#F4F4F4'); // 压暗 0.04
    expect(t['bg-panel']).toBe('#FFFFFF'); // 亮:panel=bg
    expect(t['bg-input']).toBe('#FFFFFF');
    expect(t['text']).toBe('#0A0A0A');
    expect(t['text-2']).toBe('#4C4C4C'); // mix(fg,bg,0.33·(1.1-0.275))
    expect(t['text-3']).toBe('#6A6A6A');
    expect(t['text-4']).toBe('#949494');
    expect(t['text-inv']).toBe('#FFFFFF');
    expect(t['accent']).toBe('#C94F12');
    expect(t['accent-soft']).toBe('#B84810'); // 压暗 0.08
    expect(t['accent-bg']).toBe('#C94F1214');
    expect(t['accent-ring']).toBe('#C94F1247');
    expect(t['bg-selection']).toBe('#C94F1214');
    expect(t['line']).toBe('#0A0A0A17');
    expect(t['line-strong']).toBe('#0A0A0A24');
    expect(t['bg-hover']).toBe('#0A0A0A0A'); // 净化波:fg 派生
    expect(t['bg-active']).toBe('#0A0A0A12');
    expect(t['bg-sidebar']).toBe('#F4F4F4');
    expect(t['bg-float']).toBe('#FFFFFF');
    expect(t['dot-running']).toBe('#C94F12');
    expect(t['sage']).toBe('#1D7A46'); // 净化波语义色
    expect(t['danger']).toBe('#CE3A1E');
    expect(t['warn']).toBe('#9A6A0A');
    expect(t['info']).toBe('#2563EB');
  });

  it('forge 暗表固定输入 → 全 token 期望值', () => {
    const t = applyPalette(FORGE_DARK, true);
    expect(t['bg']).toBe('#0A0A0A');
    expect(t['bg-sunk']).toBe('#090909'); // 压暗 0.06
    expect(t['bg-panel']).toBe('#131313'); // 暗:提亮 0.04
    expect(t['text']).toBe('#FAFAFA');
    expect(t['text-2']).toBe('#BABABA'); // mix(fg,bg,0.33·(1.1-0.3))
    expect(t['text-3']).toBe('#9D9D9D');
    expect(t['text-4']).toBe('#747474');
    expect(t['text-inv']).toBe('#0A0A0A');
    expect(t['accent']).toBe('#F47B33');
    expect(t['accent-soft']).toBe('#F58E51'); // 提亮 0.15
    expect(t['accent-bg']).toBe('#F47B3324');
    expect(t['accent-ring']).toBe('#F47B336B');
    expect(t['line']).toBe('#FAFAFA12');
    expect(t['line-strong']).toBe('#FAFAFA21');
    expect(t['bg-hover']).toBe('#FFFFFF0D');
    expect(t['bg-active']).toBe('#FFFFFF14');
    expect(t['bg-sidebar']).toBe('#090909');
    expect(t['bg-float']).toBe('#131313');
    expect(t['sage']).toBe('#7BC996');
    expect(t['danger']).toBe('#F0755A');
    expect(t['warn']).toBe('#E0B45C');
    expect(t['info']).toBe('#7AA8F0');
  });
});

describe('themeStore', () => {
  it('默认 = forge 预设(净化波),mode auto,13/12', () => {
    const d = defaultAppearance();
    expect(d.presetId).toBe('forge');
    expect(d.light.accent).toBe('#C94F12');
    expect(d.dark.accent).toBe('#F47B33');
    expect(d.uiSize).toBe(13);
    expect(d.codeSize).toBe(12);
    expect(THEME_PRESETS).toHaveLength(11);
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
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#F47B33');
    const raw = globalThis.localStorage?.getItem('forge:appearance');
    expect(raw).toBeTruthy();
    expect(JSON.parse(raw as string).mode).toBe('dark');
    useThemeStore.getState().setMode('light');
    expect(document.documentElement.dataset.theme).toBe('light');
    expect(document.documentElement.style.getPropertyValue('--accent')).toBe('#C94F12');
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
    expect(s.light.background).toBe('#FFFFFF'); // 缺省回默认 forge
  });

  it('净化波迁移:moonlit 预设 + 双表未自定义 → 自动切 forge', async () => {
    globalThis.localStorage?.setItem(
      'forge:appearance',
      JSON.stringify({
        ...defaultAppearance(),
        presetId: 'moonlit',
        light: { ...MOONLIT_LIGHT },
        dark: { ...MOONLIT_DARK },
      }),
    );
    vi.resetModules();
    const mod = await import('@/lib/themeStore');
    const s = mod.useThemeStore.getState();
    expect(s.presetId).toBe('forge');
    expect(s.light).toEqual(FORGE_LIGHT);
    expect(s.dark).toEqual(FORGE_DARK);
  });

  it('净化波迁移:moonlit 预设但强调色已自定义 → 不切 forge,强调色保留,奶油底按纯色波换中性', async () => {
    const customLight = { ...MOONLIT_LIGHT, accent: '#123456' };
    globalThis.localStorage?.setItem(
      'forge:appearance',
      JSON.stringify({
        ...defaultAppearance(),
        presetId: 'moonlit',
        light: customLight,
        dark: { ...MOONLIT_DARK },
      }),
    );
    vi.resetModules();
    const mod = await import('@/lib/themeStore');
    const s = mod.useThemeStore.getState();
    expect(s.presetId).toBe('moonlit');
    expect(s.light.accent).toBe('#123456');
    expect(s.light.background).toBe('#FFFFFF');
    expect(s.light.foreground).toBe('#0A0A0A');
    expect(s.dark.background).toBe('#0A0A0A');
  });

  it('纯色波:全部预设共用中性底 / 字 / 对比度,预设之间只剩强调色不同', () => {
    for (const p of THEME_PRESETS) {
      expect(p.light).toMatchObject({ background: '#FFFFFF', foreground: '#0A0A0A', contrast: 55 });
      expect(p.dark).toMatchObject({ background: '#0A0A0A', foreground: '#FAFAFA', contrast: 60 });
    }
    expect(presetById('catppuccin')?.light.accent).toBe('#8839EF');
    expect(presetById('gruvbox')?.dark.accent).toBe('#FABD2F');
  });

  it('纯色波迁移:codex 旧档(底 / 字没改)→ 换中性底字与对比度,强调色与半透明侧栏不动', async () => {
    const legacy = (accent: string, background: string, foreground: string, contrast: number): ThemePalette => ({
      accent,
      background,
      foreground,
      translucentSidebar: true,
      contrast,
    });
    globalThis.localStorage?.setItem(
      'forge:appearance',
      JSON.stringify({
        ...defaultAppearance(),
        mode: 'light',
        presetId: 'codex',
        light: legacy('#339CFF', '#FFFFFF', '#1A1C1F', 45),
        dark: legacy('#339CFF', '#181818', '#FFFFFF', 70), // 对比度手调过:保留
      }),
    );
    vi.resetModules();
    const mod = await import('@/lib/themeStore');
    const s = mod.useThemeStore.getState();
    expect(s.presetId).toBe('codex');
    expect(s.light).toEqual(legacy('#339CFF', '#FFFFFF', '#0A0A0A', 55));
    expect(s.dark).toEqual(legacy('#339CFF', '#0A0A0A', '#FAFAFA', 70));
  });

  it('纯色波迁移:手改过底色的一侧原样保留;云同步下发的旧档同样迁移', () => {
    const gruvLight: ThemePalette = { accent: '#B57614', background: '#FBF1C7', foreground: '#3C3836', translucentSidebar: true, contrast: 44 };
    const gruvDark: ThemePalette = { accent: '#FABD2F', background: '#282828', foreground: '#EBDBB2', translucentSidebar: true, contrast: 55 };
    const custom = migrateSurfaces({ ...defaultAppearance(), presetId: 'gruvbox', light: { ...gruvLight, background: '#F0E6D2' }, dark: gruvDark });
    expect(custom.light.background).toBe('#F0E6D2'); // 自定义底:不碰
    expect(custom.light.foreground).toBe('#3C3836');
    expect(custom.dark).toMatchObject({ background: '#0A0A0A', foreground: '#FAFAFA', contrast: 60, accent: '#FABD2F' });

    useThemeStore.getState().replaceAppearance({ ...defaultAppearance(), mode: 'light', presetId: 'gruvbox', light: gruvLight, dark: gruvDark });
    const s = useThemeStore.getState();
    expect(s.light).toMatchObject({ accent: '#B57614', background: '#FFFFFF', foreground: '#0A0A0A', contrast: 55 });
    expect(document.documentElement.style.getPropertyValue('--bg')).toBe('#FFFFFF');
    expect(document.documentElement.style.getPropertyValue('--text')).toBe('#0A0A0A');
  });
});
