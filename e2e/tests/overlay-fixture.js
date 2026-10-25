const fs = require('fs');
const path = require('path');
const { pathToFileURL } = require('url');
const base = require('@playwright/test');

const REPO_ROOT = path.resolve(__dirname, '..', '..');
const WEB_DIR = path.join(REPO_ROOT, 'web');
const SOCKET_URL = 'ws://127.0.0.1:2009/ws';
const PNG_1X1 = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==',
  'base64',
);

function loadFixture(name) {
  const file = path.join(REPO_ROOT, 'e2e', 'fixtures', `${name}.json`);
  return JSON.parse(fs.readFileSync(file, 'utf8'));
}

function pageUrl(htmlFile) {
  return pathToFileURL(path.join(WEB_DIR, htmlFile)).href;
}

// Records every WebSocket connection the overlay opens, so tests can push
// messages into the latest one and count reconnects.
class MockBackend {
  constructor() {
    this.connections = [];
    this.waiters = [];
  }

  accept(ws) {
    this.connections.push(ws);
    this.waiters.splice(0).forEach((resolve) => resolve());
  }

  async waitForConnections(count) {
    while (this.connections.length < count) {
      await new Promise((resolve) => this.waiters.push(resolve));
    }
    return this.connections[count - 1];
  }

  async send(message) {
    const ws = await this.waitForConnections(Math.max(this.connections.length, 1));
    ws.send(JSON.stringify(message));
  }
}

const test = base.test.extend({
  backend: async ({ page }, use) => {
    const backend = new MockBackend();
    await page.route('https://raw.communitydragon.org/**', (route) =>
      route.fulfill({ status: 200, contentType: 'image/png', body: PNG_1X1 }),
    );
    await page.routeWebSocket(SOCKET_URL, (ws) => backend.accept(ws));
    await use(backend);
  },
});

module.exports = { test, expect: base.expect, loadFixture, pageUrl };
