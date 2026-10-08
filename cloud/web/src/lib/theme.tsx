import { Monitor, Moon, Sun } from 'lucide-react';
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';

export type ThemeMode = 'system' | 'light' | 'dark';

const THEME_KEY = 'forge-admin.theme';
const ORDER: ThemeMode[] = ['system', 'light', 'dark'];

export const THEME_LABEL: Record<ThemeMode, string> = { system: '跟随系统', light: '浅色', dark: '深色' };
export const THEME_ICON = { system: Monitor, light: Sun, dark: Moon } as const;

export function readThemeMode(): ThemeMode {
  try {
    const v = window.localStorage.getItem(THEME_KEY);
    if (v === 'light' || v === 'dark' || v === 'system') return v;
  } catch {
    // 默认跟随系统
  }
  return 'system';
}

/** system：<html> 不带类名，由 prefers-color-scheme 决定（见 index.css）。 */
export function applyThemeMode(mode: ThemeMode): void {
  const root = document.documentElement;
  root.classList.remove('light', 'dark');
  if (mode !== 'system') root.classList.add(mode);
}

interface ThemeContextValue {
  mode: ThemeMode;
  setMode(mode: ThemeMode): void;
  cycle(): void;
}

const ThemeContext = createContext<ThemeContextValue>({ mode: 'system', setMode: () => undefined, cycle: () => undefined });

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<ThemeMode>(readThemeMode);

  useEffect(() => {
    applyThemeMode(mode);
    try {
      window.localStorage.setItem(THEME_KEY, mode);
    } catch {
      // 忽略
    }
  }, [mode]);

  const cycle = useCallback(() => {
    setMode((m) => ORDER[(ORDER.indexOf(m) + 1) % ORDER.length] ?? 'system');
  }, []);

  const value = useMemo(() => ({ mode, setMode, cycle }), [mode, cycle]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  return useContext(ThemeContext);
}
