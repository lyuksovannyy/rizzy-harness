// A chat's step limit against the real binary (X.14): the node's limit is
// 1,000 (the owner's default, 2026-11-15); a value out of range is refused
// with the reason before sending; set to 2, a five-step task runs two steps
// and ends with "step limit reached"; raised to 10, it carries on.
// Usage: node web/e2e/step-limit.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2esl-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 50\nmodels = [{ id = "mock" }]\n');
// The host shell is a fallback only where bubblewrap does not work here.
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n\n[execution]\nhost = true\n`);
const PORT = 7967;
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
const SELECT = 'select[aria-label="How this chat\'s commands run"]';
const PICK = '.approval-pick .select-button';
fs.mkdirSync(`${S}/shots`, { recursive: true });
const script = (calls) => `script: ${JSON.stringify(calls.map(([tool, input]) => ({ tool, input })))}`;
const exists = (f) => fs.existsSync(path.join(S, 'ws', f));

async function events(p, sid) {
  return p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events, sid);
}

async function turnsEnded(p, sid, n) {
  for (let i = 0; i < 600; i++) {
    const ends = (await events(p, sid)).filter((e) => e.kind === 'turn_ended');
    if (ends.length >= n) return ends;
    await sleep(100);
  }
  throw new Error(`turn ${n} did not end`);
}

async function setLimit(p, n) {
  await p.click('.step-pick > button');
  await p.fill('.step-pop input', String(n));
  await p.click('.step-pop button:text-is("Set")');
}

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', `127.0.0.1:${PORT}`], { detached: true, stdio: 'ignore' });
  try {
    const code = await claimCode();
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
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.composer input.mono', `${S}/ws`);
    const local = await p.evaluate(async () => (await (await fetch('/api/nodes')).json()).nodes.find((n) => n.node_id === 'local'));
    log('node max_steps', local.max_steps);
    check(local.max_steps === 1000, `the node's limit is 1000: ${local.max_steps}`);
    if (!local.exec_modes.includes('sandbox')) {
      log('bubblewrap does not work here: the chat uses the host shell');
      await p.selectOption(SELECT, 'host');
      await p.click('dialog.host-warning button:text-is("Use the host shell")');
    }
    // A first message makes the chat; then the limit is set to 2.
    await p.fill('.composer textarea', 'hello');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    const sid = (await p.evaluate(() => location.hash)).split('/').pop();
    await turnsEnded(p, sid, 1);
    await p.waitForSelector('.step-pick > button');
    check((await p.innerText('.step-pick > button')).includes('1000 steps'), 'the chat starts at the node limit');
    // Out of range: refused with the reason before anything is sent.
    await p.click('.step-pick > button');
    await p.fill('.step-pop input', '5000');
    check(await p.$eval('.step-pop button:text-is("Set")', (b) => b.disabled), 'out of range cannot be sent');
    check((await p.innerText('.step-pop')).includes('From 1 to 1000'), 'the reason is shown');
    await p.screenshot({ path: `${S}/shots/step-limit-refused.png` });
    await p.keyboard.press('Escape');
    await setLimit(p, 2);
    await p.waitForSelector('.divider:has-text("Step limit: 2")');
    // A task of five steps: two run, then the wrap-up.
    const many = script([1, 2, 3, 4, 5].map((i) => ['bash', { command: `touch s${i}.txt` }]));
    await p.fill('.composer textarea', many);
    await p.keyboard.press('Enter');
    let ends = await turnsEnded(p, sid, 2);
    check(ends[1].payload.reason === 'limit', `the turn ended at the limit: ${ends[1].payload.reason}`);
    await p.waitForSelector('.divider:has-text("step limit reached")');
    check(exists('s1.txt') && exists('s2.txt') && !exists('s3.txt'), 'two steps ran');
    await p.screenshot({ path: `${S}/shots/step-limit-reached.png` });
    // Raised, the task carries on.
    await setLimit(p, 10);
    await p.waitForSelector('.divider:has-text("Step limit: 10")');
    await p.fill('.composer textarea', many);
    await p.keyboard.press('Enter');
    ends = await turnsEnded(p, sid, 3);
    check(ends[2].payload.reason === 'completed', `carried on: ${ends[2].payload.reason}`);
    check(exists('s5.txt'), 'all five ran');
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', `${S}/shots`);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/x-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
