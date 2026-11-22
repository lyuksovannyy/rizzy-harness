// The owner's switch of a chat's execution mode against the real binary
// (N.10b): `serve`'s own node with `[execution] host = true`. In an idle
// chat the strip's "Commands" control switches to the host shell behind
// the warning (cancel leaves the mode; confirm switches), the chat shows a
// divider and its header says "host shell", a reload keeps it, and the
// switch back to the sandbox needs no warning. The control is disabled,
// with its reason, while a turn runs and while an approval waits. On a
// node without the host shell the new-chat label says how to turn it on.
// Usage: node web/e2e/exec-switch.cjs <path-to-harness-binary> [work-dir] [port]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2esw-'));
const PORT = Number(process.argv[4] || 7795);
const URL_ = `http://127.0.0.1:${PORT}/`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const script = (calls) => `script: ${JSON.stringify(calls.map(([tool, input]) => ({ tool, input })))}`;
let PAGE = null;
const CONTROL = '.exec-switch .select-button';
const PICK = '.approval-pick .select-button';

function layout(name, host) {
  const d = `${S}/${name}`;
  fs.mkdirSync(`${d}/data`, { recursive: true });
  fs.mkdirSync(`${d}/ws`, { recursive: true });
  fs.mkdirSync(`${d}/shots`, { recursive: true });
  // A slow mock, so a turn can be looked at while it runs.
  fs.writeFileSync(`${d}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 1200\nmodels = [{ id = "mock" }]\n');
  fs.writeFileSync(`${d}/data/node.toml`, `workspace_roots = ["${d}/ws"]\n\n[execution]\nhost = ${host}\n`);
  return d;
}

async function serve(d, port) {
  const srv = spawn(BIN, ['--data-dir', `${d}/data`, 'serve', '--bind', `127.0.0.1:${port}`], { detached: true, stdio: 'ignore' });
  const url = `http://127.0.0.1:${port}/`;
  for (let i = 0; i < 600; i++) {
    try {
      await fetch(url);
      return { srv, url, code: fs.readFileSync(`${d}/data/claim-code`, 'utf8').trim() };
    } catch {
      await sleep(100);
    }
  }
  throw new Error('the server did not start within a minute');
}

async function onboard(p, url, code) {
  await p.goto(url);
  await createOwner(p, code);
  await p.click('text=Skip: use the built-in demo');
  await keywordsOnly(p);
  await p.click('button:text-is("Continue")');
  await p.click('button:text-is("Finish")');
  await p.waitForSelector('h1.greeting');
}

const events = (p, sid) => p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events, sid);
async function turnsEnded(p, sid, n) {
  for (let i = 0; i < 300; i++) {
    if ((await events(p, sid)).filter((e) => e.kind === 'turn_ended').length >= n) return;
    await sleep(100);
  }
  throw new Error(`turn ${n} did not end`);
}
const changes = async (p, sid) => (await events(p, sid)).filter((e) => e.kind === 'exec_mode_changed').map((e) => `${e.payload.from}>${e.payload.mode}`);
const label = (p) => p.getAttribute(CONTROL, 'aria-label');
async function choose(p, word) {
  await p.click(CONTROL);
  await p.click(`.exec-switch li:has-text("${word}")`);
}

