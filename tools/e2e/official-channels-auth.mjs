async page => {
  const card = page.getByTestId('channel-card-kimi');
  const popupReady = page.waitForEvent('popup');
  await page.getByTestId('channel-login-kimi').click();
  const popup = await popupReady;
  await card.getByRole('link', { name: /继续官方授权/ }).waitFor({ timeout: 30000 });
  await popup.waitForURL(url => ['auth.kimi.com', 'www.kimi.com'].includes(url.hostname), { waitUntil: 'commit', timeout: 15000 });
  const host = new URL(popup.url()).hostname;
  await page.getByRole('button', { name: '取消 Kimi Code 登录' }).click();
  await page.getByTestId('channel-login-kimi').click({ trial: true });
  const cancelled = await page.evaluate(async () => (await (await fetch('/api/forge/channels/kimi')).json()).login.state);
  const isolated = popup.isClosed() || await popup.evaluate(() => window.opener === null);
  if (!popup.isClosed()) await popup.close();
  return { provider: 'kimi', officialHost: host, cancelled, isolated };
}
