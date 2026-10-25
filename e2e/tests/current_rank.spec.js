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

test('shows NA1 when the summoner message has no tag line', async ({ page, backend }) => {
  const summoner = loadFixture('summoner');
  await backend.send({ ...summoner, data: { gameName: summoner.data.gameName } });

  await expect(page.locator('#game-name')).toHaveText('PlayerOne');
  await expect(page.locator('#tag-line')).toHaveText('#NA1');
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
