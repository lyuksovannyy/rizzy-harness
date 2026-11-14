// A chat's approval mode against the real binary (T48): node.toml allows
// `auto`; a new Agentic chat picked "Accept edits" in the composer writes a
// file without a card and asks for a command; switched to "Ask" in the open
// chat, the next edit asks; switched to "Auto" (a sandbox chat) a command
// runs unasked, or, in a host chat, Auto is shown disabled with the reason;
// a reload shows the mode from the log.
// Usage: node web/e2e/approval-mode.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2eam-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 50\nmodels = [{ id = "mock" }]\n');
// The host shell is a fallback only where bubblewrap does not work here.
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\nmax_approval_mode = "auto"\n\n[execution]\nhost = true\n`);
const PORT = 7966;
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
    await p.fill('.composer input.mono', `${S}/ws`);
    const local = await p.evaluate(async () => (await (await fetch('/api/nodes')).json()).nodes.find((n) => n.node_id === 'local'));
    log('node allows', local.max_approval, 'default', local.default_approval);
    check(local.max_approval === 'auto' && local.default_approval === 'ask', 'the node announces its modes');
    const host = !local.exec_modes.includes('sandbox');
    if (host) {
      log('bubblewrap does not work here: the chat uses the host shell');
      await p.selectOption(SELECT, 'host');
      await p.click('dialog.host-warning button:text-is("Use the host shell")');
    }
    // The new chat's picker: the node's default, three choices, one line each.
    await p.waitForSelector(PICK);
    check((await p.innerText(PICK)).includes('Ask'), 'a new chat starts at the default');
    await p.click(PICK);
    const box = await p.$eval('.approval-pick .select-list', (l) => l.getBoundingClientRect().toJSON());
    check(box.right <= 1280 && box.left >= 0, `the menu stays on screen: ${JSON.stringify(box)}`);
    const rows = await p.$$eval('.approval-pick li', (ls) => ls.map((l) => l.innerText.replace(/\s+/g, ' ')));
    log('choices:', rows.join(' | '));
    check(rows.length === 3, 'three choices');
    await sleep(400); // the menu fades in
    await p.screenshot({ path: `${S}/shots/approval-new-chat.png` });
    await p.keyboard.press('Escape');
    await choose(p, 'Accept edits');
    check((await p.innerText(PICK)).includes('Accept edits'), 'picked');

    // Accept edits: the write runs unasked, the command asks.
    await p.fill('.composer textarea', script([['write', { path: 'a.txt', content: 'x' }], ['bash', { command: 'touch b.txt' }]]));
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    const sid = (await p.evaluate(() => location.hash)).split('/').pop();
    await p.waitForSelector('.approval button.danger');
    check(exists('a.txt'), 'the edit ran without a card');
    const cards = await p.$$('.approval');
    check(cards.length === 1, `one card (the command), got ${cards.length}`);
    check((await p.innerText('.approval')).includes('bash'), 'the card is the command');
    await p.screenshot({ path: `${S}/shots/approval-accept-edits-card.png` });
    await p.click('.approval button.danger');
    await turnsEnded(p, sid, 1);
    check(!exists('b.txt'), 'the denied command did not run');
    let created = (await events(p, sid)).find((e) => e.kind === 'session_created');
    check(created.payload.approval_mode === 'accept_edits', `created in ${created.payload.approval_mode}`);

    // The open chat's picker: switch to Ask; the next edit asks.
    await p.waitForSelector(PICK);
    check((await p.innerText(PICK)).includes('Accept edits'), 'the open chat shows its mode');
    await choose(p, 'Ask');
    await p.waitForSelector('.divider:has-text("Approval mode: ask")');
    await p.fill('.composer textarea', script([['write', { path: 'c.txt', content: 'x' }]]));
    await p.keyboard.press('Enter');
    await p.waitForSelector('.approval button.danger');
    await p.click('.approval button.danger');
    await turnsEnded(p, sid, 2);
    check(!exists('c.txt'), 'under Ask the edit asked and was denied');

    await p.click(PICK);
    const auto = await p.$eval('.approval-pick li:has(.option-label:text-is("Auto"))', (l) => ({ off: l.getAttribute('aria-disabled') === 'true', text: l.innerText }));
    await sleep(400);
    await p.screenshot({ path: `${S}/shots/approval-open-chat-menu.png` });
    await p.keyboard.press('Escape');
    if (host) {
      check(auto.off && auto.text.includes('host chat'), `a host chat never offers auto: ${auto.text}`);
    } else {
      check(!auto.off, 'a sandbox chat on this node may run in auto');
      await choose(p, 'Auto');
      await p.waitForSelector('.divider:has-text("Approval mode: auto")');
      await p.fill('.composer textarea', script([['bash', { command: 'touch d.txt' }]]));
      await p.keyboard.press('Enter');
      await turnsEnded(p, sid, 3);
      check(exists('d.txt'), 'under Auto the command ran unasked');
      check((await p.$$('.approval button.danger')).length === 0, 'no card');
    }

    // A reload folds the log: the same mode.
    await p.emulateMedia({ colorScheme: 'dark' });
    await p.reload();
    // Once the whole log is folded (its last change is on screen).
    await p.waitForSelector(`.divider:has-text("Approval mode: ${host ? 'ask' : 'auto'}")`);
    await p.waitForSelector(PICK);
    const after = await p.innerText(PICK);
    log('after a reload:', after);
    check(after.includes(host ? 'Ask' : 'Auto'), 'the mode from the log');
    await sleep(300);
    await p.screenshot({ path: `${S}/shots/approval-after-reload-dark.png` });

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
