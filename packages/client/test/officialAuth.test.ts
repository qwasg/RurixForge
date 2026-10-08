import { afterEach, describe, expect, it, vi } from 'vitest';
import { officialAuthUrl, reserveOfficialAuth } from '@/lib/officialAuth';
import { codexChannelStatus, codexQuotaWindows } from '@/lib/channelApi';
import * as forgeApi from '@/lib/forgeApi';

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe('official subscription authorization', () => {
  it('accepts only the selected provider HTTPS host, without credentials or custom ports', () => {
    expect(officialAuthUrl('codex', 'https://auth.openai.com/codex/device')).toBe('https://auth.openai.com/codex/device');
    expect(officialAuthUrl('kimi', 'https://auth.kimi.com/device?user_code=DEMO')).toContain('auth.kimi.com');
    expect(officialAuthUrl('glm', 'https://bigmodel.cn/coding-plan/personal/overview')).toContain('bigmodel.cn');
    expect(officialAuthUrl('antigravity', 'https://accounts.google.com/o/oauth2/v2/auth?state=google-state')).toContain('accounts.google.com');
    for (const raw of ['https://accounts.google.com.evil.test/o/oauth2/auth', 'https://accounts.google.com@evil.test/', 'https://accounts.google.com:51121/', 'http://accounts.google.com/']) expect(officialAuthUrl('antigravity', raw)).toBeNull();
    for (const raw of ['javascript:alert(1)', 'http://auth.openai.com', 'https://auth.openai.com.evil.test',
      'https://evil.test/auth.openai.com', 'https://user:pass@auth.openai.com', 'https://auth.openai.com:8443',
      'https://auth.kimi.com/device']) expect(officialAuthUrl('codex', raw)).toBeNull();
  });

  it('reserves a popup synchronously and never navigates it to an untrusted URL', async () => {
    vi.stubGlobal('forgeAPI', undefined);
    const replace = vi.fn(); const close = vi.fn();
    const popup = { closed: false, opener: {}, document: { title: '', body: { textContent: '', style: {} } }, location: { replace }, close };
    const open = vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window);
    const auth = reserveOfficialAuth('kimi');
    expect(open).toHaveBeenCalledWith('', expect.stringMatching(/^forge-kimi-login-/), expect.stringContaining('popup'));
    expect(popup.opener).toBeNull();
    await expect(auth.navigate('https://evil.test/login')).rejects.toThrow('校验失败');
    expect(replace).not.toHaveBeenCalled();
    await expect(auth.navigate('https://auth.kimi.com/device')).resolves.toBe(true);
    expect(replace).toHaveBeenCalledWith('https://auth.kimi.com/device');
    auth.close(); expect(close).toHaveBeenCalledOnce();
    reserveOfficialAuth('kimi');
    expect(open.mock.calls[0][1]).not.toBe(open.mock.calls[1][1]);
  });

  it('uses the desktop browser bridge and preserves a fallback when browser popups are blocked', async () => {
    const open = vi.spyOn(window, 'open').mockReturnValue(null);
    vi.stubGlobal('forgeAPI', { auth: { openExternal: vi.fn().mockResolvedValue(undefined) } });
    const native = reserveOfficialAuth('codex');
    expect(open).not.toHaveBeenCalled();
    await native.navigate('https://auth.openai.com/codex/device');
    expect(window.forgeAPI!.auth!.openExternal).toHaveBeenCalledWith('codex', 'https://auth.openai.com/codex/device');
    vi.stubGlobal('forgeAPI', { platform: 'win32' });
    await reserveOfficialAuth('kimi').navigate('https://auth.kimi.com/device');
    expect(open).toHaveBeenCalledWith('https://auth.kimi.com/device', '_blank', 'noopener,noreferrer');
    vi.stubGlobal('forgeAPI', undefined);
    await expect(reserveOfficialAuth('glm').navigate('https://bigmodel.cn/')).resolves.toBe(false);
  });

  it('leaves the isolated official browser page untouched when the app cancels its login request', () => {
    vi.stubGlobal('forgeAPI', undefined);
    const close = vi.fn();
    const popup = { closed: false, opener: {}, get document(): Document { throw new DOMException('cross origin', 'SecurityError'); }, close };
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window);
    const auth = reserveOfficialAuth('kimi');
    expect(() => auth.close()).not.toThrow();
    expect(close).not.toHaveBeenCalled();
  });
});

