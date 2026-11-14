// The agent's task list against the real binary: a new Agentic chat from
// the composer runs as the Agentic default (the Software Developer, TD-1);
// its demo agent calls `todo` twice (`script:`); the list is pinned above
// the composer with the item in progress in its head, follows the latest
// call, folds to its head once the turn ends (TD-2), and a reload shows
// the same list (a fold of the log).
// Usage: node web/e2e/todo.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2et-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 50\nmodels = [{ id = "mock" }]\n');
// The host shell is a fallback only where bubblewrap does not work here.
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n\n[execution]\nhost = true\n`);
const URL_ = 'http://127.0.0.1:7964/';
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
fs.mkdirSync(`${S}/shots`, { recursive: true });
const step = (items) => ({ tool: 'todo', input: { items } });
const SCRIPT = [
  step([
    { content: 'Read the session loop and the event schema', status: 'in_progress' },
    { content: 'Write the failing tests first', status: 'pending' },
    { content: 'Add the todo tool and its event', status: 'pending' },
    { content: 'Pin the list above the composer', status: 'pending' },
  ]),
  step([
    { content: 'Read the session loop and the event schema', status: 'done' },
    { content: 'Write the failing tests first', status: 'done' },
    { content: 'Add the todo tool and its event', status: 'in_progress' },
    { content: 'Pin the list above the composer', status: 'pending' },
    { content: 'Run fmt, clippy and the web checks', status: 'pending' },
  ]),
];

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7964'], { detached: true, stdio: 'ignore' });
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
    // A new Agentic chat from the composer: it takes the Agentic default
    // agent (the Software Developer), shown in the picker, without a pick.
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.composer input.mono', `${S}/ws`);
    const pickerLabel = await p.getAttribute('.agent-pick button[aria-label^="agent: "]', 'aria-label');
    log('picker:', pickerLabel);
    check(pickerLabel === 'agent: Software Developer', `the Agentic default is shown (${pickerLabel})`);
    const local = await p.evaluate(async () => (await (await fetch('/api/nodes')).json()).nodes.find((n) => n.node_id === 'local'));
    if (!local.exec_modes.includes('sandbox')) {
      log('bubblewrap does not work here: the chat uses the host shell');
      await p.selectOption(SELECT, 'host');
      await p.click('dialog.host-warning button:text-is("Use the host shell")');
    }
    await p.fill('.composer textarea', `script: ${JSON.stringify(SCRIPT)}`);
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    const sid = (await p.evaluate(() => location.hash)).split('/').pop();
    await p.waitForSelector('.todo-panel');
    const created = await p.evaluate(async (id) => (await (await fetch(`/api/sessions/${id}/events`)).json()).events.find((e) => e.kind === 'session_created'), sid);
    check(created.payload.agent_id === 'coder', `the chat runs as ${created.payload.agent_id}`);
    await p.waitForFunction(() => document.querySelector('.todo-panel .todo-count')?.textContent?.includes('2 of 5'));
    // The turn ends with an item still in progress: the panel folds to its head.
    await p.waitForFunction(() => document.querySelector('.todo-head')?.getAttribute('aria-expanded') === 'false');
    await sleep(500);
    await p.screenshot({ path: `${S}/shots/todo-settled-light.png` });
    await p.click('.todo-head');
    const head = await p.innerText('.todo-head');
    log('panel head:', head.replace(/\s+/g, ' '));
    check(head.includes('2 of 5 done'), 'the head counts the latest list');
    check(head.includes('Add the todo tool and its event'), 'the head names the item in progress');
    check((await p.$$('.todo-row')).length === 5, 'five rows');
    await p.screenshot({ path: `${S}/shots/todo-open-light.png` });

    await p.click('.todo-head');
    check((await p.$$('.todo-row')).length === 0, 'collapsed');
    await p.screenshot({ path: `${S}/shots/todo-collapsed-light.png` });

    // A reload folds the log again: the same list (folded: no turn runs).
    await p.emulateMedia({ colorScheme: 'dark' });
    await p.reload();
    await p.waitForSelector('.todo-panel');
    check((await p.getAttribute('.todo-head', 'aria-expanded')) === 'false', 'folded after a reload');
    await p.click('.todo-head');
    check((await p.$$('.todo-row')).length === 5, 'the list after a reload');
    await sleep(500);
    await p.screenshot({ path: `${S}/shots/todo-open-dark.png` });

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
