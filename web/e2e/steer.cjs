// X.5 end to end: writing while the agent works. While a turn's command
// runs, the reply box stays open; a message sent with Enter is taken (no
// error), shows where it was sent marked "sent while working", leaves the
// running command alone, and is read after the command's result (the mark
// goes, the demo provider answers it). A message sent while the agent
// streams its last answer starts the next turn. The node runs the command
// in its contained sandbox (needs bubblewrap).
// Usage: node web/e2e/steer.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2esteer-')));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\nmax_approval_mode = "auto"\n`);
const URL_ = 'http://127.0.0.1:7964/';
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
const CMD = 965301;
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7964'], { detached: true, stdio: ['ignore', 'ignore', fs.openSync(`${S}/serve.log`, 'w')] });
  try {
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 } });
    await keyed(ctx);
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    const refused = [];
    p.on('response', (r) => { if (r.url().endsWith('/prompt') && r.status() !== 200) refused.push(r.status()); });
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:text-is("Finish")');
    await p.waitForSelector('h1.greeting');

    const sid = await p.evaluate(async (cwd) => {
      const json = { 'content-type': 'application/json' };
      const r = await fetch('/api/sessions', { method: 'POST', headers: json, body: JSON.stringify({ profile: 'agentic', cwd, title: 'Steer', agent_id: 'coder', approval_mode: 'auto' }) });
      return (await r.json()).session_id;
    }, `${S}/ws`);
    check(typeof sid === 'string', 'the chat was created');
    await p.goto(`${URL_}#/s/${sid}`);
    await p.waitForSelector('.conversation');
    const events = () => p.evaluate(async (id) => {
      const r = await fetch(`/api/sessions/${id}/events?after=0`);
      return (await r.json()).events.map((e) => ({ kind: e.kind, payload: e.payload }));
    }, sid);
    const box = p.locator('.composer textarea');
    const say = async (text) => {
      await box.fill(text);
      await box.press('Enter');
    };

    // A turn whose command runs for 6 s (it times out: nothing else ends it).
    await say(`script: ${JSON.stringify([{ tool: 'bash', input: { command: `sleep ${CMD}`, timeout_secs: 6 } }])}`);
    await waitFor(() => sleeps(CMD).length === 1, 'the turn\'s command did not start');
    check(await box.isEnabled(), 'the reply box stays open while the turn runs');
    check((await p.locator('.composer button.send.stop').count()) === 1, 'Stop is there');
    await say('also say hi');
    const waiting = p.locator('.msg.user.waiting');
    await waiting.waitFor({ timeout: 10000 });
    check((await waiting.locator('.bubble').innerText()) === 'also say hi', 'the message shows where it was sent');
    check((await waiting.locator('.queued-mark').innerText()) === 'sent while working', 'marked as sent while working');
    check(sleeps(CMD).length === 1, 'the message did not interrupt the running command');
    await p.screenshot({ path: `${S}/steer-1-waiting.png` });
    log('taken while the command runs, marked, the command untouched');

    // Read after the command's result: the mark goes and the agent answers.
    await p.waitForSelector('.msg.user.waiting', { state: 'detached', timeout: 30000 });
    await p.waitForSelector('.msg.assistant:has-text("You said: also say hi")', { timeout: 15000 });
    const ev1 = await events();
    const kinds = ev1.map((e) => e.kind);
    const at = kinds.indexOf('queued_delivered');
    check(at > kinds.indexOf('tool_call_finished'), `read after the tool's result (${kinds.join(',')})`);
    check(kinds.filter((k) => k === 'turn_started').length === 1, 'in the same turn');
    check(ev1.some((e) => e.kind === 'user_message' && e.payload.queued === true && e.payload.text === 'also say hi'), 'stored as a queued message');
    await p.screenshot({ path: `${S}/steer-2-read.png` });
    log('read at the next step:', kinds.slice(at - 1, at + 2).join(' → '));

    // A message while the last answer streams starts the next turn.
    // The turn is over: Stop leaves.
    await p.waitForSelector('.composer button.send.stop', { state: 'detached', timeout: 15000 });
    await say('slow: 400');
    await p.waitForSelector('.msg.assistant.streaming', { timeout: 10000 });
    await say('one more thing');
    await p.waitForSelector('.msg.assistant:has-text("You said: one more thing")', { timeout: 30000 });
    const ev2 = await events();
    const starts = ev2.filter((e) => e.kind === 'turn_started').length;
    check(starts === 3, `the message started the next turn (${starts} turns)`);
    const last = ev2.map((e) => e.kind).lastIndexOf('turn_started');
    check(ev2[last + 1].kind === 'queued_delivered', 'that turn starts by reading it');
    log('a message after the last step started the next turn');

    check(refused.length === 0, `no prompt was refused (${refused})`);
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    await p.screenshot({ path: `${S}/steer-3-next-turn.png` });
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/steer-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
