// Rich messages end-to-end: the Markdown baseline (links, image chips,
// callouts, tables, code, math, hidden characters, Copy), lazy loading of
// the highlighter worker and math chunks, a hostile code block that must
// not freeze the page, and citations of web results (pills, footer, the
// web status row), also those a delegated sub-task hands back, and ui
// blocks (skeleton while streaming, the card, answering a question and a
// form, a stale question, a refused secret field, rejected and unknown blocks,
// a progress block moved on over an agentic turn on the local node, and a
// block cut off when that turn is stopped mid-message).
// Usage: node web/e2e/rich.cjs <path-to-harness-binary> [work-dir]
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const http = require('http');
const fs = require('fs');
const path = require('path');
const os = require('os');
const BIN = path.resolve(process.argv[2] || 'target/debug/harness');
const S = process.argv[3] || fs.mkdtempSync(path.join(os.tmpdir(), 'harness-e2e-rich-'));
const R = `${S}/repo`;
fs.mkdirSync(`${S}/data`, { recursive: true });
fs.mkdirSync(R, { recursive: true });
const URL_ = 'http://127.0.0.1:7797/';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('•', ...a);
const check = (cond, msg) => { if (!cond) throw new Error(msg); };

/** WCAG contrast of an element's text colour on another's background. */
async function contrast(fg, bg) {
  const rgb = (s) => s.match(/[\d.]+/g).slice(0, 3).map(Number);
  const lum = ([r, g, b]) => [r, g, b]
    .map((c) => (c /= 255) <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
    .reduce((a, c, i) => a + c * [0.2126, 0.7152, 0.0722][i], 0);
  const a = lum(rgb(await fg.evaluate((el) => getComputedStyle(el).color)));
  const b = lum(rgb(await bg.evaluate((el) => getComputedStyle(el).backgroundColor)));
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}
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
  fs.writeFileSync(`${S}/data/node.toml`, `workspace_roots = ["${R}"]\n`);
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
    // The reply goes on after the fence, so once that text shows (and the
    // message still streams) the whole JSON body has arrived: one snapshot.
    const streamed = await (await p.waitForFunction(() => {
      const md = document.querySelector('.msg.assistant.streaming .md');
      return md && md.innerText.includes('Tap the one') ? md.innerText : false;
    })).jsonValue();
    check(streamed.includes('Preparing question…'), 'skeleton while streaming: ' + streamed);
    check(!streamed.includes('"options"') && !streamed.includes('colour"'), 'no raw JSON while streaming: ' + streamed);
    check(chunks('UiBlock').length === 0, 'the card chunk waits for the final message');
    await idle();
    // Messages are counted from the top: `.last()` would move on as the
    // answers arrive.
    const nthMsg = async () => p.locator('.msg.assistant').nth((await p.locator('.msg.assistant').count()) - 1);
    const choices = await nthMsg();
    await choices.locator('.ui-card .ui-choices').waitFor();
    check((await choices.locator('.ui-skeleton').count()) === 0, 'the skeleton gives way to the card');
    check((await choices.locator('.ui-from').innerText()) === 'Question from General', 'harness frame names the agent');
    check((await choices.locator('.ui-footer').innerText()) === 'Sends your answers as a message. Never type passwords or keys here.', 'fixed footer');
    check((await choices.locator('.ui-title').innerText()) === 'Which colour do you like best?', 'the question');
    const labels = await choices.locator('.ui-option-label').allInnerTexts();
    check(JSON.stringify(labels) === '["Blue","Green","Red"]', 'options: ' + JSON.stringify(labels));
    check((await choices.locator('.codeblock').count()) === 0, 'no code for a shown block');
    check(chunks('UiBlock').length === 1, 'card chunk loaded once');
    log('ui: skeleton while streaming, then the card', labels.join('/'));

    // 3. A tap answers: the user's bubble names the option, the model gets
    // the canonical text of ids, and the card is answered and locked.
    const echoes = () => p.locator('.msg.assistant').count();
    let before = await echoes();
    await choices.locator('.ui-option:has-text("Blue")').click();
    await p.waitForFunction((n) => document.querySelectorAll('.msg.assistant').length > n, before);
    await idle();
    const bubble = p.locator('.msg.user .bubble.reply').last();
    check((await bubble.innerText()).includes('Answered “Which colour do you like best?”') && (await bubble.innerText()).includes('Blue'),
      'reply bubble: ' + (await bubble.innerText()));
    check((await (await nthMsg()).innerText()).includes('You said: Choice answer · colour: blue'), 'the model got the canonical answer');
    const answeredState = async (card) => ({
      chosen: await card.locator('.ui-option-chosen .ui-option-label').allInnerTexts(),
      enabled: await card.locator('.ui-option:not([disabled])').count(),
      state: await card.locator('.ui-state').innerText(),
    });
    const wantAnswered = { chosen: ['Blue'], enabled: 0, state: 'Answered' };
    check(JSON.stringify(await answeredState(choices)) === JSON.stringify(wantAnswered), 'answered: ' + JSON.stringify(await answeredState(choices)));
    await choices.screenshot({ path: `${S}/rich-5a-answered.png` });
    log('choices: a tap answers with ids; the card is answered and locked');

    // 9. Stale: typing past a question closes it, and the node refuses a
    // late answer for it with 409.
    before = await echoes();
    await p.fill('.composer textarea', 'ui: choices');
    await p.keyboard.press('Enter');
    await p.waitForFunction((n) => document.querySelectorAll('.msg.assistant').length > n, before);
    await idle();
    const stale = await nthMsg();
    await stale.locator('.ui-option:not([disabled])').first().waitFor();
    before = await echoes();
    await p.fill('.composer textarea', 'green, please');
    await p.keyboard.press('Enter');
    await p.waitForFunction((n) => document.querySelectorAll('.msg.assistant').length > n, before);
    await idle();
    check((await stale.locator('.ui-state').innerText()).startsWith('Not answered'), 'stale card: ' + (await stale.locator('.ui-state').innerText()));
    check((await stale.locator('.ui-option:not([disabled])').count()) === 0, 'stale card is disabled');
    const late = await p.evaluate(async () => {
      const sid = location.hash.split('/').pop();
      const ev = await (await fetch(`/api/sessions/${sid}/events`)).json();
      const asked = ev.events.filter((e) => e.kind === 'assistant_message' && e.payload.blocks).pop();
      const r = await fetch(`/api/sessions/${sid}/prompt`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ text: '', reply: { message_id: asked.payload.message_id, block_id: 'colour', values: { selected: ['red'] } } }),
      });
      return { status: r.status, error: (await r.json()).error };
    });
    check(late.status === 409 && late.error === 'this form is no longer open', 'late answer: ' + JSON.stringify(late));
    log('choices: prose closes the question ("Not answered"); a late answer gets', late.status);

    // 6. Errors: a block Rust rejected is its code plus the reason, an
    // unknown type is plain code, and a second question in one message is
    // rejected while the first shows.
    const send = async (text) => {
      const before = await p.locator('.msg.assistant').count();
      await p.fill('.composer textarea', text);
      await p.keyboard.press('Enter');
      await p.waitForFunction((n) => document.querySelectorAll('.msg.assistant').length > n, before);
      await idle();
      return nthMsg();
    };
    /** Click Send on a card; returns the echo that answers it. */
    const submit = async (card) => {
      const before = await p.locator('.msg.assistant').count();
      await card.locator('.ui-send').click();
      await p.waitForFunction((n) => document.querySelectorAll('.msg.assistant').length > n, before);
      await idle();
      return nthMsg();
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
    // A form field that asks for a key shows only the warning; the other
    // field of the form is an ordinary field.
    const secret = await send('ui: secret');
    await secret.locator('.ui-card .ui-secret').waitFor();
    const fields = secret.locator('.ui-fields > li');
    check((await fields.count()) === 2, 'two fields');
    check((await fields.nth(0).locator('.ui-secret').innerText()).startsWith('API key: This asks for a secret.'), 'secret note');
    check((await fields.nth(1).locator('.ui-secret').count()) === 0, 'Name is not refused');
    check((await fields.nth(1).locator('.ui-field-label').innerText()) === 'Name', 'Name is a field');
    check((await secret.locator('.ui-secret').count()) === 1, 'only the key is refused');
    // 5. Nothing can be typed for the key; Name still works.
    check((await secret.getByLabel('API key').count()) === 0, 'no input for the key');
    await secret.screenshot({ path: `${S}/rich-5c-secret.png` });
    await secret.getByLabel('Name', { exact: true }).fill('Ana');
    const named = await submit(secret);
    const namedText = await named.innerText();
    check(namedText.includes('name: "Ana"') && namedText.includes('not shown (asks for a secret): key'), 'secret form answer: ' + namedText);
    log('secret: no input for the key, Name answered:', JSON.stringify(namedText.split('\n').slice(1)));

    // 4. A form: Send waits for the required field, a value out of range
    // is an inline error, and the answer reaches the model as ids/values.
    const trip = await send('ui: form');
    await trip.locator('.ui-card .ui-form').waitFor();
    check((await trip.locator('.ui-from').innerText()) === 'Form from General', 'form header');
    check(await trip.locator('.ui-send').isDisabled(), 'Send off until Travellers is filled');
    const people = trip.getByLabel('Travellers', { exact: true });
    await people.fill('13');
    await trip.locator('.ui-field-error').waitFor();
    check((await trip.locator('.ui-field-error').innerText()) === 'Must be at most 12', 'inline error: ' + (await trip.locator('.ui-field-error').innerText()));
    check(await trip.locator('.ui-send').isDisabled(), 'Send off while a value is wrong');
    await people.fill('3');
    check((await trip.locator('.ui-field-error').count()) === 0, 'error gone');
    await trip.getByLabel('Lisbon').check();
    await p.screenshot({ path: `${S}/rich-5b-form.png`, fullPage: true });
    const tripEcho = (await (await submit(trip)).innerText());
    check(tripEcho.includes('Form answer · trip') && tripEcho.includes('people: 3') && tripEcho.includes('dest: lis'), 'form answer: ' + tripEcho);
    const answers = await trip.locator('.ui-answers dd').allInnerTexts();
    check(JSON.stringify(answers) === '["3","Lisbon"]' && (await trip.locator('.ui-card').locator('input, textarea, select, button').count()) === 0,
      'answered form shows its values read-only: ' + JSON.stringify(answers));
    const tripBubble = await p.locator('.msg.user .bubble.reply').last().innerText();
    check(tripBubble.includes('Answered “Trip details”') && tripBubble.includes('Travellers: 3') && tripBubble.includes('Destination: Lisbon'), 'form bubble: ' + tripBubble);
    log('form: Send waits for Travellers, 13 is an inline error, the answer lists people: 3');
    await p.screenshot({ path: `${S}/rich-5-ui.png`, fullPage: true });

    // The stored blocks render the same after a reload; answers stay
    // answered and locked.
    await p.reload();
    await p.locator('.msg.assistant .ui-card').first().waitFor();
    // The log can arrive in more than one batch after a reload.
    await p.waitForFunction(() => document.querySelectorAll('.msg.assistant .ui-card').length >= 5, null, { timeout: 5000 }).catch(() => {});
    check((await p.locator('.msg.assistant .ui-card').count()) === 5 && (await p.locator('.msg.assistant .ui-error').count()) === 2,
      `after reload: 5 cards, 2 errors (got ${await p.locator('.msg.assistant .ui-card').count()}, ${await p.locator('.msg.assistant .ui-error').count()})`);
    const first = p.locator('.msg.assistant .ui-card-choices').first();
    check(JSON.stringify(await answeredState(first)) === JSON.stringify(wantAnswered), 'answered after reload: ' + JSON.stringify(await answeredState(first)));
    check((await p.locator('.msg.assistant .ui-card-choices').nth(1).locator('.ui-state').innerText()).startsWith('Not answered'), 'stale after reload');
    check((await p.locator('.msg.user .bubble.reply').count()) === 3, 'three reply bubbles');
    log('ui: rejected block = code + reason, unknown type = code, one question per message, secret field refused, answers locked after reload');

    // 8. Progress in an agentic turn on the local node: the mock moves one
    // block id on over three messages, each but the last calling bash. The
    // turn's latest block is the card; the earlier two are snapshot lines.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'progress: 3');
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.agentic-row input', R);
    await p.click('.composer .send');
    // While the first call waits for approval, the first block is the live
    // card: its active step spins, unless motion is reduced.
    const allow = p.locator('.approval button:has-text("Allow bash for this session")');
    await allow.waitFor();
    const spinner = p.locator('.ui-card .ui-progress .ui-step-active .spin');
    await spinner.waitFor();
    check((await p.locator('.ui-progress-status').innerText()) === 'In progress', 'live card is in progress');
    check((await spinner.evaluate((el) => getComputedStyle(el).animationName)) === 'spin', 'the active step spins');
    await p.emulateMedia({ reducedMotion: 'reduce' });
    check((await spinner.evaluate((el) => getComputedStyle(el).animationName)) === 'none', 'no spin under reduced motion');
    await p.emulateMedia({ reducedMotion: 'no-preference' });
    await allow.click();
    await idle();
    const progressState = async () => {
      const card = p.locator('.ui-progress');
      return {
        cards: await card.count(),
        framed: await p.locator('.ui-card .ui-progress').count(),
        snapshots: await p.locator('.ui-progress-snapshot').evaluateAll((els) => els.map((el) => el.textContent)),
        snapshotFrom: await p.locator('.ui-card-snapshot:has(.ui-progress-snapshot) > .ui-from').allInnerTexts(),
        done: await card.locator('.ui-step-done').count(),
        steps: await card.locator('.ui-step').count(),
        status: await card.locator('.ui-progress-status').innerText(),
        bar: await card.locator('.ui-bar').getAttribute('aria-valuenow'),
        spinning: await p.locator('.ui-progress .spin, .ui-progress-snapshot .spin').count(),
      };
    };
    await p.locator('.msg.assistant:has-text("All 3 steps done.") .ui-progress').waitFor();
    const final = await progressState();
    const want = {
      cards: 1,
      framed: 1,
      snapshots: ['Working through 3 steps · 0/3 · Step 1', 'Working through 3 steps · 1/3 · Step 2'],
      snapshotFrom: ['from General', 'from General'],
      done: 3,
      steps: 3,
      status: 'Done',
      bar: '100',
      spinning: 0,
    };
    check(JSON.stringify(final) === JSON.stringify(want), 'progress: ' + JSON.stringify(final));
    check((await p.locator('.msg.assistant').count()) === 3, 'three messages, one per step');
    await p.screenshot({ path: `${S}/rich-6-progress.png`, fullPage: true });
    await p.reload();
    await p.locator('.msg.assistant:has-text("All 3 steps done.") .ui-progress').waitFor();
    const reloaded = await progressState();
    check(JSON.stringify(reloaded) === JSON.stringify(want), 'progress after reload: ' + JSON.stringify(reloaded));
    log('progress: one card with 3 done steps and 2 snapshot lines, also after reload; the live step spins, except under reduced motion');

    // A turn stopped while a step runs: nothing will move the card on, so
    // its active step has stopped, nothing spins, and the header says why.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'progress: 2');
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.agentic-row input', R);
    await p.click('.composer .send');
    await p.locator('.approval button:has-text("Allow bash for this session")').waitFor();
    await p.locator('.ui-card .ui-progress .ui-step-active .spin').waitFor();
    await p.click('.send.stop');
    await idle();
    const stopped = p.locator('.ui-progress');
    await stopped.locator('.ui-step-stopped').waitFor();
    const stoppedState = {
      status: await stopped.locator('.ui-progress-status').innerText(),
      steps: await stopped.locator('.ui-step').evaluateAll((els) => els.map((el) => el.className)),
      spinning: await stopped.locator('.spin').count(),
    };
    check(JSON.stringify(stoppedState) === JSON.stringify({
      status: 'Stopped · turn cancelled',
      steps: ['ui-step ui-step-stopped', 'ui-step ui-step-pending'],
      spinning: 0,
    }), 'stopped: ' + JSON.stringify(stoppedState));
    // The status colours are readable on the card in both themes: 4.5:1 for
    // the status words, 3:1 for the step icon.
    for (const colorScheme of ['light', 'dark']) {
      await p.emulateMedia({ colorScheme });
      const card = p.locator('.ui-card-progress');
      const words = await contrast(stopped.locator('.ui-progress-status'), card);
      const icon = await contrast(stopped.locator('.ui-step-stopped .ui-icon'), card);
      check(words >= 4.5 && icon >= 3, `${colorScheme}: stopped status ${words.toFixed(2)}:1, icon ${icon.toFixed(2)}:1`);
    }
    await p.emulateMedia({ colorScheme: 'light' });
    log('progress: a cancelled turn leaves the card stopped, with the reason, readable in both themes');

    // A turn stopped while the next progress message streams: that message
    // never reached Rust, so its fence has no block. It is a harness note,
    // not half a JSON body as code, live and after a reload.
    await p.click('.new-chat');
    await p.waitForSelector('h1.greeting');
    await p.fill('.composer textarea', 'progress: 3');
    await p.click('.mode-switch button:has-text("Agentic")');
    await p.fill('.agentic-row input', R);
    await p.click('.composer .send');
    await p.locator('.approval button:has-text("Allow bash for this session")').click();
    await p.locator('.msg.assistant.streaming .ui-skeleton').waitFor();
    await p.click('.send.stop');
    await idle();
    const cutState = async () => {
      const last = p.locator('.msg.assistant').last();
      const text = await last.innerText();
      return {
        cut: await last.locator('.ui-cut').allInnerTexts(),
        code: await last.locator('pre, .codeblock').count(),
        json: text.includes('ui:progress') || text.includes('"steps"'),
        card: await p.locator('.ui-card-progress .ui-progress-status').innerText(),
      };
    };
    const wantCut = { cut: ['Cut off before it was finished'], code: 0, json: false, card: 'Stopped · turn cancelled' };
    await p.locator('.msg.assistant .ui-cut').waitFor();
    const cutLive = await cutState();
    check(JSON.stringify(cutLive) === JSON.stringify(wantCut), 'cut off: ' + JSON.stringify(cutLive));
    await p.screenshot({ path: `${S}/rich-6b-cut.png`, fullPage: true });
    await p.reload();
    await p.locator('.msg.assistant .ui-cut').waitFor();
    const cutReloaded = await cutState();
    check(JSON.stringify(cutReloaded) === JSON.stringify(wantCut), 'cut off after reload: ' + JSON.stringify(cutReloaded));
    log('progress: a message stopped mid-block shows "cut off", never its partial JSON, also after reload');

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
