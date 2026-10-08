async page => {
  await page.goto('http://127.0.0.1:3082/');
  const entry = page.getByTestId('auth-official-channels');
  await entry.waitFor();
  await entry.click();
  const card = page.getByTestId('channel-card-antigravity');
  await card.getByRole('button', { name: 'Google 网页授权', exact: true }).waitFor();
  await card.getByRole('button', { name: 'Google 网页授权', exact: true }).isEnabled();
  const cards = await page.getByTestId('official-channel-connections').locator('article').count();
  if (cards !== 4) throw new Error(`Expected four channel cards, got ${cards}`);
  console.log(JSON.stringify({ preview: page.url(), cards, antigravityDirectAuthorization: true }));
}
