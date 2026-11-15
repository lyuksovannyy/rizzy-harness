// `@path` file mentions (T63, ADOPTION_PLAN X.17) end to end: in an agentic
// chat, typing `@src/ma` lists the folder's matching paths under "Files", a
// folder opens its entries, Enter picks one, a bare word still lists agents,
// a path with no match says so, and the sent message shows the mention as a
// chip with its path (the node read the file into the message).
// Usage: node web/e2e/file-mentions.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const net = require('net');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed, pickAgent } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-mentions-'));
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
  fs.writeFileSync(`${D}/node.toml`, `workspace_roots = ["${R}"]\nmax_approval_mode = "auto"\n`);
  fs.writeFileSync(`${R}/src/main.rs`, 'fn main() {}\n');
  fs.writeFileSync(`${R}/src/lib.rs`, 'pub fn x() {}\n');

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

    // An agentic chat in the folder.
    await p.fill('.composer textarea', 'hello');
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.agentic-row input', R);
    await pickAgent(p);
    await p.click('.composer .send');
    await p.waitForSelector('.msg.assistant');

    const box = '.composer textarea';
    // A folder token lists its entries under "Files".
    await p.click(box);
    await p.keyboard.type('fix @src/');
    await p.waitForSelector('.mention-popup >> text=Files');
    const listed = await p.$$eval('.mention-popup li[role="option"] b', (n) => n.map((x) => x.textContent));
    check(listed.includes('@src/main.rs') && listed.includes('@src/lib.rs'), 'the folder is listed: ' + listed);
    await p.keyboard.type('ma');
    await p.waitForFunction(() => [...document.querySelectorAll('.mention-popup li[role="option"] b')].map((x) => x.textContent).join() === '@src/main.rs');
    await p.keyboard.press('Enter');
    check((await p.inputValue(box)) === 'fix @src/main.rs ', 'picked: ' + (await p.inputValue(box)));
    check(!(await p.isVisible('.mention-popup')), 'the popup closed after the pick');
    await p.screenshot({ path: `${S}/mentions-1-picked.png` });
    // A path with no match says so before sending.
    await p.keyboard.type('and @nope.rs');
    await p.waitForSelector('.mention-none');
    log('no match:', (await p.textContent('.mention-none')).trim());
    await p.fill(box, 'fix @src/main.rs please');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.msg.user .chip >> text=src/main.rs');
    log('the sent message shows the mention as a chip');
    await p.screenshot({ path: `${S}/mentions-2-sent.png` });

    // A bare word lists agents, not files.
    await p.click(box);
    await p.keyboard.type('@');
    await p.waitForSelector('.mention-popup');
    check(!(await p.isVisible('.mention-popup >> text=Files')), 'a bare token lists agents only');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/mentions-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
