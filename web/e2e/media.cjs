// Media end to end: upload a PNG, an MP3, an MP4 and an SVG with a script;
// the thumbnails this browser makes survive a reload; the audio plays; the
// SVG shows as a picture, downloads as a file and never runs its script;
// another account (bob) sees none of it.
// Usage: node web/e2e/media.cjs <path-to-harness-binary> [work-dir]
// Needs ffmpeg on the PATH to make the test files.
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2em-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(`${S}/files`, { recursive: true });
// Media stays off by default until images in chats land; this run turns it on.
fs.writeFileSync(
  `${S}/data/config.toml`,
  '[media]\nenabled = true\n\n[[providers]]\nid = "demo"\nname = "Demo"\npreset = "demo"\nkind = "mock"\nmock_delay_ms = 5\nmodels = [{ id = "mock" }]\n',
);
const URL_ = 'http://127.0.0.1:7795/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
let PAGE = null;

/** The test files: real encodings, so the browser can decode and play them. */
function makeFiles() {
  const ff = (...args) => execFileSync('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', ...args]);
  ff('-f', 'lavfi', '-i', 'testsrc=size=320x240', '-frames:v', '1', `${S}/files/pattern.png`);
  ff('-f', 'lavfi', '-i', 'sine=frequency=440:duration=2', '-c:a', 'libmp3lame', `${S}/files/tone.mp3`);
  ff('-f', 'lavfi', '-i', 'testsrc=size=320x240:rate=25:duration=2', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', `${S}/files/clip.mp4`);
  fs.writeFileSync(
    `${S}/files/logo.svg`,
    '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><script>window.parent.__pwned = 1; window.__pwned = 1;</script><rect width="64" height="64" fill="teal"/></svg>',
  );
  return ['pattern.png', 'tone.mp3', 'clip.mp4', 'logo.svg'].map((f) => `${S}/files/${f}`);
}

(async () => {
  const files = makeFiles();
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7795'], { detached: true, stdio: 'ignore' });
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

    // Upload all four at once; each joins the grid.
    await p.click('aside.sidebar a:has-text("Media")');
    await p.waitForSelector('h1:has-text("Media")');
    await p.waitForSelector('.media-empty');
    await p.setInputFiles('input[aria-label="Files to upload"]', files);
    await p.waitForFunction(() => document.querySelectorAll('.media-tile').length === 4, null, { timeout: 30000 });
    log('tiles:', await p.$$eval('.media-tile', (ts) => ts.map((t) => t.getAttribute('aria-label')).join(' | ')));

    // Thumbnails are made here, stored on the server, and still there after a reload.
    const thumbs = () => p.$$eval('.media-tile img', (is) => is.map((i) => i.getAttribute('src')));
    await p.waitForFunction(() => document.querySelectorAll('.media-tile img').length >= 2, null, { timeout: 20000 });
    await sleep(500);
    const before = await thumbs();
    await p.reload();
    await p.waitForSelector('.media-tile');
    await p.waitForFunction((n) => document.querySelectorAll('.media-tile img').length >= n, before.length);
    const ok = await p.$$eval('.media-tile img', (is) => Promise.all(is.map((i) => (i.complete ? i.naturalWidth > 0 : new Promise((r) => (i.onload = () => r(true)))))));
    check(ok.every(Boolean), 'every stored thumbnail loads after a reload');
    log('thumbnails after reload:', before.length);
    await p.screenshot({ path: `${S}/m-1-grid.png` });

    // The audio plays.
    await p.click('.media-tile[aria-label^="tone.mp3"]');
    await p.waitForSelector('dialog.media-preview audio');
    const played = await p.$eval('dialog.media-preview audio', async (a) => {
      a.muted = true;
      await a.play();
      await new Promise((r) => setTimeout(r, 400));
      return { time: a.currentTime, duration: a.duration };
    });
    check(played.time > 0 && played.duration > 1.5, `the audio plays (${JSON.stringify(played)})`);
    log('audio played to', played.time.toFixed(2), 's of', played.duration.toFixed(2));
    await p.keyboard.press('Escape');
    await p.waitForSelector('dialog.media-preview', { state: 'detached' });

    // The SVG: shown as a picture (scripts never run in <img>), downloaded as a file.
    await p.click('.media-tile[aria-label^="logo.svg"]');
    await p.waitForSelector('dialog.media-preview .media-stage img');
    await p.waitForFunction(() => document.querySelector('dialog.media-preview .media-stage img').complete);
    const shown = await p.$eval('dialog.media-preview .media-stage img', (i) => i.naturalWidth);
    check(shown === 64, 'the SVG renders as an image');
    check((await p.evaluate(() => window.__pwned)) === undefined, 'window.__pwned stays undefined');
    const [download] = await Promise.all([p.waitForEvent('download'), p.click('dialog.media-preview a:has-text("Download")')]);
    const saved = await download.path();
    check(fs.readFileSync(saved, 'utf8').includes('<rect'), 'the download holds the SVG file');
    log('svg download:', download.suggestedFilename());
    // Its content address, asked for as anything but an image, is an attachment, never a page.
    const svgId = await p.evaluate(async () => (await (await fetch('/api/media?limit=60')).json()).items.find((m) => m.name === 'logo.svg').media_id);
    const direct = await p.evaluate(async (id) => {
      const r = await fetch(`/api/media/${id}/content`);
      return { type: r.headers.get('content-type'), disposition: r.headers.get('content-disposition'), vary: r.headers.get('vary') };
    }, svgId);
    log('direct address:', JSON.stringify(direct));
    check(direct.type === 'application/octet-stream' && /^attachment/.test(direct.disposition || ''), 'the content address downloads the SVG');
    await p.keyboard.press('Escape');
    log('svg: shown as an image, downloaded as a file, script never ran');

    // Bob, another account, sees none of it.
    await p.goto(`${URL_}#/settings/server/users`);
    await p.fill('input[placeholder="username"]', 'bob');
    await p.fill('input[placeholder="temporary password (10+)"]', 'temporary pw');
    await p.click('form >> button:has-text("Create")');
    await p.waitForSelector('td:has-text("bob")');
    const q = await (await b.newContext({ viewport: { width: 1280, height: 860 } })).newPage();
    q.on('pageerror', (e) => errors.push(`bob: ${e.message}`));
    await q.goto(URL_);
    await q.fill('input[placeholder="username"]', 'bob');
    await q.fill('input[type=password]', 'temporary pw');
    await q.click('button:has-text("Log in")');
    await q.waitForSelector('text=Choose your password');
    const pw = q.locator('input[type=password]');
    await pw.nth(0).fill('temporary pw');
    await pw.nth(1).fill('bobs own password');
    await pw.nth(2).fill('bobs own password');
    await q.click('text=Save password');
    await q.waitForSelector('text=About you');
    await q.click('button:text-is("Continue")');
    await q.click('button:text-is("Finish")');
    await q.waitForSelector('h1.greeting');
    await q.click('aside.sidebar a:has-text("Media")');
    await q.waitForSelector('.media-empty');
    check((await q.locator('.media-tile').count()) === 0, 'bob sees no tiles');
    const status = await q.evaluate(async (id) => (await fetch(`/api/media/${id}/content`)).status, svgId);
    check(status === 404, `bob gets 404 for alex's file (got ${status})`);
    log('bob: empty grid, 404 on alex\'s file');
    await q.screenshot({ path: `${S}/m-2-bob.png` });

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } catch (e) {
    if (PAGE) await PAGE.screenshot({ path: `${S}/m-fail.png` }).catch(() => {});
    throw e;
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
