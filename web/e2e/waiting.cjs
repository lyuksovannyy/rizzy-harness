// Between Send and the model's first word, against the real binary and the
// demo provider made slow (8 s before each word): the message shows at once
// (from home above the box, then in the new chat; in an open chat in the
// chat), marked "Sending…" until the log has it, then once; the chat says it
// waits for the model from the click on, counts the seconds after 5, and
// tells the wait from the answer; a reload shows the same wait; a refused
// message goes back to the box with why; a reload while the message is sent
// finds it in the box; a second tab's message does not hide this tab's; a
// new chat's first message refused after the chat opened stays, marked, with
// Edit and Retry. The browser's own POSTs are held
// 1.5 s, so "at once" is before the server has the message.
// Usage: node web/e2e/waiting.cjs <path-to-harness-binary> [work-dir] [port]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ew-'));
const PORT = Number(process.argv[4] || 7797);
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 8000\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n`);
const URL_ = `http://127.0.0.1:${PORT}/`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const HOLD_MS = 1500;
let PAGE = null;

// In the page: the time of the click on Send and of the first frame that
// shows a `.msg.user` bubble with the text being sent.
function recorder() {
  window.__w = { click: null, shown: null, text: null };
  document.addEventListener('click', (e) => {
    if (e.target.closest && e.target.closest('.composer button.send:not(.stop)')) {
      window.__w.click = performance.now();
      window.__w.shown = null;
    }
  }, true);
  new MutationObserver(() => {
    const w = window.__w;
    if (w.click == null || w.shown != null) return;
    if ([...document.querySelectorAll('.msg.user .bubble')].some((b) => b.textContent === w.text)) w.shown = performance.now() - w.click;
  }).observe(document, { subtree: true, childList: true, characterData: true });
}

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', `127.0.0.1:${PORT}`], { detached: true, stdio: ['ignore', 'ignore', fs.openSync(`${S}/serve.log`, 'w')] });
  try {
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 } });
    await keyed(ctx);
    await ctx.addInitScript(recorder);
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
    await p.route('**/api/sessions', async (r) => { if (r.request().method() === 'POST') await sleep(HOLD_MS); await r.continue(); });
    await p.route('**/api/sessions/*/prompt', async (r) => { await sleep(HOLD_MS); await r.continue(); });
    const status = () => p.locator('[data-testid="turn-status"]');
    const said = () => p.locator('[role="status"].sr-only').innerText();
    const box = p.locator('.composer textarea');
    const sendText = async (text) => {
      await box.fill(text);
      await p.evaluate((t) => { window.__w.text = t; }, text);
      await p.click('.composer button.send:not(.stop)');
    };
    const shownAfter = () => p.evaluate(() => window.__w.shown);
    const bubbles = (text) => p.locator('.msg.user .bubble', { hasText: text });

    // 1. The first message from home.
    await p.click('.mode-switch button:has-text("Ask")');
    await sendText('slow: 1');
    await p.waitForSelector('.composer-wrap > .msg.user.sending', { timeout: 1000 });
    const home = await shownAfter();
    check(home != null && home < 100, `home: the message shows at once (${home} ms)`);
    check((await box.inputValue()) === '', 'home: the box is empty');
    await p.screenshot({ path: `${S}/waiting-1-home-sending.png` });
    log(`home: shown ${Math.round(home)} ms after the click, above the box`);
    await p.waitForSelector('.conversation .msg.user.sending', { timeout: 10000 });
    check((await status().innerText()).includes('is waiting for the model'), 'the new chat says it waits for the model');
    await p.screenshot({ path: `${S}/waiting-2-chat-sending.png` });
    await p.waitForSelector('.conversation .msg.user.sending', { state: 'detached', timeout: 15000 });
    check((await bubbles('slow: 1').count()) === 1, 'the logged message replaced the row, once');
    check((await status().innerText()).includes('is waiting for the model'), 'still waiting for the first word');
    check((await said()).includes('is waiting for the model'), 'said once to screen readers');
    // A long wait counts its seconds.
    await p.waitForSelector('[data-testid="turn-status"] .waited', { timeout: 10000 });
    const count = await p.locator('[data-testid="turn-status"] .waited').innerText();
    check(/^\d+ s$/.test(count) && Number(count.split(' ')[0]) >= 5, `a quiet count (${count})`);
    check(!(await said()).match(/\d/), 'the count is not read out');
    await p.screenshot({ path: `${S}/waiting-3-count.png` });
    log('the chat: one message, "waiting for the model", a count of', count);
    await p.waitForSelector('.msg.assistant', { timeout: 20000 });
    await p.waitForSelector('[data-testid="turn-status"]', { state: 'detached', timeout: 20000 });
    check((await said()) === '', 'nothing left to say');

    // 2. A message in the open chat; a reload during the wait shows it again.
    await sendText('slow: 1');
    await p.waitForSelector('.conversation .msg.user.sending', { timeout: 1000 });
    const chat = await shownAfter();
    check(chat != null && chat < 100, `chat: the message shows at once (${chat} ms)`);
    check((await status().innerText()).includes('is waiting for the model'), 'waiting from the click on');
    await p.screenshot({ path: `${S}/waiting-4-second-sending.png` });
    await p.waitForSelector('.conversation .msg.user.sending', { state: 'detached', timeout: 15000 });
    check((await bubbles('slow: 1').count()) === 2, 'two messages, none twice');
    await p.reload();
    await p.waitForSelector('[data-testid="turn-status"]', { timeout: 10000 });
    check((await status().innerText()).includes('is waiting for the model'), 'a reload shows the same wait');
    check((await bubbles('slow: 1').count()) === 2, 'and the same messages');
    log(`chat: shown ${Math.round(chat)} ms after the click; the reload shows the wait`);
    await p.waitForSelector('[data-testid="turn-status"]', { state: 'detached', timeout: 25000 });

    // 3. Refused: the text goes back to the box with why, no row stays.
    await p.unroute('**/api/sessions/*/prompt');
    await p.route('**/api/sessions/*/prompt', (r) => r.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'the node is not connected' }) }));
    await sendText('refused');
    await p.waitForSelector('.bottom .error-text:has-text("the node is not connected")', { timeout: 5000 });
    check((await box.inputValue()) === 'refused', 'the text is back in the box');
    check((await bubbles('refused').count()) === 0, 'no row stays');
    await p.screenshot({ path: `${S}/waiting-5-refused.png` });
    log('refused: back in the box with why');
    await box.fill('');

    // 4. A reload while the POST is held: the draft kept the message.
    await p.unroute('**/api/sessions/*/prompt');
    await p.route('**/api/sessions/*/prompt', async (r) => { await sleep(3000); await r.abort().catch(() => {}); });
    await sendText('kept over a reload');
    await sleep(300);
    check((await box.inputValue()) === '', 'the box is empty while it is sent');
    await p.reload();
    await p.waitForSelector('.conversation .composer textarea');
    await sleep(500);
    check((await box.inputValue()) === 'kept over a reload', `a reload mid-send finds it in the box (${await box.inputValue()})`);
    await box.fill('');
    log('reload mid-send: the message is in the box');

    // 5. Two tabs: tab B's message lands while tab A's is held; A's bubble stays.
    await p.unroute('**/api/sessions/*/prompt');
    await p.route('**/api/sessions/*/prompt', async (r) => { await sleep(3000); await r.continue(); });
    const tabB = await ctx.newPage();
    await tabB.goto(p.url());
    await tabB.waitForSelector('.conversation .composer textarea');
    await sendText('from tab A');
    await tabB.locator('.composer textarea').fill('from tab B');
    await tabB.click('.composer button.send:not(.stop)');
    await p.waitForSelector('.msg.user .bubble:text-is("from tab B")', { timeout: 10000 });
    check((await bubbles('from tab A').count()) === 1, "tab A's message stays while B's lands");
    await p.screenshot({ path: `${S}/waiting-6-two-tabs.png` });
    await p.waitForSelector('.conversation .msg.user.sending', { state: 'detached', timeout: 15000 });
    check((await bubbles('from tab A').count()) === 1 && (await bubbles('from tab B').count()) === 1, 'each once');
    await tabB.close();
    await p.waitForSelector('[data-testid="turn-status"]', { state: 'detached', timeout: 150000 });
    log('two tabs: each message once, none hidden');

    // 6. Home: the first message refused after the chat opened, then Edit; again, then Retry.
    const homeRefused = async (text) => {
      await p.unroute('**/api/sessions/*/prompt');
      await p.route('**/api/sessions/*/prompt', async (r) => { await sleep(1500); await r.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'the node is not connected' }) }); });
      await p.click('aside.sidebar a:has-text("New chat")');
      await p.waitForSelector('h1.greeting');
      await p.click('.mode-switch button:has-text("Ask")');
      await sendText(text);
      await p.waitForSelector('.msg.user.failed', { timeout: 10000 });
      check((await p.locator('.msg.user.failed').innerText()).includes('Not sent: the node is not connected.'), 'marked not sent, with why');
      check((await box.inputValue()) === text, 'and its text is in the box');
    };
    await homeRefused('home refused, then Edit');
    await p.screenshot({ path: `${S}/waiting-7-home-refused.png` });
    await box.fill('typed since');
    await p.click('.msg.user.failed button:text-is("Edit")');
    check((await p.locator('.msg.user.failed').count()) === 0, 'Edit: the bubble goes');
    check((await box.inputValue()) === 'home refused, then Edit\ntyped since', `Edit: ahead of what was typed (${await box.inputValue()})`);
    await p.screenshot({ path: `${S}/waiting-8-after-edit.png` });
    await box.fill('');
    await homeRefused('home refused, then Retry');
    await p.unroute('**/api/sessions/*/prompt');
    await p.click('.msg.user.failed button:text-is("Retry")');
    await p.waitForSelector('.msg.user.failed', { state: 'detached', timeout: 5000 });
    check((await box.inputValue()) === '', 'Retry: out of the box');
    await p.waitForSelector('.conversation .msg.user.sending', { state: 'detached', timeout: 15000 });
    check((await bubbles('home refused, then Retry').count()) === 1, 'Retry: sent, once');
    await p.screenshot({ path: `${S}/waiting-9-after-retry.png` });
    log('home refusal: Edit and Retry');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/waiting-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
