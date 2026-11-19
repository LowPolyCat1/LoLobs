const { test, expect, loadFixture, pageUrl } = require('./overlay-fixture');

test('reconnects after the backend closes the socket', async ({ page, backend }) => {
  await page.goto(pageUrl('current_rank.html'));
  const first = await backend.waitForConnections(1);

  await first.close();
  const startedAt = Date.now();
  await backend.waitForConnections(2);

  expect(Date.now() - startedAt).toBeGreaterThanOrEqual(1500);
  await backend.send(loadFixture('ranked'));
  await expect(page.locator('#tier')).toHaveText('GOLD II');
});
