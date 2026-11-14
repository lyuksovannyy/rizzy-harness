// What the agent is doing, against the real binary and the demo provider:
// a turn that finishes while the person is elsewhere sends one notification
// with fixed words (no title) and leaves an unread dot in the sidebar,
// cleared by opening the chat; the chat header's context meter shows the
// share of a known window and where the summary is written, and a visible
// "window unknown" for a model without one.
// Usage: node web/e2e/visibility.cjs <path-to-harness-binary> [work-dir] [port]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ev-'));
const PORT = Number(process.argv[4] || 7793);
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
// "mock" has a known window; "plain" has none.
fs.writeFileSync(
  `${S}/data/config.toml`,
  '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 250\nmodels = [{ id = "mock", context = 32000 }, { id = "plain" }]\n',
);
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n`);
const URL_ = `http://127.0.0.1:${PORT}/`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
async function claimCode() {
  for (let i = 0; i < 600; i++) {
    try {
      await fetch(URL_);
      return fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    } catch {
      await sleep(100);
    }
  }
  throw new Error('the server did not start within a minute');
}
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', `127.0.0.1:${PORT}`], { detached: true, stdio: 'ignore' });
  try {
    const code = await claimCode();
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 }, permissions: ['notifications'] });
    await keyed(ctx);
    await ctx.addInitScript(() => {
      window.__notes = [];
      class N { constructor(t, o) { window.__notes.push({ t, body: o && o.body, tag: o && o.tag }); } close() {} }
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
    await p.click('button:text-is("Continue")');
    await p.click('button:text-is("Finish")');
    await p.waitForSelector('h1.greeting');
    const home = async () => {
      await p.click('aside.sidebar a:has-text("New chat")');
      await p.waitForSelector('h1.greeting');
    };
    await p.goto(`${URL_}#/settings/account/preferences`);
    await p.check('text=Desktop notifications');
    await home();

    // 1. A turn finishes while the person is on the home screen.
    await p.fill('.composer textarea', 'tell me about the harbour lighthouse');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    const chatHash = await p.evaluate(() => location.hash);
    await home();
    await p.waitForFunction(() => window.__notes.some((n) => n.body && n.body.startsWith('A chat finished')), null, { timeout: 20000 });
    const note = await p.evaluate(() => window.__notes.find((n) => n.body.startsWith('A chat finished')));
    log('notification:', JSON.stringify(note));
    check(note.body === 'A chat finished: number 1 in your recent chats', `fixed words (${note.body})`);
    check(!JSON.stringify(note).toLowerCase().includes('lighthouse'), 'no title in the notification');
    const row = `aside.sidebar a.session-item[href="${chatHash}"]`;
    await p.waitForSelector(`${row} .dot.unseen`, { timeout: 10000 });
    log('unread dot on the row');
    await p.screenshot({ path: `${S}/v-1-unread.png` });

    // Opening it clears the dot, here and on the server.
    await p.click(row);
    await p.waitForSelector('.conversation');
    await home();
    await sleep(800);
    check((await p.locator(`${row} .dot.unseen`).count()) === 0, 'the dot goes once the chat was opened');
    const listed = await p.evaluate(async () => (await (await fetch('/api/sessions')).json()).sessions);
    check(listed.every((s) => s.unread === false), 'nothing unread on the server');
    log('opened: dot cleared, server agrees');

    // 2. The meter: a known window (the chat above ran on demo::mock).
    await p.click(row);
    await p.waitForSelector('.conv-head .context-meter');
    const known = await p.innerText('.conv-head .context-meter');
    log('meter:', known);
    check(/^\d+% of 32k · summary at 50%$/.test(known), `share and summary point (${known})`);
    check((await p.locator('.conv-head .window-unknown').count()) === 0, 'no "window unknown" for a known window');
    await p.screenshot({ path: `${S}/v-2-meter.png` });

    // A model without a window: a visible "window unknown" saying how to set it.
    const plain = await p.evaluate(async () => {
      const json = { 'content-type': 'application/json' };
      const made = await (await fetch('/api/sessions', { method: 'POST', headers: json, body: JSON.stringify({ profile: 'chat', model: 'demo::plain' }) })).json();
      await fetch(`/api/sessions/${made.session_id}/prompt`, { method: 'POST', headers: json, body: JSON.stringify({ text: 'hello' }) });
      return made.session_id;
    });
    await p.goto(`${URL_}#/s/${plain}`);
    await p.waitForSelector('.conv-head .window-unknown', { timeout: 20000 });
    const title = await p.getAttribute('.conv-head .context-meter', 'title');
    log('unknown window:', await p.innerText('.conv-head .context-meter'));
    check(title.includes('context = <tokens>') && title.includes('24,000'), `how to set it (${title})`);
    await p.screenshot({ path: `${S}/v-3-unknown.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/v-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
