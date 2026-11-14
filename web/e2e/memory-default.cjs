// Memory by default against the real binary (README, Memory): in the
// Personal workspace a `remember` is saved at once and the chat says so in
// one line with Undo; Undo removes it; a private (`sensitive`) fact says it
// stays in this workspace. Then a team workspace the owner creates asks the
// join question at the top of the page until it is answered; "Keep it out"
// answers no and the question goes; Settings → Workspace shows the switch off.
// Usage: node web/e2e/memory-default.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2em-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 20\nmodels = [{ id = "mock" }]\n');
const URL_ = 'http://127.0.0.1:7966/';
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
fs.mkdirSync(`${S}/shots`, { recursive: true });
const TRAM = 'Alex rides the number 7 tram to work';
const api = (p, method, url, body) =>
  p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined });
      return { status: r.status, body: await r.json().catch(() => null) };
    },
    [method, url, body],
  );
const facts = async (p) => (await api(p, 'GET', '/api/memory')).body.memories;

async function chat(p, text) {
  await p.goto(URL_);
  await p.waitForSelector('h1.greeting');
  await p.fill('.composer textarea', text);
  await p.keyboard.press('Enter');
  await p.waitForSelector('.conversation');
  await p.waitForSelector('.tool-memory');
}

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7966'], { detached: true, stdio: 'ignore' });
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

    // 1. Saved at once, with one quiet line and Undo.
    await chat(p, `remember: ${TRAM}`);
    await p.waitForFunction(() => document.querySelector('.tool-memory')?.textContent?.includes('Remembered:'));
    const line = await p.innerText('.tool-memory');
    log('chat line:', line.replace(/\s+/g, ' '));
    check(line.includes(TRAM), 'the line names the fact');
    check(await p.$('.tool-memory .memory-undo'), 'the line has Undo');
    let mine = await facts(p);
    check(mine.length === 1 && mine[0].status === 'active', `saved at once: ${JSON.stringify(mine)}`);
    await p.screenshot({ path: `${S}/shots/memory-saved-light.png` });
    await p.click('.tool-memory .memory-undo');
    await p.waitForFunction(() => document.querySelector('.tool-memory')?.textContent?.includes('Undone:'));
    check((await facts(p)).length === 0, 'Undo removed it');

    // 2. A private detail says where it stays.
    const script = JSON.stringify([{ tool: 'remember', input: { text: 'Alex has asthma', sensitive: true } }]);
    await chat(p, `script: ${script}`);
    await p.waitForFunction(() => document.querySelector('.tool-memory')?.textContent?.includes('in this workspace only'));
    mine = await facts(p);
    check(mine.length === 1 && mine[0].place === 'workspace', `kept in Personal: ${JSON.stringify(mine)}`);
    await p.emulateMedia({ colorScheme: 'dark' });
    await p.screenshot({ path: `${S}/shots/memory-private-dark.png` });
    await p.emulateMedia({ colorScheme: 'light' });

    // 3. A new team workspace asks the join question until it is answered.
    const ws = (await api(p, 'POST', '/api/workspaces', { name: 'Acme' })).body.workspace_id;
    check(ws, 'workspace created');
    check((await api(p, 'PUT', '/api/me/workspace', { workspace_id: ws })).status === 200, 'switched');
    await p.goto(URL_);
    await p.waitForSelector('.memory-ask');
    const ask = await p.innerText('.memory-ask');
    log('question:', ask.replace(/\s+/g, ' '));
    check(ask.includes('Use your memory in Acme?'), 'the question names the workspace');
    await p.screenshot({ path: `${S}/shots/memory-ask-light.png` });
    // It stays on other pages until answered.
    await p.goto(`${URL_}#/settings/account/memory`);
    await p.waitForSelector('.memory-ask');
    await p.emulateMedia({ colorScheme: 'dark' });
    await p.screenshot({ path: `${S}/shots/memory-page-dark.png` });
    await p.emulateMedia({ colorScheme: 'light' });
    await p.click('.memory-ask button:text-is("Keep it out")');
    await p.waitForSelector('.memory-ask', { state: 'detached' });
    const d = (await api(p, 'GET', `/api/workspaces/${ws}`)).body;
    check(d.use_memory === false && d.memory_answered === true, `answered no: ${JSON.stringify(d)}`);
    await p.reload();
    await p.waitForSelector('h1, .stack');
    await sleep(500);
    check(!(await p.$('.memory-ask')), 'the question does not come back');

    // 4. A phone-width view of the question in a second workspace.
    const ws2 = (await api(p, 'POST', '/api/workspaces', { name: 'Beta' })).body.workspace_id;
    await api(p, 'PUT', '/api/me/workspace', { workspace_id: ws2 });
    await p.setViewportSize({ width: 390, height: 800 });
    await p.goto(URL_);
    await p.waitForSelector('.memory-ask');
    const wide = await p.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth);
    check(wide, 'no horizontal scroll at phone width');
    await p.screenshot({ path: `${S}/shots/memory-ask-phone.png` });

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
