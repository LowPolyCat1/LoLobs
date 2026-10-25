const { test, expect, loadFixture, pageUrl } = require('./overlay-fixture');

const ICON_PATH = 'champion-icons/';

test.beforeEach(async ({ page, backend }) => {
  await page.goto(pageUrl('past_matches.html'));
  await backend.waitForConnections(1);
});

test('shows the last five solo queue games with result borders', async ({ page, backend }) => {
  await backend.send(loadFixture('matches'));

  const icons = page.locator('#matches-container img.match-icon');
  await expect(icons).toHaveCount(5);

  const expected = [
    { championId: 103, status: 'win-border' },
    { championId: 222, status: 'loss-border' },
    { championId: 157, status: 'remake' },
    { championId: 64, status: 'win-border' },
    { championId: 25, status: 'loss-border' },
  ];
  for (const [index, { championId, status }] of expected.entries()) {
    const icon = icons.nth(index);
    await expect(icon).toHaveAttribute('src', new RegExp(`${ICON_PATH}${championId}\\.png$`));
    await expect(icon).toHaveClass(`match-icon ${status}`);
  }
});

test('leaves out games from other queues', async ({ page, backend }) => {
  await backend.send(loadFixture('matches'));

  const container = page.locator('#matches-container');
  await expect(container.locator('img')).toHaveCount(5);
  await expect(container.locator(`img[src$="${ICON_PATH}54.png"]`)).toHaveCount(0);
  await expect(container.locator(`img[src$="${ICON_PATH}81.png"]`)).toHaveCount(0);
});

test('replaces the icons on a new match history message', async ({ page, backend }) => {
  const matches = loadFixture('matches');
  await backend.send(matches);
  await expect(page.locator('#matches-container img')).toHaveCount(5);

  const single = { ...matches, games: { ...matches.games, games: matches.games.games.slice(0, 1) } };
  await backend.send(single);

  await expect(page.locator('#matches-container img')).toHaveCount(1);
  await expect(page.locator('#matches-card')).toHaveClass(/\bvisible\b/);
});
