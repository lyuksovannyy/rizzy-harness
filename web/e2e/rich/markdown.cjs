// rich_plan 12.4 items 2 (lazy chunks) and 1 (rich: demo), plus what the
// Markdown reviews added: the math clean pass, hidden characters, the bidi
// host label, the confirm in a scrolling table (and where focus goes when
// it closes), table sort (its names and focus ring), Copy, and a hostile
// code block that must not freeze the page.
const { check, log, eventually, focused, visualText, HIDDEN } = require('./lib.cjs');

module.exports = async function markdown(t) {
  const { p, ctx, S, requests, chunks } = t;

  // 2. A plain reply loads none of the lazy chunks. Each name is proven
  // real below, where the demo (and later a card) loads exactly one.
  await t.newChat('Ask');
  await t.send('hello there');
  await p.waitForSelector('.msg.assistant >> text=You said: hello there');
  for (const name of ['math', 'highlight\\.worker', 'UiBlock']) check(chunks(name).length === 0, `${name} chunk loaded for an echo`);
  log('item 2: an echo loads no math, highlight or ui chunk');

  // 1. While the demo streams, code and math stay source. Look once the
  // stream is past the rust fence and both formulas, so there is something
  // that could have been highlighted or rendered.
  await t.type('rich: demo');
  const streaming = await (await p.waitForFunction(() => {
    const md = document.querySelector('.msg.assistant.streaming .md');
    if (!md || !md.innerText.includes('Prices stay text')) return false;
    const n = (sel) => md.querySelectorAll(sel).length;
    return { rust: n('code.language-rust'), source: n('.math-src'), tokens: n('.token'), math: n('math') };
  })).jsonValue();
  check(JSON.stringify(streaming) === JSON.stringify({ rust: 1, source: 2, tokens: 0, math: 0 }), 'while streaming: ' + JSON.stringify(streaming));
  check(chunks('highlight\\.worker').length === 0 && chunks('math').length === 0, 'highlight/math must wait for the final message');
  await t.idle();
  const demo = await t.lastMsg();
  await demo.locator('.md .token.keyword').first().waitFor();
  await demo.locator('.md math').first().waitFor();
  // Each formula renders on its own once the chunk is in, except \eqref:
  // Temml leaves it for a post-processor we do not run, so it is source.
  await eventually(async () => ({
    math: await demo.locator('.md math').count(),
    source: await demo.locator('.md .math-src:visible').allInnerTexts(),
  }), { math: 4, source: ['\\eqref{user-content-fn-1}'] }, 'formulas rendered, the reference as source');
  check(chunks('highlight\\.worker').length === 1 && chunks('rust').length === 1, 'highlighter worker and the rust grammar loaded once');
  check(chunks('math').length === 1, 'math chunk loaded once');
  log('item 2: the demo loads', ['highlight\\.worker', 'rust', 'math'].flatMap((n) => chunks(n).map((u) => u.split('/').pop())).join(', '));

  // Temml's output is cleaned: \label's id (which would take over the
  // footnote's) and \char"202E are gone.
  check((await demo.locator('.md .math-out [id]').count()) === 0, 'no ids inside math');
  // Footnote ids carry a prefix of their own per message since 5d51e06, so
  // the demo's \label{user-content-fn-1} no longer names this footnote. The
  // check follows the reference to its target: exactly one element on the
  // page has that id, and it is this message's footnote.
  const ref = await demo.locator('.md sup a[id$="-fnref-1"]').getAttribute('href');
  const fnId = decodeURIComponent((ref || '').replace(/^#/, ''));
  check(/^m\d+-fn-1$/.test(fnId), `the footnote reference has a per-message id (${ref})`);
  check((await p.locator(`[id="${fnId}"]`).count()) === 1, 'the footnote keeps its id');
  check((await demo.locator(`.md section[data-footnotes] li[id="${fnId}"]`).count()) === 1, 'the id is on the footnote itself');
  check(!HIDDEN.test((await demo.locator('.md .math-out').allInnerTexts()).join('')), 'no hidden characters from \\char');
  check((await demo.locator('.callout-warning .callout-title').innerText()) === 'Check before you run', 'warning callout with its title');
  check((await demo.locator('.md img').count()) === 0, 'no <img> in messages');
  check((await demo.locator('.img-chip').innerText()).includes('img.example'), 'image chip names its host');
  const text = await demo.locator('.md').innerText();
  check(text.includes('Prices stay text: $5 and $10.'), 'single dollars stay text');
  check(!HIDDEN.test(text) && text.includes('abcdef'), 'hidden characters stripped, also entity-encoded ones');
  // [&#x202E;moc.knab](https://evil.example/): the host and the text after
  // it read as written.
  const reversed = demo.locator('p:has-text("Reversed:")');
  check((await visualText(reversed.locator('.link-host'))) === 'evil.example', 'host label reads evil.example on screen');
  check((await reversed.innerText()).includes('moc.knab') && (await reversed.innerText()).includes('and more.'), 'text after the link is not reordered');
  check((await demo.locator('.codeblock .lang').innerText()) === 'rust', 'code header names the language');

  // A link with a query the model made up asks first, with the full URL.
  const link = demo.locator('.link-wrap:has(> a[href="https://evil.example/a?q=1"])');
  check((await link.locator('.link-host').innerText()) === 'evil.example', 'real host shown next to the link');
  await link.locator('a').click();
  const confirm = demo.locator('.link-confirm');
  await confirm.waitFor();
  check((await confirm.locator('.link-confirm-url').innerText()) === 'https://evil.example/a?q=1', 'confirm shows the full URL');
  await p.screenshot({ path: `${S}/rich-1-confirm.png` });
  await confirm.locator('button:has-text("Cancel")').click();
  check((await demo.locator('.link-confirm').count()) === 0, 'Cancel closes the confirm');
  check((await focused(link.locator('a'))) === true, 'Cancel gives focus back to the link');
  // From the keyboard: Enter opens it with focus on Open, Escape closes it
  // and focus is on the link again (not the top of the page).
  await link.locator('a').focus();
  await p.keyboard.press('Enter');
  await confirm.waitFor();
  check((await focused(confirm.locator('button.primary'))) === true, 'focus on Open');
  await p.keyboard.press('Escape');
  check((await demo.locator('.link-confirm').count()) === 0 && (await focused(link.locator('a'))) === true, 'Escape gives focus back to the link');
  // The image chip follows the same rule.
  await demo.locator('a.img-chip').click();
  await demo.locator('.link-confirm').waitFor();
  await p.keyboard.press('Escape');
  check((await focused(demo.locator('a.img-chip'))) === true, 'and focus goes back to the chip');
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
  await p.keyboard.press('Escape');

  // Tables sort numerically by a header click.
  const release = demo.locator('th:has-text("release (s)") button');
  await release.click();
  await release.click();
  check((await demo.locator('th:has-text("release (s)")').getAttribute('aria-sort')) === 'descending', 'aria-sort');
  check((await demo.locator('tbody tr').first().innerText()).startsWith('master'), 'sorted by release time, descending');
  // The arrow is drawn, not read: aria-sort says it, so the name is the
  // column's alone.
  const head = await demo.locator('th:has-text("release (s)")').ariaSnapshot();
  check(head.includes('button "release (s)"') && !/[▲▼]/.test(head), 'header name without the arrow: ' + head);
  // The keyboard focus ring sits inside the cell: outside it, the table's
  // scroller and the next sticky cell would hide most of it.
  await p.keyboard.press('Shift+Tab');
  const ring = await p.evaluate(() => {
    const el = document.activeElement;
    return { text: el.textContent, visible: el.matches(':focus-visible'), offset: getComputedStyle(el).outlineOffset };
  });
  check(ring.text === 'debug (s)' && ring.visible && ring.offset === '-2px', 'focus ring inside the header cell: ' + JSON.stringify(ring));

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

  // Nothing above may reach a host the model chose: not the image, not the
  // links (cancelled or escaped).
  const leaked = requests.filter((u) => /img\.example|evil\.example/.test(u));
  check(leaked.length === 0, 'requests to model-chosen hosts: ' + leaked.join(', '));
  log('item 1: highlighted, math, callout, image chip, prices as text, link confirm; no requests to img.example or evil.example');

  // A hostile code block must not freeze the page. Unclosed quotes make
  // Prism's tsx grammar super-linear (this block takes over 80 s in Node),
  // so it runs in the worker until the 1,500 ms budget stops it, stays
  // plain, and the next block highlights in a fresh worker. It is under
  // the size caps, so it really is sent to the worker.
  const line = '\'"`x'.repeat(25);
  const hostile = Array.from({ length: 180 }, () => line).join('\n');
  await t.send('x\n\n```tsx\n' + hostile + '\n```\n\n```rust\nfn after() {}\n```\n');
  const last = await t.lastMsg();
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
  const waited = Date.now() - t0;
  check(maxGap < 500, `page stalled ${maxGap} ms`);
  check((await last.locator('code.language-rust .token.keyword').count()) > 0, 'the block after the hostile one is highlighted');
  check((await last.locator('code.language-tsx').count()) === 1 && (await last.locator('code.language-tsx .token').count()) === 0, 'the hostile block stays plain');
  check(waited > 1000, `the next block waited ${waited} ms, so the hostile one ran until its budget`);
  log(`hostile code block: page answered within ${maxGap} ms; next block highlighted after ${waited} ms`);

  // After a reload the same messages render the same way.
  await p.reload();
  await p.locator('.msg.assistant .md .token.keyword').first().waitFor();
  await p.locator('.msg.assistant .md math').first().waitFor();
  check((await p.locator('.msg.assistant .img-chip').count()) === 1, 'the image chip after reload');
  log('reload renders the same');
};
