const { test, expect, loadFixture, pageUrl } = require('./overlay-fixture');

function withSoloRecord(stats, wins, losses) {
  const queues = stats.queues.map((q) =>
    q.queueType === 'RANKED_SOLO_5x5' ? { ...q, wins, losses } : q,
  );
  return { ...stats, queues };
}

test.beforeEach(async ({ page, backend }) => {
  await page.goto(pageUrl('season_winrate.html'));
  await backend.waitForConnections(1);
});

test('renders the solo queue record and win rate', async ({ page, backend }) => {
  await backend.send(loadFixture('ranked_stats'));

  await expect(page.locator('#wins')).toHaveText('34');
  await expect(page.locator('#losses')).toHaveText('29');
  await expect(page.locator('#wr-value')).toHaveText('54.0%');
  await expect(page.locator('#stats-card')).toHaveClass(/\bvisible\b/);
});

test('renders 0.0% when no solo queue games are played', async ({ page, backend }) => {
  await backend.send(withSoloRecord(loadFixture('ranked_stats'), 0, 0));

  await expect(page.locator('#wins')).toHaveText('0');
  await expect(page.locator('#losses')).toHaveText('0');
  await expect(page.locator('#wr-value')).toHaveText('0.0%');
});
