// A pull request's card (PLAN.md T102, review R6) against the real binary,
// without a forge: a project whose recorded source and the owner's
// connection are rows written into the running server's database (the
// connection's token is junk bytes, never opened: the card is only ever
// denied, so nothing is sent to any forge). The demo provider calls
// `pull_request` with a long body and no base. The card: the destination
// from the server, "into the repository's default branch", no line written
// by the node, the body collapsed with "Show all to allow" in Allow's place
// (a focusable button, described by the note); after it, Allow appears and
// waits; Deny answers. Screenshots, light and dark.
// Usage: node web/e2e/forge-card.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const WORK = path.resolve(__dirname, '../../target/e2e-work');
const S = path.resolve(process.argv[3] || (fs.mkdirSync(WORK, { recursive: true }), fs.mkdtempSync(path.join(WORK, 'forge-card-'))));
const D = `${S}/data`, ROOT = `${S}/roots`, PROJECTS = `${S}/roots/projects`;
for (const d of [D, PROJECTS, `${S}/shots`]) fs.mkdirSync(d, { recursive: true });
fs.writeFileSync(`${D}/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 50\nmodels = [{ id = "mock" }]\n');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); }); });
const sql = (q) => execFileSync('sqlite3', [`${D}/master.db`, q]).toString();
const BODY = Array.from({ length: 80 }, (_, i) => `line ${i}: @someone Closes #${i} ![x](https://example.invalid/${i}.png)`).join('\n');

(async () => {
  const port = await freePort();
  const URL_ = `http://127.0.0.1:${port}/`;
  fs.writeFileSync(`${D}/node.toml`, `workspace_roots = ["${ROOT}"]\n\n[projects]\ndir = "${PROJECTS}"\n\n[execution]\nhost = true\n`);
  const srv = spawn(BIN, ['--data-dir', D, 'serve', '--bind', `127.0.0.1:${port}`], { detached: true, stdio: 'ignore' });
  log('server pid', srv.pid, 'port', port);
  let b = null;
  try {
    const code = await claimCode(`${D}/claim-code`, URL_);
    b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 900 } });
    await keyed(ctx);
    const p = await ctx.newPage();
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:text-is("Finish")');
    await p.waitForSelector('h1.greeting');
    const api = (method, url, body) => p.evaluate(async ([m, u, bd]) => {
      const r = await fetch(u, { method: m, headers: { 'content-type': 'application/json' }, body: bd === undefined ? undefined : JSON.stringify(bd) });
      return { status: r.status, body: await r.json().catch(() => null) };
    }, [method, url, body]);
    const me = (await api('GET', '/api/me')).body;
    const uid = me.user?.user_id ?? me.user_id;
    let r = await api('POST', '/api/projects', { name: 'Harness', folder_on: 'local' });
    check(r.status === 200 || r.status === 201, `project: ${r.status} ${JSON.stringify(r.body)}`);
    const pid = r.body.project_id;
    // The source and the connection, as a clone and a connect would leave them.
    const now = Date.now();
    sql(`UPDATE projects SET source_host = 'github.com', source_name = 'ada/harness' WHERE project_id = '${pid}';
         INSERT INTO user_connections (connection_id, user_id, host, account_id, login, sealed_token, created_ms, updated_ms)
         VALUES ('conn-e2e', '${uid}', 'github.com', '42', 'ada', X'00', ${now}, ${now});`);
    r = await api('POST', '/api/sessions', { profile: 'agentic', project_id: pid, exec_mode: 'host' });
    check(r.status === 200, `chat: ${r.status} ${JSON.stringify(r.body)}`);
    const sid = r.body.session_id;
    await p.goto(`${URL_}#/s/${sid}`);
    await p.waitForSelector('.composer textarea');
    const call = [{ tool: 'pull_request', input: { head: 'feat', title: 'Add the thing', body: BODY } }];
    await p.fill('.composer textarea', `script: ${JSON.stringify(call)}`);
    await p.keyboard.press('Enter');
    await p.waitForSelector('.approval .approval-actions button.danger', { timeout: 30000 });
    await sleep(900); // the composer's guard
    const card = await p.$eval('.approval', (el) => ({
      text: el.textContent,
      target: el.querySelector('.approval-push-target')?.textContent ?? null,
      reason: !!el.querySelector('.approval-reason'),
      buttons: [...el.querySelectorAll('.approval-actions button')].map((x) => ({ t: x.textContent, d: x.disabled, a: x.getAttribute('aria-disabled'), by: x.getAttribute('aria-describedby') })),
    }));
    log('card buttons:', JSON.stringify(card.buttons));
    check(card.target === 'Opens a pull request in github.com/ada/harness as ada (as the server knows it)', `destination: ${card.target}`);
    check(card.text.includes("Into the repository's default branch (asked from the forge after you allow)"), 'the default-branch line');
    check(!card.reason && !card.text.includes('open a pull request in github.com/ada/harness as ada'), 'no line written by the node');
    check(card.text.includes('in the middle not shown'), 'the body is collapsed');
    check(card.buttons[0].t === 'Show all to allow' && !card.buttons[0].d && card.buttons[0].by, 'Show all to allow in Allow\'s place');
    check(!card.buttons.some((x) => x.t === 'Allow once'), 'no Allow while collapsed');
    for (const scheme of ['light', 'dark']) {
      await p.emulateMedia({ colorScheme: scheme });
      await sleep(500);
      await p.screenshot({ path: `${S}/shots/pr-card-collapsed-${scheme}.png` });
    }
    await p.emulateMedia({ colorScheme: 'light' });
    // By keyboard: focus reaches the button and Enter presses it.
    await p.focus('.approval-actions button.primary');
    check(await p.evaluate(() => document.activeElement?.textContent) === 'Show all to allow', 'focus on Show all to allow');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.approval-actions button.primary:text-is("Allow once")');
    const after = await p.$eval('.approval', (el) => ({
      text: el.textContent,
      allow: el.querySelector('.approval-actions button.primary')?.getAttribute('aria-disabled'),
    }));
    check(!after.text.includes('in the middle not shown') && after.text.includes('line 40: @someone Closes #40'), 'the body shown whole');
    check(after.allow === 'true', `Allow waits after Show all: ${after.allow}`);
    await sleep(800);
    check((await p.$eval('.approval-actions button.primary', (x) => x.getAttribute('aria-disabled'))) === null, 'Allow usable after the wait');
    await p.screenshot({ path: `${S}/shots/pr-card-shown.png` });
    // Deny only: nothing is ever sent to a forge.
    await p.click('.approval button.danger');
    for (let i = 0; i < 100; i++) {
      const ev = (await api('GET', `/api/sessions/${sid}/events`)).body.events;
      if (ev.some((e) => e.kind === 'turn_ended')) break;
      await sleep(100);
    }
    const ev = (await api('GET', `/api/sessions/${sid}/events`)).body.events;
    const res = ev.find((e) => e.kind === 'approval_resolved');
    check(res && res.payload.decision === 'deny', 'denied');
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('ok; screenshots in', `${S}/shots`);
  } finally {
    if (b) await b.close();
    try { process.kill(-srv.pid); } catch {}
  }
})().catch((e) => {
  console.error('FAILED:', e.message);
  process.exitCode = 1;
});
