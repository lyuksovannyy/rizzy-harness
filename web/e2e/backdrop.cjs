// The home screen's per-mode backgrounds: each appears for its mode, stays
// out of the column and of the accessibility tree, re-rolls on New chat,
// never shows the user's own titles, waits for the classifier to settle,
// honours reduced motion, Still and Off, pauses in a hidden tab, has no
// cursor light on touch, and the opt-in still copy sits beside chats only.
// It also measures the Agentic canvas's cost per tick.
// Usage: node web/e2e/backdrop.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { log, check, sleep, contrast } = require('./rich/lib.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2eb-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/data/harness.toml`, '[provider]\nkind = "mock"\nmock_delay_ms = 50\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n`);
const URL_ = 'http://127.0.0.1:7788/';
let PAGE = null;

const overlap = (a, b) => a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
const boxes = (p, sel) => p.$$eval(sel, (els) => els.map((e) => {
  const r = e.getBoundingClientRect();
  return { x: r.x, y: r.y, width: r.width, height: r.height, text: e.dataset.text };
}));
/** The wall as text@position, once its entrance has settled. */
async function wall(p, min = 8) {
  await p.waitForFunction((n) => !document.querySelector('.backdrop-layer[data-state=exit]')
    && document.querySelectorAll('.backdrop[data-mode=ask] .wall-line[data-text]').length >= n, min, { timeout: 10000 });
  await sleep(1600);
  return (await boxes(p, '.backdrop .wall-line')).map((l) => `${l.text}@${Math.round(l.x)},${Math.round(l.y)}`);
}
const mode = (p, name) => p.click(`.mode-switch button:has-text("${name}")`);
const home = async (p) => {
  await p.click('.new-chat');
  await p.waitForSelector('h1.greeting');
};
const spotOpacity = (p) => p.$eval('.bin-spot', (e) => Number(getComputedStyle(e).opacity));
/** Main-thread busy time per second over `ms`, from Chrome's own counters. */
async function busy(cdp, ms, during = async () => sleep(ms)) {
  const read = async () => Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
  const a = await read();
  await during();
  const b = await read();
  return ((b.TaskDuration - a.TaskDuration) * 1000) / (ms / 1000);
}
async function savePrefs(p, pick) {
  await p.goto(`${URL_}#/settings`);
  await p.click('.tabs button:has-text("Preferences")');
  await pick();
  await p.click('button:has-text("Save")');
  await p.waitForSelector('text=Saved.');
}

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7788'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 }, colorScheme: 'dark' });
    const p = await ctx.newPage();
    PAGE = p;
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    const wallFetches = [];
    p.on('request', (r) => r.url().includes('/wall/') && wallFetches.push(r.url().replace(URL_, '/')));
    await p.goto(URL_);
    await p.fill('input[placeholder="claim code"]', code);
    await p.fill('input[placeholder="username"]', 'alex');
    await p.fill('input[type=password]', 'admin password');
    await p.click('text=Create account');
    await p.click('text=Skip: use the built-in demo');
    await p.click('text=Skip: keywords only');
    await p.click('button:has-text("Continue")');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');

    // 1. Ask is the default: a wall of queries, clear of the column, out of
    // the accessibility tree, its list fetched once per page load.
    const first = await wall(p);
    const bd = p.locator('.backdrop');
    check((await bd.getAttribute('aria-hidden')) === 'true', 'the backdrop is aria-hidden');
    check((await bd.getAttribute('data-motion')) === 'animated', 'animated by default');
    check((await bd.innerText()) === '', 'the wall has no text in the DOM (pseudo-elements only)');
    const sample = (await boxes(p, '.backdrop .wall-line'))[0].text;
    check((await p.getByText(sample, { exact: true }).count()) === 0, 'wall text is not findable as text');
    const col = await p.locator('.home > .column').boundingBox();
    const lines = await boxes(p, '.backdrop .wall-line');
    check(lines.every((l) => !overlap(l, col)), 'no wall line under the column');
    check(wallFetches.filter((u) => u === '/wall/queries.txt').length === 1, 'queries fetched once: ' + wallFetches);
    log(`ask: ${first.length} lines, none under the column; fetched ${wallFetches.join(', ')}`);
    await p.screenshot({ path: `${S}/bg-ask-dark.png` });

    // 2. Search: the magnifier drops in above, never over the composer, and
    // the wall is gone within 700 ms.
    let t0 = Date.now();
    await mode(p, 'Search');
    await p.waitForSelector('.backdrop[data-mode=search] .mag', { timeout: 1500 });
    await p.waitForFunction(() => !document.querySelector('.backdrop-layer[data-mode=ask]'), null, { timeout: 700 });
    log(`search: magnifier in, wall gone after ${Date.now() - t0} ms`);
    await sleep(1300);
    const mag = await p.locator('.mag').boundingBox();
    check(!overlap(mag, await p.locator('.composer').boundingBox()), 'the magnifier is not over the composer');
    check(!overlap(mag, await p.locator('h1.greeting').boundingBox()), 'the magnifier is not over the greeting');
    await p.screenshot({ path: `${S}/bg-search-dark.png` });

    // 3. Agentic: the field runs, the light follows the mouse and fades
    // when it rests; the canvas's own tick cost stays low.
    await mode(p, 'Agentic');
    await p.waitForSelector('.bin-host[data-running=true]');
    await sleep(600);
    for (let i = 0; i < 8; i++) {
      await p.mouse.move(1150 + i * 4, 120 + i * 6);
      await sleep(40);
    }
    await sleep(300);
    const lit = await spotOpacity(p);
    check(lit > 0, 'the light shows while the mouse moves: ' + lit);
    await p.screenshot({ path: `${S}/bg-agentic-dark.png` });
    await sleep(3500);
    check((await spotOpacity(p)) === 0, 'the light fades once the mouse rests');
    await p.waitForFunction(() => document.querySelector('.bin-host')?.dataset.tickMs !== undefined);
    const cdp = await ctx.newCDPSession(p);
    await cdp.send('Performance.enable');
    const agenticBusy = await busy(cdp, 3000, async () => {
      for (let i = 0; i < 60; i++) {
        await p.mouse.move(1100 + (i % 20) * 6, 100 + (i % 10) * 8);
        await sleep(50);
      }
    });
    await sleep(2100);
    const tick = Number(await p.getAttribute('.bin-host', 'data-tick-ms'));
    log(`agentic: average tick ${tick.toFixed(2)} ms; main thread ${agenticBusy.toFixed(1)} ms/s with the light moving`);
    check(tick < 4, 'agentic tick under 4 ms: ' + tick);

    // 4. The classifier's pick waits 700 ms before the background follows.
    // A fresh page: once the user picks a mode, the bar stops guessing.
    await p.reload();
    await p.waitForSelector('h1.greeting');
    await wall(p);
    const askBusy = await busy(cdp, 3000);
    await p.fill('.composer textarea', 'weather riga');
    t0 = Date.now();
    check((await p.innerText('.mode-switch button[aria-checked=true]')).startsWith('Search'), 'the bar guessed Search');
    await sleep(550 - (Date.now() - t0));
    check((await bd.getAttribute('data-mode')) === 'ask', 'the background waits while the guess settles');
    await p.waitForFunction(() => document.querySelector('.backdrop')?.dataset.mode === 'search', null, { timeout: 1000 });
    log(`debounce: background followed the guess after ${Date.now() - t0} ms`);
    await sleep(1500);
    const searchBusy = await busy(cdp, 3000);
    log(`idle main thread: ask ${askBusy.toFixed(1)} ms/s, search ${searchBusy.toFixed(1)} ms/s`);
    await p.fill('.composer textarea', '');
    await p.waitForFunction(() => document.querySelector('.backdrop')?.dataset.mode === 'ask', null, { timeout: 2000 });

    // 5. New chat re-rolls the wall, without fetching the list again.
    const before = await wall(p);
    const fetched = wallFetches.length;
    await home(p);
    await p.waitForSelector('.backdrop-layer[data-state=exit]', { timeout: 2000 });
    const after = await wall(p);
    const kept = after.filter((l) => before.includes(l)).length;
    check(kept / after.length <= 0.2, `a new chat places a new wall (${kept}/${after.length} kept)`);
    check(wallFetches.length === fetched, 'the list is not fetched again on the same page');
    log(`new chat: ${after.length - kept}/${after.length} lines new`);

    // 6. The user's own chat title never shows, though the list has it.
    await p.fill('.composer textarea', 'how to tie a tie');
    await mode(p, 'Ask');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation .msg.assistant');
    for (let i = 0; i < 5; i++) {
      await home(p);
      const texts = (await wall(p)).map((l) => l.split('@')[0]);
      check(!texts.includes('how to tie a tie'), 'the wall never shows the user\'s own title');
    }
    log('own title never on the wall (5 visits)');

    // 7. A hidden tab pauses everything.
    const setHidden = (h) => p.evaluate((hidden) => {
      Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => (hidden ? 'hidden' : 'visible') });
      document.dispatchEvent(new Event('visibilitychange'));
    }, h);
    await setHidden(true);
    await p.waitForSelector('.backdrop[data-paused]');
    await setHidden(false);
    await p.waitForSelector('.backdrop:not([data-paused])');
    log('paused while hidden');

    // 8. Text keeps its contrast over every background, in both themes.
    for (const scheme of ['dark', 'light']) {
      await p.emulateMedia({ colorScheme: scheme });
      for (const m of ['Search', 'Ask', 'Agentic']) {
        await mode(p, m);
        await sleep(m === 'Ask' ? 1800 : 1300);
        const c = await contrast(p.locator('.subtitle'), p.locator('body'));
        check(c >= 4.5, `${m} ${scheme}: subtitle contrast ${c.toFixed(2)}`);
        if (scheme === 'light') await p.screenshot({ path: `${S}/bg-${m.toLowerCase()}-light.png` });
      }
    }
    await p.emulateMedia({ colorScheme: 'dark' });
    await mode(p, 'Ask');
    log('subtitle contrast ≥ 4.5 over each background in both themes');

    // 9. Touch: no light at all; a phone gets the three-tier wall.
    const state = await ctx.storageState();
    const phone = await b.newContext({ storageState: state, viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true, colorScheme: 'dark' });
    const q = await phone.newPage();
    await q.goto(URL_);
    await q.waitForSelector('h1.greeting');
    await wall(q, 6);
    const qcol = await q.locator('.home > .column').boundingBox();
    check((await boxes(q, '.backdrop .wall-line')).every((l) => !overlap(l, qcol)), 'no phone wall line under the column');
    check((await q.$$eval('.wall-line', (ls) => ls.every((l) => Number(l.dataset.tier) < 3))), 'three tiers on a phone');
    await q.screenshot({ path: `${S}/bg-ask-mobile-dark.png` });
    await mode(q, 'Agentic');
    await q.waitForSelector('.bin-host');
    await sleep(800);
    check((await q.locator('.bin-spot').count()) === 0, 'no cursor light on touch');
    await q.screenshot({ path: `${S}/bg-agentic-mobile-dark.png` });
    await mode(q, 'Search');
    await sleep(1400);
    await q.screenshot({ path: `${S}/bg-search-mobile-dark.png` });
    await q.emulateMedia({ colorScheme: 'light' });
    for (const m of ['Search', 'Ask', 'Agentic']) {
      await mode(q, m);
      await sleep(1800);
      await q.screenshot({ path: `${S}/bg-${m.toLowerCase()}-mobile-light.png` });
    }
    await phone.close();
    log('touch: no light; phone screenshots taken');

    // 10. Reduced motion: everything still, and still re-rolled per visit.
    const calm = await b.newContext({ storageState: state, viewport: { width: 1280, height: 800 }, reducedMotion: 'reduce', colorScheme: 'light' });
    const r = await calm.newPage();
    await r.goto(URL_);
    await r.waitForSelector('.backdrop[data-motion=still]');
    const still = await wall(r);
    await sleep(4000);
    check(JSON.stringify(await wall(r)) === JSON.stringify(still), 'a still wall does not change');
    await r.screenshot({ path: `${S}/bg-still-ask-light.png` });
    await mode(r, 'Search');
    await r.waitForSelector('.mag');
    check((await r.$eval('.mag-drop', (e) => getComputedStyle(e).animationName)) === 'none', 'the magnifier does not drop in');
    await mode(r, 'Agentic');
    await r.waitForSelector('.bin-host[data-running=false]');
    check((await r.locator('.bin-spot').count()) === 0, 'no light without motion');
    await calm.close();
    log('reduced motion: still wall, static magnifier, field not running');

    // 11. Settings: Off removes the backgrounds and their downloads; Still
    // keeps them without motion; the choice is mirrored for the next load.
    await savePrefs(p, async () => {
      await p.screenshot({ path: `${S}/settings-appearance-dark.png` });
      await p.click('.seg[aria-label=backgrounds] button:has-text("Off")');
      check(await p.locator('label.check:has-text("Also in chats") input').isDisabled(), 'Also in chats needs a background');
    });
    check((await p.evaluate(() => localStorage.getItem('harness.backdrop'))) === 'off', 'the choice is mirrored');
    wallFetches.length = 0;
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');
    await sleep(1500);
    check((await p.locator('.backdrop').count()) === 0, 'no background when turned off');
    check(wallFetches.length === 0, 'no wall downloads when turned off: ' + wallFetches);
    await savePrefs(p, () => p.click('.seg[aria-label=backgrounds] button:has-text("Still")'));
    await home(p);
    await p.waitForSelector('.backdrop[data-motion=still]');
    log('settings: off and still work, mirrored');

    // 12. Also in chats: a still wall beside a chat, a still field beside an
    // agentic session, only where the gutters are wide, never under the column.
    await savePrefs(p, async () => {
      await p.click('.seg[aria-label=backgrounds] button:has-text("Animated")');
      await p.check('label.check:has-text("Also in chats") input');
    });
    await p.emulateMedia({ colorScheme: 'light' });
    await sleep(400);
    await p.screenshot({ path: `${S}/settings-appearance-light.png` });
    await p.emulateMedia({ colorScheme: 'dark' });
    await p.click('.session-item:has-text("how to tie a tie")');
    await p.waitForSelector('.conversation.with-gutter');
    await sleep(800);
    check((await p.locator('.gutter .wall-line').count()) === 0, 'no gutter art when the gutters are narrow');
    await p.setViewportSize({ width: 1600, height: 900 });
    await p.waitForSelector('.gutter .wall-line', { timeout: 5000 });
    const column = await p.locator('.timeline .column').boundingBox();
    const side = await boxes(p, '.gutter .wall-line');
    check(side.every((l) => !overlap(l, column)), 'gutter lines stay beside the column');
    check(side.every((l) => l.text !== 'how to tie a tie'), 'not even beside its own chat');
    await p.screenshot({ path: `${S}/chat-gutter-chat-dark.png` });
    await home(p);
    await mode(p, 'Agentic');
    await p.fill('.agentic-row input', `${S}/ws`);
    await p.fill('.composer textarea', 'hello agent');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.conversation .msg.assistant');
    await p.waitForSelector('.gutter canvas');
    await sleep(500);
    await p.screenshot({ path: `${S}/chat-gutter-agentic-dark.png` });
    log(`gutter: ${side.length} still lines beside the chat, a still field beside the agentic session`);

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/b-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
