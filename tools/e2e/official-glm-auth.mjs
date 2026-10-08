async page => {
  const card = page.getByTestId('channel-card-glm');
  const popupReady = page.waitForEvent('popup');
  await page.getByTestId('channel-login-glm').click();
  const popup = await popupReady;
  const key = card.getByLabel('Coding Plan Key');
  await key.waitFor();
  await popup.waitForURL(url => ['bigmodel.cn', 'www.bigmodel.cn'].includes(url.hostname), { waitUntil: 'commit', timeout: 15000 });
  const host = new URL(popup.url()).hostname;
  if (await key.getAttribute('type') !== 'password') throw new Error('Subscription key input is not masked');
  await key.fill('ui-test-unsubmitted');
  await card.getByRole('button', { name: '取消', exact: true }).click();
  if (await key.count()) throw new Error('Key form was not cleared');
  if (!popup.isClosed()) await popup.close();
  return { provider: 'glm', officialHost: host, passwordInput: true, formCleared: true };
}
