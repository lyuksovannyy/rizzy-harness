// Phase 5 end-to-end: onboarding against a fake OpenAI-compatible provider.
// Usage: node web/e2e/phase5.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const BIN = path.resolve(process.argv[2] || 'target/debug/harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e5-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${S}"]\n`);
const KEY = 'sk-e2e-fake-key-0001';
const URL_ = 'http://127.0.0.1:7800/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };

// Fake provider: /v1/models and streaming /v1/chat/completions, key-checked.
function fakeProvider() {
  return new Promise((resolve) => {
    const srv = http.createServer((req, res) => {
      if (req.headers.authorization !== `Bearer ${KEY}`) {
        res.writeHead(401).end('bad key');
        return;
      }
      if (req.method === 'GET' && req.url === '/v1/models') {
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ data: [{ id: 'fake-small' }, { id: 'fake-large' }] }));
        return;
      }
      if (req.method === 'POST' && req.url === '/v1/chat/completions') {
        let body = '';
        req.on('data', (c) => (body += c));
        req.on('end', () => {
          const model = JSON.parse(body).model;
          res.writeHead(200, { 'content-type': 'text/event-stream' });
          const chunk = (t) => `data: ${JSON.stringify({ choices: [{ index: 0, delta: { content: t } }] })}\n\n`;
          res.end(chunk(`Hello from ${model}`) + 'data: [DONE]\n\n');
        });
        return;
      }
      res.writeHead(404).end();
    });
    srv.listen(0, '127.0.0.1', () => resolve({ srv, base: `http://127.0.0.1:${srv.address().port}/v1` }));
  });
}

