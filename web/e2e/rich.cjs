// Rich messages end-to-end: the Markdown baseline (links, image chips,
// callouts, tables, code, math, hidden characters, Copy), lazy loading of
// the highlighter worker and math chunks, a hostile code block that must
// not freeze the page, and citations of web results (pills, footer, the
// web status row), also those a delegated sub-task hands back, and ui
// blocks (skeleton while streaming, the card, rejected and unknown blocks).
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

// A fake SearXNG on loopback (configured by the admin, so allowed). A
// query with "slow" in it answers after a while, so the running row shows;
// one with "other" finds a new page and one already known.
const searx = http.createServer((req, res) => {
  const q = new URL(req.url, 'http://x').searchParams.get('q') || '';
  res.setHeader('content-type', 'application/json');
  const first = q.includes('other')
    ? { title: 'Other page', url: 'https://c.example/other', content: 'Something else' }
    : { title: 'Rust language', url: 'https://a.example/rust', content: 'A language empowering everyone' };
  setTimeout(() => res.end(JSON.stringify({ results: [
    first,
    { title: 'The Rust book', url: 'https://b.example/book', content: 'Learn Rust' },
  ] })), q.includes('slow') ? 2500 : 0);
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

    // ui blocks: while the fence streams it is a typed skeleton (never the
    // JSON); once the message is final, the card Rust validated, framed by
    // the harness ("from <agent>"), from its own lazy chunk.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'ui: choices');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.msg.assistant.streaming .ui-skeleton:has-text("Preparing question…")');
    check(!(await p.locator('.msg.assistant .md').innerText()).includes('"options"'), 'no raw JSON while streaming');
    check(chunks('UiBlock').length === 0, 'the card chunk waits for the final message');
    await idle();
    const choices = p.locator('.msg.assistant').last();
    await choices.locator('.ui-card .ui-choices').waitFor();
    check((await choices.locator('.ui-skeleton').count()) === 0, 'the skeleton gives way to the card');
    check((await choices.locator('.ui-from').innerText()) === 'from General', 'harness frame names the agent');
    check((await choices.locator('.ui-title').innerText()) === 'Which colour do you like best?', 'the question');
    const labels = await choices.locator('.ui-option-label').allInnerTexts();
    check(JSON.stringify(labels) === '["Blue","Green","Red"]', 'options: ' + JSON.stringify(labels));
    check((await choices.locator('.codeblock').count()) === 0, 'no code for a shown block');
    check(chunks('UiBlock').length === 1, 'card chunk loaded once');
    log('ui: skeleton while streaming, then the card', labels.join('/'));

    // 6. Errors: a block Rust rejected is its code plus the reason, an
    // unknown type is plain code, and a second question in one message is
    // rejected while the first shows.
    const send = async (text) => {
      const before = await p.locator('.msg.assistant').count();
      await p.fill('.composer textarea', text);
      await p.keyboard.press('Enter');
      await p.waitForFunction((n) => document.querySelectorAll('.msg.assistant').length > n, before);
      await idle();
      return p.locator('.msg.assistant').last();
    };
    const bad = await send('ui: bad');
    await bad.locator('.ui-error').waitFor();
    check((await bad.locator('.codeblock .lang').innerText()) === 'ui:choices', 'rejected block shows as code');
    check((await bad.locator('.ui-error').innerText()) === 'Could not show this question: options[0].label: longer than 80 characters',
      'error note: ' + (await bad.locator('.ui-error').innerText()));
    check((await bad.locator('.ui-card').count()) === 0, 'no card for a rejected block');
    const unknown = await send('ui: unknown');
    await unknown.locator('.codeblock').waitFor();
    check((await unknown.locator('.codeblock .lang').innerText()) === 'ui:widget', 'unknown type is plain code');
    check((await unknown.locator('.ui-error, .ui-card, .ui-skeleton').count()) === 0, 'and no error for it');
    const two = await send('ui: two');
    await two.locator('.ui-error').waitFor();
    check((await two.locator('.ui-card').count()) === 1 && (await two.locator('.codeblock').count()) === 1, 'one card, one code block');
    check((await two.locator('.ui-error').innerText()) === 'Could not show this question: only one choices or form block per message', 'second question rejected');
    // A form field that asks for a key is never an input, only the warning.
    const secret = await send('ui: secret');
    await secret.locator('.ui-card .ui-secret').waitFor();
    check((await secret.locator('.ui-secret').innerText()).startsWith('API key: This asks for a secret.'), 'secret note');
    check((await secret.locator('input, textarea').count()) === 0, 'no input for the key');
    await p.screenshot({ path: `${S}/rich-5-ui.png`, fullPage: true });
    // The stored blocks render the same after a reload.
    await p.reload();
    await p.locator('.msg.assistant .ui-card').first().waitFor();
    check((await p.locator('.msg.assistant .ui-card').count()) === 3 && (await p.locator('.msg.assistant .ui-error').count()) === 2,
      'after reload: 3 cards, 2 errors');
    log('ui: rejected block = code + reason, unknown type = code, one question per message, secret field refused, same after reload');

    // 7. Citations: the labels of a web result become pills naming the
    // host, an unknown label stays text, and a footer lists what was cited.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'cite: rust');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.keyboard.press('Enter');
    const row = p.locator('.tool-web .web-status');
    await p.waitForSelector('.tool-web .web-status:has-text("Searched “rust” · 2 results")');
    await idle();
    const cited = p.locator('.msg.assistant').last();
    await cited.locator('.cite').first().waitFor();
    const pills = await cited.locator('.cite').allInnerTexts();
    check(JSON.stringify(pills) === JSON.stringify(['a.example', 'b.example', 'a.example +1']), 'pills: ' + JSON.stringify(pills));
    check((await cited.locator('.cite').first().getAttribute('href')) === 'https://a.example/rust', 'a pill links to its source');
    check((await cited.locator('.cite-missing').innerText()) === '[S9]', 'an unknown label stays text');
    check((await cited.locator('.cite-missing').getAttribute('title')) === 'No such source', 'with a note');
    check((await cited.locator('.sources summary').innerText()) === 'Sources (2)', 'footer counts the cited sources');
    check((await cited.locator('.sources li').count()) === 2, 'footer lists two sources');
    check((await cited.locator('.md').innerText()).includes('Unknown [S9].'), 'text around the labels stays');
    log('cite: pills', pills.join(', '), '· [S9] plain · footer of 2 · row:', await row.innerText());

    // Hover or focus shows the title and the full URL.
    await cited.locator('.cite').first().hover();
    const card = cited.locator('.cite-card');
    await card.waitFor();
    check((await card.locator('.cite-card-title').innerText()) === 'Rust language', 'card title');
    check((await card.locator('.cite-card-url').innerText()) === 'https://a.example/rust', 'card URL');
    await p.screenshot({ path: `${S}/rich-4-cite.png` });
    await p.mouse.move(0, 0);
    await card.waitFor({ state: 'detached' });
    await cited.locator('.cite').nth(2).focus();
    check((await cited.locator('.cite-card .cite-card-item').count()) === 2, 'a group card lists both sources');
    await p.keyboard.press('Escape');
    check((await cited.locator('.cite-card').count()) === 0, 'Escape closes the card');

    // A source needs no confirm: the click opens it (the page is stubbed).
    await ctx.route(/^https:\/\/a\.example\//, (r) => r.fulfill({ status: 200, contentType: 'text/plain', body: 'source page' }));
    const [popup] = await Promise.all([ctx.waitForEvent('page'), cited.locator('.cite').first().click()]);
    check(popup.url() === 'https://a.example/rust', 'opened ' + popup.url());
    check((await cited.locator('.link-confirm').count()) === 0, 'no confirm for a source');
    await popup.close();

    // The finished row opens to the query, host · title per result, and
    // (one more click) the text the model read.
    await p.locator('.tool-web .tool-head').click();
    check((await p.locator('.tool-web .web-input-value').innerText()) === 'rust', 'the query in full');
    const results = await p.locator('.tool-web .web-results li').allInnerTexts();
    check(JSON.stringify(results) === JSON.stringify(['a.example · Rust language', 'b.example · The Rust book']), 'results: ' + JSON.stringify(results));
    await p.locator('.tool-web details.web-output summary').click();
    check((await p.locator('.tool-web details.web-output pre').innerText()).startsWith('[S1] Rust language'), 'the output the model read');

    // Labels are stable for the session: the same pages cite the same way
    // in a later turn, and a reload resolves them again.
    await p.fill('.composer textarea', 'cite: rust again');
    await p.keyboard.press('Enter');
    await p.waitForFunction(() => document.querySelectorAll('.msg.assistant').length >= 2);
    await idle();
    const again = await p.locator('.msg.assistant').last().locator('.cite').allInnerTexts();
    check(JSON.stringify(again) === JSON.stringify(pills), 'same labels in a later turn: ' + JSON.stringify(again));
    await p.reload();
    await p.locator('.msg.assistant .cite').first().waitFor();
    check((await p.locator('.msg.assistant').first().locator('.cite').allInnerTexts()).join() === pills.join(), 'pills after reload');
    log('cite: source opens without a confirm, results listed, labels stable across turns and reload');

    // While a search runs, its row shimmers, unless motion is reduced.
    await p.fill('.composer textarea', 'cite: slow rust');
    await p.keyboard.press('Enter');
    const running = p.locator('.tool-web .web-status.shimmer:has-text("Searching the web for “slow rust”")');
    await running.waitFor();
    check((await running.evaluate((el) => getComputedStyle(el).animationName)) === 'shimmer', 'shimmer runs');
    await p.emulateMedia({ reducedMotion: 'reduce' });
    check((await running.evaluate((el) => getComputedStyle(el).animationName)) === 'none', 'no shimmer under reduced motion');
    await p.emulateMedia({ reducedMotion: 'no-preference' });
    await p.waitForSelector('.tool-web .web-status:has-text("Searched “slow rust” · 2 results")');
    await idle();
    log('web row: shimmer while running, none under reduced motion');

    // 8. A sub-task's citations: the sub-agent numbers on from this
    // session (its new page is S3, a known one keeps S2), and the labels it
    // hands back resolve to its pages here, not to this session's S1.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'cite: rust');
    await p.click('.mode-switch button:has-text("Ask")');
    await p.selectOption('.composer select[aria-label="agent"]', 'ultracode');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.tool-web .web-status:has-text("Searched “rust” · 2 results")');
    await idle();
    await p.fill('.composer textarea', 'delegate: scout=cite: other rust');
    await p.keyboard.press('Enter');
    await p.waitForSelector('.msg.assistant:has-text("Sub-agent said:") .cite');
    await idle();
    const relayed = p.locator('.msg.assistant:has-text("Sub-agent said:")');
    const subPills = await relayed.locator('.cite').allInnerTexts();
    check(JSON.stringify(subPills) === JSON.stringify(['c.example', 'b.example', 'c.example +1']), 'sub-task pills: ' + JSON.stringify(subPills));
    check((await relayed.locator('.cite').first().getAttribute('href')) === 'https://c.example/other', 'the sub-task page');
    check((await relayed.locator('.md').innerText()).includes('Unknown [S9].'), 'its unknown label stays text');
    log('delegate: the sub-task cites on from the session, and its labels resolve here:', subPills.join(', '));

    check(errors.length === 0, 'page errors: ' + errors.join('; '));
    log('screenshots in', S);
    await b.close();
  } finally {
    process.kill(-srv.pid, 'SIGKILL');
    searx.close();
  }
})().catch((e) => { console.error('FAILED:', e.message.split('\n')[0]); process.exit(1); });
