// A denial's note and "allow for this chat" against the real binary (T51):
// an Agentic chat in Ask; a command's card is denied with a note, which the
// card then shows and the agent's tool result carries, quoted; the next
// command is allowed "for this chat", listed in the side panel, runs again
// without a card, and asks again once removed there.
// Usage: node web/e2e/approval-answers.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2eaa-'));
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

async function choose(p, label) {
  await p.click(PICK);
  await p.click(`.approval-pick li:has(.option-label:text-is("${label}"))`);
}

async function events(p, sid) {
  return p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events, sid);
}

async function turnsEnded(p, sid, n) {
  for (let i = 0; i < 300; i++) {
    if ((await events(p, sid)).filter((e) => e.kind === 'turn_ended').length >= n) return;
    await sleep(100);
  }
  throw new Error(`turn ${n} did not end`);
}

async function lastCard(p) {
  await p.waitForSelector('.approval button.danger');
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
    if (!local.exec_modes.includes('sandbox')) {
      log('bubblewrap does not work here: the chat uses the host shell');
      await p.selectOption(SELECT, 'host');
      await p.click('dialog.host-warning button:text-is("Use the host shell")');
    }
    await p.waitForSelector(PICK);
    await choose(p, 'Ask');

    // 1. Deny with a note.
    await p.fill('.composer textarea', script([['bash', { command: 'touch denied.txt' }]]));
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    const sid = (await p.evaluate(() => location.hash)).split('/').pop();
    await lastCard(p);
    const focused = await p.evaluate(() => document.activeElement?.classList.contains('deny-note'));
    check(!focused, 'the note field is not focused by itself');
    await p.fill('.approval input.deny-note', 'keep the folder clean; write to /tmp instead');
    await p.press('.approval input.deny-note', 'Enter');
    await sleep(300);
    check((await p.$$('.approval button.danger')).length === 1, 'Enter in the note answered nothing');
    await p.screenshot({ path: `${S}/shots/answers-note-typed.png` });
    await p.click('.approval button.danger');
    await turnsEnded(p, sid, 1);
    await p.waitForSelector('text=Denied with your note');
    check(!exists('denied.txt'), 'the denied command did not run');
    const out = (await events(p, sid)).filter((e) => e.kind === 'tool_call_finished').pop().payload.output;
    log('the agent read:', out.replace(/\n/g, ' / '));
    check(out.includes('\n| keep the folder clean; write to /tmp instead'), 'the note reached the agent, quoted');

    // 2. Allow for this chat; listed beside the chat; Remove.
    const cmd = script([['bash', { command: 'echo granted' }]]);
    await p.fill('.composer textarea', cmd);
    await p.keyboard.press('Enter');
    await lastCard(p);
    await p.click('.approval button:has-text("Allow this exact call for this chat")');
    await turnsEnded(p, sid, 2);
    await p.waitForSelector('[data-kind="grant"]');
    check((await p.innerText('[data-kind="grant"]')).includes('echo granted'), 'the side panel lists the grant');
    await p.screenshot({ path: `${S}/shots/answers-grant-listed.png` });
    await p.fill('.composer textarea', cmd);
    await p.keyboard.press('Enter');
    await turnsEnded(p, sid, 3);
    check((await p.$$('.approval button.danger')).length === 0, 'the granted call ran without a card');
    await p.click('[data-kind="grant"] button:has-text("Remove")');
    await p.waitForFunction(() => !document.querySelector('[data-kind="grant"]'));
    await p.fill('.composer textarea', cmd);
    await p.keyboard.press('Enter');
    await lastCard(p);
    await p.click('.approval button.danger');
    await turnsEnded(p, sid, 4);
    log('removed: the call asks again');

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
