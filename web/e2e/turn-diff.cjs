// A turn's changes (X.12) end to end: an agentic turn changes a file with a
// shell command, its "N files changed" row opens the changes beside the
// chat, the added line is there with its sign, a narrow screen shows the
// panel over the chat, and Undo still works from the row.
// Usage: node web/e2e/turn-diff.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-diff-'));
const D = `${S}/data`, R = `${S}/repo`;
fs.mkdirSync(D, { recursive: true });
fs.mkdirSync(R, { recursive: true });
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const freePort = () => new Promise((res) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const { port } = s.address(); s.close(() => res(port)); });
});
let PAGE = null;

(async () => {
  const port = await freePort();
  const URL_ = `http://127.0.0.1:${port}/`;
  fs.writeFileSync(`${D}/node.toml`, `workspace_roots = ["${R}"]\nmax_approval_mode = "ask"\n`);
  const git = (...a) => execFileSync('git', ['-C', R, ...a], { env: { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' } });
  git('init', '-q');
  fs.writeFileSync(`${R}/a.txt`, 'one\ntwo\nthree\n');
  git('add', '.');
  git('commit', '-qm', 'init');

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
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:text-is("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    // A turn whose change comes from a shell command.
    await p.fill('.composer textarea', 'run: printf "added by the turn\\n" >> a.txt; echo new > b.txt');
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.agentic-row input', R);
    await pickAgent(p);
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.undo >> text=2 files changed');

    // Open its changes from the row, by keyboard.
    await p.focus('.undo button:has-text("Show changes")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.diff-panel .diff-file');
    const added = await p.textContent('.diff-panel .diff-add');
    check(added.includes('added by the turn'), 'the added line is shown: ' + added);
    check((await p.textContent('.diff-panel .diff-add .diff-sign')).trim() === '+', 'the added line has its sign');
    const newLine = (await p.$$eval('.diff-panel .diff-add .diff-n', (n) => n.map((x) => x.textContent)))[1];
    check(newLine === '4', 'the added line is line 4: ' + newLine);
    check(await p.isVisible('.diff-panel >> text=b.txt'), 'the new file is listed');
    check(await p.evaluate(() => document.activeElement?.classList.contains('diff-title')), 'focus moved into the panel');
    await p.screenshot({ path: `${S}/diff-1-wide.png` });
    log('changes shown beside the chat:', (await p.textContent('.diff-title')).trim());

    // A narrow screen: the panel covers the chat; Escape closes it and
    // focus goes back to the row.
    await p.setViewportSize({ width: 390, height: 800 });
    const box = await p.$eval('.diff-panel', (e) => e.getBoundingClientRect().width);
    check(box > 300, 'the panel fills a narrow screen: ' + box);
    await p.screenshot({ path: `${S}/diff-2-narrow.png` });
    await p.keyboard.press('Escape');
    await p.waitForSelector('.diff-panel', { state: 'detached' });
    check(await p.evaluate(() => document.activeElement?.textContent === 'Show changes'), 'focus is back on the row');
    await p.setViewportSize({ width: 1280, height: 900 });

    // Undo still works from the row.
    await p.click('.undo button:has-text("Undo file changes")');
    await p.waitForSelector('.undo >> text=undone');
    check(fs.readFileSync(`${R}/a.txt`, 'utf8') === 'one\ntwo\nthree\n', 'undo restored a.txt');
    check(!fs.existsSync(`${R}/b.txt`), 'undo removed the created file');
    // The undone turn's changes still show what it did.
    await p.click('.undo button:has-text("Show changes")');
    await p.waitForSelector('.diff-panel .diff-add');
    log('undo works; the diff of the undone turn still opens');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/diff-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
