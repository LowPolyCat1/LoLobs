const { test, expect, loadFixture, pageUrl } = require('./overlay-fixture');

test.beforeEach(async ({ page, backend }) => {
  await page.goto(pageUrl('current_rank.html'));
  await backend.waitForConnections(1);
});

test('card is hidden until the first message arrives', async ({ page, backend }) => {
  const card = page.locator('#rank-card');
  await expect(card).not.toHaveClass(/\bvisible\b/);

  await backend.send(loadFixture('summoner'));

  await expect(card).toHaveClass(/\bvisible\b/);
});

test('renders the Riot ID from the summoner message', async ({ page, backend }) => {
  await backend.send(loadFixture('summoner'));

  await expect(page.locator('#game-name')).toHaveText('PlayerOne');
  await expect(page.locator('#tag-line')).toHaveText('#EUW');
});

test('shows no tag when the summoner message has no tag line', async ({ page, backend }) => {
  const summoner = loadFixture('summoner');
  await backend.send({ ...summoner, data: { gameName: summoner.data.gameName } });

  await expect(page.locator('#game-name')).toHaveText('PlayerOne');
  await expect(page.locator('#tag-line')).toHaveText('');
});

test('renders tier, division and LP of the solo queue', async ({ page, backend }) => {
  await backend.send(loadFixture('ranked'));

  await expect(page.locator('#tier')).toHaveText('GOLD II');
  await expect(page.locator('#lp')).toHaveText('57 LP');
  await expect(page.locator('#rank-icon')).toHaveAttribute('src', /emblem-gold\.png$/);
});

test('renders an apex tier without the NA division', async ({ page, backend }) => {
  await backend.send(loadFixture('ranked_master'));

  await expect(page.locator('#tier')).toHaveText('MASTER');
  await expect(page.locator('#lp')).toHaveText('213 LP');
});

function withLpChange(ranked, lpChange) {
  return { ...ranked, data: { ...ranked.data, lpChange } };
}

test('shows the LP gain of the last game in green', async ({ page, backend }) => {
  await backend.send(loadFixture('ranked'));

  const change = page.locator('#lp-change');
  await expect(change).toBeVisible();
  await expect(change).toHaveText('+21');
  await expect(change).toHaveClass(/\bwin\b/);
});

test('shows the LP loss of the last game in red', async ({ page, backend }) => {
  await backend.send(withLpChange(loadFixture('ranked'), { lastGame: -18, sessionTotal: 49 }));

  const change = page.locator('#lp-change');
  await expect(change).toHaveText('-18');
  await expect(change).toHaveClass(/\bloss\b/);
  await expect(change).not.toHaveClass(/\bwin\b/);
});

test('hides the LP change when no game was played', async ({ page, backend }) => {
  await backend.send(loadFixture('ranked'));
  await expect(page.locator('#lp-change')).toBeVisible();

  await backend.send(loadFixture('ranked_master'));

  await expect(page.locator('#lp')).toHaveText('213 LP');
  await expect(page.locator('#lp-change')).toBeHidden();
});
