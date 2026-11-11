// Chats waiting for the user: shown under the bar in yellow, replaced by
// similar-chat results while typing, browser notification, cleared on open.
// Then a routine's result: its own row under the bar, a notification as the
// routine's `notify` asks, cleared once its chat is opened.
// Usage: node web/e2e/attention.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
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
/** The claim code once the server answers; a busy machine can take a while to start it. */
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
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7792'], { detached: true, stdio: 'ignore' });
  try {
    const code = await claimCode();
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 }, permissions: ['notifications'] });
    await keyed(ctx);
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
    await p.click('button:text-is("Continue")');
    await p.click('button:text-is("Finish")');
    await p.waitForSelector('h1.greeting');
    const home = async () => {
      await p.click('aside.sidebar a:has-text("New chat")');
      await p.waitForSelector('h1.greeting');
    };

    // Turn notifications on.
    await p.goto(`${URL_}#/settings/account/preferences`);
    await p.check('text=Desktop notifications');
    await home();
    await p.waitForSelector('h1.greeting');

    // A question arrives while the user is elsewhere.
    await p.fill('.composer textarea', 'shall we go to the tokio conference?');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    await home();
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
    await home();
    await p.waitForSelector('h1.greeting');
    await sleep(600);
    check((await p.locator('.attention').count()) === 0, 'seen question no longer waits');
    log('cleared after opening');

    // A routine's result: a row of its own, a notification, cleared once seen.
    const notesBefore = await p.evaluate(() => window.__notes.length);
    const routineId = await p.evaluate(async () => {
      const json = { 'content-type': 'application/json' };
      const spec = {
        profile: 'chat', agent_id: 'general', model: null, effort: null, prompt: 'good morning', project_id: null,
        catch_up: 'skip', on_missing: 'empty', notify: 'always', max_runtime_min: 120,
      };
      const triggers = [{ kind: 'schedule', config: { type: 'cron', expr: '30 8 * * 1-5', tz: 'UTC' } }];
      const made = await (await fetch('/api/routines', { method: 'POST', headers: json, body: JSON.stringify({ name: 'Morning check', enabled: true, spec, triggers }) })).json();
      await fetch(`/api/routines/${made.routine.routine_id}/run`, { method: 'POST', headers: json, body: '{}' });
      return made.routine.routine_id;
    });
    const result = '.routine-results button:has-text("Morning check · Finished")';
    await p.waitForSelector(result, { timeout: 20000 });
    log('routine result:', (await p.innerText('.routine-results')).replace(/\s+/g, ' '));
    await p.waitForFunction((n) => window.__notes.length > n, notesBefore);
    const note = await p.evaluate(() => window.__notes[window.__notes.length - 1]);
    check(note.t === 'Morning check' && note.body === 'Finished', `routine notification (${JSON.stringify(note)})`);
    await p.screenshot({ path: `${S}/a-2-routine.png` });
    await p.click(result);
    await p.waitForSelector('.conversation h1:has-text("Morning check")');
    await sleep(600);
    await home();
    await sleep(600);
    check((await p.locator('.routine-results').count()) === 0, 'the seen result leaves the list');
    const run = await p.evaluate(async (id) => (await (await fetch(`/api/routines/${id}/runs`)).json()).runs[0], routineId);
    check(run.seen === true, 'the run is marked seen on the server');
    log('routine result cleared after opening; run seen');

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
