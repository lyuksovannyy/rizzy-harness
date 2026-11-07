// Phase 4 end-to-end: similar earlier chats, mode prediction, chat search.
// Usage: node web/e2e/phase4.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e4-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}"]\n`);
const URL_ = 'http://127.0.0.1:7799/';

/** Finish first-run onboarding with defaults (demo provider for admins). */
async function skipOnboarding(page, admin) {
  if (admin) {
    await page.waitForSelector('text=Connect an AI provider');
    await page.click('text=Skip: use the built-in demo');
    await keywordsOnly(page);
  }
  await page.waitForSelector('text=About you');
  await page.click('button:has-text("Continue")');
  await page.waitForSelector('h2:has-text("Search engine")');
  await page.click('button:has-text("Finish")');
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7799'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const p = await b.newPage({ viewport: { width: 1280, height: 800 } });
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await createOwner(p, code);
    await skipOnboarding(p, true);
    await p.waitForSelector('h1.greeting');

    const mode = () => p.innerText('.mode-switch button[aria-checked=true]');
    const typeFresh = async (text) => {
      await p.fill('.composer textarea', '');
      await p.focus('.composer textarea');
      await p.keyboard.type(text);
      await sleep(150);
    };

    // An earlier chat to find later.
    await typeFresh('how do I configure tokio runtime worker threads');
    log('predicted for a question:', await mode());
    check((await mode()).startsWith('Ask'), 'question should predict Ask');
    await p.keyboard.press('Enter');
    await p.waitForSelector('text=You said: how do I configure tokio runtime worker threads');

    // Similar chats under the bar.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await typeFresh('configure tokio runtime thr');
    await p.waitForSelector('.similar button', { timeout: 5000 });
    log('similar:', (await p.innerText('.similar')).replace(/\s+/g, ' ').slice(0, 80));
    await p.screenshot({ path: `${S}/p4-1-similar.png` });
    await p.keyboard.press('ArrowDown');
    check(await p.evaluate(() => document.activeElement?.closest('.similar') !== null), 'ArrowDown moves into the list');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    log('opened the earlier chat from the panel');

    // Time-sensitive asks get no matches.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await typeFresh('tokio runtime news today');
    await sleep(600);
    check((await p.locator('.similar').count()) === 0, 'time-sensitive text must not suggest');
    log('time-sensitive: no suggestions');

    // Mode prediction.
    await typeFresh('youtube');
    log('youtube ->', await mode());
    check((await mode()).startsWith('Search'), 'youtube should predict Search');
    await typeFresh('fix the failing test in src/lib.rs');
    log('dev task ->', await mode());
    check((await mode()).startsWith('Agentic'), 'dev task should predict Agentic');
    // A manual choice is not overridden while typing.
    await typeFresh('youtube');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.focus('.composer textarea');
    await p.keyboard.type(' com');
    await sleep(150);
    log('after manual Ask + typing ->', await mode());
    check((await mode()).startsWith('Ask'), 'manual mode must stick');
    await p.screenshot({ path: `${S}/p4-2-manual.png` });

    // Learning reaches the server as weights only.
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation');
    await sleep(1500);
    const cls = await p.evaluate(() => fetch('/api/me/classifier').then((r) => r.json()));
    log('classifier labels stored:', cls && cls.labels, '| keys:', cls && Object.keys(cls).join(','));
    check(cls && cls.labels >= 2, 'classifier state saved');
    check(!JSON.stringify(cls).includes('youtube'), 'no text stored');

    // Sidebar full-text search finds message text, not just titles.
    await p.click('.sidebar button[aria-label="Search chats"]');
    await p.fill('.sidebar-search', 'worker thr');
    await p.waitForSelector('.session-item.hit');
    log('sidebar hit:', (await p.innerText('.session-item.hit')).replace(/\s+/g, ' ').slice(0, 70));
    await p.screenshot({ path: `${S}/p4-3-sidebar.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
