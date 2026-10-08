async page => {
  const browserErrors = [];
  const onError = error => browserErrors.push(error.message);
  page.on('pageerror', onError);
  const region = page.getByTestId('official-channel-connections');
  await region.waitFor();
  await page.getByTestId('channel-login-kimi').click({ trial: true });
  await region.locator('img').evaluateAll(images => Promise.all(images.map(image => image.complete ? null : new Promise((resolve, reject) => { image.onload = resolve; image.onerror = () => reject(new Error(image.src)); }))));
  const inspect = () => region.evaluate(root => ({
    cards: [...root.querySelectorAll('article')].map(card => { const rect = card.getBoundingClientRect(); return { id: card.dataset.testid, x: rect.x, y: rect.y, width: rect.width, height: rect.height }; }),
    images: [...root.querySelectorAll('img')].map(image => ({ src: new URL(image.src).pathname, loaded: image.complete && image.naturalWidth > 0 })),
    overflow: root.scrollWidth > root.clientWidth,
  }));
  const setTheme = async theme => {
    await page.getByTestId('settings-nav-appearance').click();
    await page.getByTestId(`theme-mode-${theme}`).click();
    await page.getByTestId('settings-nav-models').click();
    await region.waitFor();
    await page.getByTestId('channel-login-kimi').click({ trial: true });
    await region.evaluate(root => { for (let ancestor = root; ancestor; ancestor = ancestor.parentElement) ancestor.scrollTop = 0; });
    await page.mouse.move(1400, 10);
  };
  await page.setViewportSize({ width: 1440, height: 1000 });
  await setTheme('light');
  const wide = await inspect();
  if (wide.cards.length !== 4 || new Set(wide.cards.map(card => Math.round(card.y))).size !== 1 || wide.images.some(image => !image.loaded) || wide.overflow) throw new Error('Invalid four-card desktop layout');
  const lightBackground = await region.evaluate(root => getComputedStyle(root.querySelector('article')).backgroundColor);
  await page.screenshot({ path: 'docs/evidence/official-four-cards-light-v3.png', scale: 'css' });
  await region.screenshot({ path: 'docs/evidence/official-four-cards-detail-v3.png', scale: 'css' });
  await setTheme('dark');
  const darkBackground = await region.evaluate(root => getComputedStyle(root.querySelector('article')).backgroundColor);
  if (lightBackground === darkBackground) throw new Error('Theme did not apply');
  await page.screenshot({ path: 'docs/evidence/official-four-cards-dark-v3.png', scale: 'css' });
  await page.setViewportSize({ width: 900, height: 1100 });
  const medium = await inspect();
  if (new Set(medium.cards.map(card => Math.round(card.x))).size !== 2 || medium.overflow) throw new Error('Invalid two-column layout');
  await page.screenshot({ path: 'docs/evidence/official-four-cards-medium-v3.png', scale: 'css' });
  await page.setViewportSize({ width: 620, height: 950 });
  const narrow = await inspect();
  if (new Set(narrow.cards.map(card => Math.round(card.x))).size !== 1 || narrow.overflow) throw new Error('Invalid single-column layout');
  await page.screenshot({ path: 'docs/evidence/official-four-cards-narrow-v3.png', scale: 'css' });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole('button', { name: '反代配置', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: '连接反重力' });
  await dialog.waitFor();
  const key = dialog.getByLabel('反代访问密钥');
  if (await key.getAttribute('type') !== 'password') throw new Error('Connection key must be masked');
  await key.fill('unsubmitted-preview-key');
  await dialog.getByRole('button', { name: '关闭反重力配置' }).click();
  await page.getByRole('button', { name: '反代配置', exact: true }).click();
  if (await dialog.getByLabel('反代访问密钥').inputValue() !== '') throw new Error('Unsaved key was retained');
  await page.screenshot({ path: 'docs/evidence/official-four-antigravity-dialog-v3.png', scale: 'css' });
  await dialog.getByRole('button', { name: '关闭反重力配置' }).click();
  await setTheme('light');
  page.off('pageerror', onError);
  return { wide, medium, narrow, themes: { lightBackground, darkBackground }, dialog: { maskedKey: true, clearsUnsavedKey: true }, browserErrors };
}
