const { test, expect, loadFixture, pageUrl } = require('./overlay-fixture');

const ICON_PATH = 'champion-icons/';

test.beforeEach(async ({ page, backend }) => {
  await page.goto(pageUrl('past_matches.html', '?matches=3'));
  await backend.waitForConnections(1);
});

test('shows at most the number of icons from the matches parameter', async ({ page, backend }) => {
  await backend.send(loadFixture('matches'));

  const icons = page.locator('#matches-container img.match-icon');
  await expect(icons).toHaveCount(3);
  await expect(icons.nth(2)).toHaveAttribute('src', new RegExp(`${ICON_PATH}157\\.png$`));
});

test('shows all icons when the backend sends fewer games', async ({ page, backend }) => {
  const matches = loadFixture('matches');
  await backend.send({ ...matches, data: matches.data.slice(0, 2) });

  await expect(page.locator('#matches-container img.match-icon')).toHaveCount(2);
});
