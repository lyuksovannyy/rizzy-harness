// The paged chat list (FIX_PLAN 4.5, docs/store-readers.md §4 and §5): 120
// chats in no project come a page at a time as the sidebar scrolls, each
// once; a project whose chats are older than every loaded page shows them
// when it is opened (its own query) and loads its next page on "Show more";
// a chat made meanwhile shows on top after the server's nudge.
// Usage: node web/e2e/chat-list-pages.cjs <path-to-harness-binary> [out-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2epages-'));
const OUT = path.resolve(process.argv[3] || S);
fs.mkdirSync(OUT, { recursive: true });
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };

async function boundPort(logFile, ms = 60000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    const m = /listening on 127\.0\.0\.1:(\d+)/.exec(fs.existsSync(logFile) ? fs.readFileSync(logFile, 'utf8') : '');
    if (m) return Number(m[1]);
    await sleep(100);
  }
  throw new Error('the server did not say where it listens');
}

const LOOSE = '.sidebar-scroll > ul.chats > li > a.session-item';
const BOX = '.project-box:has-text("Old project")';

let PAGE = null;
(async () => {
  const logFile = `${S}/serve.log`;
  const out = fs.openSync(logFile, 'w');
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:0'], { detached: true, stdio: ['ignore', out, out] });
  try {
    const URL_ = `http://127.0.0.1:${await boundPort(logFile)}/`;
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    // Short, so the list's end is out of view until scrolled.
    const ctx = await b.newContext({ viewport: { width: 1280, height: 720 } });
    await keyed(ctx);
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    const lists = [];
    p.on('request', (r) => {
      const u = new URL(r.url());
      if (u.pathname === '/api/sessions' && r.method() === 'GET') lists.push(u.search);
    });
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:text-is("Finish")');
    await p.waitForSelector('h1.greeting');

    const api = (method, url, body) => p.evaluate(async ([m, u, b]) => {
      const r = await fetch(u, { method: m, headers: { 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: await r.json().catch(() => null) };
    }, [method, url, body]);
    const chat = async (body) => {
      const r = await api('POST', '/api/sessions', { profile: 'chat', ...body });
      check(r.status === 200, `chat ${body.title}: ${r.status} ${JSON.stringify(r.body)}`);
      return r.body.session_id;
    };

    // Oldest first: the project's twelve chats are older than every one of
    // the 120 loose chats, so no page of the main list holds them.
    const r = await api('POST', '/api/projects', { name: 'Old project' });
    check(r.status === 200 || r.status === 201, `project: ${r.status}`);
    const pid = r.body.project_id;
    for (let i = 0; i < 12; i++) await chat({ title: `Old ${i}`, project_id: pid });
    for (let i = 0; i < 120; i++) await chat({ title: `Chat ${i}` });
    log('made 12 project chats and 120 loose ones');

    // Closed in this browser before the reload: a closed group costs nothing.
    await p.reload();
    await p.waitForSelector(BOX);
    await p.click(`${BOX} button[aria-label="Collapse Old project"]`);
    lists.length = 0;
    await p.reload();
    await p.waitForSelector(LOOSE);
    await sleep(300);
    check(!lists.some((s) => s.includes('project=')), `a closed project asked for its chats: ${lists}`);
    const first = await p.locator(LOOSE).count();
    check(first >= 50 && first < 120, `one or a few pages at first (${first})`);
    check((await p.locator(LOOSE).first().innerText()).trim() === 'Chat 119', 'the newest on top');

    // Scroll to the end until there is nothing more to load.
    for (let i = 0; i < 40 && (await p.locator('.load-more').count()); i++) {
      await p.locator('.sidebar-scroll').evaluate((el) => el.scrollTo(0, el.scrollHeight));
      await sleep(250);
    }
    check((await p.locator('.load-more').count()) === 0, 'the list ends');
    const titles = await p.locator(LOOSE).allInnerTexts();
    check(titles.length === 120, `every chat once (${titles.length})`);
    check(new Set(titles).size === 120, 'none twice');
    check(titles.every((t, i) => t.trim() === `Chat ${119 - i}`), 'newest first, in order');
    const after = lists.filter((s) => s.includes('after='));
    check(after.length >= 1, `scrolling asked for the next pages: ${lists}`);
    log(`scrolled: ${titles.length} chats in ${after.length + 1} pages`);

    // Opened: the project's own newest chats, though no loaded page has them.
    await p.click(`${BOX} button[aria-label="Expand Old project"]`);
    await p.waitForFunction(() => document.querySelectorAll('.project-box ul.chats > li > a.session-item').length === 5);
    const inBox = () => p.locator(`${BOX} ul.chats > li > a.session-item`).allInnerTexts();
    check((await inBox())[0].trim() === 'Old 11', 'its newest first');
    await p.click(`${BOX} button:text-is("Show 5 more")`);
    check((await inBox()).length === 10, 'its first page: ten');
    await p.click(`${BOX} button:text-is("Show more")`);
    await p.waitForFunction(() => document.querySelectorAll('.project-box ul.chats > li > a.session-item').length === 12);
    const box = await inBox();
    check(box.every((t, i) => t.trim() === `Old ${11 - i}`), `the project's chats in order: ${box}`);
    await p.locator('aside.sidebar').screenshot({ path: `${OUT}/pages-project-open.png` });
    log('the project showed its own older chats and its next page');

    // A chat made elsewhere shows on top after the nudge, the rest stay.
    await chat({ title: 'Brand new' });
    await p.waitForFunction((sel) => document.querySelector(sel)?.textContent?.includes('Brand new'), LOOSE);
    const now = await p.locator(LOOSE).allInnerTexts();
    check(now.length === 121 && new Set(now).size === 121, `121 chats once each (${now.length})`);
    log('a new chat came on top after the nudge');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    await b.close();
    log('ok');
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${OUT}/pages-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    fs.rmSync(S, { recursive: true, force: true });
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
