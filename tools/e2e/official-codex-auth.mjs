async page => {
  const popupReady = page.waitForEvent('popup');
  await page.getByTestId('channel-login-codex').click();
  const popup = await popupReady;
  await page.getByTestId('channel-card-codex').getByRole('link', { name: /继续官方授权/ }).waitFor({ timeout: 30000 });
  await popup.waitForURL(url => ['auth.openai.com', 'chatgpt.com'].includes(url.hostname), { waitUntil: 'commit', timeout: 15000 });
  const host = new URL(popup.url()).hostname;
  await page.getByRole('button', { name: '取消 Codex 登录' }).click();
  await page.getByTestId('channel-login-codex').click({ trial: true });
  const account = await page.evaluate(async () => (await (await fetch('/api/forge/codex/account')).json()));
  if (account.lastError) throw new Error('Cancelled login surfaced an authentication error');
  const isolated = popup.isClosed() || await popup.evaluate(() => window.opener === null);
  if (!popup.isClosed()) await popup.close();
  return { provider: 'codex', officialHost: host, isolated, cancelledWithoutError: true };
}
