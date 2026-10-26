const { test, expect, loadFixture, pageUrl } = require('./overlay-fixture');

const ANIMATION_CLASSES = /rank-promotion|rank-demotion/;

function rankedMessage(tier, division, leaguePoints = 50) {
  const ranked = loadFixture('ranked');
  return { ...ranked, data: { ...ranked.data, tier, division, leaguePoints } };
}

// Records every class value of the emblem, so that a short animation class is not missed.
async function recordIconClasses(page) {
  await page.evaluate(() => {
    window.iconClasses = [];
    const icon = document.getElementById('rank-icon');
    new MutationObserver(() => window.iconClasses.push(icon.className)).observe(icon, {
      attributes: true,
      attributeFilter: ['class'],
    });
  });
}

async function sendAndWait(page, backend, message) {
  await backend.send(message);
  const expected = `${message.data.tier} ${message.data.division || ''}`.trim();
  await expect(page.locator('#lp')).toHaveText(`${message.data.leaguePoints} LP`);
  await expect(page.locator('#tier')).toHaveText(expected);
}

async function recordedClasses(page) {
  return page.evaluate(() => window.iconClasses.join(' '));
}

test.beforeEach(async ({ page, backend }) => {
  await page.goto(pageUrl('current_rank.html'));
  await backend.waitForConnections(1);
  await recordIconClasses(page);
});

test('first ranked message after connect does not animate', async ({ page, backend }) => {
  await sendAndWait(page, backend, rankedMessage('DIAMOND', 'I'));

  expect(await recordedClasses(page)).not.toMatch(ANIMATION_CLASSES);
});

test('promotion adds the promotion class and removes it at the end', async ({ page, backend }) => {
  const icon = page.locator('#rank-icon');
  await sendAndWait(page, backend, rankedMessage('GOLD', 'II'));
  await sendAndWait(page, backend, rankedMessage('GOLD', 'I'));

  await expect(icon).toHaveClass(/\brank-promotion\b/);
  await expect(icon).not.toHaveClass(ANIMATION_CLASSES, { timeout: 3000 });
});

test('tier promotion animates', async ({ page, backend }) => {
  await sendAndWait(page, backend, rankedMessage('DIAMOND', 'I'));
  await sendAndWait(page, backend, rankedMessage('MASTER', undefined));

  expect(await recordedClasses(page)).toMatch(/\brank-promotion\b/);
});

test('demotion adds the demotion class and removes it at the end', async ({ page, backend }) => {
  const icon = page.locator('#rank-icon');
  await sendAndWait(page, backend, rankedMessage('GOLD', 'IV'));
  await sendAndWait(page, backend, rankedMessage('SILVER', 'I'));

  await expect(icon).toHaveClass(/\brank-demotion\b/);
  await expect(icon).not.toHaveClass(ANIMATION_CLASSES, { timeout: 3000 });
});

test('LP change in the same division does not animate', async ({ page, backend }) => {
  await sendAndWait(page, backend, rankedMessage('GOLD', 'II', 57));
  await sendAndWait(page, backend, rankedMessage('GOLD', 'II', 80));
  await sendAndWait(page, backend, rankedMessage('GOLD', 'II', 40));

  expect(await recordedClasses(page)).not.toMatch(ANIMATION_CLASSES);
});

test('reconnect skips the first ranked message again', async ({ page, backend }) => {
  const first = await backend.waitForConnections(1);
  await sendAndWait(page, backend, rankedMessage('GOLD', 'II'));

  await first.close();
  await backend.waitForConnections(2);
  await sendAndWait(page, backend, rankedMessage('PLATINUM', 'IV'));
  expect(await recordedClasses(page)).not.toMatch(ANIMATION_CLASSES);

  await sendAndWait(page, backend, rankedMessage('PLATINUM', 'III'));
  expect(await recordedClasses(page)).toMatch(/\brank-promotion\b/);
});

test('rankChange orders tiers and divisions', async ({ page }) => {
  const changes = await page.evaluate(() => [
    rankChange(null, { tier: 'IRON', division: 'IV' }),
    rankChange({ tier: 'IRON', division: 'I' }, { tier: 'BRONZE', division: 'IV' }),
    rankChange({ tier: 'EMERALD', division: 'IV' }, { tier: 'PLATINUM', division: 'I' }),
    rankChange({ tier: 'MASTER' }, { tier: 'GRANDMASTER' }),
    rankChange({ tier: 'CHALLENGER' }, { tier: 'CHALLENGER' }),
    rankChange({ tier: 'GOLD', division: 'II' }, { tier: 'UNKNOWN', division: 'II' }),
  ]);

  expect(changes).toEqual(['none', 'promotion', 'demotion', 'promotion', 'none', 'none']);
});
