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
