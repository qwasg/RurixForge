import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { adminApi } from './api/admin';
import { authApi } from './api/auth';
import type { Settings } from './api/types';
import { formatMoney } from './format';

export const DEFAULT_SITE_NAME = 'RurixForge Cloud';
export const DEFAULT_CURRENCY = 'USD';

interface SettingsContextValue {
  /** 管理端完整设置；拉取失败时为 null（此时 siteName/currency 来自公开的 /auth/config 或默认值）。 */
  settings: Settings | null;
  siteName: string;
  currency: string;
  loading: boolean;
  reload(): Promise<void>;
  /** 保存设置后直接替换，免一次往返。 */
  replace(next: Settings): void;
}

const SettingsContext = createContext<SettingsContextValue>({
  settings: null,
  siteName: DEFAULT_SITE_NAME,
  currency: DEFAULT_CURRENCY,
  loading: false,
  reload: async () => undefined,
  replace: () => undefined,
});

/** 登录后挂载：拉一次系统设置，全局提供站点名与展示货币。 */
export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [fallback, setFallback] = useState<{ siteName: string; currency: string } | null>(null);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    try {
      setSettings(await adminApi.settings.get());
    } catch {
      try {
        const cfg = await authApi.config();
        setFallback({ siteName: cfg.siteName, currency: cfg.currency });
      } catch {
        // 两处都失败：用默认值
      }
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const siteName = settings?.siteName || fallback?.siteName || DEFAULT_SITE_NAME;
  const currency = settings?.currency || fallback?.currency || DEFAULT_CURRENCY;

  useEffect(() => {
    document.title = `管理后台 · ${siteName}`;
  }, [siteName]);

  const value = useMemo<SettingsContextValue>(
    () => ({ settings, siteName, currency, loading, reload, replace: setSettings }),
    [settings, siteName, currency, loading, reload],
  );
  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

export function useSettings(): SettingsContextValue {
  return useContext(SettingsContext);
}

/** 按系统设置的货币格式化 micros。 */
export function useMoney(): (micros: number | null | undefined) => string {
  const { currency } = useSettings();
  return useCallback((micros: number | null | undefined) => formatMoney(micros, currency), [currency]);
}
