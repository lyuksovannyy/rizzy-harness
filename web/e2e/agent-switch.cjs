// X.28 end to end: switching a chat's agent. While a turn runs the agent
// picker is disabled (the node would refuse the change); Stop ends the turn
// and the picker is back. Picking agents writes nothing to the chat; the
// next message shows "Switched to <agent> · <model>" above it, once.
// Usage: node web/e2e/agent-switch.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2eswitch-')));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
const URL_ = 'http://127.0.0.1:7966/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7966'], { detached: true, stdio: ['ignore', 'ignore', fs.openSync(`${S}/serve.log`, 'w')] });
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

    const sid = await p.evaluate(async () => {
      const r = await fetch('/api/sessions', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ profile: 'chat', title: 'Switch' }) });
      return (await r.json()).session_id;
    });
    check(typeof sid === 'string', 'the chat was created');
    await p.goto(`${URL_}#/s/${sid}`);
    await p.waitForSelector('.conversation');
    const events = () => p.evaluate(async (id) => {
      const r = await fetch(`/api/sessions/${id}/events?after=0`);
      return (await r.json()).events.map((e) => ({ kind: e.kind, payload: e.payload }));
    }, sid);
    const box = p.locator('.composer textarea');
    const picker = p.locator('.agent-pick button.select-button');
    const say = async (text) => {
      await box.fill(text);
      await box.press('Enter');
    };
    const pick = async (name) => {
      await picker.click();
      await p.locator('.agent-pick [role="option"]', { hasText: name }).first().click();
    };

    // A long answer: the picker is disabled from the send until the end.
    check(await picker.isEnabled(), 'the picker is enabled before anything runs');
    await say('slow: 4000');
    await p.waitForSelector('.composer button.send.stop', { timeout: 10000 });
    await p.waitForSelector('.msg.assistant.streaming', { timeout: 10000 });
    check(await picker.isDisabled(), 'the picker is disabled while the agent works');
    check((await p.locator('.agent-pick').getAttribute('title')) === 'Stop the agent to pick another', 'it says why');
    // A change sent anyway is refused by the node: 409.
    const status = await p.evaluate(async (id) => (await fetch(`/api/sessions/${id}/agent`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ agent_id: 'writer' }) })).status, sid);
    check(status === 409, `the server refuses a switch while working (${status})`);
    await p.screenshot({ path: `${S}/switch-1-working.png` });
    log('disabled while working; the server answered', status);

    await p.click('.composer button.send.stop');
    await p.waitForSelector('.composer button.send.stop', { state: 'detached', timeout: 15000 });
    await p.waitForFunction(() => !document.querySelector('.agent-pick button.select-button')?.disabled, null, { timeout: 15000 });
    log('enabled again after Stop');

    // Picking agents adds nothing to the chat.
    const before = (await events()).length;
    await pick('Writer');
    await pick('Researcher');
    await pick('Writer');
    await p.waitForFunction(() => document.querySelector('.agent-pick button.select-button')?.getAttribute('aria-label') === 'agent: Writer', null, { timeout: 10000 });
    await sleep(500);
    const after = await events();
    check(after.length === before, `picking wrote nothing (${before} → ${after.length}: ${after.slice(before).map((e) => e.kind)})`);
    check((await p.locator('.divider', { hasText: 'Switched to' }).count()) === 0, 'no notice before a message');
    log('three picks, no event');

    // The next message: one notice above it, naming the agent and its model.
    await say('hello');
    await p.waitForSelector('.msg.assistant:has-text("You said: hello")', { timeout: 30000 });
    const notice = p.locator('.divider', { hasText: 'Switched to' });
    await notice.first().waitFor({ timeout: 10000 });
    check((await notice.count()) === 1, 'one notice');
    const said = await notice.innerText();
    check(said.startsWith('Switched to Writer · '), `the notice names agent and model: ${said}`);
    const evs = await events();
    const at = evs.findIndex((e) => e.kind === 'agent_changed');
    check(at > 0 && evs[at + 1].kind === 'turn_started' && evs[at + 2].kind === 'user_message' && evs[at + 2].payload.text === 'hello', 'recorded right before the message\'s turn');
    await p.screenshot({ path: `${S}/switch-2-notice.png` });
    log('notice:', said);

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/switch-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
