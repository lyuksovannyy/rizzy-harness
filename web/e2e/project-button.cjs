// The start screen's Project button (ADOPTION_PLAN X.26) on the real
// binary: create a project from the start screen's popup, find its folder
// on disk (inside the node's [projects] dir, a git repository with one
// commit), have the demo agent write a file there, start a second chat in
// the same project from the popup, and read the file from it. Screenshots
// of the row and the popup, light and dark.
// Usage: node web/e2e/project-button.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = path.resolve(process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-project-')));
const D = `${S}/data`, ROOT = `${S}/roots`, PROJECTS = `${S}/roots/projects`;
fs.mkdirSync(D, { recursive: true });
fs.mkdirSync(ROOT, { recursive: true });
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});
const until = async (f, what, ms = 30000) => {
  const by = Date.now() + ms;
  for (;;) {
    const v = f();
    if (v) return v;
    if (Date.now() > by) throw new Error(`timed out: ${what}`);
    await new Promise((r) => setTimeout(r, 200));
  }
};
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

    // The row: the node, the Project button, the mode (a label: no host here).
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickAgent(p);
    await p.waitForSelector('.agentic-row .project-pick');
    check((await p.textContent('.agentic-row .project-pick')).trim() === 'No project', 'nothing chosen yet');
    await p.waitForSelector('.agentic-row .exec-only');
    log('mode shown:', (await p.textContent('.agentic-row .exec-only')).trim());
    const nodes = await p.evaluate(async () => (await (await fetch('/api/nodes')).json()).nodes);
    check(nodes.find((n) => n.node_id === 'local')?.projects === true, 'the local node offers project folders');
    for (const scheme of ['light', 'dark']) {
      await p.emulateMedia({ colorScheme: scheme });
      await p.locator('.composer').screenshot({ path: `${S}/row-${scheme}.png` });
    }
    await p.emulateMedia({ colorScheme: 'light' });

    // The popup: a new project.
    await p.click('.agentic-row .project-pick');
    await p.waitForSelector('dialog.project-picker');
    await p.fill('dialog.project-picker input[aria-label="Project name"]', 'Project Name Example');
    check((await p.textContent('dialog.project-picker .pp-preview')).includes('project-name-example-····'), 'the name preview');
    for (const scheme of ['light', 'dark']) {
      await p.emulateMedia({ colorScheme: scheme });
      await p.waitForTimeout(600); // the sheet's opening animation
      await p.screenshot({ path: `${S}/popup-${scheme}.png` });
    }
    await p.emulateMedia({ colorScheme: 'light' });
    await p.click('dialog.project-picker section[aria-labelledby="pp-new"] button[type="submit"]');
    await p.waitForSelector('dialog.project-picker', { state: 'detached' });
    check((await p.textContent('.agentic-row .project-pick')).trim() === 'Project Name Example', 'the button shows the project');

    // On disk: one folder, a repository with one commit.
    const made = fs.readdirSync(PROJECTS);
    check(made.length === 1 && /^project-name-example-[a-z0-9]{4}$/.test(made[0]), 'one folder: ' + made.join(', '));
    const F = `${PROJECTS}/${made[0]}`;
    check(fs.statSync(`${F}/.git`).isDirectory(), 'it has .git');
    const commits = execFileSync('git', ['-C', F, 'rev-list', '--count', 'HEAD'], { env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null' } }).toString().trim();
    check(commits === '1', 'one commit: ' + commits);
    log('folder made:', F, 'commits:', commits);

    // A first chat writes a file there.
    await p.fill('.composer textarea', 'run: echo from-the-first-chat > first.txt');
    await p.click('.composer .send');
    await until(() => fs.existsSync(`${F}/first.txt`), 'first.txt');
    check(fs.readFileSync(`${F}/first.txt`, 'utf8').trim() === 'from-the-first-chat', 'the first chat wrote its file');
    log('the first chat wrote first.txt');

    // A second chat, from the start screen, in the same project.
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickAgent(p);
    await p.click('.agentic-row .project-pick');
    await p.waitForSelector('dialog.project-picker .pp-list li');
    await p.click('dialog.project-picker .pp-list button:has-text("Project Name Example")');
    await p.waitForSelector('dialog.project-picker', { state: 'detached' });
    // Upper-cased, so the words appear nowhere else on the page (the first
    // chat's title in the sidebar has them as typed).
    await p.fill('.composer textarea', "run: tr a-z A-Z < first.txt");
    await p.click('.composer .send');
    await p.waitForFunction(() => document.body.innerText.includes('FROM-THE-FIRST-CHAT'), null, { timeout: 30000 });
    log('the second chat read it');
    // Both chats are in the project, on the node, in that folder.
    const rows = await p.evaluate(async () => {
      const r = await (await fetch('/api/sessions')).json();
      return (r.sessions ?? r).map((s) => ({ project: s.project_id ?? null, cwd: s.cwd ?? null }));
    });
    const inProject = rows.filter((r) => r.project);
    check(inProject.length >= 2, 'both chats are in the project: ' + JSON.stringify(rows));
    check(inProject.every((r) => !r.cwd || r.cwd === F), 'both work in its folder: ' + JSON.stringify(inProject));
    log('chats in the project:', inProject.length);

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/project-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
