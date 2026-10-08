import { apiGet, apiPost, getCodexAccount, getCodexModels, getCodexRateLimits, getCodexStatus } from './forgeApi';
import type { OfficialChannel } from './officialAuth';
import { codexQuotaWindows, type QuotaWindow } from './codexQuota';

export { codexQuotaWindows, type QuotaWindow } from './codexQuota';
export interface ChannelStatus {
  id: OfficialChannel;
  installed: boolean;
  configured: boolean;
  authMode?: string | null;
  modelId: string;
  model: string;
  models?: { id: string; label: string }[];
  account?: { email?: string | null; name?: string | null; planType?: string | null } | null;
  quota: { state: 'available' | 'unknown' | 'unavailable'; windows: QuotaWindow[]; updatedAt?: string | number; stale?: boolean; error?: string };
  quotaUrl?: string;
  install?: { running: boolean; error?: string | null };
  login?: { state: string; authUrl?: string; userCode?: string; loginId?: string; error?: string };
  error?: string | null;
  cloudMode?: boolean;
}

export const channelDefaults: Record<OfficialChannel, ChannelStatus> = {
  codex: { id: 'codex', installed: false, configured: false, modelId: '', model: 'Codex', quota: { state: 'unknown', windows: [] } },
  antigravity: { id: 'antigravity', installed: true, configured: false, modelId: 'antigravity', model: 'Google AI 订阅模型', quota: { state: 'unknown', windows: [] }, quotaUrl: 'https://antigravity.google/docs/plans/' },
  kimi: { id: 'kimi', installed: false, configured: false, modelId: 'kimi-code', model: 'Kimi Code', quota: { state: 'unknown', windows: [] } },
  glm: { id: 'glm', installed: true, configured: false, modelId: 'glm-coding', model: 'glm-5.3', quota: { state: 'unknown', windows: [] }, quotaUrl: 'https://bigmodel.cn/coding-plan/personal/overview' },
};

export async function codexChannelStatus(): Promise<ChannelStatus> {
  const status = await getCodexStatus();
  const cloudMode = status.authSource === 'cloud';
  let account = status.account;
  let models = status.models;
  if (status.installed && !cloudMode) {
    const current = await getCodexAccount().catch(() => null);
    account = { ...account, ...(current ?? {}) };
    if (account.authMode) {
      const [limits, catalog] = await Promise.all([
        getCodexRateLimits().catch(() => null), getCodexModels().catch(() => null),
      ]);
      account = { ...account, rateLimits: limits?.rateLimits ?? account.rateLimits };
      models = catalog?.models ?? models;
    } else {
      account = { ...account, rateLimits: null };
    }
  }
  const choices = (models ?? []).flatMap((item) => {
    const record = item as unknown as Record<string, unknown>;
    const id = record.id ?? record.model;
    return typeof id === 'string' ? [{ id: id.startsWith('codex:') ? id : `codex:${id}`, label: String(record.displayName ?? record.label ?? id) }] : [];
  });
  const windows = cloudMode ? [] : codexQuotaWindows(account.rateLimits, account.planType);
  return { ...channelDefaults.codex, installed: status.installed, cloudMode,
    configured: !cloudMode && !!account.authMode, authMode: cloudMode ? null : account.authMode,
    modelId: choices[0]?.id ?? '', models: cloudMode ? [] : choices,
    account: cloudMode ? null : account, install: status.install,
    error: account.lastError ?? status.install?.error,
    quota: { state: windows.length ? 'available' : 'unknown', windows } };
}

export async function getOfficialChannels(refresh = false) {
  return apiGet<{ channels: ChannelStatus[] }>(`/api/forge/channels${refresh ? '?refresh=true' : ''}`);
}
export async function getOfficialChannel(id: 'antigravity' | 'kimi' | 'glm', refresh = false) {
  return apiGet<ChannelStatus>(`/api/forge/channels/${id}${refresh ? '?refresh=true' : ''}`);
}
export async function loginOfficialChannel(id: 'antigravity' | 'kimi' | 'glm') {
  return apiPost<NonNullable<ChannelStatus['login']>>(`/api/forge/channels/${id}/login`, {});
}
export async function cancelOfficialLogin(id: 'antigravity' | 'kimi' | 'glm') {
  return apiPost(`/api/forge/channels/${id}/login/cancel`, {});
}
export async function logoutOfficialChannel(id: 'antigravity' | 'kimi' | 'glm') {
  return apiPost(`/api/forge/channels/${id}/logout`, {});
}
export async function bindGlmKey(apiKey: string, model: string) {
  return apiPost<ChannelStatus>('/api/forge/channels/glm/config', { apiKey, model });
}
