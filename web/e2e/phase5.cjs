// Phase 5 end-to-end: onboarding against a fake OpenAI-compatible provider.
// Usage: node web/e2e/phase5.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { claimCode, createOwner, keywordsOnly } = require('./onboard.cjs');
const BIN = path.resolve(process.argv[2] || 'target/debug/rizzy-harness');
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
    const code = await claimCode(`${S}/data/claim-code`, URL_);
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 860 }, timezoneId: 'Europe/Riga' });
    const p = await ctx.newPage();
    const errors = [];
    p.on('pageerror', (e) => errors.push(e.message));
    await p.goto(URL_);
    await createOwner(p, code);

    // Step 1: provider.
    await p.waitForSelector('text=Connect an AI provider');
    log('steps:', await p.innerText('.steps'));
    await p.selectOption('select', { label: 'OpenAI-compatible (custom)' });
    await p.fill('input[placeholder="https://…/v1"]', fake.base);
    // A custom endpoint is named and prefixed by the admin.
    await p.fill('input[placeholder="My endpoint"]', 'Fake');
    await p.fill('input[placeholder="my-endpoint"]', 'fake');
    await p.fill('input[type=password]', 'sk-wrong-key-999');
    await p.click('button[aria-label="Test connection"]');
    await p.waitForSelector('.error-text');
    log('wrong key:', (await p.innerText('.error-text')).slice(0, 60));
    await p.fill('input[type=password]', KEY);
    await p.click('button[aria-label="Test connection"]');
    await p.waitForSelector('.model-pick');
    log('models:', (await p.innerText('.model-pick ul')).replace(/\s+/g, ' '));
    // Offer both models; fake-large is General's main model, so everyone's default.
    await p.check('.model-pick li:has-text("fake-small") input[type=checkbox]');
    await p.check('.model-pick li:has-text("fake-large") input[type=checkbox]');
    await p.click('button.agent-pick[aria-label$="(fake-large)"]');
    await p.click('[role=menu] button[aria-label="General: main model"]');
    await p.keyboard.press('Escape');
    await p.waitForSelector('button.agent-pick[aria-label="For: General (fake-large)"]');
    // Every offered model needs an agent: fake-small is General's fallback.
    await p.click('button.agent-pick[aria-label$="(fake-small)"]');
    await p.click('[role=menu] button[aria-label="General: fallback"]');
    await p.keyboard.press('Escape');
    await p.screenshot({ path: `${S}/p5-1-provider.png` });
    await p.click('button.primary:has-text("Save and continue") >> visible=true');
    await keywordsOnly(p);

    // Step 3: profile (time zone pre-filled from the browser).
    await p.waitForSelector('text=About you');
    // The profile fields, by their ids (each is named by its label, "Time zone (optional)").
    const tzBox = 'input[role=combobox][id$="-tz"]';
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
    const country = 'input[role=combobox][id$="-country"]';
    await p.fill(country, 'latv');
    await p.click('[role=option]:has-text("Latvia")');
    check((await p.inputValue(country)) === 'Latvia', 'country picked');
    // City: suggestions limited to the chosen country (data loaded on focus).
    const city = 'input[role=combobox][id$="-city"]';
    await p.click(city);
    await p.fill(city, 'ri');
    await p.waitForSelector('[role=option]:has-text("Riga")');
    const opts = await p.$$eval('[role=option]', (o) => o.map((x) => x.textContent));
    log('city suggestions for "ri" in Latvia:', opts.join(', '));
    check(!opts.some((o) => /Rio de Janeiro/.test(o)), 'cities outside Latvia are not offered');
    await p.screenshot({ path: `${S}/p5-2-profile.png` });
    await p.click('[role=option]:has-text("Riga")');
    check((await p.inputValue(city)) === 'Riga', 'city picked');
    await p.click('button:text-is("Continue")');

    // Step 3: search engine.
    await p.waitForSelector('h2:has-text("Search engine")');
    const pickEngine = async (label, name) => {
      await p.click(`button[aria-label^="${label}:"]`);
      await p.click(`[role=listbox][aria-label="${label}"] [role=option]:has-text("${name}")`);
    };
    // The selector lists every engine able to do the kind, with its abilities.
    await p.click('button[aria-label^="Web search engine:"]');
    const webOpts = await p.$$eval('[role=listbox][aria-label="Web search engine"] [role=option]', (o) => o.map((x) => x.textContent));
    log('web engines:', webOpts.join(' | '));
    check(webOpts.some((o) => o === 'Googletext & image') && webOpts.some((o) => o === 'Wikipediatext'), 'abilities shown');
    check(!webOpts.some((o) => o.startsWith('Unsplash')), 'image-only engines are not web engines');
    await p.keyboard.press('Escape');
    await pickEngine('Web search engine', 'Google');
    await pickEngine('Image search engine', 'Yandex');
    // A custom engine with text search only.
    await p.click('button:has-text("Add a search engine")');
    await p.fill('input[aria-label="Engine name"]', 'Docs');
    await p.fill('input[aria-label="Text search URL"]', 'https://docs.example/?q={q}');
    await p.screenshot({ path: `${S}/p5-3-engines.png` });
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

    // The key landed in secrets.toml, never in config.toml.
    const toml = fs.readFileSync(`${S}/data/config.toml`, 'utf8');
    check(!toml.includes(KEY) && toml.includes('fake-large'), 'config.toml has config but no key');
    check(fs.readFileSync(`${S}/data/secrets.toml`, 'utf8').includes(KEY), 'secrets.toml has the key');
    check((fs.statSync(`${S}/data/secrets.toml`).mode & 0o777) === 0o600, 'secrets.toml is 0600');
    log('files ok: key only in secrets.toml (0600)');

    // Search mode uses the engines picked in onboarding; Images, @mention and
    // the selector override them.
    for (const host of ['www.google.com', 'yandex.com', 'duckduckgo.com', 'www.bing.com', 'docs.example']) {
      await p.route(`https://${host}/**`, (r) => r.fulfill({ body: 'ok', contentType: 'text/plain' }));
    }
    const search = async (setup, text, expect) => {
      await p.goto(URL_);
      await p.waitForSelector('h1.greeting');
      await p.click('.mode-switch button:has-text("Search")');
      if (setup) await setup();
      await p.fill('.composer textarea', text);
      await sleep(100);
      await p.click('.mode-switch button:has-text("Search")');
      await p.focus('.composer textarea');
      await p.keyboard.press('End');
      await p.keyboard.press('Enter');
      await p.waitForURL(expect);
      log('search ->', p.url());
    };
    await search(null, 'youtube', /google\.com\/search\?q=youtube/);
    await search(async () => p.click('[aria-label="search kind"] button:has-text("Images")'), 'red panda', /yandex\.com\/images\/search\?text=red%20panda/);
    await search(null, '@duckduckgo cats', /duckduckgo\.com\/\?q=cats$/);
    await search(async () => {
      await p.click('button[aria-label^="Search engine:"]');
      await p.click('[role=listbox][aria-label="Search engine"] [role=option]:has-text("Bing")');
    }, 'rust', /bing\.com\/search\?q=rust/);
    await search(null, '@docs tokio', /docs\.example\/\?q=tokio/);
    await p.goto(URL_);
    await p.waitForSelector('h1.greeting');

    // Settings (the account menu opens them on Preferences): accent applies;
    // provider tab is there for admins.
    await p.click('.sidebar-foot .nav-item.account');
    await p.getByRole('menuitem', { name: 'Settings', exact: true }).click();
    await p.click('.tabs >> text=Preferences');
    await p.click('button[aria-label="green"]');
    await p.click('button:has-text("Save")');
    await p.waitForSelector('text=Saved.');
    log('accent:', await p.evaluate(() => document.documentElement.dataset.accent));
    check((await p.evaluate(() => document.documentElement.dataset.accent)) === 'green', 'accent applied');

    // Theme: a pick shows at once, leaving the tab unsaved goes back, Save
    // keeps it on <html> and in the mirror public/theme.js reads.
    const theme = () => p.evaluate(() => document.documentElement.dataset.theme ?? 'system');
    // A theme change runs inside a view transition, a frame later.
    const themeIs = (want) => p.waitForFunction((w) => (document.documentElement.dataset.theme ?? 'system') === w, want, { timeout: 2000 }).then(() => true, () => false);
    const bodyBg = (page) => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    const themeButton = (label) => `[role=radiogroup][aria-label=theme] button:has-text("${label}")`;
    await p.emulateMedia({ colorScheme: 'light' });
    await p.click(themeButton('Dark'));
    check(await themeIs('dark'), 'the theme previews');
    await p.click('.tabs >> text=Profile');
    check(await themeIs('system'), 'an unsaved theme is undone on leaving');
    await p.click('.tabs >> text=Preferences');
    await p.click(themeButton('Dark'));
    await p.click('button:has-text("Save")');
    await p.waitForSelector('text=Saved.');
    check((await theme()) === 'dark', 'the theme is saved');
    check((await p.evaluate(() => localStorage.getItem('harness.theme'))) === 'dark', 'the theme is mirrored');
    const darkBg = await bodyBg(p);
    log('dark page:', darkBg);
    // No flash: with the settings request held back, a new page in a light
    // OS is dark from the start, from the mirror alone.
    const early = await ctx.newPage();
    await early.emulateMedia({ colorScheme: 'light' });
    await early.route('**/api/me/settings', () => {});
    await early.goto(URL_, { waitUntil: 'domcontentloaded' });
    check((await bodyBg(early)) === darkBg, 'the saved theme applies before the settings load: ' + (await bodyBg(early)));
    await early.close();
    // Back to System: the OS decides again.
    await p.click(themeButton('System'));
    await p.click('button:has-text("Save")');
    await p.waitForSelector('text=Saved.');
    check((await theme()) === 'system' && (await p.evaluate(() => localStorage.getItem('harness.theme'))) === null, 'system clears the theme');
    const lightBg = await bodyBg(p);
    await p.emulateMedia({ colorScheme: 'dark' });
    check((await bodyBg(p)) === darkBg && lightBg !== darkBg, 'the OS theme drives the page again');
    log('theme: preview, revert, saved, no flash, system');
    await p.click('.settings-sections >> text=Server');
    await p.click('.tabs >> text=AI providers');
    await p.waitForSelector('.providers li:has-text("fake::")');
    await p.click('button[aria-label="Edit Fake"]');
    await p.waitForSelector('text=a key is stored');
    await p.screenshot({ path: `${S}/p5-3-settings-provider.png` });

    // A member gets only the profile and search steps.
    await p.click('.tabs >> text=Users');
    await p.fill('input[placeholder="username"]', 'bob');
    await p.click('form >> button:has-text("Create")');
    // The server makes the temporary password; the dialog shows it once.
    const temp = (await p.innerText('dialog .temp-password')).trim();
    await p.click('dialog button:has-text("Done")');
    await p.waitForSelector('td:has-text("bob")');
    const q = await (await b.newContext({ viewport: { width: 1280, height: 860 } })).newPage();
    await q.goto(URL_);
    await q.fill('input[placeholder="username"]', 'bob');
    await q.fill('input[type=password]', temp);
    await q.click('button:has-text("Log in")');
    await q.waitForSelector('text=Choose your password');
    const pw = q.locator('input[type=password]');
    await pw.nth(0).fill(temp);
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
