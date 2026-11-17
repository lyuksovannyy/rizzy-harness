// The account menu, Profile and Personalisation against the real binary
// (README "Profile and Personalisation", PLAN.md T74): the menu's order
// (Workspace settings in the main menu, not the flyout; Settings and Log out
// side by side), an avatar and a banner picked through the page's own file
// inputs (made small in the browser, stored, shown), the profile saved with
// About you, a server refusal shown under its field, Personalisation's
// choices and instructions saved by their own call, and Workspace settings →
// Agents. Screenshots, light and dark, go to the shots folder.
// Usage: node web/e2e/profile.cjs <path-to-harness-binary> [work-dir] [shots-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2ep-'));
const SHOTS = process.argv[4] || `${S}/shots`;
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(SHOTS, { recursive: true });
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 20\nmodels = [{ id = "mock" }]\n');
const PORT = 7983;
const URL_ = `http://127.0.0.1:${PORT}/`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
async function claimCode() {
  for (let i = 0; i < 600; i++) {
    try {
      await fetch(URL_);
      return fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    } catch {
      await sleep(100);
    }
  }
  throw new Error('the server did not start within a minute');
}
const api = (p, method, url, body) =>
  p.evaluate(
    async ([m, u, b]) => {
      const r = await fetch(u, { method: m, headers: { 'content-type': 'application/json' }, body: b ? JSON.stringify(b) : undefined });
      return { status: r.status, body: await r.json().catch(() => null) };
    },
    [method, url, body],
  );
/** A PNG drawn in the page: a gradient of `w` × `h`. */
async function picture(p, w, h, a, b) {
  const url = await p.evaluate(
    ([w, h, a, b]) => {
      const c = document.createElement('canvas');
      c.width = w;
      c.height = h;
      const g = c.getContext('2d');
      const grad = g.createLinearGradient(0, 0, w, h);
      grad.addColorStop(0, a);
      grad.addColorStop(1, b);
      g.fillStyle = grad;
      g.fillRect(0, 0, w, h);
      g.fillStyle = 'rgba(255,255,255,0.35)';
      g.beginPath();
      g.arc(w * 0.7, h * 0.4, Math.min(w, h) * 0.25, 0, Math.PI * 2);
      g.fill();
      return c.toDataURL('image/png');
    },
    [w, h, a, b],
  );
  return Buffer.from(url.split(',')[1], 'base64');
}
let PAGE = null;
async function both(p, name) {
  for (const scheme of ['light', 'dark']) {
    await p.emulateMedia({ colorScheme: scheme });
    await sleep(150);
    await p.screenshot({ path: `${SHOTS}/${name}-${scheme}.png` });
  }
  await p.emulateMedia({ colorScheme: 'light' });
}
async function openMenu(p) {
  await p.click('.sidebar-foot .nav-item.account');
  await p.waitForSelector('[role=menu][aria-label=Account]');
  await sleep(200);
}

