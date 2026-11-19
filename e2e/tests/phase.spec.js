const { test, expect, loadFixture, pageUrl } = require('./overlay-fixture');

test.beforeEach(async ({ page, backend }) => {
  await page.goto(pageUrl('phase.html'));
  await backend.waitForConnections(1);
});

test('shows the phase label and the queue name', async ({ page, backend }) => {
  await backend.send(loadFixture('phase_in_queue'));

  await expect(page.locator('#phase-label')).toHaveText('In Queue');
  await expect(page.locator('#phase-queue')).toBeVisible();
  await expect(page.locator('#phase-queue')).toHaveText('Ranked Solo/Duo');
  await expect(page.locator('#phase-card')).toHaveClass(/\bvisible\b/);
});

test('shows only the label when there is no queue', async ({ page, backend }) => {
  await backend.send(loadFixture('phase_in_queue'));
  await expect(page.locator('#phase-queue')).toBeVisible();

  await backend.send(loadFixture('phase_label_only'));

  await expect(page.locator('#phase-label')).toHaveText('In Lobby');
  await expect(page.locator('#phase-queue')).toBeHidden();
  await expect(page.locator('#phase-card')).toHaveClass(/\bvisible\b/);
});

test('hides the card when there is no phase', async ({ page, backend }) => {
  await backend.send(loadFixture('phase_in_queue'));
  await expect(page.locator('#phase-card')).toHaveClass(/\bvisible\b/);

  await backend.send(loadFixture('phase_none'));

  await expect(page.locator('#phase-card')).not.toHaveClass(/\bvisible\b/);
});

test('stays hidden when other messages arrive', async ({ page, backend }) => {
  await backend.send(loadFixture('summoner'));
  await backend.send(loadFixture('ranked'));
  await backend.send(loadFixture('phase_none'));

  await expect(page.locator('#phase-card')).not.toHaveClass(/\bvisible\b/);
});

test('shows the card again after a new phase', async ({ page, backend }) => {
  await backend.send(loadFixture('phase_in_queue'));
  await backend.send(loadFixture('phase_none'));
  await expect(page.locator('#phase-card')).not.toHaveClass(/\bvisible\b/);

  await backend.send(loadFixture('phase_champ_select'));

  await expect(page.locator('#phase-card')).toHaveClass(/\bvisible\b/);
  await expect(page.locator('#phase-label')).toHaveText('Champ Select');
  await expect(page.locator('#phase-queue')).toHaveText('Ranked Flex');
});
