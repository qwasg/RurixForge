import { getCloudSettings, putCloudSetting } from './accountApi';
import { useAccountStore } from './accountStore';
import { useSettingsStore } from './settingsStore';
import {
  defaultAppearance,
  presetById,
  useThemeStore,
  type AppearanceSettings,
  type ThemePalette,
} from './themeStore';

/**
 * 设置随账号同步(15 §3.4 / §8.2):只同步 `appearance`(整份外观)与 `composer`(发送快捷键)。
 * 设置页当前页、画布视口等设备本地项不上云。
 *
 * 防回声:应用云端值期间 `applyingRemote` 屏蔽本地订阅;另记每个命名空间最近一次与云端一致的
 * 序列化值,本地值回到该值时不推送(包括远端下发触发的 store 变更)。
 */

export type SyncedNamespace = 'appearance' | 'composer';

export const SETTINGS_PUSH_DEBOUNCE_MS = 1000;

const NAMESPACES: SyncedNamespace[] = ['appearance', 'composer'];

let applyingRemote = false;
const lastSynced: Partial<Record<SyncedNamespace, string>> = {};
const timers: Partial<Record<SyncedNamespace, ReturnType<typeof setTimeout>>> = {};

export function pickAppearance(s: AppearanceSettings): AppearanceSettings {
  return {
    mode: s.mode,
    presetId: s.presetId,
    light: { ...s.light },
    dark: { ...s.dark },
    uiFont: s.uiFont,
    codeFont: s.codeFont,
    uiSize: s.uiSize,
    codeSize: s.codeSize,
    diffMarkers: s.diffMarkers,
  };
}

function normalizePalette(base: ThemePalette, raw: unknown): ThemePalette {
  const p = (raw && typeof raw === 'object' ? raw : {}) as Partial<ThemePalette>;
  return {
    accent: typeof p.accent === 'string' ? p.accent : base.accent,
    background: typeof p.background === 'string' ? p.background : base.background,
    foreground: typeof p.foreground === 'string' ? p.foreground : base.foreground,
    translucentSidebar:
      typeof p.translucentSidebar === 'boolean' ? p.translucentSidebar : base.translucentSidebar,
    contrast: typeof p.contrast === 'number' && Number.isFinite(p.contrast) ? p.contrast : base.contrast,
  };
}

/** 云端外观值 → 本地完整设置(缺项按默认补齐,类型不符的字段丢弃);非对象 → null。 */
export function normalizeAppearance(raw: unknown): AppearanceSettings | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;
  const r = raw as Partial<AppearanceSettings>;
  const d = defaultAppearance();
  return {
    mode: r.mode === 'light' || r.mode === 'dark' || r.mode === 'auto' ? r.mode : d.mode,
    presetId: typeof r.presetId === 'string' && presetById(r.presetId) ? r.presetId : d.presetId,
    light: normalizePalette(d.light, r.light),
    dark: normalizePalette(d.dark, r.dark),
    uiFont: typeof r.uiFont === 'string' ? r.uiFont : d.uiFont,
    codeFont: typeof r.codeFont === 'string' ? r.codeFont : d.codeFont,
    uiSize: typeof r.uiSize === 'number' && Number.isFinite(r.uiSize) ? r.uiSize : d.uiSize,
    codeSize: typeof r.codeSize === 'number' && Number.isFinite(r.codeSize) ? r.codeSize : d.codeSize,
    diffMarkers: r.diffMarkers === 'plusminus' ? 'plusminus' : 'color',
  };
}

function localValue(ns: SyncedNamespace): unknown {
  if (ns === 'appearance') return pickAppearance(useThemeStore.getState());
  return { submitCtrlEnter: useSettingsStore.getState().submitCtrlEnter };
}

function serialize(ns: SyncedNamespace): string {
  return JSON.stringify(localValue(ns));
}

function syncAllowed(): boolean {
  const st = useAccountStore.getState().status;
  return st?.loggedIn === true && st.sync.settings !== false;
}

function cancelPush(ns: SyncedNamespace): void {
  const t = timers[ns];
  if (t !== undefined) clearTimeout(t);
  delete timers[ns];
}

