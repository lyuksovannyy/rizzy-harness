// X.62 end to end: a background command that prints a line a second has its
// output opened from its row in the side panel (closed by default), the tail
// grows while it is open without a reload, terminal escapes and markup in it
// are only text, Hide stops the looking, and after the person's Stop the
// route still answers for the ended command and nothing keeps asking. The demo provider's `script:`
// directive starts the command in the node's contained sandbox (needs
// bubblewrap).
// Usage: node web/e2e/bg-output.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-bgout-')));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\nmax_approval_mode = "auto"\n`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});
let PAGE = null;

(async () => {
  const PORT = await freePort();
  const URL_ = `http://127.0.0.1:${PORT}/`;
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
    const reads = [];
    p.on('request', (r) => { if (/\/background\/bg-1\/output$/.test(r.url())) reads.push(Date.now()); });
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:text-is("Finish")');
    await p.waitForSelector('h1.greeting');

    const sid = await p.evaluate(async (cwd) => {
      const r = await fetch('/api/sessions', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ profile: 'agentic', cwd, title: 'Dev server', agent_id: 'coder', approval_mode: 'auto' }) });
      return (await r.json()).session_id;
    }, `${S}/ws`);
    await p.goto(`${URL_}#/s/${sid}`);
    await p.waitForSelector('.conversation');
    // A line a second, after one line of escapes and markup.
    const cmd = "printf '\\033[31mred\\033[0m <b>bold</b> <script>window.pwned=1</script>\\n'; i=0; while true; do i=$((i+1)); echo \"line $i\"; sleep 1; done";
    const status = await p.evaluate(async ([id, text]) => (await fetch(`/api/sessions/${id}/prompt`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ text }) })).status,
      [sid, `script: ${JSON.stringify([{ tool: 'bash', input: { command: cmd, background: true, timeout_secs: 900 } }])}`]);
    check(status === 200, 'the prompt was taken');
    const row = '.side-panel .side-row[data-bg="bg-1"]';
    await p.waitForSelector(row, { timeout: 30000 });

    // Closed by default: nothing was asked, no output shown.
    await sleep(1500);
    check(reads.length === 0, 'a closed row asks nothing: ' + reads.length);
    check((await p.locator(`${row} .side-output`).count()) === 0, 'closed by default');
    await p.screenshot({ path: `${S}/bgout-1-closed.png` });

    // Open it: the tail, read-only monospace, state and size.
    await p.click(`${row} button[aria-expanded]`);
    const pre = `${row} .side-output-text`;
    await p.waitForSelector(`${pre}:has-text("line 1")`, { timeout: 20000 });
    const first = await p.innerText(pre);
    check(await p.evaluate((s) => getComputedStyle(document.querySelector(s)).fontFamily.toLowerCase().includes('mono'), pre), 'monospace');
    check(first.includes('red <b>bold</b> <script>window.pwned=1</script>'), 'markup and escapes are text, escapes gone: ' + JSON.stringify(first));
    check(!first.includes('\u001b') && !first.includes('[31m'), 'no terminal escape: ' + JSON.stringify(first));
    check((await p.evaluate(() => window.pwned)) === undefined, 'nothing ran');
    check((await p.locator(`${row} script, ${row} b, ${row} a`).count()) === 0, 'no element came from the output');
    const state = (await p.innerText(`${row} .side-output [role=status]`)).trim();
    check(/running for \d+ s/.test(state), 'state: ' + state);
    log('open:', state, '|', first.split('\n').slice(0, 2).join(' / '));
    await p.screenshot({ path: `${S}/bgout-2-open.png` });

    // It grows while open, with no reload; a look about every 3 s.
    // (The row shows after the command's 3 s first wait: some lines are already there.)
    const seen = Number((first.trim().split('\n').pop().match(/line (\d+)/) || [])[1]);
    await p.waitForSelector(`${pre}:has-text("line ${seen + 3}")`, { timeout: 30000 });
    const grown = await p.innerText(pre);
    check(grown.length > first.length, 'the tail grew');
    check(reads.length >= 2 && reads.length < 20, 'looks while open: ' + reads.length);
    await p.screenshot({ path: `${S}/bgout-3-grown.png` });
    log('grew to', grown.trim().split('\n').pop(), `after ${reads.length} looks`);

    // Hide: no more looks.
    await p.click(`${row} button[aria-expanded]`);
    const n = reads.length;
    await sleep(7000);
    check(reads.length === n, 'a closed row stopped asking: ' + reads.length + ' vs ' + n);
    check((await p.locator(`${row} .side-output`).count()) === 0, 'hidden');

    // Open again, then the person's Stop. The strip lets an ended command go
    // from an earlier turn (its end is a divider in the conversation), so the
    // open row goes with it; the node still holds the output, and the route
    // answers for the ended command with its state.
    await p.click(`${row} button[aria-expanded]`);
    await p.waitForSelector(`${pre}:has-text("line ${seen + 5}")`, { timeout: 30000 });
    await p.screenshot({ path: `${S}/bgout-4-before-stop.png` });
    await p.click(`${row} button[aria-label="Stop bg-1"]`);
    await p.waitForSelector('.timeline details.divider summary:has-text("bg-1 ended")', { timeout: 30000 });
    const fin = await p.evaluate(async (id) => {
      const r = await fetch(`/api/sessions/${id}/background/bg-1/output`);
      return { status: r.status, body: await r.json() };
    }, sid);
    check(fin.status === 200 && fin.body.running === false && /line \d+\n$/.test(fin.body.text) && fin.body.ran_ms > 5000, 'an ended command is still readable: ' + JSON.stringify(fin).slice(0, 200));
    const gone = await p.evaluate(async (id) => (await fetch(`/api/sessions/${id}/background/bg-99/output`)).status, sid);
    check(gone === 409, 'a command the node does not have is a sentence (409), not a 500: ' + gone);
    const after = reads.length;
    await sleep(7000);
    check(reads.length <= after, 'nothing asks after the row is gone: ' + (reads.length - after));
    await p.screenshot({ path: `${S}/bgout-5-ended.png` });
    log('after Stop: the row is gone, the route answers ended after', fin.body.ran_ms, 'ms');

    // A narrow screen: the panel is a drawer; the output fits without sideways scroll.
    await p.setViewportSize({ width: 390, height: 800 });
    check(!(await p.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)), 'no horizontal scroll at phone width');
    await p.screenshot({ path: `${S}/bgout-6-narrow.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/bgout-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
