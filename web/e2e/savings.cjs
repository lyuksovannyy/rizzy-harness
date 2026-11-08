// Settings → Token savings: toggles persist, chats cap the style, graphify
// can be allowed for the built-in node (writes node.toml).
// Usage: node web/e2e/savings.cjs <harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2esv-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `# my node\nworkspace_roots = ["${S}"]\n`);
const URL_ = 'http://127.0.0.1:7793/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (c, m) => { if (!c) throw new Error(m); };

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7793'], { detached: true, stdio: 'ignore' });
  let p;
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    p = await b.newPage({ viewport: { width: 1280, height: 1100 } });
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await createOwner(p, code);
    await p.click('text=Skip: use the built-in demo');
    await keywordsOnly(p);
    await p.click('button:has-text("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');
    await p.goto(`${URL_}#/settings/server/savings`);
    await p.waitForSelector('text=Prompt caching');
    check(await p.isChecked('input[aria-label="Prompt caching"]'), 'caching on by default');
    check(!(await p.isChecked('input[aria-label="Repository map"]')), 'repo map off by default');
    // Chat style offers only off/lite.
    const chatOpts = await p.$$eval('select[aria-label="Chat style"] option', (o) => o.map((x) => x.value));
    check(JSON.stringify(chatOpts) === '["off","lite"]', 'chat styles: ' + chatOpts);
    await p.uncheck('input[aria-label="Hide old tool outputs"]');
    await p.selectOption('select[aria-label="Agentic style"]', 'full');
    await p.waitForSelector('text=Saved. Applies from the next message.');
    await p.reload();
    await p.click('.tabs button:has-text("Token savings")');
    await p.waitForSelector('text=Prompt caching');
    check(!(await p.isChecked('input[aria-label="Hide old tool outputs"]')), 'toggle persisted');
    check((await p.inputValue('select[aria-label="Agentic style"]')) === 'full', 'style persisted');
    const toml = fs.readFileSync(`${S}/data/config.toml`, 'utf8');
    check(/\[savings\][\s\S]*mask_old_outputs = false/.test(toml) && /agentic_style = "full"/.test(toml), 'config.toml:\n' + toml);
    log('toggles persist to config.toml');

    const g = p.locator('[data-testid=graphify]');
    await g.waitFor();
    await g.locator('text=This machine:').waitFor({ timeout: 15000 });
    const installed = await g.locator('text=/graphify \\d/').count();
    log('graphify on this machine:', installed ? 'installed' : 'not installed');
    if (installed) {
      await g.locator('label:has-text("Allow on the built-in node") input').click(); // controlled: flips after the save
      await g.locator('text=allowed for the built-in node').waitFor();
      const node = fs.readFileSync(`${S}/data/node.toml`, 'utf8');
      check(node.includes('# my node') && node.includes('graphify = true'), 'node.toml:\n' + node);
      log('allowed: node.toml updated, comments kept');
    }
    await p.screenshot({ path: `${S}/sv-1.png`, fullPage: true });
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    await b.close();
    log('screenshots in', S);
  } catch (e) {
    if (p) await p.screenshot({ path: `${S}/sv-fail.png`, fullPage: true }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