(async () => {
  const fake = await fakeProvider();
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7800'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 }, timezoneId: 'Europe/Riga' });
    const p = await ctx.newPage();
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await p.fill('input[placeholder="claim code"]', code);
    await p.fill('input[placeholder="username"]', 'alex');
    await p.fill('input[type=password]', 'admin password');
    await p.click('text=Create account');

    // Step 1: provider.
    await p.waitForSelector('text=Connect an AI provider');
    log('steps:', await p.innerText('.steps'));
    await p.selectOption('select', { label: 'Other OpenAI-compatible' });
    await p.fill('input[placeholder="https://…/v1"]', fake.base);
    await p.fill('input[type=password]', 'sk-wrong-key-999');
    await p.click('text=Test connection');
    await p.waitForSelector('.error-text');
    log('wrong key:', (await p.innerText('.error-text')).slice(0, 60));
    await p.fill('input[type=password]', KEY);
    await p.click('text=Test connection');
    await p.waitForSelector('.model-pick');
    log('models:', (await p.innerText('.model-pick ul')).replace(/\s+/g, ' '));
    await p.click('button[aria-label="make fake-large the default"]');
    await p.screenshot({ path: `${S}/p5-1-provider.png` });
    await p.click('text=Save and continue');
    await p.click('text=Skip: keywords only');

    // Step 3: profile (time zone pre-filled from the browser).
    await p.waitForSelector('text=About you');
    const tzBox = 'input[role=combobox][aria-label="Time zone"]';
    const tz = await p.inputValue(tzBox);
    log('prefilled tz:', tz);
    check(tz === 'Europe/Riga', 'timezone should be prefilled from the browser');
    // Time zone: can be emptied, then picked again from suggestions.
    await p.click('button[aria-label="Clear time zone"]');
    check((await p.inputValue(tzBox)) === '', 'time zone can be empty');
    await p.waitForSelector('text=Empty: the AI is not told your local time.');
    await p.fill(tzBox, 'new york');
    await p.waitForSelector('[role=listbox] [role=option]:has-text("America/New York")');
    await p.keyboard.press('Escape');
    await p.fill(tzBox, 'riga');
    await p.keyboard.press('Enter');
    check((await p.inputValue(tzBox)) === 'Europe/Riga', 'Enter picks the first suggestion');
    await p.fill('input[placeholder="alex"]', 'Alex');
    // Country: suggestions from the browser's country names.
    const country = 'input[role=combobox][aria-label="Country"]';
    await p.fill(country, 'latv');
    await p.click('[role=option]:has-text("Latvia")');
    check((await p.inputValue(country)) === 'Latvia', 'country picked');
    // City: suggestions limited to the chosen country (data loaded on focus).
    const city = 'input[role=combobox][aria-label="City"]';
    await p.click(city);
    await p.fill(city, 'ri');
    await p.waitForSelector('[role=option]:has-text("Riga")');
    const opts = await p.$$eval('[role=option]', (o) => o.map((x) => x.textContent));
    log('city suggestions for "ri" in Latvia:', opts.join(', '));
    check(!opts.some((o) => /Rio de Janeiro/.test(o)), 'cities outside Latvia are not offered');
    await p.screenshot({ path: `${S}/p5-2-profile.png` });
    await p.click('[role=option]:has-text("Riga")');
    check((await p.inputValue(city)) === 'Riga', 'city picked');
    await p.click('button:has-text("Continue")');

    // Step 3: search engine.
    await p.waitForSelector('h2:has-text("Search engine")');
    await p.check('label:has-text("Google") input');
    await p.click('button:has-text("Finish")');
    await p.waitForSelector('h1.greeting');
    const g = await p.innerText('h1.greeting');
    log('greeting:', g);
    check(g.includes('Alex'), 'greeting uses the display name');

    // Chat goes to the fake provider with the chosen default model.
    await p.fill('.composer textarea', 'hello there');
    await p.keyboard.press('Enter');
    await p.waitForSelector('text=Hello from fake-large');
    log('reply from fake provider with default model');

    // The key landed in secrets.toml, never in harness.toml.
    const toml = fs.readFileSync(`${S}/data/harness.toml`, 'utf8');
    check(!toml.includes(KEY) && toml.includes('fake-large'), 'harness.toml has config but no key');
    check(fs.readFileSync(`${S}/data/secrets.toml`, 'utf8').includes(KEY), 'secrets.toml has the key');
    check((fs.statSync(`${S}/data/secrets.toml`).mode & 0o777) === 0o600, 'secrets.toml is 0600');
    log('files ok: key only in secrets.toml (0600)');

    // Search mode uses the engine picked in onboarding.
    await p.route('https://www.google.com/**', (r) => r.fulfill({ body: 'ok', contentType: 'text/plain' }));
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'youtube');
    await sleep(100);
    await p.keyboard.press('Enter');
    await p.waitForURL(/google\.com/);
    log('search ->', p.url());
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');

    // Settings: accent applies; provider tab is there for admins.
    await p.click('.nav-item.user');
    await p.click('.tabs >> text=Preferences');
    await p.click('button[aria-label="green"]');
    await p.click('button:has-text("Save")');
    await p.waitForSelector('text=Saved.');
    log('accent:', await p.evaluate(() => document.documentElement.dataset.accent));
    check((await p.evaluate(() => document.documentElement.dataset.accent)) === 'green', 'accent applied');
    await p.click('.tabs >> text=AI provider');
    await p.waitForSelector('text=a key is stored');
    await p.screenshot({ path: `${S}/p5-3-settings-provider.png` });

    // A member gets only the profile and search steps.
    await p.click('.tabs >> text=Users');
    await p.fill('input[placeholder="username"]', 'bob');
    await p.fill('input[placeholder="temporary password (10+)"]', 'temporary pw');
    await p.click('form >> button:has-text("Create")');
    await p.waitForSelector('td:has-text("bob")');
    const q = await (await b.newContext({ viewport: { width: 1280, height: 860 } })).newPage();
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
    log('member steps:', await q.innerText('.steps'));
    check((await q.innerText('.steps')).toLowerCase().includes('of 2'), 'members skip the provider step');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    fake.srv.close();
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
