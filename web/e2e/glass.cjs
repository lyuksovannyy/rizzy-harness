// Rizzy Glass, by eye (docs/rizzy-glass.md): screenshots of a chat with
// messages, a finished tool card and a pending approval card, an open menu,
// the search dialog and Settings with the accent picker, in light and dark,
// with the Blue and Orange accents, at desktop width and at 390px. It also
// checks what can be checked without eyes: the sidebar and menus blur, the
// transcript and its cards never do, and the chosen accent and theme are
// what the page shows.
// Usage: node web/e2e/glass.cjs <path-to-harness-binary> [out-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly, keyed } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2eglass-'));
const OUT = path.resolve(process.argv[3] || S);
fs.mkdirSync(OUT, { recursive: true });
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/ws`, { recursive: true });
fs.writeFileSync(`${S}/ws/main.rs`, 'fn main() {}\n');
fs.writeFileSync(`${S}/data/config.toml`, '[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n');
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}/ws"]\n`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };

/** The port the kernel gave the server, from its log. */
async function boundPort(logFile, ms = 60000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    const m = /listening on 127\.0\.0\.1:(\d+)/.exec(fs.existsSync(logFile) ? fs.readFileSync(logFile, 'utf8') : '');
    if (m) return Number(m[1]);
    await sleep(100);
  }
  const tail = fs.existsSync(logFile) ? fs.readFileSync(logFile, 'utf8').split('\n').slice(-5).join(' | ') : 'no log';
  throw new Error(`the server did not say where it listens: ${tail}`);
}

let PAGE = null;
(async () => {
  const logFile = `${S}/serve.log`;
  const out = fs.openSync(logFile, 'w');
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:0'], { detached: true, stdio: ['ignore', out, out] });
  try {
    const URL_ = `http://127.0.0.1:${await boundPort(logFile)}/`;
    const code = await claimCode(`${S}/data/claim-code`, URL_);
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
    await p.screenshot({ path: `${OUT}/home-default.png` });

    // A plain chat for the sidebar, then an agentic one: a message, a call
    // that was allowed and ran, and a call that waits for approval.
    const api = (method, url, body) => p.evaluate(async ([m, u, b]) => {
      const r = await fetch(u, { method: m, headers: { 'content-type': 'application/json' }, body: b === undefined ? undefined : JSON.stringify(b) });
      return { status: r.status, body: await r.json().catch(() => null) };
    }, [method, url, body]);
    const chat = async (body, prompts) => {
      const { body: s } = await api('POST', '/api/sessions', body);
      for (const text of prompts) check((await api('POST', `/api/sessions/${s.session_id}/prompt`, { text })).status === 200, 'prompt taken');
      return s.session_id;
    };
    await p.fill('.composer textarea', 'Plans for the weekend: hello there');
    await p.keyboard.press('Enter');
    await p.waitForSelector('text=You said: Plans for the weekend');
    const sid = await chat({ profile: 'agentic', cwd: `${S}/ws`, title: 'Tidy the build', agent_id: 'coder' }, []);
    await p.goto(`${URL_}#/s/${sid}`);
    await p.waitForSelector('.conversation');
    await p.fill('.composer textarea', 'run: ls');
    await p.keyboard.press('Enter');
    await p.click('.approval button:has-text("Allow")');
    await p.waitForSelector('text=Tool said: main.rs');
    await p.waitForFunction(() => !document.querySelector('.send.stop'));
    await p.fill('.composer textarea', 'run: rm -rf target && cargo build --release');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.approval');
    log('chat with a finished call and a pending approval');

    // Blur only where the design system puts it (theme.test.ts checks the
    // stylesheet; this checks what the browser computed).
    const blur = await p.evaluate(() => {
      const f = (el) => (el ? getComputedStyle(el).backdropFilter : 'missing');
      return {
        sidebar: f(document.querySelector('.sidebar')),
        transcript: [...document.querySelectorAll('.conversation, .conversation *')].filter((e) => getComputedStyle(e).backdropFilter !== 'none').map((e) => e.className),
      };
    });
    check(blur.sidebar.includes('blur'), `the sidebar blurs (${blur.sidebar})`);
    check(blur.transcript.length === 0, `nothing in the transcript blurs: ${blur.transcript.join(', ')}`);

    const settings = (await api('GET', '/api/me/settings')).body;
    const shots = [];
    for (const [w, h, size] of [[1280, 860, 'desktop'], [390, 844, 'phone']]) {
      await p.setViewportSize({ width: w, height: h });
      for (const theme of ['light', 'dark']) {
        for (const accent of ['blue', 'orange']) {
          const tag = `${size}-${theme}-${accent}`;
          check((await api('PUT', '/api/me/settings', { ...settings, theme, accent: accent === 'blue' ? null : accent })).status === 200, 'settings saved');
          await p.goto(`${URL_}#/s/${sid}`);
          await p.reload();
          await p.waitForSelector('.approval');
          const html = await p.evaluate(() => [document.documentElement.dataset.theme, document.documentElement.dataset.accent ?? 'blue']);
          check(html[0] === theme && html[1] === accent, `the page shows ${theme}/${accent} (${html})`);
          await sleep(300);
          await p.screenshot({ path: `${OUT}/${tag}-chat.png` });
          await p.locator('.approval').scrollIntoViewIfNeeded();
          await p.screenshot({ path: `${OUT}/${tag}-approval.png` });
          shots.push(`${tag}-chat`, `${tag}-approval`);

          // The account menu (in the drawer on a phone).
          if (size === 'phone') {
            await p.click('.topbar button.icon');
            await sleep(400);
            await p.screenshot({ path: `${OUT}/${tag}-drawer.png` });
            shots.push(`${tag}-drawer`);
          }
          await p.click('.sidebar-foot .nav-item.account');
          await p.waitForSelector('.account-menu');
          await sleep(350);
          const menuBlur = await p.evaluate(() => getComputedStyle(document.querySelector('.account-menu')).backdropFilter);
          check(menuBlur.includes('blur'), `the menu blurs (${menuBlur})`);
          await p.screenshot({ path: `${OUT}/${tag}-menu.png` });
          shots.push(`${tag}-menu`);
          await p.keyboard.press('Escape');
          await p.goto(`${URL_}#/s/${sid}`);
          await p.waitForSelector('.approval');

          // The search dialog.
          await p.keyboard.press('Control+k');
          await p.waitForSelector('.search-panel[open]');
          await p.keyboard.type('weekend');
          await sleep(600);
          await p.screenshot({ path: `${OUT}/${tag}-dialog.png` });
          shots.push(`${tag}-dialog`);
          await p.keyboard.press('Escape');

          // Settings with the accent picker.
          await p.goto(`${URL_}#/settings/account/preferences`);
          await p.waitForSelector('.swatches');
          check((await p.locator('.swatches[aria-label="accent"] .swatch').count()) === 8, 'eight accents');
          await p.locator('.swatches[aria-label="accent"]').scrollIntoViewIfNeeded();
          await sleep(200);
          await p.screenshot({ path: `${OUT}/${tag}-settings.png` });
          shots.push(`${tag}-settings`);
        }
      }
    }
    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log(`${shots.length + 1} screenshots in`, OUT);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${OUT}/glass-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    if (OUT !== S) fs.rmSync(S, { recursive: true, force: true });
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n').slice(0, 3).join(' | ')); process.exit(1); });
