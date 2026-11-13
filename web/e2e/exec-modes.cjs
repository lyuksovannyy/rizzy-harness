// Execution modes against the real binary (N.10a): `serve`'s own node with
// `[execution] host = true`. The owner (a server admin) picks the host
// shell for a new agentic chat: the warning with its risks comes first;
// cancelling keeps the sandbox and creates nothing; confirming and sending
// creates a chat whose header says "host shell" and "not contained". A chat
// left at the sandbox says "sandbox".
// Usage: node web/e2e/exec-modes.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ex-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 50\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n\n[execution]\nhost = true\n`);
const URL_ = 'http://127.0.0.1:7794/';
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

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7794'], { detached: true, stdio: 'ignore' });
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
    const sessions = async () => p.evaluate(async () => (await (await fetch('/api/sessions')).json()).sessions.length);

    // What the node announced reaches the node list.
    const local = await p.evaluate(async () => (await (await fetch('/api/nodes')).json()).nodes.find((n) => n.node_id === 'local'));
    log('local node offers:', JSON.stringify(local.exec_modes), 'gaps:', JSON.stringify(local.exec_gaps));
    check(local.exec_modes.includes('host'), 'the node announces the host shell');
    const sandboxOffered = local.exec_modes.includes('sandbox');

    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.composer input.mono', `${S}/ws`);
    await p.waitForSelector(SELECT);
    check((await p.inputValue(SELECT)) === 'sandbox', 'the sandbox is the default');
    await p.screenshot({ path: `${S}/x-0-choice-light.png` });

    // Picking the host shell: the warning, with its risks, before anything.
    const before = await sessions();
    await p.selectOption(SELECT, 'host');
    await p.waitForSelector('dialog.host-warning[open]');
    await sleep(400);
    const text = await p.innerText('dialog.host-warning');
    for (const risk of [
      "as the node's own user, with nothing between them and the machine",
      'every file that user can, its keys and other projects included',
      'can keep running after the chat ends',
      'one web page or file the agent reads can turn all of that against you',
      'what a sandbox chat wrote in this folder',
      'The approval card is then the only check',
    ]) check(text.includes(risk), `the warning says: ${risk}`);
    await p.screenshot({ path: `${S}/x-1-warning-light.png` });
    log('warning shown with its risks');

    // Cancel: still the sandbox, nothing created.
    await p.click('dialog.host-warning button:text-is("Cancel")');
    await p.waitForSelector('dialog.host-warning', { state: 'detached' });
    check((await p.inputValue(SELECT)) === 'sandbox', 'cancel keeps the sandbox');
    await sleep(300);
    check((await sessions()) === before, 'cancel created nothing');
    log('cancel: sandbox kept, nothing created');

    // Confirm and send: a host chat, named so in its header.
    await p.selectOption(SELECT, 'host');
    await p.click('dialog.host-warning button:text-is("Use the host shell")');
    check((await p.inputValue(SELECT)) === 'host', 'confirmed');
    await p.fill('.composer textarea', 'hello from the host shell');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation .exec-mode');
    const head = (await p.innerText('.conv-meta')).replace(/\s+/g, ' ');
    log('host chat header:', head);
    check(head.includes('host shell'), 'the header names the host shell');
    check(head.includes('not contained'), 'the header says it is not contained');
    await p.screenshot({ path: `${S}/x-2-host-chat-light.png` });
    const hostHash = await p.evaluate(() => location.hash);
    const sid = hostHash.split('/').pop();
    const created = await p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events.find((e) => e.kind === 'session_created'), sid);
    check(created.payload.exec_mode === 'host', `recorded ${created.payload.exec_mode}`);

    // A chat left at the sandbox (where the node offers it here).
    if (sandboxOffered) {
      await p.click('aside.sidebar a:has-text("New chat")');
      await p.waitForSelector('h1.greeting');
      await p.click('.mode-switch button:has-text("Agentic")');
      await p.fill('.composer input.mono', `${S}/ws`);
      await p.fill('.composer textarea', 'hello from the sandbox');
      await p.keyboard.press('Enter');
      await p.waitForSelector('.conversation .exec-mode');
      const head = (await p.innerText('.conv-meta')).replace(/\s+/g, ' ');
      log('sandbox chat header:', head);
      check(head.includes('sandbox') && !head.includes('not contained'), 'the header names the sandbox');
    } else {
      log('SKIPPED the sandbox chat: bubblewrap does not work here');
    }

    // The same in the dark theme: the choice, the warning, the host chat's header.
    await p.emulateMedia({ colorScheme: 'dark' });
    await p.goto(`${URL_}${hostHash}`);
    await p.waitForSelector('.conversation .exec-mode');
    await p.screenshot({ path: `${S}/x-2-host-chat-dark.png` });
    await p.click('aside.sidebar a:has-text("New chat")');
    await p.waitForSelector('h1.greeting');
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.composer input.mono', `${S}/ws`);
    await p.waitForSelector(SELECT);
    await p.screenshot({ path: `${S}/x-0-choice-dark.png` });
    await p.selectOption(SELECT, 'host');
    await p.waitForSelector('dialog.host-warning[open]');
    await sleep(400);
    await p.screenshot({ path: `${S}/x-1-warning-dark.png` });
    await p.click('dialog.host-warning button:text-is("Cancel")');
    log('dark screenshots taken');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/x-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
