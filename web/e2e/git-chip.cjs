// The header's git chip (X.64) end to end: a coding chat in a repository on a
// named branch shows it, with no count while the work tree is clean; after a
// turn that writes a file the count appears without a reload; a click reads it
// again; a subfolder of a repository shows the repository's branch; a folder
// that is no repository shows no chip; a hostile branch (markup, 90 characters)
// is inert text cut for display, and at phone width the header does not
// overflow.
// Usage: node web/e2e/git-chip.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-gitchip-'));
const D = `${S}/data`, W = `${S}/work`;
const R1 = `${W}/repo`, R2 = `${W}/hostile`, PLAIN = `${W}/plain`;
for (const d of [D, R1, `${R1}/pkg`, R2, PLAIN]) fs.mkdirSync(d, { recursive: true });
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});
let PAGE = null;
const LONG = 'feature/' + 'long-branch-name-'.repeat(5) + '<b>x</b>'; // 8 + 85 + 8 = 101 characters

(async () => {
  const port = await freePort();
  const URL_ = `http://127.0.0.1:${port}/`;
  fs.writeFileSync(`${D}/node.toml`, `workspace_roots = ["${W}"]\nmax_approval_mode = "ask"\n`);
  const env = { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t', GIT_CONFIG_GLOBAL: '/dev/null' };
  const git = (dir, ...a) => execFileSync('git', ['-C', dir, ...a], { env });
  git(R1, 'init', '-q', '-b', 'feature/git-chip');
  fs.writeFileSync(`${R1}/a.txt`, 'one\n');
  fs.writeFileSync(`${R1}/pkg/p.txt`, 'p\n');
  git(R1, 'add', '.');
  git(R1, 'commit', '-qm', 'init');
  git(R2, 'init', '-q', '-b', LONG);
  fs.writeFileSync(`${R2}/a.txt`, 'one\n');
  git(R2, 'add', '.');
  git(R2, 'commit', '-qm', 'init');
  fs.writeFileSync(`${PLAIN}/a.txt`, 'not a repository\n');

  const srv = spawn(BIN, ['--data-dir', D, 'serve', '--bind', `127.0.0.1:${port}`], { detached: true, stdio: 'ignore' });
  try {
    const code = await claimCode(`${D}/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 900 } });
    await keyed(ctx);
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    const gitCalls = [];
    p.on('request', (r) => { if (/\/api\/sessions\/[^/]+\/git$/.test(r.url())) gitCalls.push(r.url()); });
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    // 1. A chat in the repository: the branch, no count (the work tree is clean).
    await p.fill('.composer textarea', 'run: cat a.txt');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, R1);
    await pickAgent(p);
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.msg.assistant:not(.streaming)');
    await p.waitForSelector('.git-chip');
    check((await p.textContent('.git-chip .git-name')) === 'feature/git-chip', 'branch: ' + await p.textContent('.git-chip'));
    check((await p.$('.git-chip .git-count')) === null, 'a clean work tree has no count');
    const title = await p.getAttribute('.git-chip', 'title');
    check(/^Branch feature\/git-chip; no changed files/.test(title), 'title: ' + title);
    await p.screenshot({ path: `${S}/git-chip-1-clean.png` });
    log('clean:', title);

    // 2. A turn writes a file and changes another: the count appears with no reload.
    const before = gitCalls.length;
    await p.fill('.composer textarea', 'run: echo new > new.txt; echo two >> a.txt');
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.undo >> text=2 files changed');
    await p.waitForSelector('.git-chip .git-count');
    check((await p.textContent('.git-chip .git-count')) === '2 changed', 'count: ' + await p.textContent('.git-chip'));
    check(gitCalls.length > before, 'read again at the end of the turn');
    await p.screenshot({ path: `${S}/git-chip-2-changed.png` });
    // A person commits outside the chat; a click reads it again.
    git(R1, 'add', '.');
    git(R1, 'commit', '-qm', 'second');
    await p.click('.git-chip');
    await p.waitForFunction(() => !document.querySelector('.git-chip .git-count'));
    log('after the click:', await p.getAttribute('.git-chip', 'title'));
    // No polling: with the page idle the number of reads stays put.
    const idle = gitCalls.length;
    await p.waitForTimeout(2500);
    check(gitCalls.length === idle, 'the chip polled: ' + (gitCalls.length - idle));

    // 3. A phone: the header holds.
    await p.setViewportSize({ width: 390, height: 800 });
    await p.reload();
    await p.waitForSelector('.git-chip');
    const phone = await p.evaluate(() => ({ overflow: document.documentElement.scrollWidth > window.innerWidth, right: document.querySelector('.git-chip').getBoundingClientRect().right }));
    check(!phone.overflow && phone.right <= 390, 'phone header: ' + JSON.stringify(phone));
    await p.screenshot({ path: `${S}/git-chip-3-phone.png` });
    await p.setViewportSize({ width: 1280, height: 900 });

    // 4. A subfolder of the repository shows the repository's branch.
    await p.goto(URL_);
    await p.click('text=New chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'run: cat p.txt');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, `${R1}/pkg`);
    await pickAgent(p);
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.git-chip');
    check((await p.textContent('.git-chip .git-name')) === 'feature/git-chip', 'subfolder: ' + await p.textContent('.git-chip'));

    // 5. A folder that is no repository: no chip, no error.
    await p.goto(URL_);
    await p.click('text=New chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'run: cat a.txt');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, PLAIN);
    await pickAgent(p);
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.chat-suffix');
    await p.waitForTimeout(1500);
    check((await p.$('.git-chip')) === null, 'a chip for a folder that is no repository');
    await p.screenshot({ path: `${S}/git-chip-4-no-repository.png` });

    // 6. A hostile branch: text, cut for display, nothing parsed as markup, and the phone header holds.
    await p.goto(URL_);
    await p.click('text=New chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'run: cat a.txt');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, R2);
    await pickAgent(p);
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.git-chip');
    const name = await p.textContent('.git-chip .git-name');
    check(Array.from(name).length === 81 && name.endsWith('…') && name.startsWith('feature/long-branch'), 'cut name: ' + name);
    check((await p.$('.conv-head b')) === null, 'markup in a branch name became an element');
    check((await p.getAttribute('.git-chip', 'title')).includes('<b>x</b>'), 'the hover text holds the whole name');
    await p.setViewportSize({ width: 390, height: 800 });
    await p.reload();
    await p.waitForSelector('.git-chip');
    const hostile = await p.evaluate(() => ({ overflow: document.documentElement.scrollWidth > window.innerWidth, right: document.querySelector('.git-chip').getBoundingClientRect().right }));
    check(!hostile.overflow && hostile.right <= 390, 'phone header with a long branch: ' + JSON.stringify(hostile));
    await p.screenshot({ path: `${S}/git-chip-5-long-branch-phone.png` });
    await p.setViewportSize({ width: 1280, height: 900 });
    await p.reload();
    await p.waitForSelector('.git-chip');
    await p.screenshot({ path: `${S}/git-chip-6-long-branch.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/git-chip-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