async function pushNow(ns: SyncedNamespace): Promise<void> {
  if (!syncAllowed()) return;
  const value = localValue(ns);
  const json = JSON.stringify(value);
  if (json === lastSynced[ns]) return;
  try {
    await putCloudSetting(ns, value);
    lastSynced[ns] = json;
  } catch {
    // agentd 离线缓存 + 补推由后端承担;这里失败只意味着下次改动再推
  }
}

function schedulePush(ns: SyncedNamespace): void {
  cancelPush(ns);
  timers[ns] = setTimeout(() => {
    delete timers[ns];
    void pushNow(ns);
  }, SETTINGS_PUSH_DEBOUNCE_MS);
}

function onLocalChange(ns: SyncedNamespace): void {
  if (applyingRemote || !syncAllowed()) return;
  if (serialize(ns) === lastSynced[ns]) {
    cancelPush(ns);
    return;
  }
  schedulePush(ns);
}

/** 应用一条云端值;返回是否被接受(形状不符的值忽略)。 */
export function applyRemoteSetting(ns: SyncedNamespace, value: unknown): boolean {
  if (ns === 'appearance') {
    const next = normalizeAppearance(value);
    if (!next) return false;
    applyingRemote = true;
    try {
      if (JSON.stringify(pickAppearance(useThemeStore.getState())) !== JSON.stringify(next)) {
        useThemeStore.getState().replaceAppearance(next);
      }
    } finally {
      applyingRemote = false;
    }
  } else {
    const v = value as { submitCtrlEnter?: unknown } | null;
    if (!v || typeof v !== 'object' || typeof v.submitCtrlEnter !== 'boolean') return false;
    applyingRemote = true;
    try {
      if (useSettingsStore.getState().submitCtrlEnter !== v.submitCtrlEnter) {
        useSettingsStore.getState().setSubmitCtrlEnter(v.submitCtrlEnter);
      }
    } finally {
      applyingRemote = false;
    }
  }
  cancelPush(ns);
  lastSynced[ns] = serialize(ns);
  return true;
}

/**
 * 登录后 / 已登录启动时拉云端设置并应用。云端还没有的命名空间用本机当前值补种
 * (仅 source=cloud;离线缓存态不补种,免得旧缓存盖掉云端)。
 */
export async function pullCloudSettings(): Promise<boolean> {
  if (!syncAllowed()) return false;
  let res: Awaited<ReturnType<typeof getCloudSettings>>;
  try {
    res = await getCloudSettings();
  } catch {
    return false;
  }
  const items = res && typeof res.items === 'object' && res.items !== null ? res.items : {};
  const seed: SyncedNamespace[] = [];
  for (const ns of NAMESPACES) {
    const entry = items[ns];
    if (entry && typeof entry === 'object') applyRemoteSetting(ns, entry.value);
    else if (res?.source !== 'cache') seed.push(ns);
  }
  await Promise.all(seed.map((ns) => pushNow(ns)));
  return true;
}

function appearanceChanged(a: AppearanceSettings, b: AppearanceSettings): boolean {
  return (
    a.mode !== b.mode ||
    a.presetId !== b.presetId ||
    a.light !== b.light ||
    a.dark !== b.dark ||
    a.uiFont !== b.uiFont ||
    a.codeFont !== b.codeFont ||
    a.uiSize !== b.uiSize ||
    a.codeSize !== b.codeSize ||
    a.diffMarkers !== b.diffMarkers
  );
}

/** 订阅本地两类设置的改动(防抖 1s 推送);返回退订函数。 */
export function startSettingsSync(): () => void {
  const offTheme = useThemeStore.subscribe((st, prev) => {
    if (appearanceChanged(st, prev)) onLocalChange('appearance');
  });
  const offSettings = useSettingsStore.subscribe((st, prev) => {
    if (st.submitCtrlEnter !== prev.submitCtrlEnter) onLocalChange('composer');
  });
  return () => {
    offTheme();
    offSettings();
    for (const ns of NAMESPACES) cancelPush(ns);
  };
}

/** 测试复位:清掉防抖计时器与「已同步」基线。 */
export function resetSettingsSync(): void {
  for (const ns of NAMESPACES) {
    cancelPush(ns);
    delete lastSynced[ns];
  }
  applyingRemote = false;
}
