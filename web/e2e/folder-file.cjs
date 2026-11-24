// A text file of a coding chat's folder in the side panel (X.63) end to end:
// "Open" on a changed file of the diff and on a file card, line numbers, the
// panel replacing the diff, a file that changed shown anew by Reload, a file
// of hostile content (markup, an escape sequence, a direction override, a
// megabyte on one line) as inert text without breaking the layout, a binary
// file, a deleted file and a link that leads out as one plain sentence each,
// Escape and focus, a phone-width screen.
// Usage: node web/e2e/folder-file.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent, pickFolder } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-folderfile-'));
const D = `${S}/data`, R = `${S}/repo`;
fs.mkdirSync(D, { recursive: true });
fs.mkdirSync(`${R}/src`, { recursive: true });
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
  fs.writeFileSync(`${R}/src/main.rs`, 'fn main() {\n    println!("one");\n}\n');
  fs.writeFileSync(`${R}/gone.txt`, 'soon gone\n');
  fs.writeFileSync(`${R}/swap.txt`, 'a plain file\n');
  git('add', '.');
  git('commit', '-qm', 'init');
  // Untracked files of every awkward kind.
  fs.writeFileSync(`${R}/hostile.html`, '<script>window.__pwned = 1</script>\n<img src=x onerror="window.__pwned = 2">\n# not markdown\n[a link](https://evil.example/)\n\u001b[31mred\u001b[0m and ‮evil‬ and zero​width\n');
  fs.writeFileSync(`${R}/big.log`, 'x'.repeat(1024 * 1024) + '\n');
  const secretFile = `${S}/outside.txt`;
  fs.writeFileSync(secretFile, 'outside the folder\n');

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

    // A turn that changes a file and adds one.
    await p.fill('.composer textarea', 'run: printf "    println!(\\"two\\");\\n" >> src/main.rs; echo new > new.txt');
    await p.click('.mode-switch button:has-text("Agentic")');
    await pickFolder(p, R);
    await pickAgent(p);
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('.undo >> text=2 files changed');

    // 1. From a changed file's row in the diff.
    await p.click('button[aria-label="All changes"]');
    await p.waitForSelector('.diff-panel .diff-file');
    const row = p.locator('.diff-panel summary button[aria-label="Open main.rs"]');
    check(await row.count() === 1, 'the changed file has an Open control');
    const wasOpen = await p.$eval('.diff-panel details.diff-file', (d) => d.open);
    await row.click();
    await p.waitForSelector('.file-view .fv-text');
    check(await p.$('.diff-panel') === null, 'the file panel replaced the diff panel');
    check(await p.$eval('.file-view h2', (h) => h.textContent) === 'main.rs', 'title');
    check((await p.textContent('.fv-path')).trim() === 'src/main.rs', 'the path is shown');
    const text1 = await p.textContent('.fv-text');
    check(text1.includes('println!("two");') && text1.startsWith('fn main() {'), 'the file as it is now: ' + text1);
    const gutter = await p.textContent('.fv-gutter');
    check(gutter === '1\n2\n3\n4', 'line numbers: ' + JSON.stringify(gutter));
    check(/Read-only/.test(await p.textContent('.file-view')) && /as of /.test(await p.textContent('.file-view')), 'size and as-of line');
    check(await p.evaluate(() => document.activeElement?.tagName === 'H2'), 'focus moved into the panel');
    await p.screenshot({ path: `${S}/folder-file-1-diff-wide.png` });
    log('opened from the diff; the row had been open:', wasOpen);

    // 2. The file changes; Reload shows it.
    fs.appendFileSync(`${R}/src/main.rs`, '// added later\n');
    await p.click('button[aria-label="Reload the file"]');
    await p.waitForFunction(() => document.querySelector('.fv-text')?.textContent.includes('// added later'));
    check(await p.textContent('.fv-gutter') === '1\n2\n3\n4\n5', 'five lines after the reload');
    // Highlighted by the chat's own highlighter (a worker), as text and spans.
    await p.waitForSelector('.fv-text .token', { timeout: 10000 }).catch(() => {});
    const tokens = await p.$$eval('.fv-text .token', (n) => n.length);
    log('highlight tokens:', tokens);
    check(await p.textContent('.fv-text') === text1 + '// added later\n', 'highlighting changed no character');

    // 3. Escape closes; focus returns to the chat (the diff's control is gone).
    await p.keyboard.press('Escape');
    await p.waitForSelector('.file-view', { state: 'detached' });

    // 4. From a file card.
    // The card has its control once it is ok; the next message waits for the turn to end (the send button is a stop button until then).
    const cards = async (n) => {
      await p.waitForFunction((k) => document.querySelectorAll('.tool-open').length >= k, n, { timeout: 30000 });
      await p.waitForSelector('.composer .send:not(.stop)', { timeout: 30000 });
    };
    let n = 0;
    const ask = async (cmd) => {
      await p.fill('.composer textarea', cmd);
      await p.click('.composer .send');
      n += 1;
      await cards(n);
    };
    await ask('read: src/main.rs');
    const card = p.locator('.tool-open button[aria-label="Open main.rs"]').last();
    await card.click();
    await p.waitForSelector('.file-view .fv-text');
    check((await p.textContent('.fv-text')).includes('// added later'), 'opened from the card');
    await p.keyboard.press('Escape');
    await p.waitForSelector('.file-view', { state: 'detached' });
    check(await p.evaluate(() => document.activeElement?.getAttribute('aria-label') === 'Open main.rs'), 'focus is back on the card\'s control');

    // 5. Hostile content is inert text.
    await ask('read: hostile.html');
    await p.locator('.tool-open button[aria-label="Open hostile.html"]').last().click();
    await p.waitForSelector('.file-view .fv-text');
    const shown = await p.textContent('.fv-text');
    check(shown.includes('<script>window.__pwned = 1</script>'), 'the markup is text: ' + shown);
    check(shown.includes('<U+001B>[31mred') && shown.includes('<U+202E>evil') && shown.includes('zero<U+200B>width'), 'hidden characters as codes: ' + JSON.stringify(shown));
    check(await p.$('.file-view script, .file-view img, .file-view a') === null, 'no element came from the file');
    check(await p.evaluate(() => window.__pwned === undefined), 'nothing ran');
    await p.screenshot({ path: `${S}/folder-file-2-hostile.png` });
    await p.keyboard.press('Escape');

    // 6. A megabyte on one line: bounded, scrolls, the page does not break.
    await ask('read: big.log');
    await p.locator('.tool-open button[aria-label="Open big.log"]').last().click();
    await p.waitForSelector('.file-view .fv-text');
    const big = await p.evaluate(() => {
      const t = document.querySelector('.fv-text');
      const body = document.querySelector('.fv-body');
      const r = document.querySelector('.file-view').getBoundingClientRect();
      return { len: t.textContent.length, note: document.querySelector('.pv-more')?.textContent || '', scrolls: body.scrollWidth > body.clientWidth, panelRight: r.right, pageOverflow: document.documentElement.scrollWidth > window.innerWidth, gutter: document.querySelector('.fv-gutter').textContent };
    });
    check(big.len === 256 * 1024 && /first 256 KB of this 1\.0 MB file/.test(big.note) && big.scrolls && !big.pageOverflow && big.gutter === '1', 'bounded long line: ' + JSON.stringify(big));
    await p.screenshot({ path: `${S}/folder-file-3-long-line.png` });
    await p.keyboard.press('Escape');

    // 7. Binary (the agent's own read refuses it; the diff row lists it), deleted,
    // swapped for a link out: one sentence each.
    await p.fill('.composer textarea', "run: printf '\\000\\001\\002' > made.bin");
    await p.click('.composer .send');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForFunction(() => document.querySelectorAll('.undo').length >= 2 && [...document.querySelectorAll('.undo')].every((u) => /files? changed/.test(u.textContent)), null, { timeout: 30000 });
    await p.waitForSelector('.composer .send:not(.stop)');
    await p.click('button[aria-label="All changes"]');
    await p.waitForSelector('.diff-panel .diff-file');
    await p.locator('.diff-panel summary button[aria-label="Open made.bin"]').click();
    await p.waitForSelector('.file-view [role=alert]');
    check(/not text/.test(await p.textContent('.file-view [role=alert]')), 'binary: ' + await p.textContent('.file-view [role=alert]'));
    check(await p.$('.fv-text') === null, 'no text for a binary file');
    await p.keyboard.press('Escape');

    await ask('read: gone.txt');
    fs.unlinkSync(`${R}/gone.txt`);
    await p.locator('.tool-open button[aria-label="Open gone.txt"]').last().click();
    await p.waitForSelector('.file-view [role=alert]');
    check(/unknown file/.test(await p.textContent('.file-view [role=alert]')), 'deleted: ' + await p.textContent('.file-view [role=alert]'));
    await p.keyboard.press('Escape');

    await ask('read: swap.txt');
    fs.unlinkSync(`${R}/swap.txt`);
    fs.symlinkSync(secretFile, `${R}/swap.txt`);
    await p.locator('.tool-open button[aria-label="Open swap.txt"]').last().click();
    await p.waitForSelector('.file-view [role=alert]');
    const out = await p.textContent('.file-view');
    check(/unknown file/.test(out) && !out.includes('outside the folder'), 'a link out: ' + out);
    await p.screenshot({ path: `${S}/folder-file-4-refused.png` });
    await p.keyboard.press('Escape');

    // 8. Phone width: the panel fills the screen and holds.
    await p.setViewportSize({ width: 390, height: 800 });
    await p.reload();
    await p.waitForSelector('.tool-open button[aria-label="Open main.rs"]');
    await p.locator('.tool-open button[aria-label="Open main.rs"]').first().click();
    await p.waitForSelector('.file-view .fv-text');
    const phone = await p.evaluate(() => {
      const r = document.querySelector('.file-view').getBoundingClientRect();
      return { w: r.width, overflow: document.documentElement.scrollWidth > window.innerWidth };
    });
    check(phone.w > 300 && !phone.overflow, 'phone: ' + JSON.stringify(phone));
    await p.screenshot({ path: `${S}/folder-file-5-phone.png` });
    await p.keyboard.press('Escape');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/folder-file-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
