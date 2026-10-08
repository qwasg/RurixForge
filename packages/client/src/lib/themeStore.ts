import { create } from 'zustand';

/**
 * F7 wave.3 主题系统:逐行移植参考仓 appearance.rs(Moonlit Agent IDE)。
 * - applyPalette:contrast 混色基比 0.42/0.62/0.78(可读性波已收深,见下)、accent_bg/ring
 *   alpha 明暗差异、translucent 双 alpha(0xCC/0xEE)——与参考 rust 源码逐行对应。
 * - 持久化键 forge:appearance(参考 moonlit:appearance,语义照搬,前缀改 forge:)。
 * - 应用 = documentElement data-theme + 注入 CSS 变量 + 字体栈/字号变量。
 *
 * 差异留痕:参考 moonlit-uikit crate(Tokens::claude_light/dark 的 sage/danger/warn/info
 * 与 dot_done/idle/blocked/queued 具体色值)未随拷贝仓提供(path dep 缺失),语义色为本仓
 * 派生值(见 theme.css 注释);apply_palette 派生面(本模块)与参考逐值一致。
 *
 * 主题净化波(2026-08-31 用户拍板:默认暖奶油显脏、字不清楚):
 * - 新增 forge 预设(中性净底 + 精炼锻橙)并取代 moonlit 成为默认;moonlit 原值保留可选。
 * - applyPalette 两处泛化:亮色 hover/active 由硬编码 #2A27240A/12 改为 fg+0x0A/0x12
 *   (对 moonlit 逐值等价,fg=#2A2724);亮色 text_inv 由 #FAF9F5 改为 #FFFFFF。
 * - 语义色 SEMANTIC_LIGHT/DARK 全局换干净同族(对所有预设生效,差异如实留档)。
 * - loadSettings 一次性迁移:presetId=moonlit 且双表未自定义 → 自动切 forge。
 *
 * 可读性波(2026-10-07 用户反馈:淡灰字、甚至黑字都太细看不清):
 * - text_2/3/4 混色基比由参考 0.42/0.62/0.78 收到 0.33/0.46/0.64(对所有预设与用户自定义
 *   双表生效;contrast 滑杆语义不变)。forge 亮表对白底对比度 5.3/3.0/2.0 → 6.9/4.6/2.8:1,
 *   text_3 过 WCAG AA。字重侧的补偿见 index.css .forge-legible。
 */

// ---------- 十六进制颜色工具(对应 appearance.rs parse_hex_rgb/mix_hex/darken_hex/lighten_hex) ----------

