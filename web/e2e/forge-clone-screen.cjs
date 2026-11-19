// "From a repository" in the Project popup (ADOPTION_PLAN X.27 step 3) on the
// real binary, without a forge (the binary clones over https only): the
// no-connection state with its link to Plugins → Connectors, and the pending
// project drawn in the popup and the sidebar, from rows written into the
// running server's database (a clone running and one failed, as the clone's
// task writes them): cloning is no link and no button, the failure shows its
// reason with Dismiss, and Dismiss removes it. Screenshots, light and dark.
// Usage: node web/e2e/forge-clone-screen.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-clone-')));
const D = `${S}/data`, ROOT = `${S}/roots`, PROJECTS = `${S}/roots/projects`;
fs.mkdirSync(D, { recursive: true });
fs.mkdirSync(ROOT, { recursive: true });
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});
const sql = (q) => execFileSync('sqlite3', [`${D}/master.db`, q]).toString();
let PAGE = null;

(async () => {
  const port = await freePort();
  const URL_ = `http://127.0.0.1:${port}/`;
  fs.writeFileSync(`${D}/node.toml`, `workspace_roots = ["${ROOT}"]\n\n[projects]\ndir = "${PROJECTS}"\n`);
  const srv = spawn(BIN, ['--data-dir', D, 'serve', '--bind', `127.0.0.1:${port}`], { detached: true, stdio: 'ignore' });
  log('server pid', srv.pid, 'port', port);
  try {
    const code = await claimCode(`${D}/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 900 }, colorScheme: 'light' });
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
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickAgent(p);

    // No connection: one line and the link.
    await p.click('.agentic-row .project-pick');
    const sec = 'dialog.project-picker section[aria-labelledby="pp-repo"]';
    await p.waitForSelector(`${sec} a[href="#/plugins"]`);
    check((await p.textContent(sec)).includes('No forge account is connected'), 'the no-connection line');
    check(!(await p.textContent('dialog.project-picker')).includes('Later'), 'no Later badge');
    log('no connection: the link to Plugins → Connectors');
    await p.keyboard.press('Escape');
    await p.waitForSelector('dialog.project-picker', { state: 'detached' });

    // Two pending rows, as the clone's task writes them.
    const me = await p.evaluate(async () => (await (await fetch('/api/me')).json()));
    const uid = me.user?.user_id ?? me.user_id;
    const list = await p.evaluate(async () => (await (await fetch('/api/projects')).json()));
    const ws = list.workspace_id;
    check(uid && ws, 'user and workspace: ' + JSON.stringify({ uid, ws }));
    const now = Date.now();
    sql(`INSERT INTO project_clones (project_id, workspace_id, name, node_id, host, full_name, created_by, started_ms, state)
         VALUES ('aa${'0'.repeat(30)}', '${ws}', 'Harness', 'local', 'github.com', 'ada/harness', '${uid}', ${now}, 'cloning');
         INSERT INTO project_clones (project_id, workspace_id, name, node_id, host, full_name, created_by, started_ms, state, reason, ended_ms)
         VALUES ('bb${'0'.repeat(30)}', '${ws}', 'Website', 'local', 'github.com', 'ada/website', '${uid}', ${now}, 'failed',
                 'this node does not clone from that host: only from the hosts its node.toml lists ([forge] hosts)', ${now});`);
    await p.goto(URL_);
    await p.waitForSelector('.pending-project');
    const side = await p.$$eval('.pending-project', (rows) => rows.map((r) => ({ text: r.textContent, links: r.querySelectorAll('a').length, buttons: r.querySelectorAll('button').length })));
    check(side.length === 2, 'two pending rows in the sidebar: ' + JSON.stringify(side));
    check(side[0].text.includes('cloning github.com/ada/harness') && side[0].links === 0 && side[0].buttons === 0, 'cloning, no link: ' + JSON.stringify(side[0]));
    check(side[1].text.includes('clone failed: this node does not clone') && side[1].buttons === 1, 'failed with Dismiss: ' + JSON.stringify(side[1]));
    log('sidebar:', side.map((r) => r.text).join(' | '));
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickAgent(p);
    await p.click('.agentic-row .project-pick');
    await p.waitForSelector('dialog.project-picker .pp-list li');
    const rows = await p.$$eval('dialog.project-picker .pp-list li', (li) => li.map((r) => ({ text: r.textContent, buttons: [...r.querySelectorAll('button')].map((b) => b.textContent) })));
    check(rows.some((r) => r.text.includes('Cloning github.com/ada/harness…') && r.buttons.length === 0), 'the popup: cloning, not pickable ' + JSON.stringify(rows));
    check(rows.some((r) => r.text.includes('Clone failed:') && r.buttons.includes('Dismiss')), 'the popup: failed with Dismiss');
    for (const scheme of ['light', 'dark']) {
      await p.emulateMedia({ colorScheme: scheme });
      await p.waitForTimeout(600);
      await p.screenshot({ path: `${S}/pending-${scheme}.png` });
    }
    await p.emulateMedia({ colorScheme: 'light' });
    // Not a project: its page is unknown.
    const st = await p.evaluate(async (id) => (await fetch(`/api/projects/${id}`)).status, `aa${'0'.repeat(30)}`);
    check(st === 404, 'a pending project has no page: ' + st);
    await p.click('dialog.project-picker .pp-list button:has-text("Dismiss")');
    await p.waitForFunction(() => !document.querySelector('dialog.project-picker')?.textContent?.includes('Clone failed'), null, { timeout: 10000 });
    check(sql(`SELECT COUNT(*) FROM project_clones WHERE state = 'failed'`).trim() === '0', 'dismissed');
    log('dismissed the failure');
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/clone-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
