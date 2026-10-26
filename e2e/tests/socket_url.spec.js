const fs = require('fs');
const path = require('path');
const { test, expect } = require('@playwright/test');

const WEB_DIR = path.resolve(__dirname, '..', '..', 'web');
const HOST = 'overlay.test:3000';
const CONTENT_TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };

function serveWebDir(route) {
  const file = path.join(WEB_DIR, new URL(route.request().url()).pathname);
  route.fulfill({
    status: 200,
    contentType: CONTENT_TYPES[path.extname(file)],
    body: fs.readFileSync(file),
  });
}

test('connects to the server that sent the page', async ({ page }) => {
  await page.route(`http://${HOST}/**`, serveWebDir);
  let resolveConnection;
  const connected = new Promise((resolve) => {
    resolveConnection = resolve;
  });
  await page.routeWebSocket(`ws://${HOST}/ws`, (ws) => resolveConnection(ws));

  await page.goto(`http://${HOST}/current_rank.html`);
  const ws = await connected;
  ws.send(JSON.stringify({ type: 'summoner', data: { gameName: 'PlayerOne', tagLine: 'EUW' } }));

  await expect(page.locator('#game-name')).toHaveText('PlayerOne');
});
