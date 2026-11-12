// Phase 4 end-to-end: similar earlier chats, mode prediction, and the search
// panel (Ctrl+K and "/", rows, the kept tab, Enter), from the sidebar and
// from its collapsed rail.
// Usage: node web/e2e/phase4.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
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
  await page.click('button:text-is("Continue")');
  await page.waitForSelector('h2:has-text("Search engine")');
  await page.click('button:has-text("Finish")');
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7799'], { detached: true, stdio: 'ignore' });
  try {
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const p = await b.newPage({ viewport: { width: 1280, height: 800 } });
    await keyed(p);
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

    // The search panel finds message text, not just titles. Ctrl+K opens it
    // anywhere, the composer included, with focus in its input.
    const panel = 'dialog.search-panel';
    const input = `${panel} input[role="combobox"]`;
    await p.focus('.composer textarea');
    await p.keyboard.press('Control+k');
    await p.waitForSelector(`${panel}[open]`);
    // The panel is a lazy chunk (13ca3e9): until it loads, a stand-in box
    // (no combobox role) holds focus and keeps what is typed. Either box must
    // have focus at once, and the loaded panel must take over focus and text.
    check(await p.evaluate(() => !!document.activeElement?.matches('dialog.search-panel[open] input')), 'focus is in the search input');
    await p.keyboard.type('worker thr');
    await p.waitForFunction(
      (sel) => document.activeElement === document.querySelector(sel) && document.activeElement.value === 'worker thr',
      input, { timeout: 10000 },
    ).catch(() => { throw new Error('the loaded search panel has focus and the typed text'); });
    await p.waitForSelector('.search-row mark');
    const row = (await p.innerText('.search-row')).replace(/\s+/g, ' ');
    log('search hit:', row.slice(0, 70));
    check(row.includes('· Chat'), 'a row names its type: ' + row);
    await p.screenshot({ path: `${S}/p4-3-search.png` });
    // A tab is kept for the next opening; Enter opens the active row and
    // closes the panel.
    await p.click(`${panel} [role="tab"]:has-text("Chats")`);
    await p.waitForSelector('.search-row mark');
    await p.focus(input);
    await p.keyboard.press('Enter');
    await p.waitForSelector(panel, { state: 'detached' });
    await p.waitForSelector('.conversation');
    log('opened a chat from the panel');
    // "/" opens it outside a text field, on the Chats tab; Esc closes it.
    await p.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
    await p.keyboard.press('/');
    await p.waitForSelector(`${panel}[open]`);
    check((await p.innerText(`${panel} [role="tab"][aria-selected="true"]`)) === 'Chats', 'the panel opens on the last tab used');
    await p.keyboard.press('Escape');
    await p.waitForSelector(panel, { state: 'detached' });
    // "/" typed in the composer is just text.
    await p.focus('.composer textarea');
    await p.keyboard.type('/');
    await sleep(200);
    check((await p.locator(panel).count()) === 0, '"/" in a text field types');
    await p.fill('.composer textarea', '');

    // The collapsed rail: icons with names, Search among them.
    await p.click('.sidebar button[aria-label="Collapse sidebar"]');
    await p.waitForSelector('.sidebar.rail');
    const rail = await p.$$eval('.sidebar.rail [aria-label]', (els) => els.map((e) => e.getAttribute('aria-label')));
    log('rail:', rail.join(' | '));
    check(['New chat', 'Search', 'Agents'].every((n) => rail.includes(n)), 'the rail names its icons');
    await p.click('.sidebar.rail button[aria-label="Search"]');
    await p.waitForSelector(`${panel}[open]`);
    await p.keyboard.press('Escape');
    await p.click('.sidebar.rail button[aria-label="Expand sidebar"]');
    await p.waitForSelector('.sidebar:not(.rail) .nav-item.new-chat');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
