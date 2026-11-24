// All of a chat's changes (X.61) end to end: two agentic turns change files
// with shell commands, the header's "All changes" opens the panel with both
// turns' changes in one view (the net change of a file both touched), the
// panel has no undo or rewind, a narrow screen shows it over the chat, Escape
// closes it and focus goes back to the button, and after the second turn is
// undone the range no longer holds its file.
// Usage: node web/e2e/chat-diff.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-chatdiff-'));
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
    await pickFolder(p, R);
    await pickAgent(p);
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.undo >> text=2 files changed');

    // The second turn, in the same chat.
    await p.fill('.composer textarea', 'run: printf "second turn\\n" >> a.txt; echo more > c.txt');
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForFunction(() => document.querySelectorAll('.undo').length >= 2 && [...document.querySelectorAll('.undo')].every((u) => /files? changed/.test(u.textContent)));

    // The header's button, by keyboard.
    const btn = 'button[aria-label="All changes"]';
    await p.focus(btn);
    await p.keyboard.press('Enter');
    await p.waitForSelector('.diff-panel .diff-file');
    check((await p.textContent('.diff-title')).includes('All changes in this chat'), 'title: ' + await p.textContent('.diff-title'));
    const names = await p.$$eval('.diff-panel nav .diff-jump .mono', (n) => n.map((x) => x.textContent.trim()).sort());
    check(JSON.stringify(names) === JSON.stringify(['a.txt', 'b.txt', 'c.txt']), 'files of both turns: ' + names);
    const adds = (await p.$$eval('.diff-panel .diff-add', (n) => n.map((x) => x.textContent))).join('|');
    check(adds.includes('added by the turn') && adds.includes('second turn'), 'both turns\' lines in a.txt: ' + adds);
    check(await p.isVisible('.diff-panel >> text=c.txt'), 'the second turn\'s new file is listed');
    const tools = (await p.textContent('.diff-panel .diff-tools')).trim();
    check(tools === '', 'no undo or rewind in the whole-chat panel: ' + tools);
    check(await p.evaluate(() => document.activeElement?.classList.contains('diff-title')), 'focus moved into the panel');
    await p.screenshot({ path: `${S}/chat-diff-1-wide.png` });
    log('whole chat:', (await p.textContent('.diff-title')).trim());

    // A narrow screen: over the chat; Escape closes it, focus is on the button.
    await p.setViewportSize({ width: 390, height: 800 });
    const w = await p.$eval('.diff-panel', (e) => e.getBoundingClientRect().width);
    check(w > 300, 'the panel fills a narrow screen: ' + w);
    await p.screenshot({ path: `${S}/chat-diff-2-narrow.png` });
    await p.keyboard.press('Escape');
    await p.waitForSelector('.diff-panel', { state: 'detached' });
    check(await p.evaluate(() => document.activeElement?.getAttribute('aria-label') === 'All changes'), 'focus is back on the button');
    await p.setViewportSize({ width: 1280, height: 900 });

    // Undo the second turn from its row: the range drops its file.
    await p.locator('.undo button:has-text("Undo file changes")').last().click();
    await p.waitForFunction(() => /undone/.test(document.querySelectorAll('.undo')[1]?.textContent || ''));
    await p.click(btn);
    await p.waitForSelector('.diff-panel .diff-file');
    const after = await p.$$eval('.diff-panel nav .diff-jump .mono', (n) => n.map((x) => x.textContent.trim()).sort());
    check(JSON.stringify(after) === JSON.stringify(['a.txt', 'b.txt']), 'the undone turn is out of the range: ' + after);
    const adds2 = (await p.$$eval('.diff-panel .diff-add', (n) => n.map((x) => x.textContent))).join('|');
    check(!adds2.includes('second turn'), 'its line is out too: ' + adds2);
    log('after the undo:', after.join(', '));

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/chat-diff-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
