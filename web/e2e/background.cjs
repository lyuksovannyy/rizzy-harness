// T.8d step 2 end to end: a background command the agent started shows in
// the side panel beside the chat (its id, its command, time left), not above
// the composer, in a second tab too; a foreground command is a block in the
// conversation; the turn's Stop leaves the background one running and the
// panel stays; the panel's Stop ends it (stopped by the person) and its
// processes are gone; after a reload nothing is left to show. The demo provider's `script:` directive
// starts the command; the node runs it in its contained sandbox (needs
// bubblewrap), with the defaults (background commands are on).
// Usage: node web/e2e/background.cjs <path-to-harness-binary> [work-dir] [port]
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
const PORT = Number(process.argv[4] || 7786);
const URL_ = `http://127.0.0.1:${PORT}/`;
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
    check((await p.locator('.side-panel').count()) === 0, 'no panel before anything runs');
    const send = (calls) => p.evaluate(async ([id, text]) => {
      const r = await fetch(`/api/sessions/${id}/prompt`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ text }) });
      return r.status;
    }, [sid, `script: ${JSON.stringify(calls)}`]);
    check((await send([{ tool: 'bash', input: { command: `sleep ${BG}`, background: true, timeout_secs: 900 } }])) === 200, 'the prompt was taken');
    const row = '.side-panel .side-row[data-bg="bg-1"]';
    await p.waitForSelector(row, { timeout: 30000 });
    const text = (await p.innerText(row)).replace(/\s+/g, ' ');
    log('panel:', text);
    check((await p.locator('.bottom .side-row, .bottom .bg-strip').count()) === 0, 'nothing above the composer');
    check(text.includes('bg-1') && text.includes(`sleep ${BG}`), 'the row names the id and the command');
    check(/1[45] min left/.test(text), `the time left comes from the timeout (${text})`);
    check(sleeps(BG).length === 1, 'the command runs');
    await p.screenshot({ path: `${S}/bg-1-panel.png` });

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
    // The foreground command is a block in the conversation, open, from the
    // moment it starts; not in the side panel.
    const fg = `.timeline .tool.tool-running:has(.tool-summary:text-is("sleep ${FG}"))`;
    await p.waitForSelector(fg, { timeout: 15000 });
    check((await p.getAttribute(`${fg} .tool-head`, 'aria-expanded')) === 'true', 'the running command\'s block is open');
    check((await p.locator(`.side-panel :text("sleep ${FG}")`).count()) === 0, 'a foreground command is not in the side panel');
    await p.screenshot({ path: `${S}/bg-2-foreground.png` });
    log('the foreground command is a block in the chat');
    await p.click('.composer button.send.stop');
    await p.waitForSelector('.divider.r-cancelled', { timeout: 30000 });
    await sleep(500);
    check((await p.locator(row).count()) === 1, 'the panel keeps it after the turn\'s Stop');
    check(sleeps(BG).length === 1, 'the turn\'s Stop did not stop the background command');
    log('the turn\'s Stop left it running; the panel keeps it');

    // The panel's Stop.
    await p.click('.side-panel button[aria-label="Stop bg-1"]');
    log('after Stop:', (await p.innerText('.side-panel').catch(() => '(panel gone)')).replace(/\s+/g, ' '));
    await waitFor(() => sleeps(BG).length === 0, 'the command is still running after its stop', 5000);
    // The process is gone before the node reports it: `background_ended`
    // follows by up to a second, so poll for it.
    let ended = [];
    for (const until = Date.now() + 15000; Date.now() < until && ended.length === 0; await sleep(250)) {
      ended = await p.evaluate(async (id) => {
        const r = await fetch(`/api/sessions/${id}/events?after=0`);
        return (await r.json()).events.filter((e) => e.kind === 'background_ended').map((e) => e.payload.reason);
      }, sid);
    }
    check(JSON.stringify(ended) === '["stopped_by_user"]', `the end is the person's (${ended})`);
    // The timeline keeps the end; it started in an earlier turn, so the
    // panel lets it go once ended.
    await p.waitForSelector('.timeline details.divider summary:has-text("bg-1 ended (stopped by user")');
    await p.waitForSelector('.side-panel', { state: 'detached', timeout: 15000 });
    await p2.waitForSelector('.side-panel', { state: 'detached', timeout: 15000 });
    log('ended: stopped_by_user, processes gone, the panel left in both tabs');

    // A reload: nothing runs, nothing to show.
    await p.reload();
    await p.waitForSelector('.conversation .timeline details.divider');
    await sleep(500);
    check((await p.locator('.side-panel').count()) === 0, 'no panel after a reload');

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