export function parseHexRgb(hexStr: string): [number, number, number] {
  const t = hexStr.trim().replace(/^#/, '');
  const n = Number.parseInt(t, 16);
  if (Number.isNaN(n)) return [0xff, 0xff, 0xff];
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

/** mix_hex:在 a↔b 间按 t 混色,t clamp [0,1];输出大写 #RRGGBB。
 * 逐字对齐参考:rust `f32 as u8` 为截断(非四舍五入),故用 Math.trunc。 */
export function mixHex(a: string, b: string, t: number): string {
  const [ar, ag, ab] = parseHexRgb(a);
  const [br, bg, bb] = parseHexRgb(b);
  const tt = Math.min(1, Math.max(0, t));
  const r = Math.trunc(ar * (1 - tt) + br * tt);
  const g = Math.trunc(ag * (1 - tt) + bg * tt);
  const bl = Math.trunc(ab * (1 - tt) + bb * tt);
  const hex = (v: number) => v.toString(16).padStart(2, '0').toUpperCase();
  return `#${hex(r)}${hex(g)}${hex(bl)}`;
}

export function darkenHex(hexStr: string, amount: number): string {
  return mixHex(hexStr, '#000000', amount);
}

export function lightenHex(hexStr: string, amount: number): string {
  return mixHex(hexStr, '#FFFFFF', amount);
}

/** 参考 normalize_hex:仅接受 6 位 hex,规范化大写带 # */
export function normalizeHex(raw: string): string | null {
  const t = raw.trim().replace(/^#/, '');
  if (t.length !== 6 || !/^[0-9a-fA-F]{6}$/.test(t)) return null;
  return `#${t.toUpperCase()}`;
}

/** 参考 rgba_from_hex:#RRGGBB + alpha 字节 → #RRGGBBAA CSS 串 */
export function hexWithAlpha(hexStr: string, alphaByte: number): string {
  const [r, g, b] = parseHexRgb(hexStr);
  const hex = (v: number) => v.toString(16).padStart(2, '0').toUpperCase();
  return `#${hex(r)}${hex(g)}${hex(b)}${hex(alphaByte)}`;
}

// ---------- 设置模型(对应 AppearanceSettings/ThemePalette/ThemeModeChoice) ----------

export type ThemeMode = 'auto' | 'light' | 'dark';
export type DiffMarkers = 'color' | 'plusminus';

export interface ThemePalette {
  accent: string;
  background: string;
  foreground: string;
  translucentSidebar: boolean;
  /** 0–100;对比度越高 text_2/3/4 混得越少(越靠近 fg) */
  contrast: number;
}

export interface AppearanceSettings {
  mode: ThemeMode;
  presetId: string;
  light: ThemePalette;
  dark: ThemePalette;
  uiFont: string;
  codeFont: string;
  /** 11–18,默认 13 */
  uiSize: number;
  /** 10–20,默认 12 */
  codeSize: number;
  diffMarkers: DiffMarkers;
}

// ---------- 主题预设(参考 theme_presets() 逐值;forge 为净化波新增默认) ----------

export interface ThemePreset {
  id: string;
  name: string;
  swatch: string;
  light: ThemePalette;
  dark: ThemePalette;
}

function palette(
  accent: string,
  background: string,
  foreground: string,
  translucentSidebar: boolean,
  contrast: number,
): ThemePalette {
  return { accent, background, foreground, translucentSidebar, contrast };
}

export const MOONLIT_LIGHT: ThemePalette = palette('#C96442', '#FAF9F5', '#2A2724', false, 45);
export const MOONLIT_DARK: ThemePalette = palette('#E2886A', '#1C1B18', '#ECE8DF', false, 60);

/** 主题净化波默认预设:中性净底(去黄去暖)+ 精炼锻橙 accent;contrast 55/60 抬高二级文字对比 */
export const FORGE_LIGHT: ThemePalette = palette('#C94F12', '#FFFFFF', '#1B1D21', false, 55);
export const FORGE_DARK: ThemePalette = palette('#F47B33', '#191A1D', '#E8E9EC', false, 60);

export const THEME_PRESETS: ThemePreset[] = [
  { id: 'forge', name: 'Forge', swatch: '#C94F12', light: FORGE_LIGHT, dark: FORGE_DARK },
  { id: 'moonlit', name: 'Moonlit', swatch: '#C96442', light: MOONLIT_LIGHT, dark: MOONLIT_DARK },
  {
    id: 'absolutely', name: 'Absolutely', swatch: '#FF6B35',
    light: palette('#FF6B35', '#FFFBF7', '#1A1410', true, 45),
    dark: palette('#FF8C5A', '#141210', '#F5EDE6', true, 58),
  },
  {
    id: 'catppuccin', name: 'Catppuccin', swatch: '#CBA6F7',
    light: palette('#8839EF', '#EFF1F5', '#4C4F69', true, 48),
    dark: palette('#CBA6F7', '#1E1E2E', '#CDD6F4', true, 62),
  },
  {
    id: 'codex', name: 'Codex', swatch: '#339CFF',
    light: palette('#339CFF', '#FFFFFF', '#1A1C1F', true, 45),
    dark: palette('#339CFF', '#181818', '#FFFFFF', true, 60),
  },
  {
    id: 'everforest', name: 'Everforest', swatch: '#A7C080',
    light: palette('#5C7A3A', '#EFEEE6', '#2B3339', true, 46),
    dark: palette('#A7C080', '#2D353B', '#D3C6AA', true, 60),
  },
  {
    id: 'github', name: 'GitHub', swatch: '#0969DA',
    light: palette('#0969DA', '#FFFFFF', '#1F2328', true, 45),
    dark: palette('#4493F8', '#0D1117', '#E6EDF3', true, 58),
  },
  {
    id: 'gruvbox', name: 'Gruvbox', swatch: '#D79921',
    light: palette('#B57614', '#FBF1C7', '#3C3836', true, 44),
    dark: palette('#FABD2F', '#282828', '#EBDBB2', true, 55),
  },
  {
    id: 'linear', name: 'Linear', swatch: '#5E6AD2',
    light: palette('#5E6AD2', '#FFFFFF', '#17181C', true, 47),
    dark: palette('#828FFF', '#121214', '#EEEFFC', true, 61),
  },
  {
    id: 'notion', name: 'Notion', swatch: '#2F80ED',
    light: palette('#2F80ED', '#FFFFFF', '#37352F', true, 43),
    dark: palette('#5294E2', '#191919', '#FFFFFF', true, 57),
  },
  {
    id: 'one', name: 'One', swatch: '#4078F2',
    light: palette('#4078F2', '#FAFAFA', '#383A42', true, 45),
    dark: palette('#61AFEF', '#282C34', '#ABB2BF', true, 59),
  },
];

export function presetById(id: string): ThemePreset | undefined {
  return THEME_PRESETS.find((p) => p.id === id);
}

// ---------- applyPalette 逐行移植(参考 appearance.rs apply_palette) ----------
// 输出 = 语义 token → CSS 值映射;语义色(sage/danger/warn/info/dot_*)来自静态基表
// (参考 uikit Tokens 默认值不在拷贝仓内,本仓派生,见文件头留痕)。

/** 派生 token 全集的键(CSS 变量 --<key>) */
export interface ThemeTokens {
  [key: string]: string;
}

/**
 * 语义基色(参考 uikit Tokens::claude_light/dark 中 apply_palette 不覆盖的部分;
 * uikit 源不在拷贝仓 → 本仓派生值,与奶油底/陶土 accent 同族,差异已留档)。
 */
const SEMANTIC_LIGHT: Record<string, string> = {
  sage: '#1D7A46',
  'sage-bg': 'rgba(29,122,70,0.12)',
  danger: '#CE3A1E',
  'danger-bg': 'rgba(206,58,30,0.10)',
  warn: '#9A6A0A',
  'warn-bg': 'rgba(154,106,10,0.12)',
  info: '#2563EB',
  'info-bg': 'rgba(37,99,235,0.10)',
};
const SEMANTIC_DARK: Record<string, string> = {
  sage: '#7BC996',
  'sage-bg': 'rgba(123,201,150,0.16)',
  danger: '#F0755A',
  'danger-bg': 'rgba(240,117,90,0.16)',
  warn: '#E0B45C',
  'warn-bg': 'rgba(224,180,92,0.16)',
  info: '#7AA8F0',
  'info-bg': 'rgba(122,168,240,0.16)',
};

export function applyPalette(p: ThemePalette, dark: boolean): ThemeTokens {
  const accent = normalizeHex(p.accent) ?? '#C96442';
  const bg = normalizeHex(p.background) ?? (dark ? '#1C1B18' : '#FAF9F5');
  const fg = normalizeHex(p.foreground) ?? (dark ? '#ECE8DF' : '#2A2724');

  // sunk / panel(参考:bg_sunk 压暗 0.04|0.06;panel 亮=bg、暗提亮 0.04)
  const sunk = dark ? darkenHex(bg, 0.06) : darkenHex(bg, 0.04);
  const panel = dark ? lightenHex(bg, 0.04) : bg;

  // contrast 混色(对比度越高 t 越小 → 越靠近 fg)。可读性波:基比由参考 0.42/0.62/0.78
  // 收到 0.33/0.46/0.64,次级/提示文字整体加深(见文件头)
  const contrast = Math.min(100, Math.max(0, p.contrast)) / 100;
  const text2 = mixHex(fg, bg, 0.33 * (1.1 - contrast * 0.5));
  const text3 = mixHex(fg, bg, 0.46 * (1.05 - contrast * 0.35));
  const text4 = mixHex(fg, bg, 0.64 * (1.02 - contrast * 0.25));

  // accent 派生(参考 accent_derivatives:soft 亮压暗 0.08/暗提亮 0.15;
  // bg alpha 亮 0x14/暗 0x24;ring alpha 亮 0x47/暗 0x6B)
  const accentSoft = dark ? lightenHex(accent, 0.15) : darkenHex(accent, 0.08);
  const accentBg = hexWithAlpha(accent, dark ? 0x24 : 0x14);
  const accentRing = hexWithAlpha(accent, dark ? 0x6b : 0x47);

  // 线与 hover/active(参考:hover/active 硬编码——亮 rgba(42,39,36,0.04/0.07)、
  // 暗 #FFFFFF alpha 0x0d/0x14。净化波泛化:亮色改 fg+0x0A/0x12,对 moonlit 逐值等价)
  const line = hexWithAlpha(fg, dark ? 0x12 : 0x17);
  const lineStrong = hexWithAlpha(fg, dark ? 0x21 : 0x24);
  const hover = dark ? '#FFFFFF0D' : hexWithAlpha(fg, 0x0a);
  const active = dark ? '#FFFFFF14' : hexWithAlpha(fg, 0x12);

  // translucent 双 alpha(参考:sidebar=sunk+0xCC、float=panel+0xEE;否则实体)
  const sidebar = p.translucentSidebar ? hexWithAlpha(sunk, 0xcc) : sunk;
  const floatBg = p.translucentSidebar ? hexWithAlpha(panel, 0xee) : panel;

  const semantic = dark ? SEMANTIC_DARK : SEMANTIC_LIGHT;
  return {
    bg,
    'bg-sunk': sunk,
    'bg-panel': panel,
    'bg-input': panel,
    'bg-float': floatBg,
    'bg-sidebar': sidebar,
    'bg-hover': hover,
    'bg-active': active,
    'bg-selection': accentBg,
    text: fg,
    'text-2': text2,
    'text-3': text3,
    'text-4': text4,
    'text-inv': dark ? bg : '#FFFFFF', // 净化波:亮反色文字奶油 #FAF9F5 → 纯白
    line,
    'line-strong': lineStrong,
    accent,
    'accent-soft': accentSoft,
    'accent-bg': accentBg,
    'accent-ring': accentRing,
    ...semantic,
    'dot-running': accent,
    'dot-done': semantic.sage,
    'dot-idle': text4,
    'dot-blocked': semantic.danger,
    'dot-queued': semantic.warn,
  };
}

// ---------- 默认设置(净化波:forge 双表取代参考 moonlit,mode=auto,13/12) ----------

export function defaultAppearance(): AppearanceSettings {
  return {
    mode: 'auto',
    presetId: 'forge',
    light: { ...FORGE_LIGHT },
    dark: { ...FORGE_DARK },
    uiFont: '',
    codeFont: '',
    uiSize: 13,
    codeSize: 12,
    diffMarkers: 'color',
  };
}

const STORE_KEY = 'forge:appearance';

function clampSettings(s: AppearanceSettings): AppearanceSettings {
  s.uiSize = Math.min(18, Math.max(11, Math.round(s.uiSize)));
  s.codeSize = Math.min(20, Math.max(10, Math.round(s.codeSize)));
  s.light.contrast = Math.min(100, Math.max(0, Math.round(s.light.contrast)));
  s.dark.contrast = Math.min(100, Math.max(0, Math.round(s.dark.contrast)));
  if (!presetById(s.presetId)) s.presetId = 'forge';
  return s;
}

function paletteEqual(a: ThemePalette, b: ThemePalette): boolean {
  return (
    a.accent === b.accent &&
    a.background === b.background &&
    a.foreground === b.foreground &&
    a.translucentSidebar === b.translucentSidebar &&
    a.contrast === b.contrast
  );
}

function loadSettings(): AppearanceSettings {
  try {
    const raw = globalThis.localStorage?.getItem(STORE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as AppearanceSettings;
      const d = defaultAppearance();
      const merged: AppearanceSettings = {
        ...d,
        ...parsed,
        light: { ...d.light, ...parsed.light },
        dark: { ...d.dark, ...parsed.dark },
      };
      // 净化波一次性迁移:moonlit 预设且双表仍是旧默认(从未自定义配色)→ 切 forge
      if (
        merged.presetId === 'moonlit' &&
        paletteEqual(merged.light, MOONLIT_LIGHT) &&
        paletteEqual(merged.dark, MOONLIT_DARK)
      ) {
        merged.presetId = 'forge';
        merged.light = { ...FORGE_LIGHT };
        merged.dark = { ...FORGE_DARK };
      }
      return clampSettings(merged);
    }
  } catch {
    // 损坏持久化 → 默认
  }
  return defaultAppearance();
}

// ---------- 系统明暗侦听(mode=auto) ----------

function systemDark(): boolean {
  try {
    return typeof globalThis.matchMedia === 'function'
      ? globalThis.matchMedia('(prefers-color-scheme: dark)').matches
      : false;
  } catch {
    return false;
  }
}

export function effectiveDark(mode: ThemeMode, sysDark: boolean): boolean {
  if (mode === 'light') return false;
  if (mode === 'dark') return true;
  return sysDark;
}

// ---------- 字体栈(净化波:Inter Variable 包接管西文/数字,中文仍系统栈;代码字体已有包) ----------

export const FONT_SANS_STACK =
  '"Inter Variable","HarmonyOS Sans SC","Microsoft YaHei UI","PingFang SC","Noto Sans SC",system-ui,-apple-system,"Segoe UI",sans-serif';
// D-046:衬线首选打包的思源宋体可变字体(main.tsx 导入),与 theme.css --font-serif 同步
export const FONT_SERIF_STACK = '"Noto Serif SC Variable","Noto Serif SC","SimSun",serif';
export const FONT_MONO_STACK = '"JetBrains Mono Variable","JetBrains Mono",Consolas,monospace';

// ---------- zustand store ----------

interface ThemeState extends AppearanceSettings {
  /** 当前生效暗态(mode 解析后;与 AppearanceSettings.dark 配色表区分) */
  isDark: boolean;
  setMode: (m: ThemeMode) => void;
  toggleMode: () => void;
  applyPreset: (presetId: string) => void;
  setPaletteValue: (side: 'light' | 'dark', key: keyof ThemePalette, value: string | number | boolean) => void;
  setUiSize: (n: number) => void;
  setCodeSize: (n: number) => void;
  setUiFont: (f: string) => void;
  setCodeFont: (f: string) => void;
  setDiffMarkers: (d: DiffMarkers) => void;
  /** 云同步下发:整份外观设置一次替换(范围按本地规则夹取) */
  replaceAppearance: (s: AppearanceSettings) => void;
  /** mode=auto 时系统明暗翻转重应用 */
  syncSystemDark: () => void;
}

let mediaHooked = false;

/** 将设置应用到 DOM:data-theme + CSS 变量注入(即改即存 localStorage) */
function applyToDom(s: AppearanceSettings, dark: boolean): void {
  const doc = globalThis.document?.documentElement;
  if (!doc) return;
  doc.dataset.theme = dark ? 'dark' : 'light';
  const tokens = applyPalette(dark ? s.dark : s.light, dark);
  for (const [k, v] of Object.entries(tokens)) {
    doc.style.setProperty(`--${k}`, v);
  }
  doc.style.setProperty('--font-sans', s.uiFont.trim() !== '' ? s.uiFont.trim() : FONT_SANS_STACK);
  doc.style.setProperty('--font-serif', FONT_SERIF_STACK);
  doc.style.setProperty('--font-mono', s.codeFont.trim() !== '' ? s.codeFont.trim() : FONT_MONO_STACK);
  doc.style.setProperty('--ui-size', `${s.uiSize}px`);
  doc.style.setProperty('--code-size', `${s.codeSize}px`);
}

function persist(s: AppearanceSettings): void {
  try {
    globalThis.localStorage?.setItem(STORE_KEY, JSON.stringify(s));
  } catch {
    // 隐私模式等写不进,静默
  }
}

function pickSettings(s: ThemeState): AppearanceSettings {
  return {
    mode: s.mode,
    presetId: s.presetId,
    light: s.light,
    dark: s.dark,
    uiFont: s.uiFont,
    codeFont: s.codeFont,
    uiSize: s.uiSize,
    codeSize: s.codeSize,
    diffMarkers: s.diffMarkers,
  };
}

export const useThemeStore = create<ThemeState>((set, get) => {
  const initial = loadSettings();
  const isDark0 = effectiveDark(initial.mode, systemDark());

  const commit = (patch: Partial<AppearanceSettings>) => {
    set(patch);
    const s = pickSettings(get());
    const dark = effectiveDark(s.mode, systemDark());
    set({ isDark: dark });
    applyToDom(s, dark);
    persist(s);
  };

  return {
    ...initial,
    isDark: isDark0,

    setMode: (m) => commit({ mode: m }),
    toggleMode: () => commit({ mode: get().isDark ? 'light' : 'dark' }),
    applyPreset: (presetId) => {
      const preset = presetById(presetId);
      if (!preset) return;
      commit({
        presetId,
        light: { ...preset.light },
        dark: { ...preset.dark },
      });
    },
    setPaletteValue: (side, key, value) => {
      const cur = get()[side];
      const next: ThemePalette = { ...cur, [key]: value };
      commit({ [side]: next } as Partial<AppearanceSettings>);
    },
    setUiSize: (n) => commit({ uiSize: Math.min(18, Math.max(11, Math.round(n))) }),
    setCodeSize: (n) => commit({ codeSize: Math.min(20, Math.max(10, Math.round(n))) }),
    setUiFont: (f) => commit({ uiFont: f }),
    setCodeFont: (f) => commit({ codeFont: f }),
    setDiffMarkers: (d) => commit({ diffMarkers: d }),
    replaceAppearance: (s) =>
      commit(clampSettings({ ...s, light: { ...s.light }, dark: { ...s.dark } })),
    syncSystemDark: () => {
      const s = pickSettings(get());
      const dark = effectiveDark(s.mode, systemDark());
      if (dark !== get().isDark) {
        set({ isDark: dark });
        applyToDom(s, dark);
      }
    },
  };
});

/** 应用入口调用一次:注入首帧变量 + mode=auto 时挂系统明暗监听 */
export function initTheme(): void {
  const s = useThemeStore.getState();
  applyToDom(pickSettings(s), s.isDark);
  if (mediaHooked || typeof globalThis.matchMedia !== 'function') return;
  mediaHooked = true;
  try {
    const mq = globalThis.matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => useThemeStore.getState().syncSystemDark();
    if (typeof mq.addEventListener === 'function') mq.addEventListener('change', onChange);
    else if (typeof mq.addListener === 'function') mq.addListener(onChange);
  } catch {
    // jsdom 等环境无实现,跳过
  }
}
