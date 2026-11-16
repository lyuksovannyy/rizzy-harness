// The sidebar to the owner's sketch of 2026-11-16 (docs/sidebar-search.md):
// two projects as boxes holding their chats, chats in no project below, an
// agentic chat whose sub-agents nest under it, and the node badge. Checks the
// order and the collapse state across a reload, and saves the sidebar in the
// light and dark themes (desktop, and the phone drawer).
// Usage: node web/e2e/sidebar.cjs <path-to-harness-binary> [out-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2esidebar-'));
const OUT = path.resolve(process.argv[3] || S);
fs.mkdirSync(OUT, { recursive: true });
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/ws/notes.md`, 'notes\n');
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\nmax_approval_mode = "ask"\n`);
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

let PAGE = null;
(async () => {
  const logFile = `${S}/serve.log`;
  const out = fs.openSync(logFile, 'w');
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:0'], { detached: true, stdio: ['ignore', out, out] });
  try {
    const URL_ = `http://127.0.0.1:${await boundPort(logFile)}/`;
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

    const api = (method, url, body) => p.evaluate(async ([m, u, b]) => {
      const r = await fetch(u, { method: m, headers: { 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: await r.json().catch(() => null) };
    }, [method, url, body]);
    const project = async (name) => {
      const r = await api('POST', '/api/projects', { name });
      check(r.status === 200 || r.status === 201, `project ${name}: ${r.status}`);
      return r.body.project_id;
    };
    const chat = async (body) => {
      const r = await api('POST', '/api/sessions', body);
      check(r.status === 200, `chat ${body.title}: ${r.status} ${JSON.stringify(r.body)}`);
      return r.body.session_id;
    };

    // The scene: oldest first, so the newest lands on top.
    const web = await project('Website relaunch');
    // Seven chats: the oldest is behind the project's "Show 2 more".
    const oldest = await chat({ profile: 'chat', title: 'Copy for the landing page', project_id: web });
    for (const t of ['Hero image', 'Pricing table', 'Footer links', 'Font pairing', 'Launch checklist']) await chat({ profile: 'chat', title: t, project_id: web });
    await chat({ profile: 'chat', title: 'Colour ideas', project_id: web });
    const cli = await project('CLI tool with a rather long name that has to be cut short');
    await chat({ profile: 'agentic', cwd: `${S}/ws`, title: 'Fix the argument parser in the CLI so flags work', agent_id: 'coder', project_id: cli });
    await chat({ profile: 'chat', title: 'Weekend plans' });
    const lead = await chat({ profile: 'agentic', cwd: `${S}/ws`, title: 'Review the notes', agent_id: 'teamlead' });
    check((await api('POST', `/api/sessions/${lead}/prompt`, { text: 'delegate: scout=look around | reviewer=check the notes' })).status === 200, 'prompt taken');
    const until = Date.now() + 60000;
    let kids = [];
    while (Date.now() < until) {
      const list = (await api('GET', '/api/sessions')).body.sessions;
      kids = list.filter((s) => s.parent_id === lead);
      if (kids.length >= 2) break;
      await sleep(300);
    }
    check(kids.length >= 2, `two sub-agent sessions (${kids.length})`);
    await chat({ profile: 'chat', title: 'A <b>title</b> with <img src=x> markup' });
    log('scene built: 2 projects, 3 loose chats, a parent with', kids.length, 'sub-agents');

    await p.goto(`${URL_}#/`);
    await p.reload();
    await p.waitForSelector('.project-box');
    await p.waitForFunction(() => document.querySelectorAll('.sidebar ul.chats.sub a').length >= 2);
    const order = await p.$$eval('.sidebar-scroll a', (a) => a.map((x) => x.textContent.trim()));
    log('sidebar rows:', JSON.stringify(order));
    check(order[0].startsWith('CLI tool'), 'the newest project first');
    check(order.indexOf('Website relaunch') > 0, 'both projects shown');
    check((await p.locator('.sidebar img').count()) === 0, 'markup in a title stays text');
    const badge = p.locator('.sidebar a.session-item:has-text("Review the notes") .node-mark');
    check((await badge.getAttribute('aria-label')).startsWith('Runs on '), 'the badge names the node');
    const tipColour = await badge.evaluate((e) => getComputedStyle(e).color);
    log('badge:', await badge.getAttribute('aria-label'), tipColour);

    const shots = [];
    for (const [w, h, size] of [[1280, 860, 'desktop'], [390, 844, 'phone']]) {
      await p.setViewportSize({ width: w, height: h });
      for (const theme of ['light', 'dark']) {
        await p.emulateMedia({ colorScheme: theme });
        const settings = (await api('GET', '/api/me/settings')).body;
        check((await api('PUT', '/api/me/settings', { ...settings, theme })).status === 200, 'settings saved');
        await p.goto(`${URL_}#/`);
        await p.reload();
        await p.waitForSelector('h1.greeting');
        if (size === 'phone') {
          await p.click('.topbar button.icon');
          await sleep(400);
        }
        await p.waitForSelector('.project-box');
        await sleep(250);
        const shot = async (file) => {
          if (size === 'phone') await p.screenshot({ path: file });
          else await p.locator('aside.sidebar').screenshot({ path: file });
          shots.push(file);
        };
        await shot(`${OUT}/v2-sidebar-${size}-${theme}.png`);
        if (size === 'desktop') {
          // Keyboard focus on the badge's row shows the node's name.
          await p.focus('.sidebar a.session-item:has-text("Review the notes")');
          await p.keyboard.press('Shift+Tab');
          await p.keyboard.press('Tab');
          await sleep(250);
          await shot(`${OUT}/v2-sidebar-${size}-${theme}-focus.png`);
        }
      }
    }

    // The open chat behind the project's cap: shown in its place, the count true.
    await p.setViewportSize({ width: 1280, height: 860 });
    await p.goto(`${URL_}#/s/${oldest}`);
    await p.reload();
    await p.waitForSelector('.project-box');
    const box = `li.project-box:has(a[href="#/p/${web}"])`;
    await p.waitForSelector(`${box} a.session-item[aria-current="page"]:has-text("Copy for the landing page")`);
    check((await p.locator(`${box} button.show-more`).innerText()) === 'Show 1 more', 'the cap counts what it hides');
    await p.locator('aside.sidebar').screenshot({ path: `${OUT}/v2-open-chat-behind-cap.png` });

    // Closed, the project stays closed, and its header says the open chat is inside; both survive a reload.
    await p.click(`${box} button[aria-label="Collapse Website relaunch"]`);
    await p.reload();
    await p.waitForSelector('.project-box');
    check((await p.getAttribute(`${box} .section-toggle`, 'aria-expanded')) === 'false', 'a closed project stays closed');
    check((await p.locator(`${box} .dot.here`).getAttribute('aria-label')) === 'The open chat is in here', 'the closed header marks the open chat');
    await p.locator('aside.sidebar').screenshot({ path: `${OUT}/v2-closed-project-holds-open-chat.png` });
    await p.click('.sidebar-scroll .section-toggle:has-text("Projects")');
    await p.reload();
    await p.waitForSelector('.sidebar-scroll');
    check((await p.locator('.project-box').count()) === 0, 'a closed section stays closed');
    check((await p.locator('.side-section > .section-row .dot.here').count()) === 1, 'the closed section marks the open chat');
    await p.locator('aside.sidebar').screenshot({ path: `${OUT}/v2-closed-section-holds-open-chat.png` });

    // Collapsed to the rail: the header's second part still works both ways.
    await p.click('.head-pair button[aria-label="Collapse sidebar"]');
    await p.waitForSelector('aside.sidebar.rail');
    await p.click('aside.sidebar.rail button[aria-label="Expand sidebar"]');
    await p.waitForSelector('aside.sidebar:not(.rail) .head-pair');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log(`${shots.length + 3} screenshots in`, OUT);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${OUT}/sidebar-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    fs.rmSync(S, { recursive: true, force: true });
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
