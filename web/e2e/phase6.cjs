// Phase 6 end-to-end: web search tool, attachments, token meter, rolling
// summary, incognito and deleting chats.
// Usage: node web/e2e/phase6.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const BIN = path.resolve(process.argv[2] || 'target/debug/harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e6-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
const URL_ = 'http://127.0.0.1:7796/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };

// A fake SearXNG on loopback (configured by the admin, so allowed).
const searx = http.createServer((req, res) => {
  res.setHeader('content-type', 'application/json');
  res.end(JSON.stringify({ results: [
    { title: 'Riga trams timetable', url: 'https://www.rigassatiksme.lv/en/timetables/', content: 'Tram routes and times' },
  ] }));
});

async function skipOnboarding(page) {
  await page.waitForSelector('text=Connect an AI provider');
  await page.click('text=Skip: use the built-in demo');
  await page.click('text=Skip: keywords only');
  await page.waitForSelector('text=About you');
  await page.click('button:has-text("Continue")');
  await page.waitForSelector('h2:has-text("Search engine")');
  await page.click('button:has-text("Finish")');
}

(async () => {
  await new Promise((r) => searx.listen(0, '127.0.0.1', r));
  const sx = `http://127.0.0.1:${searx.address().port}`;
  fs.writeFileSync(`${S}/data/harness.toml`, [
    '[provider]', 'kind = "mock"', 'mock_delay_ms = 2', '',
    '[chat]', 'summarize_after_tokens = 400', '',
    '[web]', 'search = "searxng"', `searxng_url = "${sx}"`, '',
  ].join('\n'));
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7796'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const p = await b.newPage({ viewport: { width: 1280, height: 860 } });
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await p.fill('input[placeholder="claim code"]', code);
    await p.fill('input[placeholder="username"]', 'alex');
    await p.fill('input[type=password]', 'admin password');
    await p.click('text=Create account');
    await skipOnboarding(p);
    await p.waitForSelector('h1.greeting');
    const ask = async (text) => {
      await p.fill('.composer textarea', text);
      await p.click('.mode-switch button:has-text("Ask")');
      await p.keyboard.press('Enter');
    };

    // 1. Web search runs as a tool in a chat.
    await ask('search: riga tram times');
    await p.waitForSelector('.tool-web >> text=Searched “riga tram times” · 1 result');
    // The mock repeats the result's first line; its label shows as a pill.
    await p.waitForSelector('.msg.assistant:has-text("Tool said:") .cite:has-text("rigassatiksme.lv")');
    await p.waitForSelector('.msg.assistant >> text=Riga trams timetable');
    log('web_search answered through the fake SearXNG');
    // web_fetch never reaches loopback, even with a URL the user typed.
    await p.fill('.composer textarea', 'fetch: http://127.0.0.1:7796/api/me');
    await p.keyboard.press('Enter');
    await p.waitForSelector('text=Tool said: private and local addresses are not fetched');
    log('web_fetch refused a local address');

    // 2. Attachments: the file's text reaches the model.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    fs.writeFileSync(`${S}/notes.md`, '# Plan\nbuy oat milk on friday\n');
    await p.setInputFiles('.composer input[type=file]', `${S}/notes.md`);
    await p.waitForSelector('.composer .chip:has-text("notes.md")');
    await p.screenshot({ path: `${S}/p6-1-attachment.png` });
    await ask('what is in my notes?');
    await p.waitForSelector('.msg.user .chip:has-text("notes.md")');
    await p.waitForSelector('.msg.assistant >> text=buy oat milk on friday');
    log('attachment text reached the model');
    // A binary file is refused with a clear message.
    fs.writeFileSync(`${S}/blob.bin`, Buffer.from([0, 1, 2, 3]));
    await p.setInputFiles('.composer input[type=file]', `${S}/blob.bin`);
    await p.waitForSelector('text=unsupported file');

    // 3. Token meter and rolling summary on a long chat.
    for (let i = 0; i < 4; i++) {
      await p.fill('.composer textarea', `message ${i} ` + 'lorem ipsum dolor sit amet '.repeat(20));
      await p.keyboard.press('Enter');
      await p.waitForSelector(`.msg.assistant >> text=message ${i}`);
      await p.waitForFunction(() => !document.querySelector('.send.stop'));
    }
    await p.waitForSelector('details.summary');
    const meter = await p.innerText('.meter');
    log('meter:', meter, '| summary shown');
    check(/tokens/.test(meter) && /context/.test(meter), 'token meter visible');
    await p.screenshot({ path: `${S}/p6-2-summary.png` });

    // 4. Incognito: not in recents or search; delete now removes it.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'my secret okapi plan');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.click('button.incognito');
    await p.click('.composer .send');
    await p.waitForSelector('.incognito-banner');
    await p.waitForSelector('text=You said: my secret okapi plan');
    await p.screenshot({ path: `${S}/p6-3-incognito.png` });
    const recents = await p.innerText('.sidebar');
    check(!recents.includes('okapi'), 'incognito chat must not be in recents');
    const hits = await p.evaluate(() => fetch('/api/search?q=okapi').then((r) => r.json()));
    check(hits.results.length === 0, 'incognito chat must not be searchable');
    p.once('dialog', (d) => d.accept());
    await p.click('.incognito-banner button');
    await p.waitForSelector('h1.greeting');
    const rows = await p.evaluate(() => fetch('/api/sessions').then((r) => r.json()));
    check(!rows.sessions.some((s) => s.incognito), 'deleted incognito chat is gone');
    log('incognito: hidden, unsearchable, deleted');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    searx.close();
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