(async () => {
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', `127.0.0.1:${PORT}`], { detached: true, stdio: 'ignore' });
  log('server pid', srv.pid, 'data', `${S}/data`);
  try {
    const code = await claimCode();
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 } });
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
    await p.click('button:text-is("Finish")');
    await p.waitForSelector('h1.greeting');

    // 1. The account menu.
    await openMenu(p);
    const items = await p.$$eval('[role=menu][aria-label=Account] [role^=menuitem]', (els) => els.map((e) => e.textContent.trim()));
    log('menu:', items.join(' | '));
    check(items.slice(1).join('|') === 'Profile|Personalisation|Workspace settings|Settings|Log out', `menu order: ${items}`);
    const row = await p.$$eval('.menu-split [role=menuitem]', (els) => els.map((e) => [e.textContent.trim(), Math.round(e.getBoundingClientRect().top)]));
    check(row.length === 2 && row[0][1] === row[1][1], `Settings and Log out on one row: ${JSON.stringify(row)}`);
    await both(p, 'menu');
    await p.keyboard.press('ArrowRight');
    await p.waitForSelector('[role=menu][aria-label=Workspaces]');
    const sub = await p.innerText('[role=menu][aria-label=Workspaces]');
    check(!sub.includes('Workspace settings'), 'Workspace settings left the flyout');
    await p.screenshot({ path: `${SHOTS}/menu-workspaces-light.png` });
    await p.keyboard.press('Escape');
    await p.keyboard.press('Escape');

    // 2. Profile: pictures through the page's own inputs, then the fields.
    await openMenu(p);
    await p.click('[role=menu][aria-label=Account] [role=menuitem]:has-text("Profile")');
    await p.waitForSelector('.profile-card');
    await p.setInputFiles('input[aria-label="Choose banner"]', { name: 'banner.png', mimeType: 'image/png', buffer: await picture(p, 2400, 1200, '#3a7bd5', '#8e44ad') });
    await p.waitForSelector('.profile-banner-pick img');
    await p.setInputFiles('input[aria-label="Choose avatar"]', { name: 'me.png', mimeType: 'image/png', buffer: await picture(p, 900, 600, '#f39c12', '#e74c3c') });
    await p.waitForSelector('.profile-avatar-pick img');
    const imgs = (await api(p, 'GET', '/api/me/profile-images')).body;
    check(imgs.avatar && imgs.banner, `both stored: ${JSON.stringify(imgs)}`);
    await p.waitForFunction(() => [...document.querySelectorAll('.profile-head img')].every((i) => i.complete && i.naturalWidth > 0));
    const sizes = await p.$$eval('.profile-head img', (els) => els.map((i) => [i.naturalWidth, i.naturalHeight]));
    log('picture sizes as served:', JSON.stringify(sizes));
    check(sizes[0][0] <= 1500 && sizes[0][1] <= 500 && sizes[1][0] <= 256 && sizes[1][0] === sizes[1][1], `made small: ${JSON.stringify(sizes)}`);
    const field = (label) => p.locator('label.field-label', { hasText: label }).first().getAttribute('for').then((id) => `[id="${id}"]`);
    await p.fill(await field('Name'), 'Alex <b>Grey</b>');
    await p.fill(await field('Country'), 'Latvia');
    await p.keyboard.press('Escape');
    await p.fill(await field('City'), 'Riga');
    await p.keyboard.press('Escape');
    await p.fill(await field('Time zone'), 'Europe/Riga');
    await p.keyboard.press('Escape');
    await p.fill(await field('About you'), 'Gardener and weekend cyclist; I build small tools for my allotment society.');
    await p.click('.profile-card button.primary');
    await p.waitForSelector('.profile-card >> text=Saved.');
    const saved = (await api(p, 'GET', '/api/me/settings')).body;
    check(saved.description.startsWith('Gardener') && saved.display_name === 'Alex <b>Grey</b>', `saved: ${JSON.stringify(saved)}`);
    check(!(await p.$('.profile-card b')), 'the name stays text');
    await both(p, 'profile');
    // A time zone that does not exist is refused by the server.
    await api(p, 'PUT', '/api/me/settings', { ...saved, timezone: 'Mars/Olympus' }).then((r) => check(r.status === 400, `refused: ${r.status}`));

    // The account menu shows the avatar now.
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');
    await openMenu(p);
    await p.waitForSelector('[role=menu][aria-label=Account] .menu-head img');
    await p.waitForFunction(() => document.querySelector('.menu-head img')?.naturalWidth > 0);
    await both(p, 'menu-avatar');
    await p.keyboard.press('Escape');

    // 3. Personalisation.
    await openMenu(p);
    await p.click('[role=menu][aria-label=Account] [role=menuitem]:has-text("Personalisation")');
    await p.waitForSelector('.personalisation');
    const sel = await p.$$('.personalisation select');
    check(sel.length === 5, `five choices: ${sel.length}`);
    await sel[0].selectOption('concise');
    await sel[3].selectOption('less');
    await sel[4].selectOption('more');
    await p.fill('.personalisation textarea', 'I work in metric units.\nExplain a shell command in one line before you run it.');
    await p.click('.personalisation button.primary');
    await p.waitForSelector('.personalisation >> text=applies from your next message');
    const pers = (await api(p, 'GET', '/api/me/settings')).body.personalisation;
    check(pers.style === 'concise' && pers.emoji === 'less' && pers.headings === 'more' && pers.instructions.startsWith('I work'), `saved: ${JSON.stringify(pers)}`);
    await both(p, 'personalisation');
    await p.setViewportSize({ width: 390, height: 860 });
    await sleep(200);
    const wide = await p.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth);
    check(wide, 'no horizontal scroll at phone width');
    await p.screenshot({ path: `${SHOTS}/personalisation-phone-light.png` });
    await p.setViewportSize({ width: 1280, height: 860 });

    // 4. Workspace settings → Agents, and the agent page's switch.
    await p.goto(`${URL_}#/settings/workspace/agents`);
    await p.waitForSelector('.agents-index');
    const n = await p.$$eval('.agents-index .agent-item', (els) => els.length);
    const listed = (await api(p, 'GET', '/api/agents')).body.agents.length;
    check(n === listed, `the Agents page's list: ${n} vs ${listed}`);
    await both(p, 'workspace-agents');
    await p.click('.agents-index .agent-open >> nth=0');
    await p.waitForSelector('.feature-grid');
    await p.locator('label.feature', { hasText: 'Personalisation' }).scrollIntoViewIfNeeded();
    await p.screenshot({ path: `${SHOTS}/agent-switch-light.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', SHOTS);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${SHOTS}/x-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    log('stopped server pid', srv.pid);
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
