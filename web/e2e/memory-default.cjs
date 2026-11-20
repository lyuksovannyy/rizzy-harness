// Memory by default against the real binary (README, Memory): in the
// Personal workspace a `remember` is saved at once and the chat shows a
// memory block saying so, with Undo (owner, 2026-11-14); Undo removes it,
// the block then says it was undone (also after a reload: its state is the
// server's), and the fact is gone from Settings → Memory and from the next
// chat's prompt. A private (`sensitive`) fact says it stays in this
// workspace. Then a team workspace the owner creates asks the
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
    await p.waitForFunction(() => document.querySelector('.tool-memory')?.textContent?.includes('Saved to your facts:'));
    const line = await p.innerText('.tool-memory');
    log('memory block:', line.replace(/\s+/g, ' '));
    check(line.includes(TRAM), 'the block names the fact');
    check(await p.$('.tool-memory .memory-undo'), 'the block has Undo');
    check(!(await p.$('.tool:not(.tool-memory) .tool-head')), 'no tool card beside it');
    let mine = await facts(p);
    check(mine.length === 1 && mine[0].status === 'active', `saved at once: ${JSON.stringify(mine)}`);
    await p.screenshot({ path: `${S}/shots/memory-saved-light.png` });
    await p.click('.tool-memory .memory-undo');
    await p.waitForFunction(() => document.querySelector('.tool-memory')?.textContent?.includes('Undone:'));
    check((await facts(p)).length === 0, 'Undo removed it');
    // The block stays undone after a reload.
    await p.reload();
    await p.waitForFunction(() => document.querySelector('.tool-memory')?.textContent?.includes('is no longer remembered'));
    await p.screenshot({ path: `${S}/shots/memory-undone-light.png` });
    // Gone from Settings → Memory and from the next chat's prompt.
    await p.goto(`${URL_}#/settings/account/memory`);
    await p.waitForSelector('text=Remembered');
    check(!(await p.innerText('main')).includes(TRAM), 'not on the Memory page');
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'memory?');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    await p.waitForFunction(() => /Memory:/.test(document.querySelector('.conversation')?.textContent ?? ''));
    const recalled = await p.innerText('.conversation');
    check(!recalled.includes(TRAM), `not in the next chat's prompt: ${recalled.slice(0, 300)}`);

    // 2. A private detail says where it stays.
    const script = JSON.stringify([{ tool: 'remember', input: { text: 'Alex has asthma', sensitive: true } }]);
    await chat(p, `script: ${script}`);
    await p.waitForFunction(() => document.querySelector('.tool-memory')?.textContent?.includes('in your Personal workspace only'));
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

    // 3b. The workspace's memory is shared: Alex (its owner) adds a fact on
    // its Memory tab; Sam, a member, sees it on the same tab.
    const SHARED = 'Acme deploys on Fridays after the standup';
    const made = await api(p, 'POST', '/api/users', { username: 'sam', exec_ceiling: 'none' });
    check(made.status === 200, `user: ${JSON.stringify(made)}`);
    check((await api(p, 'POST', `/api/workspaces/${ws}/members`, { username: 'sam', role: 'member' })).status === 200, 'member added');
    await p.goto(`${URL_}#/settings/workspace/facts`);
    await p.waitForSelector('input[aria-label="New fact"]');
    await p.fill('input[aria-label="New fact"]', SHARED);
    await p.click('button:text-is("Add")');
    await p.waitForSelector(`text=${SHARED}`);
    await p.screenshot({ path: `${S}/shots/workspace-memory-owner-light.png` });
    const ctx2 = await b.newContext({ viewport: { width: 1280, height: 860 } });
    const login = await ctx2.request.post(`${URL_}api/login`, { data: { username: 'sam', password: made.body.password } });
    const key1 = (await login.json()).login_key;
    const changed = await ctx2.request.post(`${URL_}api/me/password`, {
      data: { current: made.body.password, new: 'sams own password' },
      headers: { 'x-harness-login': key1, origin: URL_.slice(0, -1) },
    });
    check(changed.status() === 200, `password changed: ${changed.status()}`);
    const key = (await changed.json()).login_key || key1;
    await ctx2.addInitScript((k) => localStorage.setItem('harness.login', k), key);
    await keyed(ctx2);
    const p2 = await ctx2.newPage();
    PAGE = p2;
    p2.on('pageerror', (e) => errors.push(e.message));
    await p2.goto(URL_);
    // A new member's first visit is the two-step onboarding.
    await p2.click('button:text-is("Continue")');
    await p2.click('button:text-is("Finish")');
    await p2.waitForSelector('h1.greeting');
    await api(p2, 'PUT', '/api/me/workspace', { workspace_id: ws });
    // A tab keeps the workspace it opened in: a new one opens in Acme.
    await p2.close();
    const p3 = await ctx2.newPage();
    PAGE = p3;
    p3.on('pageerror', (e) => errors.push(e.message));
    await p3.goto(`${URL_}#/settings/workspace/facts`);
    await p3.waitForSelector(`text=${SHARED}`);
    check(!(await p3.$('input[aria-label="New fact"]')), 'a member does not edit it by hand');
    await p3.screenshot({ path: `${S}/shots/workspace-memory-member-light.png` });
    // What a member's chat there is given is checked in
    // crates/master/tests/it/memory_places_http.rs (the demo model does not echo it).
    PAGE = p;
    await ctx2.close();

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
