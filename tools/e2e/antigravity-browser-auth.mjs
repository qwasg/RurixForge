async page => {
  const errors = [];
  const onError = error => errors.push(error.message);
  page.on('pageerror', onError);
  const card = page.getByTestId('channel-card-antigravity');
  const statusURL = new URL('/api/forge/channels/antigravity', page.url()).href;
  const popupReady = page.waitForEvent('popup');
  await card.getByRole('button', { name: 'Google 网页授权', exact: true }).click();
  const popup = await popupReady;
  await popup.waitForURL(url => url.hostname === 'accounts.google.com', { timeout: 30000 });
  await popup.waitForLoadState('domcontentloaded');
  const authURL = new URL(popup.url());
  await card.getByRole('link', { name: /继续官方授权/ }).waitFor();
  const status = await (await page.request.get(statusURL)).json();
  if (status.configured || status.login.state !== 'pending' || !status.login.loginId) throw new Error('Authorization was marked complete before Google callback');
  const invalid = await page.request.get('http://127.0.0.1:51121/oauth-callback?state=invalid-test-state&code=untrusted-code');
  if (invalid.status() !== 400) throw new Error('Invalid Google callback was accepted');
  await popup.screenshot({ path: 'docs/evidence/antigravity-google-authorization-20261007-v2.png', scale: 'css' });
  const signInText = (await popup.locator('body').innerText()).slice(0, 1200);
  if (/Access blocked|invalid_client|Error 400/i.test(signInText)) throw new Error('Google rejected the OAuth application');
  await page.screenshot({ path: 'docs/evidence/antigravity-pending-auth-20261007-v2.png', scale: 'css' });
  await card.getByRole('button', { name: '取消 Antigravity 登录' }).click();
  await card.getByRole('button', { name: 'Google 网页授权', exact: true }).waitFor();
  const cancelled = await (await page.request.get(statusURL)).json();
  if (cancelled.configured || cancelled.login.state !== 'cancelled') throw new Error('Google cancellation did not clear pending authentication');
  await popup.close();
  const deniedLogin = await (await page.request.post(new URL('/api/forge/channels/antigravity/login', page.url()).href, { data: {} })).json();
  const deniedCallback = await page.request.get(`http://127.0.0.1:51121/oauth-callback?state=${encodeURIComponent(deniedLogin.loginId)}&error=access_denied`);
  if (deniedCallback.status() !== 200) throw new Error('Google denial was not handled');
  let denied;
  for (let n = 0; n < 40; n++) {
    denied = await (await page.request.get(statusURL)).json();
    if (denied.login.state === 'failed') break;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  if (denied.configured || denied.login.state !== 'failed') throw new Error('Denied Google consent was marked authenticated');
  await page.request.post(new URL('/api/forge/channels/antigravity/login/cancel', page.url()).href, { data: {} });
  await card.getByRole('button', { name: '刷新 Antigravity 额度' }).click();
  page.off('pageerror', onError);
  if (errors.length) throw new Error(errors.join('\n'));
  const result = { authHost: authURL.hostname, googleSignInVisible: /Sign in|登录|登入/i.test(signInText), popupOpened: true, invalidCallbackRejected: true, cancellationState: cancelled.login.state, deniedConsentState: denied.login.state, configuredBeforeHumanLogin: false, browserErrors: errors };
  console.log(JSON.stringify(result));
  return result;
}
