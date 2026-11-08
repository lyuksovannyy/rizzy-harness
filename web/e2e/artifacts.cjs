// Artifacts end to end: an agent publishes a built file (the mock model's
// "publish:" prompt), the chat's card follows the upload, and the download
// from the Artifacts page carries the same bytes (sha256); a file written in
// an agentic chat is saved by hand with "Save as artifact" and lands there too.
// Usage: node web/e2e/artifacts.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2eart-'));
const R = `${S}/ws`;
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${R}/target/release`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${R}"]\n`);
// A "build output" of a few MB, so the upload takes more than one chunk.
const built = crypto.randomBytes(3 * 1024 * 1024 + 17);
fs.writeFileSync(`${R}/target/release/myapp`, built);
const sha = (buf) => crypto.createHash('sha256').update(buf).digest('hex');
const URL_ = 'http://127.0.0.1:7796/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7796'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 }, acceptDownloads: true });
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
    const agentic = async (text) => {
      await p.click('aside.sidebar a:has-text("New chat")');
      await p.waitForSelector('h1.greeting');
      await p.fill('.composer textarea', text);
      await p.click('.mode-switch button:has-text("Agentic")');
      await p.fill('.agentic-row input', R);
      await p.click('.composer .send');
      await p.waitForSelector('.conversation');
    };
    const download = async (click) => {
      const [d] = await Promise.all([p.waitForEvent('download'), click()]);
      return fs.readFileSync(await d.path());
    };

    // 1. The agent publishes; publishing asks first, then the card follows the upload.
    await agentic('publish: target/release/myapp');
    await p.click('.approval button.primary:has-text("Allow")');
    await p.waitForSelector('.artifact-card >> text=In Artifacts', { timeout: 30000 });
    log('card:', (await p.innerText('.artifact-card')).replace(/\s+/g, ' '));
    await p.screenshot({ path: `${S}/art-1-card.png` });
    const fromCard = await download(() => p.click('.artifact-card a:has-text("Download")'));
    check(sha(fromCard) === sha(built), 'the card\'s download has the built bytes');

    // 2. The Artifacts page: one row, Download latest, the version's sha256.
    await p.click('.artifact-card a:has-text("Open in Artifacts")');
    await p.waitForSelector('h1.artifact-title:has-text("myapp")');
    const shown = await p.$eval('.artifact-fact:has(dt:text-is("sha256")) dd', (d) => d.textContent);
    check(shown === sha(built), 'the page shows the sha256 of the built file');
    const latest = await download(() => p.click('.artifact-actions a:has-text("Download")'));
    check(sha(latest) === sha(built), 'Download latest has the built bytes');
    await p.screenshot({ path: `${S}/art-2-detail.png` });
    log('published, downloaded twice, sha256 matches');

    // 3. Save as artifact on a file an agentic turn wrote.
    await agentic('run: echo "release notes" > NOTES.txt');
    await p.click('.approval button.primary:has-text("Allow")');
    await p.waitForSelector('.tool .tool-status.s-ok');
    await p.click('button[aria-label="Save a file as an artifact"]');
    await p.fill('.save-file-bar input[aria-label="File or folder to save"]', 'NOTES.txt');
    await p.click('.save-file-bar button:has-text("Save as artifact")');
    await p.waitForSelector('.artifact-card:has-text("Saved") >> text=In Artifacts', { timeout: 30000 });
    const notes = await download(() => p.click('.artifact-card:has-text("Saved") a:has-text("Download")'));
    check(notes.toString() === 'release notes\n', 'the saved file downloads as written');
    await p.click('aside.sidebar a:has-text("Artifacts")');
    await p.waitForSelector('.artifact-row a.agent-name:has-text("NOTES.txt")');
    log('rows:', (await p.$$eval('.artifact-row a.agent-name', (as) => as.map((a) => a.textContent))).join(', '));
    await p.screenshot({ path: `${S}/art-3-list.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/art-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
