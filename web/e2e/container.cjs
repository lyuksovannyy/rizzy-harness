// The central server running in a container: claim, onboarding, a chat
// answered by the master, and agentic refused until a remote node joins.
// Usage: node web/e2e/container.cjs <url> <claim-code> [work-dir]
const { chromium } = require('playwright');
const fs = require('fs');
const os = require('os');
const path = require('path');
const URL_ = process.argv[2];
const CODE = process.argv[3];
const S = process.argv[4] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ec-'));
const log = (...a) => console.log('•', ...a);
const check = (c, m) => { if (!c) throw new Error(m); };

(async () => {
  const b = await chromium.launch();
  const p = await b.newPage({ viewport: { width: 1280, height: 800 } });
  const errors = [];
  p.on('pageerror', (e) => errors.push(e.message));
  try {
    await p.goto(URL_);
    await p.fill('input[placeholder="claim code"]', CODE);
    await p.fill('input[placeholder="username"]', 'alex');
    await p.fill('input[type=password]', 'admin password');
    await p.click('text=Create account');
    await p.click('text=Skip: use the built-in demo');
    await p.click('text=Skip: keywords only');
    await p.click('button:has-text("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');
    await p.keyboard.type('hello from the container');
    await p.keyboard.press('Enter');
    await p.waitForSelector('text=You said: hello from the container');
    log('chat answered by the master');
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    check(await p.locator('.mode-switch button:has-text("Agentic")').isDisabled(), 'agentic disabled');
    await p.waitForSelector('text=No worker node is online');
    log('agentic disabled: no worker node (none in the container, by design)');
    await p.goto(`${URL_}#/settings/nodes`);
    await p.waitForSelector('table.users td:text-is("master")');
    const rows = await p.$$eval('table.users tbody tr td:first-child', (t) => t.map((x) => x.textContent));
    check(!rows.includes('local'), 'no local node in the container: ' + rows.join(','));
    log('nodes:', rows.join(', '));
    await p.screenshot({ path: `${S}/c-1-nodes.png` });
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
  } catch (e) {
    await p.screenshot({ path: `${S}/c-fail.png`, fullPage: true }).catch(() => {});
    throw e;
  } finally {
    await b.close();
  }
  log('screenshots in', S);
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
