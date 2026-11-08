// Routines end to end: create an agentic routine, Run now, find its chat in
// "Waiting for you", approve the command, see the result row under the bar,
// open it, and see the row clear; the Runs tab shows the finished run.
// Usage: node web/e2e/routines.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2er-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
// A slow mock, so the run is still going when the page leaves its chat.
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 400\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n`);
const URL_ = 'http://127.0.0.1:7794/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7794'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 } });
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
    const home = async () => {
      await p.click('aside.sidebar a:has-text("New chat")');
      await p.waitForSelector('h1.greeting');
    };

    // Create: the sidebar entry, the editor, an agentic routine that runs a command.
    await p.click('aside.sidebar a:has-text("Routines")');
    await p.waitForSelector('h1:has-text("Routines")');
    await p.click('button:has-text("New routine")');
    await p.fill('form input[placeholder="Morning briefing"]', 'Echo check');
    await p.click('[role=radiogroup][aria-label="Runs as"] [role=radio]:has-text("Agentic")');
    await p.selectOption('select:has(option:has-text("Pick a node"))', { label: 'local' });
    await p.fill('input[placeholder="/home/me/project"]', `${S}/ws`);
    await p.fill('textarea[aria-label="Prompt"]', 'run: echo routine-says-hi');
    await p.waitForSelector('.schedule-next');
    log('preview:', (await p.innerText('.schedule-next')).replace(/\s+/g, ' '));
    await p.click('button:has-text("Create routine")');
    await p.waitForSelector('h1:has-text("Echo check")');
    await p.screenshot({ path: `${S}/r-1-editor.png` });

    // Run now: its chat waits for the approval.
    await p.click('.agents-head button:has-text("Run now")');
    await p.waitForSelector('text=Started. Its chat appears in Recent.');
    await home();
    const waiting = '.attention button:has-text("Echo check")';
    await p.waitForSelector(waiting, { timeout: 20000 });
    log('waiting:', (await p.innerText('.attention')).replace(/\s+/g, ' '));
    await p.click(waiting);
    await p.waitForSelector('.conversation .approval button:has-text("Allow")');
    await p.screenshot({ path: `${S}/r-2-approval.png` });
    await p.click('.conversation .approval button.primary:has-text("Allow")');
    // Leave before it finishes, so the result arrives unseen.
    await home();

    // The result row appears, opens the run's chat, and clears once seen.
    const result = '.routine-results button:has-text("Echo check · Finished")';
    await p.waitForSelector(result, { timeout: 20000 });
    log('result:', (await p.innerText('.routine-results')).replace(/\s+/g, ' '));
    await p.screenshot({ path: `${S}/r-3-result.png` });
    await p.click(result);
    await p.waitForSelector('.conversation h1:has-text("Echo check")');
    await p.waitForSelector('.conversation >> text=routine-says-hi');
    await sleep(800);
    await home();
    await sleep(800);
    check((await p.locator('.routine-results').count()) === 0, 'the seen result leaves the list');
    log('result row cleared after opening');

    // The Runs tab: the finished run, its trigger and its result.
    await p.click('aside.sidebar a:has-text("Routines")');
    await p.click('button.agent-open:has-text("Echo check")');
    await p.click('[role=tab]:has-text("Runs")');
    await p.waitForSelector('.runs-table tbody tr');
    const row = (await p.innerText('.runs-table tbody tr')).replace(/\s+/g, ' ');
    log('run:', row);
    check(row.includes('Run now') && row.includes('Finished'), 'the run is listed as a finished Run now');
    await p.screenshot({ path: `${S}/r-4-runs.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/r-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
