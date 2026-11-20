// The approval takes the composer's place (row W.6) against the real binary:
// an Agentic chat in Ask. Two parallel calls wait while a draft is being
// typed (continuously, across the moment the choice appears): the text field is replaced by the choice (the card above its
// buttons), no button has focus, Enter and Space typed at once answer
// nothing, Stop stays, and the buttons work after the guard; the calls come
// one at a time ("1 of 2"), the second guarded again; once answered the field
// is back with the draft and the caret where they were.
// Usage: node web/e2e/approval-in-composer.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2eac-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
// A slow mock, so a draft can be typed before the first call asks.
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 400\nmodels = [{ id = "mock" }]\n');
// The host shell is a fallback only where bubblewrap does not work here.
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n\n[execution]\nhost = true\n`);
const PORT = 7968;
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
    await pickFolder(p, `${S}/ws`);
    const local = await p.evaluate(async () => (await (await fetch('/api/nodes')).json()).nodes.find((n) => n.node_id === 'local'));
    if (!local.exec_modes.includes('sandbox')) {
      log('bubblewrap does not work here: the chat uses the host shell');
      await p.selectOption(SELECT, 'host');
      await p.click('dialog.host-warning button:text-is("Use the host shell")');
    }
    await p.waitForSelector(PICK);
    await choose(p, 'Ask');

    // 1. Two parallel calls; a draft is typed while the turn runs.
    await p.fill('.composer textarea', script([['bash', { command: 'touch first.txt' }], ['bash', { command: 'touch second.txt' }]]));
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    const sid = (await p.evaluate(() => location.hash)).split('/').pop();
    // The person types on, key after key, across the moment the choice
    // appears (no screenshot, no fixed wait: the loop ends when the choice's
    // element is there), then "/tmp/x" and Enter: a "/" that reached the page
    // would open search, an Enter that reached a button would answer.
    await p.focus('.composer textarea');
    let typed = 0;
    while (!(await p.$('.approval-choice'))) {
      check(typed++ < 400, 'the choice did not appear while typing');
      await p.keyboard.type('a', { delay: 25 });
    }
    check(typed > 0, 'the choice appeared before any key was typed: the burst did not span it');
    await p.keyboard.type('/tmp/x', { delay: 25 });
    await p.keyboard.press('Enter');
    await p.keyboard.type(' ', { delay: 25 });
    await p.screenshot({ path: `${S}/shots/choice.png` });

    // 2. The field is replaced; no button has focus; the burst answered nothing.
    check(await p.isHidden('.composer textarea'), 'the text field is replaced by the choice');
    check(await p.evaluate(() => document.activeElement?.classList.contains('approval-choice')), 'focus waits on the choice container, not a button');
    check(!(await p.$('dialog.search-panel[open]')), 'the typed "/" opened search');
    const draft = await p.inputValue('.composer textarea');
    check(/^a+$/.test(draft) && draft.length <= typed && draft.length >= typed - 1, 'the draft is what was typed before the choice, nothing after: ' + draft.length + ' of ' + typed);
    const shown = await p.innerText('.approval-choice');
    check(shown.includes('1 of 2'), 'the count is shown');
    check(shown.includes('first.txt'), 'the first call the node asked is shown, with what it runs');
    const now = await events(p, sid);
    check(now.every((e) => e.kind !== 'approval_resolved'), 'the burst answered an approval');
    check(now.filter((e) => e.kind === 'user_message').length === 1, 'the burst sent a message');
    check((await p.$$('.composer button.stop')).length === 1, 'Stop stays reachable');
    check((await p.$$('.tool-waiting button.primary')).length === 0, 'the transcript does not repeat the buttons');
    // The first Tab from the container reaches the first button.
    await p.keyboard.press('Tab');
    check(await p.evaluate(() => document.activeElement?.classList.contains('primary')), 'the first Tab reaches Allow once');
    await p.focus('.approval-choice');

    // 3. Answer the first; the second comes alone, guarded again.
    await p.click('.approval-choice button.primary');
    await p.waitForFunction(() => document.querySelector('.approval-choice')?.textContent.includes('second.txt'));
    check(!(await p.innerText('.approval-choice')).includes(' of '), 'one call left: no count');
    await p.keyboard.press('Enter');
    await sleep(150);
    check((await events(p, sid)).filter((e) => e.kind === 'approval_resolved').length === 1, 'the second is guarded as it appears');
    await p.screenshot({ path: `${S}/shots/second.png` });
    await p.click('.approval-choice button.danger');
    await turnsEnded(p, sid, 1);

    // 4. The field is back with the draft and the caret.
    await p.waitForSelector('.composer textarea', { state: 'visible' });
    check(!(await p.$('.approval-choice')), 'the choice is gone');
    const back = await p.evaluate(() => {
      const t = document.querySelector('.composer textarea');
      return { v: t.value, c: t.selectionStart, f: document.activeElement === t };
    });
    check(back.v === draft && back.c === draft.length && back.f, 'the draft, the caret and the focus are back: ' + JSON.stringify(back));
    check(exists('first.txt') && !exists('second.txt'), 'the first call ran, the denied second did not');

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