describe('official quota normalization', () => {
  it('makes login available before requesting authenticated Codex quotas and models', async () => {
    vi.spyOn(forgeApi, 'getCodexStatus').mockResolvedValue({ installed: true, authSource: 'chatgpt',
      account: { authMode: null }, install: { running: false }, models: [] } as never);
    vi.spyOn(forgeApi, 'getCodexAccount').mockResolvedValue({ ok: true, authMode: null } as never);
    const limits = vi.spyOn(forgeApi, 'getCodexRateLimits');
    const models = vi.spyOn(forgeApi, 'getCodexModels');
    const status = await codexChannelStatus();
    expect(status.installed).toBe(true);
    expect(status.configured).toBe(false);
    expect(status.quota.windows).toEqual([]);
    expect(limits).not.toHaveBeenCalled();
    expect(models).not.toHaveBeenCalled();
  });
  it('preserves independent Codex buckets and refuses to turn missing usage into a full allowance', () => {
    expect(codexQuotaWindows(null)).toEqual([]);
    expect(codexQuotaWindows({ primary: { windowDurationMins: 300 } })).toEqual([]);
    expect(codexQuotaWindows({ primary: { usedPercent: NaN } })).toEqual([]);
    const windows = codexQuotaWindows({ primary: { usedPercent: 0 }, rateLimitsByLimitId: {
      codex: { primary: { usedPercent: 25, windowDurationMins: 300, resetsAt: 1900000000 }, secondary: { usedPercent: 80, windowDurationMins: 10080 } },
      spark: { primary: { usedPercent: 104, windowDurationMins: 300 } },
    } });
    expect(windows).toHaveLength(3);
    expect(windows[0]).toMatchObject({ id: 'codex-primary', label: '5 小时', usedPercent: 25, resetsAt: 1900000000 });
    expect(windows[1]).toMatchObject({ label: '周额度', usedPercent: 80 });
    expect(windows[2]).toMatchObject({ label: 'spark · 5 小时', usedPercent: 100 });
  });

  it('shows the Pro Codex weekly allowance without duplicating the legacy summary or model reserve', () => {
    const windows = codexQuotaWindows({ planType: 'pro', primary: { usedPercent: 0, windowDurationMins: 300 },
      rateLimitsByLimitId: {
        base_model_inference: { limitName: 'gpt-reserve', primary: { usedPercent: 0, windowDurationMins: 10080 } },
        codex: { primary: { usedPercent: 20, windowDurationMins: 10080, resetsAt: 1900000000 }, secondary: null },
      } });
    expect(windows).toEqual([{ id: 'codex-primary', label: '周额度', usedPercent: 20,
      windowDurationMins: 10080, resetsAt: 1900000000 }]);
    expect(codexQuotaWindows({ rateLimitsByLimitId: {
      base_model_inference: { primary: { usedPercent: 0, windowDurationMins: 10080 } },
    } }, 'pro')).toEqual([]);
  });

  it('uses the fresh Pro account plan to omit a cached five-hour bucket while preserving the weekly window', async () => {
    vi.spyOn(forgeApi, 'getCodexStatus').mockResolvedValue({ installed: true, authSource: 'chatgpt',
      account: { authMode: 'chatgpt', planType: 'plus' }, install: { running: false }, models: [] } as never);
    vi.spyOn(forgeApi, 'getCodexAccount').mockResolvedValue({ ok: true, authMode: 'chatgpt', planType: 'Pro' } as never);
    vi.spyOn(forgeApi, 'getCodexRateLimits').mockResolvedValue({ ok: true, rateLimits: {
      planType: 'plus', primary: { usedPercent: 25, windowDurationMins: 300 },
      secondary: { usedPercent: 20, windowDurationMins: 10080, resetsAt: 1900000000 },
    } });
    vi.spyOn(forgeApi, 'getCodexModels').mockResolvedValue({ ok: true, models: [] });
    const status = await codexChannelStatus();
    expect(status.account?.planType).toBe('Pro');
    expect(status.quota.windows).toHaveLength(1);
    expect(status.quota.windows[0]).toMatchObject({ label: '周额度', usedPercent: 20, resetsAt: 1900000000 });
  });

  it('preserves Plus five-hour and weekly windows and never invents a duration', () => {
    const windows = codexQuotaWindows({ primary: { usedPercent: 25, windowDurationMins: 300 },
      secondary: { usedPercent: 20, windowDurationMins: 10080 } }, 'Plus');
    expect(windows.map(({ label, usedPercent }) => ({ label, usedPercent }))).toEqual([
      { label: '5 小时', usedPercent: 25 }, { label: '周额度', usedPercent: 20 },
    ]);
    expect(codexQuotaWindows({ primary: { usedPercent: 20 } }, 'pro')[0].label).toBe('主要窗口');
  });
});
