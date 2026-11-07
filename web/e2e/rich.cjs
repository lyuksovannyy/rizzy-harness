// Rich messages end-to-end: the Markdown baseline (links, image chips,
// callouts, tables, code, math, hidden characters, Copy), lazy loading of
// the highlighter worker and math chunks, and a hostile code block that
// must not freeze the page.
// Usage: node web/e2e/rich.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const BIN = path.resolve(process.argv[2] || 'target/debug/harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-rich-'));
fs.mkdirSync(`${S}/data`, { recursive: true });
const URL_ = 'http://127.0.0.1:7797/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };
const HIDDEN = /[\u{061C}\u{200B}-\u{200F}\u{202A}-\u{202E}\u{2060}-\u{2064}\u{2066}-\u{2069}\u{FEFF}\u{E000}-\u{F8FF}]/u;

/** The characters of an element's first text node, in on-screen order. */
const visualText = (loc) => loc.evaluate((el) => {
  const t = el.firstChild;
  const xs = [];
  for (let i = 0; i < t.length; i++) {
    const r = document.createRange();
    r.setStart(t, i);
    r.setEnd(t, i + 1);
    xs.push([t.data[i], r.getBoundingClientRect().x]);
  }
  return xs.sort((a, b) => a[1] - b[1]).map((x) => x[0]).join('');
});

