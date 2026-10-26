const { test, expect, loadFixture, pageUrl } = require('./overlay-fixture');

test.beforeEach(async ({ page, backend }) => {
  await page.goto(pageUrl('session_winrate.html'));
  await backend.waitForConnections(1);
});

test('renders the session record and win rate', async ({ page, backend }) => {
  await backend.send(loadFixture('ranked'));

  await expect(page.locator('#s-wins')).toHaveText('2');
  await expect(page.locator('#s-losses')).toHaveText('1');
  await expect(page.locator('#s-wr')).toHaveText('67%');
  await expect(page.locator('#session-card')).toHaveClass(/\bvisible\b/);
});

test('updates the session stats on a second message', async ({ page, backend }) => {
  const stats = loadFixture('ranked');
  await backend.send(stats);
  await expect(page.locator('#s-wins')).toHaveText('2');

  await backend.send({ ...stats, data: { ...stats.data, session: { wins: 3, losses: 3 } } });

  await expect(page.locator('#s-wins')).toHaveText('3');
  await expect(page.locator('#s-losses')).toHaveText('3');
  await expect(page.locator('#s-wr')).toHaveText('50%');
});

test('shows the LP total of the session', async ({ page, backend }) => {
  await backend.send(loadFixture('ranked'));

  await expect(page.locator('#s-lp')).toHaveText('+67 LP');
  await expect(page.locator('#s-lp')).toHaveClass(/\bwin\b/);
});

test('shows a negative LP total in red and zero without a sign', async ({ page, backend }) => {
  const stats = loadFixture('ranked');
  await backend.send({ ...stats, data: { ...stats.data, lpChange: { sessionTotal: -35 } } });
  await expect(page.locator('#s-lp')).toHaveText('-35 LP');
  await expect(page.locator('#s-lp')).toHaveClass(/\bloss\b/);

  await backend.send(loadFixture('ranked_master'));

  await expect(page.locator('#s-lp')).toHaveText('0 LP');
  await expect(page.locator('#s-lp')).not.toHaveClass(/\b(win|loss)\b/);
});

test('shows no LP total when the rank is not on the ladder', async ({ page, backend }) => {
  const stats = loadFixture('ranked');
  const { lpChange, ...withoutChange } = stats.data;
  await backend.send({ ...stats, data: withoutChange });

  await expect(page.locator('#s-wins')).toHaveText('2');
  await expect(page.locator('#s-lp')).toHaveText('-- LP');
});
