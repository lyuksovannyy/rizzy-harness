// T.8d step 2 end to end: a background command the agent started shows in
// the chat's strip (its id, its command, time left), in a second tab too;
// the turn's Stop leaves it running and the strip stays; the strip's Stop
// ends it (stopped by the person) and its processes are gone; after a
// reload nothing is left to show. The demo provider's `script:` directive
// starts the command; the node runs it in its contained sandbox (needs
// bubblewrap), with the defaults (background commands are on).
// Usage: node web/e2e/background.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ebg-')));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
// The node's defaults otherwise: background commands are on (8 a chat).
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\nmax_approval_mode = "auto"\n`);
const URL_ = 'http://127.0.0.1:7786/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };

/** Live processes whose command line is exactly `sleep <secs>`. */
function sleeps(secs) {
  const want = `sleep\0${secs}\0`;
  return fs.readdirSync('/proc').filter((p) => /^\d+$/.test(p)).filter((p) => {
    try {
      if (fs.readFileSync(`/proc/${p}/cmdline`, 'latin1') !== want) return false;
      const state = fs.readFileSync(`/proc/${p}/stat`, 'latin1').split(')').pop().trim()[0];
      return state !== 'Z' && state !== 'X';
    } catch {
      return false;
    }
  });
}
async function waitFor(ok, what, ms = 20000) {
  const until = Date.now() + ms;
  while (!ok()) {
    if (Date.now() > until) throw new Error(what);
    await sleep(50);
  }
}
const BG = 976201;
const FG = 976202;
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7786'], { detached: true, stdio: ['ignore', 'ignore', fs.openSync(`${S}/serve.log`, 'w')] });
  try {
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 } });
    await keyed(ctx);
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

    // An agentic chat with the Software Developer, in auto; its first turn
    // starts a background command.
    const sid = await p.evaluate(async (cwd) => {
      const json = { 'content-type': 'application/json' };
      const r = await fetch('/api/sessions', { method: 'POST', headers: json, body: JSON.stringify({ profile: 'agentic', cwd, title: 'Dev server', agent_id: 'coder', approval_mode: 'auto' }) });
      return (await r.json()).session_id;
    }, `${S}/ws`);
    check(typeof sid === 'string', 'the chat was created');
    await p.goto(`${URL_}#/s/${sid}`);
    await p.waitForSelector('.conversation');
    check((await p.locator('.bg-strip').count()) === 0, 'no strip before anything runs');
    const send = (calls) => p.evaluate(async ([id, text]) => {
      const r = await fetch(`/api/sessions/${id}/prompt`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ text }) });
      return r.status;
    }, [sid, `script: ${JSON.stringify(calls)}`]);
    check((await send([{ tool: 'bash', input: { command: `sleep ${BG}`, background: true, timeout_secs: 900 } }])) === 200, 'the prompt was taken');
    const row = '.bg-strip .bg-row[data-bg="bg-1"]';
    await p.waitForSelector(row, { timeout: 30000 });
    const text = (await p.innerText(row)).replace(/\s+/g, ' ');
    log('strip:', text);
    check(text.includes('bg-1') && text.includes(`sleep ${BG}`), 'the row names the id and the command');
    check(/1[45] min left/.test(text), `the time left comes from the timeout (${text})`);
    check(sleeps(BG).length === 1, 'the command runs');
    await p.screenshot({ path: `${S}/bg-1-strip.png` });

    // A second tab sees the same, from the log.
    const p2 = await ctx.newPage();
    p2.on('pageerror', (e) => errors.push(e.message));
    await p2.goto(`${URL_}#/s/${sid}`);
    await p2.waitForSelector(row, { timeout: 30000 });
    log('second tab shows it');

    // A turn that runs, and the turn's Stop: the background command keeps
    // running and the strip stays.
    check((await send([{ tool: 'bash', input: { command: `sleep ${FG}`, timeout_secs: 120 } }])) === 200, 'the second prompt was taken');
    await waitFor(() => sleeps(FG).length === 1, 'the turn\'s command did not start');
    await p.click('.composer button.send.stop');
    await p.waitForSelector('.divider.r-cancelled', { timeout: 30000 });
    await sleep(500);
    check((await p.locator(row).count()) === 1, 'the strip stays after the turn\'s Stop');
    check(sleeps(BG).length === 1, 'the turn\'s Stop did not stop the background command');
    log('the turn\'s Stop left it running; the strip stays');

    // The strip's Stop.
    await p.click('.bg-strip button[aria-label="Stop bg-1"]');
    await p.waitForSelector(`${row}:has-text("ended: stopped")`, { timeout: 30000 });
    log('after Stop:', (await p.innerText('.bg-strip')).replace(/\s+/g, ' '));
    await p.screenshot({ path: `${S}/bg-2-ended.png` });
    await waitFor(() => sleeps(BG).length === 0, 'the command is still running after its stop', 5000);
    const ended = await p.evaluate(async (id) => {
      const r = await fetch(`/api/sessions/${id}/events?after=0`);
      return (await r.json()).events.filter((e) => e.kind === 'background_ended').map((e) => e.payload.reason);
    }, sid);
    check(JSON.stringify(ended) === '["stopped_by_user"]', `the end is the person's (${ended})`);
    // The timeline keeps the end; the strip leaves.
    await p.waitForSelector('.timeline details.divider summary:has-text("bg-1 ended (stopped by user")');
    await p.waitForSelector('.bg-strip', { state: 'detached', timeout: 15000 });
    await p2.waitForSelector('.bg-strip', { state: 'detached', timeout: 15000 });
    log('ended: stopped_by_user, processes gone, the strip left in both tabs');

    // A reload: nothing runs, nothing to show.
    await p.reload();
    await p.waitForSelector('.conversation .timeline details.divider');
    await sleep(500);
    check((await p.locator('.bg-strip').count()) === 0, 'no strip after a reload');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/bg-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