// A fake SearXNG on loopback (configured by the admin, so allowed).
const searx = http.createServer((req, res) => {
  res.setHeader('content-type', 'application/json');
  res.end(JSON.stringify({ results: [
    { title: 'Rust language', url: 'https://a.example/rust', content: 'A language empowering everyone' },
    { title: 'The Rust book', url: 'https://b.example/book', content: 'Learn Rust' },
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
  fs.writeFileSync(`${S}/data/harness.toml`, [
    '[provider]', 'kind = "mock"', 'mock_delay_ms = 20', '',
    '[web]', 'search = "searxng"', `searxng_url = "http://127.0.0.1:${searx.address().port}"`, '',
  ].join('\n'));
  const srv = spawn(BIN, ['--data-dir', `${S}/data`, 'serve', '--bind', '127.0.0.1:7797'], { detached: true, stdio: 'ignore' });
  try {
    await sleep(1500);
    const code = fs.readFileSync(`${S}/data/claim-code`, 'utf8').trim();
    const b = await chromium.launch();
    const ctx = await b.newContext({ viewport: { width: 1280, height: 900 }, permissions: ['clipboard-read', 'clipboard-write'] });
    const p = await ctx.newPage();
    const errors = [];
    const requests = [];
    p.on('pageerror', (e) => errors.push(e.message));
    ctx.on('request', (r) => requests.push(r.url()));
    const chunks = (name) => requests.filter((u) => new RegExp(`/assets/${name}-[^/]*\\.js$`).test(u));
    await p.goto(URL_);
    await p.fill('input[placeholder="claim code"]', code);
    await p.fill('input[placeholder="username"]', 'alex');
    await p.fill('input[type=password]', 'admin password');
    await p.click('text=Create account');
    await skipOnboarding(p);
    await p.waitForSelector('h1.greeting');
    const idle = () => p.waitForFunction(() => !document.querySelector('.send.stop'));

    // 2a. A plain reply loads none of the lazy rendering chunks.
    await p.fill('.composer textarea', 'hello there');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.msg.assistant >> text=You said: hello there');
    await idle();
    for (const name of ['math', 'highlight\\.worker', 'UiBlock']) check(chunks(name).length === 0, `${name} chunk loaded for an echo`);
    log('echo turn: no math, highlight or ui chunks');

    // 1. The demo: while streaming, code and math are plain source.
    await p.fill('.composer textarea', 'rich: demo');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.msg.assistant.streaming .md table');
    check(chunks('highlight\\.worker').length === 0 && chunks('math').length === 0, 'highlight/math must wait for the final message');
    check((await p.locator('.md .token').count()) === 0, 'no highlighting while streaming');
    await idle();
    const demo = p.locator('.msg.assistant').last();
    await demo.locator('.md .token.keyword').first().waitFor();
    await demo.locator('.md math').first().waitFor();
    check((await demo.locator('.md math').count()) >= 2, 'inline and display math render');
    check((await demo.locator('.md .math-src:visible').count()) === 0, 'math source hidden once rendered');
    check((await demo.locator('.md .math-out [href]').count()) === 0, 'no links inside math (\\eqref makes one)');
    check((await demo.locator('.md .math-out [id]').count()) === 0, 'no ids inside math (\\label makes one)');
    check((await demo.locator('[id="user-content-fn-1"]').count()) === 1, 'the footnote keeps its id');
    check(!HIDDEN.test(await demo.locator('.md .math-out').allInnerTexts().then((t) => t.join(''))), 'no hidden characters from \\char');
    check((await demo.locator('.callout-warning').count()) === 1, 'warning callout');
    check((await demo.locator('.callout-warning .callout-title').innerText()) === 'Check before you run', 'callout title');
    check((await demo.locator('.md img').count()) === 0, 'no <img> in messages');
    check((await demo.locator('.img-chip').innerText()).includes('img.example'), 'image chip names its host');
    const text = await demo.locator('.md').innerText();
    check(text.includes('Prices stay text: $5 and $10.'), 'single dollars stay text');
    check(!HIDDEN.test(text) && text.includes('abcdef'), 'hidden characters stripped, also entity-encoded ones');
    // [&#x202E;moc.knab](https://evil.example/): the host and the text after it read as written.
    const reversed = demo.locator('p:has-text("Reversed:")');
    check((await visualText(reversed.locator('.link-host'))) === 'evil.example', 'host label reads evil.example on screen');
    check((await reversed.innerText()).includes('moc.knab') && (await reversed.innerText()).includes('and more.'), 'text after the link is not reordered');
    check((await demo.locator('.codeblock .lang').innerText()) === 'rust', 'code header names the language');
    log('demo: highlighted, math rendered, callout, image chip, prices as text');
    check(chunks('highlight\\.worker').length === 1 && chunks('rust').length === 1, 'highlighter worker and the rust grammar loaded once');
    check(chunks('math').length === 1, 'math chunk loaded');
    log('chunks after the demo:', ['highlight\\.worker', 'rust', 'math'].map((n) => chunks(n).map((u) => u.split('/').pop())).flat().join(', '));

    // A link with a query the model made up asks first, showing the full URL.
    await demo.locator('a[href="https://evil.example/a?q=1"]').click();
    const confirm = demo.locator('.link-confirm');
    await confirm.waitFor();
    check((await confirm.locator('.link-confirm-url').innerText()) === 'https://evil.example/a?q=1', 'confirm shows the full URL');
    check((await demo.locator('.link-host').first().innerText()) === 'evil.example', 'real host shown next to the link');
    await p.screenshot({ path: `${S}/rich-1-confirm.png` });
    await confirm.locator('button:has-text("Cancel")').click();
    check((await demo.locator('.link-confirm').count()) === 0, 'cancel closes the confirm');
    // The image chip follows the same rule.
    await demo.locator('a.img-chip').click();
    await demo.locator('.link-confirm').waitFor();
    await p.keyboard.press('Escape');
    check(ctx.pages().length === 1, 'nothing opened');

    // A confirm opened inside a scrolling table is not clipped by it.
    await demo.locator('a[href="https://evil.example/t?q=2"]').click();
    const inTable = demo.locator('.link-confirm');
    await inTable.waitFor();
    const clip = await inTable.evaluate((box) => {
      const hits = (sel) => {
        const r = box.querySelector(sel).getBoundingClientRect();
        const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
        return !!hit && box.contains(hit);
      };
      return { note: hits('.link-confirm-note'), url: hits('.link-confirm-url'), open: hits('button.primary') };
    });
    check(clip.note && clip.url && clip.open, 'confirm in a table fully visible: ' + JSON.stringify(clip));
    await p.screenshot({ path: `${S}/rich-1b-table-confirm.png` });
    await p.keyboard.press('Escape');

    // Tables sort numerically by a header click.
    const release = demo.locator('th:has-text("release (s)") button');
    await release.click();
    await release.click();
    check((await demo.locator('th:has-text("release (s)")').getAttribute('aria-sort')) === 'descending', 'aria-sort');
    check((await demo.locator('tbody tr').first().innerText()).startsWith('master'), 'sorted by release time, descending');

    // Copy gives the Markdown as rendered: no hidden character, math normalised.
    await demo.hover();
    await demo.locator('.msg-actions button').click();
    await p.waitForSelector('.msg-actions button:has-text("Copied")');
    const copied = await p.evaluate(() => navigator.clipboard.readText());
    check(copied.includes('```rust') && copied.includes('$$a^2+b^2=c^2$$') && copied.includes('abcdef') && !HIDDEN.test(copied),
      'copied Markdown: ' + JSON.stringify(copied.slice(-120)));
    await p.screenshot({ path: `${S}/rich-2-demo.png`, fullPage: true });
    await p.emulateMedia({ colorScheme: 'dark' });
    await p.screenshot({ path: `${S}/rich-3-dark.png`, fullPage: true });
    await p.emulateMedia({ colorScheme: 'light' });

    const leaked = requests.filter((u) => /img\.example|evil\.example/.test(u));
    check(leaked.length === 0, 'requests to model-chosen hosts: ' + leaked.join(', '));
    log('link confirm, table sort, copy; no requests to img.example or evil.example');

    // A hostile code block (Prism's grammars are super-linear on it) must
    // not freeze the page: it highlights in a worker that is stopped after
    // its budget, and the next block still highlights.
    const pat = "a'b\"c`d/e*f<g>h{i}j(k)l[m]n\\o";
    const line = pat.repeat(4).slice(0, 100);
    const hostile = Array.from({ length: 180 }, () => line).join('\n'); // under the 20,000-char cap
    await p.fill('.composer textarea', 'x\n\n```tsx\n' + hostile + '\n```\n\n```rust\nfn after() {}\n```\n');
    await p.keyboard.press('Enter');
    await p.waitForFunction(() => document.querySelectorAll('.msg.assistant').length >= 3);
    await idle();
    const last = p.locator('.msg.assistant').last();
    let maxGap = 0;
    let prev = Date.now();
    const t0 = Date.now();
    while (Date.now() - t0 < 8000) {
      await p.evaluate(() => 1);
      const now = Date.now();
      maxGap = Math.max(maxGap, now - prev);
      prev = now;
      if ((await last.locator('code.language-rust .token').count()) > 0) break;
    }
    check(maxGap < 500, `page stalled ${maxGap} ms`);
    check((await last.locator('code.language-rust .token.keyword').count()) > 0, 'the block after the hostile one is highlighted');
    const waited = Date.now() - t0;
    log(`hostile code block: page answered within ${maxGap} ms; next block highlighted after ${waited} ms; tsx tokens: ${await last.locator('code.language-tsx .token').count()}`);

    // After a reload the same message renders the same way.
    await p.reload();
    await p.locator('.msg.assistant .md .token.keyword').first().waitFor();
    await p.locator('.msg.assistant .md math').first().waitFor();
    log('reload renders the same');

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    searx.close();
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