(async () => {
  const d1 = layout('on', true);
  const one = await serve(d1, PORT);
  const servers = [one.srv];
  try {
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1440, height: 900 } });
    await keyed(ctx);
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await onboard(p, one.url, one.code);
    const local = await p.evaluate(async () => (await (await fetch('/api/nodes')).json()).nodes.find((n) => n.node_id === 'local'));
    check(local.exec_switch === true, 'the node announces the switch');
    check(local.exec_modes.includes('sandbox'), 'this check needs the sandbox (bubblewrap) on this machine');

    // An agentic chat, started in the sandbox; one turn; then idle.
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, `${d1}/ws`);
    await p.fill('.composer textarea', 'hello');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation .exec-mode');
    const sid = (await p.evaluate(() => location.hash)).split('/').pop();
    await turnsEnded(p, sid, 1);
    await p.waitForSelector(CONTROL);
    check((await label(p)) === 'Commands: sandbox', `the control says sandbox: ${await label(p)}`);
    check(!(await p.isDisabled(CONTROL)), 'idle: the control works');
    await p.screenshot({ path: `${d1}/shots/1-idle-1440.png` });
    log('idle chat: the control reads', await label(p));

    // Host: the warning first, with what Auto means; cancel leaves it.
    await choose(p, 'host shell');
    await p.waitForSelector('dialog.host-warning[open]');
    await sleep(400);
    const text = await p.innerText('dialog.host-warning');
    check(text.includes("Switch this chat's commands to the host shell?"), 'the warning is for a switch');
    check(text.includes('This chat is in Auto: from its next turn every command runs in the host shell with no question asked'), 'the warning says what Auto means');
    check(text.includes('does not carry over'), 'the warning says the grants do not carry over');
    await p.screenshot({ path: `${d1}/shots/2-warning-1440.png` });
    await p.click('dialog.host-warning button:text-is("Cancel")');
    await p.waitForSelector('dialog.host-warning', { state: 'detached' });
    check((await label(p)) === 'Commands: sandbox', 'cancel leaves the sandbox');
    check((await changes(p, sid)).length === 0, 'cancel recorded nothing');
    log('cancel: sandbox kept, nothing recorded');

    // Confirm: recorded, a divider, the header, and it survives a reload.
    await choose(p, 'host shell');
    await p.click('dialog.host-warning button:text-is("Switch to the host shell")');
    await p.waitForSelector('[data-testid=exec-mode-divider]');
    check((await p.innerText('[data-testid=exec-mode-divider]')).includes('Commands run on the host shell from here'), 'the divider says so');
    check((await label(p)) === 'Commands: host shell', `the control follows the log: ${await label(p)}`);
    check((await p.innerText('.chat-suffix')).includes('host shell'), 'the header follows the log');
    check(JSON.stringify(await changes(p, sid)) === '["sandbox>host"]', `recorded ${await changes(p, sid)}`);
    await p.screenshot({ path: `${d1}/shots/3-host-1440.png` });
    await p.reload();
    await p.waitForSelector(CONTROL);
    check((await label(p)) === 'Commands: host shell', 'a reload keeps the switched mode');
    check((await p.innerText('.chat-suffix')).includes('host shell'), 'a reload: the header');
    log('switched to the host shell; a reload keeps it');

    // A turn runs: the control is disabled and says why.
    // A command that lasts: the demo model's own turn is over before a click lands.
    await p.fill('.composer textarea', script([['bash', { command: 'sleep 4' }]]));
    await p.keyboard.press('Enter');
    await p.waitForSelector(`${CONTROL}[aria-disabled="true"]`, { timeout: 5000 });
    const why = await p.getAttribute('.exec-switch', 'title');
    check(why && why.includes('Stop the agent'), `the reason: ${why}`);
    // Nothing moves until it is asked for; a click says why, in words, over the controls.
    check(!(await p.$('[data-testid=exec-busy-note]')), 'no note before a click');
    // Forced: Playwright waits for an aria-disabled control to be enabled, a person does not.
    await p.click(CONTROL, { force: true });
    await p.waitForSelector('[data-testid=exec-busy-note]');
    check((await p.innerText('[data-testid=exec-busy-note]')).includes('Stop the agent'), 'the note says why');
    await p.screenshot({ path: `${d1}/shots/4-running-1440.png` });
    await turnsEnded(p, sid, 2);
    await p.waitForSelector(`${CONTROL}:not([aria-disabled])`);
    log('while a turn runs: disabled with its reason;', why);

    // An approval waits (Ask): disabled again.
    await p.click(PICK);
    await p.click('.approval-pick li:has(.option-label:text-is("Ask"))');
    await p.fill('.composer textarea', script([['bash', { command: 'echo asked' }]]));
    await p.keyboard.press('Enter');
    await p.waitForSelector('.approval-choice', { timeout: 10000 });
    check((await p.getAttribute(CONTROL, 'aria-disabled')) === 'true', 'an approval waits: disabled');
    const why2 = await p.getAttribute('.exec-switch', 'title');
    check(why2 && (why2.includes('Answer the approval') || why2.includes('Stop the agent')), `the reason: ${why2}`);
    // Forced: Playwright waits for an aria-disabled control to be enabled, a person does not.
    await p.click(CONTROL, { force: true });
    await p.waitForSelector('[data-testid=exec-busy-note]');
    const note2 = await p.innerText('[data-testid=exec-busy-note]');
    check(note2.includes('Answer the approval'), `while an approval waits the reason is the approval: ${note2}`);
    await p.screenshot({ path: `${d1}/shots/5-approval-1440.png` });
    await p.setViewportSize({ width: 390, height: 844 });
    await sleep(300);
    await p.screenshot({ path: `${d1}/shots/5-approval-390.png` });
    await p.setViewportSize({ width: 1440, height: 900 });
    await p.click('.approval-choice button.primary');
    await turnsEnded(p, sid, 3);
    await p.waitForSelector(`${CONTROL}:not([aria-disabled])`);
    log('while an approval waits: disabled with its reason;', why2);

    // The sandbox again: no warning, a divider.
    await choose(p, 'sandbox');
    await p.waitForFunction(() => document.querySelectorAll('[data-testid=exec-mode-divider]').length === 2);
    check(!(await p.$('dialog.host-warning[open]')), 'no warning back to the sandbox');
    check((await label(p)) === 'Commands: sandbox', 'back in the sandbox');
    const dividers = await p.$$eval('[data-testid=exec-mode-divider]', (els) => els.map((e) => e.textContent));
    check(dividers[1].includes('Commands run in the sandbox from here'), 'the second divider');
    check(JSON.stringify(await changes(p, sid)) === '["sandbox>host","host>sandbox"]', `recorded ${await changes(p, sid)}`);
    await p.screenshot({ path: `${d1}/shots/6-back-1440.png` });

    // A fork of the second turn (in the host shell), made after the chat came back, is in the mode its chat has now.
    // Turn 2 ran in the host shell; the chat is in the sandbox again.
    const first = (await events(p, sid)).filter((e) => e.kind === 'turn_ended')[1].payload.turn_id;
    const forked = await p.evaluate(async ([id, t]) => (await (await fetch(`/api/sessions/${id}/fork`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ turn_id: t }) })).json()).session_id, [sid, first]);
    await p.goto(`${URL_}#/s/${forked}`);
    await p.waitForSelector(CONTROL);
    check((await label(p)) === 'Commands: sandbox', `the fork follows its parent's mode: ${await label(p)}`);
    check((await p.innerText('.chat-suffix')).includes('sandbox'), 'the fork header');
    check((await changes(p, forked)).slice(-1)[0] === 'host>sandbox', `the fork's own change: ${await changes(p, forked)}`);
    await p.screenshot({ path: `${d1}/shots/6b-fork-1440.png` });
    log('fork of the first turn after the switches:', await label(p), JSON.stringify(await changes(p, forked)));
    await p.goto(`${URL_}#/s/${sid}`);
    await p.waitForSelector(CONTROL);

    // The narrow screen.
    await p.setViewportSize({ width: 390, height: 844 });
    await sleep(400);
    check((await p.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)) <= 0, 'no sideways scroll at 390');
    await p.screenshot({ path: `${d1}/shots/7-back-390.png` });
    await p.click(CONTROL);
    await sleep(300);
    await p.screenshot({ path: `${d1}/shots/8-open-390.png` });
    await p.keyboard.press('Escape');
    await p.setViewportSize({ width: 1440, height: 900 });

    // A node without the host shell: the new-chat label says how to turn it on.
    const d2 = layout('off', false);
    const two = await serve(d2, PORT + 1);
    servers.push(two.srv);
    const c2 = await b.newContext({ viewport: { width: 1440, height: 900 } });
    await keyed(c2);
    const q = await c2.newPage();
    q.on('pageerror', (e) => errors.push(e.message));
    await onboard(q, two.url, two.code);
    await q.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(q, `${d2}/ws`);
    await q.waitForSelector('.exec-hint');
    const hint = await q.innerText('.exec-hint');
    check(hint.includes('the host shell is off on this node') && hint.includes('host = true') && hint.includes('[execution]') && hint.includes('node.toml'), `the hint: ${hint}`);
    await q.screenshot({ path: `${d2}/shots/9-hint-1440.png` });
    await q.setViewportSize({ width: 390, height: 844 });
    await sleep(300);
    await q.screenshot({ path: `${d2}/shots/10-hint-390.png` });
    log('node without the host shell: the new-chat label says how to turn it on');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/fail.png` }).catch(() => {});
    throw e;
  } finally {
    for (const s of servers) {
      try {
        process.kill(-s.pid, 'SIGKILL');
      } catch {}
    }
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
