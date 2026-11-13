// T.8d step 3 end to end: a background command ends while its chat is
// idle and the agent is resumed without anyone writing (a callback turn);
// the chat shows what started it where a message would be, and the chat
// list shows the command running before. The command's output steers the
// demo provider into starting the same command again (the demo obeys the
// first quoted line of program output in a callback turn), so the chain
// runs into the node's budget: after three turns without the person the
// chat says the agent was not resumed, and why. Needs bubblewrap (the node
// contains the shell); every command is held on a file this script writes.
// Usage: node web/e2e/callbacks.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ecb-')));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmock_obeys_output = true\nmodels = [{ id = "mock" }]\n');
// The defaults: callback turns on, 3 in a row, 20 steps each.
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\nmax_approval_mode = "auto"\n`);
const PORT = 7893;
const URL_ = `http://127.0.0.1:${PORT}/`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

// Waits for `go` in the chat's folder, takes it, prints loop.txt (a line
// that starts it again) and fails.
const HELD = 'while [ ! -e go ]; do sleep 0.05; done; rm -f go; cat loop.txt; exit 1';
const START = [{ tool: 'bash', input: { command: HELD, background: true, timeout_secs: 300 } }];
fs.writeFileSync(`${S}/ws/loop.txt`, `script: ${JSON.stringify(START)}\n`);

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', `127.0.0.1:${PORT}`], { detached: true, stdio: ['ignore', 'ignore', fs.openSync(`${S}/serve.log`, 'w')] });
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

    const sid = await p.evaluate(async (cwd) => {
      const json = { 'content-type': 'application/json' };
      const r = await fetch('/api/sessions', { method: 'POST', headers: json, body: JSON.stringify({ profile: 'agentic', cwd, title: 'Nightly build', agent_id: 'coder', approval_mode: 'auto' }) });
      return (await r.json()).session_id;
    }, `${S}/ws`);
    check(typeof sid === 'string', 'the chat was created');
    const events = () => p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events?after=0&limit=5000`)).json()).events, sid);
    const count = async (kind) => (await events()).filter((e) => e.kind === kind).length;
    const until = async (ok, what, ms = 60000) => {
      const by = Date.now() + ms;
      while (!(await ok())) {
        if (Date.now() > by) throw new Error(what);
        await sleep(100);
      }
    };
    await p.goto(`${URL_}#/s/${sid}`);
    await p.waitForSelector('.conversation');
    const r = await p.evaluate(async ([id, text]) => (await fetch(`/api/sessions/${id}/prompt`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ text }) })).status, [sid, `script: ${JSON.stringify(START)}`]);
    check(r === 200, 'the prompt was taken');
    await until(async () => (await count('turn_ended')) === 1, 'the first turn did not end');
    // The chat list: a background command runs there, no turn.
    await p.waitForSelector(`a.session-item[href="#/s/${sid}"] .dot.background`, { timeout: 30000 });
    log('the chat list shows a background command running');

    // Three ends, three callback turns, each started by its end.
    for (let n = 1; n <= 3; n++) {
      await until(async () => (await count('background_started')) >= n, `bg-${n} did not start`);
      fs.writeFileSync(`${S}/ws/go`, '');
      await p.waitForSelector(`.divider.callback:has-text("Started by bg-${n} ended (exit 1), not by you")`, { timeout: 60000 });
      await until(async () => (await count('turn_ended')) === n + 1, `callback turn ${n} did not end`);
      log(`bg-${n} ended: the agent was resumed (callback turn ${n})`);
    }
    await p.screenshot({ path: `${S}/cb-1-resumed.png` });
    const users = await p.locator('.msg.user').count();
    check(users === 1, `only the person's own message reads as one (${users})`);

    // The fourth end in a row: not resumed, and the chat says why.
    await until(async () => (await count('background_started')) >= 4, 'bg-4 did not start');
    fs.writeFileSync(`${S}/ws/go`, '');
    const refused = '.msg.error:has-text("bg-4 ended (exit 1); the agent was not resumed: 3 turns in a row without you (node limit)")';
    await p.waitForSelector(refused, { timeout: 60000 });
    await sleep(1000);
    check((await p.locator('.divider.callback').count()) === 3, 'no fourth callback turn');
    log('bg-4 ended: not resumed, 3 turns in a row without you (node limit)');
    await p.screenshot({ path: `${S}/cb-2-refused.png` });

    // After a reload the chat says the same, from the log.
    await p.reload();
    await p.waitForSelector(refused, { timeout: 30000 });
    check((await p.locator('.divider.callback').count()) === 3, 'three callback turns after a reload');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/cb-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
