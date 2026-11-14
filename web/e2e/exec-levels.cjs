// Execution levels against the real binary (N.10a part 2). The owner (a
// server admin) sets an account's level in Settings → Users and a member's
// level in Workspace admin → Members. Granting `host` in either place shows
// the host warning with its risks before anything is saved; cancelling
// saves nothing (the server still answers `none`); confirming saves `host`.
// A lower level is saved at once, with no warning. An admin of the team at
// `sandbox` is offered nothing above `sandbox` and never `host`.
// Usage: node web/e2e/exec-levels.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2el-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 50\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n`);
const URL_ = 'http://127.0.0.1:7795/';
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
const RISKS = [
  'with nothing between them and the machine',
  'every file that user can, its keys and other projects included',
  'can keep running after the chat ends',
  'one web page or file the agent reads can turn all of that against you',
  'The approval card is then the only check',
];
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7795'], { detached: true, stdio: 'ignore' });
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
    const api = (method, url, body) =>
      p.evaluate(
        async ([m, u, b]) => {
          const r = await fetch(u, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined });
          return { status: r.status, body: await r.json().catch(() => null) };
        },
        [method, url, body],
      );
    const bob = (await api('POST', '/api/users', { username: 'bob' })).body;
    const carol = (await api('POST', '/api/users', { username: 'carol' })).body;
    check(bob.exec_ceiling === 'none', `a new account starts at none: ${bob.exec_ceiling}`);
    const ceiling = async (id) => (await api('GET', '/api/users')).body.users.find((u) => u.user_id === id).exec_ceiling;

    // ---- Settings → Users: the account's level ----
    await p.goto(`${URL_}#/settings/server/users`);
    const BOB = 'select[aria-label="Level of bob"]';
    await p.waitForSelector(BOB);
    check((await p.inputValue(BOB)) === 'none', 'bob shows none');
    await p.screenshot({ path: `${S}/l-0-users-light.png` });

    await p.selectOption(BOB, 'host');
    await p.waitForSelector('dialog.host-warning[open]');
    await sleep(400);
    const text = await p.innerText('dialog.host-warning');
    check(text.includes('Let bob run commands without the sandbox?'), `the warning names bob: ${text.slice(0, 80)}`);
    for (const risk of RISKS) check(text.includes(risk), `the warning says: ${risk}`);
    check((await ceiling(bob.user_id)) === 'none', 'nothing saved while the warning is open');
    await p.screenshot({ path: `${S}/l-1-ceiling-warning-light.png` });
    log('account level: warning shown with its risks, nothing saved yet');

    await p.click('dialog.host-warning button:text-is("Cancel")');
    await p.waitForSelector('dialog.host-warning', { state: 'detached' });
    await sleep(300);
    check((await p.inputValue(BOB)) === 'none', 'cancel leaves the select at none');
    check((await ceiling(bob.user_id)) === 'none', 'cancel saved nothing');
    log('account level: cancel saved nothing');

    await p.selectOption(BOB, 'host');
    await p.click('dialog.host-warning button:text-is("Grant host")');
    await p.waitForFunction(() => document.querySelector('select[aria-label="Level of bob"]')?.value === 'host');
    check((await ceiling(bob.user_id)) === 'host', 'confirmed: host saved');
    log('account level: confirmed, host saved');

    // A lower level: saved at once, no warning.
    await p.selectOption('select[aria-label="Level of carol"]', 'sandbox');
    await sleep(500);
    check((await p.locator('dialog.host-warning').count()) === 0, 'no warning below host');
    check((await ceiling(carol.user_id)) === 'sandbox', 'sandbox saved');

    // ---- Workspace admin → Members: the member's level ----
    const team = (await api('POST', '/api/workspaces', { name: 'Acme' })).body;
    await api('POST', `/api/workspaces/${team.workspace_id}/members`, { username: 'bob', role: 'member' });
    await api('POST', `/api/workspaces/${team.workspace_id}/members`, { username: 'carol', role: 'admin' });
    await api('PUT', '/api/me/workspace', { workspace_id: team.workspace_id });
    const level = async (id) =>
      (await api('GET', `/api/workspaces/${team.workspace_id}/members`)).body.members.find((m) => m.user_id === id).exec_level;
    check((await level(bob.user_id)) === 'none', 'a new member starts at none');
    await p.goto(`${URL_}#/settings/workspace-admin/members`);
    await p.reload();
    const BOBW = 'select[aria-label="Level of bob"]';
    await p.waitForSelector(BOBW);
    await p.screenshot({ path: `${S}/l-2-members-light.png` });
    await p.selectOption(BOBW, 'host');
    await p.waitForSelector('dialog.host-warning[open]');
    await sleep(400);
    const wtext = await p.innerText('dialog.host-warning');
    check(wtext.includes('in Acme'), 'the warning names the workspace');
    for (const risk of RISKS) check(wtext.includes(risk), `the warning says: ${risk}`);
    check((await level(bob.user_id)) === 'none', 'nothing saved while the warning is open');
    await p.screenshot({ path: `${S}/l-3-member-warning-light.png` });
    await p.click('dialog.host-warning button:text-is("Cancel")');
    await p.waitForSelector('dialog.host-warning', { state: 'detached' });
    await sleep(300);
    check((await level(bob.user_id)) === 'none', 'cancel saved nothing');
    check((await p.inputValue(BOBW)) === 'none', 'cancel leaves the select at none');
    log('member level: warning shown, cancel saved nothing');
    await p.selectOption(BOBW, 'host');
    await p.click('dialog.host-warning button:text-is("Grant host")');
    await p.waitForFunction(() => document.querySelector('select[aria-label="Level of bob"]')?.value === 'host');
    check((await level(bob.user_id)) === 'host', 'confirmed: host saved');
    log('member level: confirmed, host saved');

    // The dark theme: the members list and the warning.
    await p.emulateMedia({ colorScheme: 'dark' });
    await p.reload();
    await p.waitForSelector(BOBW);
    await p.screenshot({ path: `${S}/l-2-members-dark.png` });
    await p.selectOption(BOBW, 'sandbox');
    await sleep(400);
    check((await level(bob.user_id)) === 'sandbox', 'lowered at once');
    await p.selectOption(BOBW, 'host');
    await p.waitForSelector('dialog.host-warning[open]');
    await sleep(400);
    await p.screenshot({ path: `${S}/l-3-member-warning-dark.png` });
    await p.click('dialog.host-warning button:text-is("Cancel")');
    check((await level(bob.user_id)) === 'sandbox', 'cancel in the dark saved nothing');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/l-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
