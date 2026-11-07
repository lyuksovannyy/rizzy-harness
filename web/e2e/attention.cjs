// Chats waiting for the user: shown under the bar in yellow, replaced by
// similar-chat results while typing, browser notification, cleared on open.
// Usage: node web/e2e/attention.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ea-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 250\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n`);
const URL_ = 'http://127.0.0.1:7792/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7792'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 }, permissions: ['notifications'] });
    // Record notifications instead of showing them.
    await ctx.addInitScript(() => {
      window.__notes = [];
      class N { constructor(t, o) { window.__notes.push({ t, body: o && o.body }); } close() {} }
      N.permission = 'granted';
      N.requestPermission = async () => 'granted';
      window.Notification = N;
    });
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:has-text("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    // Turn notifications on.
    await p.goto(`${URL_}#/settings`);
    await p.click('.tabs button:has-text("Preferences")');
    await p.check('text=Desktop notifications');
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');

    // A question arrives while the user is elsewhere.
    await p.fill('.composer textarea', 'shall we go to the tokio conference?');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    await p.click('.new-chat');
    await p.waitForSelector('.attention button:has-text("tokio conference")', { timeout: 15000 });
    log('waiting:', (await p.innerText('.attention')).replace(/\s+/g, ' '));
    await p.waitForFunction(() => window.__notes.length > 0);
    log('notification:', JSON.stringify(await p.evaluate(() => window.__notes[0])));
    check((await p.title()).startsWith('(1)'), 'tab title shows the count');
    await p.screenshot({ path: `${S}/a-1-waiting.png` });

    // Typing with similar results replaces the list; clearing brings it back.
    await p.fill('.composer textarea', '');
    await p.focus('.composer textarea');
    await p.keyboard.type('tokio conference shall');
    await p.waitForSelector('.similar:not(.attention)');
    check((await p.locator('.attention').count()) === 0, 'search results replace the waiting list');
    await p.fill('.composer textarea', '');
    await p.waitForSelector('.attention');
    log('search results replace it while typing');

    // Opening the chat counts as seeing the question.
    await p.click('.attention button');
    await p.waitForSelector('.conversation');
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await sleep(600);
    check((await p.locator('.attention').count()) === 0, 'seen question no longer waits');
    log('cleared after opening');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/a-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
