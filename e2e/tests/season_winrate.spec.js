const { test, expect, loadFixture, pageUrl } = require('./overlay-fixture');

function withSeasonRecord(ranked, wins, losses) {
  return { ...ranked, data: { ...ranked.data, wins, losses } };
}

test.beforeEach(async ({ page, backend }) => {
  await page.goto(pageUrl('season_winrate.html'));
  await backend.waitForConnections(1);
});

test('renders the solo queue record and win rate', async ({ page, backend }) => {
  await backend.send(loadFixture('ranked'));

  await expect(page.locator('#wins')).toHaveText('34');
  await expect(page.locator('#losses')).toHaveText('29');
  await expect(page.locator('#wr-value')).toHaveText('54.0%');
  await expect(page.locator('#stats-card')).toHaveClass(/\bvisible\b/);
});

test('renders 0.0% when no solo queue games are played', async ({ page, backend }) => {
  await backend.send(withSeasonRecord(loadFixture('ranked'), 0, 0));

  await expect(page.locator('#wins')).toHaveText('0');
  await expect(page.locator('#losses')).toHaveText('0');
  await expect(page.locator('#wr-value')).toHaveText('0.0%');
});
