import './bridge';

export type OfficialChannel = 'codex' | 'antigravity' | 'kimi' | 'glm';
const hosts: Record<OfficialChannel, string[]> = {
  codex: ['auth.openai.com', 'login.openai.com', 'chatgpt.com', 'www.chatgpt.com'],
  antigravity: ['accounts.google.com', 'antigravity.google'],
  kimi: ['kimi.com', 'www.kimi.com', 'kimi.ai', 'www.kimi.ai', 'auth.kimi.com', 'auth.kimi.ai'],
  glm: ['bigmodel.cn', 'www.bigmodel.cn', 'open.bigmodel.cn'],
};

export function officialAuthUrl(channel: OfficialChannel, raw: unknown): string | null {
  if (typeof raw !== 'string') return null;
  try {
    const url = new URL(raw);
    return url.protocol === 'https:' && !url.username && !url.password && !url.port
      && hosts[channel].includes(url.hostname) ? url.href : null;
  } catch { return null; }
}

/** Reserve a popup during the click, before awaiting the local CLI. */
export function reserveOfficialAuth(channel: OfficialChannel) {
  const native = window.forgeAPI?.auth?.openExternal;
  const desktop = !!window.forgeAPI;
  const popup = desktop ? null : window.open('', `forge-${channel}-login-${crypto.randomUUID()}`, 'popup,width=560,height=760');
  if (popup) {
    popup.opener = null;
    try {
      popup.document.title = '正在连接官方授权';
      popup.document.body.textContent = '正在准备官方登录，请稍候…';
      popup.document.body.style.cssText = 'font:16px system-ui;padding:40px;background:#101114;color:#f4f4f5';
    } catch { /* A previously opened authorization window may already be cross-origin. */ }
  }
  return {
    async navigate(raw: unknown): Promise<boolean> {
      const url = officialAuthUrl(channel, raw);
      if (!url) throw new Error('官方授权地址校验失败');
      if (native) { await native(channel, url); return true; }
      // Existing desktop windows use the external-link handler until their preload is refreshed.
      if (desktop) { window.open(url, '_blank', 'noopener,noreferrer'); return true; }
      if (!popup || popup.closed) return false;
      popup.location.replace(url);
      return true;
    },
    close() {
      if (!popup || popup.closed) return;
      try {
        // After navigating with opener removed, the official page belongs to the browser.
        void popup.document;
        popup.close();
      } catch { /* Only the local loading window can be closed from this origin. */ }
    },
  };
}
