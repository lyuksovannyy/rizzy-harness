const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
// Phase 3 end-to-end flows against the real binary.
// Usage: node web/e2e/phase3.cjs <path-to-harness-binary> [work-dir]
// Needs Playwright with Chromium. Exits non-zero on the first failure.
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-'));
fs.mkdirSync(`${S}/p3/data`, { recursive: true });
fs.mkdirSync(`${S}/p3/proj`, { recursive: true });
fs.writeFileSync(`${S}/p3/proj/main.rs`, 'fn main() {}\n');
fs.writeFileSync(`${S}/p3/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 25\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/p3/data/node.toml`, `workspace_roots = ["${S}/p3"]\n`);
const URL = 'http://127.0.0.1:7798/';

/** Finish first-run onboarding with defaults (demo provider for admins). */
async function skipOnboarding(page, admin) {
  if (admin) {
    await page.waitForSelector('text=Connect an AI provider');
    await page.click('text=Skip: use the built-in demo');
    await keywordsOnly(page);
  }
  await page.waitForSelector('text=About you');
  await page.click('button:text-is("Continue")');
  await page.waitForSelector('h2:has-text("Search engine")');
  await page.click('button:has-text("Finish")');
}
const sleep = (ms) => new Promise(r => setTimeout(r, ms));
const start = () => spawn(BIN, ['--data-dir', `${S}/p3/data`, 'serve', '--bind', '127.0.0.1:7798'], { detached: true, stdio: 'ignore' });
const log = (...a) => console.log('•', ...a);
// The server of the moment (the test restarts it); killed on failure too, so
// a failed run never leaves its port taken.
let srv = null;
(async () => {
  srv = start();
  const code = await claimCode(`${S}/p3/data/claim-code`, URL);
  const b = await chromium.launch();
  const ctx = await b.newContext({ viewport: { width: 1280, height: 800 } });
  const p = await ctx.newPage();
  const errors = [];
  p.on('pageerror', e => errors.push(e.message));
  await p.goto(URL);
  await createOwner(p, code);
  await skipOnboarding(p, true);
  await p.waitForSelector('h1.greeting');
  log('greeting:', await p.innerText('h1.greeting'));
  await p.screenshot({ path: `${S}/p3-1-home.png` });

  // Ask
  await p.keyboard.type('hello there');
  await p.keyboard.press('Enter');
  await p.waitForSelector('text=You said: hello there');
  log('ask ok, hash', p.url().split('#')[1]?.slice(0, 4));

  // Agentic: new chat, Tab once (ask -> agentic)
  await p.click('.new-chat');
  await p.waitForSelector('h1.greeting');
  await p.focus('.composer textarea');
  await p.keyboard.press('Tab');
  log('mode after Tab:', await p.innerText('.mode-switch button[aria-checked=true]'));
  await p.fill('.agentic-row input', `${S}/p3/proj`);
  await p.fill('.composer textarea', 'run: ls');
  await p.keyboard.press('Enter');
  await p.waitForSelector('.approval');
  await p.screenshot({ path: `${S}/p3-2-approval.png` });
  await p.click('.approval button:has-text("Allow")');
  await p.waitForSelector('text=Tool said: main.rs');
  log('agentic + approval ok');

  // @mention popup routes one turn to Writer
  await p.fill('.composer textarea', '');
  await p.focus('.composer textarea');
  await p.keyboard.type('@wr');
  await p.waitForSelector('.mention-popup li');
  log('popup:', (await p.innerText('.mention-popup')).replace(/\s+/g, ' ').slice(0, 40));
  await p.keyboard.press('Enter');
  await p.keyboard.type('draft notes');
  log('override chip:', await p.innerText('.pill.override'));
  await p.screenshot({ path: `${S}/p3-3-mention.png` });
  await p.keyboard.press('Enter');
  await p.waitForSelector('.author:has-text("Writer")');
  const bubble = await p.locator('.msg.user .bubble').last().innerText();
  if (bubble !== '@writer draft notes') throw new Error('mention caret bug: ' + bubble);
  log('writer turn labelled');

  // Switch session agent
  // The composer's agent picker is a listbox button, not a native select.
  await p.click('.composer button.select-button[aria-label^="agent:"]');
  await p.click('[role=listbox][aria-label=agent] [role=option]:has-text("Scout")');
  await p.waitForSelector('.divider:has-text("Scout")');
  log('agent switch recorded');
  await p.screenshot({ path: `${S}/p3-4-conversation.png` });

  // Search mode -> redirect to search URL (intercepted)
  await p.route('https://duckduckgo.com/**', r => r.fulfill({ body: 'search ok', contentType: 'text/plain' }));
  await p.click('.new-chat');
  await p.waitForSelector('h1.greeting');
  await p.focus('.composer textarea');
  await p.keyboard.press('Shift+Tab');
  log('mode after Shift+Tab:', await p.innerText('.mode-switch button[aria-checked=true]'));
  await p.keyboard.type('rust tokio');
  await p.keyboard.press('Enter');
  await p.waitForURL(/duckduckgo/);
  log('search url:', p.url());
  await p.goto(URL);
  await p.waitForSelector('h1.greeting');

  // Settings, through the account menu in the sidebar's footer.
  await p.click('.sidebar-foot .nav-item.account');
  await p.getByRole('menuitem', { name: 'Settings', exact: true }).click();
  await p.waitForSelector('.tabs');
  await p.screenshot({ path: `${S}/p3-5-settings.png` });
  await p.click('.settings-sections >> text=Server');
  await p.click('.tabs >> text=Users');
  await p.fill('input[placeholder="username"]', 'bob');
  await p.click('form >> button:has-text("Create")');
  // The server makes the temporary password; the dialog shows it once.
  const temp = (await p.innerText('dialog .temp-password')).trim();
  await p.click('dialog button:has-text("Done")');
  await p.waitForSelector('td:has-text("bob")');
  log('bob created');

  // Bob: agentic disabled
  const ctx2 = await b.newContext({ viewport: { width: 1280, height: 800 } });
  const q = await ctx2.newPage();
  await q.goto(URL);
  await q.fill('input[placeholder="username"]', 'bob');
  await q.fill('input[type=password]', temp);
  await q.click('button:has-text("Log in")');
  const pw = q.locator('input[type=password]');
  await q.waitForSelector('text=Choose your password');
  await pw.nth(0).fill(temp);
  await pw.nth(1).fill('bobs own password');
  await pw.nth(2).fill('bobs own password');
  await q.click('text=Save password');
  await skipOnboarding(q, false);
  await q.waitForSelector('h1.greeting');
  log('bob agentic disabled:', await q.locator('.mode-switch button:has-text("Agentic")').isDisabled(),
      '| hint:', await q.innerText('.hint'), '| bob chats:', await q.locator('.session-item').count());
  await q.screenshot({ path: `${S}/p3-6-member.png` });

  // Kill -9 mid-stream, restart, recover
  await p.click('.new-chat');
  await p.waitForSelector('h1.greeting');
  await p.focus('.composer textarea');
  await p.keyboard.type('slow: 300');
  // Mock commands look like search terms to the mode predictor; pick Ask.
  await p.click('.mode-switch button:has-text("Ask")');
  await p.focus('.composer textarea');
  await p.keyboard.press('Enter');
  await p.waitForSelector('.msg.assistant.streaming');
  await sleep(800);
  process.kill(-srv.pid, 'SIGKILL');
  // The sidebar shows a connection icon only while something is wrong.
  await p.waitForSelector('.conn-icon', { timeout: 10000 });
  log('connection icon while the server is down:', await p.getAttribute('.conn-icon', 'class'));
  await sleep(300);
  srv = start();
  await p.waitForSelector('text=turn interrupted', { timeout: 20000 });
  await p.waitForSelector('.conn-icon', { state: 'detached', timeout: 20000 });
  log('recovered after kill -9; connection icon gone');
  const brand = await p.innerText('.brand');
  if (brand !== 'rizzy harness') throw new Error('brand: ' + brand);

  // Mobile
  await p.setViewportSize({ width: 390, height: 800 });
  await p.goto(URL);
  await p.waitForSelector('h1.greeting');
  await p.screenshot({ path: `${S}/p3-7-mobile.png` });
  await p.click('.topbar button[aria-label="Open sidebar"]');
  await sleep(300);
  await p.screenshot({ path: `${S}/p3-8-drawer.png` });
  if (errors.length) throw new Error('page errors: ' + errors.join('; '));
  log('screenshots in', S);
  await b.close();
  process.kill(-srv.pid, 'SIGKILL');
})().catch(e => {
  console.error('FAILED:', e.message.split('\n')[0]);
  if (srv) process.kill(-srv.pid, 'SIGKILL');
  process.exit(1);
});
